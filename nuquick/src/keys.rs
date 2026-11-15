//! The keyboard hook, on a thread of its own, deciding with atomics only.
//!
//! Why a thread of its own: a low-level hook is called by Windows on the thread that installed it, and Windows gives it ~300 ms
//! per key; a hook that is late is skipped and, after a few, removed. The agent's main thread has timers, tray and drawing to do,
//! so the hook lives on a thread that does nothing else. It never touches COM or the agent's state: it reads a few atomics and
//! posts messages to the agent window.

use crate::{WM_CLOSE_REQ, WM_OPEN_REQ, WM_SPACE};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Instant;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Marks the Space we send ourselves (a key the hook took that turned out not to be for a picture): our own hook lets it through.
pub const MAGIC: usize = 0x4E51_5543;
/// The Explorer / desktop window the agent serves (0 = none: the hook is idle).
pub static FG: AtomicIsize = AtomicIsize::new(0);
/// The agent window (messages go there).
pub static AGENT: AtomicIsize = AtomicIsize::new(0);
/// When the last Space reached us (tests and timing).
pub static T_SPACE: Mutex<Option<Instant>> = Mutex::new(None);

static SWALLOWED: AtomicBool = AtomicBool::new(false);
static LAST_CHAR: AtomicU64 = AtomicU64::new(0);
static HOOK_TID: AtomicU32 = AtomicU32::new(0);
const WM_HOOK_CTL: u32 = WM_APP + 20;

/// Milliseconds on a monotonic clock.
pub fn tick() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1_000_000
}

fn class_is(h: HWND, names: &[&str]) -> bool {
    let mut b = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut b) } as usize;
    let s = String::from_utf16_lossy(&b[..n]);
    names.iter().any(|x| *x == s)
}

/// The focus is on the file list (not a rename / search / address box): Space there must stay a typed space.
fn list_has_focus(fg: HWND) -> bool {
    unsafe {
        let tid = GetWindowThreadProcessId(fg, None);
        let mut gi = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if GetGUIThreadInfo(tid, &mut gi).is_err() || gi.hwndFocus.0.is_null() {
            return false;
        }
        class_is(gi.hwndFocus, &["DirectUIHWND", "SysListView32", "SHELLDLL_DefView"])
    }
}

fn post(msg: u32) {
    let h = AGENT.load(Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), msg, WPARAM(0), LPARAM(0));
        }
    }
}

/// True = swallow this key event.
fn decide(msg: u32, k: &KBDLLHOOKSTRUCT) -> bool {
    if k.dwExtraInfo == MAGIC {
        return false;
    }
    let fg = FG.load(Relaxed);
    // Only while the window the agent serves is really in front (the foreground event can lag a moment).
    if fg == 0 || unsafe { GetForegroundWindow() }.0 as isize != fg {
        return false;
    }
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    let visible = crate::view::is_visible();
    match k.vkCode {
        0x20 => {
            let swallowed = SWALLOWED.load(Relaxed);
            if up && swallowed {
                SWALLOWED.store(false, Relaxed);
                return true;
            }
            if !down {
                return false;
            }
            if swallowed {
                return true; // key repeat of a press we took
            }
            let held = |v: VIRTUAL_KEY| unsafe { GetAsyncKeyState(v.0 as i32) } < 0;
            if held(VK_CONTROL) || held(VK_MENU) || held(VK_SHIFT) || held(VK_LWIN) || held(VK_RWIN) {
                return false;
            }
            if visible {
                SWALLOWED.store(true, Relaxed);
                post(WM_CLOSE_REQ);
                return true;
            }
            // Type-ahead: a space inside a word being typed in the file list stays a space.
            let typing = tick().saturating_sub(LAST_CHAR.load(Relaxed)) < 700;
            if !typing && list_has_focus(HWND(fg as *mut _)) {
                // Taken whatever is selected: the agent looks it up and hands the key back (re-sends it) if it is not for a picture.
                SWALLOWED.store(true, Relaxed);
                *T_SPACE.lock().unwrap() = Some(Instant::now());
                post(WM_SPACE);
                return true;
            }
            false
        }
        0x1B if visible && down => {
            post(WM_CLOSE_REQ);
            true
        }
        0x0D if visible && down => {
            post(WM_OPEN_REQ);
            true
        }
        0x30..=0x5A if down => {
            LAST_CHAR.store(tick(), Relaxed);
            false
        }
        _ => false,
    }
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && decide(wp.0 as u32, &*(lp.0 as *const KBDLLHOOKSTRUCT)) {
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wp, lp)
}

/// The key we took was not for a picture: type it after all (from yet another thread, `SendInput` waits for the hooks).
pub fn give_back_space() {
    SWALLOWED.store(false, Relaxed);
    std::thread::spawn(|| {
        let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_SPACE, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: MAGIC } },
        };
        let inputs = [mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)];
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    });
}

/// Start the hook thread (it installs nothing until `enable(true)`).
pub fn start_thread() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("nuquick-keys".into())
        .spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE); // create the queue
            HOOK_TID.store(GetCurrentThreadId(), Relaxed);
            let _ = tx.send(());
            let mut hook: Option<HHOOK> = None;
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if msg.message == WM_HOOK_CTL {
                    if msg.wParam.0 != 0 {
                        if hook.is_none() {
                            hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), GetModuleHandleW(None).ok().map(|m| m.into()), 0).ok();
                        }
                    } else if let Some(h) = hook.take() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                    SWALLOWED.store(false, Relaxed);
                }
            }
        })
        .expect("keys thread");
    let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
}

/// Install / remove the hook (on the hook thread).
pub fn enable(on: bool) {
    let tid = HOOK_TID.load(Relaxed);
    if tid != 0 {
        unsafe {
            let _ = PostThreadMessageW(tid, WM_HOOK_CTL, WPARAM(on as usize), LPARAM(0));
        }
    }
}
