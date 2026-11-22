//! Extract frames: everything that moves, or has pages, saved as a numbered run of pictures. Every program has the command.
//!
//!  * an animation (GIF / APNG / animated WebP): every frame, composited, with its transparency,
//!  * a video: every frame (ffmpeg writes them; every n-th with the setting),
//!  * a PDF (also `.ai`): every page of the document as a picture,
//!  * a Unity asset bundle: every texture inside it as a picture (with its transparency),
//!  * a 3D model: the turntable from the view on screen: `n` pictures as the model turns once about its up axis, the first one
//!    exactly the view the user has (1 = just that view); the model's shape on transparent where the format keeps alpha.
//!
//! Format, quality and folder are the video snapshot's settings (Settings > Video snapshot), so there is one place to say how
//! pictures are saved. **Transparency comes out**: if any frame has some and the chosen format cannot carry it (JPEG, BMP, TIFF here),
//! the run is saved as PNG instead and the report says so; nothing is flattened onto black by accident.

use crate::prefs::Prefs;
use nv_editor::convert::{self, Out};
use nv_editor::raster::Pixels;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// What a file turns into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Animation,
    Video,
    Pdf,
    /// A Unity asset bundle: its textures.
    Bundle,
    Model,
    /// Nothing to extract: a still picture.
    Still,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Animation => "animation frames",
            Source::Video => "video frames",
            Source::Pdf => "pages",
            Source::Bundle => "textures",
            Source::Model => "turntable views",
            Source::Still => "nothing",
        }
    }
}

/// What kind of source `path` is. A GIF / PNG / WebP counts as an animation only when it really has more than one frame; that needs
/// the file read, which is why this is not free for those three.
pub fn source_of(path: &Path) -> Source {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "pdf" | "ai") {
        return Source::Pdf;
    }
    if nv_codecs::unityfs::is_ext(&ext) {
        return Source::Bundle;
    }
    if nv_codecs::model::EXTS.contains(&ext.as_str()) {
        return Source::Model;
    }
    if nv_codecs::media::media_kind(&ext) == Some(nv_codecs::media::MediaKind::Video) {
        return Source::Video;
    }
    if matches!(ext.as_str(), "gif" | "apng" | "png" | "webp") {
        return match nv_codecs::decode_file(path) {
            Ok(img) if !img.extra_frames.is_empty() => Source::Animation,
            _ => Source::Still,
        };
    }
    Source::Still
}

/// Can `format` (a `snapshot_format` value) carry transparency?
pub fn format_keeps_alpha(format: &str) -> bool {
    matches!(format, "png" | "png16" | "webp" | "qoi" | "tga")
}

fn out_for(format: &str, quality: u8) -> (Out, &'static str) {
    match format {
        "jpg" => (Out::Jpeg(quality), "jpg"),
        "webp" => (Out::Webp, "webp"),
        "bmp" => (Out::Bmp, "bmp"),
        "tiff" => (Out::Tiff, "tif"),
        "qoi" => (Out::Qoi, "qoi"),
        "tga" => (Out::Tga, "tga"),
        _ => (Out::Png, "png"),
    }
}

/// How a run is made.
#[derive(Clone, Debug)]
pub struct Options {
    /// A `snapshot_format` value: png, png16 (saved as PNG), jpg, webp, tiff, bmp.
    pub format: String,
    pub quality: u8,
    /// Where the folder of pictures goes (empty = next to the file); the folder is `<name>_frames`.
    pub base_dir: Option<PathBuf>,
    /// Every n-th frame of an animation or video.
    pub every: u32,
    /// Pictures of a 3D model's turntable (1 = the view on screen only), their size, the view and how it is drawn.
    pub model_frames: u32,
    pub model_size: (u32, u32),
    pub orbit: nv_codecs::model::Orbit,
    pub style: nv_codecs::model::ModelStyle,
    /// PDF pages: long side in pixels, and whether the white paper is drawn (false = transparent where nothing is painted).
    pub pdf_long_side: u32,
    pub pdf_paper: bool,
}

impl Options {
    pub fn from_prefs(p: &Prefs) -> Options {
        let dir = p.snapshot_dir.trim();
        Options {
            format: p.snapshot_format.clone(),
            quality: p.snapshot_quality,
            base_dir: (!dir.is_empty()).then(|| PathBuf::from(dir)),
            every: p.frames_every.max(1),
            model_frames: p.frames_model.max(1),
            model_size: (1600, 1200),
            orbit: nv_codecs::model::Orbit::default(),
            style: nv_codecs::model::ModelStyle::default(),
            pdf_long_side: 2480,
            pdf_paper: false,
        }
    }
}

/// What a run did.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    /// Some picture had transparency, and whether the saved pictures keep it.
    pub had_alpha: bool,
    pub kept_alpha: bool,
    /// The format the pictures really are in (PNG when the chosen one could not carry the transparency).
    pub format: String,
    /// One line for the user.
    pub note: String,
}

impl Report {
    /// "36 pictures in D:\x\cat_frames (transparency kept)".
    pub fn summary(&self) -> String {
        let alpha = if !self.had_alpha {
            ""
        } else if self.kept_alpha {
            " (transparency kept)"
        } else {
            " (transparency flattened)"
        };
        let note = if self.note.is_empty() { String::new() } else { format!(". {}", self.note) };
        format!("{} picture{} in {}{alpha}{note}", self.files.len(), if self.files.len() == 1 { "" } else { "s" }, self.dir.display())
    }
}

/// A folder name that does not exist yet: `<name>_frames`, `<name>_frames (2)` ...
pub fn new_dir_for(path: &Path, opts: &Options) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "frames".into());
    let base = opts.base_dir.clone().or_else(|| path.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let clean: String = stem.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let mut dir = base.join(format!("{clean}_frames"));
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("{clean}_frames ({n})"));
        n += 1;
    }
    dir
}

pub fn has_alpha(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).any(|p| p[3] < 255)
}

/// Writes pictures into the run's folder under one format, switching to PNG once for the whole run when a frame needs the alpha.
struct Sink {
    dir: PathBuf,
    stem: String,
    format: String,
    quality: u8,
    files: Vec<PathBuf>,
    had_alpha: bool,
}

impl Sink {
    fn new(dir: PathBuf, stem: String, opts: &Options, expect_alpha: bool) -> Result<Sink, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut format = if opts.format == "png16" { "png".to_string() } else { opts.format.clone() };
        if expect_alpha && !format_keeps_alpha(&format) {
            format = "png".into();
        }
        Ok(Sink { dir, stem, format, quality: opts.quality, files: Vec::new(), had_alpha: expect_alpha })
    }

    fn put(&mut self, w: u32, h: u32, rgba: Vec<u8>) -> Result<(), String> {
        if has_alpha(&rgba) {
            self.had_alpha = true;
            // A transparent frame turned up after the run started in a format without alpha: the earlier ones stay as they are,
            // the rest go PNG (the report says the transparency was kept only for these).
            if !format_keeps_alpha(&self.format) {
                self.format = "png".into();
            }
        }
        let (out, ext) = out_for(&self.format, self.quality);
        let bytes = convert::encode(&Pixels::new(w, h, rgba), out, [255, 255, 255], None)?;
        let name = format!("{}_{:05}.{ext}", self.stem, self.files.len() + 1);
        let path = self.dir.join(name);
        nv_editor::save::write_atomic(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.files.push(path);
        Ok(())
    }

    fn finish(self, opts: &Options, note: String) -> Report {
        let kept = self.had_alpha && format_keeps_alpha(&self.format);
        let mut note = note;
        if self.had_alpha && self.format != opts.format && !(opts.format == "png16" && self.format == "png") {
            let sep = if note.is_empty() { "" } else { " " };
            note = format!("{note}{sep}Saved as PNG: {} cannot keep transparency.", opts.format.to_uppercase());
        }
        Report { dir: self.dir, files: self.files, had_alpha: self.had_alpha, kept_alpha: kept, format: self.format, note }
    }
}

/// Extract the frames of `path` into a new folder. `progress(done, total)` is called as pictures are written (total 0 = unknown);
/// a `cancel` that turns true stops the run (what was written stays).
pub fn extract(path: &Path, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "frame".into());
    let dir = new_dir_for(path, opts);
    let result = match source_of(path) {
        Source::Animation => animation(path, &stem, dir.clone(), opts, cancel, progress),
        Source::Video => video(path, &stem, dir.clone(), opts, cancel, progress),
        Source::Pdf => pdf(path, &stem, dir.clone(), opts, cancel, progress),
        Source::Bundle => bundle(path, &stem, dir.clone(), opts, cancel, progress),
        Source::Model => model(path, &stem, dir.clone(), opts, cancel, progress),
        Source::Still => Err("This is a single picture: there are no frames to extract.".into()),
    };
    if result.is_err() && dir.is_dir() && std::fs::read_dir(&dir).map(|mut r| r.next().is_none()).unwrap_or(false) {
        let _ = std::fs::remove_dir(&dir); // nothing was written: no empty folder left behind
    }
    result
}

/// Which frame of a file is meant when one picture is wanted (to edit it, say).
pub enum Pick {
    /// Frame `n` (0-based) of an animation.
    Animation(usize),
    /// A video at `ms` (`first_ms`: where Media Foundation counts the first frame from; `shown`: the frame as the player has it,
    /// for when ffmpeg is not there).
    Video { ms: u64, first_ms: u64, shown: Option<crate::snapshot::Shown> },
    /// Page `n` (1-based) of a document.
    Page(u32),
    /// A 3D model as it is seen: the view, how it is drawn and the size of the picture.
    Model { orbit: nv_codecs::model::Orbit, style: nv_codecs::model::ModelStyle, size: (u32, u32) },
}

/// One picture of a file that is not a picture: the frame / page / view `pick` names, saved the way Settings > Video snapshot say
/// (the format falls back to PNG for transparency). This is what NuEditor edits for a video, an animation other than a GIF (a GIF is
/// edited as a whole), a PDF or a 3D model. Returns the file and a note for the user.
pub fn frame_for_editing(src: &Path, pick: Pick, prefs: &Prefs) -> Result<(PathBuf, String), String> {
    use crate::snapshot::{self, Shown};
    let keeps = format_keeps_alpha(if prefs.snapshot_format == "png16" { "png" } else { &prefs.snapshot_format });
    let saved = match pick {
        Pick::Video { ms, first_ms, shown } => snapshot::take(src, ms, first_ms, prefs, shown)?,
        Pick::Animation(n) => {
            let img = nv_codecs::decode_file(src)?;
            let rgba = if n == 0 { img.rgba.clone() } else { img.extra_frames.get(n - 1).map(|f| f.rgba.clone()).unwrap_or_else(|| img.rgba.clone()) };
            snapshot::take_shown_labeled(src, &format!("frame {}", n + 1), prefs, Shown { width: img.width, height: img.height, rgba })?
        }
        Pick::Page(page) => {
            let img = nv_codecs::pdf_render_with(src, page, 2480, !keeps)?;
            snapshot::take_shown_labeled(src, &format!("page {}", img.page.map(|p| p.0).unwrap_or(page)), prefs, Shown { width: img.width, height: img.height, rgba: img.rgba })?
        }
        Pick::Model { orbit, style, size } => {
            let img = nv_codecs::decode_file(src)?;
            let mesh = img.mesh.ok_or("this model has no geometry to draw")?;
            let (w, h) = (size.0.clamp(64, 8192), size.1.clamp(64, 8192));
            let rgba = nv_codecs::model::render_with(&mesh, &orbit, &style, w, h, if keeps { None } else { Some([34, 36, 42]) });
            snapshot::take_shown_labeled(src, "view", prefs, Shown { width: w, height: h, rgba })?
        }
    };
    Ok((saved.path, saved.note))
}

/// The outcome of a run over several files: (file, its report or why it failed).
pub type Results = Vec<(PathBuf, Result<Report, String>)>;

/// Extract the frames of every file in `files` (a file that is a still picture is skipped with a note, not an error that stops the
/// rest). `progress(file index, file count, file name, done, total)`.
pub fn extract_many(files: &[PathBuf], opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize, &str, usize, usize)) -> Results {
    let mut out: Results = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        progress(i, files.len(), &name, 0, 0);
        let r = extract(f, opts, cancel, &mut |d, t| progress(i, files.len(), &name, d, t));
        out.push((f.clone(), r));
    }
    out
}

/// What to tell the user after `extract_many`: one line per file that made pictures, the reasons of the ones that did not.
pub fn summary_many(results: &Results) -> String {
    let mut lines: Vec<String> = Vec::new();
    let (mut pictures, mut failed) = (0usize, 0usize);
    for (path, r) in results {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match r {
            Ok(rep) if rep.files.is_empty() => lines.push(format!("{name}: stopped before the first picture.")),
            Ok(rep) => {
                pictures += rep.files.len();
                lines.push(if results.len() == 1 { rep.summary() } else { format!("{name}: {}", rep.summary()) });
            }
            Err(e) => {
                failed += 1;
                lines.push(format!("{name}: {e}"));
            }
        }
    }
    if results.is_empty() {
        return "Nothing was extracted.".into();
    }
    if results.len() > 1 {
        lines.insert(0, format!("{pictures} picture{} from {} file{}{}.", if pictures == 1 { "" } else { "s" }, results.len() - failed, if results.len() - failed == 1 { "" } else { "s" }, if failed > 0 { format!(", {failed} could not be done") } else { String::new() }));
    }
    lines.join("\n")
}

/// The first picture that was written (to show its folder).
pub fn first_output(results: &Results) -> Option<PathBuf> {
    results.iter().find_map(|(_, r)| r.as_ref().ok().and_then(|rep| rep.files.first().cloned()))
}

fn animation(path: &Path, stem: &str, dir: PathBuf, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    let img = nv_codecs::decode_file(path)?;
    let (w, h) = (img.width, img.height);
    let every = opts.every.max(1) as usize;
    let mut frames: Vec<Vec<u8>> = vec![img.rgba];
    frames.extend(img.extra_frames.into_iter().map(|f| f.rgba));
    let total_all = frames.len();
    let picked: Vec<Vec<u8>> = frames.into_iter().enumerate().filter(|(i, _)| i % every == 0).map(|(_, f)| f).collect();
    // Any frame with transparency decides for the whole run (the pictures of one run share a format).
    let expect_alpha = picked.iter().any(|f| has_alpha(f));
    let mut sink = Sink::new(dir, stem.to_string(), opts, expect_alpha)?;
    let total = picked.len();
    for (i, f) in picked.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        sink.put(w, h, f)?;
        progress(i + 1, total);
    }
    let note = if every > 1 { format!("Every {every}. frame of {total_all}.") } else { String::new() };
    Ok(sink.finish(opts, note))
}

fn pdf(path: &Path, stem: &str, dir: PathBuf, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    // Transparent pages only where the format can carry the transparency; otherwise the white paper (a JPEG of a page is white-backed).
    let paper = opts.pdf_paper || !format_keeps_alpha(if opts.format == "png16" { "png" } else { &opts.format });
    let first = nv_codecs::pdf_render_with(path, 1, opts.pdf_long_side, paper)?;
    let count = first.page.map(|p| p.1).unwrap_or(1).max(1) as usize;
    let expect_alpha = has_alpha(&first.rgba);
    let mut sink = Sink::new(dir, stem.to_string(), opts, expect_alpha)?;
    let (w, h) = (first.width, first.height);
    sink.put(w, h, first.rgba)?;
    progress(1, count);
    for page in 2..=count {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let img = nv_codecs::pdf_render_with(path, page as u32, opts.pdf_long_side, paper)?;
        sink.put(img.width, img.height, img.rgba)?;
        progress(page, count);
    }
    let note = if paper { String::new() } else { "Pages are transparent where nothing is painted.".to_string() };
    Ok(sink.finish(opts, note))
}

/// Every Texture2D of a Unity bundle, in the order the bundle lists them. A texture that can't be decoded (a format this build does
/// not read) is skipped and named in the report; the run only fails when no texture came out at all.
fn bundle(path: &Path, stem: &str, dir: PathBuf, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let c = nv_codecs::unityfs::open(&bytes)?;
    let count = c.textures.len();
    if count == 0 {
        return Err("This bundle holds no textures (it has meshes, scripts or other assets).".into());
    }
    let names = c.texture_names();
    // The first texture decides whether the run needs alpha; later ones with alpha raise it (the Sink checks each picture).
    let mut sink: Option<Sink> = None;
    let mut skipped = Vec::new();
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        match c.texture(i) {
            Ok(t) => {
                if sink.is_none() {
                    sink = Some(Sink::new(dir.clone(), stem.to_string(), opts, has_alpha(&t.rgba))?);
                }
                sink.as_mut().unwrap().put(t.width, t.height, t.rgba)?;
            }
            Err(e) => skipped.push(format!("{}: {e}", names.get(i).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| format!("texture {}", i + 1)))),
        }
        progress(i + 1, count);
    }
    let Some(sink) = sink else { return Err(skipped.first().cloned().unwrap_or_else(|| "no texture could be read".into())) };
    let note = if skipped.is_empty() { String::new() } else { format!("{} texture(s) skipped - {}", skipped.len(), skipped.join("; ")) };
    Ok(sink.finish(opts, note))
}

fn model(path: &Path, stem: &str, dir: PathBuf, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    let img = nv_codecs::decode_file(path)?;
    let mesh = img.mesh.ok_or("this model has no geometry to draw")?;
    let n = opts.model_frames.max(1) as usize;
    let (w, h) = (opts.model_size.0.clamp(64, 8192), opts.model_size.1.clamp(64, 8192));
    // The model's shape on transparent where the format carries alpha, else on the viewer's dark backdrop.
    let keeps = format_keeps_alpha(if opts.format == "png16" { "png" } else { &opts.format });
    let backdrop = if keeps { None } else { Some([34u8, 36, 42]) };
    let mut sink = Sink::new(dir, stem.to_string(), opts, false)?;
    for i in 0..n {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut orbit = opts.orbit;
        orbit.yaw += i as f32 * std::f32::consts::TAU / n as f32;
        let rgba = nv_codecs::model::render_with(&mesh, &orbit, &opts.style, w, h, backdrop);
        sink.put(w, h, rgba)?;
        progress(i + 1, n);
    }
    let note = if n == 1 { "The view on screen.".to_string() } else { format!("{n} views, a turn about the up axis starting at the view on screen.") };
    Ok(sink.finish(opts, note))
}

fn video(path: &Path, stem: &str, dir: PathBuf, opts: &Options, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Report, String> {
    let probe = crate::ffmpeg::probe(path).ok();
    let alpha = probe.as_ref().map(|p| crate::ffmpeg::pix_fmt_has_alpha(&p.pix_fmt)).unwrap_or(false);
    let mut format = if opts.format == "png16" { "png".to_string() } else { opts.format.clone() };
    if alpha && !format_keeps_alpha(&format) {
        format = "png".into();
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (_, ext) = out_for(&format, opts.quality);
    let every = opts.every.max(1);
    let total = probe.as_ref().map(|p| ((p.duration_s * p.fps as f64).round() as usize) / every as usize).unwrap_or(0);
    let written = crate::ffmpeg::extract_frames(path, &dir, stem, &format, ext, opts.quality, every, alpha, total, cancel, progress)?;
    let mut files: Vec<PathBuf> = (1..=written).map(|n| dir.join(format!("{stem}_{n:05}.{ext}"))).collect();
    files.retain(|f| f.is_file());
    let mut note = String::new();
    if every > 1 {
        note = format!("Every {every}. frame.");
    }
    if probe.as_ref().map(|p| p.colour.is_hdr()).unwrap_or(false) {
        note += &format!("{}HDR video: the frames are saved without tone mapping.", if note.is_empty() { "" } else { " " });
    }
    let changed = alpha && format != opts.format && !(opts.format == "png16" && format == "png");
    if changed {
        note += &format!("{}Saved as PNG: {} cannot keep the video's transparency.", if note.is_empty() { "" } else { " " }, opts.format.to_uppercase());
    }
    Ok(Report { dir, files, had_alpha: alpha, kept_alpha: alpha && format_keeps_alpha(&format), format, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_frames_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn opts(format: &str) -> Options {
        Options { format: format.into(), quality: 90, base_dir: None, every: 1, model_frames: 4, model_size: (96, 72), orbit: nv_codecs::model::Orbit::default(), style: nv_codecs::model::ModelStyle::default(), pdf_long_side: 400, pdf_paper: false }
    }

    /// A 2-frame animated GIF, 4x3, frame 1 red and opaque, frame 2 with a transparent left half.
    fn gif_file(path: &Path, transparent: bool) {
        use std::io::Write;
        // A GIF written by hand: global palette of 4 colours, graphic control extensions with a transparent index when asked.
        let mut g: Vec<u8> = b"GIF89a".to_vec();
        g.extend([4, 0, 3, 0, 0b1000_0001, 0, 0]); // 4x3, global table with 4 entries
        g.extend([255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 0, 0]); // red, blue, green, black (index 3 = transparent)
        let frame = |g: &mut Vec<u8>, indices: &[u8], use_transparent: bool| {
            // Disposal 2 (restore to background): what the next frame does not cover is transparent again.
            g.extend([0x21, 0xF9, 4, 0b0000_1000 | u8::from(use_transparent), 10, 0, 3, 0]);
            g.extend([0x2C, 0, 0, 0, 0, 4, 0, 3, 0, 0]);
            // LZW with min code size 2, stored the simple way: clear code, each index, end code (codes of 3 bits, never growing past 8).
            let mut bits: Vec<bool> = Vec::new();
            let push = |bits: &mut Vec<bool>, code: u32| (0..3).for_each(|i| bits.push(code >> i & 1 == 1));
            push(&mut bits, 4); // clear
            for (i, &ix) in indices.iter().enumerate() {
                push(&mut bits, ix as u32);
                if i % 2 == 1 && i + 1 < indices.len() {
                    push(&mut bits, 4); // clear again, so the code size stays 3
                }
            }
            push(&mut bits, 5); // end
            let mut bytes: Vec<u8> = Vec::new();
            for chunk in bits.chunks(8) {
                bytes.push(chunk.iter().enumerate().fold(0u8, |a, (i, &b)| a | ((b as u8) << i)));
            }
            g.push(2);
            for part in bytes.chunks(255) {
                g.push(part.len() as u8);
                g.extend(part);
            }
            g.push(0);
        };
        frame(&mut g, &[0; 12], false);
        let second: Vec<u8> = (0..12).map(|i| if i % 4 < 2 { 3 } else { 1 }).collect();
        frame(&mut g, &second, transparent);
        g.push(0x3B);
        std::fs::File::create(path).unwrap().write_all(&g).unwrap();
    }

    #[test]
    fn a_gif_gives_its_frames_and_transparency_forces_png() {
        let d = scratch("gif");
        let f = d.join("anim.gif");
        gif_file(&f, true);
        assert_eq!(source_of(&f), Source::Animation);
        let mut seen = Vec::new();
        let r = extract(&f, &opts("jpg"), &AtomicBool::new(false), &mut |a, b| seen.push((a, b))).unwrap();
        assert_eq!(r.files.len(), 2);
        assert_eq!(seen, vec![(1, 2), (2, 2)]);
        assert!(r.had_alpha && r.kept_alpha, "a transparent GIF frame must come out with its alpha: {r:?}");
        assert_eq!(r.format, "png", "JPEG cannot carry it, so the run is PNG");
        assert!(r.note.contains("Saved as PNG"), "{}", r.note);
        assert!(r.dir.ends_with("anim_frames"));
        assert!(r.files[0].file_name().unwrap().to_string_lossy().starts_with("anim_00001"));
        let back = nv_codecs::decode_file(&r.files[1]).unwrap();
        assert_eq!(back.rgba[3], 0, "left half of frame 2 is transparent");
        assert_eq!(back.rgba[3 * 4 + 3], 255, "the right half is not");
        // The same run again does not overwrite: a second folder.
        let r2 = extract(&f, &opts("png"), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert!(r2.dir.to_string_lossy().ends_with("anim_frames (2)"), "{}", r2.dir.display());
        assert!(r2.summary().contains("transparency kept"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_opaque_animation_keeps_the_chosen_format_and_every_nth_works() {
        let d = scratch("opaque");
        let f = d.join("o.gif");
        gif_file(&f, false);
        let mut o = opts("jpg");
        let r = extract(&f, &o, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!((r.format.as_str(), r.had_alpha, r.files.len()), ("jpg", false, 2));
        assert!(r.files[0].extension().unwrap() == "jpg");
        o.every = 2;
        let r = extract(&f, &o, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(r.files.len(), 1);
        assert!(r.note.contains("Every 2"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cancelling_stops_the_run_and_a_still_picture_is_refused() {
        let d = scratch("cancel");
        let f = d.join("c.gif");
        gif_file(&f, false);
        let stop = AtomicBool::new(true);
        let r = extract(&f, &opts("png"), &stop, &mut |_, _| {}).unwrap();
        assert!(r.files.is_empty());
        let png = d.join("still.png");
        std::fs::write(&png, convert::encode(&Pixels::new(2, 2, vec![255; 16]), Out::Png, [0; 3], None).unwrap()).unwrap();
        assert_eq!(source_of(&png), Source::Still);
        assert!(extract(&png, &opts("png"), &AtomicBool::new(false), &mut |_, _| {}).is_err());
        assert!(!d.join("still_frames").exists(), "no empty folder is left behind");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_pdf_gives_one_picture_per_page_and_alpha_follows_the_format() {
        let d = scratch("pdf");
        let f = d.join("doc.pdf");
        // Two pages (100 x 50 points); a red square in the lower left of page 1, nothing painted on page 2.
        let objs = [
            "<</Type/Catalog/Pages 2 0 R>>".to_string(),
            "<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 50]/Contents 5 0 R>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 50]/Contents 6 0 R>>".to_string(),
            format!("<</Length {}>>\nstream\n{}\nendstream", "1 0 0 rg 0 0 50 25 re f".len(), "1 0 0 rg 0 0 50 25 re f"),
            "<</Length 0>>\nstream\n\nendstream".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        std::fs::write(&f, out).unwrap();
        assert_eq!(source_of(&f), Source::Pdf);

        // PNG: transparent where nothing is painted, so the alpha comes out.
        let r = extract(&f, &opts("png"), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(r.files.len(), 2, "every page of the document");
        assert!(r.had_alpha && r.kept_alpha && r.format == "png", "{r:?}");
        let p1 = nv_codecs::decode_file(&r.files[0]).unwrap();
        let px = |i: &nv_codecs::Image, x: u32, y: u32| {
            let o = ((y * i.width + x) * 4) as usize;
            [i.rgba[o], i.rgba[o + 1], i.rgba[o + 2], i.rgba[o + 3]]
        };
        assert_eq!(px(&p1, 3 * p1.width / 4, p1.height / 8)[3], 0, "unpainted corner is transparent");
        let red = px(&p1, p1.width / 4, p1.height * 7 / 8);
        assert!(red[0] > 200 && red[1] < 40 && red[3] == 255, "{red:?}");
        let p2 = nv_codecs::decode_file(&r.files[1]).unwrap();
        assert!(p2.rgba.chunks_exact(4).all(|p| p[3] == 0), "page 2 is empty: all transparent");

        // JPEG: no alpha to keep, so the pages are on white paper (not black, not transparent).
        let r = extract(&f, &opts("jpg"), &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!((r.format.as_str(), r.had_alpha), ("jpg", false), "{r:?}");
        let j = nv_codecs::decode_file(&r.files[1]).unwrap();
        assert!(j.rgba.chunks_exact(4).all(|p| p[0] > 240 && p[1] > 240 && p[2] > 240), "an empty page on white");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_unity_bundle_gives_its_textures_with_transparency() {
        // A real bundle from a game's cache; skipped when the folder is not there.
        let Some(home) = std::env::var_os("USERPROFILE") else { return };
        let dir = Path::new(&home).join("AppData/LocalLow/Watcher of Realms/Watcher of Realms/cacheimages");
        let Some(f) = std::fs::read_dir(&dir).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("shop_growthfund_banner3")))) else { return };
        assert_eq!(source_of(&f), Source::Bundle);
        let out = scratch("unity");
        let o = Options { base_dir: Some(out.clone()), ..opts("jpeg") };
        let r = extract(&f, &o, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(r.files.len(), 1);
        assert!(r.had_alpha && r.kept_alpha, "the banner is mostly transparent: PNG is forced");
        assert_eq!(r.format, "png");
        let img = nv_codecs::decode_file(&r.files[0]).unwrap();
        assert_eq!((img.width, img.height), (576, 88));
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn a_model_turntable_starts_at_the_view_on_screen_and_has_a_transparent_background() {
        let d = scratch("model");
        // A tetrahedron as an ASCII STL.
        let stl = "solid t\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 0 10 0\nvertex 0 0 10\nendloop\nendfacet\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 0 0 10\nvertex 10 0 0\nendloop\nendfacet\nfacet normal 0 0 0\nouter loop\nvertex 10 0 0\nvertex 0 0 10\nvertex 0 10 0\nendloop\nendfacet\nendsolid t\n";
        let f = d.join("t.stl");
        std::fs::write(&f, stl).unwrap();
        assert_eq!(source_of(&f), Source::Model);
        let mut o = opts("png");
        o.orbit.yaw = 0.7;
        let r = extract(&f, &o, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!(r.files.len(), 4);
        assert!(r.had_alpha && r.kept_alpha, "the background around the model is transparent: {r:?}");
        let a = nv_codecs::decode_file(&r.files[0]).unwrap();
        assert_eq!(a.rgba[3], 0, "a corner is background");
        assert!(a.rgba.chunks_exact(4).any(|p| p[3] == 255), "the model is drawn");
        // The first picture is exactly the view that was asked for, the second one is not.
        let again = nv_codecs::model::render_with(&nv_codecs::decode_file(&f).unwrap().mesh.unwrap(), &o.orbit, &o.style, 96, 72, None);
        assert_eq!(a.rgba, again);
        let b = nv_codecs::decode_file(&r.files[1]).unwrap();
        assert_ne!(a.rgba, b.rgba);
        // JPEG: a backdrop instead, no alpha.
        let mut oj = o.clone();
        oj.format = "jpg".into();
        let rj = extract(&f, &oj, &AtomicBool::new(false), &mut |_, _| {}).unwrap();
        assert_eq!((rj.format.as_str(), rj.had_alpha), ("jpg", false));
        // One picture = just the view on screen.
        oj.model_frames = 1;
        assert_eq!(extract(&f, &oj, &AtomicBool::new(false), &mut |_, _| {}).unwrap().files.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_frame_to_edit_is_one_picture_in_the_snapshot_format_and_transparency_forces_png() {
        let d = scratch("edit");
        let mut prefs = Prefs::default();
        prefs.snapshot_format = "jpg".into();
        prefs.snapshot_dir = d.join("out").display().to_string();
        // A transparent GIF frame cannot be a JPEG: PNG, and it says so.
        let g = d.join("g.gif");
        gif_file(&g, true);
        let (p, note) = frame_for_editing(&g, Pick::Animation(1), &prefs).unwrap();
        assert_eq!(p.extension().unwrap(), "png");
        assert!(note.contains("PNG"), "{note}");
        assert!(p.file_name().unwrap().to_string_lossy().contains("frame 2"), "{}", p.display());
        assert_eq!(nv_codecs::decode_file(&p).unwrap().rgba[3], 0);
        // An opaque frame keeps the JPEG format.
        let o = d.join("o.gif");
        gif_file(&o, false);
        let (p, note) = frame_for_editing(&o, Pick::Animation(0), &prefs).unwrap();
        assert_eq!((p.extension().unwrap().to_string_lossy().as_ref(), note.as_str()), ("jpg", ""));
        // A model: the view as asked, one picture.
        let m = d.join("m.stl");
        std::fs::write(&m, "solid t\nfacet normal 0 0 0\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 0 10 0\nendloop\nendfacet\nendsolid t\n").unwrap();
        let (p, _) = frame_for_editing(&m, Pick::Model { orbit: nv_codecs::model::Orbit::default(), style: nv_codecs::model::ModelStyle::default(), size: (120, 90) }, &prefs).unwrap();
        let back = nv_codecs::decode_file(&p).unwrap();
        assert_eq!((back.width, back.height), (120, 90));
        assert!(p.file_name().unwrap().to_string_lossy().contains("view"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn several_files_are_done_one_after_another_and_summed_up() {
        let d = scratch("many");
        let (a, b) = (d.join("a.gif"), d.join("b.gif"));
        gif_file(&a, false);
        gif_file(&b, true);
        let still = d.join("s.png");
        std::fs::write(&still, convert::encode(&Pixels::new(2, 2, vec![255; 16]), Out::Png, [0; 3], None).unwrap()).unwrap();
        let mut ticks = Vec::new();
        let results = extract_many(&[a.clone(), still.clone(), b.clone()], &opts("png"), &AtomicBool::new(false), &mut |i, n, name, done, _| ticks.push((i, n, name.to_string(), done)));
        assert_eq!(results.len(), 3);
        assert!(results[0].1.is_ok() && results[1].1.is_err() && results[2].1.is_ok());
        assert!(ticks.iter().any(|t| t.0 == 2 && t.2 == "b.gif"));
        let text = summary_many(&results);
        assert!(text.starts_with("4 pictures from 2 files, 1 could not be done."), "{text}");
        assert!(text.contains("s.png: This is a single picture"));
        assert_eq!(first_output(&results), results[0].1.as_ref().unwrap().files.first().cloned());
        assert_eq!(summary_many(&Vec::new()), "Nothing was extracted.");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn options_follow_the_snapshot_settings() {
        let mut p = Prefs::default();
        p.snapshot_format = "webp".into();
        p.snapshot_quality = 77;
        p.snapshot_dir = "D:\\out".into();
        p.frames_every = 3;
        p.frames_model = 12;
        let o = Options::from_prefs(&p);
        assert_eq!((o.format.as_str(), o.quality, o.every, o.model_frames), ("webp", 77, 3, 12));
        assert_eq!(o.base_dir, Some(PathBuf::from("D:\\out")));
        assert_eq!(new_dir_for(Path::new("C:\\x\\a b.mp4"), &o), PathBuf::from("D:\\out\\a b_frames"));
        assert!(format_keeps_alpha("png16") && format_keeps_alpha("webp") && !format_keeps_alpha("jpg") && !format_keeps_alpha("bmp") && !format_keeps_alpha("tiff"));
    }
}
