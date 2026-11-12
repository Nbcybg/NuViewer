//! Image decoders behind a single entry point. Everything decodes to straight-alpha RGBA8 in sRGB.
//!
//! Formats: JPEG, PNG/APNG, GIF, WebP (incl. animated), PSD, camera RAW (embedded preview),
//! and on Windows anything WIC can open (HEIC/AVIF with the Store extensions, TIFF, BMP, ICO, ...).

use std::path::Path;

mod develop;
#[cfg(windows)]
mod eps;
mod gifdec;
mod icc;
mod imgfmt;
mod jpeg;
pub mod model;
#[cfg(windows)]
mod metafile;
pub mod metadata;
mod orient;
#[cfg(windows)]
mod pdf;
mod pngdec;
mod psddec;
pub mod progress;
mod raw;
pub mod resize;
mod svgdec;
#[cfg(windows)]
mod wic;
mod webpdec;

pub use resize::downscale;

static ICC_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Turn the conversion of embedded colour profiles to sRGB on or off (a setting). Affects pictures decoded afterwards.
pub fn set_icc_enabled(on: bool) {
    ICC_ENABLED.store(on, std::sync::atomic::Ordering::Relaxed);
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
    /// (page, page count) when this picture is one page of a multi-page document (PDF).
    pub page: Option<(u32, u32)>,
    /// For 3D models: the geometry (`pixels` hold a software-rendered preview). The viewer draws the mesh itself.
    pub mesh: Option<std::sync::Arc<model::Mesh>>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Image { width, height, rgba, delay_ms: 0, extra_frames: Vec::new(), reduced_from: None, orientation: 1, icc: None, page: None, mesh: None }
    }

    pub fn frame_count(&self) -> usize {
        1 + self.extra_frames.len()
    }

    /// Size as it should be shown, i.e. after applying `orientation`.
    pub fn display_size(&self) -> (u32, u32) {
        if self.orientation >= 5 { (self.height, self.width) } else { (self.width, self.height) }
    }

    /// Convert pending ICC-profiled pixels to sRGB (no-op if there's nothing pending, or if conversion is switched off).
    pub fn apply_icc(&mut self) {
        if let Some(profile) = self.icc.take() {
            if ICC_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
                icc::to_srgb_in_place(&mut self.rgba, &profile);
            }
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
            Image { rgba: self.rgba.clone(), extra_frames: Vec::new(), icc: None, page: self.page, mesh: self.mesh.clone(), ..*self }.oriented()
        } else {
            Image { rgba: self.rgba.clone(), extra_frames: Vec::new(), icc: None, page: self.page, mesh: self.mesh.clone(), ..*self }
        }
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
        Some(Image { width, height, rgba, delay_ms, extra_frames: it.map(|(rgba, delay_ms)| Frame { rgba, delay_ms }).collect(), reduced_from: None, orientation: 1, icc: None, page: None, mesh: None })
    }
}

const RAW_EXTS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "dng", "orf", "rw2", "raf", "pef", "srw", "3fr", "erf", "kdc", "mrw", "raw", "rwl", "iiq",
];
/// RAW extensions that are TIFF containers (everything in `RAW_EXTS` except RAF, CR3, CRW and MRW).
const TIFF_RAW_EXTS: &[&str] = &["cr2", "nef", "nrw", "arw", "sr2", "srf", "dng", "orf", "rw2", "pef", "srw", "3fr", "erf", "kdc", "raw", "rwl", "iiq"];
/// Formats we hand straight to WIC.
const WIC_EXTS: &[&str] = &["heic", "heif", "avif", "tif", "tiff", "bmp", "ico", "jxr", "wdp", "hdp"];
const NATIVE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "psd"];
/// PDF and PDF-compatible Illustrator files, rendered by Windows (`Windows.Data.Pdf`).
const DOC_EXTS: &[&str] = &["pdf", "ai"];
/// SVG, rendered by resvg.
const SVG_EXTS: &[&str] = &["svg", "svgz"];
/// Windows metafiles, played back by GDI+.
const META_EXTS: &[&str] = &["emf", "wmf"];
/// Encapsulated PostScript (Ghostscript if installed, else the preview inside the file).
const EPS_EXTS: &[&str] = &["eps"];

/// Every extension we can open (lowercase, no dot): what the file-association registration lists.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = NATIVE_EXTS.iter().chain(RAW_EXTS.iter()).chain(model::EXTS.iter()).chain(imgfmt::EXTS.iter()).copied().collect();
    if cfg!(windows) {
        v.extend(WIC_EXTS);
        v.extend(DOC_EXTS);
        v.extend(META_EXTS);
        v.extend(EPS_EXTS);
    }
    v.extend(SVG_EXTS);
    v
}

pub fn is_supported_ext(ext: &str) -> bool {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    NATIVE_EXTS.contains(&e) || RAW_EXTS.contains(&e) || model::EXTS.contains(&e) || imgfmt::is_ext(e) || (cfg!(windows) && (WIC_EXTS.contains(&e) || DOC_EXTS.contains(&e) || META_EXTS.contains(&e) || EPS_EXTS.contains(&e))) || SVG_EXTS.contains(&e)
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

/// Decode `path` in full (all animation frames), shrunk if needed to fit what the GPU can show.
pub fn decode_file(path: &Path) -> Result<Image, String> {
    // Order matters for speed: shrink first, then colour-convert, so a 100-megapixel panorama is converted at the
    // size it will actually be shown (and orientation is left to the GPU).
    decode_file_opts(path, false).map(fit_gpu_limits).map(|mut img| {
        img.apply_icc();
        img
    })
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
    decode_file_opts(path, true)
}

fn decode_file_opts(path: &Path, first_only: bool) -> Result<Image, String> {
    // A page of a multi-page document is addressed as "file.pdf?page=N": peel that off first.
    let (real_path, page) = split_page(path);
    let path = real_path.as_path();
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if WIC_EXTS.contains(&ext.as_str()) {
        return decode_wic(path);
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
            if !first_only && (img.width as u64 * img.height as u64) < DEVELOP_BELOW_PX {
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
            if !first_only {
                if let Some(dev) = catch_develop(path) {
                    return Ok(dev);
                }
            }
            decode_wic(path).map_err(|w| format!("{e}; {w}"))
        });
    }
    match sniff(&bytes) {
        Some(Kind::Jpeg) => jpeg::decode(&bytes, None),
        Some(Kind::Png) => pngdec::decode(&bytes, first_only),
        Some(Kind::Gif) => gifdec::decode(&bytes, first_only),
        Some(Kind::Webp) => webpdec::decode(&bytes, first_only),
        Some(Kind::Psd) => psddec::decode(&bytes),
        Some(Kind::Raw) => raw::decode_preview(&bytes),
        Some(Kind::Wic) => decode_wic(path),
        None => Err(format!("unsupported format: .{ext}")),
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
        #[cfg(windows)]
        Some(Kind::Wic) => wic::decode_bytes(bytes),
        _ if bytes.starts_with(b"DDS ") => imgfmt::decode(bytes, "dds"),
        _ if imgfmt::is_ext(&hint) => imgfmt::decode(bytes, &hint),
        _ => Err("unrecognised picture format".into()),
    }
}

/// Embedded previews below this many pixels are replaced by a half-size development of the sensor data. Kept at 1 MP
/// on purpose (speed first): a Sony ARW's 1.7 MP preview opens in ~20 ms and is not developed (~170 ms), while an
/// Adobe DNG's 0.7 MP preview is.
const DEVELOP_BELOW_PX: u64 = 1_000_000;

/// `develop::develop_half`, but a decoder panic or error just means "no result".
fn catch_develop(path: &Path) -> Option<Image> {
    if std::env::var_os("NUVIEWER_NO_DEVELOP").is_some() {
        return None; // opt-out (and handy for comparing against the embedded preview)
    }
    std::panic::catch_unwind(|| develop::develop_half(path)).ok()?.ok()
}

/// Develop `path` if that yields a clearly larger picture than `preview`.
fn try_develop(path: &Path, preview: &Image) -> Option<Image> {
    let dev = catch_develop(path)?;
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
