//! Thin Win32 layer: DPI-aware window + message loop delivering `Event`s.

pub mod assoc;
pub mod crash;
pub mod diskstat;
pub mod instance;
pub mod menu;
pub mod registry;
pub mod shell;
pub mod text;
pub mod ui;

use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetDC, InvalidateRect, ReleaseDC, SetDIBitsToDevice, UpdateWindow, ValidateRect, BITMAPINFO,
    BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

pub use windows::Win32::Foundation::HWND as WindowHandle;

#[derive(Debug, Clone)]
pub enum Event {
    Resize { width: u32, height: u32 },
    Paint,
    /// Windows virtual-key code.
    Key(u32),
    Wheel { delta: i32, x: i32, y: i32, ctrl: bool },
    MouseDown { x: i32, y: i32, ctrl: bool },
    MouseMove { x: i32, y: i32, left_down: bool },
    MouseUp,
    DoubleClick { x: i32, y: i32, ctrl: bool },
    /// Right mouse button released over the client area (x, y in client pixels): the app shows its context menu.
    RightClick { x: i32, y: i32 },
    /// Middle mouse button released (sent on release so the matching "up" can't land on a window behind ours).
    MiddleClick,
    Close,
    /// Posted by a `waker()` from any thread (e.g. a background decode finished).
    User,
    /// A timer armed with `set_timer(hwnd, id, ..)` fired.
    Timer(u32),
    /// The window moved to a monitor with a different DPI; the value is the new UI scale (1.0 = 96 DPI).
    /// The window has already been resized to the size Windows suggested.
    Dpi(f32),
    /// Another launch of NuViewer asked this window to open something (see `instance`).
    Open { path: Option<String>, gallery: bool },
    /// A typed character (control characters are filtered out; use `Key` for those).
    Char(char),
}

/// Modifier keys at the time of the event being handled.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

thread_local! {
    /// Set while handling an injected key (see `WM_NV_INJECT_KEY`), so tests can send Ctrl+Z etc.
    static FORCED_MODS: std::cell::Cell<Option<Mods>> = const { std::cell::Cell::new(None) };
}

/// Posted from outside (tests/automation): wParam = virtual key | modifier bits << 16 (1 ctrl, 2 shift, 4 alt).
/// Real keyboards can't be simulated reliably with `PostMessage` because it doesn't change `GetKeyState`.
pub const WM_NV_INJECT_KEY: u32 = WM_APP + 1;

pub fn modifiers() -> Mods {
    if let Some(m) = FORCED_MODS.with(|f| f.get()) {
        return m;
    }
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    let down = |vk: u16| unsafe { GetKeyState(vk as i32) < 0 };
    Mods { ctrl: down(VK_CONTROL.0), shift: down(VK_SHIFT.0), alt: down(VK_MENU.0) }
}

static HIDE_CURSOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Hide (or restore) the mouse pointer while it's over our client area, e.g. in fullscreen after a pause.
pub fn set_cursor_hidden(hidden: bool) {
    HIDE_CURSOR.store(hidden, std::sync::atomic::Ordering::Relaxed);
    unsafe {
        if hidden {
            SetCursor(None);
        } else if let Ok(arrow) = LoadCursorW(None, IDC_ARROW) {
            SetCursor(Some(arrow));
        }
    }
}

/// Client-area size in pixels.
pub fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rc = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
    }
    ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32)
}

/// Minutes to add to UTC to get local time (includes daylight saving if active).
pub fn utc_offset_minutes() -> i32 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let extra = if id == 2 { tz.DaylightBias } else { tz.StandardBias }; // 2 = TIME_ZONE_ID_DAYLIGHT
    -(tz.Bias + extra)
}

thread_local! {
    static FULLSCREEN_SAVED: std::cell::RefCell<Option<(isize, WINDOWPLACEMENT)>> = const { std::cell::RefCell::new(None) };
}

pub fn is_fullscreen() -> bool {
    FULLSCREEN_SAVED.with(|s| s.borrow().is_some())
}

thread_local! {
    static TASKBAR_LIST: std::cell::RefCell<Option<windows::Win32::UI::Shell::ITaskbarList2>> = const { std::cell::RefCell::new(None) };
}

/// Tell the shell whether `hwnd` counts as a fullscreen window: `true` hides the Windows taskbar while it is in front,
/// `false` shows the taskbar over it. This is the documented way (ITaskbarList2::MarkFullscreenWindow); fighting the
/// taskbar's z-order by hand is unreliable on Windows 11.
pub fn set_taskbar_hidden(hwnd: HWND, hidden: bool) {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList2, TaskbarList};
    TASKBAR_LIST.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            unsafe {
                if let Ok(list) = CoCreateInstance::<_, ITaskbarList2>(&TaskbarList, None, CLSCTX_INPROC_SERVER) {
                    let _ = list.HrInit();
                    *slot = Some(list);
                }
            }
        }
        if let Some(list) = slot.as_ref() {
            unsafe {
                let _ = list.MarkFullscreenWindow(hwnd, hidden);
            }
        }
    });
}

/// Mouse position in screen pixels.
pub fn cursor_pos() -> (i32, i32) {
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

/// Bounds (left, top, right, bottom) of the monitor the window is mostly on, in screen pixels.
pub fn monitor_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r = mi.rcMonitor;
            (r.left, r.top, r.right, r.bottom)
        } else {
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        }
    }
}

/// Resolution of the monitor the window is on.
pub fn monitor_size(hwnd: HWND) -> (u32, u32) {
    let (l, t, r, b) = monitor_rect(hwnd);
    (((r - l).max(1)) as u32, ((b - t).max(1)) as u32)
}

/// Height of the Windows taskbar in pixels (a sensible default if it can't be found).
pub fn taskbar_height() -> i32 {
    unsafe {
        if let Ok(tray) = FindWindowW(w!("Shell_TrayWnd"), None) {
            let mut rc = windows::Win32::Foundation::RECT::default();
            if GetWindowRect(tray, &mut rc).is_ok() && rc.bottom - rc.top > 0 {
                return rc.bottom - rc.top;
            }
        }
    }
    60
}

/// True if our window is the foreground window.
pub fn is_foreground(hwnd: HWND) -> bool {
    unsafe { GetForegroundWindow() == hwnd }
}

/// Run `f` as if the given modifier keys were held (menu items reuse the keyboard shortcuts' code paths).
pub fn call_with_mods<R>(mods: Mods, f: impl FnOnce() -> R) -> R {
    let old = FORCED_MODS.with(|m| m.replace(Some(mods)));
    let r = f();
    FORCED_MODS.with(|m| m.set(old));
    r
}

/// Toggle borderless fullscreen on the window's current monitor. Returns whether it is fullscreen now.
/// The caller must resize its swapchain afterwards: the resulting WM_SIZE arrives while the handler is busy.
pub fn toggle_fullscreen(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    unsafe {
        FULLSCREEN_SAVED.with(|saved| {
            let mut saved = saved.borrow_mut();
            if let Some((style, placement)) = saved.take() {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                let _ = SetWindowPlacement(hwnd, &placement);
                let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
                set_taskbar_hidden(hwnd, false);
                false
            } else {
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                let mut placement = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let _ = GetWindowPlacement(hwnd, &mut placement);
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if !GetMonitorInfoW(mon, &mut mi).as_bool() {
                    return false;
                }
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_OVERLAPPEDWINDOW.0 as isize));
                let r = mi.rcMonitor;
                let _ = SetWindowPos(hwnd, Some(HWND_TOP), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOOWNERZORDER | SWP_FRAMECHANGED);
                *saved = Some((style, placement));
                set_taskbar_hidden(hwnd, true);
                true
            }
        })
    }
}

/// Wake the UI thread (Event::User) given a raw window handle value; safe to call from any thread.
pub fn post_user_raw(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut c_void)), WM_APP, WPARAM(0), LPARAM(0));
    }
}

/// Returns a thread-safe closure that wakes the UI thread with `Event::User`.
pub fn waker(hwnd: HWND) -> impl Fn() + Send + Sync + 'static {
    let h = hwnd.0 as isize;
    move || unsafe {
        let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_APP, WPARAM(0), LPARAM(0));
    }
}

type Handler = Box<dyn FnMut(Event)>;

struct State {
    handler: Handler,
    busy: bool,
    /// The handler panicked; ignore everything from here on while the window closes.
    dead: bool,
}

pub fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn set_title(hwnd: HWND, title: &str) {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
    }
}

/// Paint a top-down 32-bit BGRA image centred in the client area with plain GDI (no GPU needed), filling
/// the rest with `bg` (0xRRGGBB). Used for the very first frame while the GPU device is still starting up.
pub fn paint_bgra(hwnd: HWND, x: i32, y: i32, img_w: i32, img_h: i32, bgra: &[u8], bg: u32) {
    if bgra.len() < img_w as usize * img_h as usize * 4 || img_w <= 0 || img_h <= 0 {
        return;
    }
    unsafe {
        let hdc = GetDC(Some(hwnd));
        if hdc.is_invalid() {
            return;
        }
        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (cw, ch) = (rc.right, rc.bottom);
        // 0xRRGGBB -> COLORREF (0x00BBGGRR)
        let colorref = ((bg & 0xFF) << 16) | (bg & 0xFF00) | ((bg >> 16) & 0xFF);
        let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(colorref));
        let mk = |l: i32, t: i32, r: i32, b: i32| windows::Win32::Foundation::RECT { left: l, top: t, right: r, bottom: b };
        // Margins only, so the image area isn't cleared and re-filled (no flicker).
        for r in [mk(0, 0, cw, y.max(0)), mk(0, (y + img_h).max(0), cw, ch), mk(0, y, x.max(0), y + img_h), mk((x + img_w).max(0), y, cw, y + img_h)] {
            if r.right > r.left && r.bottom > r.top {
                FillRect(hdc, &r, brush);
            }
        }
        let _ = DeleteObject(brush.into());
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: img_w,
                biHeight: -img_h, // negative = top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                ..Default::default()
            },
            ..Default::default()
        };
        SetDIBitsToDevice(hdc, x, y, img_w as u32, img_h as u32, 0, 0, 0, img_h as u32, bgra.as_ptr() as *const c_void, &bmi, DIB_RGB_COLORS);
        ReleaseDC(Some(hwnd), hdc);
    }
}

/// UI scale of the monitor the window is on (1.0 = 96 DPI).
pub fn dpi_scale(hwnd: HWND) -> f32 {
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 }
}

/// (Re)arm timer `id`; arming an id that's already running restarts it.
pub fn set_timer(hwnd: HWND, id: u32, ms: u32) {
    unsafe {
        SetTimer(Some(hwnd), id as usize, ms.max(10), None);
    }
}

pub fn kill_timer(hwnd: HWND, id: u32) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id as usize);
    }
}

/// Initialise COM (single-threaded apartment) on this thread; the shell dialogs need it. True on success.
pub fn com_init() -> bool {
    unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED).is_ok() }
}

pub fn quit() {
    unsafe { PostQuitMessage(0) }
}

static MAIN_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
/// The form (Settings, the file-type chooser) that is open right now, innermost last (0 = none).
pub(crate) static TOP_FORM: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// The window a new dialog should belong to: the form that is open (so a message box raised from Settings is modal to
/// Settings), else the viewer's main window, else none.
pub fn dialog_parent() -> Option<HWND> {
    let h = TOP_FORM.load(std::sync::atomic::Ordering::Relaxed);
    let form = HWND(h as *mut c_void);
    if h != 0 && unsafe { IsWindow(Some(form)).as_bool() } {
        return Some(form);
    }
    main_window()
}

/// The viewer's main window, once it exists. Dialogs and forms use it as their owner, so Windows keeps them above it,
/// Alt+Tab shows one entry for the whole program and switching back lands on the dialog instead of on a window that
/// is waiting for it and looks frozen.
pub fn main_window() -> Option<HWND> {
    let h = MAIN_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    let hwnd = HWND(h as *mut c_void);
    (h != 0 && unsafe { IsWindow(Some(hwnd)).as_bool() }).then_some(hwnd)
}

static CLOSE_VETO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Call while handling `Event::Close` to keep the window open (e.g. the user answered Cancel to "save changes?").
pub fn veto_close() {
    CLOSE_VETO.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Creates the window, calls `init(hwnd, client_w, client_h)` to build the event handler,
/// then runs the message loop until the window closes. With `fullscreen` the window is made borderless-fullscreen
/// before it is first shown, so there is no windowed frame at all.
pub fn run<F>(title: &str, width: i32, height: i32, fullscreen: bool, init: F) -> Result<(), String>
where
    F: FnOnce(HWND, u32, u32) -> Result<Handler, String>,
{
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("NuViewerWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            lpszClassName: class,
            // Shown until the first real frame: matches the renderer's clear colour.
            hbrBackground: CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x001A_1A1A)),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW failed".into());
        }
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR(wide.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            None,
            None,
            Some(hinst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        MAIN_WINDOW.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);

        if fullscreen {
            toggle_fullscreen(hwnd);
        }
        // Show the (dark) window first so slow GPU init doesn't look like a hang.
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        if fullscreen {
            set_taskbar_hidden(hwnd, true); // once more now that the window is visible: the shell ignores it before that
        }

        let mut rc = windows::Win32::Foundation::RECT::default();
        GetClientRect(hwnd, &mut rc).map_err(|e| e.to_string())?;
        let handler = init(hwnd, (rc.right - rc.left) as u32, (rc.bottom - rc.top) as u32)?;
        let state = Box::into_raw(Box::new(State { handler, busy: false, dead: false }));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        invalidate(hwnd);
        // Background work that finished while `init` ran posted its wake-up before we could receive it.
        let _ = PostMessageW(Some(hwnd), WM_APP, WPARAM(0), LPARAM(0));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(state));
    }
    Ok(())
}

fn lo_i16(l: isize) -> i32 {
    (l & 0xFFFF) as u16 as i16 as i32
}
fn hi_i16(l: isize) -> i32 {
    ((l >> 16) & 0xFFFF) as u16 as i16 as i32
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    // Returns whether the handler actually ran (false while it is busy, e.g. inside a modal dialog, or before it exists).
    let dispatch = |ev: Event| -> bool {
        if let Some(st) = state_ptr.as_mut() {
            if !st.busy && !st.dead {
                st.busy = true;
                // A panic must not unwind through this extern "system" frame: catch it, report it, close cleanly.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (st.handler)(ev)));
                st.busy = false;
                if outcome.is_err() {
                    st.dead = true;
                    crash::fatal(hwnd);
                }
                return true;
            }
        }
        false
    };
    match msg {
        WM_SIZE => {
            let (w, h) = ((lp.0 & 0xFFFF) as u32, ((lp.0 >> 16) & 0xFFFF) as u32);
            if w > 0 && h > 0 {
                dispatch(Event::Resize { width: w, height: h });
            }
            LRESULT(0)
        }
        WM_ERASEBKGND if state_ptr.is_null() => DefWindowProcW(hwnd, msg, wp, lp),
        WM_PAINT if state_ptr.is_null() => DefWindowProcW(hwnd, msg, wp, lp),
        WM_ERASEBKGND => LRESULT(1),
        WM_APP => {
            dispatch(Event::User);
            LRESULT(0)
        }
        WM_SETCURSOR if HIDE_CURSOR.load(std::sync::atomic::Ordering::Relaxed) && (lp.0 & 0xFFFF) == 1 /* HTCLIENT */ => {
            SetCursor(None);
            LRESULT(1)
        }
        WM_DPICHANGED => {
            // wParam: new DPI (x in the low word). lParam: the rectangle Windows suggests so the window keeps its physical size.
            let r = *(lp.0 as *const windows::Win32::Foundation::RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            dispatch(Event::Dpi((wp.0 & 0xFFFF) as f32 / 96.0));
            LRESULT(0)
        }
        WM_TIMER => {
            dispatch(Event::Timer(wp.0 as u32));
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32).filter(|c| !c.is_control()) {
                dispatch(Event::Char(c));
            }
            LRESULT(0)
        }
        WM_COPYDATA => {
            // Answer 1 only if the request was really handled, so the sender retries while we're still starting up.
            match instance::request_from_message(lp) {
                Some(req) => LRESULT(dispatch(Event::Open { path: req.path, gallery: req.gallery }) as isize),
                None => LRESULT(0),
            }
        }
        WM_NV_INJECT_KEY => {
            let (vk, bits) = ((wp.0 & 0xFFFF) as u32, (wp.0 >> 16) as u32);
            FORCED_MODS.with(|f| f.set(Some(Mods { ctrl: bits & 1 != 0, shift: bits & 2 != 0, alt: bits & 4 != 0 })));
            dispatch(Event::Key(vk));
            FORCED_MODS.with(|f| f.set(None));
            LRESULT(0)
        }
        WM_PAINT => {
            dispatch(Event::Paint);
            let _ = ValidateRect(Some(hwnd), None);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            dispatch(Event::Key(wp.0 as u32));
            LRESULT(0)
        }
        // Keys pressed with Alt held arrive as WM_SYSKEYDOWN. Alt+Enter is the usual "toggle fullscreen"; everything
        // else (Alt+F4, F10, the system menu) must keep its normal behaviour, so it goes to DefWindowProc.
        WM_SYSKEYDOWN if wp.0 as u32 == 0x0D => {
            // WM_SYSKEYDOWN means Alt is down by definition; say so explicitly rather than trusting GetKeyState alone.
            let mut m = modifiers();
            m.alt = true;
            call_with_mods(m, || dispatch(Event::Key(0x0D)));
            LRESULT(0)
        }
        WM_SYSCHAR if wp.0 as u32 == 0x0D => LRESULT(0), // swallow the character so Windows doesn't beep
        WM_MOUSEWHEEL => {
            // Wheel coords are in screen space.
            let mut pt = windows::Win32::Foundation::POINT { x: lo_i16(lp.0), y: hi_i16(lp.0) };
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
            let ctrl = (wp.0 & 0x0008) != 0; // MK_CONTROL
            dispatch(Event::Wheel { delta: hi_i16(wp.0 as isize), x: pt.x, y: pt.y, ctrl });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            SetCapture(hwnd);
            dispatch(Event::MouseDown { x: lo_i16(lp.0), y: hi_i16(lp.0), ctrl: (wp.0 & 0x0008) != 0 });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            dispatch(Event::MouseUp);
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            dispatch(Event::MiddleClick);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            dispatch(Event::RightClick { x: lo_i16(lp.0), y: hi_i16(lp.0) });
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            // The second press of a quick double click arrives as this instead of WM_LBUTTONDOWN: capture the mouse
            // like a normal press so a held-button gesture (zoom, drag) works on it too, and its release is seen.
            SetCapture(hwnd);
            dispatch(Event::DoubleClick { x: lo_i16(lp.0), y: hi_i16(lp.0), ctrl: (wp.0 & 0x0008) != 0 });
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            // Capture was taken away mid-gesture (Alt+Tab, a dialog): treat it as releasing the button. Harmless when
            // it follows our own ReleaseCapture, because MouseUp is idempotent.
            dispatch(Event::MouseUp);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let left_down = (wp.0 & 0x0001) != 0; // MK_LBUTTON
            dispatch(Event::MouseMove { x: lo_i16(lp.0), y: hi_i16(lp.0), left_down });
            LRESULT(0)
        }
        WM_CLOSE => {
            CLOSE_VETO.store(false, std::sync::atomic::Ordering::Relaxed);
            dispatch(Event::Close);
            if !CLOSE_VETO.swap(false, std::sync::atomic::Ordering::Relaxed) {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[allow(dead_code)]
fn _assert_send_ptr(_: *mut c_void) {}

/// The ICC profile Windows has assigned to the display the window is on (the primary display without a window):
/// the file from Settings > Display > Colour profiles / Colour Management. None if there is none or Windows won't say.
pub fn display_profile_path(hwnd: Option<HWND>) -> Option<std::path::PathBuf> {
    use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, GetMonitorInfoW, MonitorFromWindow, MONITORINFOEXW, MONITOR_DEFAULTTOPRIMARY};
    // GetICMProfileW lives in gdi32 (the `windows` crate files it under another feature we don't otherwise need).
    #[link(name = "gdi32")]
    extern "system" {
        fn GetICMProfileW(hdc: isize, size: *mut u32, name: *mut u16) -> i32;
    }
    unsafe {
        let mon = MonitorFromWindow(hwnd.unwrap_or_default(), MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut _).as_bool() {
            return None;
        }
        // A device context for this one monitor, so the answer is for it and not for the whole desktop.
        let dc = CreateDCW(w!("DISPLAY"), PCWSTR(mi.szDevice.as_ptr()), PCWSTR::null(), None);
        if dc.is_invalid() {
            return None;
        }
        let mut buf = vec![0u16; 520];
        let mut size = buf.len() as u32;
        let ok = GetICMProfileW(dc.0 as isize, &mut size, buf.as_mut_ptr());
        let _ = DeleteDC(dc);
        if ok == 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

#[cfg(test)]
mod display_profile_tests {
    #[test]
    fn display_profile_is_a_real_file_when_windows_names_one() {
        // Windows names some profile for every display (at worst its sRGB one); a headless machine may name none.
        if let Some(p) = super::display_profile_path(None) {
            assert!(p.is_file(), "{} should exist", p.display());
            println!("display profile: {}", p.display());
        }
    }
}
