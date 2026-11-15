//! The pixel engine: applies a [`Recipe`] to RGBA8 pixels. The same code serves the live preview, "burn in" saving and
//! (later) batch conversion in NuGallery, so what you see is what is written and there is only one thing to keep correct.
//!
//! **Speed rules** (see docs/nueditor.md):
//! * The preview is rendered from a *proxy* the size of the window, never from the full picture, so a slider costs the
//!   same for a 2 MP file and a 100 MP file. Full resolution is only touched by saving.
//! * Per-pixel work is split into rows over all cores. The steps that depend only on one channel (exposure, white
//!   balance, levels, gamma, contrast, curves, invert) are folded into **one 256-entry table per channel**: a plain slider
//!   move then costs one table read per byte. Steps that mix channels (highlights / shadows, saturation, hue, vibrance,
//!   mid-tone shift, vignette) switch to the general path, which still does no allocation and only table reads + a few
//!   multiplies per pixel.
//! * An unchanged recipe is a plain copy.
//!
//! After the per-pixel stage come the neighbourhood filters (`crate::spatial`: LUT, noise, clarity, sharpen, skin, bloom,
//! film blur, grain). Heal strokes (`crate::retouch`) run first, on the picture's own pixels. Not applied yet (stored in the
//! recipe): nothing; every slider does something.

use crate::raster::{self, Pixels};
use crate::recipe::*;
use std::sync::{Arc, OnceLock};

/// Does the engine do anything with slider `i` (an index into `PARAMS`) yet? The editor greys the others.
pub fn is_implemented(i: usize) -> bool {
    use P::*;
    const DONE: &[P] = &[
        Temp, Tint, ShiftShadowsA, ShiftShadowsB, ShiftHighlightsA, ShiftHighlightsB, Exposure, Brightness, Contrast, Highlights, Midtones, Shadows, Whites, Blacks, Gamma, Saturation, Vibrance, Hue, Vignette, VignetteMid, VignetteFeather,
        Angle, Pitch, Yaw, Zoom, Sharpen, SharpenRadius, Clarity, Bloom, BloomThreshold, BloomRadius, SkinSmooth, LutStrength, NoiseLuma, NoiseChroma, Grain, GrainSize, GrainRough, GrainColor, FilmBlur,
    ];
    DONE.iter().any(|p| *p as usize == i)
}

/// Where a pixel buffer sits inside the whole picture, so position-dependent effects (the vignette) come out the same
/// for a preview, a tile and the full picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Size of the whole picture (after crop and rotation).
    pub w: u32,
    pub h: u32,
    /// Position of this buffer's top-left pixel in the whole picture, in the same units as `w` / `h`.
    pub x0: i32,
    pub y0: i32,
}

impl Frame {
    pub fn whole(p: &Pixels) -> Frame {
        Frame { w: p.w, h: p.h, x0: 0, y0: 0 }
    }
}

// ---------------------------------------------------------------------------------------------------- tables

const TONE_N: usize = 1024;
const ENC_N: usize = 16384;

fn srgb_to_linear_table() -> &'static [f32; 256] {
    static T: OnceLock<[f32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        t
    })
}

fn linear_to_srgb_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        (0..=ENC_N)
            .map(|i| {
                let l = i as f32 / ENC_N as f32;
                if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
            })
            .collect()
    })
}

#[inline]
fn encode_linear(l: f32, t: &[f32]) -> f32 {
    t[(l.clamp(0.0, 1.0) * ENC_N as f32 + 0.5) as usize]
}

#[inline]
fn table_at(t: &[f32], v: f32) -> f32 {
    let p = v.clamp(0.0, 1.0) * TONE_N as f32;
    let i = p as usize;
    if i >= TONE_N {
        return t[TONE_N];
    }
    let f = p - i as f32;
    t[i] * (1.0 - f) + t[i + 1] * f
}

/// Monotone cubic through the points (Fritsch-Carlson), so a curve never overshoots between its handles.
pub fn eval_curve(points: &[(f32, f32)], x: f32) -> f32 {
    let mut pts: Vec<(f32, f32)> = Vec::with_capacity(points.len() + 2);
    if points.first().map(|p| p.0 > 0.0).unwrap_or(true) {
        pts.push((0.0, 0.0));
    }
    pts.extend_from_slice(points);
    if pts.last().map(|p| p.0 < 1.0).unwrap_or(true) {
        pts.push((1.0, 1.0));
    }
    let n = pts.len();
    let x = x.clamp(0.0, 1.0);
    if x <= pts[0].0 {
        return pts[0].1;
    }
    if x >= pts[n - 1].0 {
        return pts[n - 1].1;
    }
    let d: Vec<f32> = (0..n - 1).map(|i| (pts[i + 1].1 - pts[i].1) / (pts[i + 1].0 - pts[i].0).max(1e-6)).collect();
    let mut m = vec![0.0f32; n];
    m[0] = d[0];
    m[n - 1] = d[n - 2];
    for i in 1..n - 1 {
        m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    let k = pts.iter().position(|p| p.0 > x).unwrap_or(n - 1).saturating_sub(1).min(n - 2);
    let h = (pts[k + 1].0 - pts[k].0).max(1e-6);
    let t = (x - pts[k].0) / h;
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * pts[k].1 + (t3 - 2.0 * t2 + t) * h * m[k] + (-2.0 * t3 + 3.0 * t2) * pts[k + 1].1 + (t3 - t2) * h * m[k + 1]
}

// ---------------------------------------------------------------------------------------------------- the plan

struct Vignette {
    amount: f32,
    edge0: f32,
    edge1: f32,
}

/// Everything the per-pixel loop needs, worked out once per recipe.
struct Plan {
    /// Gain per channel in linear light (white balance times exposure).
    gain: Option<[f32; 3]>,
    shadows: f32,
    highlights: f32,
    /// One table per channel: encoded value in, encoded value out (levels, brightness, mid-tones, gamma, contrast, curves).
    tone: Option<Box<[Vec<f32>; 3]>>,
    shift: Option<[f32; 4]>,
    /// Row-major 3x3 for saturation and hue.
    matrix: Option<[f32; 9]>,
    vibrance: f32,
    /// Hue wheel: per band (hue, saturation, luminance) in -1..1, None when untouched.
    bands: Option<[[f32; 3]; 8]>,
    invert: bool,
    vignette: Option<Vignette>,
}

impl Plan {
    fn new(r: &Recipe) -> Plan {
        // White balance: relative temperature (blue <-> amber) and tint (green <-> magenta), or the picked neutral.
        let mut gain = [1.0f32; 3];
        if let Some(c) = r.wb_chart {
            let g = c[1].max(1e-4);
            gain = [g / c[0].max(1e-4), 1.0, g / c[2].max(1e-4)];
        }
        let (temp, tint) = (r.get(P::Temp) / 100.0, r.get(P::Tint) / 100.0);
        gain[0] *= (0.5 * temp).exp2();
        gain[2] *= (-0.5 * temp).exp2();
        gain[1] *= (-0.35 * tint).exp2();
        let ev = r.get(P::Exposure).exp2();
        let gain = gain.map(|g| g * ev);
        let gain = (gain.iter().any(|g| (g - 1.0).abs() > 1e-6)).then_some(gain);

        let (hl, sh) = (r.get(P::Highlights) / 100.0 * 0.35, r.get(P::Shadows) / 100.0 * 0.35);

        let tone = tone_tables(r);

        let s = [P::ShiftShadowsA, P::ShiftShadowsB, P::ShiftHighlightsA, P::ShiftHighlightsB].map(|p| r.get(p) / 100.0);
        let shift = s.iter().any(|v| *v != 0.0).then_some(s);

        // Saturation and hue as one matrix in YIQ: scale and turn the chroma plane, keep luma.
        let sat = 1.0 + r.get(P::Saturation) / 100.0;
        let hue = r.get(P::Hue).to_radians();
        let matrix = (sat != 1.0 || hue != 0.0).then(|| {
            let (c, s) = (hue.cos() * sat, hue.sin() * sat);
            // rgb -> yiq
            let to = [0.299, 0.587, 0.114, 0.596, -0.274, -0.322, 0.211, -0.523, 0.312];
            // yiq -> rgb
            let from = [1.0, 0.956, 0.621, 1.0, -0.272, -0.647, 1.0, -1.106, 1.703];
            let rot = [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c];
            mul3(&from, &mul3(&rot, &to))
        });

        let vig = r.get(P::Vignette) / 100.0;
        let vignette = (vig != 0.0).then(|| {
            let edge0 = r.get(P::VignetteMid) / 100.0 * 0.8;
            Vignette { amount: vig, edge0, edge1: edge0 + 0.05 + r.get(P::VignetteFeather) / 100.0 * 0.9 }
        });

        let bands = r.hue_wheel.iter().any(|b| b.hue != 0.0 || b.sat != 0.0 || b.lum != 0.0).then(|| r.hue_wheel.map(|b| [b.hue / 100.0, b.sat / 100.0, b.lum / 100.0]));
        Plan { gain, shadows: sh, highlights: hl, tone, shift, matrix, vibrance: r.get(P::Vibrance) / 100.0, bands, invert: r.invert, vignette }
    }

    /// True when every step works on one channel at a time: then the whole recipe is three 256-entry tables.
    fn per_channel_only(&self) -> bool {
        self.shadows == 0.0 && self.highlights == 0.0 && self.shift.is_none() && self.matrix.is_none() && self.vibrance == 0.0 && self.bands.is_none() && self.vignette.is_none()
    }
}

fn mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut o = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            o[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    o
}

fn tone_tables(r: &Recipe) -> Option<Box<[Vec<f32>; 3]>> {
    let (blacks, whites) = (r.get(P::Blacks) / 100.0 * 0.2, r.get(P::Whites) / 100.0 * 0.2);
    let (lo, hi) = (-blacks, 1.0 - whites);
    let brightness = r.get(P::Brightness) / 100.0 * 0.3;
    let mids = r.get(P::Midtones) / 100.0 * 0.25;
    let gamma = r.get(P::Gamma);
    let c = r.get(P::Contrast) / 100.0;
    let k = if c >= 0.0 { 1.0 / (1.0 - 0.75 * c) } else { 1.0 + c };
    let plain = blacks == 0.0 && whites == 0.0 && brightness == 0.0 && mids == 0.0 && gamma == 1.0 && c == 0.0;
    if plain && r.curves.is_identity() {
        return None;
    }
    let build = |channel: &Curve| -> Vec<f32> {
        (0..=TONE_N)
            .map(|i| {
                let mut v = i as f32 / TONE_N as f32;
                v = ((v - lo) / (hi - lo).max(0.05)).clamp(0.0, 1.0);
                v = (v + brightness + mids * 4.0 * v * (1.0 - v)).clamp(0.0, 1.0);
                if gamma != 1.0 {
                    v = v.powf(1.0 / gamma);
                }
                if c != 0.0 {
                    v = (0.5 + (v - 0.5) * k).clamp(0.0, 1.0);
                }
                if !r.curves.master.is_identity() && !r.curves.master.points.is_empty() {
                    v = eval_curve(&r.curves.master.points, v);
                }
                if !channel.points.is_empty() {
                    v = eval_curve(&channel.points, v);
                }
                v
            })
            .collect()
    };
    Some(Box::new([build(&r.curves.r), build(&r.curves.g), build(&r.curves.b)]))
}

// ---------------------------------------------------------------------------------------------------- the loops

/// The three tables of the per-channel-only case, 256 entries each, indexed by the input byte.
fn byte_tables(plan: &Plan) -> [[u8; 256]; 3] {
    let s2l = srgb_to_linear_table();
    let enc = linear_to_srgb_table();
    let mut out = [[0u8; 256]; 3];
    for (c, table) in out.iter_mut().enumerate() {
        for (i, o) in table.iter_mut().enumerate() {
            let mut v = match plan.gain {
                Some(g) => encode_linear(s2l[i] * g[c], enc),
                None => i as f32 / 255.0,
            };
            if let Some(t) = &plan.tone {
                v = table_at(&t[c], v);
            }
            if plan.invert {
                v = 1.0 - v;
            }
            *o = (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// Centres of the eight hue-wheel bands in degrees: red, orange, yellow, green, aqua, blue, purple, magenta.
pub const BAND_HUES: [f32; 8] = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0];

/// Weights of the eight bands for a hue in degrees: a hue between two band centres is shared between those two by distance
/// (linear), so the weights always add up to 1 and a hue exactly on a centre belongs to that band alone.
pub fn band_weights(hue: f32) -> [f32; 8] {
    let h = hue.rem_euclid(360.0);
    let mut w = [0.0f32; 8];
    for i in 0..8 {
        let (lo, hi) = (BAND_HUES[i], if i == 7 { 360.0 } else { BAND_HUES[i + 1] });
        if h >= lo && h < hi {
            let t = (h - lo) / (hi - lo);
            w[i] = 1.0 - t;
            w[(i + 1) % 8] += t;
            break;
        }
    }
    w
}

/// The hue wheel on one pixel: shift the hue (up to 30 degrees), scale the saturation and change the brightness of the colours
/// near each band's hue. Greys (no saturation) are not touched.
fn hue_wheel(v: [f32; 3], bands: &[[f32; 3]; 8]) -> [f32; 3] {
    let (mx, mn) = (v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]));
    let c = mx - mn;
    if c < 1e-4 {
        return v;
    }
    let h = if mx == v[0] { ((v[1] - v[2]) / c).rem_euclid(6.0) } else if mx == v[1] { (v[2] - v[0]) / c + 2.0 } else { (v[0] - v[1]) / c + 4.0 } * 60.0;
    let s = c / mx;
    let w = band_weights(h);
    let (mut dh, mut ds, mut dl) = (0.0, 0.0, 0.0);
    for k in 0..8 {
        dh += w[k] * bands[k][0];
        ds += w[k] * bands[k][1];
        dl += w[k] * bands[k][2];
    }
    if dh == 0.0 && ds == 0.0 && dl == 0.0 {
        return v;
    }
    let h2 = (h + dh * 30.0).rem_euclid(360.0);
    let s2 = (s * (1.0 + ds)).clamp(0.0, 1.0);
    let v2 = (mx * (1.0 + dl * 0.5)).clamp(0.0, 1.0);
    // HSV -> RGB
    let c2 = v2 * s2;
    let hp = h2 / 60.0;
    let x = c2 * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c2, x, 0.0),
        1 => (x, c2, 0.0),
        2 => (0.0, c2, x),
        3 => (0.0, x, c2),
        4 => (x, 0.0, c2),
        _ => (c2, 0.0, x),
    };
    let m = v2 - c2;
    [r + m, g + m, b + m]
}

fn luma(v: [f32; 3]) -> f32 {
    0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
}

/// The general per-pixel path for one band of rows. `y_first` is the picture row of the band's first row.
fn general_rows(plan: &Plan, frame: &Frame, y_first: usize, w: usize, src: &[u8], dst: &mut [u8]) {
    let s2l = srgb_to_linear_table();
    let enc = linear_to_srgb_table();
    let (fw, fh) = (frame.w.max(1) as f32, frame.h.max(1) as f32);
    for (row, (s, d)) in src.chunks_exact(w * 4).zip(dst.chunks_exact_mut(w * 4)).enumerate() {
        let ny = ((y_first + row) as f32 + frame.y0 as f32 + 0.5) / fh * 2.0 - 1.0;
        for (x, (sp, dp)) in s.chunks_exact(4).zip(d.chunks_exact_mut(4)).enumerate() {
            let mut v = match plan.gain {
                Some(g) => [encode_linear(s2l[sp[0] as usize] * g[0], enc), encode_linear(s2l[sp[1] as usize] * g[1], enc), encode_linear(s2l[sp[2] as usize] * g[2], enc)],
                None => [sp[0] as f32 / 255.0, sp[1] as f32 / 255.0, sp[2] as f32 / 255.0],
            };
            if plan.shadows != 0.0 || plan.highlights != 0.0 {
                let y = luma(v);
                let delta = plan.shadows * (1.0 - y) * (1.0 - y) + plan.highlights * y * y;
                v = [(v[0] + delta).clamp(0.0, 1.0), (v[1] + delta).clamp(0.0, 1.0), (v[2] + delta).clamp(0.0, 1.0)];
            }
            if let Some(t) = &plan.tone {
                v = [table_at(&t[0], v[0]), table_at(&t[1], v[1]), table_at(&t[2], v[2])];
            }
            if let Some([sa, sb, ha, hb]) = plan.shift {
                // Split-tone on four axes: the shadows weigh most in the dark, the highlights in the light (luma mask).
                // a: green (-) <-> magenta (+), b: blue (-) <-> yellow (+).
                let y = luma(v);
                let (ws, wh) = ((1.0 - y) * (1.0 - y) * 0.15, y * y * 0.15);
                let a = sa * ws + ha * wh;
                let b = sb * ws + hb * wh;
                v = [(v[0] + a + b).clamp(0.0, 1.0), (v[1] - a + b * 0.5).clamp(0.0, 1.0), (v[2] + a - b).clamp(0.0, 1.0)];
            }
            if let Some(m) = &plan.matrix {
                v = [
                    (m[0] * v[0] + m[1] * v[1] + m[2] * v[2]).clamp(0.0, 1.0),
                    (m[3] * v[0] + m[4] * v[1] + m[5] * v[2]).clamp(0.0, 1.0),
                    (m[6] * v[0] + m[7] * v[1] + m[8] * v[2]).clamp(0.0, 1.0),
                ];
            }
            if plan.vibrance != 0.0 {
                // Lift weakly coloured pixels more than strongly coloured ones (and the reverse for a negative amount).
                let sat = v[0].max(v[1]).max(v[2]) - v[0].min(v[1]).min(v[2]);
                let k = 1.0 + plan.vibrance * (1.0 - sat);
                let y = 0.299 * v[0] + 0.587 * v[1] + 0.114 * v[2];
                v = [(y + (v[0] - y) * k).clamp(0.0, 1.0), (y + (v[1] - y) * k).clamp(0.0, 1.0), (y + (v[2] - y) * k).clamp(0.0, 1.0)];
            }
            if let Some(b) = &plan.bands {
                v = hue_wheel(v, b);
            }
            if plan.invert {
                v = [1.0 - v[0], 1.0 - v[1], 1.0 - v[2]];
            }
            if let Some(g) = &plan.vignette {
                let nx = ((x as f32) + frame.x0 as f32 + 0.5) / fw * 2.0 - 1.0;
                let dist = ((nx * nx + ny * ny) * 0.5).sqrt();
                let t = ((dist - g.edge0) / (g.edge1 - g.edge0)).clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                if g.amount < 0.0 {
                    let f = 1.0 + g.amount * t;
                    v = [v[0] * f, v[1] * f, v[2] * f];
                } else {
                    let f = g.amount * t;
                    v = [v[0] + (1.0 - v[0]) * f, v[1] + (1.0 - v[1]) * f, v[2] + (1.0 - v[2]) * f];
                }
            }
            dp[0] = (v[0] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dp[1] = (v[1] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dp[2] = (v[2] * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            dp[3] = sp[3];
        }
    }
}

fn table_rows(tables: &[[u8; 256]; 3], src: &[u8], dst: &mut [u8]) {
    for (sp, dp) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
        dp[0] = tables[0][sp[0] as usize];
        dp[1] = tables[1][sp[1] as usize];
        dp[2] = tables[2][sp[2] as usize];
        dp[3] = sp[3];
    }
}

fn threads_for(rows: usize, row_bytes: usize) -> usize {
    // Don't start threads for a few kilobytes of work.
    let by_size = (rows * row_bytes) / (256 * 1024);
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(by_size.max(1)).min(rows.max(1))
}

/// The per-pixel stage (white balance, light, colour, vignette) of `recipe` on `src`, which sits at `frame` in the whole
/// picture. Alpha is kept.
pub fn apply_pixels(src: &Pixels, recipe: &Recipe, frame: Frame) -> Pixels {
    let out = apply_pixels_with(src, recipe, frame, false);
    crate::spatial::apply(out, recipe, frame)
}

fn apply_pixels_with(src: &Pixels, recipe: &Recipe, frame: Frame, force_general: bool) -> Pixels {
    if recipe.pixel_identity() {
        return src.clone();
    }
    let plan = Plan::new(recipe);
    let (w, h) = (src.w as usize, src.h as usize);
    let mut out = vec![0u8; src.rgba.len()];
    if w == 0 || h == 0 {
        return Pixels { w: src.w, h: src.h, rgba: out };
    }
    let tables = (!force_general && plan.per_channel_only()).then(|| byte_tables(&plan));
    let n = threads_for(h, w * 4);
    let band = h.div_ceil(n);
    std::thread::scope(|scope| {
        for (i, (s, d)) in src.rgba.chunks(band * w * 4).zip(out.chunks_mut(band * w * 4)).enumerate() {
            let (plan, tables) = (&plan, &tables);
            let mut run = move || match tables {
                Some(t) => table_rows(t, s, d),
                None => general_rows(plan, &frame, i * band, w, s, d),
            };
            if n == 1 {
                run();
            } else {
                scope.spawn(run);
            }
        }
    });
    Pixels { w: src.w, h: src.h, rgba: out }
}

/// Turn, flip and crop (the recipe's composition that is applied to pixels).
pub fn apply_geometry(src: &Pixels, recipe: &Recipe) -> Pixels {
    if recipe.geometry_identity() {
        return src.clone();
    }
    let angle = recipe.get(P::Angle) + 90.0 * (recipe.quarter_turns % 4) as f32;
    let mut cur = if raster::normalize_degrees(angle) == 0.0 { src.clone() } else { raster::rotate(src, angle) };
    if recipe.flip_h {
        cur = raster::flip_horizontal(&cur);
    }
    if recipe.flip_v {
        cur = raster::flip_vertical(&cur);
    }
    if !recipe.perspective_identity() {
        cur = raster::perspective(&cur, recipe.get(P::Pitch), recipe.get(P::Yaw), recipe.get(P::Zoom));
    }
    if let Some(c) = recipe.crop {
        let (w, h) = (cur.w as f32, cur.h as f32);
        let x0 = ((c.x * w).round() as u32).min(cur.w.saturating_sub(1));
        let y0 = ((c.y * h).round() as u32).min(cur.h.saturating_sub(1));
        let cw = ((c.w * w).round() as u32).clamp(1, cur.w - x0);
        let ch = ((c.h * h).round() as u32).clamp(1, cur.h - y0);
        let mut rgba = Vec::with_capacity(cw as usize * ch as usize * 4);
        for y in y0..y0 + ch {
            let start = (y as usize * cur.w as usize + x0 as usize) * 4;
            rgba.extend_from_slice(&cur.rgba[start..start + cw as usize * 4]);
        }
        cur = Pixels { w: cw, h: ch, rgba };
    }
    cur
}

/// Where a point of the picture as it is shown (turned, flipped, cropped; 0..1 across and down) is in the original
/// picture of `w` x `h` pixels, as 0..1 of the original. Points in the corners that the turn left empty land outside 0..1.
pub fn display_to_source(r: &Recipe, w: u32, h: u32, nx: f32, ny: f32) -> (f32, f32) {
    let angle = r.get(P::Angle) + 90.0 * (r.quarter_turns % 4) as f32;
    let (rw, rh) = raster::rotated_size(w, h, angle);
    let (rw, rh) = (rw as f32, rh as f32);
    let c = r.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
    let (mut cx, mut cy) = ((c.x + nx * c.w) * rw, (c.y + ny * c.h) * rh);
    if r.flip_h {
        cx = rw - cx;
    }
    if r.flip_v {
        cy = rh - cy;
    }
    let a = angle.to_radians();
    let (dx, dy) = (cx - rw / 2.0, cy - rh / 2.0);
    let (sx, sy) = (dx * a.cos() + dy * a.sin() + w as f32 / 2.0, -dx * a.sin() + dy * a.cos() + h as f32 / 2.0);
    (sx / w as f32, sy / h as f32)
}

/// The other way: where a point of the original (0..1) is in the picture as it is shown (0..1 of the shown picture).
pub fn source_to_display(r: &Recipe, w: u32, h: u32, sx: f32, sy: f32) -> (f32, f32) {
    let angle = r.get(P::Angle) + 90.0 * (r.quarter_turns % 4) as f32;
    let (rw, rh) = raster::rotated_size(w, h, angle);
    let (rw, rh) = (rw as f32, rh as f32);
    let a = angle.to_radians();
    let (px, py) = (sx * w as f32 - w as f32 / 2.0, sy * h as f32 - h as f32 / 2.0);
    let (mut cx, mut cy) = (px * a.cos() - py * a.sin() + rw / 2.0, px * a.sin() + py * a.cos() + rh / 2.0);
    if r.flip_h {
        cx = rw - cx;
    }
    if r.flip_v {
        cy = rh - cy;
    }
    let c = r.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
    (((cx / rw) - c.x) / c.w, ((cy / rh) - c.y) / c.h)
}

/// The whole recipe on a whole picture: heal strokes on the picture's own pixels, then the geometry, then the per-pixel and
/// neighbourhood stages on the result.
pub fn render(src: &Pixels, recipe: &Recipe) -> Pixels {
    let retouched;
    let src = if recipe.retouch.is_empty() {
        src
    } else {
        retouched = crate::retouch::apply(src, &recipe.retouch);
        &retouched
    };
    let g = apply_geometry(src, recipe);
    let frame = Frame::whole(&g);
    apply_pixels(&g, recipe, frame)
}

/// Keeps the picture after the heal strokes between two renders, so moving a slider does not redo the strokes.
#[derive(Default)]
pub struct RetouchCache {
    strokes: Vec<Retouch>,
    src_gen: u64,
    out: Option<Arc<Pixels>>,
}

impl RetouchCache {
    /// `src` after `strokes`. `src_gen` names the source: change it whenever `src` is not the same picture any more.
    pub fn get(&mut self, src: &Arc<Pixels>, src_gen: u64, strokes: &[Retouch]) -> Arc<Pixels> {
        if strokes.is_empty() {
            return src.clone();
        }
        if let Some(out) = &self.out {
            if self.src_gen == src_gen && self.strokes == strokes {
                return out.clone();
            }
        }
        let out = Arc::new(crate::retouch::apply(src, strokes));
        self.strokes = strokes.to_vec();
        self.src_gen = src_gen;
        self.out = Some(out.clone());
        out
    }
}

/// The preview: `src` is the proxy (or, with `geo_done`, a picture that already has its heal strokes, turn and crop).
pub fn render_proxy(src: &Arc<Pixels>, src_gen: u64, recipe: &Recipe, geo_done: bool, cache: &mut RetouchCache) -> Pixels {
    if geo_done {
        return apply_pixels(src, recipe, Frame::whole(src));
    }
    let base = cache.get(src, src_gen, &recipe.retouch);
    let g = apply_geometry(&base, recipe);
    apply_pixels(&g, recipe, Frame::whole(&g))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{EditStack, Op};

    fn solid(w: u32, h: u32, rgb: [u8; 3]) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect())
    }

    fn ramp() -> Pixels {
        // 256 x 4: every grey level, plus some colour rows.
        let mut v = Vec::new();
        for y in 0..4u32 {
            for x in 0..256u32 {
                let g = x as u8;
                v.extend_from_slice(&match y {
                    0 => [g, g, g, 255],
                    1 => [g, 255 - g, 128, 255],
                    2 => [255 - g, g / 2, g, 200],
                    _ => [g / 3, g, 255 - g / 2, 255],
                });
            }
        }
        Pixels::new(256, 4, v)
    }

    #[test]
    fn an_unchanged_recipe_returns_the_same_pixels() {
        let p = ramp();
        assert_eq!(render(&p, &Recipe::default()), p);
    }

    #[test]
    fn exposure_follows_linear_light() {
        let mut r = Recipe::default();
        r.set(P::Exposure, 1.0);
        let out = render(&solid(2, 2, [128, 128, 128]), &r);
        // 128 is 0.2158 in linear light; twice that is 0.4316, which is 0.686 encoded, about 175.
        assert!((173..=178).contains(&out.rgba[0]), "{}", out.rgba[0]);
        r.set(P::Exposure, -1.0);
        let out = render(&solid(2, 2, [128, 128, 128]), &r);
        assert!((90..=94).contains(&out.rgba[0]), "{}", out.rgba[0]);
        // Alpha is untouched.
        assert_eq!(render(&ramp(), &r).rgba[3 + 4 * 256 * 2], 200);
    }

    #[test]
    fn invert_contrast_and_gamma() {
        let mut r = Recipe::default();
        r.invert = true;
        let out = render(&solid(1, 1, [0, 100, 255]), &r);
        assert_eq!(&out.rgba[..3], &[255, 155, 0]);

        let mut r = Recipe::default();
        r.set(P::Contrast, 50.0);
        let p = ramp();
        let out = render(&p, &r);
        assert_eq!(out.rgba[128 * 4], 128, "mid grey stays");
        assert!(out.rgba[192 * 4] > 192 && out.rgba[64 * 4] < 64, "light gets lighter, dark darker");

        let mut r = Recipe::default();
        r.set(P::Gamma, 2.0);
        assert!(render(&p, &r).rgba[64 * 4] > 64);
    }

    #[test]
    fn white_balance_moves_red_against_blue() {
        let mut r = Recipe::default();
        r.set(P::Temp, 60.0);
        let out = render(&solid(1, 1, [128, 128, 128]), &r);
        assert!(out.rgba[0] > 128 && out.rgba[2] < 128 && out.rgba[1].abs_diff(128) <= 1, "{:?}", &out.rgba[..3]);
        // A picked neutral patch becomes neutral.
        let mut r = Recipe::default();
        r.wb_chart = Some([0.30, 0.20, 0.10]);
        let out = render(&solid(1, 1, [149, 124, 89]), &r);
        let spread = out.rgba[..3].iter().max().unwrap() - out.rgba[..3].iter().min().unwrap();
        assert!(spread <= 8, "{:?}", &out.rgba[..3]);
    }

    #[test]
    fn the_four_shifts_tint_shadows_and_highlights_separately() {
        let dark = solid(1, 1, [40, 40, 40]);
        let light = solid(1, 1, [220, 220, 220]);
        // Shadows towards magenta: the dark grey gets red + blue over green, the light grey hardly changes.
        let mut r = Recipe::default();
        r.set(P::ShiftShadowsA, 100.0);
        let d = render(&dark, &r);
        assert!(d.rgba[0] > d.rgba[1] + 8 && d.rgba[2] > d.rgba[1] + 8, "{:?}", &d.rgba[..3]);
        assert!(render(&light, &r).rgba[..3].iter().all(|&c| c.abs_diff(220) <= 3));
        // Highlights towards yellow: the light grey loses blue, the dark one hardly changes.
        let mut r = Recipe::default();
        r.set(P::ShiftHighlightsB, 100.0);
        let l = render(&light, &r);
        assert!(l.rgba[2] < l.rgba[0] - 8, "{:?}", &l.rgba[..3]);
        assert!(render(&dark, &r).rgba[..3].iter().all(|&c| c.abs_diff(40) <= 3));
        // Each axis on its own, in both directions.
        for p in [P::ShiftShadowsA, P::ShiftShadowsB, P::ShiftHighlightsA, P::ShiftHighlightsB] {
            let (mut up, mut down) = (Recipe::default(), Recipe::default());
            up.set(p, 100.0);
            down.set(p, -100.0);
            let mid = solid(1, 1, [128, 128, 128]);
            assert_ne!(render(&mid, &up).rgba, render(&mid, &down).rgba, "{p:?}");
        }
    }

    #[test]
    fn the_hue_wheel_moves_only_the_colours_near_a_band() {
        use crate::recipe::Band;
        let green = solid(1, 1, [60, 200, 60]);
        let red = solid(1, 1, [200, 60, 60]);
        let grey = solid(1, 1, [120, 120, 120]);
        let mut r = Recipe::default();
        r.hue_wheel[3] = Band { hue: 0.0, sat: -100.0, lum: 0.0 }; // green: no saturation
        let g = render(&green, &r);
        let spread = g.rgba[..3].iter().max().unwrap() - g.rgba[..3].iter().min().unwrap();
        assert!(spread < 20, "green was drained: {:?}", &g.rgba[..3]);
        assert_eq!(render(&red, &r).rgba, red.rgba, "red is out of reach of the green band");
        assert_eq!(render(&grey, &r).rgba, grey.rgba, "grey has no hue to move");
        // Hue shift of the red band turns red towards orange / yellow (green rises).
        let mut r = Recipe::default();
        r.hue_wheel[0] = Band { hue: 100.0, sat: 0.0, lum: 0.0 };
        let o = render(&red, &r);
        assert!(o.rgba[1] > 100, "{:?}", &o.rgba[..3]);
        // Brightness of the blue band.
        let blue = solid(1, 1, [60, 60, 200]);
        let mut r = Recipe::default();
        r.hue_wheel[5] = Band { hue: 0.0, sat: 0.0, lum: 100.0 };
        assert!(render(&blue, &r).rgba[2] > 230);
    }

    #[test]
    fn band_weights_blend_neighbours_and_never_exceed_one() {
        for h in (0..360).step_by(5) {
            let w = band_weights(h as f32);
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-4, "hue {h}");
        }
        assert_eq!(band_weights(120.0)[3], 1.0 - 0.0, "centre of green");
        let mid = band_weights(90.0);
        assert!(mid[2] > 0.3 && mid[3] > 0.3, "between yellow and green both count: {mid:?}");
    }

    #[test]
    fn saturation_to_zero_is_grey_and_hue_keeps_grey() {
        let mut r = Recipe::default();
        r.set(P::Saturation, -100.0);
        let out = render(&solid(1, 1, [200, 80, 40]), &r);
        assert!(out.rgba[0].abs_diff(out.rgba[1]) <= 1 && out.rgba[1].abs_diff(out.rgba[2]) <= 1, "{:?}", &out.rgba[..3]);
        let mut r = Recipe::default();
        r.set(P::Hue, 120.0);
        let out = render(&solid(1, 1, [100, 100, 100]), &r);
        assert!(out.rgba[..3].iter().all(|&c| c.abs_diff(100) <= 2), "{:?}", &out.rgba[..3]);
        let out = render(&solid(1, 1, [255, 0, 0]), &r);
        assert!(out.rgba[1] > out.rgba[0], "red turned 120 degrees is no longer red: {:?}", &out.rgba[..3]);
    }

    #[test]
    fn highlights_and_shadows_work_on_their_own_end() {
        let p = solid(1, 2, [40, 40, 40]);
        let mut r = Recipe::default();
        r.set(P::Shadows, 100.0);
        assert!(render(&p, &r).rgba[0] > 55);
        let bright = solid(1, 1, [230, 230, 230]);
        assert!(render(&bright, &r).rgba[0] <= 232, "shadows leave the highlights alone");
        r.set(P::Shadows, 0.0);
        r.set(P::Highlights, -100.0);
        assert!(render(&bright, &r).rgba[0] < 215);
        assert!(render(&p, &r).rgba[0].abs_diff(40) <= 3, "highlights leave the shadows alone");
    }

    #[test]
    fn curve_moves_the_midpoint_and_keeps_the_ends() {
        let mut r = Recipe::default();
        r.curves.master.set_point(0.5, 0.7);
        let out = render(&ramp(), &r);
        assert!(out.rgba[128 * 4] >= 176 && out.rgba[128 * 4] <= 182, "{}", out.rgba[128 * 4]);
        assert_eq!(out.rgba[0], 0);
        assert_eq!(out.rgba[255 * 4], 255);
        // Monotone: never goes down along the ramp.
        assert!((1..256).all(|i| out.rgba[i * 4] >= out.rgba[(i - 1) * 4]));
    }

    #[test]
    fn vignette_darkens_the_corners_only_and_tiles_match_the_whole() {
        let p = solid(64, 48, [200, 200, 200]);
        let mut r = Recipe::default();
        r.set(P::Vignette, -80.0);
        let out = render(&p, &r);
        let at = |x: usize, y: usize| out.rgba[(y * 64 + x) * 4];
        assert_eq!(at(32, 24), 200);
        assert!(at(0, 0) < 140, "{}", at(0, 0));
        // The right half rendered as a tile with its frame position equals the right half of the whole.
        let mut half = Vec::new();
        for y in 0..48usize {
            half.extend_from_slice(&p.rgba[(y * 64 + 32) * 4..(y * 64 + 64) * 4]);
        }
        let tile = apply_pixels(&Pixels::new(32, 48, half), &r, Frame { w: 64, h: 48, x0: 32, y0: 0 });
        for y in 0..48usize {
            assert_eq!(&tile.rgba[y * 32 * 4..(y + 1) * 32 * 4], &out.rgba[(y * 64 + 32) * 4..(y * 64 + 64) * 4], "row {y}");
        }
        r.set(P::Vignette, 80.0);
        assert!(render(&p, &r).rgba[0] > 230, "a positive vignette lightens the corners");
    }

    #[test]
    fn the_table_path_and_the_general_path_agree() {
        let p = ramp();
        let mut r = Recipe::default();
        r.set(P::Exposure, 0.7);
        r.set(P::Temp, -30.0);
        r.set(P::Contrast, 25.0);
        r.set(P::Gamma, 1.3);
        r.set(P::Blacks, 30.0);
        r.set(P::Whites, -20.0);
        r.invert = true;
        r.curves.g.set_point(0.4, 0.5);
        let f = Frame::whole(&p);
        let fast = apply_pixels_with(&p, &r, f, false);
        let slow = apply_pixels_with(&p, &r, f, true);
        let worst = fast.rgba.iter().zip(&slow.rgba).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "the two paths differ by {worst}");
    }

    #[test]
    fn threaded_and_single_band_results_are_identical() {
        // A big enough picture to be split over several threads.
        let mut v = Vec::new();
        for i in 0..(1200 * 900) {
            v.extend_from_slice(&[(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255]);
        }
        let big = Pixels::new(1200, 900, v);
        let mut r = Recipe::default();
        r.set(P::Saturation, 30.0);
        r.set(P::Vignette, -50.0);
        let a = render(&big, &r);
        let f = Frame::whole(&big);
        let plan = Plan::new(&r);
        let mut one = vec![0u8; big.rgba.len()];
        general_rows(&plan, &f, 0, 1200, &big.rgba, &mut one);
        assert_eq!(a.rgba, one);
    }

    #[test]
    fn geometry_matches_the_viewers_edit_stack() {
        let p = Pixels::new(3, 2, vec![1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255]);
        for ops in [
            vec![Op::Rotate(90.0)],
            vec![Op::Rotate(270.0), Op::Rotate(90.0)],
            vec![Op::FlipH],
            vec![Op::FlipV],
            vec![Op::FlipH, Op::Rotate(90.0)],
            vec![Op::Rotate(90.0), Op::FlipH],
            vec![Op::FlipV, Op::Rotate(90.0), Op::FlipH],
            vec![Op::Rotate(180.0), Op::FlipV],
        ] {
            let mut s = EditStack::new();
            for op in &ops {
                s.push(*op);
            }
            let expected = s.render(&p);
            let got = apply_geometry(&p, &Recipe::from_ops(&ops));
            assert_eq!(got, expected, "ops {ops:?}");
        }
    }

    #[test]
    fn points_map_between_the_shown_picture_and_the_original() {
        // A picture with one bright pixel; after turn / flip / crop the bright pixel is found where the mapping says.
        let (w, h) = (120u32, 80u32);
        let (bx, by) = (90u32, 20u32);
        let mut v = vec![0u8; (w * h * 4) as usize];
        v[((by * w + bx) * 4) as usize..][..4].copy_from_slice(&[255, 255, 255, 255]);
        let p = Pixels::new(w, h, v);
        let cases: Vec<Box<dyn Fn(&mut Recipe)>> = vec![
            Box::new(|_| {}),
            Box::new(|r| r.set(P::Angle, 90.0)),
            Box::new(|r| r.quarter_turns = 3),
            Box::new(|r| r.flip_h = true),
            Box::new(|r| {
                r.flip_v = true;
                r.set(P::Angle, 180.0);
            }),
            Box::new(|r| {
                r.set(P::Angle, 30.0);
                r.flip_h = true;
            }),
            Box::new(|r| {
                r.set(P::Angle, -17.0);
                r.crop = Some(Crop { x: 0.2, y: 0.1, w: 0.7, h: 0.8 });
            }),
        ];
        for (i, f) in cases.iter().enumerate() {
            let mut r = Recipe::default();
            f(&mut r);
            let out = apply_geometry(&p, &r);
            // The brightest pixel of the output.
            let (mut best, mut at) = (0u32, (0u32, 0u32));
            for y in 0..out.h {
                for x in 0..out.w {
                    let v = out.rgba[((y * out.w + x) * 4) as usize] as u32 * out.rgba[((y * out.w + x) * 4 + 3) as usize] as u32;
                    if v > best {
                        best = v;
                        at = (x, y);
                    }
                }
            }
            let (nx, ny) = ((at.0 as f32 + 0.5) / out.w as f32, (at.1 as f32 + 0.5) / out.h as f32);
            let (sx, sy) = display_to_source(&r, w, h, nx, ny);
            assert!((sx * w as f32 - bx as f32 - 0.5).abs() < 1.6 && (sy * h as f32 - by as f32 - 0.5).abs() < 1.6, "case {i}: back at ({:.1}, {:.1})", sx * w as f32, sy * h as f32);
            let (dx, dy) = source_to_display(&r, w, h, (bx as f32 + 0.5) / w as f32, (by as f32 + 0.5) / h as f32);
            assert!((dx - nx).abs() * out.w as f32 <= 1.6 && (dy - ny).abs() * out.h as f32 <= 1.6, "case {i}: forward ({dx:.3}, {dy:.3}) vs ({nx:.3}, {ny:.3})");
        }
    }

    #[test]
    fn pitch_yaw_and_zoom_move_the_picture_the_way_a_tipped_sheet_would() {
        let p = solid(100, 60, [200, 100, 50]);
        let mut r = Recipe::default();
        assert_eq!(apply_geometry(&p, &r), p);
        // Tipping the top away makes the top edge shorter: the top corners become empty, the bottom ones stay.
        r.set(P::Pitch, 60.0);
        let a = apply_geometry(&p, &r);
        let alpha = |q: &Pixels, x: usize, y: usize| q.rgba[(y * q.w as usize + x) * 4 + 3];
        let (top_left, bottom_left) = (alpha(&a, 0, 0), alpha(&a, 0, 59));
        assert!(top_left != bottom_left, "the two ends differ: {top_left} {bottom_left}");
        // The other direction swaps them.
        r.set(P::Pitch, -60.0);
        let b = apply_geometry(&p, &r);
        assert_eq!((alpha(&b, 0, 0), alpha(&b, 0, 59)), (bottom_left, top_left));
        // Yaw does the same left to right.
        r.set(P::Pitch, 0.0);
        r.set(P::Yaw, 60.0);
        let c = apply_geometry(&p, &r);
        assert_ne!(alpha(&c, 0, 30), alpha(&c, 99, 30));
        // The middle stays on the middle.
        assert_eq!(&c.rgba[((30 * 100 + 50) * 4) as usize..][..3], &[200, 100, 50]);
        // Zoom enlarges: a mark just beside the centre ends up further out.
        let mut m = vec![0u8; 100 * 60 * 4];
        for px in m.chunks_exact_mut(4) {
            px[3] = 255;
        }
        m[((30 * 100 + 55) * 4) as usize..][..3].copy_from_slice(&[255, 255, 255]);
        let mark = Pixels::new(100, 60, m);
        let mut r = Recipe::default();
        r.set(P::Zoom, 200.0);
        let z = apply_geometry(&mark, &r);
        let at = (0..100).find(|&x| z.rgba[((30 * 100 + x) * 4) as usize] > 100).unwrap();
        assert!((59..=61).contains(&at), "5 px from the centre becomes about 10: {at}");
    }

    #[test]
    fn crop_takes_the_asked_part() {
        let p = Pixels::new(4, 2, (0..8).flat_map(|i| [i as u8, 0, 0, 255]).collect());
        let mut r = Recipe::default();
        r.crop = Some(Crop { x: 0.5, y: 0.0, w: 0.5, h: 1.0 });
        let out = apply_geometry(&p, &r);
        assert_eq!((out.w, out.h), (2, 2));
        assert_eq!(out.rgba.iter().step_by(4).copied().collect::<Vec<_>>(), vec![2, 3, 6, 7]);
    }

    #[test]
    #[ignore = "timing, run with --ignored --nocapture"]
    fn timing_of_a_screen_sized_proxy() {
        let (w, h) = (1920usize, 1080usize);
        let big = Pixels::new(w as u32, h as u32, (0..w * h * 4).map(|i| (i * 7 % 251) as u8).collect());
        for (name, setup) in [
            ("exposure + contrast (tables)", Box::new(|r: &mut Recipe| { r.set(P::Exposure, 0.5); r.set(P::Contrast, 20.0); }) as Box<dyn Fn(&mut Recipe)>),
            ("saturation + shadows (general)", Box::new(|r: &mut Recipe| { r.set(P::Saturation, 20.0); r.set(P::Shadows, 30.0); })),
            ("everything", Box::new(|r: &mut Recipe| { r.set(P::Exposure, 0.5); r.set(P::Saturation, 20.0); r.set(P::Vibrance, 30.0); r.set(P::Shadows, 30.0); r.set(P::Vignette, -30.0); })),
        ] {
            let mut r = Recipe::default();
            setup(&mut r);
            let mut best = f64::MAX;
            for _ in 0..8 {
                let t = std::time::Instant::now();
                let _ = render(&big, &r);
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            println!("{name}: {best:.2} ms for {w}x{h}");
        }
    }
}
