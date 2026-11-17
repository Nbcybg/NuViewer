//! NuGallery: the folder browser and gallery of NuViewer, as a program of its own. It shows folders as thumbnails, a
//! detailed list or a plain list, with the folder tree, details and EXIF on the screen edges and the file-manager
//! commands in the right-click menu. Pictures open in NuViewer (`nuviewer.exe <file> --from-gallery`), which brings the
//! gallery back, on the last picture seen, when it is closed.
//!
//! Start-up is arranged for speed: the window, the GPU device, the folder listing and the thumbnail workers all start at
//! once, and thumbnails that are ready before the GPU is wait in memory to be put on screen at the first frame.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod batch_ui;
mod chrome;
#[path = "../../nuviewer/src/colour.rs"]
#[allow(dead_code)]
mod colour;
mod cpu;
mod draw;
mod gfx;
mod hints;
mod grid;
mod info;
mod input;
mod menus;
mod model;
mod ops;
mod panels;
mod preview;
mod state;
mod tools;
mod tree;
mod treeui;
mod undo;

use app::{App, Msg, Options};
use nv_core::settings::Settings;
use nv_core::Loader;
use nv_win::{Event, WindowHandle};
use state::Persist;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Where to start when nothing was asked for: the folder of last time, else Pictures, else the home folder.
fn default_dir(persist: &Persist) -> PathBuf {
    persist
        .dir
        .clone()
        .filter(|d| d.is_dir())
        .or_else(|| nv_win::files::known_folder("pictures").filter(|d| d.is_dir()))
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from).filter(|d| d.is_dir()))
        .unwrap_or_else(|| PathBuf::from("C:\\"))
}

fn main() {
    let started = Instant::now();
    nv_win::crash::install();
    let early = nv_core::settings::Settings::load();
    nv_core::i18n::init(&early.more.language, &nv_win::ui_language());
    nv_core::ffmpeg::configure(&early.more.ffmpeg);
    nv_core::posters::register(); // video frames and sound waveforms for the thumbnails (loaded on first use)
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let mut path_arg: Option<PathBuf> = None;
    let mut bench_out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        if s == "--bench-out" {
            bench_out = it.next().map(PathBuf::from);
        } else if !s.starts_with("--") && path_arg.is_none() {
            path_arg = Some(nv_win::instance::absolute(Path::new(a)));
        }
    }

    let settings = Settings::load();
    colour::apply_at_startup(&settings);
    nv_codecs::set_model_textures(settings.model_textures);
    nv_core::folder::set_include_documents(true);

    // One gallery window: a second launch hands its folder to the first.
    if !flag("--new-window") {
        let request = nv_win::instance::OpenRequest { path: path_arg.as_ref().map(|p| p.display().to_string()), gallery: true, from_gallery: false };
        if let nv_win::instance::Instance::Forwarded = nv_win::instance::claim_or_forward_as(nv_win::instance::Program::Gallery, &request) {
            return;
        }
    }

    let persist = Persist::load();
    let (start_dir, select): (PathBuf, Vec<PathBuf>) = match &path_arg {
        Some(p) if p.is_dir() => (p.clone(), Vec::new()),
        Some(p) if p.is_file() => (p.parent().map(Path::to_path_buf).unwrap_or_else(|| default_dir(&persist)), vec![p.clone()]),
        _ => (default_dir(&persist), Vec::new()),
    };

    // Everything slow starts now, before the window exists. Worker threads can't wake the window until its handle is
    // known; they go through this slot, and the window handler polls once when it starts, so nothing is missed.
    let hwnd_slot = Arc::new(AtomicIsize::new(0));
    let wake: Arc<dyn Fn() + Send + Sync> = {
        let slot = hwnd_slot.clone();
        Arc::new(move || {
            let h = slot.load(Ordering::Acquire);
            if h != 0 {
                nv_win::post_user_raw(h);
            }
        })
    };
    // 1. The GPU driver (~200+ ms) on its own thread.
    let (gpu_tx, gpu_rx) = mpsc::channel();
    {
        let wake = wake.clone();
        std::thread::spawn(move || {
            let _ = gpu_tx.send(nv_render::create_device());
            wake();
        });
    }
    // 2. The folder listing.
    let (tx, rx) = mpsc::channel::<Msg>();
    {
        let (tx, wake, dir, select, hidden) = (tx.clone(), wake.clone(), start_dir.clone(), select.clone(), persist.hidden);
        std::thread::spawn(move || {
            let result = model::scan(&dir, hidden);
            let _ = tx.send(Msg::Scanned { gen: 1, dir, result, select, keep_view: false });
            wake();
        });
    }
    // 3. The thumbnail workers (idle until the first request).
    let thumbs = Loader::new(app::worker_threads(), false, wake.clone(), Arc::new(model::thumb_work));
    nv_win::prewarm_siblings();
    // Housekeeping long after start-up, so it never competes with the first thumbnails.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(8));
        nv_core::thumbs::prune_cache();
        nv_core::archive_tools::prune_cache(1024 * 1024 * 1024);
        nv_editor::save::prune_backups(&ops::backup_dir(), Duration::from_secs(7 * 24 * 3600));
    });

    let opts = Options { windowed: flag("--windowed"), bench_out, bench_exit: flag("--bench-exit") };
    let fullscreen = !opts.windowed && nv_win::winstate::load().map(|s| s.fullscreen).unwrap_or(settings.start_fullscreen);
    let result = nv_win::run_as(nv_win::instance::Program::Gallery, "NuGallery", 1280, 800, fullscreen, move |hwnd: WindowHandle, w, h| {
        hwnd_slot.store(hwnd.0 as isize, Ordering::Release);
        let mut app = App::new(hwnd, (w, h), nv_win::dpi_scale(hwnd), wake, gpu_rx, tx, rx, thumbs, persist, settings, opts, started);
        app.loading = Some(start_dir);
        Ok(Box::new(move |ev: Event| app.handle(ev)) as Box<dyn FnMut(Event)>)
    });
    if let Err(e) = result {
        eprintln!("NuGallery error: {e}");
        std::process::exit(1);
    }
}
