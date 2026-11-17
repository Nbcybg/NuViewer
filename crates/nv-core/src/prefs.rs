//! The newer settings, grouped so `settings.rs` does not keep growing: screen (dim, canvas picture / blur), playback,
//! archive and PDF tools. `Settings.more` holds them; the settings file stays one flat `key = value` list.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CanvasMode {
    /// A plain colour (`Settings.canvas`).
    Solid,
    /// The picture itself, stretched over the window and blurred.
    Blur,
    /// A picture file the user chose, stretched to cover the window.
    Image,
}

impl CanvasMode {
    pub fn id(self) -> &'static str {
        match self {
            CanvasMode::Solid => "solid",
            CanvasMode::Blur => "blur",
            CanvasMode::Image => "image",
        }
    }
    pub fn from_id(v: &str) -> Option<CanvasMode> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "solid" | "colour" | "color" => CanvasMode::Solid,
            "blur" => CanvasMode::Blur,
            "image" | "picture" => CanvasMode::Image,
            _ => return None,
        })
    }
}

/// Colour presets for the canvas (0xRRGGBB): name, value.
pub const CANVAS_PRESETS: [(&str, u32); 8] = [
    ("Black", 0x000000),
    ("Charcoal", 0x1A1A1A),
    ("Dark grey", 0x333333),
    ("Mid grey", 0x808080),
    ("Light grey", 0xD0D0D0),
    ("White", 0xFFFFFF),
    ("Slate blue", 0x1B2433),
    ("Warm paper", 0xEFE6D2),
];

/// How a made PDF sizes its pages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PdfPage {
    /// Each page is as big as its picture.
    Fit,
    A4,
    Letter,
}

impl PdfPage {
    pub fn id(self) -> &'static str {
        match self {
            PdfPage::Fit => "fit",
            PdfPage::A4 => "a4",
            PdfPage::Letter => "letter",
        }
    }
    pub fn from_id(v: &str) -> Option<PdfPage> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "fit" | "picture" => PdfPage::Fit,
            "a4" => PdfPage::A4,
            "letter" => PdfPage::Letter,
            _ => return None,
        })
    }
}

use crate::videocolour::{SdrAssume, ToneMap};

#[derive(Clone, PartialEq, Debug)]
pub struct Prefs {
    /// How much the Dim toggle darkens the screen, percent of black laid over everything.
    pub dim_pct: u8,
    pub canvas_mode: CanvasMode,
    /// The picture file for `CanvasMode::Image`.
    pub canvas_image: String,
    /// How much the blurred / chosen background is darkened so the picture stands out, percent.
    pub canvas_shade: u8,
    /// Playback: loop animations / videos / slideshow, volume 0..=100, muted, the speed the bar starts at.
    pub play_loop: bool,
    /// Animations and sequences play forward, then backward, then forward again (instead of jumping back to the start).
    pub play_pingpong: bool,
    /// A video or sound starts playing when it comes up.
    pub autoplay: bool,
    pub volume: u8,
    pub muted: bool,
    /// Pictures that appear over the seek bar while the pointer is on it.
    pub scrub_preview: bool,
    /// 7-Zip (or a compatible `7z.exe`); empty = look in the usual places, then use what Windows has.
    pub seven_zip: String,
    /// ffmpeg.exe, for video Windows can't decode (empty = look in the usual places).
    pub ffmpeg: String,
    /// Format a new archive gets: "zip", "7z" or "tar.gz" (anything but zip needs 7-Zip or Windows' tar).
    pub archive_format: String,
    /// 1 (fast) ..= 9 (small).
    pub archive_level: u8,
    pub pdf_page: PdfPage,
    /// JPEG quality of pictures that have to be re-encoded for a PDF (JPEG files go in untouched).
    pub pdf_quality: u8,
    /// Language of the text on screen: "auto" (Windows' own) or a code with a `lang/<code>.txt` file ("th", "ja" ...).
    pub language: String,
    /// Video snapshot: file format ("png", "png16", "jpg", "webp", "bmp", "tiff"), JPEG / WebP quality, folder (empty = next to
    /// the video), and the name pattern ({name} {time} {n}).
    pub snapshot_format: String,
    pub snapshot_quality: u8,
    pub snapshot_dir: String,
    pub snapshot_name: String,
    /// How SDR video numbers become light, how HDR is brought into SDR, the HDR master's peak, or keep HDR as 16-bit PQ.
    pub snapshot_sdr: SdrAssume,
    pub snapshot_tonemap: ToneMap,
    pub snapshot_peak: u32,
    pub snapshot_keep_hdr: bool,
    /// NuImporter: what happens to a fresh import ("save", "confirm", "clipboard"), where it goes (empty = Pictures\NuViewer),
    /// the format and quality it is saved in, and the name pattern ({source} {date} {time} {n}).
    pub import_after: String,
    pub import_dir: String,
    pub import_format: String,
    pub import_quality: u8,
    pub import_name: String,
    /// Open NuEditor on every import.
    pub import_edit: bool,
    /// Pictures in a folder the camera writes to are imported as they appear (tether by folder).
    pub tether_dir: String,
    /// NuQuick listens for Ctrl+Print Screen (a region) and Ctrl+Alt+Print Screen (the whole screen) and starts NuImporter.
    pub screenshot_keys: bool,
    /// The sentence a font file is shown with (up to 100 characters; empty = "The quick brown fox jumps over the lazy dog").
    pub font_sample: String,
    /// How a picked colour is written to the clipboard (`colours::ColourFormat::id`), and how many colours a palette has.
    pub pick_format: String,
    pub palette_size: u8,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            dim_pct: 45,
            canvas_mode: CanvasMode::Solid,
            canvas_image: String::new(),
            canvas_shade: 35,
            play_loop: true,
            play_pingpong: false,
            autoplay: true,
            volume: 80,
            muted: false,
            scrub_preview: true,
            seven_zip: String::new(),
            ffmpeg: String::new(),
            archive_format: "zip".into(),
            archive_level: 5,
            pdf_page: PdfPage::Fit,
            pdf_quality: 88,
            language: "auto".into(),
            snapshot_format: "png".into(),
            snapshot_quality: 98,
            snapshot_dir: String::new(),
            snapshot_name: "{name} {time}".into(),
            snapshot_sdr: SdrAssume::AsShown,
            snapshot_tonemap: ToneMap::Reinhard,
            snapshot_peak: 1000,
            snapshot_keep_hdr: false,
            import_after: "confirm".into(),
            import_dir: String::new(),
            import_format: "png".into(),
            import_quality: 95,
            import_name: "{source} {date} {time}".into(),
            import_edit: false,
            tether_dir: String::new(),
            screenshot_keys: true,
            font_sample: nv_codecs::fontview::DEFAULT_SAMPLE.to_string(),
            pick_format: "hex".into(),
            palette_size: 8,
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Prefs {
    /// One `key = value` line of the settings file; false if the key is not one of ours.
    pub fn apply(&mut self, k: &str, v: &str) -> bool {
        let pct = |v: &str, lo: u8, hi: u8, cur: u8| v.parse::<u8>().ok().filter(|p| (lo..=hi).contains(p)).unwrap_or(cur);
        match k {
            "dim_pct" => self.dim_pct = pct(v, 5, 95, self.dim_pct),
            "canvas_mode" => self.canvas_mode = CanvasMode::from_id(v).unwrap_or(self.canvas_mode),
            "canvas_image" => self.canvas_image = v.to_string(),
            "canvas_shade" => self.canvas_shade = pct(v, 0, 90, self.canvas_shade),
            "play_loop" => self.play_loop = parse_bool(v).unwrap_or(self.play_loop),
            "play_pingpong" => self.play_pingpong = parse_bool(v).unwrap_or(self.play_pingpong),
            "autoplay" => self.autoplay = parse_bool(v).unwrap_or(self.autoplay),
            "volume" => self.volume = pct(v, 0, 100, self.volume),
            "muted" => self.muted = parse_bool(v).unwrap_or(self.muted),
            "scrub_preview" => self.scrub_preview = parse_bool(v).unwrap_or(self.scrub_preview),
            "seven_zip" => self.seven_zip = v.to_string(),
            "ffmpeg" => self.ffmpeg = v.to_string(),
            "archive_format" => {
                if matches!(v.to_ascii_lowercase().as_str(), "zip" | "7z" | "tar.gz") {
                    self.archive_format = v.to_ascii_lowercase();
                }
            }
            "archive_level" => self.archive_level = pct(v, 1, 9, self.archive_level),
            "pdf_page" => self.pdf_page = PdfPage::from_id(v).unwrap_or(self.pdf_page),
            "pdf_quality" => self.pdf_quality = pct(v, 30, 100, self.pdf_quality),
            "language" => self.language = v.trim().to_string(),
            "snapshot_format" => {
                if matches!(v.to_ascii_lowercase().as_str(), "png" | "png16" | "jpg" | "webp" | "bmp" | "tiff") {
                    self.snapshot_format = v.to_ascii_lowercase();
                }
            }
            "snapshot_quality" => self.snapshot_quality = pct(v, 30, 100, self.snapshot_quality),
            "snapshot_dir" => self.snapshot_dir = v.to_string(),
            "snapshot_name" => {
                if !v.trim().is_empty() {
                    self.snapshot_name = v.trim().to_string();
                }
            }
            "snapshot_sdr" => self.snapshot_sdr = SdrAssume::from_id(v).unwrap_or(self.snapshot_sdr),
            "snapshot_tonemap" => self.snapshot_tonemap = ToneMap::from_id(v).unwrap_or(self.snapshot_tonemap),
            "snapshot_peak" => self.snapshot_peak = v.parse::<u32>().ok().filter(|p| (100..=10000).contains(p)).unwrap_or(self.snapshot_peak),
            "snapshot_keep_hdr" => self.snapshot_keep_hdr = parse_bool(v).unwrap_or(self.snapshot_keep_hdr),
            "import_after" => {
                if matches!(v.to_ascii_lowercase().as_str(), "save" | "confirm" | "clipboard") {
                    self.import_after = v.to_ascii_lowercase();
                }
            }
            "import_dir" => self.import_dir = v.to_string(),
            "import_format" => {
                if matches!(v.to_ascii_lowercase().as_str(), "png" | "jpg" | "webp" | "bmp" | "tiff" | "qoi" | "tga") {
                    self.import_format = v.to_ascii_lowercase();
                }
            }
            "import_quality" => self.import_quality = pct(v, 30, 100, self.import_quality),
            "import_name" => {
                if !v.trim().is_empty() {
                    self.import_name = v.trim().to_string();
                }
            }
            "import_edit" => self.import_edit = parse_bool(v).unwrap_or(self.import_edit),
            "tether_dir" => self.tether_dir = v.to_string(),
            "screenshot_keys" => self.screenshot_keys = parse_bool(v).unwrap_or(self.screenshot_keys),
            "font_sample" => self.font_sample = nv_codecs::fontview::clean_sample(v),
            "pick_format" => {
                if crate::colours::ColourFormat::from_id(v).is_some() {
                    self.pick_format = v.trim().to_ascii_lowercase();
                }
            }
            "palette_size" => self.palette_size = pct(v, 2, 16, self.palette_size),
            _ => return false,
        }
        true
    }

    pub fn serialize(&self) -> String {
        let b = |v: bool| if v { "true" } else { "false" };
        format!(
            "dim_pct = {}\ncanvas_mode = {}\ncanvas_image = {}\ncanvas_shade = {}\nplay_loop = {}\nplay_pingpong = {}\nautoplay = {}\nvolume = {}\nmuted = {}\nscrub_preview = {}\nseven_zip = {}\nffmpeg = {}\narchive_format = {}\narchive_level = {}\npdf_page = {}\npdf_quality = {}\nlanguage = {}\nsnapshot_format = {}\nsnapshot_quality = {}\nsnapshot_dir = {}\nsnapshot_name = {}\nsnapshot_sdr = {}\nsnapshot_tonemap = {}\nsnapshot_peak = {}\nsnapshot_keep_hdr = {}\nimport_after = {}\nimport_dir = {}\nimport_format = {}\nimport_quality = {}\nimport_name = {}\nimport_edit = {}\ntether_dir = {}\nscreenshot_keys = {}\nfont_sample = {}\npick_format = {}\npalette_size = {}\n",
            self.dim_pct,
            self.canvas_mode.id(),
            self.canvas_image,
            self.canvas_shade,
            b(self.play_loop),
            b(self.play_pingpong),
            b(self.autoplay),
            self.volume,
            b(self.muted),
            b(self.scrub_preview),
            self.seven_zip,
            self.ffmpeg,
            self.archive_format,
            self.archive_level,
            self.pdf_page.id(),
            self.pdf_quality,
            self.language,
            self.snapshot_format,
            self.snapshot_quality,
            self.snapshot_dir,
            self.snapshot_name,
            self.snapshot_sdr.id(),
            self.snapshot_tonemap.id(),
            self.snapshot_peak,
            b(self.snapshot_keep_hdr),
            self.import_after,
            self.import_dir,
            self.import_format,
            self.import_quality,
            self.import_name,
            b(self.import_edit),
            self.tether_dir,
            b(self.screenshot_keys),
            self.font_sample,
            self.pick_format,
            self.palette_size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let mut p = Prefs::default();
        assert!(p.apply("dim_pct", "60"));
        assert!(p.apply("canvas_mode", "blur"));
        assert!(p.apply("archive_format", "7Z"));
        assert!(p.apply("volume", "999")); // refused, key still ours
        assert!(!p.apply("nonsense", "1"));
        assert_eq!((p.dim_pct, p.canvas_mode, p.archive_format.as_str(), p.volume), (60, CanvasMode::Blur, "7z", 80));
        let mut q = Prefs::default();
        for line in p.serialize().lines() {
            let (k, v) = line.split_once('=').unwrap();
            q.apply(k.trim(), v.trim());
        }
        assert_eq!(p, q);
    }
}
