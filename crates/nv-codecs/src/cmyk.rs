//! CMYK pictures.
//!
//! Decoders hand CMYK over **raw**: an `Image` with `cmyk = true` holds the four ink amounts of every pixel in its RGBA
//! buffer (byte 0 = cyan ... byte 3 = black, 0 = no ink, 255 = full ink; the "Adobe inverted" convention of JPEG and
//! Photoshop files is undone by the decoder, see `jpeg.rs` and `psdraw.rs`). Nothing is converted to RGB in a decoder.
//! Conversion happens later, in one place, here: through the picture's own ICC profile if it has one, otherwise through
//! the profile the user chose for CMYK pictures without one (built-in Coated FOGRA39 by default), by way of the same
//! colour engine (moxcms) as every other picture, with the rendering intent and black point setting the user chose.
//!
//! moxcms converts CMYK -> RGB through the profile's A2B tables (perceptual, relative colorimetric and saturation agree
//! with LittleCMS to within one level, see `tests/cmyk_fogra39.rs`), but it has no black point compensation for CMYK
//! sources and does nothing for absolute colorimetric. Those two are done here, in floating point, on the linear light
//! of the output: black point compensation takes the profile's black point (its darkest colorant, all four inks, read
//! through the relative table) out of the light and stretches the rest back to white, as LittleCMS does for relative
//! colorimetric; absolute colorimetric scales the light by the paper's own white (the profile's media white point
//! against D50), which is how the paper tint is shown.

use crate::icc::{self, Intent, Profile, Target, Transform};
use moxcms::{ColorProfile, Layout, RenderingIntent, ToneReprCurve, TransformF32Executor, TransformOptions};
use std::sync::{Arc, Mutex};

/// A ready CMYK -> RGB conversion.
pub struct CmykTransform {
    stage: Stage,
}

enum Stage {
    /// 8-bit table lookup straight from the profile.
    Plain(Transform),
    /// Black point compensation (relative colorimetric) or paper white (absolute colorimetric), in floating point.
    Float(Float),
}

struct Float {
    /// CMYK -> linear light in the output's primaries (unclipped).
    to_linear: Arc<TransformF32Executor>,
    /// Black point in that linear light, per channel (zero = no compensation).
    black: [f32; 3],
    /// Absolute colorimetric: paper white scaling as a matrix on the linear light.
    paper: Option<[[f32; 3]; 3]>,
    /// Linear -> the output's transfer curve.
    encode: Arc<TransformF32Executor>,
    /// When the output isn't a matrix profile: the sRGB result still has to go through it.
    then: Option<Transform>,
}

/// How a picture will be converted, and what to tell the user about it.
pub struct Plan {
    pub transform: Arc<CmykTransform>,
    /// One line for the info panel: which profile, which intent.
    pub note: String,
    /// The picture had no (usable) profile: the user's CMYK choice was assumed.
    pub assumed: bool,
}

/// What `convert` did.
pub struct Outcome {
    pub note: String,
    pub assumed: bool,
}

/// The classic quick conversion (no colour management): used when colour management is switched off, so that a CMYK
/// picture is still a picture.
pub fn naive(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let k = 255 - p[3] as u32;
        let (c, m, y) = (255 - p[0] as u32, 255 - p[1] as u32, 255 - p[2] as u32);
        p[0] = (c * k / 255) as u8;
        p[1] = (m * k / 255) as u8;
        p[2] = (y * k / 255) as u8;
        p[3] = 255;
    }
}

fn linear(mut p: ColorProfile) -> ColorProfile {
    let lin = ToneReprCurve::Parametric(vec![1.0]);
    p.red_trc = Some(lin.clone());
    p.green_trc = Some(lin.clone());
    p.blue_trc = Some(lin);
    p.cicp = None; // a profile that names its transfer curve by CICP would ignore the curves above
    p
}

fn options(intent: RenderingIntent) -> TransformOptions {
    TransformOptions { rendering_intent: intent, ..Default::default() }
}

/// The profile's black point in `to_linear`'s linear light: its darkest colorant (every ink at 100%) through the
/// relative colorimetric table, made neutral (only its luminance counts, the way LittleCMS does it). Anything that
/// isn't dark means the detection went wrong: then nothing is compensated.
fn detect_black(to_linear: &TransformF32Executor, out_profile: &ColorProfile) -> [f32; 3] {
    let mut out = [0f32; 4];
    if to_linear.transform(&[1.0; 4], &mut out).is_err() {
        return [0.0; 3];
    }
    let y = out_profile.red_colorant.y as f32 * out[0] + out_profile.green_colorant.y as f32 * out[1] + out_profile.blue_colorant.y as f32 * out[2];
    if y.is_finite() && (0.0..0.2).contains(&y) { [y; 3] } else { [0.0; 3] }
}

type M3 = [[f64; 3]; 3];

fn invert3(m: &M3) -> Option<M3> {
    let c = |i: usize, j: usize| m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3] - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3];
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    if !det.is_finite() || det.abs() < 1e-9 {
        return None;
    }
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = c(j, i) / det;
        }
    }
    Some(o)
}

/// Linear light -> XYZ (D50) of a matrix profile: its colorants as columns.
fn colorant_matrix(p: &ColorProfile) -> M3 {
    [[p.red_colorant.x, p.green_colorant.x, p.blue_colorant.x], [p.red_colorant.y, p.green_colorant.y, p.blue_colorant.y], [p.red_colorant.z, p.green_colorant.z, p.blue_colorant.z]]
}

/// Absolute colorimetric: linear output light -> XYZ -> scaled by (the source's paper white / D50) -> linear output light.
fn paper_matrix(src: &ColorProfile, mid: &ColorProfile) -> Option<[[f32; 3]; 3]> {
    let w = src.media_white_point?;
    let k = [w.x / 0.9642, w.y / 1.0, w.z / 0.8249];
    if k.iter().any(|v| !v.is_finite() || *v <= 0.5 || *v > 1.5) {
        return None;
    }
    let m = colorant_matrix(mid);
    let inv = invert3(&m)?;
    let mut out = [[0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|n| inv[i][n] * k[n] * m[n][j]).sum::<f64>() as f32;
        }
    }
    Some(out)
}

fn build_float(src: &ColorProfile, dst: &ColorProfile, intent: Intent, bpc: bool) -> Result<Float, String> {
    let srgb = ColorProfile::new_srgb();
    let (mid, then) = if dst.is_matrix_shaper() { (dst.clone(), None) } else { (srgb.clone(), Some(icc::make_transform_with(&srgb, dst, Intent::Relative)?)) };
    let lin = linear(mid.clone());
    let o = options(RenderingIntent::RelativeColorimetric);
    let to_linear = src.create_transform_f32(Layout::Rgba, &lin, Layout::Rgba, o).map_err(|e| format!("{e:?}"))?;
    let encode = lin.create_transform_f32(Layout::Rgba, &mid, Layout::Rgba, o).map_err(|e| format!("{e:?}"))?;
    let black = if bpc && intent == Intent::Relative { detect_black(&*to_linear, &mid) } else { [0.0; 3] };
    let paper = if intent == Intent::Absolute { paper_matrix(src, &mid) } else { None };
    Ok(Float { to_linear, black, paper, encode, then })
}

fn build(src: &ColorProfile, target: Target) -> Result<CmykTransform, String> {
    let cfg = icc::current();
    let srgb = ColorProfile::new_srgb();
    let dst = match (target, &cfg.output) {
        (Target::Display, Some(out)) => out.color_profile(),
        _ => &srgb,
    };
    if (cfg.bpc && cfg.intent == Intent::Relative) || cfg.intent == Intent::Absolute {
        if let Ok(f) = build_float(src, dst, cfg.intent, cfg.bpc) {
            return Ok(CmykTransform { stage: Stage::Float(f) });
        } // else fall through to the plain conversion
    }
    Ok(CmykTransform { stage: Stage::Plain(icc::make_transform_with(src, dst, cfg.intent)?) })
}

/// Recently built conversions, so a folder of CMYK pictures that share a profile doesn't rebuild it for every picture.
fn cache() -> &'static Mutex<Vec<(u64, Arc<CmykTransform>)>> {
    static C: std::sync::OnceLock<Mutex<Vec<(u64, Arc<CmykTransform>)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

fn cached(key: u64, make: impl FnOnce() -> Result<CmykTransform, String>) -> Result<Arc<CmykTransform>, String> {
    if let Some(t) = cache().lock().ok().and_then(|c| c.iter().find(|(k, _)| *k == key).map(|(_, t)| t.clone())) {
        return Ok(t);
    }
    let t = Arc::new(make()?);
    if let Ok(mut c) = cache().lock() {
        c.retain(|(k, _)| *k != key);
        c.insert(0, (key, t.clone()));
        c.truncate(6);
    }
    Ok(t)
}

fn key_of(profile_id: u64, target: Target) -> u64 {
    let cfg = icc::current();
    let out = if target == Target::Display { cfg.output.as_ref().map(|p| p.id()).unwrap_or(0) } else { 0 };
    let mut k = profile_id ^ out.rotate_left(13) ^ ((cfg.intent as u64 + 1) << 58) ^ ((cfg.bpc as u64) << 62);
    if target == Target::Srgb {
        k = !k;
    }
    k
}

fn intent_text() -> String {
    let cfg = icc::current();
    if cfg.bpc && cfg.intent == Intent::Relative { format!("{}, black point compensation", cfg.intent.label()) } else { cfg.intent.label().to_string() }
}

/// The conversion for a CMYK picture whose own profile bytes are `embedded` (if it has any). None when colour
/// management is switched off (the caller then uses `naive`).
pub fn plan(embedded: Option<&[u8]>, target: Target) -> Option<Plan> {
    let cfg = icc::current();
    if !cfg.enabled {
        return None;
    }
    let mut problem = None;
    if let Some(bytes) = embedded {
        match Profile::cmyk_from_bytes(bytes) {
            Ok(p) => match cached(key_of(p.id(), target), || build(p.color_profile(), target)) {
                Ok(transform) => return Some(Plan { transform, note: format!("CMYK, profile \"{}\" (embedded), {}", p.name, intent_text()), assumed: false }),
                Err(e) => problem = Some(format!("the embedded profile \"{}\" can't be used ({e})", p.name)),
            },
            Err(e) => problem = Some(format!("the embedded profile can't be used ({e})")),
        }
    }
    let assumed = cfg.cmyk.clone().unwrap_or_else(Profile::fogra39);
    let transform = cached(key_of(assumed.id(), target), || build(assumed.color_profile(), target)).ok()?;
    let why = problem.unwrap_or_else(|| "no embedded profile".to_string());
    Some(Plan { transform, note: format!("CMYK: {why}, assuming \"{}\", {}", assumed.name, intent_text()), assumed: true })
}

/// Convert CMYK ink amounts in `rgba` to RGB in place (alpha becomes 255).
pub fn apply(rgba: &mut [u8], t: &CmykTransform) {
    match &t.stage {
        Stage::Plain(tr) => icc::par_parts(rgba, |part| {
            let mut tmp = vec![0u8; icc::CHUNK_PIXELS * 4];
            for chunk in part.chunks_mut(icc::CHUNK_PIXELS * 4) {
                let n = chunk.len();
                tmp[..n].copy_from_slice(chunk);
                if tr.transform(&tmp[..n], chunk).is_err() {
                    naive(chunk);
                    continue;
                }
                chunk.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            }
        }),
        Stage::Float(b) => icc::par_parts(rgba, |part| {
            let cap = icc::CHUNK_PIXELS * 4;
            let (mut src, mut lin, mut enc) = (vec![0f32; cap], vec![0f32; cap], vec![0f32; cap]);
            for chunk in part.chunks_mut(cap) {
                let n = chunk.len();
                for (d, &s) in src[..n].iter_mut().zip(chunk.iter()) {
                    *d = s as f32 * (1.0 / 255.0);
                }
                if b.to_linear.transform(&src[..n], &mut lin[..n]).is_err() {
                    naive(chunk);
                    continue;
                }
                for px in lin[..n].chunks_exact_mut(4) {
                    if let Some(m) = &b.paper {
                        let (r, g, bl) = (px[0], px[1], px[2]);
                        px[0] = m[0][0] * r + m[0][1] * g + m[0][2] * bl;
                        px[1] = m[1][0] * r + m[1][1] * g + m[1][2] * bl;
                        px[2] = m[2][0] * r + m[2][1] * g + m[2][2] * bl;
                    }
                    for c in 0..3 {
                        // Take the black point out of the light, then stretch back to white.
                        px[c] = ((px[c] - b.black[c]) / (1.0 - b.black[c])).clamp(0.0, 1.0);
                    }
                    px[3] = 1.0;
                }
                if b.encode.transform(&lin[..n], &mut enc[..n]).is_err() {
                    naive(chunk);
                    continue;
                }
                for (d, &s) in chunk.iter_mut().zip(enc[..n].iter()) {
                    *d = (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                }
                if let Some(t) = &b.then {
                    let copy = chunk.to_vec();
                    let _ = t.transform(&copy, chunk);
                }
                chunk.chunks_exact_mut(4).for_each(|p| p[3] = 255);
            }
        }),
    }
}

/// Convert a picture's raw CMYK to RGB in `target`'s terms. Always leaves valid RGBA behind.
pub fn convert(rgba: &mut [u8], embedded: Option<&[u8]>, target: Target) -> Outcome {
    match plan(embedded, target) {
        Some(p) => {
            apply(rgba, &p.transform);
            Outcome { note: p.note, assumed: p.assumed }
        }
        None => {
            naive(rgba);
            Outcome { note: "CMYK: approximate conversion, colour management is off".into(), assumed: false }
        }
    }
}
