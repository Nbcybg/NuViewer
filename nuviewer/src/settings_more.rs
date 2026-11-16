//! Two pages of the Settings window that came later: "Screen & play" (dim, canvas picture / blur, playback) and
//! "Archives & PDF" (7-Zip, the format of new archives, PDF page size). Kept apart so `settings_ui.rs` does not grow;
//! the window calls `build`, `click` and `read`.

use super::*;
use nv_core::prefs::{CanvasMode, PdfPage, CANVAS_PRESETS};
use nv_core::videocolour::{SdrAssume, ToneMap};
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
const S_PINGPONG: u16 = 1015;
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
    form.check(S_AUTOPLAY, 28, 408, 660, 22, "A video or a sound starts playing when it comes up", m.autoplay);
    form.check(S_LOOP, 28, 430, 660, 22, "Loop animations, videos, sounds, sequences and the slideshow  (Shift+L)", m.play_loop);
    form.check(S_PINGPONG, 28, 452, 660, 22, "Ping-pong: animations and sequences play forward, then backward  (Shift+K)", m.play_pingpong);
    form.check(S_SCRUB, 28, 474, 660, 22, "Show a picture of the spot under the pointer over the seek bar", m.scrub_preview);
    form.label(28, 514, 90, 20, "Volume:");
    form.slider(S_VOLUME, 120, 510, 250, 28, 0, 100, m.volume as i32);
    form.label_id(S_VOLUME_TEXT, 380, 514, 80, 20, &format!("{}%", m.volume));

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
    m.play_pingpong = form.is_checked(S_PINGPONG);
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

// ---- Video snapshot, Import, Language & Explorer ---------------------------------------------------------------

pub const TAB_SNAP: u16 = 21;
pub const TAB_IMPORT: u16 = 22;
pub const TAB_GENERAL: u16 = 23;
pub const PAGE_SNAP: u8 = 12;
pub const PAGE_IMPORT: u8 = 13;
pub const PAGE_GENERAL: u8 = 14;

const N_FORMAT: u16 = 1201;
const N_QUALITY: u16 = 1202;
const N_DIR: u16 = 1203;
const N_BROWSE: u16 = 1204;
const N_NAME: u16 = 1205;
const N_SDR: u16 = 1206;
const N_HDR: u16 = 1207;
const N_TONEMAP: u16 = 1208;
const N_PEAK: u16 = 1209;
const N_FFMPEG_STATE: u16 = 1210;
const I_AFTER: u16 = 1301;
const I_FORMAT: u16 = 1302;
const I_QUALITY: u16 = 1303;
const I_DIR: u16 = 1304;
const I_BROWSE: u16 = 1305;
const I_NAME: u16 = 1306;
const I_EDIT: u16 = 1307;
const I_KEYS: u16 = 1308;
const I_TETHER: u16 = 1309;
const I_TETHER_BROWSE: u16 = 1310;
const G_LANG: u16 = 1401;
const G_LANG_FOLDER: u16 = 1402;
const G_EXPLORER_ADD: u16 = 1403;
const G_EXPLORER_REMOVE: u16 = 1404;
const G_EXPLORER_STATE: u16 = 1405;
const G_WEBSITE: u16 = 1406;
const G_SUPPORT: u16 = 1407;

const SNAP_FORMATS: [&str; 6] = ["png", "png16", "jpg", "webp", "tiff", "bmp"];
const SNAP_SDR: [SdrAssume; 4] = [SdrAssume::AsShown, SdrAssume::Bt1886, SdrAssume::Rec709A, SdrAssume::Gamma22];
const AFTER_IDS: [&str; 3] = ["save", "confirm", "clipboard"];

fn languages() -> Vec<String> {
    let mut v = vec!["auto".to_string()];
    v.extend(nv_core::i18n::available());
    v
}

pub fn build_more(form: &Form, s: &Settings) {
    let m = &s.more;
    let tx = nv_core::i18n::t;
    // -- video snapshot
    form.begin_page(PAGE_SNAP);
    form.group(12, 52, 696, 150, tx("snapshot.group_file"));
    form.label(28, 80, 120, 20, tx("snapshot.format"));
    form.combo(N_FORMAT, 150, 76, 330, 160, &[tx("snapshot.f_png"), tx("snapshot.f_png16"), tx("snapshot.f_jpg"), tx("snapshot.f_webp"), tx("snapshot.f_tiff"), tx("snapshot.f_bmp")], SNAP_FORMATS.iter().position(|f| *f == m.snapshot_format).unwrap_or(0));
    form.label(496, 80, 130, 20, tx("snapshot.quality"));
    form.edit(N_QUALITY, 632, 76, 56, 24, &m.snapshot_quality.to_string());
    form.label(28, 114, 120, 20, tx("snapshot.folder"));
    form.edit(N_DIR, 150, 110, 430, 24, &m.snapshot_dir);
    form.button(N_BROWSE, 590, 109, 98, 26, tx("importer.browse"), false);
    form.label(28, 144, 660, 20, tx("snapshot.folder_hint"));
    form.label(28, 172, 120, 20, tx("snapshot.names"));
    form.edit(N_NAME, 150, 168, 330, 24, &m.snapshot_name);
    form.label(490, 172, 200, 20, "{name} {time}");
    form.group(12, 212, 696, 262, tx("snapshot.group_colour"));
    form.label(28, 242, 660, 20, tx("snapshot.sdr_label"));
    form.combo(N_SDR, 28, 264, 660, 120, &[tx("snapshot.sdr_asis"), tx("snapshot.sdr_bt1886"), tx("snapshot.sdr_709a"), tx("snapshot.sdr_22")], SNAP_SDR.iter().position(|v| *v == m.snapshot_sdr).unwrap_or(0));
    form.label(28, 304, 660, 20, tx("snapshot.hdr_label"));
    form.combo(N_HDR, 28, 326, 660, 100, &[tx("snapshot.hdr_tone"), tx("snapshot.hdr_keep")], m.snapshot_keep_hdr as usize);
    form.label(28, 366, 130, 20, tx("snapshot.tonemap"));
    form.combo(N_TONEMAP, 160, 362, 330, 100, &[tx("snapshot.tm_reinhard"), tx("snapshot.tm_hable")], (m.snapshot_tonemap == ToneMap::Hable) as usize);
    form.label(28, 400, 260, 20, tx("snapshot.peak"));
    form.edit(N_PEAK, 290, 396, 70, 24, &m.snapshot_peak.to_string());
    form.label(370, 400, 320, 20, tx("snapshot.peak_hint"));
    form.label_id(N_FFMPEG_STATE, 28, 432, 660, 36, "");
    form.label(12, 484, 696, 60, tx("snapshot.note"));

    // -- import
    form.begin_page(PAGE_IMPORT);
    form.group(12, 52, 696, 250, tx("importer.group_after"));
    form.label(28, 80, 150, 20, tx("importer.after"));
    form.combo(I_AFTER, 180, 76, 400, 120, &[tx("importer.after_save"), tx("importer.after_confirm"), tx("importer.after_clipboard")], AFTER_IDS.iter().position(|a| *a == m.import_after).unwrap_or(1));
    form.label(28, 114, 150, 20, tx("importer.format"));
    let formats: Vec<&str> = nv_core::importer::FORMATS.iter().map(|f| f.1).collect();
    form.combo(I_FORMAT, 180, 110, 400, 200, &formats, nv_core::importer::FORMATS.iter().position(|f| f.0 == m.import_format).unwrap_or(0));
    form.label(28, 148, 150, 20, tx("importer.quality"));
    form.edit(I_QUALITY, 180, 144, 60, 24, &m.import_quality.to_string());
    form.label(28, 182, 150, 20, tx("importer.folder"));
    form.edit(I_DIR, 180, 178, 400, 24, &m.import_dir);
    form.button(I_BROWSE, 590, 177, 98, 26, tx("importer.browse"), false);
    form.label(28, 210, 660, 20, tx("import.folder_hint"));
    form.label(28, 242, 150, 20, tx("importer.names"));
    form.edit(I_NAME, 180, 238, 400, 24, &m.import_name);
    form.label(590, 242, 110, 20, "{source} {date} {time}");
    form.check(I_EDIT, 28, 270, 660, 22, tx("importer.open_editor"), m.import_edit);
    form.group(12, 312, 696, 130, tx("import.group_sources"));
    form.check(I_KEYS, 28, 340, 660, 22, tx("import.keys"), m.screenshot_keys);
    form.label(28, 370, 130, 20, tx("import.tether_folder"));
    form.edit(I_TETHER, 160, 366, 420, 24, &m.tether_dir);
    form.button(I_TETHER_BROWSE, 590, 365, 98, 26, tx("importer.browse"), false);
    form.label(28, 400, 660, 34, tx("import.tether_hint"));

    // -- language and Explorer
    form.begin_page(PAGE_GENERAL);
    form.group(12, 52, 696, 124, tx("general.group_language"));
    form.label(28, 80, 150, 20, tx("general.language"));
    let langs = languages();
    let names: Vec<String> = langs.iter().map(|l| if l == "auto" { tx("general.lang_auto").to_string() } else { l.clone() }).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    form.combo(G_LANG, 180, 76, 300, 160, &name_refs, langs.iter().position(|l| *l == m.language).unwrap_or(0));
    form.button(G_LANG_FOLDER, 490, 75, 198, 26, tx("general.lang_folder"), false);
    form.label(28, 110, 660, 54, tx("general.lang_hint"));
    form.group(12, 186, 696, 150, tx("general.group_explorer"));
    form.label(28, 212, 660, 36, tx("general.explorer_hint"));
    form.button(G_EXPLORER_ADD, 28, 256, 300, 28, tx("general.explorer_add"), false);
    form.button(G_EXPLORER_REMOVE, 338, 256, 200, 28, tx("general.explorer_remove"), false);
    form.label_id(G_EXPLORER_STATE, 28, 296, 660, 30, "");
    form.group(12, 346, 696, 96, tx("general.group_about"));
    form.label(28, 372, 660, 20, &format!("{}  {}   ·   {}", nv_core::about::PROGRAMS.join(" · "), env!("CARGO_PKG_VERSION"), nv_core::i18n::tf("about.made_by", &[&nv_core::about::STUDIO])));
    form.button(G_WEBSITE, 28, 400, 300, 28, nv_core::about::WEBSITE, false);
    form.button(G_SUPPORT, 338, 400, 300, 28, tx("general.support"), false);
    explorer_state(form);
    ffmpeg_state(form, s);
}

fn explorer_state(form: &Form) {
    let on = nv_win::explorer_menu::installed(&nv_win::registry::Layout::real());
    form.set_text(G_EXPLORER_STATE, nv_core::i18n::t(if on { "general.explorer_on" } else { "general.explorer_off" }));
}

fn ffmpeg_state(form: &Form, s: &Settings) {
    nv_core::ffmpeg::configure(&s.more.ffmpeg);
    let found = nv_core::ffmpeg::find();
    let text = match (&found, found.as_ref().and_then(|_| nv_core::ffmpeg::ffprobe())) {
        (Some(p), Some(_)) => nv_core::i18n::tf("snapshot.ffmpeg_ok", &[&p.display()]),
        (Some(_), None) => nv_core::i18n::t("snapshot.ffprobe_missing").to_string(),
        _ => nv_core::i18n::t("snapshot.ffmpeg_missing").to_string(),
    };
    form.set_text(N_FFMPEG_STATE, &text);
}

pub fn click_more(form: &Form, id: u16) -> bool {
    match id {
        N_BROWSE | I_BROWSE | I_TETHER_BROWSE => {
            let box_id = match id {
                N_BROWSE => N_DIR,
                I_BROWSE => I_DIR,
                _ => I_TETHER,
            };
            let cur = form.text(box_id);
            if let Some(p) = nv_win::shell::pick_folder(form.hwnd, nv_core::i18n::t("importer.pick_folder"), Some(std::path::Path::new(cur.trim()))) {
                form.set_text(box_id, &p.display().to_string());
            }
        }
        G_LANG_FOLDER => {
            let dir = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_default().join("NuViewer").join("lang");
            let _ = std::fs::create_dir_all(&dir);
            let sample = dir.join("en-template.txt");
            if !sample.exists() {
                let _ = std::fs::write(&sample, include_str!("../../crates/nv-text/lang/en.txt"));
            }
            nv_win::shell::reveal_in_explorer(&sample);
        }
        G_EXPLORER_ADD => {
            let exts: Vec<String> = nv_codecs::convertible_extensions().iter().map(|e| format!(".{e}")).collect();
            let exe = std::env::current_exe().unwrap_or_default();
            let tx = nv_core::i18n::t;
            match nv_win::explorer_menu::install(&nv_win::registry::Layout::real(), &exe, &exts, tx("explorer.to_png"), tx("explorer.to_jpg")) {
                Ok(()) => {}
                Err(e) => nv_win::shell::message(&e),
            }
            explorer_state(form);
        }
        G_EXPLORER_REMOVE => {
            let exts: Vec<String> = nv_codecs::convertible_extensions().iter().map(|e| format!(".{e}")).collect();
            nv_win::explorer_menu::remove(&nv_win::registry::Layout::real(), &exts);
            explorer_state(form);
        }
        G_WEBSITE => nv_win::shell::open_url(nv_core::about::WEBSITE),
        G_SUPPORT => nv_win::shell::open_url(nv_core::about::SUPPORT),
        _ => return false,
    }
    true
}

pub fn read_more(form: &Form, s: &mut Settings) {
    let m = &mut s.more;
    m.snapshot_format = SNAP_FORMATS[form.combo_index(N_FORMAT).min(SNAP_FORMATS.len() - 1)].to_string();
    m.snapshot_quality = form.text(N_QUALITY).trim().parse::<u8>().map(|v| v.clamp(30, 100)).unwrap_or(m.snapshot_quality);
    m.snapshot_dir = form.text(N_DIR).trim().trim_matches('"').to_string();
    let name = form.text(N_NAME);
    if !name.trim().is_empty() {
        m.snapshot_name = name.trim().to_string();
    }
    m.snapshot_sdr = SNAP_SDR[form.combo_index(N_SDR).min(SNAP_SDR.len() - 1)];
    m.snapshot_keep_hdr = form.combo_index(N_HDR) == 1;
    m.snapshot_tonemap = if form.combo_index(N_TONEMAP) == 1 { ToneMap::Hable } else { ToneMap::Reinhard };
    m.snapshot_peak = form.text(N_PEAK).trim().parse::<u32>().map(|v| v.clamp(100, 10000)).unwrap_or(m.snapshot_peak);
    m.import_after = AFTER_IDS[form.combo_index(I_AFTER).min(2)].to_string();
    m.import_format = nv_core::importer::FORMATS[form.combo_index(I_FORMAT).min(nv_core::importer::FORMATS.len() - 1)].0.to_string();
    m.import_quality = form.text(I_QUALITY).trim().parse::<u8>().map(|v| v.clamp(30, 100)).unwrap_or(m.import_quality);
    m.import_dir = form.text(I_DIR).trim().trim_matches('"').to_string();
    let n = form.text(I_NAME);
    if !n.trim().is_empty() {
        m.import_name = n.trim().to_string();
    }
    m.import_edit = form.is_checked(I_EDIT);
    m.screenshot_keys = form.is_checked(I_KEYS);
    m.tether_dir = form.text(I_TETHER).trim().trim_matches('"').to_string();
    let langs = languages();
    m.language = langs.get(form.combo_index(G_LANG)).cloned().unwrap_or_else(|| "auto".into());
}
