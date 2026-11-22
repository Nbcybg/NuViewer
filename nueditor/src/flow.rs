//! Leaving an edit: the "what do you want to keep?" question, the three ways of saving (burn in, new file, XML pair), and
//! handing the user back to the program they came from.

use nv_codecs::Image;
use nv_core::settings::{EditorFormat, EditorSave, Settings};
use nv_editor::convert::Out;
use nv_editor::export::{self, Export};
use nv_editor::recipe::Recipe;
use nv_editor::sidecar::{self, Source};
use nv_editor::{engine, raster, save, Pixels};
use nv_win::instance::{self, OpenRequest, Program};
use nv_win::ui::{Form, ID_CANCEL};
use nv_win::WindowHandle;
use std::path::{Path, PathBuf};

/// Where the user came from, and so where they go back to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    Nowhere,
    Viewer,
    /// Viewer, which itself was opened from the gallery: going back to the viewer keeps that chain.
    ViewerViaGallery,
    Gallery,
}

impl Origin {
    pub fn parse(from: Option<&str>, via_gallery: bool) -> Origin {
        match (from, via_gallery) {
            (Some("viewer"), true) => Origin::ViewerViaGallery,
            (Some("viewer"), false) => Origin::Viewer,
            (Some("gallery"), _) => Origin::Gallery,
            _ => Origin::Nowhere,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    /// Replace the picture (the original is backed up first).
    Burn,
    /// A new file next to it; the original is untouched.
    NewFile,
    /// The recipe as a pair file (.nuedit); the picture is untouched and the edit can be changed later.
    Pair,
    Discard,
    Cancel,
}

const ID_BURN: u16 = 10;
const ID_NEW: u16 = 11;
const ID_PAIR: u16 = 12;
const ID_DISCARD: u16 = 13;

/// The way of saving the settings call the default, as a choice.
pub fn default_choice(d: EditorSave, can_burn: bool) -> Choice {
    match d {
        EditorSave::Pair => Choice::Pair,
        EditorSave::NewFile => Choice::NewFile,
        EditorSave::Burn if can_burn => Choice::Burn,
        EditorSave::Burn => Choice::Pair,
    }
}

/// The question shown when leaving with edits. Enter picks `default` (the factory one is the pair file, which can never lose
/// anything); Esc keeps editing.
pub fn ask(name: &str, can_burn: bool, default: EditorSave) -> Choice {
    let Ok(form) = Form::new("NuEditor - keep your edits?", 440, 262) else { return Choice::Cancel };
    form.label(16, 14, 408, 36, &format!("{name} has edits that are not kept yet."));
    let def = default_choice(default, can_burn);
    form.button(ID_PAIR, 16, 54, 408, 34, "Pair file  -  picture untouched, edit again later", def == Choice::Pair);
    form.button(ID_NEW, 16, 94, 408, 34, "New file  -  save a copy next to it", def == Choice::NewFile);
    form.button(ID_BURN, 16, 134, 408, 34, "Burn in  -  replace the picture (original backed up)", def == Choice::Burn);
    form.enable(ID_BURN, can_burn);
    form.button(ID_DISCARD, 16, 174, 198, 34, "Discard edits", false);
    form.button(ID_CANCEL, 226, 174, 198, 34, "Keep editing", false);
    let mut choice = Choice::Cancel;
    form.run(&mut |_, id| {
        choice = match id {
            ID_BURN => Choice::Burn,
            ID_NEW => Choice::NewFile,
            ID_PAIR => Choice::Pair,
            ID_DISCARD => Choice::Discard,
            _ => Choice::Cancel,
        };
        true
    });
    choice
}

/// Where originals are kept before a picture is rewritten (the same place the viewer and gallery use, so their clean-up covers it).
pub fn backup_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NUEDITOR_BACKUP_DIR") {
        return PathBuf::from(d); // tests
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("undo")
}

/// What was done, for the toast and for the hand-back.
pub struct Saved {
    /// The picture to show next.
    pub show: PathBuf,
    pub message: String,
}

/// The picture the user sees in the editor, full size and still in memory.
pub struct Full<'a> {
    pub path: &'a Path,
    pub pixels: &'a Pixels,
    /// An animated GIF: all its frames (the first is `pixels`) and how long each stays. Saving it writes every frame with the recipe.
    pub anim: Option<&'a [(Pixels, u32)]>,
}

/// The choices (Settings > NuEditor) that decide what a rewritten or copied picture is like.
#[derive(Clone, Debug)]
pub struct CopyOptions {
    pub format: EditorFormat,
    pub quality: u8,
    pub suffix: String,
    pub exif: bool,
}

impl Default for CopyOptions {
    fn default() -> CopyOptions {
        CopyOptions { format: EditorFormat::Same, quality: 95, suffix: "_edit".into(), exif: true }
    }
}

impl CopyOptions {
    pub fn from_settings(s: &Settings) -> CopyOptions {
        let suffix = if s.editor_suffix.trim().is_empty() { "_edit".to_string() } else { s.editor_suffix.clone() };
        CopyOptions { format: s.editor_format, quality: s.editor_quality, suffix, exif: s.editor_exif }
    }

    /// The format a copy of `original` is written in.
    fn out_for(&self, original: &Path) -> Out {
        match self.format {
            EditorFormat::Jpeg => Out::Jpeg(self.quality),
            EditorFormat::Png => Out::Png,
            EditorFormat::Webp => Out::Webp,
            EditorFormat::Bmp => Out::Bmp,
            EditorFormat::Tiff => Out::Tiff,
            EditorFormat::Tga => Out::Tga,
            EditorFormat::Qoi => Out::Qoi,
            EditorFormat::Same => match save::Format::of_path(original) {
                Some(save::Format::Jpeg) => Out::Jpeg(self.quality),
                _ => Out::Png,
            },
        }
    }
}

fn unique_copy_name(original: &Path, suffix: &str, ext: &str) -> PathBuf {
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let dir = original.parent().unwrap_or(Path::new("."));
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}{suffix}.{ext}") } else { format!("{stem}{suffix} ({n}).{ext}") };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

/// The recipe at full size, ready for the export pipeline.
/// The picture with the edits applied, at full size (what a copy would be made from).
pub fn render_pixels(full: &Full, r: &Recipe) -> Result<Pixels, String> {
    render_full(full, r)
}

fn render_full(full: &Full, r: &Recipe) -> Result<Pixels, String> {
    let (w, h) = raster::rotated_size(full.pixels.w, full.pixels.h, r.get(nv_editor::P::Angle));
    if w as u64 * h as u64 > 250_000_000 {
        return Err("the result would be too large".into());
    }
    Ok(engine::render(full.pixels, r))
}

/// An animated GIF with the recipe applied to every frame, and how many frames that is; None when `full` is not an animation.
pub fn gif_bytes(full: &Full, r: &Recipe) -> Result<Option<(Vec<u8>, usize)>, String> {
    let Some(frames) = full.anim.filter(|a| a.len() > 1) else { return Ok(None) };
    let mut done: Vec<nv_editor::gifenc::Frame> = Vec::with_capacity(frames.len());
    for (px, delay) in frames {
        let one = Full { path: full.path, pixels: px, anim: None };
        done.push(nv_editor::gifenc::Frame { pixels: render_full(&one, r)?, delay_ms: *delay });
    }
    let n = done.len();
    Ok(Some((nv_editor::gifenc::encode(&done, true)?, n)))
}

/// Render the recipe at full size and encode it the way `format` says (a JPEG keeps its EXIF block).
fn render_and_encode(full: &Full, r: &Recipe, format: Out, exif: bool) -> Result<Vec<u8>, String> {
    let out = render_full(full, r)?;
    let mut e = Export::new(format);
    e.keep_exif = exif;
    let source = if exif && matches!(format, Out::Jpeg(_)) { std::fs::read(full.path).ok() } else { None };
    Ok(export::run(out, &e, source.as_deref())?.bytes)
}

/// Replace the picture. The original goes to the backup folder first; a failed write puts nothing half-written in its place.
pub fn burn_in(full: &Full, r: &Recipe, o: &CopyOptions) -> Result<Saved, String> {
    // An animated GIF: every frame gets the edits, the file stays a GIF (the original goes to the backup folder first).
    if let Some((bytes, n)) = gif_bytes(full, r)? {
        let path = full.path.to_path_buf();
        let backup = save::backup_copy(&path, &backup_dir()).map_err(|e| format!("no backup possible ({e})"))?;
        if let Err(e) = save::write_atomic(&path, &bytes) {
            let _ = std::fs::remove_file(&backup);
            return Err(format!("{}: {e}", path.display()));
        }
        let _ = nv_editor::journal::add(&backup_dir(), &path, &backup);
        let _ = sidecar::save(&path, &Recipe::default(), &Source::default());
        return Ok(Saved { show: path, message: format!("Burned in: all {n} frames of the GIF are edited - the original is kept in the backup folder for 7 days") });
    }
    let target = save::target_for(full.path, |p| p.exists());
    match target {
        save::Target::Overwrite { path, format } => {
            let backup = save::backup_copy(&path, &backup_dir()).map_err(|e| format!("no backup possible ({e})"))?;
            let kept_backup = backup.clone();
            let out = match format {
                save::Format::Jpeg => Out::Jpeg(o.quality),
                save::Format::Png => Out::Png,
            };
            let bytes = render_and_encode(full, r, out, o.exif).inspect_err(|_| {
                let _ = std::fs::remove_file(&backup);
            })?;
            if let Err(e) = save::write_atomic(&path, &bytes) {
                let _ = std::fs::remove_file(&backup);
                return Err(format!("{}: {e}", path.display()));
            }
            // Noted for the viewer and the editor, so the burn-in can be taken back later (Ctrl+Z / Ctrl+Alt+Z).
            let _ = nv_editor::journal::add(&backup_dir(), &path, &kept_backup);
            // The edit now lives in the pixels: a pair file would apply it a second time.
            let _ = sidecar::save(&path, &Recipe::default(), &Source::default());
            Ok(Saved { show: path, message: "Burned in - the original is kept in the backup folder for 7 days".into() })
        }
        save::Target::Copy { path } => {
            // A format we can't write back (RAW, GIF, ...): the PNG copy is the only honest "burn in".
            let bytes = render_and_encode(full, r, Out::Png, false)?;
            save::write_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(Saved { message: format!("This format can't be rewritten; saved a PNG copy: {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), show: path })
        }
    }
}

/// A new file next to the picture: the format, quality, name suffix and EXIF choice of Settings > NuEditor.
pub fn new_file(full: &Full, r: &Recipe, o: &CopyOptions) -> Result<Saved, String> {
    if let Some((bytes, n)) = gif_bytes(full, r)? {
        let path = unique_copy_name(full.path, &o.suffix, "gif");
        save::write_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return Ok(Saved { show: path, message: format!("Saved a new GIF with all {n} frames edited: {name}") });
    }
    let out = o.out_for(full.path);
    let path = unique_copy_name(full.path, &o.suffix, out.ext());
    let bytes = render_and_encode(full, r, out, o.exif)?;
    save::write_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Saved { show: path, message: format!("Saved a new file: {name}") })
}

/// "Save a copy as": the recipe, then the export pipeline (size, colours, watermark ...), to the path the user chose.
pub fn save_copy(full: &Full, r: &Recipe, e: &Export, dest: &Path) -> Result<Saved, String> {
    let out = render_full(full, r)?;
    let source = if e.keep_exif && matches!(e.format, Out::Jpeg(_)) { std::fs::read(full.path).ok() } else { None };
    let done = export::run(out, e, source.as_deref())?;
    save::write_atomic(dest, &done.bytes).map_err(|e| format!("{}: {e}", dest.display()))?;
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kb = (done.bytes.len() as f64 / 1024.0).round();
    let note = if done.note.is_empty() { String::new() } else { format!(" ({})", done.note) };
    Ok(Saved { show: dest.to_path_buf(), message: format!("Saved {name}: {}x{}, {kb} KB{note}", done.w, done.h) })
}

/// A watermark of `text` as a picture: white letters with a soft dark shadow, so it reads on light and dark parts alike.
pub fn text_mark(text: &str) -> Option<Pixels> {
    nv_win::text::watermark_bitmap(text).map(|(w, h, rgba)| Pixels::new(w, h, rgba))
}

/// The recipe next to the picture; the picture itself is not touched.
pub fn pair_file(full: &Full, r: &Recipe) -> Result<Saved, String> {
    let src = Source::of(full.path, full.pixels.w, full.pixels.h);
    sidecar::save(full.path, r, &src).map_err(|e| format!("{}: {e}", sidecar::sidecar_path(full.path).display()))?;
    Ok(Saved { show: full.path.to_path_buf(), message: "Edits kept in the pair file".into() })
}

// ---------------------------------------------------------------------------------------------------- hand-back

/// A program that sits next to this one.
fn sibling(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.with_file_name(name);
    p.is_file().then_some(p)
}

/// Bring the user back to where they came from, showing `path`. Never blocks: a running program is told, a missing one is started.
pub fn hand_back(origin: Origin, path: &Path) {
    let spawn = |exe: &str, extra: &[&str]| {
        if let Some(exe) = sibling(exe) {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg(path).args(extra);
            if let Ok(child) = cmd.spawn() {
                instance::allow_foreground(child.id());
            }
        }
    };
    let text = path.display().to_string();
    match origin {
        Origin::Nowhere => {}
        Origin::Gallery => {
            let req = OpenRequest { path: Some(text), gallery: true, from_gallery: false };
            if !instance::send_to(Program::Gallery, &req) {
                spawn("nugallery.exe", &[]);
            }
        }
        Origin::Viewer | Origin::ViewerViaGallery => {
            let from_gallery = origin == Origin::ViewerViaGallery;
            let req = OpenRequest { path: Some(text), gallery: false, from_gallery };
            if !instance::send_to(Program::Viewer, &req) {
                spawn("nuviewer.exe", if from_gallery { &["--from-gallery"] } else { &[] });
            }
        }
    }
}

/// The picture decoded for editing: upright sRGB pixels at full size.
pub struct Decoded {
    pub image: Image,
    /// The picture was too big to hold at full size (it was reduced for the GPU), so it can't be rewritten.
    pub reduced_from: Option<(u32, u32)>,
    pub ms: f64,
    /// A RAW file: what the camera recorded (as-shot white balance, ISO ...), read from the file.
    pub camera: Option<String>,
    /// The same, as numbers the Temperature and Tint sliders are based on.
    pub base: Option<nv_editor::rawbase::RawBase>,
    /// An animated GIF: every frame and its delay (ms). The editor shows the first and applies the edits to all of them when saving.
    pub anim: Option<Vec<(Pixels, u32)>>,
}

pub fn decode(path: &Path) -> Result<Decoded, String> {
    let t = std::time::Instant::now();
    let is_gif = path.extension().map(|e| e.eq_ignore_ascii_case("gif")).unwrap_or(false);
    let mut img = if is_gif { nv_codecs::decode_file(path)? } else { nv_codecs::decode_file_first_frame(path)? };
    // A GIF is edited as a whole: keep its frames (unless that is more than the editor should hold in memory).
    let anim = if is_gif && !img.extra_frames.is_empty() && (img.extra_frames.len() as u64 + 1) * img.width as u64 * img.height as u64 * 4 <= 600 << 20 {
        let (w, h, d0) = (img.width, img.height, img.delay_ms);
        let mut frames = vec![(Pixels::new(w, h, img.rgba.clone()), d0)];
        frames.extend(std::mem::take(&mut img.extra_frames).into_iter().map(|f| (Pixels::new(w, h, f.rgba), f.delay_ms)));
        Some(frames)
    } else {
        img.extra_frames.clear();
        None
    };
    let reduced_from = img.reduced_from;
    let image = img.finish_for_cpu();
    let data = nv_codecs::is_raw_ext(&path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()).then(|| nv_codecs::camera_data(path)).flatten();
    let base = data.as_ref().and_then(|c| nv_editor::rawbase::RawBase::from_values(c.kelvin, c.tint, c.iso, c.exposure_bias));
    let camera = data.map(|c| c.summary());
    Ok(Decoded { image, reduced_from, ms: t.elapsed().as_secs_f64() * 1000.0, camera, base, anim })
}

/// Name for dialogs and the title.
pub fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A modal message with the app's window as its parent.
pub fn tell(hwnd: WindowHandle, text: &str) {
    let _ = hwnd;
    nv_win::shell::message(text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        // Backups of the tests go to a scratch folder, not into the user's real backup folder.
        std::env::set_var("NUEDITOR_BACKUP_DIR", std::env::temp_dir().join("nueditor-flow-backups"));
        let d = std::env::temp_dir().join(format!("nueditor-flow-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn picture() -> Pixels {
        Pixels::new(8, 6, (0..8 * 6).flat_map(|i| [(i * 5) as u8, 100, (255 - i * 4) as u8, 255]).collect())
    }

    #[test]
    fn origin_from_the_command_line() {
        assert_eq!(Origin::parse(Some("viewer"), false), Origin::Viewer);
        assert_eq!(Origin::parse(Some("viewer"), true), Origin::ViewerViaGallery);
        assert_eq!(Origin::parse(Some("gallery"), true), Origin::Gallery);
        assert_eq!(Origin::parse(None, false), Origin::Nowhere);
    }

    #[test]
    fn the_three_ways_of_saving_leave_the_right_files() {
        let dir = tmp("save");
        let pic = dir.join("a.png");
        let p = picture();
        std::fs::write(&pic, save::encode(&p, save::Format::Png, None).unwrap()).unwrap();
        let mut r = Recipe::default();
        r.invert = true;
        let full = Full { path: &pic, pixels: &p, anim: None };

        // XML pair: the picture stays byte for byte, a pair file appears and reads back.
        let before = std::fs::read(&pic).unwrap();
        let s = pair_file(&full, &r).unwrap();
        assert_eq!(s.show, pic);
        assert_eq!(std::fs::read(&pic).unwrap(), before);
        assert_eq!(sidecar::load(&pic).unwrap().unwrap().0, r);

        // New file: next to it, the original untouched, and a second save doesn't overwrite the first.
        let n1 = new_file(&full, &r, &CopyOptions::default()).unwrap().show;
        let n2 = new_file(&full, &r, &CopyOptions::default()).unwrap().show;
        assert_ne!(n1, n2);
        assert_eq!(n1.file_name().unwrap(), "a_edit.png");
        assert_eq!(std::fs::read(&pic).unwrap(), before);
        let back = nv_codecs::decode_file(&n1).unwrap();
        assert_eq!(&back.rgba[..4], &[255, 155, 0, 255], "inverted first pixel (0, 100, 255)");

        // Burn in: the picture changes, its pair file goes away (the edit is in the pixels now).
        let saved = burn_in(&full, &r, &CopyOptions::default()).unwrap();
        assert_eq!(saved.show, pic);
        assert_ne!(std::fs::read(&pic).unwrap(), before);
        assert!(!sidecar::has_sidecar(&pic));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_follow_the_options_and_the_export_pipeline() {
        let dir = tmp("options");
        let pic = dir.join("shot.png");
        let p = picture();
        std::fs::write(&pic, save::encode(&p, save::Format::Png, None).unwrap()).unwrap();
        let mut r = Recipe::default();
        r.set(nv_editor::P::Exposure, 0.5);
        let full = Full { path: &pic, pixels: &p, anim: None };

        // Another format, another name suffix.
        let mut o = CopyOptions::default();
        o.format = EditorFormat::Jpeg;
        o.quality = 60;
        o.suffix = "-small".into();
        let jpg = new_file(&full, &r, &o).unwrap().show;
        assert_eq!(jpg.file_name().unwrap(), "shot-small.jpg");
        assert_eq!(&std::fs::read(&jpg).unwrap()[..3], &[0xFF, 0xD8, 0xFF]);
        o.format = EditorFormat::Webp;
        let webp = new_file(&full, &r, &o).unwrap().show;
        assert_eq!(webp.extension().unwrap(), "webp");

        // The export pipeline: half size, 8 colours, to a path the user chose.
        let mut e = nv_editor::export::Export::new(nv_editor::convert::Out::Png);
        e.size = nv_editor::export::SizeMode::Percent(50.0);
        e.colours = Some(8);
        let dest = dir.join("chosen.png");
        let saved = save_copy(&full, &r, &e, &dest).unwrap();
        assert_eq!(saved.show, dest);
        assert!(saved.message.contains("4x3"), "{}", saved.message);
        let back = nv_codecs::decode_file(&dest).unwrap();
        assert_eq!((back.width, back.height), (4, 3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_format_we_cannot_rewrite_gets_a_png_copy() {
        let dir = tmp("copy");
        let pic = dir.join("a.bmp");
        std::fs::write(&pic, b"not really a bmp").unwrap();
        let p = picture();
        let full = Full { path: &pic, pixels: &p, anim: None };
        let mut r = Recipe::default();
        r.invert = true;
        let saved = burn_in(&full, &r, &CopyOptions::default()).unwrap();
        assert_eq!(saved.show.file_name().unwrap(), "a_edited.png");
        assert_eq!(std::fs::read(&pic).unwrap(), b"not really a bmp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A 3-frame GIF (red, green, blue, 6 x 4, 100 ms each) written with the editor's own GIF writer.
    fn make_gif(path: &Path) {
        let frames: Vec<nv_editor::gifenc::Frame> = [[220u8, 30, 30, 255], [30, 200, 30, 255], [30, 30, 220, 255]].iter().map(|c| nv_editor::gifenc::Frame { pixels: Pixels::new(6, 4, (0..24).flat_map(|_| *c).collect()), delay_ms: 100 }).collect();
        std::fs::write(path, nv_editor::gifenc::encode(&frames, false).unwrap()).unwrap();
    }

    #[test]
    fn a_gif_is_edited_as_a_whole_every_frame_gets_the_recipe() {
        let dir = tmp("gif");
        let pic = dir.join("a.gif");
        make_gif(&pic);
        let d = decode(&pic).unwrap();
        let anim = d.anim.expect("an animated GIF keeps its frames");
        assert_eq!(anim.len(), 3);
        assert_eq!(anim[1].1, 100);
        let first = Pixels::new(d.image.width, d.image.height, d.image.rgba.clone());
        let full = Full { path: &pic, pixels: &first, anim: Some(&anim) };
        let mut r = Recipe::default();
        r.invert = true;
        // A new file: three frames, each inverted (red -> cyan, green -> magenta, blue -> yellow).
        let saved = new_file(&full, &r, &CopyOptions::default()).unwrap();
        assert_eq!(saved.show.extension().unwrap(), "gif");
        assert!(saved.message.contains("all 3 frames"), "{}", saved.message);
        let back = nv_codecs::decode_file(&saved.show).unwrap();
        assert_eq!(back.extra_frames.len(), 2);
        let near = |p: &[u8], want: [u8; 3]| (0..3).all(|c| (p[c] as i32 - want[c] as i32).abs() <= 10);
        assert!(near(&back.rgba[..4], [35, 225, 225]));
        assert!(near(&back.extra_frames[0].rgba[..4], [225, 55, 225]));
        assert!(near(&back.extra_frames[1].rgba[..4], [225, 225, 35]));
        assert_eq!(back.delay_ms, 100);
        // Burn in: the GIF itself is replaced (still 3 frames), the original is in the backup folder.
        let saved = burn_in(&full, &r, &CopyOptions::default()).unwrap();
        assert_eq!(saved.show, pic);
        assert!(saved.message.contains("all 3 frames"));
        assert_eq!(nv_codecs::decode_file(&pic).unwrap().extra_frames.len(), 2);
        // A GIF with one frame is not an animation: no frames kept.
        let one = dir.join("one.gif");
        std::fs::write(&one, nv_editor::gifenc::encode(&[nv_editor::gifenc::Frame { pixels: Pixels::new(4, 4, vec![200; 64]), delay_ms: 100 }], false).unwrap()).unwrap();
        assert!(decode(&one).unwrap().anim.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
