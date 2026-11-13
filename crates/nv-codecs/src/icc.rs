//! Colour management: what the pixel numbers of a picture mean, and what they have to become for the screen.
//!
//! Every picture is first brought into one of three states of meaning:
//!  * it carries its own ICC profile (an `Image::icc` the decoder found),
//!  * it carries none but is a photo-style bitmap (JPEG, PNG, WebP): the user's "pictures without a profile are ..."
//!    choice says what it is (sRGB unless told otherwise),
//!  * it carries none and is a render (SVG, PDF, ...): sRGB.
//!
//! The *output* is the profile of whatever the pixels are shown on: plain sRGB (the default: nothing to do for most
//! pictures), the profile Windows has for the display, or a `.icc` file the user chose. This is deliberately not
//! tied to the system: someone grading video or doing graphic work can drive the viewer with another profile.
//!
//! Conversions use moxcms (matrix/shaper and LUT profiles). The transforms that don't depend on a picture are built
//! once, when the settings change; only a picture's own embedded profile is parsed per picture.

use moxcms::{ColorProfile, DataColorSpace, Layout, RenderingIntent, Transform8BitExecutor, TransformOptions};
use std::sync::{Arc, OnceLock, RwLock};

/// Pixels converted per call, so we never need a full-image scratch copy.
const CHUNK_PIXELS: usize = 1 << 16;

pub type Transform = Arc<Transform8BitExecutor>;

/// An RGB profile: one of the built-in ones or one read from an `.icc` / `.icm` file.
#[derive(Clone)]
pub struct Profile {
    /// The profile's own description ("sRGB IEC61966-2.1", "DELL U2723QE", ...).
    pub name: String,
    profile: Arc<ColorProfile>,
    is_srgb: bool,
    /// Identifies the profile in cache keys.
    id: u64,
}

impl Profile {
    pub fn srgb() -> Profile {
        Profile { name: "sRGB".into(), profile: Arc::new(ColorProfile::new_srgb()), is_srgb: true, id: 1 }
    }

    pub fn display_p3() -> Profile {
        Profile { name: "Display P3".into(), profile: Arc::new(ColorProfile::new_display_p3()), is_srgb: false, id: 2 }
    }

    pub fn adobe_rgb() -> Profile {
        Profile { name: "Adobe RGB (1998)".into(), profile: Arc::new(ColorProfile::new_adobe_rgb()), is_srgb: false, id: 3 }
    }

    /// Parse an ICC profile (the bytes of an `.icc` file). Only RGB profiles can describe or receive a colour picture.
    pub fn from_bytes(bytes: &[u8]) -> Result<Profile, String> {
        let parsed = ColorProfile::new_from_slice(bytes).map_err(|e| format!("not a readable ICC profile ({e:?})"))?;
        if parsed.color_space != DataColorSpace::Rgb {
            return Err(format!("it is a {:?} profile; an RGB profile is needed", parsed.color_space));
        }
        let name = profile_description(bytes).unwrap_or_else(|| "(unnamed profile)".into());
        let is_srgb = name.trim_start().to_ascii_lowercase().starts_with("srgb");
        let mut id = 0xcbf2_9ce4_8422_2325u64;
        for &b in bytes {
            id = (id ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3);
        }
        Ok(Profile { name, profile: Arc::new(parsed), is_srgb, id: id | 8 })
    }

    pub fn from_file(path: &std::path::Path) -> Result<Profile, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
        Self::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn is_srgb(&self) -> bool {
        self.is_srgb
    }
}

/// The text of an ICC profile's `desc` tag (v2 `desc` or v4 `mluc`), if it has a readable one.
pub fn profile_description(icc: &[u8]) -> Option<String> {
    let be32 = |o: usize| icc.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let count = be32(128)?;
    for i in 0..count.min(200) {
        let e = 132 + i * 12;
        if icc.get(e..e + 4)? != b"desc" {
            continue;
        }
        let (off, len) = (be32(e + 4)?, be32(e + 8)?);
        let tag = icc.get(off..off.checked_add(len)?)?;
        match tag.get(..4)? {
            b"desc" => {
                // ASCII count (including the trailing NUL) then the characters.
                let n = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
                let text = tag.get(12..12 + n.min(tag.len().saturating_sub(12)))?;
                return Some(String::from_utf8_lossy(text).trim_end_matches('\0').trim().to_string()).filter(|s| !s.is_empty());
            }
            b"mluc" => {
                // First record: length, offset (both from the start of the tag), UTF-16BE text.
                let (rlen, roff) = (u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?) as usize, u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?) as usize);
                let text = tag.get(roff..roff.checked_add(rlen)?)?;
                let units: Vec<u16> = text.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                return Some(String::from_utf16_lossy(&units).trim_end_matches('\0').trim().to_string()).filter(|s| !s.is_empty());
            }
            _ => return None,
        }
    }
    None
}

fn make_transform(src: &ColorProfile, dst: &ColorProfile) -> Result<Transform, String> {
    let options = TransformOptions { rendering_intent: RenderingIntent::RelativeColorimetric, ..Default::default() };
    src.create_transform_8bit(Layout::Rgba, dst, Layout::Rgba, options).map_err(|e| format!("{e:?}"))
}

/// Everything derived from the settings, ready to use. Built by `configure`.
struct Config {
    enabled: bool,
    /// Profile of the output, None = sRGB.
    output: Option<Profile>,
    /// sRGB -> output; None = nothing to do (output is sRGB).
    srgb_to_out: Option<Transform>,
    /// What a profile-less photo-style bitmap becomes on the output; None = nothing to do.
    untagged_to_out: Option<Transform>,
    /// ... and in sRGB (for thumbnails, which are cached as sRGB); None = nothing to do.
    untagged_to_srgb: Option<Transform>,
    /// Changes whenever the meaning of a cached sRGB thumbnail changes.
    thumb_signature: u64,
}

fn lock() -> &'static RwLock<Arc<Config>> {
    static CONFIG: OnceLock<RwLock<Arc<Config>>> = OnceLock::new();
    CONFIG.get_or_init(|| RwLock::new(Arc::new(Config { enabled: true, output: None, srgb_to_out: None, untagged_to_out: None, untagged_to_srgb: None, thumb_signature: 1 })))
}

fn current() -> Arc<Config> {
    lock().read().map(|g| g.clone()).unwrap_or_else(|e| e.into_inner().clone())
}

fn install(cfg: Config) {
    match lock().write() {
        Ok(mut g) => *g = Arc::new(cfg),
        Err(e) => *e.into_inner() = Arc::new(cfg),
    }
}

/// Apply new colour settings (pictures decoded afterwards use them). `output` / `untagged` of None mean sRGB.
/// On error nothing changes and the message says which profile can't be used and why.
pub fn configure(enabled: bool, output: Option<Profile>, untagged: Option<Profile>) -> Result<(), String> {
    install(build(enabled, output, untagged)?);
    Ok(())
}

/// Would `configure` accept these? Changes nothing (for validating a settings form before it is saved).
pub fn check(output: Option<Profile>, untagged: Option<Profile>) -> Result<(), String> {
    build(true, output, untagged).map(|_| ())
}

fn build(enabled: bool, output: Option<Profile>, untagged: Option<Profile>) -> Result<Config, String> {
    let srgb = ColorProfile::new_srgb();
    let output = output.filter(|p| !p.is_srgb);
    let out_profile = output.as_ref().map(|p| &*p.profile).unwrap_or(&srgb);
    let srgb_to_out = match &output {
        Some(p) => Some(make_transform(&srgb, &p.profile).map_err(|e| format!("The output profile \"{}\" can't be used as a display profile ({e}).", p.name))?),
        None => None,
    };
    let untagged = untagged.filter(|p| !p.is_srgb);
    let (untagged_to_out, untagged_to_srgb) = match &untagged {
        Some(p) => (
            Some(make_transform(&p.profile, out_profile).map_err(|e| format!("The profile \"{}\" can't be used for pictures without a profile ({e}).", p.name))?),
            Some(make_transform(&p.profile, &srgb).map_err(|e| format!("The profile \"{}\" can't be used for pictures without a profile ({e}).", p.name))?),
        ),
        None => (srgb_to_out.clone(), None),
    };
    let thumb_signature = (enabled as u64) ^ untagged.as_ref().map(|p| p.id).unwrap_or(0).rotate_left(1);
    Ok(Config { enabled, output, srgb_to_out, untagged_to_out, untagged_to_srgb, thumb_signature })
}

/// Colour management on or off, as last configured.
pub fn enabled() -> bool {
    current().enabled
}

/// Name of the output profile in use ("sRGB" when none was chosen).
pub fn output_name() -> String {
    current().output.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "sRGB".into())
}

/// Part of the thumbnail cache key: cached thumbnails hold sRGB, so only the *input* side of the settings matters.
pub fn thumb_signature() -> u64 {
    current().thumb_signature
}

/// Where a conversion goes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The screen: the output profile.
    Display,
    /// Plain sRGB: files that are written, the clipboard, cached thumbnails.
    Srgb,
}

/// The transform for a picture, or None if its numbers can stay as they are.
/// `embedded` is the picture's own profile; `assumable` says a missing profile may be replaced by the user's choice.
pub fn transform_for(embedded: Option<&[u8]>, assumable: bool, target: Target) -> Option<Transform> {
    let cfg = current();
    if !cfg.enabled {
        return None;
    }
    match embedded {
        None if assumable => match target {
            Target::Display => cfg.untagged_to_out.clone(),
            Target::Srgb => cfg.untagged_to_srgb.clone(),
        },
        None => match target {
            Target::Display => cfg.srgb_to_out.clone(),
            Target::Srgb => None,
        },
        Some(icc) => {
            // Fast path: the overwhelmingly common profile is already sRGB (its description says so).
            if icc.windows(4).any(|w| w == b"sRGB") {
                return match target {
                    Target::Display => cfg.srgb_to_out.clone(),
                    Target::Srgb => None,
                };
            }
            let src = ColorProfile::new_from_slice(icc).ok()?;
            if src.color_space != DataColorSpace::Rgb {
                return None;
            }
            match (target, &cfg.output) {
                (Target::Display, Some(out)) => make_transform(&src, &out.profile).ok(),
                _ => make_transform(&src, &ColorProfile::new_srgb()).ok(),
            }
        }
    }
}

/// sRGB -> output, for pictures that are already sRGB (cached thumbnails). None if the output is sRGB.
pub fn srgb_to_output() -> Option<Transform> {
    let cfg = current();
    if cfg.enabled { cfg.srgb_to_out.clone() } else { None }
}

/// Converts `rgba` in place with `t`. Big images are split across threads (the transform is immutable and shareable).
pub fn apply(rgba: &mut [u8], t: &Transform) {
    let pixels = rgba.len() / 4;
    let threads = if pixels < 1_000_000 { 1 } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 6) };
    let per_thread = (pixels.div_ceil(threads)).max(1) * 4;
    let background = crate::progress::is_background();
    std::thread::scope(|scope| {
        for part in rgba.chunks_mut(per_thread) {
            scope.spawn(move || {
                crate::progress::set_background(background);
                let mut tmp = vec![0u8; CHUNK_PIXELS * 4];
                for chunk in part.chunks_mut(CHUNK_PIXELS * 4) {
                    let n = chunk.len();
                    tmp[..n].copy_from_slice(chunk);
                    let _ = t.transform(&tmp[..n], chunk);
                }
            });
        }
    });
}

/// Converts `rgba` from the embedded profile `icc` to sRGB in place. Does nothing for sRGB profiles, non-RGB
/// profiles, or anything we can't parse.
pub fn to_srgb_in_place(rgba: &mut [u8], icc: &[u8]) {
    if icc.windows(4).any(|w| w == b"sRGB") {
        return;
    }
    let Ok(src) = ColorProfile::new_from_slice(icc) else { return };
    if src.color_space != DataColorSpace::Rgb {
        return;
    }
    if let Ok(t) = make_transform(&src, &ColorProfile::new_srgb()) {
        apply(rgba, &t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings are process-wide: tests that change them must not overlap.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn p3_icc() -> Vec<u8> {
        ColorProfile::new_display_p3().encode().expect("encodes")
    }

    #[test]
    fn defaults_leave_pictures_alone() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        configure(true, None, None).unwrap();
        assert!(transform_for(None, true, Target::Display).is_none());
        assert!(transform_for(None, false, Target::Display).is_none());
        assert!(srgb_to_output().is_none());
        assert_eq!(output_name(), "sRGB");
    }

    #[test]
    fn output_profile_changes_srgb_pictures_and_embedded_ones() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        configure(true, Some(Profile::display_p3()), None).unwrap();
        // An sRGB saturated red is *less* saturated in P3 numbers.
        let mut px = [255u8, 0, 0, 255];
        apply(&mut px, &transform_for(None, true, Target::Display).expect("untagged photo is converted"));
        assert!(px[0] < 255 && px[1] > 0, "{px:?}");
        assert_eq!(px[3], 255);
        // A P3 picture shown on a P3 output is untouched (to within rounding).
        let icc = p3_icc();
        let mut p3 = [200u8, 80, 40, 255];
        let t = transform_for(Some(&icc), false, Target::Display).expect("embedded profile");
        apply(&mut p3, &t);
        assert!((p3[0] as i32 - 200).abs() <= 1 && (p3[1] as i32 - 80).abs() <= 1 && (p3[2] as i32 - 40).abs() <= 1, "{p3:?}");
        // For a file or the clipboard the target is sRGB whatever the screen is.
        let mut to_srgb = [200u8, 80, 40, 255];
        apply(&mut to_srgb, &transform_for(Some(&icc), false, Target::Srgb).expect("p3 -> srgb"));
        assert!(to_srgb[0] != 200 || to_srgb[1] != 80);
        configure(true, None, None).unwrap();
    }

    #[test]
    fn assumed_profile_for_untagged_photos() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        configure(true, None, Some(Profile::adobe_rgb())).unwrap();
        assert!(transform_for(None, true, Target::Display).is_some(), "untagged photo is read as Adobe RGB");
        assert!(transform_for(None, false, Target::Display).is_none(), "a render stays sRGB");
        assert!(transform_for(None, true, Target::Srgb).is_some());
        configure(true, None, None).unwrap();
    }

    #[test]
    fn switched_off_means_numbers_as_they_are() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        configure(false, Some(Profile::display_p3()), None).unwrap();
        assert!(transform_for(Some(&p3_icc()), true, Target::Display).is_none());
        configure(true, None, None).unwrap();
    }

    #[test]
    fn profile_files_are_checked_and_named() {
        let icc = p3_icc();
        let p = Profile::from_bytes(&icc).expect("a P3 profile loads");
        assert!(!p.is_srgb());
        assert!(!p.name.is_empty());
        assert!(Profile::from_bytes(b"definitely not a profile").is_err());
        let srgb = ColorProfile::new_srgb().encode().expect("encodes");
        assert!(Profile::from_bytes(&srgb).map(|p| p.is_srgb()).unwrap_or(false), "sRGB by its description");
    }

    #[test]
    fn thumbnail_signature_follows_the_input_side_only() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        configure(true, None, None).unwrap();
        let base = thumb_signature();
        configure(true, Some(Profile::display_p3()), None).unwrap();
        assert_eq!(thumb_signature(), base, "the output profile is applied after the cache");
        configure(true, None, Some(Profile::adobe_rgb())).unwrap();
        assert_ne!(thumb_signature(), base);
        configure(true, None, None).unwrap();
    }
}
