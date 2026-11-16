//! Image decoders behind a single entry point. Everything decodes to straight-alpha RGBA8 in sRGB.
//!
//! Formats: JPEG, PNG/APNG, GIF, WebP (incl. animated), PSD / PSB, JPEG XL, JPEG 2000, FLIF, PCX, DICOM, ICNS / ICO / CUR /
//! DIB, camera RAW (embedded preview), and on Windows anything WIC can open (HEIC/HIF/AVIF with the Store extensions,
//! TIFF, BMP, ...).

use std::path::Path;

mod artfmt;
mod blend;
mod codepage;
mod cmyk;
mod develop;
mod dicom;
mod dims;
mod docthumb;
#[cfg(windows)]
mod eps;
pub mod errors;
mod flifdec;
mod gpufit;
mod gifdec;
pub mod icc;
mod icon;
mod imgfmt;
mod jp2;
mod jpeg;
mod jpegmeta;
mod jxldec;
pub mod model;
#[cfg(windows)]
mod metafile;
pub mod metadata;
pub mod midi;
mod ole;
pub mod media;
pub mod metastrip;
mod orient;
mod pcx;
#[cfg(windows)]
mod pdf;
mod pngdec;
mod psddec;
mod psdraw;
pub mod progress;
mod raw;
pub mod resize;
mod sai;
mod sai_keys;
pub mod wave;
pub mod stats;
mod svgdec;
mod swf;
mod webpage;
#[cfg(windows)]
mod wic;
mod webpdec;

pub use dims::peek_size;
pub use codepage::is_code_ext;
pub use svgdec::render_icon;

/// Decode a picture held in memory; `ext` (no dot) says what it is when the bytes alone don't.
pub fn decode_bytes_hint(bytes: &[u8], ext: &str) -> Result<Image, String> {
    decode_image_bytes(bytes, ext)
}

/// Every picture inside a zip-based file (PowerPoint, Word, OpenDocument, zip, cbz, epub ...): (name, bytes).
pub fn zip_pictures(path: &Path, limit: usize) -> Result<Vec<(String, Vec<u8>)>, String> {
    docthumb::pictures_in_zip(path, limit)
}
pub use docthumb::is_document_ext;
pub use gpufit::fit_like_gpu;
pub use resize::downscale;

pub use develop::{camera_data, raw_mode, set_raw_mode, set_raw_thumbs_develop, CameraData, RawMode};
pub use icc::{ColourSettings, Intent, Profile};

/// Colour management settings (see `icc`): on / off, the output profile (None = sRGB), what pictures that carry no
/// profile are taken to be (None = sRGB), what CMYK pictures without one are (None = Coated FOGRA39), the rendering
/// intent and black point compensation. Affects pictures decoded afterwards; on error nothing changes.
pub fn configure_colour(c: icc::ColourSettings) -> Result<(), String> {
    icc::configure(c)
}

/// Would these colour choices be accepted by `configure_colour`? Changes nothing.
pub fn check_colour(c: icc::ColourSettings) -> Result<(), String> {
    icc::check(c)
}

/// Part of a thumbnail cache key: changes when what an untagged picture's numbers mean changes.
pub fn colour_signature() -> u64 {
    // RAW thumbnails made from the sensor data are not the embedded preview's: they must not share cache files.
    icc::thumb_signature() ^ (((develop::raw_thumbs_develop() && develop::raw_mode() != RawMode::Preview) as u64) << 44)
}

static MODEL_TEXTURES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Load the textures of 3D models (a setting). Affects models decoded afterwards.
pub fn set_model_textures(on: bool) {
    MODEL_TEXTURES.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn model_textures_enabled() -> bool {
    MODEL_TEXTURES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Pixels on the long side when vector art or a document page is rasterised for full-size viewing: the screen's long
/// side plus half again (room to zoom a little), within sensible bounds.
pub(crate) fn render_long_side() -> u32 {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let screen = unsafe { GetSystemMetrics(SM_CXSCREEN).max(GetSystemMetrics(SM_CYSCREEN)) }.max(800) as u32;
        (screen * 3 / 2).clamp(2048, 6000)
    }
    #[cfg(not(windows))]
    {
        3000
    }
}
#[cfg(windows)]
pub use pdf::{page_path, split_page};

/// Documents have no pages on other platforms; keeps callers portable.
#[cfg(not(windows))]
pub fn page_path(path: &Path, _page: u32) -> std::path::PathBuf {
    path.to_path_buf()
}
#[cfg(not(windows))]
pub fn split_page(path: &Path) -> (std::path::PathBuf, u32) {
    (path.to_path_buf(), 1)
}

/// A thumbnail source JPEG is decoded at no less than this many pixels on the long side (the thumbnails are 256: this is
/// twice that, so the final shrink still averages real pixels).
const THUMB_SOURCE_EDGE: u32 = 512;

/// Largest texture dimension the renderer can create (D3D11 feature level 11_0).
pub const GPU_MAX_DIM: u32 = 16384;
/// Pixel budget for one displayed image (RGBA + mip chain is ~5.3 bytes/pixel on the GPU).
pub const GPU_MAX_PIXELS: u64 = 120_000_000;
/// Anything bigger than this is refused before any pixel memory is reserved (decompression-bomb guard).
pub const MAX_DECODE_PIXELS: u64 = 250_000_000;

/// Refuse absurd dimensions up front, from the header, so we never try to allocate gigabytes.
pub(crate) fn check_size(w: u64, h: u64) -> Result<(), String> {
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    if w * h > MAX_DECODE_PIXELS {
        return Err(format!("image too large to open ({w}x{h}, limit {} megapixels)", MAX_DECODE_PIXELS / 1_000_000));
    }
    Ok(())
}

/// Animations are truncated once their decoded frames would exceed this many bytes.
pub const MAX_ANIM_BYTES: usize = 512 * 1024 * 1024;

pub struct Frame {
    /// Same dimensions as the parent `Image`; fully composited (not a sub-rectangle).
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, row-major. For animations this is the first frame.
    pub rgba: Vec<u8>,
    /// Display time of the first frame (only meaningful when `extra_frames` is non-empty).
    pub delay_ms: u32,
    /// Frames after the first, for animated images.
    pub extra_frames: Vec<Frame>,
    /// Set when the image was scaled down to fit GPU limits: the original (width, height).
    pub reduced_from: Option<(u32, u32)>,
    /// EXIF orientation 1..=8 that still has to be applied. The renderer does it on the GPU; anything that needs
    /// upright pixels on the CPU calls `oriented()`. `width`/`height` are the stored (unrotated) size.
    pub orientation: u8,
    /// An embedded ICC profile whose conversion to sRGB is still pending (see `apply_icc`).
    pub icc: Option<Vec<u8>>,
    /// A photo-style bitmap (JPEG, PNG, WebP): when it carries no profile the user's "pictures without a profile"
    /// choice applies. Renders (SVG, PDF, ...) are always sRGB.
    pub profile_assumable: bool,
    /// The pixel buffer holds raw CMYK ink amounts (byte 0 = C ... byte 3 = K, 0 = no ink) that still have to be converted
    /// to RGB through a profile (see `cmyk`). Every `apply_*` / `finish_for_*` below does that; until then the buffer is
    /// not colours, so nothing may display or save it. `icc` then holds the CMYK profile of the file, if it has one.
    pub cmyk: bool,
    /// One line about how the colours were read, for the info panel (set when a CMYK picture is converted).
    pub colour_note: Option<String>,
    /// The picture had no profile and the user's CMYK choice was assumed: the viewer says so.
    pub colour_assumed: bool,
    /// How long the stages of decoding this picture took, in ms: ("decode", ..) (reading included), ("shrink", ..),
    /// ("colour", ..). For the info panel; the process-wide table is in `stats`.
    pub stages: Vec<(&'static str, f32)>,
    /// (page, page count) when this picture is one page of a multi-page document (PDF).
    pub page: Option<(u32, u32)>,
    /// For 3D models: the geometry (`pixels` hold a software-rendered preview). The viewer draws the mesh itself.
    pub mesh: Option<std::sync::Arc<model::Mesh>>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Image { width, height, rgba, delay_ms: 0, extra_frames: Vec::new(), reduced_from: None, orientation: 1, icc: None, profile_assumable: false, cmyk: false, colour_note: None, colour_assumed: false, stages: Vec::new(), page: None, mesh: None }
    }

    pub fn frame_count(&self) -> usize {
        1 + self.extra_frames.len()
    }

    /// Size as it should be shown, i.e. after applying `orientation`.
    pub fn display_size(&self) -> (u32, u32) {
        if self.orientation >= 5 { (self.height, self.width) } else { (self.width, self.height) }
    }

    /// Convert pixels that carry their own ICC profile to sRGB (what files, the clipboard and 3D textures need). Pictures
    /// without a profile are left alone (CMYK ones are read as the user's CMYK choice: they have to become RGB). No-op if
    /// there's nothing pending, or if colour management is switched off.
    pub fn apply_icc(&mut self) {
        self.colour(false, icc::Target::Srgb);
    }

    /// Like `apply_icc`, but a photo without a profile is first read as what the user said such pictures are
    /// (thumbnails are cached as sRGB, so this is their conversion).
    pub fn apply_icc_assumed(&mut self) {
        self.colour(true, icc::Target::Srgb);
    }

    /// Bring the pixels to the output profile (the screen): from their own profile, the assumed one for a photo
    /// without a profile, or sRGB. Does nothing with the default settings and an sRGB picture.
    pub fn apply_display_colour(&mut self) {
        self.colour(true, icc::Target::Display);
    }

    /// sRGB pixels (a cached thumbnail) to the output profile.
    pub fn srgb_to_output(&mut self) {
        self.convert(icc::srgb_to_output());
    }

    fn colour(&mut self, assumable: bool, target: icc::Target) {
        let icc = self.icc.take();
        if self.cmyk {
            self.cmyk = false;
            if self.mesh.is_none() {
                self.convert_cmyk(icc.as_deref(), target);
            }
            return;
        }
        self.convert(icc::transform_for(icc.as_deref(), assumable && self.profile_assumable, target));
    }

    /// Raw CMYK -> RGB (all frames), through the picture's own profile or the user's CMYK choice.
    fn convert_cmyk(&mut self, profile: Option<&[u8]>, target: icc::Target) {
        let outcome = cmyk::convert(&mut self.rgba, profile, target);
        for f in &mut self.extra_frames {
            cmyk::convert(&mut f.rgba, profile, target);
        }
        self.colour_note = Some(outcome.note);
        self.colour_assumed = outcome.assumed;
    }

    fn convert(&mut self, t: Option<icc::Transform>) {
        let Some(t) = t else { return };
        if self.mesh.is_some() {
            return; // a 3D preview is lit by the viewer itself
        }
        icc::apply(&mut self.rgba, &t);
        for f in &mut self.extra_frames {
            icc::apply(&mut f.rgba, &t);
        }
    }

    /// Rotate/flip the pixels per `orientation` on the CPU. Only needed when upright pixels are required
    /// (thumbnails, clipboard); the viewer lets the GPU do it.
    pub fn oriented(self) -> Image {
        let o = self.orientation as u32;
        if (2..=8).contains(&o) { orient::apply(self, o) } else { self }
    }

    /// Like `oriented`, but borrowing (copies the pixels when a rotation is needed).
    pub fn to_oriented(&self) -> Image {
        if (2..=8).contains(&(self.orientation as u32)) {
            Image { rgba: self.rgba.clone(), extra_frames: Vec::new(), icc: None, page: self.page, mesh: self.mesh.clone(), colour_note: self.colour_note.clone(), stages: Vec::new(), ..*self }.oriented()
        } else {
            Image { rgba: self.rgba.clone(), extra_frames: Vec::new(), icc: None, page: self.page, mesh: self.mesh.clone(), colour_note: self.colour_note.clone(), stages: Vec::new(), ..*self }
        }
    }

    /// Upright sRGB pixels for a thumbnail (a photo without a profile is read as the user's assumed profile).
    pub fn finish_for_thumb(mut self) -> Image {
        self.apply_icc_assumed();
        self.oriented()
    }

    /// Upright sRGB pixels, ready for CPU consumers.
    pub fn finish_for_cpu(mut self) -> Image {
        self.apply_icc();
        self.oriented()
    }

    /// Build an image from fully composited frames `(rgba, delay_ms)`.
    pub(crate) fn from_frames(width: u32, height: u32, frames: Vec<(Vec<u8>, u32)>) -> Option<Self> {
        let mut it = frames.into_iter();
        let (rgba, delay_ms) = it.next()?;
        Some(Image { width, height, rgba, delay_ms, extra_frames: it.map(|(rgba, delay_ms)| Frame { rgba, delay_ms }).collect(), reduced_from: None, orientation: 1, icc: None, profile_assumable: false, cmyk: false, colour_note: None, colour_assumed: false, stages: Vec::new(), page: None, mesh: None })
    }
}

const RAW_EXTS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "dng", "orf", "rw2", "raf", "pef", "srw", "3fr", "erf", "kdc", "mrw", "raw", "rwl", "iiq", "x3f",
];
/// RAW extensions that are TIFF containers (everything in `RAW_EXTS` except RAF, CR3, CRW and MRW).
const TIFF_RAW_EXTS: &[&str] = &["cr2", "nef", "nrw", "arw", "sr2", "srf", "dng", "orf", "rw2", "pef", "srw", "3fr", "erf", "kdc", "raw", "rwl", "iiq"];
/// Formats we hand straight to WIC.
const WIC_EXTS: &[&str] = &["heic", "heif", "hif", "avif", "tif", "tiff", "bmp", "ico", "jxr", "wdp", "hdp"];
const NATIVE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "psd"];
/// Formats with a decoder of our own (see `decode_more`): Photoshop large documents, JPEG XL, JPEG 2000, FLIF, Apple icons,
/// cursors, bare Windows bitmaps, PCX and DICOM.
const MORE_EXTS: &[&str] = &["psb", "jxl", "jp2", "j2k", "j2c", "jpf", "jpx", "jpc", "flif", "icns", "cur", "dib", "pcx", "dcm", "dicom"];
/// Formats that are listed in folders but not registered as file types: ECW (no decoder we can ship; Windows' codecs are
/// asked, then the error says what to do).
const TRY_EXTS: &[&str] = &["ecw"];
/// Flash: movies (`swf`: the largest bitmap inside, else a card with the header's facts) and authoring files (`fla`).
const FLASH_EXTS: &[&str] = &["swf", "fla"];
/// PDF and PDF-compatible Illustrator files, rendered by Windows (`Windows.Data.Pdf`).
const DOC_EXTS: &[&str] = &["pdf", "ai"];
/// SVG, rendered by resvg.
const SVG_EXTS: &[&str] = &["svg", "svgz"];
/// Windows metafiles, played back by GDI+.
const META_EXTS: &[&str] = &["emf", "wmf"];
/// Encapsulated PostScript (Ghostscript if installed, else the preview inside the file).
const EPS_EXTS: &[&str] = &["eps"];

/// Blender files: the preview picture saved inside.
const BLEND_EXTS: &[&str] = &["blend", "blend1"];

/// Every extension we can open (lowercase, no dot): what the file-association registration lists.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = NATIVE_EXTS.iter().chain(RAW_EXTS.iter()).chain(MORE_EXTS.iter()).chain(model::EXTS.iter()).chain(imgfmt::EXTS.iter()).chain(BLEND_EXTS.iter()).chain(artfmt::EXTS.iter()).copied().collect();
    if cfg!(windows) {
        v.extend(WIC_EXTS);
        v.extend(DOC_EXTS);
        v.extend(META_EXTS);
        v.extend(EPS_EXTS);
    }
    v.extend(SVG_EXTS);
    v.extend(FLASH_EXTS);
    v.extend(webpage::EXTS);
    v.extend(media::VIDEO_EXTS);
    v.extend(media::AUDIO_EXTS);
    v.extend(media::PRO_EXTS);
    v
}

/// Extensions of still pictures that can be converted to PNG / JPEG: everything we open except video, sound, 3D models,
/// documents and Blender files.
pub fn convertible_extensions() -> Vec<&'static str> {
    let skip = |e: &str| media::media_kind(e).is_some() || e == "duf" || e == "lip" || e == "oci" || model::EXTS.contains(&e) || DOC_EXTS.contains(&e) || BLEND_EXTS.contains(&e) || EPS_EXTS.contains(&e) || TRY_EXTS.contains(&e) || FLASH_EXTS.contains(&e) || webpage::is_ext(e);
    supported_extensions().into_iter().filter(|e| !skip(e)).collect()
}

/// Is this a camera RAW format (listed separately from ordinary pictures in the gallery's filters)?
pub fn is_raw_ext(ext: &str) -> bool {
    RAW_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn is_supported_ext(ext: &str) -> bool {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    NATIVE_EXTS.contains(&e) || RAW_EXTS.contains(&e) || MORE_EXTS.contains(&e) || TRY_EXTS.contains(&e) || FLASH_EXTS.contains(&e) || webpage::is_ext(e) || model::EXTS.contains(&e) || imgfmt::is_ext(e) || artfmt::is_ext(e) || (cfg!(windows) && (WIC_EXTS.contains(&e) || DOC_EXTS.contains(&e) || META_EXTS.contains(&e) || EPS_EXTS.contains(&e))) || SVG_EXTS.contains(&e) || BLEND_EXTS.contains(&e) || media::media_kind(e).is_some()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Jpeg,
    Png,
    Gif,
    Webp,
    Psd,
    Raw,
    Wic,
    Jxl,
    Jp2,
    Flif,
    Icns,
    Dicom,
}

fn sniff(b: &[u8]) -> Option<Kind> {
    if b.starts_with(&[0xFF, 0xD8]) {
        Some(Kind::Jpeg)
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(Kind::Png)
    } else if b.starts_with(b"GIF8") {
        Some(Kind::Gif)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else if b.starts_with(b"8BPS") {
        Some(Kind::Psd)
    } else if b.len() >= 12 && &b[4..8] == b"ftyp" {
        Some(if &b[8..12] == b"crx " { Kind::Raw } else { Kind::Wic })
    } else if b.starts_with(b"FUJIFILMCCD-RAW") {
        Some(Kind::Raw)
    } else if jxldec::is_jxl(b) {
        Some(Kind::Jxl)
    } else if jp2::is_jp2(b) {
        Some(Kind::Jp2)
    } else if flifdec::is_flif(b) {
        Some(Kind::Flif)
    } else if b.starts_with(b"icns") {
        Some(Kind::Icns)
    } else if dicom::is_dicom(b) {
        Some(Kind::Dicom)
    } else if b.starts_with(b"BM") || b.starts_with(&[0, 0, 1, 0]) || b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        Some(Kind::Wic)
    } else {
        None
    }
}

/// True if the file starts with a PDF header (within the first KB, as the spec allows some leading junk).
#[cfg(windows)]
fn looks_like_pdf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 1024];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    head[..n].windows(5).any(|w| w == b"%PDF-")
}

/// A quick first picture for a big JPEG: only as big as a window of `win` pixels needs it (colour-converted, orientation carried), or
/// `None` when the file is not a JPEG of 24 megapixels or more, or can't be decoded that way. The full decode follows separately.
pub fn decode_jpeg_for_window(path: &Path, win: (u32, u32)) -> Option<Image> {
    let ext = path.extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "jpe" | "jfif") {
        return None;
    }
    // Only pictures big enough to gain: below about 24 megapixels the full decode is as quick (measured: 6 MP 78 ms against 99 ms
    // scaled, 18 MP 103 against 127, 30 MP 293 against 136).
    let (w, h) = peek_size(path)?;
    if (w as u64) * (h as u64) < 24_000_000 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut img = std::panic::catch_unwind(|| jpeg::decode_scaled_for_window(&bytes, win)).ok()??;
    img.apply_display_colour();
    check_buffers(img).ok()
}

/// Decode `path` in full (all animation frames), shrunk if needed to fit what the GPU can show.
pub fn decode_file(path: &Path) -> Result<Image, String> {
    // Order matters for speed: shrink first, then colour-convert, so a 100-megapixel panorama is converted at the
    // size it will actually be shown (and orientation is left to the GPU).
    let t0 = std::time::Instant::now();
    let decoded = decode_file_opts(path, false, false).and_then(check_buffers);
    let decode_ms = t0.elapsed().as_secs_f64() * 1000.0;
    decoded
        .map(|img| {
            let t = std::time::Instant::now();
            let mut img = fit_gpu_limits(img);
            let shrink_ms = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            img.apply_display_colour();
            let colour_ms = t.elapsed().as_secs_f64() * 1000.0;
            img.stages = vec![("decode", decode_ms as f32), ("shrink", shrink_ms as f32), ("colour", colour_ms as f32)];
            record_stats("view", path, &img, decode_ms + shrink_ms + colour_ms);
            stats::record_stage("shrink to GPU limits", shrink_ms);
            stats::record_stage("colour conversion", colour_ms);
            img
        })
        .map_err(|e| with_code(path, e))
}

/// Add a finished decode to the process-wide table (`stats`).
fn record_stats(kind: &str, path: &Path, img: &Image, ms: f64) {
    let (real, _) = split_page(path);
    let ext = real.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let bytes = std::fs::metadata(&real).map(|m| m.len()).unwrap_or(0);
    stats::record_decode(kind, &ext, ms, img.width, img.height, bytes);
}

/// Attach the error code (see `errors`) to a decode failure of `path`.
fn with_code(path: &Path, message: String) -> String {
    let (real, _) = split_page(path);
    let ext = real.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if ext == "ecw" {
        return errors::tag(2004, &message); // a format only its maker's component can read
    }
    errors::coded(&message, &ext)
}

/// Scale a still image down so it fits `GPU_MAX_DIM` and `GPU_MAX_PIXELS` (records the original size in
/// `reduced_from`). Animations are left alone. Images within limits are returned untouched.
pub fn fit_gpu_limits(img: Image) -> Image {
    fit_within(img, GPU_MAX_DIM, GPU_MAX_PIXELS)
}

/// `fit_gpu_limits` with explicit limits (so tests can use tiny ones instead of allocating gigabytes).
fn fit_within(img: Image, max_dim: u32, max_pixels: u64) -> Image {
    if !img.extra_frames.is_empty() {
        return img;
    }
    let (w, h) = (img.width as f64, img.height as f64);
    let s = (max_dim as f64 / w.max(h)).min((max_pixels as f64 / (w * h)).sqrt()).min(1.0);
    if s >= 1.0 {
        return img;
    }
    let edge = ((w.max(h) * s).floor() as u32).max(1);
    let mut out = downscale(&img, edge);
    out.reduced_from = Some((img.width, img.height));
    out.page = img.page;
    out.mesh = img.mesh.clone();
    out
}

/// Decode only the first frame of animations (cheap; used for thumbnails).
pub fn decode_file_first_frame(path: &Path) -> Result<Image, String> {
    let t0 = std::time::Instant::now();
    let r = decode_file_opts(path, true, false).and_then(check_buffers);
    if let Ok(img) = &r {
        record_stats("first frame", path, img, t0.elapsed().as_secs_f64() * 1000.0);
    }
    r.map_err(|e| with_code(path, e))
}

/// A picture to make a thumbnail from (the gallery, the film strip): the first frame, but only as big as a thumbnail needs
/// it to be. A JPEG is decoded with its DCT scaled down (so the result can be smaller than the file's size, never smaller
/// than twice the thumbnail); a RAW file gives its embedded preview, unless the setting asks for developing thumbnails too,
/// then a half-size development. Use `decode_file_first_frame` when the whole picture is wanted (clipboard).
pub fn decode_for_thumbnail(path: &Path) -> Result<Image, String> {
    let t0 = std::time::Instant::now();
    let r = decode_file_opts(path, true, true).and_then(check_buffers);
    if let Ok(img) = &r {
        record_stats("thumb", path, img, t0.elapsed().as_secs_f64() * 1000.0);
    }
    r.map_err(|e| with_code(path, e))
}

/// Every decoder must hand back `width * height * 4` bytes per frame; the downscaler, the CPU preview and the GPU
/// upload all index by that. A decoder that gets it wrong (this once crashed the viewer on greyscale JPEGs) becomes
/// an error message instead of a panic or a read past the end of the buffer.
pub(crate) fn check_buffers(img: Image) -> Result<Image, String> {
    let want = img.width as usize * img.height as usize * 4;
    if img.rgba.len() != want {
        return Err(format!("decoder produced {} bytes for {}x{} (expected {want})", img.rgba.len(), img.width, img.height));
    }
    if img.extra_frames.iter().any(|f| f.rgba.len() != want) {
        return Err("decoder produced an animation frame of the wrong size".into());
    }
    Ok(img)
}

fn decode_file_opts(path: &Path, first_only: bool, thumb: bool) -> Result<Image, String> {
    // A page of a multi-page document is addressed as "file.pdf?page=N": peel that off first.
    let (real_path, page) = split_page(path);
    let path = real_path.as_path();
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    // Archives, Office / OpenDocument files, Affinity files and text: the picture inside, or a card (never an error).
    if docthumb::is_document_ext(&ext) {
        return docthumb::decode(path, first_only);
    }
    if media::media_kind(&ext).is_some() {
        return media::decode(path, first_only);
    }
    if BLEND_EXTS.contains(&ext.as_str()) {
        return blend::decode(path);
    }
    if ext == "ico" {
        // Windows' own icon codec first; our reader for the odd files it refuses.
        return decode_wic(path).or_else(|e| progress::read_file(path).and_then(|b| icon::decode_ico(&b)).map_err(|_| e));
    }
    if WIC_EXTS.contains(&ext.as_str()) {
        return decode_wic(path);
    }
    if webpage::is_ext(&ext) {
        return webpage::decode(path, first_only);
    }
    if ext == "swf" {
        return swf::decode_or_card(path, &progress::read_file(path)?, first_only);
    }
    if ext == "fla" {
        return swf::decode_fla(path, first_only);
    }
    if TRY_EXTS.contains(&ext.as_str()) {
        // Nothing we can ship decodes these: Windows might have a codec installed for them, otherwise say what to do.
        return decode_wic(path).map_err(|e| format!("unsupported format: .{ext} needs a codec from its maker ({e}). Convert it first, for example with GDAL: gdal_translate in.{ext} out.jp2"));
    }
    // Photoshop: only the header sections and the merged picture are read, never the layers.
    if ext == "psd" || ext == "psb" {
        if let Ok(img) = psddec::decode_file(path) {
            return Ok(img);
        }
    }
    if let Some(r) = artfmt::decode(path, &ext, first_only) {
        return r;
    }
    if MORE_EXTS.contains(&ext.as_str()) {
        let bytes = progress::read_file(path)?;
        return decode_more(&bytes, &ext, first_only);
    }
    #[cfg(windows)]
    if DOC_EXTS.contains(&ext.as_str()) {
        // Illustrator files saved without "PDF compatibility" are PostScript, like EPS.
        if ext == "ai" && !looks_like_pdf(path) {
            return eps::decode(path, first_only);
        }
        return pdf::render(path, page, first_only);
    }
    #[cfg(windows)]
    if EPS_EXTS.contains(&ext.as_str()) {
        return eps::decode(path, first_only);
    }
    #[cfg(windows)]
    if META_EXTS.contains(&ext.as_str()) {
        return metafile::decode_file(path, first_only);
    }
    let _ = page;
    if model::EXTS.contains(&ext.as_str()) {
        return model::decode(path, first_only);
    }
    if SVG_EXTS.contains(&ext.as_str()) {
        let bytes = progress::read_file(path)?;
        return svgdec::decode(&bytes, first_only);
    }
    // TGA, DDS, PNM, HDR, EXR, QOI.
    if imgfmt::is_ext(&ext) {
        let bytes = progress::read_file(path)?;
        return imgfmt::decode(&bytes, &ext);
    }
    // Camera RAW: developed from the sensor data (what the camera recorded, with only what the camera wrote into the file
    // applied), unless the user chose the embedded preview. Thumbnails only need a small picture: half size. A file that
    // can't be developed (an unusual sensor, a damaged file) falls through to its embedded preview, as before.
    if RAW_EXTS.contains(&ext.as_str()) {
        let mode = develop::raw_mode();
        if mode != RawMode::Preview && (!thumb || develop::raw_thumbs_develop()) {
            if let Some(img) = catch_develop(path, if thumb { RawMode::Half } else { mode }) {
                return Ok(img);
            }
        }
    }
    // Fujifilm RAF: the preview JPEG is ~10% of the file, so read just that range instead of 56 MB (a big win on
    // slow disks). Anything unexpected falls through to the whole-file path below.
    if ext == "raf" {
        if let Some(jpeg_bytes) = raw::read_raf_preview(path) {
            if let Ok(img) = jpeg::decode(&jpeg_bytes, None) {
                return Ok(img);
            }
        }
    }
    // TIFF-style RAWs: walk the IFD tree with a few small reads and fetch only the chosen preview JPEG. `None` (a big
    // sensor with only a thumbnail listed, an unfamiliar layout, ...) means "scan the whole file", as before.
    if TIFF_RAW_EXTS.contains(&ext.as_str()) {
        if let Some((jpeg_bytes, orientation)) = raw::read_tiff_preview(path) {
            if let Ok(img) = jpeg::decode(&jpeg_bytes, orientation) {
                return Ok(img);
            }
        }
        // No big enough preview listed in the IFDs (e.g. Sony ARW, Adobe DNG with a small preview): scan everything
        // except the sensor-data block, which gives the same answer as scanning the whole file.
        if let Some(img) = raw::decode_tiff_skipping_sensor(path) {
            // A thumbnail-sized preview (Adobe DNG's 0.7 MP) looks blurry full screen: develop the sensor data at
            // half size instead, when that gives a clearly bigger picture.
            if !thumb && develop::raw_mode() == RawMode::Preview && (img.width as u64 * img.height as u64) < DEVELOP_BELOW_PX {
                if let Some(dev) = try_develop(path, &img) {
                    return Ok(dev);
                }
            }
            return Ok(img);
        }
    }
    let bytes = progress::read_file(path)?;
    if bytes.is_empty() {
        return Err("empty file".into());
    }
    if RAW_EXTS.contains(&ext.as_str()) {
        // Embedded preview first (fast); if there isn't one, let Windows' RAW codec try.
        return raw::decode_preview(&bytes).or_else(|e| {
            // No usable preview at all: develop the sensor data ourselves before asking Windows' RAW codec.
            if !thumb && develop::raw_mode() == RawMode::Preview {
                if let Some(dev) = catch_develop(path, RawMode::Half) {
                    return Ok(dev);
                }
            }
            decode_wic(path).map_err(|w| format!("{e}; {w}"))
        });
    }
    match sniff(&bytes) {
        Some(Kind::Jpeg) => thumb.then(|| jpeg::decode_scaled(&bytes, THUMB_SOURCE_EDGE, None)).flatten().map(Ok).unwrap_or_else(|| jpeg::decode(&bytes, None)),
        Some(Kind::Png) => pngdec::decode(&bytes, first_only),
        Some(Kind::Gif) => gifdec::decode(&bytes, first_only),
        Some(Kind::Webp) => webpdec::decode(&bytes, first_only),
        Some(Kind::Psd) => psddec::decode(&bytes),
        Some(Kind::Raw) => raw::decode_preview(&bytes),
        Some(Kind::Wic) => decode_wic(path),
        Some(Kind::Jxl) => jxldec::decode(&bytes, first_only),
        Some(Kind::Jp2) => jp2::decode(&bytes),
        Some(Kind::Flif) => flifdec::decode(&bytes),
        Some(Kind::Icns) => icon::decode_icns(&bytes),
        Some(Kind::Dicom) => dicom::decode(&bytes),
        // A known extension whose contents match none of the formats: the file is not what its name says.
        None if is_supported_ext(&ext) => Err(format!("not a valid .{ext} picture (unrecognised header)")),
        None => Err(format!("unsupported format: .{ext}")),
    }
}

/// The formats with a decoder of our own, by extension (`MORE_EXTS`).
fn decode_more(bytes: &[u8], ext: &str, first_only: bool) -> Result<Image, String> {
    if bytes.is_empty() {
        return Err("empty file".into());
    }
    match ext {
        "psb" => psddec::decode(bytes),
        "jxl" => jxldec::decode(bytes, first_only),
        "jp2" | "j2k" | "j2c" | "jpf" | "jpx" | "jpc" => jp2::decode(bytes),
        "flif" => flifdec::decode(bytes),
        "icns" => icon::decode_icns(bytes),
        "cur" => icon::decode_ico(bytes),
        "dib" => icon::decode_dib_file(bytes),
        "pcx" => pcx::decode(bytes),
        "dcm" | "dicom" => dicom::decode(bytes),
        _ => Err(format!("unsupported format: .{ext}")),
    }
}

/// Decode a picture held in memory (a texture inside a model file). `ext_hint` helps with formats that have no
/// magic number (TGA, ...); anything recognisable by its first bytes is decoded by what it is.
pub(crate) fn decode_image_bytes(bytes: &[u8], ext_hint: &str) -> Result<Image, String> {
    let hint = ext_hint.trim_start_matches('.').to_ascii_lowercase();
    match sniff(bytes) {
        Some(Kind::Jpeg) => jpeg::decode(bytes, None),
        Some(Kind::Png) => pngdec::decode(bytes, true),
        Some(Kind::Webp) => webpdec::decode(bytes, true),
        Some(Kind::Gif) => gifdec::decode(bytes, true),
        Some(Kind::Jxl) => jxldec::decode(bytes, true),
        Some(Kind::Jp2) => jp2::decode(bytes),
        #[cfg(windows)]
        Some(Kind::Wic) => wic::decode_bytes(bytes),
        _ if is_raw_ext(&hint) => decode_raw_bytes(bytes, &hint),
        _ if bytes.starts_with(b"DDS ") => imgfmt::decode(bytes, "dds"),
        _ if imgfmt::is_ext(&hint) => imgfmt::decode(bytes, &hint),
        _ => Err("unrecognised picture format".into()),
    }
}

/// A camera RAW file held in memory (an importer's incoming file): the RAW decoders work on files, so it goes through a
/// temporary one. Follows the RAW look in force (camera look = embedded JPEG, otherwise developed from the sensor data).
fn decode_raw_bytes(bytes: &[u8], ext: &str) -> Result<Image, String> {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("nuviewer-raw-{}-{n}.{ext}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("temporary file: {e}"))?;
    let r = decode_file(&tmp);
    let _ = std::fs::remove_file(&tmp);
    r
}

/// In *preview* mode (the user chose the camera's embedded JPEG), a preview below this many pixels is still replaced by
/// a half-size development of the sensor data: an Adobe DNG's 0.7 MP preview is a thumbnail, not a picture.
const DEVELOP_BELOW_PX: u64 = 1_000_000;

/// `develop::develop`, but a decoder panic or error just means "no result" (the caller falls back to the preview).
/// `NUVIEWER_RAW_DEBUG=1` prints why a file wasn't developed.
fn catch_develop(path: &Path, mode: RawMode) -> Option<Image> {
    match std::panic::catch_unwind(|| develop::develop(path, mode)) {
        Ok(Ok(img)) => Some(img),
        Ok(Err(e)) => {
            if std::env::var_os("NUVIEWER_RAW_DEBUG").is_some() {
                eprintln!("raw develop failed: {e}");
            }
            None
        }
        Err(_) => None,
    }
}

/// Develop `path` (half size) if that yields a clearly larger picture than `preview`.
fn try_develop(path: &Path, preview: &Image) -> Option<Image> {
    let dev = catch_develop(path, RawMode::Half)?;
    ((dev.width as u64 * dev.height as u64) * 2 > (preview.width as u64 * preview.height as u64) * 3).then_some(dev)
}

#[cfg(windows)]
fn decode_wic(path: &Path) -> Result<Image, String> {
    wic::decode_file(path)
}

#[cfg(not(windows))]
fn decode_wic(_path: &Path) -> Result<Image, String> {
    Err("format not supported on this platform".into())
}

/// Decode JPEG bytes (used for the thumbnail disk cache); returns upright sRGB pixels.
pub fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    jpeg::decode(bytes, None).map(Image::finish_for_cpu)
}

/// Encode packed RGB8 pixels as a baseline JPEG (what a PDF gets for pictures that can't go in as they are).
pub fn encode_jpeg_rgb(rgb: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || width > 65535 || height > 65535 || rgb.len() < (width * height * 3) as usize {
        return Err("bad picture size for JPEG".into());
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100)).encode(rgb, width as u16, height as u16, jpeg_encoder::ColorType::Rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

pub use jpegmeta::{jpeg_info, JpegInfo};

/// Encode a thumbnail as JPEG for the on-disk cache. Alpha is flattened over `bg`.
pub fn encode_thumb_jpeg(img: &Image, bg: [u8; 3]) -> Result<Vec<u8>, String> {
    let mut rgb = Vec::with_capacity(img.rgba.len() / 4 * 3);
    for p in img.rgba.chunks_exact(4) {
        let a = p[3] as u32;
        for c in 0..3 {
            rgb.push(((p[c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255) as u8);
        }
    }
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 88)
        .encode(&rgb, img.width as u16, img.height as u16, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| format!("jpeg encode: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Kind::Jpeg));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Some(Kind::Png));
        assert_eq!(sniff(b"GIF89a"), Some(Kind::Gif));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some(Kind::Webp));
        assert_eq!(sniff(b"8BPS\x00\x01"), Some(Kind::Psd));
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypheic"), Some(Kind::Wic));
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypavif"), Some(Kind::Wic));
        assert_eq!(sniff(b"\x00\x00\x00\x18ftypcrx "), Some(Kind::Raw));
        assert_eq!(sniff(b"nonsense"), None);
    }

    #[test]
    fn display_size_swaps_for_quarter_turns() {
        let mut img = solid(4, 2);
        for (o, want) in [(1, (4, 2)), (2, (4, 2)), (4, (4, 2)), (5, (2, 4)), (6, (2, 4)), (8, (2, 4))] {
            img.orientation = o;
            assert_eq!(img.display_size(), want, "orientation {o}");
        }
    }

    #[test]
    fn oriented_rotates_pixels_like_the_shader_does() {
        // 3x2 image, each pixel's red channel = its index: row0 = 0 1 2, row1 = 3 4 5.
        let make = |o: u8| {
            let mut img = Image::new(3, 2, (0..6u8).flat_map(|i| [i, 0, 0, 255]).collect());
            img.orientation = o;
            let out = img.oriented();
            (out.width, out.height, out.rgba.chunks_exact(4).map(|p| p[0]).collect::<Vec<_>>(), out.orientation)
        };
        assert_eq!(make(1), (3, 2, vec![0, 1, 2, 3, 4, 5], 1));
        assert_eq!(make(2), (3, 2, vec![2, 1, 0, 5, 4, 3], 1)); // mirror
        assert_eq!(make(3), (3, 2, vec![5, 4, 3, 2, 1, 0], 1)); // 180
        assert_eq!(make(4), (3, 2, vec![3, 4, 5, 0, 1, 2], 1)); // flip vertical
        assert_eq!(make(6), (2, 3, vec![3, 0, 4, 1, 5, 2], 1)); // 90 clockwise
        assert_eq!(make(8), (2, 3, vec![2, 5, 1, 4, 0, 3], 1)); // 90 counter-clockwise
        assert_eq!(make(5), (2, 3, vec![0, 3, 1, 4, 2, 5], 1)); // transpose
        assert_eq!(make(7), (2, 3, vec![5, 2, 4, 1, 3, 0], 1)); // transverse
    }

    #[test]
    fn size_guard() {
        assert!(check_size(10_000, 10_000).is_ok()); // 100 MP
        assert!(check_size(20_000, 20_000).is_err()); // 400 MP
        assert!(check_size(0, 5).is_err());
        assert!(check_size(u32::MAX as u64, u32::MAX as u64).is_err(), "must not overflow");
    }

    fn solid(w: u32, h: u32) -> Image {
        Image::new(w, h, vec![200; (w * h * 4) as usize])
    }

    // The real limits are 16384 px / 120 MP; the tests use tiny ones so they don't allocate hundreds of megabytes.
    #[test]
    fn gpu_fit_leaves_normal_images_alone() {
        let img = fit_within(solid(400, 300), 1000, 1_000_000);
        assert_eq!((img.width, img.height, img.reduced_from), (400, 300, None));
    }

    #[test]
    fn gpu_fit_shrinks_overlong_images_to_the_dimension_limit() {
        let img = fit_within(solid(2000, 30), 1000, u64::MAX); // few pixels but 2000 wide
        assert_eq!(img.width, 1000);
        assert_eq!(img.reduced_from, Some((2000, 30)));
        assert_eq!(img.height, 15, "aspect kept");
    }

    #[test]
    fn gpu_fit_shrinks_to_the_pixel_budget() {
        // 1300 x 1000 = 1.3 MP > 1.2 MP budget, though both sides fit in the dimension limit.
        let img = fit_within(solid(1300, 1000), 5000, 1_200_000);
        assert!((img.width as u64 * img.height as u64) <= 1_200_000);
        assert!(img.width < 1300 && img.reduced_from == Some((1300, 1000)));
    }

    #[test]
    fn real_limits_are_sane() {
        assert_eq!(GPU_MAX_DIM, 16384);
        assert!(GPU_MAX_PIXELS < MAX_DECODE_PIXELS);
    }

    #[test]
    fn extension_support() {
        for e in ["JPG", "webp", "GIF", "psd", "ARW", "dng", "cr3"] {
            assert!(is_supported_ext(e), "{e}");
        }
        assert!(!is_supported_ext("txt"));
    }
}
