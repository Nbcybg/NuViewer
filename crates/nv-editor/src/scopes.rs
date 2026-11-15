//! The measuring displays of the editor, computed from RGBA8 pixels (the edited proxy) and drawn into small bitmaps that the
//! program puts on the picture: histogram, RGB parade, vector scope (with the skin-tone line) and false colour. No window and
//! no GPU, so they are unit-tested here. Every function looks at no more than ~1.5 million pixels (rows are skipped on bigger
//! buffers), so a scope costs about a millisecond or two next to the edit itself.

use crate::raster::Pixels;

/// At most this many pixels are looked at.
const MAX_SAMPLES: usize = 1_500_000;

/// Row step so that `w x h` pixels give no more than `MAX_SAMPLES` samples.
fn row_step(w: u32, h: u32) -> usize {
    ((w as usize * h as usize).div_ceil(MAX_SAMPLES)).max(1)
}

const BACK: [u8; 4] = [16, 18, 22, 205];

fn bitmap(w: usize, h: usize) -> Pixels {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        rgba.extend_from_slice(&BACK);
    }
    Pixels::new(w as u32, h as u32, rgba)
}

fn put(p: &mut Pixels, x: i64, y: i64, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= p.w as i64 || y >= p.h as i64 {
        return;
    }
    let i = (y as usize * p.w as usize + x as usize) * 4;
    p.rgba[i..i + 4].copy_from_slice(&c);
}

/// Add light to a pixel (so overlapping channels turn into yellow, cyan, white ...).
fn add(p: &mut Pixels, x: usize, y: usize, c: [u8; 3]) {
    let i = (y * p.w as usize + x) * 4;
    for k in 0..3 {
        p.rgba[i + k] = p.rgba[i + k].saturating_add(c[k]);
    }
    p.rgba[i + 3] = 255;
}

fn line(p: &mut Pixels, x0: i64, y0: i64, x1: i64, y1: i64, c: [u8; 4]) {
    let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=n {
        put(p, x0 + (x1 - x0) * i / n, y0 + (y1 - y0) * i / n, c);
    }
}

// ---------------------------------------------------------------------------------------------------- histogram

#[derive(Clone, Debug, PartialEq)]
pub struct Hist {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub luma: [u32; 256],
    pub samples: u64,
}

pub fn histogram(p: &Pixels) -> Hist {
    let mut h = Hist { r: [0; 256], g: [0; 256], b: [0; 256], luma: [0; 256], samples: 0 };
    let step = row_step(p.w, p.h);
    for y in (0..p.h as usize).step_by(step) {
        for px in p.rgba[y * p.w as usize * 4..(y + 1) * p.w as usize * 4].chunks_exact(4) {
            h.r[px[0] as usize] += 1;
            h.g[px[1] as usize] += 1;
            h.b[px[2] as usize] += 1;
            let l = (px[0] as u32 * 54 + px[1] as u32 * 183 + px[2] as u32 * 19) >> 8;
            h.luma[l.min(255) as usize] += 1;
            h.samples += 1;
        }
    }
    h
}

impl Hist {
    /// Share (0..1) of the samples in the lowest / highest bin of any channel: how much is clipped.
    pub fn clipped(&self) -> (f32, f32) {
        let n = self.samples.max(1) as f32;
        let lo = self.r[0].max(self.g[0]).max(self.b[0]) as f32 / n;
        let hi = self.r[255].max(self.g[255]).max(self.b[255]) as f32 / n;
        (lo, hi)
    }
}

/// The histogram as a `w x h` bitmap: R, G, B added together, the luma as a white outline, red marks at the ends when something
/// is clipped. The scale ignores the two end bins (they are often huge) so the rest stays readable.
pub fn histogram_bitmap(hist: &Hist, w: usize, h: usize) -> Pixels {
    let mut out = bitmap(w, h);
    for q in 1..4 {
        let x = (w * q / 4) as i64;
        line(&mut out, x, 0, x, h as i64 - 1, [60, 64, 72, 255]);
    }
    let max = (1..255).map(|i| hist.r[i].max(hist.g[i]).max(hist.b[i]).max(hist.luma[i])).max().unwrap_or(1).max(1) as f32;
    let height = |v: u32| (((v as f32 / max).min(1.0).sqrt()) * (h as f32 - 2.0)) as usize;
    let mut prev: Option<(i64, i64)> = None;
    for x in 0..w {
        let bin = (x * 256 / w).min(255);
        for (arr, c) in [(&hist.r, [150u8, 0, 0]), (&hist.g, [0, 130, 0]), (&hist.b, [0, 20, 170])] {
            for yy in 0..height(arr[bin]) {
                add(&mut out, x, h - 1 - yy, c);
            }
        }
        let ly = h as i64 - 1 - height(hist.luma[bin]) as i64;
        if let Some((px, py)) = prev {
            line(&mut out, px, py, x as i64, ly, [235, 235, 235, 255]);
        }
        prev = Some((x as i64, ly));
    }
    let (lo, hi) = hist.clipped();
    if lo > 0.001 {
        for y in 0..h.min(6) {
            put(&mut out, 0, y as i64, [255, 60, 60, 255]);
            put(&mut out, 1, y as i64, [255, 60, 60, 255]);
        }
    }
    if hi > 0.001 {
        for y in 0..h.min(6) {
            put(&mut out, w as i64 - 1, y as i64, [255, 60, 60, 255]);
            put(&mut out, w as i64 - 2, y as i64, [255, 60, 60, 255]);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------- parade

/// RGB parade: three panels side by side (R, G, B), each showing, for every column of the picture, how the values of that
/// column are spread (bright where many pixels share a level). Low values at the bottom.
pub fn parade_bitmap(p: &Pixels, w: usize, h: usize) -> Pixels {
    let mut out = bitmap(w, h);
    let pw = (w / 3).max(1);
    let (lw, lh) = (pw, h);
    let mut cells = vec![[0u32; 3]; lw * lh];
    let step = row_step(p.w, p.h);
    for y in (0..p.h as usize).step_by(step) {
        let row = &p.rgba[y * p.w as usize * 4..(y + 1) * p.w as usize * 4];
        for (x, px) in row.chunks_exact(4).enumerate() {
            let cx = (x * lw / p.w.max(1) as usize).min(lw - 1);
            for c in 0..3 {
                let level = (255 - px[c] as usize) * (lh - 1) / 255;
                cells[level * lw + cx][c] += 1;
            }
        }
    }
    let max = cells.iter().flat_map(|c| c.iter()).copied().max().unwrap_or(1).max(1) as f32;
    let tint = [[255u8, 70, 70], [70, 255, 90], [90, 130, 255]];
    for c in 0..3 {
        for y in 0..lh {
            for x in 0..lw {
                let n = cells[y * lw + x][c];
                if n > 0 {
                    let a = ((1.0 + n as f32).ln() / (1.0 + max).ln()).clamp(0.0, 1.0);
                    let a = 0.25 + 0.75 * a;
                    add(&mut out, c * pw + x, y, [(tint[c][0] as f32 * a) as u8, (tint[c][1] as f32 * a) as u8, (tint[c][2] as f32 * a) as u8]);
                }
            }
        }
    }
    for q in 1..4 {
        let y = (h * q / 4) as i64;
        line(&mut out, 0, y, w as i64 - 1, y, [60, 64, 72, 255]);
    }
    for c in 1..3 {
        line(&mut out, (c * pw) as i64, 0, (c * pw) as i64, h as i64 - 1, [90, 96, 108, 255]);
    }
    out
}

// ---------------------------------------------------------------------------------------------------- vector scope

/// Chroma (Cb, Cr) of an RGB colour, each -0.5..0.5 for 0..1 inputs.
pub fn chroma(r: f32, g: f32, b: f32) -> (f32, f32) {
    (-0.168_736 * r - 0.331_264 * g + 0.5 * b, 0.5 * r - 0.418_688 * g - 0.081_312 * b)
}

/// Direction of the skin-tone line in the (Cb, Cr) plane, in radians from +Cb counter-clockwise (up is +Cr): the mean direction
/// of skin colours from light to dark (Fitzpatrick scale). Real skin, whatever the light or the tan, sits near this line.
pub fn skin_angle() -> f32 {
    let samples = [(255.0, 224.0, 189.0), (234.0, 192.0, 134.0), (224.0, 172.0, 105.0), (198.0, 134.0, 66.0), (141.0, 85.0, 36.0), (92.0, 56.0, 28.0)];
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    for (r, g, b) in samples {
        let (cb, cr) = chroma(r / 255.0, g / 255.0, b / 255.0);
        let len = (cb * cb + cr * cr).sqrt().max(1e-6);
        sx += cb / len;
        sy += cr / len;
    }
    sy.atan2(sx)
}

/// A square vector scope of `size` pixels: where the picture's colours lie around the centre (grey), the six primaries /
/// secondaries marked, and the skin-tone line. Faces should end up on that line.
pub fn vectorscope_bitmap(p: &Pixels, size: usize) -> Pixels {
    let mut out = bitmap(size, size);
    let c = size as f32 / 2.0;
    let scale = (c - 3.0) / 0.5;
    // Graticule.
    for (rad, col) in [(0.5f32, [70u8, 74, 84, 255]), (0.25, [50, 54, 62, 255])] {
        let r = rad * scale;
        for i in 0..360 {
            let a = (i as f32).to_radians();
            put(&mut out, (c + r * a.cos()) as i64, (c - r * a.sin()) as i64, col);
        }
    }
    line(&mut out, c as i64, 2, c as i64, size as i64 - 3, [50, 54, 62, 255]);
    line(&mut out, 2, c as i64, size as i64 - 3, c as i64, [50, 54, 62, 255]);
    // Skin-tone line.
    let a = skin_angle();
    line(&mut out, c as i64, c as i64, (c + (c - 3.0) * a.cos()) as i64, (c - (c - 3.0) * a.sin()) as i64, [255, 170, 60, 255]);
    // The colours of the picture.
    let mut bins = vec![0u32; size * size];
    let step = row_step(p.w, p.h);
    for y in (0..p.h as usize).step_by(step) {
        for px in p.rgba[y * p.w as usize * 4..(y + 1) * p.w as usize * 4].chunks_exact(4) {
            let (cb, cr) = chroma(px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0);
            let (x, yy) = ((c + cb * scale) as i64, (c - cr * scale) as i64);
            if x >= 0 && yy >= 0 && (x as usize) < size && (yy as usize) < size {
                bins[yy as usize * size + x as usize] += 1;
            }
        }
    }
    let max = bins.iter().copied().max().unwrap_or(1).max(1) as f32;
    for y in 0..size {
        for x in 0..size {
            let n = bins[y * size + x];
            if n > 0 {
                let a = 0.3 + 0.7 * ((1.0 + n as f32).ln() / (1.0 + max).ln());
                let v = (230.0 * a) as u8;
                let i = (y * size + x) * 4;
                out.rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
    }
    // Marks for R, Y, G, C, B, M at full saturation.
    for (r, g, b, col) in [(1.0, 0.0, 0.0, [255u8, 60, 60, 255]), (1.0, 1.0, 0.0, [255, 255, 60, 255]), (0.0, 1.0, 0.0, [60, 255, 60, 255]), (0.0, 1.0, 1.0, [60, 255, 255, 255]), (0.0, 0.0, 1.0, [90, 120, 255, 255]), (1.0, 0.0, 1.0, [255, 60, 255, 255])] {
        let (cb, cr) = chroma(r, g, b);
        let (x, y) = ((c + cb * scale) as i64, (c - cr * scale) as i64);
        for d in -2..=2 {
            put(&mut out, x + d, y - 2, col);
            put(&mut out, x + d, y + 2, col);
            put(&mut out, x - 2, y + d, col);
            put(&mut out, x + 2, y + d, col);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------------- false colour

/// Colour for a luma value 0..1 (exposure bands the way cine cameras show them): purple = crushed, blue / teal = shadows,
/// green = middle grey, pink = skin in the light, yellow / orange / red = about to clip. Other values keep their grey.
pub fn false_colour_of(luma: f32, grey: u8) -> [u8; 3] {
    let l = luma * 100.0;
    match l {
        l if l < 2.0 => [120, 0, 160],
        l if l < 8.0 => [20, 20, 230],
        l if l < 16.0 => [0, 140, 170],
        l if l >= 44.0 && l < 56.0 => [40, 170, 60],
        l if l >= 68.0 && l < 78.0 => [235, 140, 160],
        l if l >= 92.0 && l < 97.0 => [250, 230, 40],
        l if l >= 97.0 && l < 99.5 => [250, 140, 20],
        l if l >= 99.5 => [240, 20, 20],
        _ => [grey, grey, grey],
    }
}

/// The picture in false colour (a display transform; the picture is not changed).
pub fn false_colour(p: &Pixels) -> Pixels {
    let mut out = Vec::with_capacity(p.rgba.len());
    for px in p.rgba.chunks_exact(4) {
        let l = (px[0] as f32 * 0.2126 + px[1] as f32 * 0.7152 + px[2] as f32 * 0.0722) / 255.0;
        let grey = (l * 150.0 + 20.0) as u8; // dimmed, so the colours stand out
        let c = false_colour_of(l, grey);
        out.extend_from_slice(&[c[0], c[1], c[2], px[3]]);
    }
    Pixels::new(p.w, p.h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect())
    }

    #[test]
    fn histogram_counts_each_channel() {
        let h = histogram(&solid(10, 10, [10, 200, 255]));
        assert_eq!((h.r[10], h.g[200], h.b[255], h.samples), (100, 100, 100, 100));
        assert_eq!(h.r.iter().sum::<u32>(), 100);
        let (lo, hi) = h.clipped();
        assert_eq!((lo, hi), (0.0, 1.0));
    }

    #[test]
    fn big_pictures_are_sampled_not_read_whole() {
        let h = histogram(&solid(4000, 3000, [1, 2, 3]));
        assert!(h.samples <= 3_000_000 && h.samples >= 1_000_000, "{}", h.samples);
    }

    #[test]
    fn bitmaps_have_the_asked_size_and_draw_something() {
        let p = solid(64, 64, [200, 100, 50]);
        let bg = bitmap(120, 60);
        let hb = histogram_bitmap(&histogram(&p), 120, 60);
        assert_eq!((hb.w, hb.h), (120, 60));
        assert_ne!(hb, bg, "the histogram drew nothing");
        let pb = parade_bitmap(&p, 120, 60);
        assert_eq!((pb.w, pb.h), (120, 60));
        assert_ne!(pb, bg);
        let vb = vectorscope_bitmap(&p, 100);
        assert_eq!((vb.w, vb.h), (100, 100));
    }

    #[test]
    fn parade_puts_the_levels_in_the_right_panel_and_height() {
        // Pure red at full level: only the first panel, at the top.
        let pb = parade_bitmap(&solid(30, 30, [255, 0, 0]), 90, 40);
        let at = |x: usize, y: usize| pb.rgba[(y * 90 + x) * 4..(y * 90 + x) * 4 + 3].to_vec();
        assert!(at(10, 0)[0] > 150, "{:?}", at(10, 0));
        assert!(at(40, 0)[1] < 60 && at(70, 0)[2] < 60, "green and blue panels stay empty at the top");
        assert!(at(40, 39)[1] > 100, "green is zero: bottom of its panel");
    }

    #[test]
    fn vector_scope_places_red_left_of_and_above_grey() {
        let size = 128;
        let grey = vectorscope_bitmap(&solid(8, 8, [128, 128, 128]), size);
        let red = vectorscope_bitmap(&solid(8, 8, [220, 40, 40]), size);
        // Brightest pixel of the picture's own colours (white-ish grey values, not the coloured marks).
        let spot = |p: &Pixels| {
            let mut best = (0u8, 0usize, 0usize);
            for y in 0..size {
                for x in 0..size {
                    let i = (y * size + x) * 4;
                    let (r, g, b) = (p.rgba[i], p.rgba[i + 1], p.rgba[i + 2]);
                    if r == g && g == b && r > best.0 && r != 255 && r > 120 {
                        best = (r, x, y);
                    }
                }
            }
            (best.1, best.2)
        };
        let g = spot(&grey);
        assert!((g.0 as i32 - 64).abs() <= 2 && (g.1 as i32 - 64).abs() <= 2, "grey sits in the middle: {g:?}");
        let r = spot(&red);
        assert!(r.0 < 64 && r.1 < 64, "red is up and to the left of the centre: {r:?}");
    }

    #[test]
    fn the_skin_line_points_up_and_left() {
        let a = skin_angle().to_degrees();
        assert!((100.0..=150.0).contains(&a), "{a}");
        // Light skin and dark skin both lie within 20 degrees of it.
        for rgb in [(255.0, 224.0, 189.0), (141.0, 85.0, 36.0)] {
            let (cb, cr) = chroma(rgb.0 / 255.0, rgb.1 / 255.0, rgb.2 / 255.0);
            let d = (cr.atan2(cb).to_degrees() - a).abs();
            assert!(d < 20.0, "{rgb:?} is {d} degrees off");
        }
    }

    #[test]
    fn false_colour_bands() {
        let band = |v: u8| false_colour(&solid(1, 1, [v, v, v])).rgba[..3].to_vec();
        assert_eq!(band(0), vec![120, 0, 160], "crushed");
        assert_eq!(band(128), vec![40, 170, 60], "middle grey is green");
        assert_eq!(band(255), vec![240, 20, 20], "clipped is red");
        let g = band(64);
        assert!(g[0] == g[1] && g[1] == g[2], "unmarked values stay grey: {g:?}");
    }
}
