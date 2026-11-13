//! A minimal modal form made of plain Win32 controls (labels, check boxes, edit boxes, buttons). Used by the
//! file-type chooser (`assoc`) and the installer, so neither needs a resource file or a dialog template.
//! All coordinates are in device-independent pixels (96 DPI) and are scaled to the system DPI here.

use std::ffi::c_void;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontW, DeleteObject, FONT_CHARSET, FONT_CLIP_PRECISION, FONT_OUTPUT_PRECISION, FONT_QUALITY, HFONT, HGDIOBJ};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const ID_OK: u16 = 1;
pub const ID_CANCEL: u16 = 2;

/// Called for every button click (and for Esc / the close box, which arrive as `ID_CANCEL`).
/// Return true to close the form; read the controls *inside* the callback, they are gone afterwards.
pub type Handler<'a> = dyn FnMut(&Form, u16) -> bool + 'a;

pub struct Form {
    pub hwnd: HWND,
    font: HFONT,
    dpi: u32,
    /// Page that controls created from now on belong to (0 = always visible), see `show_page`.
    page: std::cell::Cell<u8>,
    tracked: std::cell::RefCell<Vec<(u8, HWND)>>,
}

/// A list box or combo box reports a changed selection to the handler as `id | SELECTION_CHANGED`.
pub const SELECTION_CHANGED: u16 = 0x4000;

struct Ctx {
    form: *const Form,
    handler: *mut Handler<'static>,
    done: bool,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Form {
    /// A centred, fixed-size window with a client area of `w` x `h` dips. Not visible until `run`.
    pub fn new(title: &str, w: i32, h: i32) -> Result<Form, String> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2); // fails harmlessly if already set
            let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = w!("NuViewerForm");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(form_proc),
                hInstance: hinst.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH((windows::Win32::Graphics::Gdi::COLOR_BTNFACE.0 + 1) as usize as *mut c_void),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc); // 0 when the class already exists (a second form in one process): fine

            let dpi = GetDpiForSystem().max(96);
            let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let mut rc = RECT { left: 0, top: 0, right: w * dpi as i32 / 96, bottom: h * dpi as i32 / 96 };
            let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
            let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let t = wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(t.as_ptr()),
                style,
                ((sw - ww) / 2).max(0),
                ((sh - wh) / 2).max(0),
                ww,
                wh,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .map_err(|e| e.to_string())?;
            let font = CreateFontW(
                -(9 * dpi as i32 / 72),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                FONT_CHARSET(1), // DEFAULT_CHARSET
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(5), // CLEARTYPE_QUALITY
                0,
                w!("Segoe UI"),
            );
            Ok(Form { hwnd, font, dpi, page: std::cell::Cell::new(0), tracked: std::cell::RefCell::new(Vec::new()) })
        }
    }

    fn px(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    fn child(&self, class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, id: u16, x: i32, y: i32, w: i32, h: i32) -> HWND {
        unsafe {
            let t = wide(text);
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let c = CreateWindowExW(
                ex,
                class,
                PCWSTR(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                self.px(x),
                self.px(y),
                self.px(w),
                self.px(h),
                Some(self.hwnd),
                Some(HMENU(id as usize as *mut c_void)),
                Some(hinst.into()),
                None,
            )
            .unwrap_or_default();
            SendMessageW(c, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
            self.tracked.borrow_mut().push((self.page.get(), c));
            c
        }
    }

    /// Controls created after this call belong to `page` (0 = shown on every page).
    pub fn begin_page(&self, page: u8) {
        self.page.set(page);
    }

    /// Show the controls of `page` (and page 0), hide those of the other pages.
    pub fn show_page(&self, page: u8) {
        for (p, h) in self.tracked.borrow().iter() {
            unsafe {
                let _ = ShowWindow(*h, if *p == 0 || *p == page { SW_SHOW } else { SW_HIDE });
            }
        }
    }

    /// A label that can be found again by `id` (to change its text).
    pub fn label_id(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str) {
        self.child(w!("STATIC"), text, WINDOW_STYLE(0), WINDOW_EX_STYLE(0), id, x, y, w, h);
    }

    /// A push-button that stays pressed while its page is shown (radio behaviour inside one group).
    pub fn tab(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str, selected: bool, first_in_group: bool) {
        let mut style = WS_TABSTOP.0 | BS_AUTORADIOBUTTON as u32 | 0x1000 /* BS_PUSHLIKE */;
        if first_in_group {
            style |= WS_GROUP.0;
        }
        let c = self.child(w!("BUTTON"), text, WINDOW_STYLE(style), WINDOW_EX_STYLE(0), id, x, y, w, h);
        unsafe {
            SendMessageW(c, BM_SETCHECK, Some(WPARAM(selected as usize)), Some(LPARAM(0)));
        }
    }

    /// Drop-down list; `h` is the height of the opened list. Selection index via `combo_index`.
    pub fn combo(&self, id: u16, x: i32, y: i32, w: i32, h: i32, items: &[&str], selected: usize) {
        let c = self.child(w!("COMBOBOX"), "", WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(0x0003 /* CBS_DROPDOWNLIST */), WINDOW_EX_STYLE(0), id, x, y, w, h);
        for it in items {
            let t = wide(it);
            unsafe {
                SendMessageW(c, 0x0143 /* CB_ADDSTRING */, None, Some(LPARAM(t.as_ptr() as isize)));
            }
        }
        unsafe {
            SendMessageW(c, 0x014E /* CB_SETCURSEL */, Some(WPARAM(selected)), Some(LPARAM(0)));
        }
    }

    pub fn combo_index(&self, id: u16) -> usize {
        self.item(id).map(|c| unsafe { SendMessageW(c, 0x0147 /* CB_GETCURSEL */, None, None).0.max(0) as usize }).unwrap_or(0)
    }

    /// Select entry `index` of a combo box.
    pub fn combo_set(&self, id: u16, index: usize) {
        if let Some(c) = self.item(id) {
            unsafe {
                SendMessageW(c, 0x014E /* CB_SETCURSEL */, Some(WPARAM(index)), Some(LPARAM(0)));
            }
        }
    }

    /// Single-selection list box (notifies the handler with `id | SELECTION_CHANGED`).
    pub fn list(&self, id: u16, x: i32, y: i32, w: i32, h: i32) {
        self.child(w!("LISTBOX"), "", WS_TABSTOP | WS_VSCROLL | WS_BORDER | WINDOW_STYLE(0x0001 /* LBS_NOTIFY */ | 0x0040 /* LBS_NOINTEGRALHEIGHT */), WINDOW_EX_STYLE(0), id, x, y, w, h);
    }

    pub fn list_set(&self, id: u16, items: &[String], selected: Option<usize>) {
        let Some(c) = self.item(id) else { return };
        unsafe {
            SendMessageW(c, 0x0184 /* LB_RESETCONTENT */, None, None);
            for it in items {
                let t = wide(it);
                SendMessageW(c, 0x0180 /* LB_ADDSTRING */, None, Some(LPARAM(t.as_ptr() as isize)));
            }
            if let Some(s) = selected {
                SendMessageW(c, 0x0186 /* LB_SETCURSEL */, Some(WPARAM(s)), Some(LPARAM(0)));
            }
        }
    }

    /// Selected row of a list box, if any.
    pub fn list_index(&self, id: u16) -> Option<usize> {
        let c = self.item(id)?;
        let r = unsafe { SendMessageW(c, 0x0188 /* LB_GETCURSEL */, None, None).0 };
        (r >= 0).then_some(r as usize)
    }

    pub fn label(&self, x: i32, y: i32, w: i32, h: i32, text: &str) {
        self.child(w!("STATIC"), text, WINDOW_STYLE(0) /* SS_LEFT */, WINDOW_EX_STYLE(0), 0, x, y, w, h);
    }

    pub fn group(&self, x: i32, y: i32, w: i32, h: i32, text: &str) {
        self.child(w!("BUTTON"), text, WINDOW_STYLE(BS_GROUPBOX as u32), WINDOW_EX_STYLE(0), 0, x, y, w, h);
    }

    pub fn check(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str, checked: bool) {
        let c = self.child(w!("BUTTON"), text, WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0), id, x, y, w, h);
        unsafe {
            SendMessageW(c, BM_SETCHECK, Some(WPARAM(checked as usize)), Some(LPARAM(0)));
        }
    }

    pub fn edit(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str) {
        self.child(w!("EDIT"), text, WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WS_EX_CLIENTEDGE, id, x, y, w, h);
    }

    pub fn button(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str, default: bool) {
        let kind = if default { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON };
        self.child(w!("BUTTON"), text, WS_TABSTOP | WINDOW_STYLE(kind as u32), WINDOW_EX_STYLE(0), id, x, y, w, h);
    }

    fn item(&self, id: u16) -> Option<HWND> {
        unsafe { GetDlgItem(Some(self.hwnd), id as i32).ok() }
    }

    pub fn is_checked(&self, id: u16) -> bool {
        self.item(id).map(|c| unsafe { SendMessageW(c, BM_GETCHECK, None, None).0 == 1 }).unwrap_or(false)
    }

    pub fn set_checked(&self, id: u16, on: bool) {
        if let Some(c) = self.item(id) {
            unsafe {
                SendMessageW(c, BM_SETCHECK, Some(WPARAM(on as usize)), Some(LPARAM(0)));
            }
        }
    }

    pub fn text(&self, id: u16) -> String {
        let Some(c) = self.item(id) else { return String::new() };
        unsafe {
            let n = GetWindowTextLengthW(c).max(0) as usize;
            let mut buf = vec![0u16; n + 1];
            let got = GetWindowTextW(c, &mut buf).max(0) as usize;
            String::from_utf16_lossy(&buf[..got])
        }
    }

    pub fn set_text(&self, id: u16, text: &str) {
        if let Some(c) = self.item(id) {
            let t = wide(text);
            unsafe {
                let _ = SetWindowTextW(c, PCWSTR(t.as_ptr()));
            }
        }
    }

    pub fn enable(&self, id: u16, on: bool) {
        if let Some(c) = self.item(id) {
            unsafe {
                let _ = EnableWindow(c, on);
            }
        }
    }

    /// Show the form and run its own message loop until `handler` returns true (or the window is destroyed).
    pub fn run(self, handler: &mut Handler<'_>) {
        unsafe {
            // The pointer outlives this call only inside the window proc, which can't run after the loop below ends.
            let handler_ptr: *mut Handler<'static> = std::mem::transmute(handler as *mut Handler<'_>);
            let mut ctx = Ctx { form: &self, handler: handler_ptr, done: false };
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, &mut ctx as *mut Ctx as isize);
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            // Focus the first control that takes input.
            if let Ok(first) = GetWindow(self.hwnd, GW_CHILD) {
                let _ = SetFocus(Some(first));
            }
            let mut msg = MSG::default();
            while !ctx.done && GetMessageW(&mut msg, None, 0, 0).as_bool() {
                if !IsDialogMessageW(self.hwnd, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}

unsafe extern "system" fn form_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as u16;
            let notify = ((wp.0 >> 16) & 0xFFFF) as u32;
            // Buttons report BN_CLICKED (0); Esc / Enter from IsDialogMessage arrive as IDCANCEL / IDOK with notify 0 too.
            // List and combo boxes report a changed selection as 1 (LBN_SELCHANGE / CBN_SELCHANGE).
            let id = if notify == 1 { id | SELECTION_CHANGED } else { id };
            if (notify == 0 || notify == 1) && id != 0 {
                if let Some(c) = ctx.as_mut() {
                    let handler = &mut *c.handler;
                    if handler(&*c.form, id) {
                        c.done = true;
                        let _ = DestroyWindow(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(c) = ctx.as_mut() {
                let handler = &mut *c.handler;
                if handler(&*c.form, ID_CANCEL) {
                    c.done = true;
                    let _ = DestroyWindow(hwnd);
                }
            } else {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(c) = ctx.as_mut() {
                c.done = true;
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
