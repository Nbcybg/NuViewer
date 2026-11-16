//! NuEditor: the picture editor of NuViewer, as a program of its own (like NuGallery) so the viewer and the gallery stay
//! small and fast. It is opened from either of them (`nueditor.exe <file> --from viewer|gallery`) and, when it closes, puts
//! the user back where they came from, on the picture they were editing.
//!
//! Start-up is arranged like the viewer's: the window, the GPU device and the decoding of the picture all start at once;
//! the first frame is painted on the CPU the moment the picture is decoded, and the GPU takes over when it is ready.
//! Edits are a *recipe* (see `nv_editor::recipe`), applied to a screen-sized proxy of the picture while you work, and to the
//! full picture only when you save.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod flow;
#[path = "../../nuviewer/src/colour.rs"]
#[allow(dead_code)]
mod colour;
#[path = "../../nugallery/src/gfx.rs"]
#[allow(dead_code)]
mod gfx;

use app::{App, Msg, Options};
use nv_core::settings::Settings;
use nv_win::instance::{self, Instance, OpenRequest, Program};
use nv_win::{Event, WindowHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

fn main() {
    let started = Instant::now();
    nv_win::crash::install();
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let mut path_arg: Option<PathBuf> = None;
    let (mut from, mut angle): (Option<String>, Option<f32>) = (None, None);
    // `--import <source>`: the picture was just brought in by NuImporter and is not saved anywhere yet (a temporary file).
    let mut import: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        match s.as_ref() {
            "--from" => from = it.next().map(|v| v.to_string_lossy().into_owned()),
            "--angle" => angle = it.next().and_then(|v| v.to_string_lossy().parse().ok()),
            "--import" => import = it.next().map(|v| v.to_string_lossy().into_owned()),
            _ if !s.starts_with("--") && path_arg.is_none() => path_arg = Some(instance::absolute(Path::new(a))),
            _ => {}
        }
    }

    // Nothing to edit: the gallery is the place to pick a picture.
    let Some(path) = path_arg.filter(|p| p.is_file()) else {
        if let Ok(exe) = std::env::current_exe() {
            let g = exe.with_file_name("nugallery.exe");
            if g.is_file() {
                let _ = std::process::Command::new(g).spawn();
            }
        }
        return;
    };
    let origin = flow::Origin::parse(from.as_deref(), flag("--via-gallery"));

    let settings = Settings::load();
    nv_core::i18n::init(&settings.more.language, &nv_win::ui_language());
    colour::apply_at_startup(&settings);

    // One editor window: a second launch hands its picture to the first.
    if !flag("--new-window") && import.is_none() {
        let request = OpenRequest { path: Some(path.display().to_string()), gallery: false, from_gallery: false };
        if let Instance::Forwarded = instance::claim_or_forward_as(Program::Editor, &request) {
            return;
        }
    }

    nv_win::prewarm_siblings();
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
    // 2. The picture.
    let (tx, rx) = mpsc::channel::<Msg>();
    app::spawn_decode(&tx, &wake, path.clone());

    let opts = Options { windowed: flag("--windowed"), origin, start_angle: angle, import };
    let fullscreen = !opts.windowed && nv_win::winstate::load().map(|s| s.fullscreen).unwrap_or(settings.start_fullscreen);
    let result = nv_win::run_as(Program::Editor, "NuEditor", 1280, 800, fullscreen, move |hwnd: WindowHandle, w, h| {
        hwnd_slot.store(hwnd.0 as isize, Ordering::Release);
        let mut app = App::new(hwnd, (w, h), nv_win::dpi_scale(hwnd), wake, gpu_rx, tx, rx, settings, opts, started, path);
        Ok(Box::new(move |ev: Event| app.handle(ev)) as Box<dyn FnMut(Event)>)
    });
    if let Err(e) = result {
        eprintln!("NuEditor error: {e}");
        std::process::exit(1);
    }
}
