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
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            dim_pct: 45,
            canvas_mode: CanvasMode::Solid,
            canvas_image: String::new(),
            canvas_shade: 35,
            play_loop: true,
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
            _ => return false,
        }
        true
    }

    pub fn serialize(&self) -> String {
        let b = |v: bool| if v { "true" } else { "false" };
        format!(
            "dim_pct = {}\ncanvas_mode = {}\ncanvas_image = {}\ncanvas_shade = {}\nplay_loop = {}\nautoplay = {}\nvolume = {}\nmuted = {}\nscrub_preview = {}\nseven_zip = {}\nffmpeg = {}\narchive_format = {}\narchive_level = {}\npdf_page = {}\npdf_quality = {}\n",
            self.dim_pct,
            self.canvas_mode.id(),
            self.canvas_image,
            self.canvas_shade,
            b(self.play_loop),
            b(self.autoplay),
            self.volume,
            b(self.muted),
            b(self.scrub_preview),
            self.seven_zip,
            self.ffmpeg,
            self.archive_format,
            self.archive_level,
            self.pdf_page.id(),
            self.pdf_quality
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
