//! Two pages of the Settings window that came later: "Screen & play" (dim, canvas picture / blur, playback) and
//! "Archives & PDF" (7-Zip, the format of new archives, PDF page size). Kept apart so `settings_ui.rs` does not grow;
//! the window calls `build`, `click` and `read`.

use super::*;
use nv_core::prefs::{CanvasMode, PdfPage, CANVAS_PRESETS};
use nv_win::ui::{Form, SELECTION_CHANGED, SLIDER_MOVED};

pub const TAB_SCREEN: u16 = 19;
pub const TAB_TOOLS: u16 = 20;
pub const PAGE_SCREEN: u8 = 10;
pub const PAGE_TOOLS: u8 = 11;

const S_DIM: u16 = 1001;
const S_DIM_TEXT: u16 = 1002;
const S_MODE: u16 = 1003;
const S_PRESET: u16 = 1004;
const S_GREY: u16 = 1005;
const S_IMAGE: u16 = 1006;
const S_IMAGE_BROWSE: u16 = 1007;
const S_SHADE: u16 = 1008;
const S_SHADE_TEXT: u16 = 1009;
const S_AUTOPLAY: u16 = 1010;
const S_LOOP: u16 = 1011;
const S_SCRUB: u16 = 1012;
const S_VOLUME: u16 = 1013;
const S_VOLUME_TEXT: u16 = 1014;
const T_7Z: u16 = 1101;
const T_7Z_BROWSE: u16 = 1102;
const T_7Z_STATUS: u16 = 1103;
const T_FORMAT: u16 = 1104;
const T_LEVEL: u16 = 1105;
const T_PDF_PAGE: u16 = 1106;
const T_PDF_QUALITY: u16 = 1107;
const T_FFMPEG: u16 = 1108;
const T_FFMPEG_BROWSE: u16 = 1109;
const T_FFMPEG_STATUS: u16 = 1110;
/// The hex box of the Viewer page holds the canvas colour; the presets and the grey slider write into it.
const CANVAS_HEX: u16 = 101;

const MODES: [&str; 3] = ["A plain colour", "The picture itself, blurred", "A picture file of my own"];
const MODE_VALUES: [CanvasMode; 3] = [CanvasMode::Solid, CanvasMode::Blur, CanvasMode::Image];
const FORMATS: [&str; 3] = ["zip (opens everywhere)", "7z (smaller; needs 7-Zip)", "tar.gz (needs 7-Zip or Windows' tar)"];
const FORMAT_VALUES: [&str; 3] = ["zip", "7z", "tar.gz"];
const PDF_PAGES: [&str; 3] = ["Each page the size of its picture", "A4", "US Letter"];
const PDF_VALUES: [PdfPage; 3] = [PdfPage::Fit, PdfPage::A4, PdfPage::Letter];

pub fn tabs() -> [(u16, &'static str, i32); 2] {
    [(TAB_SCREEN, "Screen && play", 104), (TAB_TOOLS, "Archives && PDF", 112)]
}

fn grey_of(hex: u32) -> i32 {
    (((hex >> 16) & 0xFF) + ((hex >> 8) & 0xFF) + (hex & 0xFF)) as i32 / 3
}

pub fn build(form: &Form, s: &Settings) {
    let m = &s.more;
    form.begin_page(PAGE_SCREEN);
    form.group(12, 52, 696, 80, "Dim");
    form.label(28, 80, 330, 20, "The Dim key (Shift+D) darkens the screen by:");
    form.slider(S_DIM, 360, 76, 250, 28, 5, 95, m.dim_pct as i32);
    form.label_id(S_DIM_TEXT, 620, 80, 80, 20, &format!("{}%", m.dim_pct));
    form.group(12, 142, 696, 232, "The canvas around the picture");
    form.label(28, 170, 90, 20, "Canvas is:");
    form.combo(S_MODE, 120, 166, 280, 110, &MODES, MODE_VALUES.iter().position(|v| *v == m.canvas_mode).unwrap_or(0));
    form.label(28, 204, 90, 20, "Colour:");
    let names: Vec<&str> = CANVAS_PRESETS.iter().map(|p| p.0).collect();
    form.combo(S_PRESET, 120, 200, 160, 200, &names, CANVAS_PRESETS.iter().position(|p| p.1 == s.canvas).unwrap_or(0));
    form.label(292, 204, 70, 20, "or grey:");
    form.slider(S_GREY, 360, 200, 250, 28, 0, 255, grey_of(s.canvas));
    form.label(28, 244, 90, 20, "Picture file:");
    form.edit(S_IMAGE, 120, 240, 480, 24, &m.canvas_image);
    form.button(S_IMAGE_BROWSE, 608, 239, 90, 26, "Browse…", false);
    form.label(28, 284, 330, 20, "Darken the blurred / chosen picture by:");
    form.slider(S_SHADE, 360, 280, 250, 28, 0, 90, m.canvas_shade as i32);
    form.label_id(S_SHADE_TEXT, 620, 284, 80, 20, &format!("{}%", m.canvas_shade));
    form.label(28, 322, 660, 44, "The colour is also the hex box on the Viewer tab. A blurred or chosen picture shows only where the picture does not fill the window.");
    form.group(12, 384, 696, 180, "Video, sound and animation");
    form.check(S_AUTOPLAY, 28, 410, 660, 22, "A video or a sound starts playing when it comes up", m.autoplay);
    form.check(S_LOOP, 28, 438, 660, 22, "Loop animations, videos, sounds, sequences and the slideshow  (Shift+L)", m.play_loop);
    form.check(S_SCRUB, 28, 466, 660, 22, "Show a picture of the spot under the pointer over the seek bar", m.scrub_preview);
    form.label(28, 500, 90, 20, "Volume:");
    form.slider(S_VOLUME, 120, 496, 250, 28, 0, 100, m.volume as i32);
    form.label_id(S_VOLUME_TEXT, 380, 500, 80, 20, &format!("{}%", m.volume));

    form.begin_page(PAGE_TOOLS);
    form.group(12, 52, 696, 190, "Archives (zip, 7z, rar, tar ...) in NuGallery");
    form.label(28, 80, 660, 20, "7-Zip (7z.exe) compresses and unpacks every format. Leave empty to look in the usual places;");
    form.label(28, 100, 660, 20, "without it Windows' own tools are used (zip, tar and, for opening, what tar.exe reads).");
    form.edit(T_7Z, 28, 128, 566, 24, &m.seven_zip);
    form.button(T_7Z_BROWSE, 602, 127, 94, 26, "Browse…", false);
    form.label_id(T_7Z_STATUS, 28, 160, 660, 20, "");
    form.label(28, 192, 130, 20, "New archives are:");
    form.combo(T_FORMAT, 160, 188, 300, 110, &FORMATS, FORMAT_VALUES.iter().position(|v| *v == m.archive_format).unwrap_or(0));
    form.label(480, 192, 130, 20, "Level (1 fast … 9 small):");
    form.edit(T_LEVEL, 640, 188, 40, 24, &m.archive_level.to_string());
    form.group(12, 252, 696, 130, "PDF made from pictures");
    form.label(28, 282, 130, 20, "Page size:");
    form.combo(T_PDF_PAGE, 160, 278, 300, 110, &PDF_PAGES, PDF_VALUES.iter().position(|v| *v == m.pdf_page).unwrap_or(0));
    form.label(28, 318, 330, 20, "JPEG quality of re-encoded pictures:");
    form.edit(T_PDF_QUALITY, 360, 314, 40, 24, &m.pdf_quality.to_string());
    form.label(28, 346, 660, 28, "JPEG files go into the PDF as they are (no quality lost); other pictures are stored compressed without loss when they are small, as JPEG otherwise.");
    form.group(12, 392, 696, 130, "Video Windows can't play (10-bit camera files, MXF ...)");
    form.label(28, 420, 660, 20, "ffmpeg.exe makes a picture of such a clip and a playable copy (kept in a cache). Empty = look in the usual places.");
    form.edit(T_FFMPEG, 28, 448, 566, 24, &m.ffmpeg);
    form.button(T_FFMPEG_BROWSE, 602, 447, 94, 26, "Browse…", false);
    form.label_id(T_FFMPEG_STATUS, 28, 480, 660, 20, "");
    status(form, s);
}

fn status(form: &Form, s: &Settings) {
    let typed = form.text(T_7Z);
    let found = nv_core::archive_tools::seven_zip(if typed.trim().is_empty() { &s.more.seven_zip } else { typed.trim() });
    form.set_text(
        T_7Z_STATUS,
        &match found {
            Some(p) => format!("In use: {}", p.display()),
            None => "7-Zip not found: Windows' own zip and tar will be used.".to_string(),
        },
    );
    nv_core::ffmpeg::configure(form.text(T_FFMPEG).as_str());
    form.set_text(
        T_FFMPEG_STATUS,
        &match nv_core::ffmpeg::find() {
            Some(p) => format!("In use: {}", p.display()),
            None => "ffmpeg not found: such video shows a message instead.".to_string(),
        },
    );
    nv_core::ffmpeg::configure(&s.more.ffmpeg);
}

/// A click or slider move on one of these pages; true if it was ours.
pub fn click(form: &Form, id: u16, s: &Settings) -> bool {
    match id {
        x if x == (S_DIM | SLIDER_MOVED) => form.set_text(S_DIM_TEXT, &format!("{}%", form.slider_value(S_DIM))),
        x if x == (S_SHADE | SLIDER_MOVED) => form.set_text(S_SHADE_TEXT, &format!("{}%", form.slider_value(S_SHADE))),
        x if x == (S_VOLUME | SLIDER_MOVED) => form.set_text(S_VOLUME_TEXT, &format!("{}%", form.slider_value(S_VOLUME))),
        x if x == (S_GREY | SLIDER_MOVED) => {
            let g = form.slider_value(S_GREY) as u32;
            form.set_text(CANVAS_HEX, &format!("{:02X}{:02X}{:02X}", g, g, g));
        }
        x if x == (S_PRESET | SELECTION_CHANGED) => {
            if let Some((_, c)) = CANVAS_PRESETS.get(form.combo_index(S_PRESET)) {
                form.set_text(CANVAS_HEX, &format!("{c:06X}"));
                form.slider_set(S_GREY, grey_of(*c));
            }
        }
        S_IMAGE_BROWSE => {
            if let Some(p) = nv_win::shell::pick_picture(form.hwnd) {
                form.set_text(S_IMAGE, &p.display().to_string());
                form.combo_set(S_MODE, 2);
            }
        }
        T_FFMPEG_BROWSE => {
            if let Some(p) = nv_win::shell::pick_program(form.hwnd) {
                form.set_text(T_FFMPEG, &p.display().to_string());
                status(form, s);
            }
        }
        T_7Z_BROWSE => {
            if let Some(p) = nv_win::shell::pick_program(form.hwnd) {
                form.set_text(T_7Z, &p.display().to_string());
                status(form, s);
            }
        }
        _ => return false,
    }
    true
}

/// Take this page's values into `s`.
pub fn read(form: &Form, s: &mut Settings) {
    let m = &mut s.more;
    m.dim_pct = form.slider_value(S_DIM).clamp(5, 95) as u8;
    m.canvas_mode = MODE_VALUES[form.combo_index(S_MODE).min(2)];
    m.canvas_image = form.text(S_IMAGE).trim().trim_matches('"').to_string();
    m.canvas_shade = form.slider_value(S_SHADE).clamp(0, 90) as u8;
    m.autoplay = form.is_checked(S_AUTOPLAY);
    m.play_loop = form.is_checked(S_LOOP);
    m.scrub_preview = form.is_checked(S_SCRUB);
    m.volume = form.slider_value(S_VOLUME).clamp(0, 100) as u8;
    m.seven_zip = form.text(T_7Z).trim().trim_matches('"').to_string();
    m.ffmpeg = form.text(T_FFMPEG).trim().trim_matches('"').to_string();
    nv_core::ffmpeg::configure(&m.ffmpeg);
    m.archive_format = FORMAT_VALUES[form.combo_index(T_FORMAT).min(2)].to_string();
    m.archive_level = form.text(T_LEVEL).trim().parse::<u8>().map(|v| v.clamp(1, 9)).unwrap_or(m.archive_level);
    m.pdf_page = PDF_VALUES[form.combo_index(T_PDF_PAGE).min(2)];
    m.pdf_quality = form.text(T_PDF_QUALITY).trim().parse::<u8>().map(|v| v.clamp(30, 100)).unwrap_or(m.pdf_quality);
}
