//! NuQuick: Space-bar preview for Explorer and the desktop.
//!
//! Resident design (see docs/nuquick.md). Threads, so that nothing the user types or sees ever waits for Explorer:
//! - the main thread: a hidden window, the tray icon, timers, the preview window, two cheap WinEvent hooks;
//! - the keyboard-hook thread (`keys.rs`): the low-level hook, atomics only;
//! - the lookup thread (`lookup.rs`): asks Explorer (COM) which item is selected;
//! - the decode thread (`view.rs`): decodes a picture to a screen-sized bitmap.
//! The keyboard hook and the selection WinEvent hook exist only while Explorer / the desktop is in front. Space on a picture
//! shows it in a borderless window that does NOT take the focus, so Explorer's arrow keys keep working and the preview follows
//! the selection (a card for anything that is not a picture) until Space / Esc closes it. Enter opens it in NuViewer.
#![windows_subsystem = "windows"]

mod card;
mod explorer;
mod keys;
mod lookup;
mod view;

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Instant;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const AGENT_CLASS: PCWSTR = nv_win::quick::AGENT_CLASS;

pub const WM_SPACE: u32 = WM_APP + 1; // posted by the keyboard hook: Space was taken
pub const WM_CLOSE_REQ: u32 = WM_APP + 2;
pub const WM_OPEN_REQ: u32 = WM_APP + 3;
const WM_SEL: u32 = WM_APP + 4; // posted by the selection WinEvent hook
pub const WM_LOOKED: u32 = WM_APP + 6; // posted by the lookup thread
const WM_TRAY: u32 = WM_APP + 7;
const WM_TEST_SPACE: u32 = WM_APP + 10; // tests: same as Space pressed in the foreground Explorer window
const WM_TEST_QUIT: u32 = WM_APP + 11;

const T_PREFETCH: usize = 1;
const T_TRIM: usize = 3;
const T_POLL: usize = 4;

const ID_PAUSE: usize = 1;
const ID_STARTUP: usize = 2;
const ID_QUIT: usize = 3;
const ID_SETTINGS: usize = 4;

/// State of the main thread only (the hook thread has its own atomics in `keys`).
struct Agent {
    hwnd: HWND,
    /// Top-level Explorer window in front (null = something else is).
    fg: HWND,
    desktop: bool,
    sel_hook: Option<HWINEVENTHOOK>,
    sel_pid: u32,
    paused: bool,
    /// What Explorer said is selected, the last time we asked.
    current: Option<PathBuf>,
    /// What the preview shows (or is about to show).
    shown: Option<PathBuf>,
    /// A Space was taken and its answer is awaited: Some(true) = by the hook (give the key back if it is not for a picture).
    space_pending: Option<bool>,
    /// A picture we are waiting for to show.
    want: Option<PathBuf>,
    size: (u32, u32),
    test: bool,
    taskbar_created: u32,
}

thread_local! {
    static AGENT: RefCell<Option<Agent>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Agent) -> R) -> Option<R> {
    AGENT.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn log(msg: &str) {
    if std::env::var_os("NUQUICK_TEST").is_none() {
        return;
    }
    use std::io::Write;
    let path = std::env::temp_dir().join("nuquick-test.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{msg}");
    }
}

pub fn close_request() {
    if let Some(h) = with(|a| a.hwnd) {
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE_REQ, WPARAM(0), LPARAM(0));
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn class_is(h: HWND, names: &[&str]) -> bool {
    let mut b = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut b) } as usize;
    let s = String::from_utf16_lossy(&b[..n]);
    names.iter().any(|x| *x == s)
}

// ---------------------------------------------------------------------------------------------------------------------
// WinEvent hooks: which window is in front, and "the selection changed"

unsafe extern "system" fn sel_hook(_h: HWINEVENTHOOK, _event: u32, _hwnd: HWND, _obj: i32, _child: i32, _tid: u32, _time: u32) {
    // Many events arrive while an arrow key is held; the handler looks the item up itself (and the lookups are coalesced).
    let h = keys::AGENT.load(std::sync::atomic::Ordering::Relaxed);
    if h != 0 {
        let _ = PostMessageW(Some(HWND(h as *mut _)), WM_SEL, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn fg_hook(_h: HWINEVENTHOOK, _event: u32, hwnd: HWND, _obj: i32, _child: i32, _tid: u32, _time: u32) {
    foreground_changed(hwnd);
}

fn foreground_changed(hwnd: HWND) {
    log(&format!("foreground {:?}", hwnd.0));
    let explorer = class_is(hwnd, &["CabinetWClass", "ExploreWClass"]);
    let desktop = !explorer && class_is(hwnd, &["Progman", "WorkerW"]);
    if (explorer || desktop) && !with(|a| a.paused).unwrap_or(true) {
        attach(hwnd, desktop);
    } else {
        detach();
    }
}

fn attach(hwnd: HWND, desktop: bool) {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    with(|a| {
        a.fg = hwnd;
        a.desktop = desktop;
        a.size = view::max_size(hwnd);
        if a.sel_pid != pid {
            unsafe {
                if let Some(h) = a.sel_hook.take() {
                    let _ = UnhookWinEvent(h);
                }
                // Only that one process (Explorer) reports to us.
                let h = SetWinEventHook(EVENT_OBJECT_FOCUS, EVENT_OBJECT_SELECTIONWITHIN, None, Some(sel_hook), pid, 0, WINEVENT_OUTOFCONTEXT);
                a.sel_hook = if h.0.is_null() { None } else { Some(h) };
            }
            a.sel_pid = pid;
        }
    });
    keys::FG.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
    keys::enable(true);
    request_lookup();
}

fn detach() {
    keys::FG.store(0, std::sync::atomic::Ordering::Relaxed);
    keys::enable(false);
    with(|a| {
        a.fg = HWND::default();
        a.current = None;
        a.space_pending = None;
        if let Some(h) = a.sel_hook.take() {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
        a.sel_pid = 0;
        // Explorer lost the front: the pre-decoded pictures and the shell objects can go in half a minute.
        unsafe { SetTimer(Some(a.hwnd), T_TRIM, 30_000, None) };
    });
    hide_preview();
}

// ---------------------------------------------------------------------------------------------------------------------
// behaviour (all on the main thread)

/// Ask the lookup thread which item is selected (the answer comes back as `WM_LOOKED`).
fn request_lookup() {
    let Some((fg, desktop, any)) = with(|a| (a.fg.0 as isize, a.desktop, a.test && a.fg.0.is_null())) else { return };
    if fg == 0 && !any {
        return;
    }
    lookup::request(lookup::Req { fg, desktop, any });
}

fn anchor_window() -> HWND {
    let fg = with(|a| a.fg).unwrap_or_default();
    if fg.0.is_null() { unsafe { GetDesktopWindow() } } else { fg }
}

fn hide_preview() {
    let was = view::is_visible();
    view::hide();
    with(|a| {
        a.want = None;
        a.shown = None;
        unsafe {
            let _ = KillTimer(Some(a.hwnd), T_POLL);
            if was {
                SetTimer(Some(a.hwnd), T_TRIM, 60_000, None);
            }
        }
    });
}

fn show(p: std::sync::Arc<view::Preview>, path: PathBuf) {
    view::show(p.clone(), anchor_window(), false);
    let t = keys::T_SPACE.lock().unwrap().take();
    if let Some(t) = t {
        log(&format!("space_to_shown_ms={:.1} decode_ms={:.1} size={}x{}", t.elapsed().as_secs_f64() * 1000.0, p.decode_ms, p.w, p.h));
    }
    log(&format!("shown {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    with(|a| {
        a.shown = Some(path);
        a.want = None;
        unsafe {
            // While the preview is up, ask now and then which item is selected: it does not depend on Explorer's events arriving.
            SetTimer(Some(a.hwnd), T_POLL, 150, None);
            let _ = KillTimer(Some(a.hwnd), T_TRIM);
        }
    });
}

/// Show `path` (a picture, or an info card for anything else): now if it is ready, else when the decode thread has it.
fn present(path: PathBuf) {
    let Some((mw, mh)) = with(|a| a.size) else { return };
    if let Some(p) = view::cached(&path, mw, mh) {
        show(p, path);
    } else {
        with(|a| a.want = Some(path.clone()));
        view::request(&path, mw, mh, false);
    }
}

fn on_space(from_hook: bool) {
    log(&format!("space from_hook={from_hook}"));
    // Show what we last knew to be selected at once (kept fresh by the selection events) and check it with a new lookup:
    // asking Explorer takes from 1.5 ms to tens of ms, and the preview should not wait for it.
    let known = with(|a| a.current.clone()).flatten();
    with(|a| a.space_pending = Some(from_hook));
    if let Some(p) = known {
        present(p);
    }
    request_lookup();
}

/// The lookup thread answered.
fn on_lookup() {
    let Some(result) = lookup::take() else { return };
    let (pending, hwnd) = match with(|a| {
        a.current = result.clone();
        (a.space_pending.take(), a.hwnd)
    }) {
        Some(x) => x,
        None => return,
    };
    match (pending, result) {
        (Some(_), Some(p)) => {
            // Already showing (or about to show) exactly this: nothing to do.
            if with(|a| a.shown.as_ref() != Some(&p) && a.want.as_ref() != Some(&p)).unwrap_or(true) {
                present(p);
            }
        }
        (Some(from_hook), _) => {
            // Nothing is selected (or the item has no file path): Explorer gets its Space back. A test message never types into
            // what has the focus. (If the last known selection made us show something, take it down again.)
            hide_preview();
            if from_hook {
                keys::give_back_space();
            }
        }
        (None, Some(p)) => {
            if view::is_visible() {
                // The selection moved while the preview is up: show the new one (the preview stays until Space / Esc).
                if with(|a| a.shown.as_ref() != Some(&p)).unwrap_or(false) {
                    present(p);
                }
            } else if view::previewable(&p) {
                // Decode ahead, after a short pause (while an arrow key is held only the picture it rests on is wanted).
                unsafe { SetTimer(Some(hwnd), T_PREFETCH, 120, None) };
            }
        }
        (None, None) => {}
    }
}

fn on_timer(id: usize) {
    let Some(hwnd) = with(|a| a.hwnd) else { return };
    if id == T_POLL {
        if view::is_visible() {
            request_lookup();
        }
        return;
    }
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
    match id {
        T_PREFETCH => {
            if let Some((Some(p), (mw, mh))) = with(|a| (a.current.clone(), a.size)) {
                if view::previewable(&p) {
                    view::request(&p, mw, mh, true);
                    // Nothing is shown yet: give the memory back if no Space follows.
                    unsafe { SetTimer(Some(hwnd), T_TRIM, 60_000, None) };
                }
            }
        }
        T_TRIM => {
            view::clear_cache();
            unsafe {
                let _ = EmptyWorkingSet(GetCurrentProcess());
            }
        }
        _ => {}
    }
}

fn on_decoded() {
    let Some((want, (mw, mh), current)) = with(|a| (a.want.clone(), a.size, a.current.clone())) else { return };
    if let Some(path) = want {
        if let Some(p) = view::cached(&path, mw, mh) {
            show(p, path);
        }
    } else if let (Some(path), false) = (current, view::is_visible()) {
        // A prefetch finished: put the (hidden) window in place so Space only has to show it.
        if let Some(p) = view::cached(&path, mw, mh) {
            view::prepare(&p, anchor_window());
        }
    }
}

fn open_in_viewer() {
    let Some(path) = with(|a| a.shown.clone().or(a.current.clone())).flatten() else { return };
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("nuviewer.exe")));
    // A picture opens in NuViewer; a folder or any other file opens the way Windows opens it.
    if let Some(exe) = exe.filter(|e| e.exists() && view::previewable(&path)) {
        let _ = std::process::Command::new(exe).arg(path).spawn();
    } else {
        let _ = open_default(&path);
    }
    hide_preview();
}

fn open_default(path: &std::path::Path) -> bool {
    let p = wide(&path.to_string_lossy());
    unsafe { ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL).0 as isize > 32 }
}

// ---------------------------------------------------------------------------------------------------------------------
// tray + start-up

fn startup_enabled() -> bool {
    nv_win::quick::startup_enabled()
}

fn set_startup(on: bool) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = nv_win::quick::set_startup(on, &exe);
    }
}

/// Open NuViewer's Settings window (the "Space preview" tab is where NuQuick is switched on and off).
fn open_settings() {
    let viewer = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("nuviewer.exe"))).filter(|p| p.exists());
    if let Some(v) = viewer {
        let _ = std::process::Command::new(v).arg("--settings").spawn();
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP, uCallbackMessage: WM_TRAY, ..Default::default() };
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
    for (i, c) in "NuQuick: Space previews the picture selected in Explorer".encode_utf16().take(127).enumerate() {
        nid.szTip[i] = c;
    }
    nid
}

fn tray_add(hwnd: HWND, balloon: bool) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
        if balloon {
            // Started by hand: say where it went (the icon often sits behind the little arrow near the clock).
            let mut nid = tray_data(hwnd);
            nid.uFlags = NIF_INFO;
            nid.dwInfoFlags = NIIF_INFO;
            for (dst, text) in [(&mut nid.szInfoTitle[..], "NuQuick is on"), (&mut nid.szInfo[..], "Select a picture in Explorer and press Space. Right-click this icon for settings.")] {
                for (i, c) in text.encode_utf16().take(dst.len() - 1).enumerate() {
                    dst[i] = c;
                }
            }
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let paused = with(|a| a.paused).unwrap_or(false);
        let _ = AppendMenuW(menu, if paused { MF_CHECKED | MF_STRING } else { MF_STRING }, ID_PAUSE, w!("Pause"));
        let _ = AppendMenuW(menu, if startup_enabled() { MF_CHECKED | MF_STRING } else { MF_STRING }, ID_STARTUP, w!("Start with Windows"));
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings..."));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit NuQuick"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd); // so the menu closes when the user clicks elsewhere
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            ID_PAUSE => {
                let now_paused = !paused;
                with(|a| a.paused = now_paused);
                if now_paused {
                    detach();
                } else {
                    foreground_changed(GetForegroundWindow());
                }
            }
            ID_STARTUP => set_startup(!startup_enabled()),
            ID_SETTINGS => open_settings(),
            ID_QUIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn agent_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SPACE | WM_TEST_SPACE => {
            if msg == WM_TEST_SPACE {
                *keys::T_SPACE.lock().unwrap() = Some(Instant::now());
            }
            on_space(msg == WM_SPACE);
            LRESULT(0)
        }
        WM_CLOSE_REQ => {
            hide_preview();
            LRESULT(0)
        }
        WM_OPEN_REQ => {
            open_in_viewer();
            LRESULT(0)
        }
        WM_SEL => {
            request_lookup();
            LRESULT(0)
        }
        WM_LOOKED => {
            on_lookup();
            LRESULT(0)
        }
        view::WM_DECODED => {
            on_decoded();
            LRESULT(0)
        }
        WM_TIMER => {
            on_timer(wp.0);
            LRESULT(0)
        }
        WM_TRAY => {
            if (lp.0 as u32 & 0xFFFF) == WM_RBUTTONUP || (lp.0 as u32 & 0xFFFF) == WM_LBUTTONUP {
                tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_TEST_QUIT => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            log("destroy");
            detach();
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if with(|a| a.taskbar_created == m).unwrap_or(false) => {
            tray_add(hwnd, false); // Explorer restarted
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// entry

fn run_agent() -> i32 {
    unsafe {
        // One agent only: a second start finds the first one's window. (A named mutex was tried first; a killed process can stay
        // listed with its handles for a long time, and the mutex then refused every later start. A window is the truth.)
        // Tests run their own agent next to the user's: no check then.
        if std::env::var_os("NUQUICK_TEST").is_none() && nv_win::quick::is_running() {
            return 0;
        }
        nv_codecs::set_raw_mode(nv_codecs::RawMode::Preview);
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(agent_proc), hInstance: hinst.into(), lpszClassName: AGENT_CLASS, ..Default::default() };
        RegisterClassExW(&wc);
        // A hidden top-level window (not message-only: it has to receive the broadcast "TaskbarCreated").
        let Ok(hwnd) = CreateWindowExW(WS_EX_TOOLWINDOW, AGENT_CLASS, w!("NuQuick"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst.into()), None) else { return 1 };
        let test = std::env::var_os("NUQUICK_TEST").is_some();
        AGENT.with(|a| {
            *a.borrow_mut() = Some(Agent {
                hwnd,
                fg: HWND::default(),
                desktop: false,
                sel_hook: None,
                sel_pid: 0,
                paused: false,
                current: None,
                shown: None,
                space_pending: None,
                want: None,
                size: (1600, 900),
                test,
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            })
        });
        keys::AGENT.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
        keys::start_thread();
        // Tests look only inside their own folder (NUQUICK_TEST_DIR), never at what the user has selected.
        lookup::start(hwnd.0 as isize, std::env::var_os("NUQUICK_TEST_DIR").map(PathBuf::from));
        view::start_worker(hwnd);
        tray_add(hwnd, !std::env::args().any(|a| a == "--silent"));
        // Always-on part: one cheap event, only when the foreground window changes.
        let fg = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(fg_hook), 0, 0, WINEVENT_OUTOFCONTEXT);
        foreground_changed(GetForegroundWindow());
        // Give the start-up memory back (the first lookup warms the shell objects on its own thread).
        let _ = EmptyWorkingSet(GetCurrentProcess());
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !fg.0.is_null() {
            let _ = UnhookWinEvent(fg);
        }
    }
    0
}

/// `nuquick.exe --preview <file>`: show one picture in the same window, taking the focus (Esc / Space / click closes).
/// Used for tests and as a fallback; prints how long the first pixels took when `--timing` is given.
fn run_preview(path: &str, timing: bool) -> i32 {
    let t0 = Instant::now();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED); // the info card asks the shell
    }
    nv_codecs::set_raw_mode(nv_codecs::RawMode::Preview);
    let desktop = unsafe { GetDesktopWindow() };
    let (mw, mh) = view::max_size(desktop);
    match view::decode_now(std::path::Path::new(path), mw, mh) {
        Ok(p) => {
            view::show(p.clone(), desktop, true);
            if timing {
                let _ = std::fs::write(std::env::temp_dir().join("nuquick-preview.txt"), format!("shown_ms={:.1} decode_ms={:.1} size={}x{}", t0.elapsed().as_secs_f64() * 1000.0, p.decode_ms, p.w, p.h));
            }
            unsafe {
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            0
        }
        Err(e) => {
            log(&format!("preview failed: {e}"));
            1
        }
    }
}

/// `--quit`: ask a running agent to exit.
fn quit_running() -> i32 {
    unsafe {
        match FindWindowW(AGENT_CLASS, PCWSTR::null()) {
            Ok(h) => {
                let _ = PostMessageW(Some(h), WM_TEST_QUIT, WPARAM(0), LPARAM(0));
                0
            }
            Err(_) => 1,
        }
    }
}

fn main() {
    nv_core::posters::register(); // a video's frame / a sound's waveform; Media Foundation loads only when one is previewed
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("--preview") => run_preview(args.get(1).map(|s| s.as_str()).unwrap_or(""), args.iter().any(|a| a == "--timing")),
        Some("--quit") => quit_running(),
        Some("--startup-on") => {
            set_startup(true);
            0
        }
        Some("--startup-off") => {
            set_startup(false);
            0
        }
        _ => run_agent(),
    };
    std::process::exit(code);
}
