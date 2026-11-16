#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NuImporter: bring pictures in from wherever they are.
//!
//!  1. a scanner (Windows' own scan dialog, through WIA),
//!  2. a screenshot (the window hides for a moment; drag a rectangle, or take the whole screen; NuQuick's hot keys do the same
//!     from anywhere),
//!  3. the clipboard (a picture, or files copied in Explorer),
//!  4. a camera on a cable (WIA: take a picture from here) or any camera that writes into a folder (tether: wired or wireless,
//!     the pictures are imported as they appear),
//!  5. a PowerPoint / Word / OpenDocument / zip / epub file, or an ISO disc image: the pictures inside,
//!  6. a folder of pictures.
//!
//! What happens next is the user's choice (Settings > Import, or the three boxes in this window): saved at once, opened in
//! NuEditor first and saved when it is closed, or put on the clipboard; in PNG, JPEG, WebP, BMP, TIFF, QOI or TGA, or as they were.
//! Started from NuGallery (Tools > Import) or on its own: `nuimporter.exe [--from gallery] [--into <folder>] [--shot region|full]`.

use nv_core::i18n::{t, tf};
use nv_core::importer::{self, After, Source};
use nv_core::prefs::Prefs;
use nv_core::settings::Settings;
use nv_win::capture::{self, Shot};
use nv_win::ui::{Form, ID_CANCEL, SELECTION_CHANGED};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const ID_BACK: u16 = 90;
const ID_SCAN: u16 = 100;
const ID_SHOT_REGION: u16 = 101;
const ID_SHOT_FULL: u16 = 102;
const ID_CLIP: u16 = 103;
const ID_CAM_SHOOT: u16 = 104;
const ID_CAM_WATCH: u16 = 105;
const ID_FILE: u16 = 106;
const ID_FOLDER: u16 = 107;
const ID_AFTER: u16 = 110;
const ID_FORMAT: u16 = 111;
const ID_QUALITY: u16 = 112;
const ID_DIR: u16 = 113;
const ID_BROWSE: u16 = 114;
const ID_NAME: u16 = 115;
const ID_EDIT_AFTER: u16 = 116;
const ID_LIST: u16 = 120;
const ID_STATUS: u16 = 121;
const ID_SHOW_FOLDER: u16 = 130;
const ID_EDIT_SELECTED: u16 = 131;
const ID_RAW_LOOK: u16 = 132;
const ID_TICK: u16 = 999;

const AFTER_IDS: [&str; 3] = ["save", "confirm", "clipboard"];

/// One thing that arrived.
enum Incoming {
    Pixels(Shot),
    /// A picture file's bytes and the name it had (inside a container, or on disk).
    File(String, Vec<u8>),
}

struct State {
    from_gallery: bool,
    start_dir: PathBuf,
    pictures: Option<PathBuf>,
    log: Vec<(String, Option<PathBuf>)>,
    watch: Option<(PathBuf, importer::Watch, Arc<AtomicBool>)>,
    temp_seq: u32,
}

fn main() {
    nv_win::crash::install();
    let s = Settings::load();
    nv_core::i18n::init(&s.more.language, &nv_win::ui_language());
    nv_core::ffmpeg::configure(&s.more.ffmpeg);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg_after = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let from_gallery = arg_after("--from").as_deref() == Some("gallery");
    let start_dir = arg_after("--into").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_default();
    let shot = arg_after("--shot");
    if let Err(e) = run(from_gallery, start_dir, shot) {
        nv_win::shell::message(&tf("importer.cant_start", &[&e]));
    }
}

fn utc() -> i32 {
    nv_win::utc_offset_minutes()
}

fn run(from_gallery: bool, start_dir: PathBuf, shot_mode: Option<String>) -> Result<(), String> {
    let _ = nv_win::com_init();
    let mut st = State { from_gallery, start_dir, pictures: nv_win::files::known_folder("pictures"), log: Vec::new(), watch: None, temp_seq: 0 };
    let prefs = Settings::load().more;
    nv_codecs::set_raw_mode(Settings::load().raw_mode);

    let form = Form::new(t("importer.title"), 780, 640)?;
    form.button(ID_BACK, 14, 10, 110, 28, t("importer.back"), false);
    form.label(136, 14, 520, 22, t("importer.heading"));
    form.button(ID_CANCEL, 656, 10, 110, 28, t("importer.close"), false);

    form.group(12, 46, 756, 132, t("importer.group_bring"));
    form.button(ID_SCAN, 28, 70, 176, 30, t("importer.scanner"), false);
    form.button(ID_SHOT_REGION, 212, 70, 176, 30, t("importer.shot_region"), false);
    form.button(ID_SHOT_FULL, 396, 70, 176, 30, t("importer.shot_full"), false);
    form.button(ID_CLIP, 580, 70, 176, 30, t("importer.clipboard"), false);
    form.button(ID_CAM_SHOOT, 28, 108, 176, 30, t("importer.cam_shoot"), false);
    form.button(ID_CAM_WATCH, 212, 108, 176, 30, t("importer.cam_watch"), false);
    form.button(ID_FILE, 396, 108, 176, 30, t("importer.from_file"), false);
    form.button(ID_FOLDER, 580, 108, 176, 30, t("importer.from_folder"), false);
    form.label(28, 146, 728, 26, t("importer.hint_sources"));

    form.group(12, 186, 756, 168, t("importer.group_after"));
    form.label(28, 214, 130, 20, t("importer.after"));
    form.combo(ID_AFTER, 160, 210, 320, 120, &[t("importer.after_save"), t("importer.after_confirm"), t("importer.after_clipboard")], AFTER_IDS.iter().position(|a| *a == prefs.import_after).unwrap_or(1));
    form.check(ID_EDIT_AFTER, 500, 212, 256, 22, t("importer.open_editor"), prefs.import_edit);
    form.label(28, 248, 130, 20, t("importer.format"));
    let formats: Vec<&str> = importer::FORMATS.iter().map(|f| f.1).collect();
    form.combo(ID_FORMAT, 160, 244, 320, 200, &formats, importer::FORMATS.iter().position(|f| f.0 == prefs.import_format).unwrap_or(0));
    form.label(500, 248, 150, 20, t("importer.quality"));
    form.edit(ID_QUALITY, 656, 244, 60, 24, &prefs.import_quality.to_string());
    form.label(28, 282, 130, 20, t("importer.folder"));
    form.edit(ID_DIR, 160, 278, 480, 24, &importer::folder_for(&prefs, st.pictures.clone()).display().to_string());
    form.button(ID_BROWSE, 650, 277, 106, 26, t("importer.browse"), false);
    form.label(28, 316, 130, 20, t("importer.names"));
    form.edit(ID_NAME, 160, 312, 320, 24, &prefs.import_name);
    form.label(490, 316, 266, 20, t("importer.names_hint"));

    form.group(12, 362, 756, 222, t("importer.group_log"));
    form.list(ID_LIST, 28, 386, 724, 150);
    form.label_id(ID_STATUS, 28, 544, 724, 34, t("importer.ready"));

    form.check(ID_RAW_LOOK, 14, 600, 456, 22, t("importer.raw_look"), Settings::load().raw_mode.is_camera_look());
    form.button(ID_SHOW_FOLDER, 480, 596, 140, 30, t("importer.show_folder"), false);
    form.button(ID_EDIT_SELECTED, 626, 596, 140, 30, t("importer.edit_selected"), false);

    // A hot key of NuQuick (or --shot) asked for a screenshot at once.
    let mut first = shot_mode;
    let hwnd_value = form.hwnd.0 as isize;
    if first.is_some() {
        nv_win::post_command(hwnd_value, ID_TICK); // wakes the handler once the window runs, so the screenshot starts at once
    }
    form.run(&mut |f, id| {
        if let Some(mode) = first.take() {
            screenshot(f, &mut st, mode == "region");
        }
        match id {
            ID_BACK => {
                go_back(&st);
                return true;
            }
            ID_CANCEL => {
                stop_watch(&mut st);
                persist(f);
                return true;
            }
            ID_SCAN => scan(f, &mut st, false),
            ID_SHOT_REGION => screenshot(f, &mut st, true),
            ID_SHOT_FULL => screenshot(f, &mut st, false),
            ID_CLIP => from_clipboard(f, &mut st),
            ID_CAM_SHOOT => scan(f, &mut st, true),
            ID_CAM_WATCH => toggle_watch(f, &mut st, hwnd_value),
            ID_FILE => from_file(f, &mut st),
            ID_FOLDER => from_folder(f, &mut st),
            ID_TICK => tick(f, &mut st),
            ID_RAW_LOOK => {
                // Camera look (the camera's own processing) or sensor data: shared with the other programs.
                let mut s = Settings::load();
                s.raw_mode = if f.is_checked(ID_RAW_LOOK) { nv_codecs::RawMode::Preview } else { nv_codecs::RawMode::Full };
                let _ = s.save();
                nv_codecs::set_raw_mode(s.raw_mode);
                status(f, s.raw_mode.look_name());
            }
            ID_BROWSE => {
                if let Some(p) = nv_win::shell::pick_folder(f.hwnd, t("importer.pick_folder"), Some(Path::new(f.text(ID_DIR).trim()))) {
                    f.set_text(ID_DIR, &p.display().to_string());
                    persist(f);
                }
            }
            ID_SHOW_FOLDER => nv_win::shell::reveal_in_explorer(Path::new(f.text(ID_DIR).trim())),
            ID_EDIT_SELECTED => {
                if let Some(i) = f.list_index(ID_LIST) {
                    if let Some((_, Some(p))) = st.log.get(i) {
                        open_in_editor(p, None);
                    }
                }
            }
            x if x == (ID_AFTER | SELECTION_CHANGED) || x == (ID_FORMAT | SELECTION_CHANGED) => persist(f),
            _ => {}
        }
        false
    });
    stop_watch(&mut st);
    Ok(())
}

/// The choices in the window, as settings (and saved: the next start has them).
fn prefs_of(f: &Form) -> Prefs {
    let mut p = Settings::load().more;
    p.import_after = AFTER_IDS[f.combo_index(ID_AFTER).min(2)].to_string();
    p.import_format = importer::FORMATS[f.combo_index(ID_FORMAT).min(importer::FORMATS.len() - 1)].0.to_string();
    p.import_quality = f.text(ID_QUALITY).trim().parse::<u8>().ok().filter(|q| (30..=100).contains(q)).unwrap_or(p.import_quality);
    p.import_dir = f.text(ID_DIR).trim().to_string();
    let name = f.text(ID_NAME);
    if !name.trim().is_empty() {
        p.import_name = name.trim().to_string();
    }
    p.import_edit = f.is_checked(ID_EDIT_AFTER);
    p
}

fn persist(f: &Form) {
    let more = prefs_of(f);
    let mut s = Settings::load();
    s.more = more;
    let _ = s.save();
}

fn status(f: &Form, text: &str) {
    f.set_text(ID_STATUS, text);
}

fn log(f: &Form, st: &mut State, text: String, path: Option<PathBuf>) {
    let time = {
        let (_, _, _, h, m, s) = nv_core::batch::civil_from_unix(nv_core::batch::unix_of(std::time::SystemTime::now()), utc());
        format!("{h:02}:{m:02}:{s:02}")
    };
    // newest first
    st.log.insert(0, (format!("{time}   {text}"), path));
    let rows: Vec<String> = st.log.iter().map(|(t, _)| t.clone()).collect();
    f.list_set(ID_LIST, &rows, Some(0));
}

// ---- the sources ---------------------------------------------------------------------------------------------------------

fn screenshot(f: &Form, st: &mut State, region: bool) {
    // The window steps aside (and the desktop repaints) before the screen is taken.
    nv_win::set_visible(f.hwnd, false);
    std::thread::sleep(std::time::Duration::from_millis(350));
    let (left, top, _, _) = capture::virtual_screen();
    let shot = capture::capture_desktop();
    let result = match shot {
        Ok(full) => {
            if region {
                match nv_win::regionpick::pick(&full, left, top) {
                    Some(r) => Some(nv_win::regionpick::crop(&full, r)),
                    None => None,
                }
            } else {
                Some(full)
            }
        }
        Err(e) => {
            nv_win::set_visible(f.hwnd, true);
            status(f, &tf("importer.shot_failed", &[&e]));
            return;
        }
    };
    nv_win::set_visible(f.hwnd, true);
    match result {
        Some(s) => deliver(f, st, Source::Screenshot, vec![Incoming::Pixels(s)]),
        None => status(f, t("importer.cancelled")),
    }
}

fn from_clipboard(f: &Form, st: &mut State) {
    if let Some(s) = capture::clipboard_image(f.hwnd) {
        return deliver(f, st, Source::Clipboard, vec![Incoming::Pixels(s)]);
    }
    let files: Vec<PathBuf> = capture::clipboard_files(f.hwnd).into_iter().filter(|p| p.is_file()).collect();
    if files.is_empty() {
        return status(f, t("importer.clipboard_empty"));
    }
    let items = incoming_from_paths(f, &files);
    deliver(f, st, Source::Clipboard, items);
}

/// The scanner's / camera's own Windows dialog (or a direct "take a picture" for a camera).
fn scan(f: &Form, st: &mut State, camera: bool) {
    let out = std::env::temp_dir().join(format!("nuviewer-wia-{}-{}.png", std::process::id(), st.temp_seq));
    st.temp_seq += 1;
    let _ = std::fs::remove_file(&out);
    status(f, t(if camera { "importer.cam_working" } else { "importer.scan_working" }));
    let try_script = |script: String| -> Option<String> {
        use std::os::windows::process::CommandExt;
        let o = std::process::Command::new("powershell.exe").args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-Command", &script]).creation_flags(0x0800_0000).output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr))
    };
    let mut said = None;
    if camera {
        said = try_script(importer::wia_take_picture_script(&out));
    }
    if !out.is_file() {
        said = try_script(importer::wia_script(&out, camera)).or(said);
    }
    match std::fs::read(&out) {
        Ok(bytes) => {
            let _ = std::fs::remove_file(&out);
            deliver(f, st, if camera { Source::Camera } else { Source::Scanner }, vec![Incoming::File("scan.png".into(), bytes)]);
        }
        Err(_) => {
            let why = said.map(|s| s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string()).unwrap_or_default();
            status(f, &tf("importer.nothing_acquired", &[&why]));
        }
    }
}

fn toggle_watch(f: &Form, st: &mut State, hwnd_value: isize) {
    if st.watch.is_some() {
        stop_watch(st);
        f.set_text(ID_CAM_WATCH, t("importer.cam_watch"));
        return status(f, t("importer.watch_stopped"));
    }
    let configured = Settings::load().more.tether_dir;
    let start = if configured.trim().is_empty() { None } else { Some(PathBuf::from(configured.trim())) };
    // NUIMPORTER_TETHER_DIR stands in for the folder dialog (tests).
    let chosen = std::env::var_os("NUIMPORTER_TETHER_DIR").map(PathBuf::from).or_else(|| nv_win::shell::pick_folder(f.hwnd, t("importer.pick_tether"), start.as_deref()));
    let Some(dir) = chosen else { return };
    let mut s = Settings::load();
    s.more.tether_dir = dir.display().to_string();
    let _ = s.save();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(600));
            if !nv_win::post_command(hwnd_value, ID_TICK) {
                break;
            }
        }
    });
    st.watch = Some((dir.clone(), importer::Watch::new(&dir), stop));
    f.set_text(ID_CAM_WATCH, t("importer.cam_stop"));
    status(f, &tf("importer.watching", &[&dir.display()]));
}

fn stop_watch(st: &mut State) {
    if let Some((_, _, stop)) = st.watch.take() {
        stop.store(true, Ordering::Relaxed);
    }
}

fn tick(f: &Form, st: &mut State) {
    let Some((dir, watch, _)) = st.watch.as_mut() else { return };
    let ready = watch.poll(dir);
    if ready.is_empty() {
        return;
    }
    let items = incoming_from_paths(f, &ready);
    deliver(f, st, Source::Camera, items);
}

fn from_file(f: &Form, st: &mut State) {
    let filter = "Pictures, Office files, zip and ISO\0*.pptx;*.docx;*.xlsx;*.odp;*.odt;*.zip;*.cbz;*.epub;*.iso;*.img;*.jpg;*.jpeg;*.png;*.webp;*.bmp;*.tif;*.tiff;*.gif;*.heic;*.avif;*.dng;*.cr2;*.nef;*.arw;*.raf\0All files\0*.*\0\0";
    // NUIMPORTER_FILE stands in for the file dialog (tests).
    let chosen = std::env::var_os("NUIMPORTER_FILE").map(PathBuf::from).or_else(|| nv_win::shell::open_dialog(f.hwnd, t("importer.pick_file"), filter, None));
    let Some(p) = chosen else { return };
    if importer::is_container(&p) {
        status(f, &tf("importer.reading", &[&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]));
        match importer::pictures_in(&p, 500, &Settings::load().more.seven_zip) {
            Ok(v) if !v.is_empty() => {
                let items: Vec<Incoming> = v.into_iter().map(|piece| Incoming::File(piece.name, piece.bytes)).collect();
                deliver(f, st, Source::Archive, items)
            }
            Ok(_) => status(f, t("importer.no_pictures")),
            Err(e) => status(f, &tf("importer.read_failed", &[&e])),
        }
    } else {
        let items = incoming_from_paths(f, &[p]);
        deliver(f, st, Source::Folder, items);
    }
}

fn from_folder(f: &Form, st: &mut State) {
    let start = if st.start_dir.as_os_str().is_empty() { None } else { Some(st.start_dir.clone()) };
    let Some(dir) = nv_win::shell::pick_folder(f.hwnd, t("importer.pick_folder_from"), start.as_deref()) else { return };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|e| nv_codecs::is_supported_ext(&e.to_string_lossy()) && !nv_codecs::is_code_ext(&e.to_string_lossy()))).collect()).unwrap_or_default();
    files.sort();
    if files.is_empty() {
        return status(f, t("importer.no_pictures"));
    }
    let items = incoming_from_paths(f, &files);
    deliver(f, st, Source::Folder, items);
}

fn incoming_from_paths(_f: &Form, paths: &[PathBuf]) -> Vec<Incoming> {
    paths.iter().filter_map(|p| std::fs::read(p).ok().map(|b| Incoming::File(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into()), b))).collect()
}

// ---- what happens next ---------------------------------------------------------------------------------------------------------

fn shot_of(item: &Incoming) -> Option<Shot> {
    match item {
        Incoming::Pixels(s) => Some(Shot { width: s.width, height: s.height, rgba: s.rgba.clone() }),
        Incoming::File(name, bytes) => {
            let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let img = nv_codecs::decode_bytes_hint(bytes, &ext).ok()?.finish_for_cpu();
            Some(Shot { width: img.width, height: img.height, rgba: img.rgba })
        }
    }
}

fn deliver(f: &Form, st: &mut State, source: Source, items: Vec<Incoming>) {
    if items.is_empty() {
        return status(f, t("importer.no_pictures"));
    }
    let prefs = prefs_of(f);
    persist(f);
    let mut after = After::from_id(&prefs.import_after);
    let n = items.len();
    // Opening dozens of pictures in the editor one after another helps nobody: with several, "ask first" asks once.
    if after == After::Confirm && n > 1 {
        let msg = tf("importer.ask_many", &[&n, &importer::folder_for(&prefs, st.pictures.clone()).display()]);
        match nv_win::shell::ask3(f.hwnd, &msg) {
            nv_win::shell::Answer::Yes => after = After::Save,
            _ => return status(f, t("importer.cancelled")),
        }
    }
    match after {
        After::Clipboard => {
            let Some(shot) = items.last().and_then(shot_of) else { return status(f, t("importer.cant_read")) };
            match capture::clipboard_set_image(f.hwnd, &shot) {
                Ok(()) => {
                    log(f, st, tf("importer.log_clipboard", &[&source.name(), &shot.width, &shot.height]), None);
                    status(f, t("importer.on_clipboard"));
                }
                Err(e) => status(f, &e),
            }
        }
        After::Confirm => {
            let Some(item) = items.first() else { return };
            let Some(shot) = shot_of(item) else { return status(f, t("importer.cant_read")) };
            let dir = std::env::temp_dir().join("NuViewer").join("import");
            let _ = std::fs::create_dir_all(&dir);
            let temp = dir.join(format!("{}-{}-{}.png", source.name(), std::process::id(), st.temp_seq));
            st.temp_seq += 1;
            let png = nv_editor::convert::encode(&nv_editor::raster::Pixels::new(shot.width, shot.height, shot.rgba), nv_editor::convert::Out::Png, [255; 3], None);
            match png.and_then(|b| std::fs::write(&temp, b).map_err(|e| e.to_string())) {
                Ok(()) => {
                    open_in_editor(&temp, Some(source.name()));
                    log(f, st, tf("importer.log_editing", &[&source.name(), &shot.width, &shot.height]), None);
                    status(f, t("importer.editing"));
                }
                Err(e) => status(f, &e),
            }
        }
        After::Save => {
            let (mut saved, mut failed, mut last) = (0, 0, None);
            for item in items {
                let r = match item {
                    Incoming::Pixels(s) => importer::save_picture(&prefs, st.pictures.clone(), source, s.width, s.height, s.rgba, utc()),
                    Incoming::File(name, bytes) => importer::save_file(&prefs, st.pictures.clone(), source, &name, &bytes, utc()),
                };
                match r {
                    Ok(p) => {
                        saved += 1;
                        log(f, st, tf("importer.log_saved", &[&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]), Some(p.clone()));
                        last = Some(p);
                    }
                    Err(_) => failed += 1,
                }
            }
            status(f, &if failed == 0 { tf("importer.saved_n", &[&saved]) } else { tf("importer.saved_n_failed", &[&saved, &failed]) });
            if prefs.import_edit && saved == 1 {
                if let Some(p) = last {
                    open_in_editor(&p, None);
                }
            }
        }
    }
}

fn open_in_editor(path: &Path, import_source: Option<&str>) {
    let Some(exe) = std::env::current_exe().ok().map(|e| e.with_file_name("nueditor.exe")).filter(|p| p.is_file()) else {
        return nv_win::shell::message(t("importer.no_editor"));
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(path);
    if let Some(s) = import_source {
        cmd.arg("--import").arg(s);
    }
    if let Ok(child) = cmd.spawn() {
        nv_win::instance::allow_foreground(child.id());
    }
}

fn go_back(st: &State) {
    use nv_win::instance::{send_to, OpenRequest, Program};
    let dir = if st.start_dir.as_os_str().is_empty() { None } else { Some(st.start_dir.display().to_string()) };
    let req = OpenRequest { path: dir, gallery: true, from_gallery: false };
    if !send_to(Program::Gallery, &req) {
        if let Some(exe) = std::env::current_exe().ok().map(|e| e.with_file_name("nugallery.exe")).filter(|p| p.is_file()) {
            let mut cmd = std::process::Command::new(exe);
            if let Some(d) = &req.path {
                cmd.arg(d);
            }
            if let Ok(c) = cmd.spawn() {
                nv_win::instance::allow_foreground(c.id());
            }
        }
    }
    let _ = st.from_gallery;
}
