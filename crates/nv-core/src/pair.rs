//! Showing a picture with its NuEditor pair file (`IMG.jpg.nuedit`) applied: the viewer's picture and the gallery's thumbnails.
//! The picture on disk is never touched; the edits are applied to what is shown, with the same engine NuEditor uses.
//!
//! The cost for a picture without a pair file is one `metadata` call. A pair file that does not fit the picture (another size)
//! is ignored, exactly as NuEditor ignores it.

use nv_codecs::Image;
use nv_editor::sidecar::{self, Source};
use nv_editor::{engine, Pixels, Recipe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static SHOW: AtomicBool = AtomicBool::new(true);

/// Settings > NuEditor: show pictures with their pair files applied.
pub fn set_show(on: bool) {
    SHOW.store(on, Ordering::Relaxed);
}

pub fn show() -> bool {
    SHOW.load(Ordering::Relaxed)
}

/// A number that changes when the pair file of `picture` does; 0 when there is none (or edits are not shown). Part of the
/// thumbnail cache key, so a thumbnail is made again after the picture was edited.
pub fn stamp(picture: &Path) -> u64 {
    if !show() {
        return 0;
    }
    let Ok(meta) = std::fs::metadata(sidecar::sidecar_path(picture)) else { return 0 };
    let t = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(1);
    (t ^ meta.len().wrapping_mul(0x9E37_79B9_7F4A_7C15)) | 1
}

/// The recipe of `picture`'s pair file if it was made for a picture of exactly `w` x `h` (upright pixels); None when there is no
/// pair file, it is for something else, or it changes nothing.
pub fn recipe_for(picture: &Path, w: u32, h: u32) -> Option<Recipe> {
    let (r, src) = sidecar::load(picture).ok().flatten()?;
    (src.matches(&Source { width: w, height: h, ..Default::default() }) && !r.is_identity()).then_some(r)
}

/// The same for a picture known only by its shape (a thumbnail, or a preview of another size): the pair file must have been
/// made for a picture of about the same aspect ratio.
pub fn recipe_for_shape(picture: &Path, w: u32, h: u32) -> Option<Recipe> {
    let (r, src) = sidecar::load(picture).ok().flatten()?;
    let fits = src.width == 0 || src.height == 0 || h == 0 || ((src.width as f32 / src.height as f32) / (w as f32 / h as f32) - 1.0).abs() < 0.03;
    (fits && !r.is_identity()).then_some(r)
}

/// Decode a picture for the viewer: as `nv_codecs::decode_file`, but with the pair file's edits applied when there is one.
pub fn decode(path: &Path) -> Result<Image, String> {
    let img = nv_codecs::decode_file(path)?;
    if !show() || img.mesh.is_some() || !img.extra_frames.is_empty() || !sidecar::has_sidecar(path) {
        return Ok(img);
    }
    // The pair file is about the upright picture; the decoder hands it over unrotated with the EXIF orientation still to do.
    let (uw, uh) = if img.orientation >= 5 { (img.height, img.width) } else { (img.width, img.height) };
    let (uw, uh) = match img.reduced_from {
        Some((w, h)) => if img.orientation >= 5 { (h, w) } else { (w, h) },
        None => (uw, uh),
    };
    let Some(recipe) = recipe_for(path, uw, uh) else { return Ok(img) };
    let t = std::time::Instant::now();
    let (reduced, mut stages) = (img.reduced_from, img.stages.clone());
    let up = img.finish_for_cpu();
    let out = engine::render(&Pixels::new(up.width, up.height, up.rgba.clone()), &recipe);
    let mut shown = Image::new(out.w, out.h, out.rgba);
    shown.reduced_from = reduced;
    shown.colour_note = up.colour_note.clone();
    shown.colour_assumed = up.colour_assumed;
    stages.push(("pair file edits", t.elapsed().as_secs_f32() * 1000.0));
    shown.stages = stages;
    Ok(shown)
}

/// A thumbnail (upright sRGB, already small) with the pair file of `picture` applied.
pub fn apply_to_thumb(small: Image, picture: &Path) -> Image {
    let Some(recipe) = recipe_for_shape(picture, small.width, small.height) else { return small };
    let out = engine::render(&Pixels::new(small.width, small.height, small.rgba.clone()), &recipe);
    let mut shown = Image::new(out.w, out.h, out.rgba);
    shown.reduced_from = small.reduced_from;
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use nv_editor::P;

    /// The "show edits" switch is global: the tests that touch it take turns.
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn folder(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nv-pair-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_png(path: &Path, w: u32, h: u32, rgb: [u8; 3]) {
        let p = Pixels::new(w, h, (0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect());
        std::fs::write(path, nv_editor::save::encode(&p, nv_editor::save::Format::Png, None).unwrap()).unwrap();
    }

    #[test]
    fn a_picture_with_a_pair_file_is_shown_edited_and_without_one_untouched() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let d = folder("decode");
        let pic = d.join("a.png");
        write_png(&pic, 40, 30, [100, 100, 100]);
        set_show(true);
        assert_eq!(stamp(&pic), 0);
        let plain = decode(&pic).unwrap();
        assert_eq!(&plain.rgba[..3], &[100, 100, 100]);

        let mut r = Recipe::default();
        r.invert = true;
        r.crop = Some(nv_editor::recipe::Crop { x: 0.0, y: 0.0, w: 0.5, h: 1.0 });
        sidecar::save(&pic, &r, &Source::of(&pic, 40, 30)).unwrap();
        assert_ne!(stamp(&pic), 0);
        let shown = decode(&pic).unwrap();
        assert_eq!((shown.width, shown.height), (20, 30));
        assert_eq!(&shown.rgba[..3], &[155, 155, 155], "inverted");
        assert!(shown.stages.iter().any(|s| s.0 == "pair file edits"));

        // Turned off in the settings: the picture as it is.
        set_show(false);
        assert_eq!(decode(&pic).unwrap().width, 40);
        assert_eq!(stamp(&pic), 0);
        set_show(true);

        // A pair file for another size is ignored.
        let other = d.join("b.png");
        write_png(&other, 10, 10, [1, 2, 3]);
        sidecar::save(&other, &r, &Source::of(&other, 99, 99)).unwrap();
        assert_eq!(decode(&other).unwrap().width, 10);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_thumbnail_follows_the_pair_file_by_shape() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let d = folder("thumb");
        let pic = d.join("a.png");
        write_png(&pic, 400, 300, [10, 10, 10]);
        let mut r = Recipe::default();
        r.set(P::Exposure, 2.0);
        sidecar::save(&pic, &r, &Source::of(&pic, 400, 300)).unwrap();
        set_show(true);
        let small = Image::new(40, 30, (0..40 * 30).flat_map(|_| [10u8, 10, 10, 255]).collect());
        let t = apply_to_thumb(small, &pic);
        assert!(t.rgba[0] > 25, "brightened: {}", t.rgba[0]);
        // The wrong shape is left alone.
        let wide = Image::new(60, 20, (0..60 * 20).flat_map(|_| [10u8, 10, 10, 255]).collect());
        assert_eq!(apply_to_thumb(wide, &pic).rgba[0], 10);
        let _ = std::fs::remove_dir_all(&d);
    }
}
