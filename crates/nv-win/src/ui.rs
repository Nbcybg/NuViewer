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
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const ID_OK: u16 = 1;
pub const ID_CANCEL: u16 = 2;

/// Called for every button click (and for Esc / the close box, which arrive as `ID_CANCEL`).
/// Return true to close the form; read the controls *inside* the callback, they are gone afterwards.
pub type Handler<'a> = dyn FnMut(&Form, u16) -> bool + 'a;

pub struct Form {
    pub hwnd: HWND,
    /// The program's main window while this form is open (disabled until the form closes), if there is one.
    owner: Option<HWND>,
    owner_disabled: std::cell::Cell<bool>,
    /// The form that was the innermost one before this one opened (put back when it closes).
    previous_top: std::cell::Cell<isize>,
    top_registered: std::cell::Cell<bool>,
    font: HFONT,
    dpi: u32,
    /// Page that controls created from now on belong to (0 = always visible), see `show_page`.
    page: std::cell::Cell<u8>,
    tracked: std::cell::RefCell<Vec<(u8, HWND)>>,
    /// Added to the position of every control created from now on (a sidebar on the left pushes the pages to the right).
    origin: std::cell::Cell<(i32, i32)>,
    /// The sidebar entries (drawn by `draw_side_tab`) and which one is selected.
    side: std::cell::RefCell<Vec<SideTab>>,
    side_selected: std::cell::Cell<u16>,
}

/// One entry of a sidebar: an icon (premultiplied BGRA bitmap) and a text.
pub struct SideTab {
    id: u16,
    text: String,
    bitmap: Option<(windows::Win32::Graphics::Gdi::HBITMAP, i32)>,
    hwnd: HWND,
}

/// A list box or combo box reports a changed selection to the handler as `id | SELECTION_CHANGED`.
pub const SELECTION_CHANGED: u16 = 0x4000;
/// A slider that was moved is reported as `id | SLIDER_MOVED` (control ids stay below 0x2000).
pub const SLIDER_MOVED: u16 = 0x2000;

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
            // With the viewer's window around, the form is *owned* by it: it stays above it, shares its Alt+Tab entry
            // (switching back lands on the form) and cannot be minimised separately.
            let owner = crate::dialog_parent();
            let style = if owner.is_some() { WS_CAPTION | WS_SYSMENU } else { WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX };
            let mut rc = RECT { left: 0, top: 0, right: w * dpi as i32 / 96, bottom: h * dpi as i32 / 96 };
            let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
            let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
            // Centred on the monitor the viewer is on (the primary one when there is no viewer window).
            let (ml, mt, mr, mb) = match owner {
                Some(o) => crate::monitor_rect(o),
                None => (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)),
            };
            let t = wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(t.as_ptr()),
                style,
                ml + ((mr - ml - ww) / 2).max(0),
                mt + ((mb - mt - wh) / 2).max(0),
                ww,
                wh,
                owner,
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
            Ok(Form { hwnd, owner, owner_disabled: std::cell::Cell::new(false), previous_top: std::cell::Cell::new(0), top_registered: std::cell::Cell::new(false), font, dpi, page: std::cell::Cell::new(0), tracked: std::cell::RefCell::new(Vec::new()), origin: std::cell::Cell::new((0, 0)), side: std::cell::RefCell::new(Vec::new()), side_selected: std::cell::Cell::new(0) })
        }
    }

    /// Give the owner window back its input. Must happen *before* the form is destroyed, otherwise Windows hands the
    /// activation to some other program's window and the viewer ends up behind it.
    fn release_owner(&self) {
        if self.owner_disabled.replace(false) {
            if let Some(o) = self.owner {
                unsafe {
                    let _ = EnableWindow(o, true);
                }
            }
        }
        if self.top_registered.replace(false) {
            // This form is no longer the one new dialogs belong to.
            crate::TOP_FORM.store(self.previous_top.get(), std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Release the owner and destroy the form's window.
    fn close(&self) {
        self.release_owner();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn px(&self, v: i32) -> i32 {
        v * self.dpi as i32 / 96
    }

    fn child(&self, class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, id: u16, x: i32, y: i32, w: i32, h: i32) -> HWND {
        unsafe {
            // Words on labels, check boxes, buttons and group boxes are translated; an edit box holds the user's own text.
            let is_text = class.to_string().map(|c| c == "STATIC" || c == "BUTTON").unwrap_or(false);
            let t = if is_text { wide(&nv_text::tr(text)) } else { wide(text) };
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let c = CreateWindowExW(
                ex,
                class,
                PCWSTR(t.as_ptr()),
                WS_CHILD | WS_VISIBLE | style,
                self.px(x + self.origin.get().0),
                self.px(y + self.origin.get().1),
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

    /// Controls created from now on are placed relative to (`x`, `y`) instead of the top-left corner.
    pub fn set_origin(&self, x: i32, y: i32) {
        self.origin.set((x, y));
    }

    /// A sidebar entry: icon (`px` x `px` premultiplied BGRA, or none) and text, drawn by the form; a click arrives as `id`.
    /// Call `select_side` from the handler to show which one is current.
    pub fn side_tab(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str, icon: Option<(i32, &[u8])>, selected: bool) {
        use windows::Win32::Graphics::Gdi::*;
        let c = self.child(w!("BUTTON"), text, WINDOW_STYLE(WS_TABSTOP.0 | 0x0B /* BS_OWNERDRAW */), WINDOW_EX_STYLE(0), id, x, y, w, h);
        let bitmap = icon.and_then(|(px, bgra)| unsafe {
            let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: px, biHeight: -px, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            if bits.is_null() || bgra.len() < (px * px * 4) as usize {
                return None;
            }
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, (px * px * 4) as usize);
            Some((dib, px))
        });
        self.side.borrow_mut().push(SideTab { id, text: text.to_string(), bitmap, hwnd: c });
        if selected {
            self.side_selected.set(id);
        }
    }

    /// Mark `id` as the current sidebar entry and redraw them.
    pub fn select_side(&self, id: u16) {
        self.side_selected.set(id);
        for t in self.side.borrow().iter() {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(t.hwnd), None, true);
            }
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
            let t = wide(&nv_text::tr(it));
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
        let g = self.child(w!("BUTTON"), text, WINDOW_STYLE(BS_GROUPBOX as u32), WINDOW_EX_STYLE(0), 0, x, y, w, h);
        // A group box goes to the bottom of the stack so it can never paint over the controls inside it.
        unsafe {
            let _ = SetWindowPos(g, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn check(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str, checked: bool) {
        let c = self.child(w!("BUTTON"), text, WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0), id, x, y, w, h);
        unsafe {
            SendMessageW(c, BM_SETCHECK, Some(WPARAM(checked as usize)), Some(LPARAM(0)));
        }
    }

    /// A horizontal slider from `lo` to `hi`. Moving it reports `id | SLIDER_MOVED` to the handler.
    pub fn slider(&self, id: u16, x: i32, y: i32, w: i32, h: i32, lo: i32, hi: i32, value: i32) {
        unsafe {
            let icc = windows::Win32::UI::Controls::INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<windows::Win32::UI::Controls::INITCOMMONCONTROLSEX>() as u32, dwICC: windows::Win32::UI::Controls::ICC_BAR_CLASSES };
            let _ = windows::Win32::UI::Controls::InitCommonControlsEx(&icc);
        }
        let c = self.child(w!("msctls_trackbar32"), "", WS_TABSTOP | WINDOW_STYLE(0x0010 /* TBS_NOTICKS */), WINDOW_EX_STYLE(0), id, x, y, w, h);
        unsafe {
            SendMessageW(c, 0x0407 /* TBM_SETRANGEMIN */, Some(WPARAM(0)), Some(LPARAM(lo as isize)));
            SendMessageW(c, 0x0408 /* TBM_SETRANGEMAX */, Some(WPARAM(1)), Some(LPARAM(hi as isize)));
            SendMessageW(c, 0x0405 /* TBM_SETPOS */, Some(WPARAM(1)), Some(LPARAM(value as isize)));
        }
    }

    pub fn slider_value(&self, id: u16) -> i32 {
        self.item(id).map(|c| unsafe { SendMessageW(c, 0x0400 /* TBM_GETPOS */, None, None).0 as i32 }).unwrap_or(0)
    }

    pub fn slider_set(&self, id: u16, value: i32) {
        if let Some(c) = self.item(id) {
            unsafe {
                SendMessageW(c, 0x0405, Some(WPARAM(1)), Some(LPARAM(value as isize)));
            }
        }
    }

    pub fn edit(&self, id: u16, x: i32, y: i32, w: i32, h: i32, text: &str) {
        self.child(w!("EDIT"), text, WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WS_EX_CLIENTEDGE, id, x, y, w, h);
    }

    /// An edit box that shows dots instead of what is typed.
    pub fn password(&self, id: u16, x: i32, y: i32, w: i32, h: i32) {
        self.child(w!("EDIT"), "", WS_TABSTOP | WINDOW_STYLE((ES_AUTOHSCROLL | ES_PASSWORD) as u32), WS_EX_CLIENTEDGE, id, x, y, w, h);
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
            if let Some(o) = self.owner {
                let _ = EnableWindow(o, false); // modal: the viewer behind it takes no input until the form closes
                self.owner_disabled.set(true);
            }
            self.previous_top.set(crate::TOP_FORM.swap(self.hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed));
            self.top_registered.set(true);
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
            self.release_owner();
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
            let _ = DeleteObject(HGDIOBJ(self.font.0));
        }
    }
}

/// A sidebar entry: a pale blue plate with an accent bar when selected, a light grey one under the pointer, the icon and the text.
unsafe fn draw_side_tab(form: &Form, d: &DRAWITEMSTRUCT) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::*;
    let side = form.side.borrow();
    let Some(tab) = side.iter().find(|t| t.id == d.CtlID as u16) else { return };
    let selected = form.side_selected.get() == tab.id;
    let hot = d.itemState.0 & 0x40 != 0 /* ODS_HOTLIGHT */ || {
        let mut pt = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mut r = RECT::default();
        let _ = GetWindowRect(tab.hwnd, &mut r);
        pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom
    };
    let dc = d.hDC;
    let r = d.rcItem;
    let fill = if selected { 0x00F7_E6D6 } else if hot { 0x00F0_EDEA } else { 0x00FF_FFFF }; // BGR
    let brush = CreateSolidBrush(COLORREF(fill));
    FillRect(dc, &r, brush);
    let _ = DeleteObject(brush.into());
    let s = |v: i32| v * form.dpi as i32 / 96;
    if selected {
        let bar = CreateSolidBrush(COLORREF(0x00D7_7800)); // accent blue
        FillRect(dc, &RECT { left: r.left, top: r.top + s(5), right: r.left + s(3), bottom: r.bottom - s(5) }, bar);
        let _ = DeleteObject(bar.into());
    }
    if let Some((bmp, px)) = tab.bitmap {
        let mem = CreateCompatibleDC(Some(dc));
        let old = SelectObject(mem, bmp.into());
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 /* AC_SRC_ALPHA */ };
        let size = s(20);
        let _ = AlphaBlend(dc, r.left + s(14), r.top + (r.bottom - r.top - size) / 2, size, size, mem, 0, 0, px, px, blend);
        SelectObject(mem, old);
        let _ = DeleteDC(mem);
    }
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, COLORREF(if selected { 0x0060_3000 } else { 0x0020_2020 }));
    let old_font = SelectObject(dc, form.font.into());
    let mut text: Vec<u16> = tab.text.encode_utf16().collect();
    let mut tr = RECT { left: r.left + s(44), top: r.top, right: r.right - s(6), bottom: r.bottom };
    DrawTextW(dc, &mut text, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS | DT_NOPREFIX);
    SelectObject(dc, old_font);
}

unsafe extern "system" fn form_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ctx;
    match msg {
        WM_DRAWITEM => {
            if let Some(c) = ctx.as_ref() {
                draw_side_tab(&*c.form, &*(lp.0 as *const DRAWITEMSTRUCT));
            }
            LRESULT(1)
        }
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as u16;
            let notify = ((wp.0 >> 16) & 0xFFFF) as u32;
            if let Some(c) = ctx.as_ref() {
                if notify == 0 && (*c.form).side.borrow().iter().any(|t| t.id == id) {
                    (*c.form).select_side(id);
                }
            }
            // Buttons report BN_CLICKED (0); Esc / Enter from IsDialogMessage arrive as IDCANCEL / IDOK with notify 0 too.
            // List and combo boxes report a changed selection as 1 (LBN_SELCHANGE / CBN_SELCHANGE).
            let id = if notify == 1 { id | SELECTION_CHANGED } else { id };
            if (notify == 0 || notify == 1) && id != 0 {
                if let Some(c) = ctx.as_mut() {
                    let handler = &mut *c.handler;
                    if handler(&*c.form, id) {
                        c.done = true;
                        (*c.form).close();
                    }
                }
            }
            LRESULT(0)
        }
        WM_HSCROLL => {
            // A slider moved: lp is the slider's window.
            let id = GetDlgCtrlID(HWND(lp.0 as *mut c_void)) as u16;
            if let Some(c) = ctx.as_mut() {
                if id != 0 {
                    let handler = &mut *c.handler;
                    if handler(&*c.form, id | SLIDER_MOVED) {
                        c.done = true;
                        (*c.form).close();
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
                    (*c.form).close();
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
