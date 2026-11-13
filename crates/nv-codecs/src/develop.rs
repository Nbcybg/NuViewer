//! Development of camera RAW sensor data: the main way RAW files are shown.
//!
//! An embedded preview is the camera's own rendering (its tone curve, sharpening, picture style): not what the sensor
//! recorded. So the picture is developed from the sensor data, and **only from what the camera wrote into the file**:
//!
//!  * black and white level of the sensor (per colour site),
//!  * the as-shot white balance (the camera's own multipliers),
//!  * the camera's colour matrix (DNG-style XYZ -> camera, for the two illuminants the camera gives, interpolated at the
//!    colour temperature of the as-shot white balance when there are two),
//!  * the sRGB transfer curve.
//!
//! Nothing is added: no automatic exposure, no tone curve, no highlight roll-off, no saturation or sharpening. A channel
//! that clipped on the sensor stays clipped. (The first version of this module brightened the picture automatically and
//! rolled highlights off; that was an embellishment and is gone.)
//!
//! `rawler` decodes the file (all the vendor compression schemes). The demosaic is ours so that it runs on every core:
//! Malvar-He-Cutler for Bayer sensors, a 3x3 same-colour average for X-Trans. Two sizes: `Full` (every photosite becomes
//! a pixel) and `Half` (2x2 Bayer blocks / 3x3 X-Trans blocks are binned, no demosaic; ~4x / ~9x fewer pixels, much faster).

use crate::Image;
use rawler::decoders::RawDecodeParams;
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::RawImageData;
use rawler::rawsource::RawSource;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

/// How camera RAW files are shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RawMode {
    /// Developed from the sensor data, every photosite a pixel.
    Full,
    /// Developed from the sensor data at reduced size (binned): faster.
    Half,
    /// The camera's embedded JPEG preview (fast; it is the camera's rendering, not the sensor data).
    Preview,
}

impl RawMode {
    pub const ALL: [RawMode; 3] = [RawMode::Full, RawMode::Half, RawMode::Preview];

    pub fn id(self) -> &'static str {
        match self {
            RawMode::Full => "full",
            RawMode::Half => "half",
            RawMode::Preview => "preview",
        }
    }

    pub fn from_id(v: &str) -> Option<RawMode> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "full" | "develop" | "develop-full" => RawMode::Full,
            "half" | "develop-half" => RawMode::Half,
            "preview" | "embedded" => RawMode::Preview,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            RawMode::Full => "developed from the sensor data, full resolution",
            RawMode::Half => "developed from the sensor data, half size (faster)",
            RawMode::Preview => "the camera's embedded preview (fastest)",
        }
    }
}

static MODE: AtomicU8 = AtomicU8::new(0);
static THUMBS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Gallery thumbnails of RAW files are developed from the sensor data too (the whole file is read, so a folder of RAWs
/// on a slow disk takes much longer); off (the default) = from the embedded preview, which is read with a partial read.
pub fn set_raw_thumbs_develop(on: bool) {
    THUMBS.store(on, Ordering::Relaxed);
}

pub(crate) fn raw_thumbs_develop() -> bool {
    THUMBS.load(Ordering::Relaxed)
}

/// Choose how RAW files are shown (affects files decoded afterwards).
pub fn set_raw_mode(m: RawMode) {
    MODE.store(m as u8, Ordering::Relaxed);
}

/// The mode in force: the setting, unless the environment variable `NUVIEWER_RAW` (full / half / preview) overrides it
/// (handy for timing and comparing).
pub fn raw_mode() -> RawMode {
    if let Some(m) = std::env::var("NUVIEWER_RAW").ok().and_then(|v| RawMode::from_id(&v)) {
        return m;
    }
    if std::env::var_os("NUVIEWER_NO_DEVELOP").is_some() {
        return RawMode::Preview; // the old switch
    }
    match MODE.load(Ordering::Relaxed) {
        1 => RawMode::Half,
        2 => RawMode::Preview,
        _ => RawMode::Full,
    }
}

/// sRGB (D65) -> XYZ.
const SRGB_TO_XYZ: [[f32; 3]; 3] = [[0.412_456_4, 0.357_576_1, 0.180_437_5], [0.212_672_9, 0.715_152_2, 0.072_175_0], [0.019_333_9, 0.119_192_0, 0.950_304_1]];

pub(crate) type M3 = [[f32; 3]; 3];
const IDENTITY: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn mul(a: &M3, b: &M3) -> M3 {
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

fn invert(m: &M3) -> Option<M3> {
    let c = |i: usize, j: usize| m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3] - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3];
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    if !det.is_finite() || det.abs() < 1e-6 {
        return None;
    }
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = c(j, i) / det; // adjugate = transposed cofactors
        }
    }
    Some(o)
}

/// Camera RGB (white balanced, 0..1) -> linear sRGB, from the DNG-style XYZ -> camera matrix. Rows of the
/// sRGB -> camera matrix are normalised so that white stays white (the same recipe as dcraw).
pub(crate) fn camera_to_srgb(xyz_to_cam: &[[f32; 3]]) -> M3 {
    if xyz_to_cam.len() < 3 {
        return IDENTITY;
    }
    let x2c: M3 = [xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2]];
    let mut rgb_to_cam = mul(&x2c, &SRGB_TO_XYZ);
    for row in rgb_to_cam.iter_mut() {
        let s: f32 = row.iter().sum();
        if !s.is_finite() || s.abs() < 1e-6 {
            return IDENTITY;
        }
        row.iter_mut().for_each(|v| *v /= s);
    }
    invert(&rgb_to_cam).unwrap_or(IDENTITY)
}

/// Correlated colour temperature (K) of a chromaticity (McCamy's approximation).
fn cct_of_xy(x: f32, y: f32) -> f32 {
    let n = (x - 0.3320) / (0.1858 - y);
    (449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33).clamp(1500.0, 20000.0)
}

fn flat3(m: &[f32]) -> Option<M3> {
    (m.len() >= 9 && m[..9].iter().all(|v| v.is_finite())).then(|| [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]])
}

/// The XYZ -> camera matrix to use, and a line saying where it came from. Deterministic (the matrices come in a hash
/// map; the first one found must not depend on its order). With a tungsten-ish matrix (A) and a daylight one (D65) the
/// result is interpolated, in reciprocal temperature, at the colour temperature of the as-shot white balance, solved the
/// way the DNG specification describes (the temperature depends on the matrix and the matrix on the temperature).
pub(crate) fn pick_matrix(matrices: &[(Illuminant, Vec<f32>)], neutral: [f32; 3]) -> Option<(M3, String)> {
    let get = |names: &[Illuminant]| names.iter().find_map(|i| matrices.iter().find(|(k, _)| k == i).and_then(|(_, m)| flat3(m)));
    let low = get(&[Illuminant::A, Illuminant::Tungsten, Illuminant::IsoStudioTungsten]);
    let high = get(&[Illuminant::D65, Illuminant::Daylight, Illuminant::FineWeather, Illuminant::D75, Illuminant::CloudyWeather, Illuminant::D55, Illuminant::D50]);
    match (low, high) {
        (Some(a), Some(d)) => {
            let (mired_a, mired_d) = (1e6 / 2856.0f32, 1e6 / 6504.0f32);
            let blend = |w: f32| -> M3 { std::array::from_fn(|i| std::array::from_fn(|j| w * a[i][j] + (1.0 - w) * d[i][j])) };
            let (mut xy, mut cct) = ([0.3457f32, 0.3585], 5000.0f32);
            for _ in 0..30 {
                let w = ((1e6 / cct - mired_d) / (mired_a - mired_d)).clamp(0.0, 1.0);
                let Some(inv) = invert(&blend(w)) else { break };
                let xyz: [f32; 3] = std::array::from_fn(|i| inv[i][0] * neutral[0] + inv[i][1] * neutral[1] + inv[i][2] * neutral[2]);
                let sum = xyz[0] + xyz[1] + xyz[2];
                if !sum.is_finite() || sum <= 0.0 {
                    break;
                }
                let next = [xyz[0] / sum, xyz[1] / sum];
                let t = cct_of_xy(next[0], next[1]);
                let done = (t - cct).abs() < 1.0;
                (xy, cct) = (next, t);
                if done {
                    break;
                }
            }
            let _ = xy;
            let w = ((1e6 / cct - mired_d) / (mired_a - mired_d)).clamp(0.0, 1.0);
            Some((blend(w), format!("camera colour matrix interpolated for {:.0} K", cct)))
        }
        (Some(m), None) | (None, Some(m)) => Some((m, "camera colour matrix".to_string())),
        (None, None) => matrices.iter().filter_map(|(i, m)| flat3(m).map(|m| (*i as u16, m))).min_by_key(|(i, _)| *i).map(|(_, m)| (m, "camera colour matrix".to_string())),
    }
}

/// The colour filter array: 2x2 Bayer or 6x6 X-Trans, as colours 0 (R), 1 (G), 2 (B).
#[derive(Clone, Debug)]
pub(crate) struct Cfa {
    pub w: usize,
    pub h: usize,
    pub pat: Vec<u8>,
}

impl Cfa {
    #[cfg(test)]
    pub fn bayer(pattern: [u8; 4]) -> Cfa {
        Cfa { w: 2, h: 2, pat: pattern.to_vec() }
    }

    #[inline(always)]
    fn at(&self, y: usize, x: usize) -> usize {
        self.pat[(y % self.h) * self.w + (x % self.w)] as usize
    }

    fn is_bayer(&self) -> bool {
        self.w == 2 && self.h == 2
    }

    fn valid(&self) -> bool {
        (self.is_bayer() || (self.w == 6 && self.h == 6)) && self.pat.len() == self.w * self.h && self.pat.iter().all(|&c| c <= 2) && (0..3).all(|c| self.pat.contains(&(c as u8)))
    }
}

/// Black level per site (a repeating tile).
#[derive(Clone, Debug)]
pub(crate) struct Black {
    pub levels: Vec<f32>,
    pub w: usize,
    pub h: usize,
}

impl Black {
    pub fn flat(v: f32) -> Black {
        Black { levels: vec![v], w: 1, h: 1 }
    }

    #[inline(always)]
    fn at(&self, y: usize, x: usize) -> f32 {
        self.levels[(y % self.h) * self.w + (x % self.w)]
    }
}

/// Everything the camera wrote that development needs, plus the sensor data.
pub(crate) struct Mosaic<'a> {
    pub data: &'a [u16],
    pub width: usize,
    pub height: usize,
    pub cfa: Cfa,
    pub black: Black,
    /// White (saturation) level per colour R, G, B.
    pub white: [f32; 3],
    /// As-shot white balance multipliers R, G, B (G = 1).
    pub wb: [f32; 3],
    /// White-balanced camera RGB -> linear sRGB.
    pub cam_to_srgb: M3,
    /// x, y, w, h of the area to develop.
    pub crop: (usize, usize, usize, usize),
}

/// Linear 0..1 -> sRGB byte, through a lookup table.
fn gamma_table() -> Vec<u8> {
    (0..4096)
        .map(|i| {
            let l = i as f32 / 4095.0;
            let s = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Camera RGB (0..1 of the sensor's range) -> white balance, clip, matrix -> sRGB bytes. This is all the "look" there is.
struct Finish {
    wb: [f32; 3],
    m: M3,
    lut: Vec<u8>,
}

impl Finish {
    fn new(m: &Mosaic) -> Finish {
        Finish { wb: m.wb, m: m.cam_to_srgb, lut: gamma_table() }
    }

    #[inline(always)]
    fn px(&self, cam: [f32; 3], out: &mut [u8]) {
        // A channel that clipped on the sensor is at its maximum: after the multipliers it is clipped to the range again.
        let c = [(cam[0] * self.wb[0]).clamp(0.0, 1.0), (cam[1] * self.wb[1]).clamp(0.0, 1.0), (cam[2] * self.wb[2]).clamp(0.0, 1.0)];
        for k in 0..3 {
            let lin = self.m[k][0] * c[0] + self.m[k][1] * c[1] + self.m[k][2] * c[2];
            out[k] = self.lut[(lin.clamp(0.0, 1.0) * 4095.0 + 0.5) as usize];
        }
        out[3] = 255;
    }
}

fn threads_for(rows: usize) -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8).min(rows.max(1))
}

/// Split `rgba` (rows of `row_len` bytes) over threads; `f(first_row, rows)` fills a band.
fn par_rows(rgba: &mut [u8], row_len: usize, f: impl Fn(usize, &mut [u8]) + Sync) {
    let rows = rgba.len() / row_len.max(1);
    let per = rows.div_ceil(threads_for(rows)).max(1);
    let f = &f;
    std::thread::scope(|s| {
        for (t, band) in rgba.chunks_mut(per * row_len).enumerate() {
            s.spawn(move || f(t * per, band));
        }
    });
}

/// Half size: every 2x2 (Bayer) or 3x3 (X-Trans) block becomes one pixel, averaging the photosites of each colour. No
/// demosaic.
pub(crate) fn develop_half(m: &Mosaic) -> (usize, usize, Vec<u8>) {
    let b = if m.cfa.is_bayer() { 2 } else { 3 };
    let (cx, cy, cw, ch) = m.crop;
    let (ow, oh) = (cw / b, ch / b);
    let fin = Finish::new(m);
    let mut rgba = vec![255u8; ow * oh * 4];
    par_rows(&mut rgba, ow * 4, |first, band| {
        for (r, line) in band.chunks_mut(ow * 4).enumerate() {
            let y0 = cy + (first + r) * b;
            for (x, px) in line.chunks_exact_mut(4).enumerate() {
                let x0 = cx + x * b;
                let (mut sum, mut n) = ([0.0f32; 3], [0.0f32; 3]);
                for dy in 0..b {
                    let row = &m.data[(y0 + dy) * m.width + x0..(y0 + dy) * m.width + x0 + b];
                    for (dx, &v) in row.iter().enumerate() {
                        let c = m.cfa.at(y0 + dy, x0 + dx);
                        sum[c] += ((v as f32 - m.black.at(y0 + dy, x0 + dx)) / (m.white[c] - m.black.at(y0 + dy, x0 + dx)).max(1.0)).max(0.0);
                        n[c] += 1.0;
                    }
                }
                fin.px([sum[0] / n[0].max(1.0), sum[1] / n[1].max(1.0), sum[2] / n[2].max(1.0)], px);
            }
        }
    });
    (ow, oh, rgba)
}

/// For X-Trans: for each of the 36 phases and each colour, where the nearest photosites of that colour are (a 3x3 window,
/// widened to 5x5 where it has none).
fn xtrans_taps(cfa: &Cfa) -> Vec<[Vec<(isize, isize)>; 3]> {
    let mut all = Vec::with_capacity(36);
    for py in 0..6 {
        for px in 0..6 {
            let mut per: [Vec<(isize, isize)>; 3] = Default::default();
            for c in 0..3 {
                for r in [1isize, 2] {
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if cfa.at((py as isize + dy + 60) as usize, (px as isize + dx + 60) as usize) == c {
                                per[c].push((dx, dy));
                            }
                        }
                    }
                    if !per[c].is_empty() {
                        break;
                    }
                }
            }
            all.push(per);
        }
    }
    all
}

/// Every photosite becomes a pixel.
pub(crate) fn develop_full(m: &Mosaic) -> (usize, usize, Vec<u8>) {
    const MARGIN: usize = 3;
    // The demosaic filter needs MARGIN photosites around every pixel it makes: at the very edge of the sensor, where there
    // are none, the picture is MARGIN pixels smaller (the crop the camera gives is inside the sensor, so normally not).
    let (cx, cy) = (m.crop.0.max(MARGIN), m.crop.1.max(MARGIN));
    let cw = (m.crop.0 + m.crop.2).min(m.width.saturating_sub(MARGIN)).saturating_sub(cx);
    let ch = (m.crop.1 + m.crop.3).min(m.height.saturating_sub(MARGIN)).saturating_sub(cy);
    if cw == 0 || ch == 0 {
        return (0, 0, Vec::new());
    }
    // The part of the sensor the demosaic reads: the crop and a margin for its filter, normalised to 0..1 per site.
    let (rx0, ry0) = (cx.saturating_sub(MARGIN), cy.saturating_sub(MARGIN));
    let (rx1, ry1) = ((cx + cw + MARGIN).min(m.width), (cy + ch + MARGIN).min(m.height));
    let (rw, rh) = (rx1 - rx0, ry1 - ry0);
    let mut buf = vec![0f32; rw * rh];
    {
        let per = rh.div_ceil(threads_for(rh)).max(1);
        std::thread::scope(|s| {
            for (t, band) in buf.chunks_mut(per * rw).enumerate() {
                s.spawn(move || {
                    for (r, line) in band.chunks_mut(rw).enumerate() {
                        let y = ry0 + t * per + r;
                        let src = &m.data[y * m.width + rx0..y * m.width + rx1];
                        for (i, (d, &v)) in line.iter_mut().zip(src).enumerate() {
                            let x = rx0 + i;
                            let c = m.cfa.at(y, x);
                            let bl = m.black.at(y, x);
                            *d = ((v as f32 - bl) / (m.white[c] - bl).max(1.0)).max(0.0);
                        }
                    }
                });
            }
        });
    }
    let fin = Finish::new(m);
    let mut rgba = vec![255u8; cw * ch * 4];
    // Every pixel made has MARGIN photosites around it inside the buffer (the crop was pulled in from the sensor edge to
    // guarantee that), so the taps need no clamping.
    let at = |x: isize, y: isize| -> f32 { buf[(y as usize) * rw + x as usize] };
    if m.cfa.is_bayer() {
        par_rows(&mut rgba, cw * 4, |first, band| {
            for (r, line) in band.chunks_mut(cw * 4).enumerate() {
                let y = cy + first + r;
                let ly = (y - ry0) as isize;
                for (i, px) in line.chunks_exact_mut(4).enumerate() {
                    let x = cx + i;
                    let lx = (x - rx0) as isize;
                    fin.px(mhc(&at, lx, ly, m.cfa.at(y, x), m.cfa.at(y, x + 1) == 0), px);
                }
            }
        });
    } else {
        let taps = xtrans_taps(&m.cfa);
        par_rows(&mut rgba, cw * 4, |first, band| {
            for (r, line) in band.chunks_mut(cw * 4).enumerate() {
                let y = cy + first + r;
                let ly = (y - ry0) as isize;
                for (i, px) in line.chunks_exact_mut(4).enumerate() {
                    let x = cx + i;
                    let lx = (x - rx0) as isize;
                    let own = m.cfa.at(y, x);
                    let t = &taps[(y % 6) * 6 + x % 6];
                    let mut cam = [0f32; 3];
                    for c in 0..3 {
                        cam[c] = if c == own {
                            at(lx, ly)
                        } else {
                            let list = &t[c];
                            list.iter().map(|&(dx, dy)| at(lx + dx, ly + dy)).sum::<f32>() / list.len().max(1) as f32
                        };
                    }
                    fin.px(cam, px);
                }
            }
        });
    }
    (cw, ch, rgba)
}

/// Malvar-He-Cutler (2004) 5x5 gradient-corrected linear interpolation. `c` is the colour of the photosite at (x, y);
/// for a green one `h_is_red` says that its left and right neighbours are red (so its up / down neighbours are blue).
#[inline(always)]
fn mhc(at: &impl Fn(isize, isize) -> f32, x: isize, y: isize, c: usize, h_is_red: bool) -> [f32; 3] {
    let s = |dx: isize, dy: isize| at(x + dx, y + dy);
    let centre = s(0, 0);
    // Green at a red or blue site.
    let g_at_rb = || (4.0 * centre + 2.0 * (s(0, -1) + s(0, 1) + s(-1, 0) + s(1, 0)) - (s(0, -2) + s(0, 2) + s(-2, 0) + s(2, 0))) * 0.125;
    // Red or blue at the other of the two (diagonal neighbours).
    let diag = || (6.0 * centre + 2.0 * (s(-1, -1) + s(1, -1) + s(-1, 1) + s(1, 1)) - 1.5 * (s(0, -2) + s(0, 2) + s(-2, 0) + s(2, 0))) * 0.125;
    // Red / blue at a green site whose red / blue neighbours are left and right, or up and down.
    let horizontal = || (5.0 * centre + 4.0 * (s(-1, 0) + s(1, 0)) - (s(-2, 0) + s(2, 0)) - (s(-1, -1) + s(1, -1) + s(-1, 1) + s(1, 1)) + 0.5 * (s(0, -2) + s(0, 2))) * 0.125;
    let vertical = || (5.0 * centre + 4.0 * (s(0, -1) + s(0, 1)) - (s(0, -2) + s(0, 2)) - (s(-1, -1) + s(1, -1) + s(-1, 1) + s(1, 1)) + 0.5 * (s(-2, 0) + s(2, 0))) * 0.125;
    let rgb = match c {
        0 => [centre, g_at_rb(), diag()],
        2 => [diag(), g_at_rb(), centre],
        _ if h_is_red => [horizontal(), centre, vertical()],
        _ => [vertical(), centre, horizontal()],
    };
    rgb.map(|v| v.max(0.0))
}

/// Decode `path` and develop it. Errors (unsupported layout, rawler failure) mean "use the preview".
pub fn develop(path: &Path, mode: RawMode) -> Result<Image, String> {
    let t_all = Instant::now();
    // Read the file ourselves (the loading panel shows the progress, a cancelled load stops at the next chunk, the disk
    // gating of the loader applies) and let rawler decode from memory.
    let bytes = crate::progress::read_file(path)?;
    let src = RawSource::new_from_shared_vec(std::sync::Arc::new(bytes)).with_path(path);
    let dec = rawler::get_decoder(&src).map_err(|e| format!("raw: {e}"))?;
    let params = RawDecodeParams::default();
    let t_decode = Instant::now();
    let raw = dec.raw_image(&src, &params, false).map_err(|e| format!("raw: {e}"))?;
    let decode_ms = t_decode.elapsed().as_secs_f64() * 1000.0;
    let orientation = dec.raw_metadata(&src, &params).ok().and_then(|m| m.exif.orientation).filter(|o| (1..=8).contains(o)).unwrap_or(1) as u8;

    if raw.cpp != 1 {
        return Err("RAW is not a single-channel mosaic".into());
    }
    let RawImageData::Integer(data) = &raw.data else { return Err("floating point RAW data is not supported".into()) };
    let (rw, rh) = (raw.width, raw.height);
    if data.len() < rw * rh {
        return Err("short RAW data".into());
    }
    let cam_cfa = &raw.camera.cfa;
    let cfa = Cfa { w: cam_cfa.width, h: cam_cfa.height, pat: (0..cam_cfa.height).flat_map(|y| (0..cam_cfa.width).map(move |x| (y, x))).map(|(y, x)| cam_cfa.color_at(y, x) as u8).collect() };
    if !cfa.valid() {
        return Err(format!("unsupported colour filter layout {}", cam_cfa.name));
    }

    // Crop to the recommended area (else the active area).
    let (cx, cy, cw, ch) = match raw.crop_area.or(raw.active_area) {
        Some(r) if r.d.w >= 6 && r.d.h >= 6 && r.p.x + r.d.w <= rw && r.p.y + r.d.h <= rh => (r.p.x, r.p.y, r.d.w, r.d.h),
        _ => (0, 0, rw, rh),
    };
    crate::check_size(cw as u64, ch as u64)?;

    // What the camera says: levels, white balance, colour matrix.
    let b = &raw.blacklevel;
    let black = if b.levels.len() == b.width * b.height * b.cpp.max(1) && !b.levels.is_empty() && b.cpp <= 1 {
        Black { levels: b.levels.iter().map(|l| l.as_f32()).collect(), w: b.width.max(1), h: b.height.max(1) }
    } else {
        Black::flat(b.levels.first().map(|l| l.as_f32()).unwrap_or(0.0))
    };
    let white_of = |c: usize| -> f32 {
        let w = &raw.whitelevel.0;
        (if w.len() >= 4 { w[c] } else { w.first().copied().unwrap_or(65535) }) as f32
    };
    let white = [white_of(0), white_of(1), white_of(2)];
    let wb = {
        let w = raw.wb_coeffs;
        if w[..3].iter().all(|v| v.is_finite() && *v > 0.0) { [w[0] / w[1], 1.0, w[2] / w[1]] } else { [1.0, 1.0, 1.0] }
    };
    let neutral = [1.0 / wb[0], 1.0, 1.0 / wb[2]];
    let matrices: Vec<(Illuminant, Vec<f32>)> = raw.color_matrix.iter().map(|(k, v)| (*k, v.clone())).collect();
    let (xyz_to_cam, matrix_note) = pick_matrix(&matrices, neutral)
        .or_else(|| flat3(&raw.xyz_to_cam.iter().flatten().copied().collect::<Vec<f32>>()).map(|m| (m, "camera colour matrix".to_string())))
        .ok_or("the file has no colour matrix")?;
    let cam_to_srgb = camera_to_srgb(&xyz_to_cam);

    let mosaic = Mosaic { data, width: rw, height: rh, cfa, black, white, wb, cam_to_srgb, crop: (cx, cy, cw, ch) };
    let t_dev = Instant::now();
    let (w, h, rgba) = match mode {
        RawMode::Half => develop_half(&mosaic),
        _ => develop_full(&mosaic),
    };
    if w == 0 || h == 0 {
        return Err("empty RAW area".into());
    }
    let develop_ms = t_dev.elapsed().as_secs_f64() * 1000.0;
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.orientation = orientation;
    img.colour_note = Some(format!(
        "RAW developed from the sensor data ({}), {}x{}: as-shot white balance, {}, sRGB curve; no exposure or tone adjustment",
        if mode == RawMode::Half { "half size" } else { "full size" },
        w,
        h,
        matrix_note
    ));
    crate::stats::record_stage("raw sensor decode (rawler)", decode_ms);
    crate::stats::record_stage(if mode == RawMode::Half { "raw develop half" } else { "raw develop full" }, develop_ms);
    crate::stats::record_stage("raw total", t_all.elapsed().as_secs_f64() * 1000.0);
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mosaic of a flat colour: every site holds that colour's value (as the sensor would for a uniform patch).
    fn flat_mosaic(cfa: Cfa, w: usize, h: usize, rgb: [u16; 3]) -> Vec<u16> {
        (0..h).flat_map(|y| (0..w).map(move |x| (y, x))).map(|(y, x)| rgb[cfa.at(y, x)]).collect()
    }

    fn mosaic<'a>(data: &'a [u16], cfa: Cfa, w: usize, h: usize) -> Mosaic<'a> {
        Mosaic { data, width: w, height: h, cfa, black: Black::flat(0.0), white: [1000.0; 3], wb: [1.0; 3], cam_to_srgb: IDENTITY, crop: (3, 3, w - 6, h - 6) }
    }

    fn xtrans() -> Cfa {
        let rows = ["GBGGRG", "RGRBGB", "GBGGRG", "GRGGBG", "BGBRGR", "GRGGBG"];
        Cfa { w: 6, h: 6, pat: rows.iter().flat_map(|r| r.chars()).map(|c| match c { 'R' => 0, 'G' => 1, _ => 2 }).collect() }
    }

    #[test]
    fn a_flat_colour_stays_flat_through_the_bayer_demosaic() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let (w, h) = (24, 20);
        let data = flat_mosaic(cfa.clone(), w, h, [800, 400, 200]);
        let (ow, oh, rgba) = develop_full(&mosaic(&data, cfa, w, h));
        assert_eq!((ow, oh), (w - 6, h - 6));
        let first = &rgba[..3];
        assert!(first[0] > first[1] && first[1] > first[2], "channels keep their order: {first:?}");
        for p in rgba.chunks_exact(4) {
            assert!(p[..3] == first[..3] && p[3] == 255, "pixel {p:?} differs from {first:?}");
        }
    }

    #[test]
    fn a_flat_colour_stays_flat_through_the_xtrans_demosaic_and_the_half_size_paths() {
        let cfa = xtrans();
        let (w, h) = (36, 30);
        let data = flat_mosaic(cfa.clone(), w, h, [900, 300, 100]);
        let m = mosaic(&data, cfa, w, h);
        let (_, _, full) = develop_full(&m);
        let first = full[..3].to_vec();
        assert!(full.chunks_exact(4).all(|p| p[..3] == first[..]), "X-Trans full");
        let (hw, hh, half) = develop_half(&m);
        assert_eq!((hw, hh), (10, 8), "3x3 blocks of the inset crop");
        assert!(half.chunks_exact(4).all(|p| p[..3] == first[..]), "X-Trans half agrees with full");
        let bayer = Cfa::bayer([0, 1, 1, 2]);
        let data = flat_mosaic(bayer.clone(), 16, 16, [900, 300, 100]);
        let (bw, bh, b) = develop_half(&mosaic(&data, bayer, 16, 16));
        assert_eq!((bw, bh), (5, 5), "2x2 blocks of the inset crop");
        assert_eq!(&b[..3], &first[..]);
    }

    #[test]
    fn nothing_is_added_a_mid_grey_stays_exactly_mid_grey() {
        // No exposure, no tone curve: sensor 50% of the range with neutral matrix and white balance = sRGB of linear 0.5.
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let data = flat_mosaic(cfa.clone(), 16, 16, [500, 500, 500]);
        let (_, _, rgba) = develop_full(&mosaic(&data, cfa, 16, 16));
        let want = (1.055 * 0.5f32.powf(1.0 / 2.4) - 0.055) * 255.0;
        assert!((rgba[0] as f32 - want).abs() <= 1.0, "{} vs {want}", rgba[0]);
        // Black and clipped white are not touched either.
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let black = flat_mosaic(cfa.clone(), 16, 16, [0, 0, 0]);
        assert_eq!(&develop_full(&mosaic(&black, cfa.clone(), 16, 16)).2[..3], &[0, 0, 0]);
        let white = flat_mosaic(cfa.clone(), 16, 16, [1000, 1000, 1000]);
        assert_eq!(&develop_full(&mosaic(&white, cfa, 16, 16)).2[..3], &[255, 255, 255]);
    }

    #[test]
    fn white_balance_and_levels_are_applied() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        // Sensor: red reads half of green and blue reads a quarter (a warm light): the as-shot multipliers 2 and 4 make grey.
        let data = flat_mosaic(cfa.clone(), 16, 16, [300, 600, 150]);
        let mut m = mosaic(&data, cfa, 16, 16);
        m.white = [1000.0, 1000.0, 1000.0];
        m.wb = [2.0, 1.0, 4.0];
        let (_, _, rgba) = develop_full(&m);
        assert!(rgba[0] == rgba[1] && rgba[1] == rgba[2], "grey after white balance: {:?}", &rgba[..3]);
        // A black level is subtracted before the range is scaled.
        let data = flat_mosaic(Cfa::bayer([0, 1, 1, 2]), 16, 16, [600, 600, 600]);
        let mut m = mosaic(&data, Cfa::bayer([0, 1, 1, 2]), 16, 16);
        m.black = Black::flat(200.0);
        let (_, _, a) = develop_full(&m);
        let want = (1.055 * 0.5f32.powf(1.0 / 2.4) - 0.055) * 255.0; // (600 - 200) / (1000 - 200) = 0.5
        assert!((a[0] as f32 - want).abs() <= 1.0, "{} vs {want}", a[0]);
    }

    #[test]
    fn the_bayer_demosaic_follows_a_gradient() {
        // A horizontal ramp in all three colours: the interpolated result must rise from left to right and the colours stay equal.
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let (w, h) = (40, 10);
        let data: Vec<u16> = (0..h).flat_map(|_| (0..w).map(|x| (x * 20) as u16)).collect();
        let (_, _, rgba) = develop_full(&mosaic(&data, cfa, w, h));
        let ow = w - 6;
        let row = &rgba[2 * ow * 4..3 * ow * 4];
        for x in 4..ow - 4 {
            let (a, b) = (row[(x - 1) * 4], row[x * 4]);
            assert!(b >= a, "monotonic at {x}");
            assert!((row[x * 4] as i32 - row[x * 4 + 1] as i32).abs() <= 2 && (row[x * 4 + 1] as i32 - row[x * 4 + 2] as i32).abs() <= 2, "grey ramp stays grey at {x}: {:?}", &row[x * 4..x * 4 + 3]);
        }
    }

    #[test]
    fn the_crop_changes_the_output_size_only() {
        let cfa = Cfa::bayer([0, 1, 1, 2]);
        let data = flat_mosaic(cfa.clone(), 30, 30, [700, 400, 200]);
        let mut m = mosaic(&data, cfa, 30, 30);
        let (_, _, whole) = develop_full(&m);
        m.crop = (5, 7, 12, 9);
        let (w, h, part) = develop_full(&m);
        assert_eq!((w, h), (12, 9));
        assert_eq!(&part[..3], &whole[..3]);
    }

    #[test]
    fn matrix_choice_is_deterministic_and_interpolates() {
        let a = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let d = vec![2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0];
        // One illuminant: that matrix, whatever the order.
        let one = pick_matrix(&[(Illuminant::D65, d.clone())], [1.0; 3]).unwrap().0;
        assert_eq!(one[0][0], 2.0);
        // Two: the same answer in either order.
        let (m1, _) = pick_matrix(&[(Illuminant::A, a.clone()), (Illuminant::D65, d.clone())], [1.0; 3]).unwrap();
        let (m2, _) = pick_matrix(&[(Illuminant::D65, d.clone()), (Illuminant::A, a.clone())], [1.0; 3]).unwrap();
        assert_eq!(m1, m2);
        assert!(m1[0][0] >= 1.0 && m1[0][0] <= 2.0);
        assert!(pick_matrix(&[], [1.0; 3]).is_none());
    }

    #[test]
    fn identity_like_matrix_keeps_white() {
        // A camera whose XYZ -> camera matrix is the sRGB one must map white balanced grey to grey.
        let inv = invert(&SRGB_TO_XYZ).unwrap();
        let m = camera_to_srgb(&inv);
        for v in m.iter().flatten() {
            assert!(v.is_finite());
        }
        let white: Vec<f32> = (0..3).map(|i| m[i].iter().sum()).collect();
        for w in white {
            assert!((w - 1.0).abs() < 1e-3, "row sum {w}");
        }
    }

    #[test]
    fn degenerate_matrices_fall_back_to_identity() {
        let ones = [[1.0; 3]; 3];
        assert_eq!(camera_to_srgb(&ones), IDENTITY);
        assert_eq!(camera_to_srgb(&[]), IDENTITY);
    }

    #[test]
    fn modes_have_ids() {
        for m in RawMode::ALL {
            assert_eq!(RawMode::from_id(m.id()), Some(m));
        }
        assert_eq!(RawMode::from_id("nonsense"), None);
    }

    #[test]
    fn gamma_is_monotonic_and_spans_the_range() {
        let g = gamma_table();
        assert_eq!((g[0], g[4095]), (0, 255));
        assert!(g.windows(2).all(|w| w[0] <= w[1]));
    }
}
