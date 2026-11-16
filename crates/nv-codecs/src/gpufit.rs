//! The fitted view of a picture, made on the CPU **the way the GPU makes it** (`nv-render`, `quad.hlsl`): shrink by whole
//! halvings (a mip chain of 2x2 averages in linear light, kept as 8-bit sRGB like the texture is) until the rest of the way is
//! between one half and one, then resample with Catmull-Rom (16 texel loads, in linear light), clamp, and encode.
//!
//! Why: the first picture is painted before the GPU exists. If that first frame were made with another filter (a plain area
//! average, softer than Catmull-Rom), the GPU frame that replaces it would change the picture, and so would every return to
//! that picture later. Made this way the CPU frame and the GPU frame agree to within a level or two, so the first frame can
//! simply stay.

use crate::Image;

struct Luts {
    /// sRGB byte -> linear light.
    decode: [f32; 256],
    /// Linear light (0..1 in 8192 steps) -> sRGB byte.
    encode: Vec<u8>,
}

fn luts() -> &'static Luts {
    static L: std::sync::OnceLock<Luts> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        let mut decode = [0f32; 256];
        for (i, d) in decode.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *d = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        let encode = (0..=8192)
            .map(|i| {
                let c = i as f32 / 8192.0;
                let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
                (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
            })
            .collect();
        Luts { decode, encode }
    })
}

fn enc(l: &Luts, c: f32) -> u8 {
    l.encode[(c.clamp(0.0, 1.0) * 8192.0 + 0.5) as usize]
}

fn threads_for(rows: usize, pixels: usize) -> usize {
    if pixels < 400_000 {
        return 1;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8).min(rows.max(1))
}

/// One mip level down: every texel the average (in linear light) of a 2x2 block of the level above.
fn halve(src: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let l = luts();
    let mut out = vec![0u8; nw * nh * 4];
    let threads = threads_for(nh, w * h);
    let per = nh.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(per * nw * 4).enumerate() {
            s.spawn(move || {
                for (k, row) in chunk.chunks_mut(nw * 4).enumerate() {
                    let y = ci * per + k;
                    let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
                    for (x, px) in row.chunks_exact_mut(4).enumerate() {
                        let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
                        let taps = [(y0, x0), (y0, x1), (y1, x0), (y1, x1)];
                        for c in 0..3 {
                            let sum: f32 = taps.iter().map(|&(yy, xx)| l.decode[src[(yy * w + xx) * 4 + c] as usize]).sum();
                            px[c] = enc(l, sum * 0.25);
                        }
                        let a: u32 = taps.iter().map(|&(yy, xx)| src[(yy * w + xx) * 4 + 3] as u32).sum();
                        px[3] = ((a + 2) / 4) as u8;
                    }
                }
            });
        }
    });
    (out, nw, nh)
}

/// Catmull-Rom weights for the fraction `f` (the same polynomials as the shader).
fn weights(f: f32) -> [f32; 4] {
    [f * (-0.5 + f * (1.0 - 0.5 * f)), 1.0 + f * f * (-2.5 + 1.5 * f), f * (0.5 + f * (2.0 - 1.5 * f)), f * f * (-0.5 + 0.5 * f)]
}

/// `img` shrunk by `scale` (< 1) for display: the result is `round(w * scale)` by `round(h * scale)` of the *displayed*
/// (oriented) picture, upright, straight-alpha RGBA, as the GPU would draw it into a rectangle of that size.
pub fn fit_like_gpu(img: &Image, scale: f32) -> Image {
    let (sw, sh) = (img.width as usize, img.height as usize);
    let o = img.orientation.clamp(1, 8);
    let (dw, dh) = if o >= 5 { (sh, sw) } else { (sw, sh) };
    let (wf, hf) = (dw as f32 * scale, dh as f32 * scale);
    let (pw, ph) = ((wf.round() as usize).max(1), (hf.round() as usize).max(1));
    // The mip level: whole halvings until the rest of the way is in [0.5, 1).
    let lod = if scale < 1.0 { (-scale.log2()).floor().max(0.0) as u32 } else { 0 };
    let mut level: Option<(Vec<u8>, usize, usize)> = None;
    for _ in 0..lod {
        let next = match &level {
            None => halve(&img.rgba, sw, sh),
            Some((d, w, h)) => halve(d, *w, *h),
        };
        let tiny = next.1 == 1 && next.2 == 1;
        level = Some(next);
        if tiny {
            break;
        }
    }
    let (data, lw, lh): (&[u8], usize, usize) = match &level {
        Some((d, w, h)) => (d, *w, *h),
        None => (&img.rgba, sw, sh),
    };
    let l = luts();
    let mut out = vec![0u8; pw * ph * 4];
    let threads = threads_for(ph, pw * ph);
    let per = ph.div_ceil(threads).max(1);
    let (lwf, lhf) = (lw as f32, lh as f32);
    std::thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(per * pw * 4).enumerate() {
            s.spawn(move || {
                let at = |x: i64, y: i64| -> &[u8] {
                    let (x, y) = (x.clamp(0, lw as i64 - 1) as usize, y.clamp(0, lh as i64 - 1) as usize);
                    &data[(y * lw + x) * 4..(y * lw + x) * 4 + 4]
                };
                for (k, row) in chunk.chunks_mut(pw * 4).enumerate() {
                    let j = ci * per + k;
                    let v = (j as f32 + 0.5) / hf;
                    for (i, px) in row.chunks_exact_mut(4).enumerate() {
                        let u = (i as f32 + 0.5) / wf;
                        // Orientation: the displayed quad's uv back to the stored pixels (as the shader does).
                        let (u, v) = match o {
                            2 => (1.0 - u, v),
                            3 => (1.0 - u, 1.0 - v),
                            4 => (u, 1.0 - v),
                            5 => (v, u),
                            6 => (v, 1.0 - u),
                            7 => (1.0 - v, 1.0 - u),
                            8 => (1.0 - v, u),
                            _ => (u, v),
                        };
                        let (px_, py_) = (u * lwf - 0.5, v * lhf - 0.5);
                        let (fx, fy) = (px_ - px_.floor(), py_ - py_.floor());
                        let (bx, by) = (px_.floor() as i64, py_.floor() as i64);
                        let (wx, wy) = (weights(fx), weights(fy));
                        let mut acc = [0f32; 4];
                        for (yy, wyv) in wy.iter().enumerate() {
                            let mut r = [0f32; 4];
                            for (xx, wxv) in wx.iter().enumerate() {
                                let t = at(bx + xx as i64 - 1, by + yy as i64 - 1);
                                r[0] += wxv * l.decode[t[0] as usize];
                                r[1] += wxv * l.decode[t[1] as usize];
                                r[2] += wxv * l.decode[t[2] as usize];
                                r[3] += wxv * (t[3] as f32 / 255.0);
                            }
                            for c in 0..4 {
                                acc[c] += wyv * r[c];
                            }
                        }
                        px[0] = enc(l, acc[0]);
                        px[1] = enc(l, acc[1]);
                        px[2] = enc(l, acc[2]);
                        px[3] = (acc[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    }
                }
            });
        }
    });
    let mut r = Image::new(pw as u32, ph as u32, out);
    r.profile_assumable = img.profile_assumable;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, p: [u8; 4]) -> Image {
        Image::new(w, h, p.iter().copied().cycle().take((w * h * 4) as usize).collect())
    }

    #[test]
    fn a_flat_picture_stays_flat_and_sizes_round_like_the_view() {
        let r = fit_like_gpu(&solid(1000, 700, [200, 100, 50, 255]), 0.37);
        assert_eq!((r.width, r.height), (370, 259));
        assert!(r.rgba.chunks_exact(4).all(|p| (p[0] as i32 - 200).abs() <= 1 && (p[1] as i32 - 100).abs() <= 1 && (p[2] as i32 - 50).abs() <= 1 && p[3] == 255));
    }

    #[test]
    fn a_quarter_turn_swaps_the_shape() {
        let mut img = solid(400, 200, [10, 20, 30, 255]);
        img.orientation = 6;
        let r = fit_like_gpu(&img, 0.5);
        assert_eq!((r.width, r.height), (100, 200));
    }

    #[test]
    fn fine_stripes_average_to_the_light_average_not_the_byte_average() {
        // 1-pixel black / white stripes shrunk 2x: the average in linear light is 0.5 = sRGB 188, not 128.
        let (w, h) = (256u32, 8u32);
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x % 2 == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let r = fit_like_gpu(&Image::new(w, h, rgba), 0.5);
        let mid = &r.rgba[(r.width as usize / 2) * 4..][..3];
        assert!((mid[0] as i32 - 188).abs() <= 3, "{mid:?}");
    }
}

/// A small picture (a few dozen pixels: the preview stored in a 3ds Max file) enlarged `k` times with Catmull-Rom, so it is not a
/// postage stamp. Filtering is done in linear light.
pub fn upscale_smooth(img: &Image, k: u32) -> Image {
    let k = k.clamp(1, 16) as usize;
    let (sw, sh) = (img.width as usize, img.height as usize);
    let (ow, oh) = (sw * k, sh * k);
    let l = luts();
    let mut out = vec![0u8; ow * oh * 4];
    let at = |x: i64, y: i64| -> &[u8] {
        let (x, y) = (x.clamp(0, sw as i64 - 1) as usize, y.clamp(0, sh as i64 - 1) as usize);
        &img.rgba[(y * sw + x) * 4..(y * sw + x) * 4 + 4]
    };
    for j in 0..oh {
        let py = (j as f32 + 0.5) / k as f32 - 0.5;
        let (by, fy) = (py.floor() as i64, py - py.floor());
        let wy = weights(fy);
        for i in 0..ow {
            let px = (i as f32 + 0.5) / k as f32 - 0.5;
            let (bx, fx) = (px.floor() as i64, px - px.floor());
            let wx = weights(fx);
            let mut acc = [0f32; 4];
            for (yy, wyv) in wy.iter().enumerate() {
                for (xx, wxv) in wx.iter().enumerate() {
                    let t = at(bx + xx as i64 - 1, by + yy as i64 - 1);
                    let w = wxv * wyv;
                    acc[0] += w * l.decode[t[0] as usize];
                    acc[1] += w * l.decode[t[1] as usize];
                    acc[2] += w * l.decode[t[2] as usize];
                    acc[3] += w * (t[3] as f32 / 255.0);
                }
            }
            let o = (j * ow + i) * 4;
            out[o] = enc(l, acc[0]);
            out[o + 1] = enc(l, acc[1]);
            out[o + 2] = enc(l, acc[2]);
            out[o + 3] = (acc[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    let mut r = Image::new(ow as u32, oh as u32, out);
    r.profile_assumable = img.profile_assumable;
    r
}
