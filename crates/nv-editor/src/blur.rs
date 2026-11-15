//! Gaussian blur as three box blurs (the classic O(1)-per-pixel approximation: the cost does not grow with the radius), on
//! interleaved 8-bit buffers, split over all cores. The neighbourhood filters (sharpen, clarity, bloom, noise reduction,
//! film blur, skin smoothing) are built on this.
//!
//! Edges repeat the border pixel. Only the first `blur_ch` channels of each pixel are blurred (3 for RGBA: alpha is left to
//! the caller; 1 for a luma plane).

/// Radii of three successive box blurs whose sum approximates a gaussian of `sigma` (Kovesi / Ivan Kutskir's method).
pub fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0)).round() as i32;
    let size = |i: i32| if i < m { wl } else { wu };
    [((size(0) - 1) / 2) as usize, ((size(1) - 1) / 2) as usize, ((size(2) - 1) / 2) as usize]
}

fn workers(units: usize, work: usize) -> usize {
    // Not worth a thread below ~256 KB of work.
    let by_size = work / (256 * 1024);
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(by_size.max(1)).min(units.max(1))
}

/// One horizontal box blur of radius `r` over rows `src` -> `dst` (both `w * ch` bytes per row).
fn box_h(src: &[u8], dst: &mut [u8], w: usize, ch: usize, blur_ch: usize, r: usize) {
    let n = (2 * r + 1) as u64;
    let recip = (1u64 << 32) / n;
    for (srow, drow) in src.chunks_exact(w * ch).zip(dst.chunks_exact_mut(w * ch)) {
        if ch > blur_ch {
            drow.copy_from_slice(srow); // keeps the channels that are not blurred
        }
        for c in 0..blur_ch {
            let at = |x: isize| srow[(x.clamp(0, w as isize - 1) as usize) * ch + c] as u64;
            let mut acc: u64 = 0;
            for i in -(r as isize)..=(r as isize) {
                acc += at(i);
            }
            for x in 0..w {
                drow[x * ch + c] = ((acc * recip + (1 << 31)) >> 32) as u8;
                acc = acc + at(x as isize + r as isize + 1) - at(x as isize - r as isize);
            }
        }
    }
}

/// Vertical box blur of the column strip `[x0, x1)`; returns the strip's pixels (`h` rows of `(x1 - x0) * ch`).
fn box_v_strip(src: &[u8], w: usize, h: usize, ch: usize, blur_ch: usize, r: usize, x0: usize, x1: usize) -> Vec<u8> {
    let sw = x1 - x0;
    let n = (2 * r + 1) as u64;
    let recip = (1u64 << 32) / n;
    let mut out = vec![0u8; sw * ch * h];
    let mut sums = vec![0u64; sw * ch];
    let row = |y: isize| {
        let y = y.clamp(0, h as isize - 1) as usize;
        &src[(y * w + x0) * ch..(y * w + x1) * ch]
    };
    for i in -(r as isize)..=(r as isize) {
        for (s, v) in sums.iter_mut().zip(row(i)) {
            *s += *v as u64;
        }
    }
    for y in 0..h {
        let orow = &mut out[y * sw * ch..(y + 1) * sw * ch];
        if ch > blur_ch {
            orow.copy_from_slice(row(y as isize));
        }
        let (add, sub) = (row(y as isize + r as isize + 1), row(y as isize - r as isize));
        for x in 0..sw {
            for c in 0..blur_ch {
                let k = x * ch + c;
                orow[k] = ((sums[k] * recip + (1 << 31)) >> 32) as u8;
                sums[k] = sums[k] + add[k] as u64 - sub[k] as u64;
            }
        }
    }
    out
}

/// Weights of a true gaussian kernel (radius 3 sigma), for the small sigmas the box approximation cannot do.
fn kernel(sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.0).ceil().max(1.0) as i32;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    k
}

fn conv_h(src: &[u8], dst: &mut [u8], w: usize, ch: usize, blur_ch: usize, k: &[f32]) {
    let r = (k.len() / 2) as isize;
    for (srow, drow) in src.chunks_exact(w * ch).zip(dst.chunks_exact_mut(w * ch)) {
        if ch > blur_ch {
            drow.copy_from_slice(srow);
        }
        for x in 0..w {
            for c in 0..blur_ch {
                let mut acc = 0.0f32;
                for (i, wt) in k.iter().enumerate() {
                    let xx = (x as isize + i as isize - r).clamp(0, w as isize - 1) as usize;
                    acc += srow[xx * ch + c] as f32 * wt;
                }
                drow[x * ch + c] = (acc + 0.5) as u8;
            }
        }
    }
}

fn conv_v_strip(src: &[u8], w: usize, h: usize, ch: usize, blur_ch: usize, k: &[f32], x0: usize, x1: usize) -> Vec<u8> {
    let sw = x1 - x0;
    let r = (k.len() / 2) as isize;
    let mut out = vec![0u8; sw * ch * h];
    let mut acc = vec![0.0f32; sw * ch];
    for y in 0..h {
        acc.iter_mut().for_each(|a| *a = 0.0);
        for (i, wt) in k.iter().enumerate() {
            let yy = (y as isize + i as isize - r).clamp(0, h as isize - 1) as usize;
            let row = &src[(yy * w + x0) * ch..(yy * w + x1) * ch];
            for x in 0..sw {
                for c in 0..blur_ch {
                    acc[x * ch + c] += row[x * ch + c] as f32 * wt;
                }
            }
        }
        let orow = &mut out[y * sw * ch..(y + 1) * sw * ch];
        if ch > blur_ch {
            orow.copy_from_slice(&src[(y * w + x0) * ch..(y * w + x1) * ch]);
        }
        for x in 0..sw {
            for c in 0..blur_ch {
                orow[x * ch + c] = (acc[x * ch + c] + 0.5) as u8;
            }
        }
    }
    out
}

/// One horizontal and one vertical pass (`h_pass` over bands of rows, `v_pass` over strips of columns), each on all cores.
fn two_passes(
    cur: &mut [u8],
    tmp: &mut [u8],
    w: usize,
    h: usize,
    ch: usize,
    h_pass: impl Fn(&[u8], &mut [u8]) + Sync,
    v_pass: impl Fn(&[u8], usize, usize) -> Vec<u8> + Sync,
) {
    let n = workers(h, w * h * ch);
    let band = h.div_ceil(n);
    std::thread::scope(|s| {
        for (a, b) in cur.chunks(band * w * ch).zip(tmp.chunks_mut(band * w * ch)) {
            let h_pass = &h_pass;
            if n == 1 {
                h_pass(a, b);
            } else {
                s.spawn(move || h_pass(a, b));
            }
        }
    });
    let n = workers(w / 16, w * h * ch);
    let strip = w.div_ceil(n);
    let tmp_ref: &[u8] = tmp;
    let parts: Vec<(usize, usize, Vec<u8>)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..n)
            .filter_map(|i| {
                let (x0, x1) = (i * strip, ((i + 1) * strip).min(w));
                let v_pass = &v_pass;
                (x0 < x1).then(|| s.spawn(move || (x0, x1, v_pass(tmp_ref, x0, x1))))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for (x0, x1, data) in parts {
        let sw = x1 - x0;
        for y in 0..h {
            cur[(y * w + x0) * ch..(y * w + x1) * ch].copy_from_slice(&data[y * sw * ch..(y + 1) * sw * ch]);
        }
    }
}

/// A gaussian blur of `sigma` pixels. `sigma` below about 0.45 returns the input unchanged.
pub fn gaussian(src: &[u8], w: usize, h: usize, ch: usize, blur_ch: usize, sigma: f32) -> Vec<u8> {
    assert_eq!(src.len(), w * h * ch);
    if w == 0 || h == 0 || sigma < 0.45 {
        return src.to_vec();
    }
    let mut cur = src.to_vec();
    let mut tmp = vec![0u8; src.len()];
    if sigma < 1.6 {
        let k = kernel(sigma);
        two_passes(&mut cur, &mut tmp, w, h, ch, |a, b| conv_h(a, b, w, ch, blur_ch, &k), |t, x0, x1| conv_v_strip(t, w, h, ch, blur_ch, &k, x0, x1));
        return cur;
    }
    for r in box_radii(sigma) {
        if r == 0 {
            continue;
        }
        let r = r.min(w.max(h));
        two_passes(&mut cur, &mut tmp, w, h, ch, |a, b| box_h(a, b, w, ch, blur_ch, r), |t, x0, x1| box_v_strip(t, w, h, ch, blur_ch, r, x0, x1));
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_boxes_have_roughly_the_variance_of_the_gaussian() {
        for sigma in [2.0f32, 2.5, 8.0, 30.0] {
            let r = box_radii(sigma);
            // Variance of a box of width 2r+1 is ((2r+1)^2 - 1) / 12; the three add up.
            let var: f32 = r.iter().map(|&r| (((2 * r + 1) * (2 * r + 1)) as f32 - 1.0) / 12.0).sum();
            assert!((var.sqrt() - sigma).abs() / sigma < 0.12, "sigma {sigma}: {}", var.sqrt());
        }
    }

    #[test]
    fn a_flat_picture_stays_flat_and_an_impulse_spreads_evenly() {
        let flat = vec![100u8; 40 * 30 * 4];
        assert_eq!(gaussian(&flat, 40, 30, 4, 3, 3.0), flat);
        let mut p = vec![0u8; 41 * 41];
        p[20 * 41 + 20] = 255;
        let b = gaussian(&p, 41, 41, 1, 1, 2.0);
        let total: u32 = b.iter().map(|&v| v as u32).sum();
        assert!((200..=300).contains(&total), "energy kept: {total}");
        assert!(b[20 * 41 + 20] > b[20 * 41 + 24] && b[20 * 41 + 24] > 0);
        assert_eq!(b[20 * 41 + 17], b[20 * 41 + 23], "symmetric");
        assert!(b[17 * 41 + 20].abs_diff(b[20 * 41 + 23]) <= 1, "same in both directions");
    }

    #[test]
    fn alpha_and_other_channels_are_left_alone_and_threads_agree_with_one_thread() {
        let (w, h) = (700, 500);
        let src: Vec<u8> = (0..w * h * 4).map(|i| ((i * 31 + i / 7) % 251) as u8).collect();
        let out = gaussian(&src, w, h, 4, 3, 4.0);
        assert!((0..w * h).all(|i| out[i * 4 + 3] == src[i * 4 + 3]));
        // The same through a single band.
        let mut cur = src.clone();
        let mut tmp = vec![0u8; src.len()];
        for r in box_radii(4.0) {
            box_h(&cur, &mut tmp, w, 4, 3, r);
            let v = box_v_strip(&tmp, w, h, 4, 3, r, 0, w);
            cur.copy_from_slice(&v);
        }
        assert_eq!(out, cur);
    }
}
