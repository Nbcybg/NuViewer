//! Pixel operations. Rotation is clockwise, like on screen.

/// RGBA8, straight (not premultiplied) alpha, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    pub fn new(w: u32, h: u32, rgba: Vec<u8>) -> Pixels {
        assert_eq!(rgba.len(), w as usize * h as usize * 4, "pixel buffer doesn't match {w}x{h}");
        Pixels { w, h, rgba }
    }
}

/// Angle in degrees brought into 0..360.
pub fn normalize_degrees(d: f32) -> f32 {
    let r = d.rem_euclid(360.0);
    if (r - 360.0).abs() < 1e-4 { 0.0 } else { r }
}

/// A right angle (0, 90, 180, 270) as a number of clockwise quarter turns.
pub fn quarter_turns(d: f32) -> Option<u8> {
    let r = normalize_degrees(d);
    let q = (r / 90.0).round();
    ((r - q * 90.0).abs() < 1e-3).then_some((q as u8) % 4)
}

/// Size of the canvas that holds a `w` x `h` picture turned by `degrees`.
pub fn rotated_size(w: u32, h: u32, degrees: f32) -> (u32, u32) {
    if let Some(q) = quarter_turns(degrees) {
        return if q % 2 == 1 { (h, w) } else { (w, h) };
    }
    let a = degrees.to_radians();
    let (s, c) = (a.sin().abs(), a.cos().abs());
    let (fw, fh) = (w as f32, h as f32);
    (((fw * c + fh * s) - 1e-3).ceil().max(1.0) as u32, ((fw * s + fh * c) - 1e-3).ceil().max(1.0) as u32)
}

/// The picture turned clockwise by `degrees`. Right angles are exact pixel moves; other angles grow the canvas and leave
/// the corners transparent (the edges are smoothed).
pub fn rotate(src: &Pixels, degrees: f32) -> Pixels {
    match quarter_turns(degrees) {
        Some(0) => src.clone(),
        Some(q) => rotate_quarter(src, q),
        None => rotate_any(src, degrees),
    }
}

fn rotate_quarter(src: &Pixels, q: u8) -> Pixels {
    let (w, h) = (src.w as usize, src.h as usize);
    let (ow, oh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; ow * oh * 4];
    parallel_rows(&mut out, ow * 4, |y, row| {
        for x in 0..ow {
            let (sx, sy) = match q {
                1 => (y, h - 1 - x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (w - 1 - y, x),
            };
            row[x * 4..x * 4 + 4].copy_from_slice(&src.rgba[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4]);
        }
    });
    Pixels { w: ow as u32, h: oh as u32, rgba: out }
}

fn rotate_any(src: &Pixels, degrees: f32) -> Pixels {
    let (ow, oh) = rotated_size(src.w, src.h, degrees);
    let a = degrees.to_radians();
    let (sin, cos) = (a.sin(), a.cos());
    let (w, h) = (src.w as i32, src.h as i32);
    let (scx, scy) = (src.w as f32 / 2.0, src.h as f32 / 2.0);
    let (ocx, ocy) = (ow as f32 / 2.0, oh as f32 / 2.0);
    let mut out = vec![0u8; ow as usize * oh as usize * 4];
    // Premultiplied sample of the source; outside the picture is transparent.
    let texel = |x: i32, y: i32| -> [f32; 4] {
        if x < 0 || y < 0 || x >= w || y >= h {
            return [0.0; 4];
        }
        let o = (y as usize * w as usize + x as usize) * 4;
        let a = src.rgba[o + 3] as f32;
        [src.rgba[o] as f32 * a, src.rgba[o + 1] as f32 * a, src.rgba[o + 2] as f32 * a, a]
    };
    parallel_rows(&mut out, ow as usize * 4, |y, row| {
        for x in 0..ow as usize {
            let (dx, dy) = (x as f32 + 0.5 - ocx, y as f32 + 0.5 - ocy);
            // Back to the source: undo the clockwise turn.
            let (sx, sy) = (dx * cos + dy * sin + scx - 0.5, -dx * sin + dy * cos + scy - 0.5);
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i32, y0 as i32);
            let mut acc = [0.0f32; 4];
            for (dxi, dyi, wgt) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] {
                if wgt > 0.0 {
                    let t = texel(x0 + dxi, y0 + dyi);
                    for k in 0..4 {
                        acc[k] += t[k] * wgt;
                    }
                }
            }
            let o = x * 4;
            if acc[3] > 0.0 {
                row[o] = (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8;
                row[o + 1] = (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8;
                row[o + 2] = (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8;
                row[o + 3] = acc[3].round().clamp(0.0, 255.0) as u8;
            }
        }
    });
    Pixels { w: ow, h: oh, rgba: out }
}

pub fn flip_horizontal(src: &Pixels) -> Pixels {
    let w = src.w as usize;
    let mut out = vec![0u8; src.rgba.len()];
    parallel_rows(&mut out, w * 4, |y, row| {
        for x in 0..w {
            let s = (y * w + (w - 1 - x)) * 4;
            row[x * 4..x * 4 + 4].copy_from_slice(&src.rgba[s..s + 4]);
        }
    });
    Pixels { w: src.w, h: src.h, rgba: out }
}

pub fn flip_vertical(src: &Pixels) -> Pixels {
    let (w, h) = (src.w as usize, src.h as usize);
    let mut out = vec![0u8; src.rgba.len()];
    for y in 0..h {
        out[y * w * 4..(y + 1) * w * 4].copy_from_slice(&src.rgba[(h - 1 - y) * w * 4..(h - y) * w * 4]);
    }
    Pixels { w: src.w, h: src.h, rgba: out }
}

/// Composite onto an opaque background (for formats without alpha, such as JPEG). Alpha becomes 255.
pub fn flatten(p: &mut Pixels, bg: [u8; 3]) {
    for px in p.rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a == 255 {
            continue;
        }
        for k in 0..3 {
            px[k] = ((px[k] as u32 * a + bg[k] as u32 * (255 - a) + 127) / 255) as u8;
        }
        px[3] = 255;
    }
}

/// Fill `buf` (rows of `stride` bytes) by calling `f(row_index, row)` on several threads.
fn parallel_rows(buf: &mut [u8], stride: usize, f: impl Fn(usize, &mut [u8]) + Sync) {
    let rows = buf.len() / stride.max(1);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8).min(rows.max(1));
    if threads <= 1 || rows < 64 {
        for (y, row) in buf.chunks_exact_mut(stride).enumerate() {
            f(y, row);
        }
        return;
    }
    let per = rows.div_ceil(threads);
    std::thread::scope(|s| {
        for (t, chunk) in buf.chunks_mut(per * stride).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (i, row) in chunk.chunks_exact_mut(stride).enumerate() {
                    f(t * per + i, row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 3x2 picture with a distinct colour per pixel: red channel = x + 10*y.
    fn sample() -> Pixels {
        let mut rgba = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                rgba.extend_from_slice(&[x + 10 * y, 0, 0, 255]);
            }
        }
        Pixels::new(3, 2, rgba)
    }

    fn reds(p: &Pixels) -> Vec<u8> {
        p.rgba.chunks_exact(4).map(|c| c[0]).collect()
    }

    #[test]
    fn quarter_turns_move_pixels_clockwise() {
        let p = sample(); // rows: [0 1 2] / [10 11 12]
        let cw = rotate(&p, 90.0);
        assert_eq!((cw.w, cw.h), (2, 3));
        assert_eq!(reds(&cw), vec![10, 0, 11, 1, 12, 2], "first column of the source (top to bottom) becomes the top row, right to left... read as rows");
        let half = rotate(&p, 180.0);
        assert_eq!(reds(&half), vec![12, 11, 10, 2, 1, 0]);
        let ccw = rotate(&p, -90.0);
        assert_eq!((ccw.w, ccw.h), (2, 3));
        assert_eq!(reds(&ccw), vec![2, 12, 1, 11, 0, 10]);
        assert_eq!(rotate(&p, 360.0), p);
        assert_eq!(rotate(&rotate(&p, 90.0), 270.0), p);
    }

    #[test]
    fn four_quarter_turns_are_the_identity_for_bigger_pictures() {
        let (w, h) = (130u32, 97u32);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect();
        let p = Pixels::new(w, h, rgba);
        let r = rotate(&rotate(&rotate(&rotate(&p, 90.0), 90.0), 90.0), 90.0);
        assert_eq!(r, p);
    }

    #[test]
    fn canvas_for_45_degrees_holds_the_whole_picture() {
        let (w, h) = rotated_size(100, 100, 45.0);
        assert!((w as i32 - 142).abs() <= 1 && (h as i32 - 142).abs() <= 1, "{w}x{h}");
        assert_eq!(rotated_size(300, 200, 90.0), (200, 300));
        assert_eq!(rotated_size(300, 200, 180.0), (300, 200));
        let (w2, h2) = rotated_size(300, 200, 45.0);
        assert_eq!(w2, h2, "a 45 degree turn gives a square-ish canvas: (w+h)/sqrt2 both ways");
    }

    #[test]
    fn rotating_45_keeps_the_centre_and_leaves_the_corners_transparent() {
        let (w, h) = (64u32, 64u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[200, 100, 50, 255]);
        }
        let r = rotate(&Pixels::new(w, h, rgba), 45.0);
        let at = |x: u32, y: u32| &r.rgba[((y * r.w + x) * 4) as usize..((y * r.w + x) * 4) as usize + 4];
        assert_eq!(at(r.w / 2, r.h / 2), &[200, 100, 50, 255], "the middle is untouched");
        assert_eq!(at(0, 0)[3], 0, "the canvas corner is empty");
        assert_eq!(at(r.w - 1, r.h - 1)[3], 0);
        // The middle of each canvas edge is a corner of the picture: still covered.
        assert!(at(r.w / 2, 2)[3] > 128);
    }

    #[test]
    fn eight_turns_of_45_return_close_to_the_start() {
        // A smooth gradient survives the resampling well enough (this checks direction and centring, not exactness).
        let (w, h) = (80u32, 60u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 3) as u8, (y * 4) as u8, 128, 255]);
            }
        }
        let p = Pixels::new(w, h, rgba);
        let r = rotate(&p, 45.0);
        // 45 clockwise then 315 clockwise: back to upright, on a bigger canvas. Compare the central region.
        let back = rotate(&r, -45.0);
        let (ox, oy) = ((back.w - w) / 2, (back.h - h) / 2);
        let mut worst = 0i32;
        for y in 8..h - 8 {
            for x in 8..w - 8 {
                let a = &p.rgba[((y * w + x) * 4) as usize..][..3];
                let b = &back.rgba[(((y + oy) * back.w + x + ox) * 4) as usize..][..3];
                for k in 0..3 {
                    worst = worst.max((a[k] as i32 - b[k] as i32).abs());
                }
            }
        }
        assert!(worst <= 14, "round trip drifted by {worst}");
    }

    #[test]
    fn flips_and_flatten() {
        let p = sample();
        assert_eq!(reds(&flip_horizontal(&p)), vec![2, 1, 0, 12, 11, 10]);
        assert_eq!(reds(&flip_vertical(&p)), vec![10, 11, 12, 0, 1, 2]);
        let mut t = Pixels::new(1, 2, vec![255, 0, 0, 0, 255, 0, 0, 255]);
        flatten(&mut t, [0, 0, 255]);
        assert_eq!(t.rgba, vec![0, 0, 255, 255, 255, 0, 0, 255], "transparent becomes the background, opaque is kept");
    }

    #[test]
    fn angles_normalise() {
        assert_eq!(normalize_degrees(-45.0), 315.0);
        assert_eq!(normalize_degrees(405.0), 45.0);
        assert_eq!(quarter_turns(270.0), Some(3));
        assert_eq!(quarter_turns(-90.0), Some(3));
        assert_eq!(quarter_turns(45.0), None);
    }
}
