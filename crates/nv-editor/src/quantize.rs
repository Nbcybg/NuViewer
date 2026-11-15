//! Colour reduction: a palette of N colours chosen by median cut, optionally with Floyd-Steinberg dithering. The result is still
//! an RGBA picture (alpha untouched); `indexed_png` writes it as a palette PNG, which is where the saving comes from.

use crate::raster::Pixels;

const BITS: usize = 5;
const BINS: usize = 1 << (3 * BITS);

#[inline]
fn bin_of(r: u8, g: u8, b: u8) -> usize {
    (((r >> 3) as usize) << (2 * BITS)) | (((g >> 3) as usize) << BITS) | (b >> 3) as usize
}

struct Cell {
    /// Bin numbers in this cell.
    bins: Vec<usize>,
    count: u64,
}

/// A palette of at most `n` colours for the opaque-ish pixels of `p`.
pub fn palette(p: &Pixels, n: usize) -> Vec<[u8; 3]> {
    let n = n.clamp(2, 256);
    let mut count = vec![0u64; BINS];
    let mut sum = vec![[0u64; 3]; BINS];
    for px in p.rgba.chunks_exact(4) {
        if px[3] == 0 {
            continue;
        }
        let b = bin_of(px[0], px[1], px[2]);
        count[b] += 1;
        for c in 0..3 {
            sum[b][c] += px[c] as u64;
        }
    }
    let used: Vec<usize> = (0..BINS).filter(|&b| count[b] > 0).collect();
    if used.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut cells = vec![Cell { count: used.iter().map(|&b| count[b]).sum(), bins: used }];
    let channel = |b: usize, c: usize| (b >> ((2 - c) * BITS)) & 31;
    while cells.len() < n {
        // The most populated cell that can still be split.
        let Some(ci) = (0..cells.len()).filter(|&i| cells[i].bins.len() > 1).max_by_key(|&i| cells[i].count) else { break };
        let cell = cells.swap_remove(ci);
        // Split along the channel with the widest spread.
        let spread = |c: usize| {
            let (lo, hi) = cell.bins.iter().fold((31, 0), |(lo, hi), &b| (lo.min(channel(b, c)), hi.max(channel(b, c))));
            hi - lo
        };
        let ch = (0..3).max_by_key(|&c| spread(c)).unwrap_or(0);
        let mut bins = cell.bins;
        bins.sort_by_key(|&b| channel(b, ch));
        // At the weighted median.
        let half = cell.count / 2;
        let (mut acc, mut cut) = (0u64, 1usize);
        for (i, &b) in bins.iter().enumerate() {
            acc += count[b];
            if acc >= half {
                cut = (i + 1).clamp(1, bins.len() - 1);
                break;
            }
        }
        let right = bins.split_off(cut);
        for part in [bins, right] {
            let c = part.iter().map(|&b| count[b]).sum();
            cells.push(Cell { bins: part, count: c });
        }
    }
    cells
        .iter()
        .map(|cell| {
            let (mut s, mut n) = ([0u64; 3], 0u64);
            for &b in &cell.bins {
                n += count[b];
                for c in 0..3 {
                    s[c] += sum[b][c];
                }
            }
            let n = n.max(1);
            [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
        })
        .collect()
}

fn nearest(pal: &[[u8; 3]], c: [i32; 3]) -> usize {
    let mut best = (i32::MAX, 0);
    for (i, q) in pal.iter().enumerate() {
        let d = (c[0] - q[0] as i32).pow(2) * 3 + (c[1] - q[1] as i32).pow(2) * 4 + (c[2] - q[2] as i32).pow(2) * 2;
        if d < best.0 {
            best = (d, i);
        }
    }
    best.1
}

/// `p` with its colours replaced by the nearest of `n` palette colours.
pub fn reduce(p: &Pixels, n: usize, dither: bool) -> Pixels {
    let pal = palette(p, n);
    // Nearest palette entry of every 5-bit colour, worked out once.
    let mut lut = vec![0u8; BINS];
    for (b, slot) in lut.iter_mut().enumerate() {
        let c = [((b >> (2 * BITS)) << 3 | 4) as i32, (((b >> BITS) & 31) << 3 | 4) as i32, ((b & 31) << 3 | 4) as i32];
        *slot = nearest(&pal, c) as u8;
    }
    let (w, h) = (p.w as usize, p.h as usize);
    let mut out = p.clone();
    if !dither {
        for px in out.rgba.chunks_exact_mut(4) {
            let q = pal[lut[bin_of(px[0], px[1], px[2])] as usize];
            px[..3].copy_from_slice(&q);
        }
        return out;
    }
    // Floyd-Steinberg, one row of error ahead.
    let mut err = vec![[0f32; 3]; (w + 2) * 2];
    for y in 0..h {
        let (cur, next) = (y % 2, (y + 1) % 2);
        err[next * (w + 2)..(next + 1) * (w + 2)].iter_mut().for_each(|e| *e = [0.0; 3]);
        for x in 0..w {
            let i = (y * w + x) * 4;
            let e = err[cur * (w + 2) + x + 1];
            let want = [0, 1, 2].map(|c| (out.rgba[i + c] as f32 + e[c]).clamp(0.0, 255.0));
            let q = pal[lut[bin_of(want[0] as u8, want[1] as u8, want[2] as u8)] as usize];
            let diff = [0, 1, 2].map(|c| want[c] - q[c] as f32);
            out.rgba[i..i + 3].copy_from_slice(&q);
            for c in 0..3 {
                err[cur * (w + 2) + x + 2][c] += diff[c] * 7.0 / 16.0;
                err[next * (w + 2) + x][c] += diff[c] * 3.0 / 16.0;
                err[next * (w + 2) + x + 1][c] += diff[c] * 5.0 / 16.0;
                err[next * (w + 2) + x + 2][c] += diff[c] / 16.0;
            }
        }
        err[cur * (w + 2)..(cur + 1) * (w + 2)].iter_mut().for_each(|e| *e = [0.0; 3]);
    }
    out
}

/// The picture as a palette PNG, if it has at most 256 different colours (with their alpha); None if it has more.
pub fn indexed_png(p: &Pixels) -> Option<Vec<u8>> {
    let mut pal: Vec<[u8; 4]> = Vec::new();
    let mut index = std::collections::HashMap::<[u8; 4], u8>::new();
    let mut data = Vec::with_capacity(p.w as usize * p.h as usize);
    for px in p.rgba.chunks_exact(4) {
        let key = [px[0], px[1], px[2], px[3]];
        let i = match index.get(&key) {
            Some(i) => *i,
            None => {
                if pal.len() >= 256 {
                    return None;
                }
                pal.push(key);
                index.insert(key, (pal.len() - 1) as u8);
                (pal.len() - 1) as u8
            }
        };
        data.push(i);
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, p.w, p.h);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(pal.iter().flat_map(|c| [c[0], c[1], c[2]]).collect::<Vec<u8>>());
    if pal.iter().any(|c| c[3] != 255) {
        enc.set_trns(pal.iter().map(|c| c[3]).collect::<Vec<u8>>());
    }
    enc.set_compression(png::Compression::High);
    let mut w = enc.write_header().ok()?;
    w.write_image_data(&data).ok()?;
    w.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rainbow(w: u32, h: u32) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|i| { let x = (i % w) as f32 / w as f32; let y = (i / w) as f32 / h as f32; [(x * 255.0) as u8, (y * 255.0) as u8, ((1.0 - x) * 255.0) as u8, 255] }).collect())
    }

    fn distinct(p: &Pixels) -> usize {
        p.rgba.chunks_exact(4).map(|px| [px[0], px[1], px[2]]).collect::<std::collections::HashSet<_>>().len()
    }

    #[test]
    fn the_result_has_at_most_n_colours_and_stays_close() {
        let p = rainbow(64, 64);
        assert!(distinct(&p) > 1000);
        for n in [2, 8, 16, 64, 256] {
            let r = reduce(&p, n, false);
            assert!(distinct(&r) <= n, "{n}: {}", distinct(&r));
        }
        // 64 colours of a smooth gradient: no pixel is far from its original.
        let r = reduce(&p, 64, false);
        let worst = p.rgba.iter().zip(&r.rgba).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst < 70, "{worst}");
        assert_eq!(r.rgba.iter().skip(3).step_by(4).copied().min(), Some(255), "alpha kept");
    }

    #[test]
    fn dithering_keeps_the_average_brighter_than_banding_does() {
        // A grey ramp reduced to 4 colours: with dithering the local average follows the ramp.
        let p = Pixels::new(256, 8, (0..256 * 8).flat_map(|i| { let g = (i % 256) as u8; [g, g, g, 255] }).collect());
        let plain = reduce(&p, 4, false);
        let dith = reduce(&p, 4, true);
        let avg = |q: &Pixels, x0: usize| (0..8).flat_map(|y| (x0..x0 + 16).map(move |x| (y, x))).map(|(y, x)| q.rgba[(y * 256 + x) * 4] as f32).sum::<f32>() / 128.0;
        let truth = |x0: usize| x0 as f32 + 7.5;
        let err = |q: &Pixels| [30usize, 90, 150, 210].iter().map(|&x| (avg(q, x) - truth(x)).abs()).sum::<f32>();
        assert!(err(&dith) < err(&plain), "{} vs {}", err(&dith), err(&plain));
    }

    #[test]
    fn a_reduced_picture_makes_a_much_smaller_palette_png() {
        let p = rainbow(96, 96);
        assert!(indexed_png(&p).is_none(), "too many colours for a palette");
        let r = reduce(&p, 16, true);
        let png = indexed_png(&r).unwrap();
        let full = crate::save::encode(&r, crate::save::Format::Png, None).unwrap();
        assert!(png.len() < full.len(), "{} vs {}", png.len(), full.len());
        // And it reads back to the same pixels.
        let mut dec = png::Decoder::new(std::io::Cursor::new(png));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(buf[0..3], r.rgba[0..3]);
    }

    #[test]
    fn a_picture_with_few_colours_and_transparency_round_trips() {
        let p = Pixels::new(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 0, 0, 255]);
        let png = indexed_png(&p).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(png));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        assert_eq!(buf, p.rgba);
    }
}
