//! The selection lookup on a thread of its own. It talks to Explorer through COM, which took from 1.5 ms to over 500 ms
//! (Explorer busy, many windows open); the user interface and the keyboard hook must never wait for that.
//! Requests are coalesced (only the newest is answered) and the answer is posted to the agent window.

use crate::explorer::Lookup;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub struct Req {
    pub fg: isize,
    pub desktop: bool,
    pub any: bool,
}

static TX: Mutex<Option<Sender<Req>>> = Mutex::new(None);
static RESULT: Mutex<Option<Option<PathBuf>>> = Mutex::new(None);

pub fn start(agent: isize, only_under: Option<PathBuf>) {
    let (tx, rx) = channel::<Req>();
    *TX.lock().unwrap() = Some(tx);
    std::thread::Builder::new()
        .name("nuquick-lookup".into())
        .spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
            let mut lookup = Lookup::new();
            while let Ok(mut req) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    req = newer;
                }
                let t = std::time::Instant::now();
                let path = lookup.selected(HWND(req.fg as *mut _), req.desktop, req.any, only_under.as_deref());
                crate::log(&format!("lookup {:.1} ms -> {}", t.elapsed().as_secs_f64() * 1000.0, path.as_deref().map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).unwrap_or("none".into())));
                *RESULT.lock().unwrap() = Some(path);
                unsafe {
                    let _ = PostMessageW(Some(HWND(agent as *mut _)), crate::WM_LOOKED, WPARAM(0), LPARAM(0));
                }
            }
        })
        .expect("lookup thread");
}

pub fn request(r: Req) {
    if let Some(tx) = TX.lock().unwrap().as_ref() {
        let _ = tx.send(r);
    }
}

/// The newest answer, once.
pub fn take() -> Option<Option<PathBuf>> {
    RESULT.lock().unwrap().take()
}
