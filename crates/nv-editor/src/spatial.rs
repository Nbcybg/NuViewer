//! The neighbourhood stage of the engine: LUT, noise reduction, clarity, sharpen, skin smoothing, bloom, film blur, grain.
//!
//! These work on a pixel and what is around it, so their size has to follow the *picture*, not the buffer they are given:
//! every radius is a fraction of the long side of the whole picture (`Frame`), 1.0 = 2000 pixels. A preview on a 1080p proxy
//! and the saved 24 MP picture therefore look the same. A buffer that is only a piece of the picture (a zoomed-in view) needs
//! [`margin`] extra pixels around it, which the caller cuts off again afterwards.
//!
//! Order: LUT -> noise reduction -> clarity -> sharpen -> skin smoothing -> bloom -> film blur -> grain.

use crate::blur;
use crate::engine::Frame;
use crate::lut;
use crate::raster::Pixels;
use crate::recipe::*;

/// Pixels per "unit" of size: the long side of the whole picture over 2000.
pub fn unit(frame: &Frame) -> f32 {
    frame.w.max(frame.h) as f32 / 2000.0
}

fn sigma_sharpen(r: &Recipe, frame: &Frame) -> f32 {
    (r.get(P::SharpenRadius) * unit(frame)).max(0.5)
}

fn sigma_clarity(frame: &Frame) -> f32 {
    (0.012 * frame.w.max(frame.h) as f32).max(2.0)
}

fn sigma_bloom(r: &Recipe, frame: &Frame) -> f32 {
    ((0.004 + 0.03 * r.get(P::BloomRadius) / 100.0) * frame.w.max(frame.h) as f32).max(1.0)
}

fn sigma_film(r: &Recipe, frame: &Frame) -> f32 {
    ((0.002 + 0.010 * r.get(P::FilmBlur) / 100.0) * frame.w.max(frame.h) as f32).max(0.8)
}

fn sigma_nr_luma(r: &Recipe, frame: &Frame) -> f32 {
    (0.7 + 2.2 * r.get(P::NoiseLuma) / 100.0) * unit(frame).max(0.5)
}

fn sigma_nr_chroma(r: &Recipe, frame: &Frame) -> f32 {
    (1.5 + 9.0 * r.get(P::NoiseChroma) / 100.0) * unit(frame).max(0.5)
}

fn sigma_skin(r: &Recipe, frame: &Frame) -> f32 {
    (1.5 + 3.5 * r.get(P::SkinSmooth) / 100.0) * unit(frame).max(0.5)
}

/// How many pixels of surroundings a piece of the picture needs for the neighbourhood stage to come out as on the whole.
pub fn margin(r: &Recipe, frame: &Frame) -> u32 {
    let mut s = 0.0f32;
    if r.get(P::Sharpen) != 0.0 {
        s = s.max(sigma_sharpen(r, frame));
    }
    if r.get(P::Clarity) != 0.0 {
        s = s.max(sigma_clarity(frame));
    }
    if r.get(P::Bloom) != 0.0 {
        s = s.max(sigma_bloom(r, frame));
    }
    if r.get(P::FilmBlur) != 0.0 {
        s = s.max(sigma_film(r, frame));
    }
    if r.get(P::NoiseLuma) != 0.0 {
        s = s.max(sigma_nr_luma(r, frame));
    }
    if r.get(P::NoiseChroma) != 0.0 {
        s = s.max(sigma_nr_chroma(r, frame));
    }
    if r.get(P::SkinSmooth) != 0.0 {
        s = s.max(sigma_skin(r, frame));
    }
    if s == 0.0 { 0 } else { (s * 3.0).ceil() as u32 + 2 }
}

// ---------------------------------------------------------------------------------------------------- helpers

fn threads_for(rows: usize, row_bytes: usize) -> usize {
    let by_size = (rows * row_bytes) / (256 * 1024);
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(by_size.max(1)).min(rows.max(1))
}

/// Run `f(row index, the row's RGBA bytes)` over all rows, in bands on all cores.
fn par_rows<F: Fn(usize, &mut [u8]) + Sync>(buf: &mut [u8], w: usize, h: usize, f: F) {
    if w == 0 || h == 0 {
        return;
    }
    let n = threads_for(h, w * 4);
    let band = h.div_ceil(n);
    std::thread::scope(|s| {
        for (i, chunk) in buf.chunks_mut(band * w * 4).enumerate() {
            let f = &f;
            let mut run = move || {
                for (r, row) in chunk.chunks_exact_mut(w * 4).enumerate() {
                    f(i * band + r, row);
                }
            };
            if n == 1 {
                run();
            } else {
                s.spawn(run);
            }
        }
    });
}

#[inline]
fn luma8(p: &[u8]) -> f32 {
    0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
}

fn luma_plane(p: &Pixels) -> Vec<u8> {
    p.rgba.chunks_exact(4).map(|px| (luma8(px) + 0.5) as u8).collect()
}

#[inline]
fn put(c: &mut u8, v: f32) {
    *c = (v + 0.5).clamp(0.0, 255.0) as u8;
}

/// Add `gain(luma, difference to the blurred luma)` to all three channels: the shape of both sharpen and clarity.
fn add_detail(p: &mut Pixels, sigma: f32, gain: impl Fn(f32, f32) -> f32 + Sync) {
    let (w, h) = (p.w as usize, p.h as usize);
    let y = luma_plane(p);
    let yb = blur::gaussian(&y, w, h, 1, 1, sigma);
    let (y, yb) = (&y, &yb);
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let i = row * w + x;
            let add = gain(y[i] as f32, y[i] as f32 - yb[i] as f32);
            for c in 0..3 {
                let v = data[x * 4 + c] as f32 + add;
                put(&mut data[x * 4 + c], v);
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------------- effects

fn apply_lut(p: &mut Pixels, path: &str, strength: f32) {
    let Ok(lut) = lut::load(path) else { return };
    let k = strength.clamp(0.0, 1.0);
    if k == 0.0 {
        return;
    }
    let (w, h) = (p.w as usize, p.h as usize);
    par_rows(&mut p.rgba, w, h, |_, data| {
        for px in data.chunks_exact_mut(4) {
            let src = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
            let o = lut.lookup(src);
            for c in 0..3 {
                put(&mut px[c], (src[c] + (o[c] - src[c]) * k) * 255.0);
            }
        }
    });
}

/// Edge-preserving smoothing of the luminance: blur, but not across an edge.
fn noise_reduce_luma(p: &mut Pixels, amount: f32, sigma: f32) {
    let (w, h) = (p.w as usize, p.h as usize);
    let bl = blur::gaussian(&p.rgba, w, h, 4, 3, sigma);
    let bl = &bl;
    let t = 4.0 + amount * 28.0; // an edge is a luma step well above the noise this strong
    let strength = (amount * 1.4).min(1.0);
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let b = &bl[(row * w + x) * 4..];
            let d = (luma8(&data[x * 4..]) - luma8(b)).abs() / t;
            let mix = strength / (1.0 + d * d);
            for c in 0..3 {
                let s = data[x * 4 + c] as f32;
                put(&mut data[x * 4 + c], s + (b[c] as f32 - s) * mix);
            }
        }
    });
}

/// Blur the colour (not the brightness), where the picture has no brightness edge.
fn noise_reduce_chroma(p: &mut Pixels, amount: f32, sigma: f32) {
    let (w, h) = (p.w as usize, p.h as usize);
    let bl = blur::gaussian(&p.rgba, w, h, 4, 3, sigma);
    let bl = &bl;
    let strength = (amount * 1.2).min(1.0);
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let b = &bl[(row * w + x) * 4..];
            let (ys, yb) = (luma8(&data[x * 4..]), luma8(b));
            let d = (ys - yb).abs() / 10.0;
            let mix = strength / (1.0 + d * d);
            for c in 0..3 {
                let (cs, cb) = (data[x * 4 + c] as f32 - ys, b[c] as f32 - yb);
                put(&mut data[x * 4 + c], ys + cs + (cb - cs) * mix);
            }
        }
    });
}

fn clarity(p: &mut Pixels, amount: f32, sigma: f32) {
    // Local contrast, strongest in the mid-tones (the ends of the scale are left alone so nothing clips or greys out).
    add_detail(p, sigma, |y, d| {
        let m = y / 127.5 - 1.0;
        amount * 1.1 * d * (1.0 - 0.7 * m * m)
    });
}

fn sharpen(p: &mut Pixels, amount: f32, sigma: f32) {
    // Unsharp mask on the luminance; tiny differences (noise) are let through less.
    add_detail(p, sigma, |_, d| {
        let gate = d.abs() / (d.abs() + 2.0);
        amount * 2.0 * d * gate
    });
}

/// Skin-like colours get the smoothing; everything else is left as it was.
fn skin_smooth(p: &mut Pixels, amount: f32, sigma: f32) {
    let (w, h) = (p.w as usize, p.h as usize);
    let bl = blur::gaussian(&p.rgba, w, h, 4, 3, sigma);
    let bl = &bl;
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let px = &data[x * 4..x * 4 + 4];
            let (r, g, b) = (px[0] as f32, px[1] as f32, px[2] as f32);
            let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
            let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
            // Distance outside the usual skin box in the Cb / Cr plane, turned into a soft membership.
            let dcb = (77.0 - cb).max(cb - 127.0).max(0.0);
            let dcr = (133.0 - cr).max(cr - 173.0).max(0.0);
            let y = luma8(px);
            let dy = (35.0 - y).max(y - 250.0).max(0.0);
            let skin = (1.0 - (dcb + dcr + dy) / 14.0).clamp(0.0, 1.0);
            if skin == 0.0 {
                continue;
            }
            let bp = &bl[(row * w + x) * 4..];
            // Not across an edge: eyes, lips and hair keep their detail.
            let d = (y - luma8(bp)).abs() / 14.0;
            let mix = amount * 0.9 * skin / (1.0 + d * d);
            for c in 0..3 {
                let s = data[x * 4 + c] as f32;
                put(&mut data[x * 4 + c], s + (bp[c] as f32 - s) * mix);
            }
        }
    });
}

fn bloom(p: &mut Pixels, amount: f32, threshold: f32, sigma: f32) {
    let (w, h) = (p.w as usize, p.h as usize);
    // The bright part of the picture only.
    let mut bright = p.clone();
    let thr = threshold * 255.0;
    let span = (255.0 - thr).max(1.0);
    par_rows(&mut bright.rgba, w, h, |_, data| {
        for px in data.chunks_exact_mut(4) {
            let k = ((luma8(px) - thr) / span).clamp(0.0, 1.0);
            for c in 0..3 {
                px[c] = (px[c] as f32 * k + 0.5) as u8;
            }
        }
    });
    let glow = blur::gaussian(&bright.rgba, w, h, 4, 3, sigma);
    let glow = &glow;
    let k = amount * 1.6;
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let g = &glow[(row * w + x) * 4..];
            for c in 0..3 {
                let (s, b) = (data[x * 4 + c] as f32 / 255.0, (g[c] as f32 / 255.0 * k).min(1.0));
                put(&mut data[x * 4 + c], (1.0 - (1.0 - s) * (1.0 - b)) * 255.0); // screen
            }
        }
    });
}

fn film_blur(p: &mut Pixels, amount: f32, sigma: f32) {
    let (w, h) = (p.w as usize, p.h as usize);
    let bl = blur::gaussian(&p.rgba, w, h, 4, 3, sigma);
    let bl = &bl;
    let mix = amount * 0.8;
    par_rows(&mut p.rgba, w, h, |row, data| {
        for x in 0..w {
            let b = &bl[(row * w + x) * 4..];
            for c in 0..3 {
                let s = data[x * 4 + c] as f32;
                put(&mut data[x * 4 + c], s + (b[c] as f32 - s) * mix);
            }
        }
    });
}

/// A value in -1..1 for the lattice point (ix, iy); the same inputs always give the same value, so grain does not move
/// between a preview, a tile and the saved picture.
fn hash(ix: i32, iy: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x9E37_79B1) ^ (iy as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32 * 2.0 - 1.0
}

/// Smooth value noise: lattice values blended with a smooth step.
fn lattice(x: f32, y: f32, seed: u32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (ix, iy) = (fx as i32, fy as i32);
    let a = hash(ix, iy, seed) * (1.0 - sx) + hash(ix + 1, iy, seed) * sx;
    let b = hash(ix, iy + 1, seed) * (1.0 - sx) + hash(ix + 1, iy + 1, seed) * sx;
    a * (1.0 - sy) + b * sy
}

/// One grain sample: `rough` 0 = soft clumps of the chosen size, 1 = single-pixel noise.
fn grain_at(x: f32, y: f32, cell: f32, rough: f32, seed: u32) -> f32 {
    let soft = lattice(x / cell, y / cell, seed) * 1.7;
    let white = hash(x as i32, y as i32, seed ^ 0x55AA);
    soft * (1.0 - rough) + white * rough
}

fn grain(p: &mut Pixels, r: &Recipe, frame: Frame) {
    let (w, h) = (p.w as usize, p.h as usize);
    let amp = r.get(P::Grain) / 100.0 * 0.18 * 255.0;
    let cell = ((0.5 + r.get(P::GrainSize) / 100.0 * 3.5) * unit(&frame)).max(1.0);
    let rough = r.get(P::GrainRough) / 100.0;
    let colour = r.get(P::GrainColor) / 100.0;
    let (x0, y0) = (frame.x0 as f32, frame.y0 as f32);
    par_rows(&mut p.rgba, w, h, |row, data| {
        let gy = y0 + row as f32;
        for x in 0..w {
            let gx = x0 + x as f32;
            let px = &mut data[x * 4..x * 4 + 4];
            let y = luma8(px);
            let m = y / 127.5 - 1.0;
            let weight = amp * (1.0 - 0.7 * m * m); // most visible in the mid-tones, as on film
            let mono = grain_at(gx, gy, cell, rough, 1);
            for c in 0..3 {
                let n = if colour > 0.0 { mono + (grain_at(gx, gy, cell, rough, 11 + c as u32) - mono) * colour } else { mono };
                let v = px[c] as f32 + n * weight;
                put(&mut px[c], v);
            }
        }
    });
}

/// Apply the whole neighbourhood stage. `frame` says where `p` sits in the whole picture.
pub fn apply(mut p: Pixels, r: &Recipe, frame: Frame) -> Pixels {
    if r.lut.is_none() && r.spatial_identity() {
        return p;
    }
    if let Some(path) = &r.lut {
        apply_lut(&mut p, path, r.get(P::LutStrength) / 100.0);
    }
    if r.get(P::NoiseLuma) > 0.0 {
        noise_reduce_luma(&mut p, r.get(P::NoiseLuma) / 100.0, sigma_nr_luma(r, &frame));
    }
    if r.get(P::NoiseChroma) > 0.0 {
        noise_reduce_chroma(&mut p, r.get(P::NoiseChroma) / 100.0, sigma_nr_chroma(r, &frame));
    }
    if r.get(P::Clarity) != 0.0 {
        clarity(&mut p, r.get(P::Clarity) / 100.0, sigma_clarity(&frame));
    }
    if r.get(P::Sharpen) > 0.0 {
        sharpen(&mut p, r.get(P::Sharpen) / 100.0, sigma_sharpen(r, &frame));
    }
    if r.get(P::SkinSmooth) > 0.0 {
        skin_smooth(&mut p, r.get(P::SkinSmooth) / 100.0, sigma_skin(r, &frame));
    }
    if r.get(P::Bloom) > 0.0 {
        bloom(&mut p, r.get(P::Bloom) / 100.0, r.get(P::BloomThreshold) / 100.0, sigma_bloom(r, &frame));
    }
    if r.get(P::FilmBlur) > 0.0 {
        film_blur(&mut p, r.get(P::FilmBlur) / 100.0, sigma_film(r, &frame));
    }
    if r.get(P::Grain) > 0.0 {
        grain(&mut p, r, frame);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(p: &Pixels) -> Frame {
        Frame::whole(p)
    }

    /// A grey picture with a vertical step edge in the middle.
    fn step(w: u32, h: u32, lo: u8, hi: u8) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|i| if (i % w) < w / 2 { [lo, lo, lo, 255] } else { [hi, hi, hi, 255] }).collect())
    }

    fn noise(w: u32, h: u32, base: u8, spread: i32) -> Pixels {
        let mut seed = 12345u32;
        Pixels::new(
            w,
            h,
            (0..w * h)
                .flat_map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let n = ((seed >> 24) as i32 % (2 * spread + 1)) - spread;
                    let v = (base as i32 + n).clamp(0, 255) as u8;
                    [v, v, v, 255]
                })
                .collect(),
        )
    }

    fn variance(p: &Pixels) -> f32 {
        let v: Vec<f32> = p.rgba.chunks_exact(4).map(|px| px[0] as f32).collect();
        let m = v.iter().sum::<f32>() / v.len() as f32;
        v.iter().map(|x| (x - m) * (x - m)).sum::<f32>() / v.len() as f32
    }

    fn with(f: impl Fn(&mut Recipe)) -> Recipe {
        let mut r = Recipe::default();
        f(&mut r);
        r
    }

    #[test]
    fn an_unchanged_recipe_is_a_no_op() {
        let p = noise(40, 30, 100, 20);
        assert_eq!(apply(p.clone(), &Recipe::default(), frame_of(&p)), p);
    }

    #[test]
    fn sharpen_adds_overshoot_at_an_edge_and_leaves_flat_areas() {
        let p = step(64, 16, 80, 160);
        let out = apply(p.clone(), &with(|r| r.set(P::Sharpen, 100.0)), frame_of(&p));
        let at = |x: usize| out.rgba[(8 * 64 + x) * 4];
        assert!(at(31) < 80 && at(32) > 160, "halo on both sides: {} {}", at(31), at(32));
        assert_eq!(at(2), 80, "flat stays flat");
        assert_eq!(at(61), 160);
    }

    #[test]
    fn clarity_both_ways_and_midtones_first() {
        let p = step(64, 16, 100, 150);
        let up = apply(p.clone(), &with(|r| r.set(P::Clarity, 100.0)), frame_of(&p));
        let down = apply(p.clone(), &with(|r| r.set(P::Clarity, -100.0)), frame_of(&p));
        let range = |q: &Pixels| q.rgba[(8 * 64 + 33) * 4] as i32 - q.rgba[(8 * 64 + 30) * 4] as i32;
        assert!(range(&up) > range(&p), "more local contrast");
        assert!(range(&down) < range(&p), "less local contrast");
        // Whites and blacks hardly move.
        let ends = step(64, 16, 0, 255);
        let o = apply(ends.clone(), &with(|r| r.set(P::Clarity, 100.0)), frame_of(&ends));
        assert!(o.rgba[(8 * 64 + 2) * 4] <= 6 && o.rgba[(8 * 64 + 61) * 4] >= 249);
    }

    #[test]
    fn noise_reduction_lowers_the_variance_but_keeps_an_edge() {
        let p = noise(120, 80, 128, 24);
        let out = apply(p.clone(), &with(|r| r.set(P::NoiseLuma, 70.0)), frame_of(&p));
        assert!(variance(&out) < variance(&p) * 0.5, "{} vs {}", variance(&out), variance(&p));
        // A clean edge survives.
        let e = step(64, 16, 40, 215);
        let o = apply(e.clone(), &with(|r| r.set(P::NoiseLuma, 100.0)), frame_of(&e));
        let (a, b) = (o.rgba[(8 * 64 + 31) * 4] as i32, o.rgba[(8 * 64 + 32) * 4] as i32);
        assert!(b - a > 120, "edge kept: {a} {b}");
    }

    #[test]
    fn colour_noise_reduction_blurs_chroma_and_not_luma() {
        // Alternating red-ish / green-ish pixels of the same brightness: pure colour noise.
        let (w, h) = (64u32, 32u32);
        let p = Pixels::new(w, h, (0..w * h).flat_map(|i| if i % 2 == 0 { [150, 100, 100, 255] } else { [100, 115, 100, 255] }).collect());
        let out = apply(p.clone(), &with(|r| r.set(P::NoiseChroma, 100.0)), frame_of(&p));
        // Away from the borders (which repeat their edge pixel).
        let spread = |q: &Pixels| (8..24u32).flat_map(|y| (14..50u32).map(move |x| (y, x))).map(|(y, x)| q.rgba[((y * 64 + x) * 4) as usize..][..2].to_vec()).map(|px| (px[0] as i32 - px[1] as i32).abs()).max().unwrap();
        assert!(spread(&out) <= 20 && spread(&p) >= 50, "{} vs {}", spread(&out), spread(&p));
    }

    #[test]
    fn bloom_glows_around_highlights_and_film_blur_softens() {
        let (w, h) = (400u32, 400u32);
        let mut v = vec![40u8; (w * h * 4) as usize];
        for px in v.chunks_exact_mut(4) {
            px[3] = 255;
        }
        for y in 180..220 {
            for x in 180..220 {
                let i = ((y * w + x) * 4) as usize;
                v[i..i + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let p = Pixels::new(w, h, v);
        let out = apply(p.clone(), &with(|r| r.set(P::Bloom, 100.0)), frame_of(&p));
        let near = out.rgba[((200 * w + 172) * 4) as usize];
        assert!(near > 60, "light spills outside the highlight: {near}");
        let soft = apply(p.clone(), &with(|r| r.set(P::FilmBlur, 100.0)), frame_of(&p));
        assert!(soft.rgba[((200 * w + 181) * 4) as usize] < 255 && soft.rgba[((200 * w + 176) * 4) as usize] > 40);
    }

    #[test]
    fn grain_is_the_same_everywhere_it_is_computed() {
        let p = Pixels::new(64, 48, (0..64 * 48).flat_map(|_| [128, 128, 128, 255]).collect());
        let r = with(|r| {
            r.set(P::Grain, 80.0);
            r.set(P::GrainRough, 100.0);
        });
        let whole = apply(p.clone(), &r, frame_of(&p));
        assert!(variance(&whole) > 20.0, "grain is visible: {}", variance(&whole));
        // The right half computed as a tile, given its place in the picture, matches the whole.
        let mut half = Vec::new();
        for y in 0..48usize {
            half.extend_from_slice(&p.rgba[(y * 64 + 32) * 4..(y * 64 + 64) * 4]);
        }
        let tile = apply(Pixels::new(32, 48, half), &r, Frame { w: 64, h: 48, x0: 32, y0: 0 });
        for y in 0..48usize {
            assert_eq!(&tile.rgba[y * 32 * 4..(y + 1) * 32 * 4], &whole.rgba[(y * 64 + 32) * 4..(y * 64 + 64) * 4], "row {y}");
        }
        // Mono grain keeps the channels together, colour grain does not.
        assert!(whole.rgba.chunks_exact(4).all(|px| px[0] == px[1] && px[1] == px[2]));
        let c = apply(p.clone(), &with(|r| { r.set(P::Grain, 80.0); r.set(P::GrainColor, 100.0); r.set(P::GrainRough, 100.0); }), frame_of(&p));
        assert!(c.rgba.chunks_exact(4).any(|px| px[0] != px[1]));
    }

    #[test]
    fn skin_smoothing_only_touches_skin_colours() {
        // Noisy skin tone on the left, noisy blue on the right.
        let (w, h) = (80u32, 40u32);
        let mut seed = 7u32;
        let mut v = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let n = ((seed >> 24) as i32 % 21) - 10;
                let (r, g, b) = if x < w / 2 { (200, 150, 125) } else { (60, 80, 200) };
                v.extend_from_slice(&[(r + n) as u8, (g + n) as u8, (b + n) as u8, 255]);
            }
        }
        let p = Pixels::new(w, h, v);
        let out = apply(p.clone(), &with(|r| r.set(P::SkinSmooth, 100.0)), frame_of(&p));
        let var = |q: &Pixels, x0: u32, x1: u32| {
            let vals: Vec<f32> = (0..h).flat_map(|y| (x0..x1).map(move |x| (y, x))).map(|(y, x)| q.rgba[((y * w + x) * 4) as usize] as f32).collect();
            let m = vals.iter().sum::<f32>() / vals.len() as f32;
            vals.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / vals.len() as f32
        };
        assert!(var(&out, 4, 36) < var(&p, 4, 36) * 0.4, "skin got smoother");
        assert_eq!(var(&out, 44, 76), var(&p, 44, 76), "blue untouched");
    }

    #[test]
    fn a_lut_is_applied_at_its_strength() {
        let dir = std::env::temp_dir().join(format!("nv-lut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("invert.cube");
        std::fs::write(&path, "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n").unwrap();
        let p = Pixels::new(2, 2, vec![0, 0, 0, 255, 255, 255, 255, 255, 100, 100, 100, 255, 255, 0, 0, 255]);
        let mut r = Recipe::default();
        r.lut = Some(path.display().to_string());
        let out = apply(p.clone(), &r, frame_of(&p));
        assert_eq!(&out.rgba[..3], &[255, 255, 255]);
        assert_eq!(&out.rgba[4..7], &[0, 0, 0]);
        r.set(P::LutStrength, 50.0);
        let half = apply(p.clone(), &r, frame_of(&p));
        assert!(half.rgba[0].abs_diff(128) <= 1, "{}", half.rgba[0]);
        // A missing file changes nothing.
        r.lut = Some("nowhere.cube".into());
        assert_eq!(apply(p.clone(), &r, frame_of(&p)), p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_follow_the_picture_not_the_buffer() {
        // The same recipe on a picture and on its half-size copy: the radii are fractions of the long side, so the look matches.
        let big = step(400, 40, 80, 160);
        let small = step(200, 20, 80, 160);
        let r = with(|r| r.set(P::Clarity, 100.0));
        let a = apply(big.clone(), &r, frame_of(&big));
        let b = apply(small.clone(), &r, frame_of(&small));
        let bump = |q: &Pixels, w: usize, x: usize| q.rgba[(q.h as usize / 2 * w + x) * 4] as i32;
        // Distance from the edge, in picture widths: 4% of the width on the bright side.
        assert!((bump(&a, 400, 200 + 16) - bump(&b, 200, 100 + 8)).abs() <= 4);
        assert!(margin(&r, &Frame::whole(&big)) > margin(&r, &Frame::whole(&small)));
        assert_eq!(margin(&Recipe::default(), &Frame::whole(&big)), 0);
    }

    #[test]
    #[ignore = "timing, run with --release --ignored --nocapture"]
    fn timing_of_the_neighbourhood_filters_on_a_screen_sized_proxy() {
        let (w, h) = (1920usize, 1080usize);
        let big = Pixels::new(w as u32, h as u32, (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect());
        let frame = Frame::whole(&big);
        let cases: Vec<(&str, Box<dyn Fn(&mut Recipe)>)> = vec![
            ("sharpen", Box::new(|r| r.set(P::Sharpen, 60.0))),
            ("clarity", Box::new(|r| r.set(P::Clarity, 60.0))),
            ("noise reduction (luma + colour)", Box::new(|r| { r.set(P::NoiseLuma, 50.0); r.set(P::NoiseChroma, 50.0); })),
            ("bloom", Box::new(|r| r.set(P::Bloom, 50.0))),
            ("film blur", Box::new(|r| r.set(P::FilmBlur, 50.0))),
            ("grain", Box::new(|r| r.set(P::Grain, 50.0))),
            ("skin smooth", Box::new(|r| r.set(P::SkinSmooth, 50.0))),
            ("all of them", Box::new(|r| { r.set(P::Sharpen, 60.0); r.set(P::Clarity, 40.0); r.set(P::NoiseLuma, 40.0); r.set(P::NoiseChroma, 40.0); r.set(P::Bloom, 30.0); r.set(P::FilmBlur, 20.0); r.set(P::Grain, 30.0); r.set(P::SkinSmooth, 30.0); })),
        ];
        for (name, set) in cases {
            let mut r = Recipe::default();
            set(&mut r);
            let mut best = f64::MAX;
            for _ in 0..5 {
                let t = std::time::Instant::now();
                let _ = apply(big.clone(), &r, frame);
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            println!("{name}: {best:.1} ms for {w}x{h}");
        }
    }
}
