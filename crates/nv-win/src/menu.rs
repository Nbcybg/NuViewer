//! Popup (context) menus, drawn by us: the system menu is only the frame and the keyboard / pointer handling, every line is
//! painted here in the colours of the skin in use (`nv_text::skin`), with an icon column, a hint column for the shortcut and
//! an arrow for a submenu. All three programs and their menu bars go through `popup`, so they all look the same.
//!
//! How it works: every line is appended as an owner-drawn item whose data is its index in a table kept for the time the menu is
//! open; while it is open the owner window is sub-classed to answer `WM_MEASUREITEM` and `WM_DRAWITEM`, so no program's own
//! window procedure has to know about any of it.

use std::cell::RefCell;
use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Entry {
    /// `id` must be non-zero. `text` may contain a tab: what follows is drawn right-aligned (the shortcut hint).
    Item { id: u32, text: String, checked: bool, enabled: bool, icon: char },
    /// A nested menu (its items' ids are returned like any other).
    Submenu { text: String, items: Vec<Entry>, enabled: bool, icon: char },
    Separator,
}

/// Segoe MDL2 Assets code points used by the menus (the font has shipped with every Windows 10 and 11).
pub mod icon {
    pub const OPEN: char = '\u{E8E5}';
    pub const OPEN_WITH: char = '\u{E8A7}';
    pub const REFRESH: char = '\u{E72C}';
    pub const SAVE: char = '\u{E74E}';
    pub const SAVE_AS: char = '\u{E792}';
    pub const GALLERY: char = '\u{E8A9}';
    pub const SLIDESHOW: char = '\u{E786}';
    pub const PLAY: char = '\u{E768}';
    pub const ZOOM: char = '\u{E71E}';
    pub const ZOOM_IN: char = '\u{E8A3}';
    pub const ZOOM_OUT: char = '\u{E71F}';
    pub const FIT: char = '\u{E9A6}';
    pub const ACTUAL: char = '\u{E739}';
    pub const ROTATE: char = '\u{E8B4}';
    pub const TURN_LEFT: char = '\u{E7A7}';
    pub const TURN_RIGHT: char = '\u{E7A6}';
    pub const FLIP_H: char = '\u{E8AB}';
    pub const FLIP_V: char = '\u{E8CB}';
    pub const VIEW: char = '\u{E7B3}';
    pub const FULLSCREEN: char = '\u{E740}';
    pub const WINDOW: char = '\u{E737}';
    pub const INFO: char = '\u{E946}';
    pub const FILM: char = '\u{E8B2}';
    pub const SIDEBAR: char = '\u{E700}';
    pub const DIM: char = '\u{E708}';
    pub const GLOBE: char = '\u{E774}';
    pub const BOOK: char = '\u{E736}';
    pub const SORT: char = '\u{E8CB}';
    pub const SHUFFLE: char = '\u{E8B1}';
    pub const LIBRARY: char = '\u{E8F1}';
    pub const STACK: char = '\u{E8B9}';
    pub const CAMERA: char = '\u{E722}';
    pub const STAR: char = '\u{E734}';
    pub const TOOLS: char = '\u{E90F}';
    pub const DOT: char = '\u{E7C8}';
    pub const PALETTE: char = '\u{E790}';
    pub const CHART: char = '\u{E9D9}';
    pub const SCAN: char = '\u{E8B8}';
    pub const CONVERT: char = '\u{E8EB}';
    pub const COPY: char = '\u{E8C8}';
    pub const LINK: char = '\u{E71B}';
    pub const RENAME: char = '\u{E8AC}';
    pub const DELETE: char = '\u{E74D}';
    pub const UNDO: char = '\u{E7A7}';
    pub const FOLDER: char = '\u{E838}';
    pub const CUT: char = '\u{E8C6}';
    pub const SHARE: char = '\u{E72D}';
    pub const EDIT: char = '\u{E70F}';
    pub const PLUGIN: char = '\u{E74C}';
    pub const SETTINGS: char = '\u{E713}';
    pub const PAGE: char = '\u{E7C3}';
    pub const HELP: char = '\u{E897}';
    pub const CLOSE: char = '\u{E711}';
    pub const LAYERS: char = '\u{E81E}';
    pub const NEXT: char = '\u{E72A}';
    pub const PREV: char = '\u{E72B}';
    pub const SLIDERS: char = '\u{E9E9}';
    pub const SKIN: char = '\u{E790}';
    pub const CHECK: char = '\u{E73E}';
    pub const CHEVRON: char = '\u{E76C}';
    pub const FILTER: char = '\u{E71C}';
    pub const ADD: char = '\u{E710}';
    pub const SELECT: char = '\u{E73A}';
    pub const MORE: char = '\u{E712}';
    pub const PRINT: char = '\u{E749}';
}

impl Entry {
    pub fn item(id: u32, text: impl Into<String>) -> Entry {
        Entry::Item { id, text: text.into(), checked: false, enabled: true, icon: '\0' }
    }

    pub fn checked(id: u32, text: impl Into<String>, checked: bool) -> Entry {
        Entry::Item { id, text: text.into(), checked, enabled: true, icon: '\0' }
    }

    pub fn sub(text: impl Into<String>, items: Vec<Entry>) -> Entry {
        Entry::Submenu { text: text.into(), items, enabled: true, icon: '\0' }
    }

    pub fn enabled(self, on: bool) -> Entry {
        match self {
            Entry::Item { id, text, checked, icon, .. } => Entry::Item { id, text, checked, enabled: on, icon },
            Entry::Submenu { text, items, icon, .. } => Entry::Submenu { text, items, enabled: on, icon },
            sep => sep,
        }
    }

    /// The picture (a Segoe MDL2 Assets character) shown in front of the line.
    pub fn icon(self, c: char) -> Entry {
        match self {
            Entry::Item { id, text, checked, enabled, .. } => Entry::Item { id, text, checked, enabled, icon: c },
            Entry::Submenu { text, items, enabled, .. } => Entry::Submenu { text, items, enabled, icon: c },
            sep => sep,
        }
    }

    /// A line the user cannot pick, drawn like a heading's small print.
    pub fn is_separator(&self) -> bool {
        matches!(self, Entry::Separator)
    }
}

/// An icon for a line that has none of its own, from the words on it (the programs share these words: Rename, Delete, ...).
pub fn icon_for(text: &str) -> char {
    let t = text.split('\t').next().unwrap_or(text).trim_end_matches(['\u{2026}', '.', ' ']).to_ascii_lowercase();
    match t.as_str() {
        "open" | "open file" | "open folder" => icon::OPEN,
        "open with" => icon::OPEN_WITH,
        "refresh" | "reload" => icon::REFRESH,
        "save" => icon::SAVE,
        "save as" | "save a copy" | "save copy" => icon::SAVE_AS,
        "copy" | "copy picture" | "copy image" | "copy pictures" => icon::COPY,
        "copy path" | "copy file path" | "copy full path" => icon::LINK,
        "cut" => icon::CUT,
        "paste" => icon::PAGE,
        "rename" | "batch rename" => icon::RENAME,
        "delete" | "delete (recycle bin)" | "delete permanently" | "remove" => icon::DELETE,
        "undo" => icon::UNDO,
        "properties" | "info" | "info / exif" | "details" => icon::INFO,
        "share" | "share to" => icon::SHARE,
        "settings" | "options" | "preferences" => icon::SETTINGS,
        "help" | "keys, mouse and credits" => icon::HELP,
        "close" | "quit" | "exit" => icon::CLOSE,
        "new folder" | "add" => icon::ADD,
        "show in explorer" | "reveal in explorer" | "open folder location" | "open in explorer" => icon::FOLDER,
        "select all" => icon::STACK,
        "sort by" | "sort" => icon::SORT,
        "slideshow" => icon::SLIDESHOW,
        "gallery" | "grid" | "thumbnails" => icon::GALLERY,
        "fullscreen" | "full screen" => icon::FULLSCREEN,
        "zoom in" => icon::ZOOM_IN,
        "zoom out" => icon::ZOOM_OUT,
        "zoom" => icon::ZOOM,
        "rotate" | "rotate left" | "rotate right" => icon::ROTATE,
        "flip horizontally" => icon::FLIP_H,
        "flip vertically" => icon::FLIP_V,
        "plug-ins" | "plugins" => icon::PLUGIN,
        "edit" | "edit in nueditor" | "nueditor" | "edit with" => icon::EDIT,
        "extract frames" | "snapshot" => icon::SCAN,
        "convert" => icon::CONVERT,
        "filter" | "show only" => icon::FILTER,
        "copy file" => icon::COPY,
        "copy to" => icon::COPY,
        "copy & move" => icon::COPY,
        "move to" => icon::CUT,
        "print" => icon::PRINT,
        "select" => icon::SELECT,
        "more" => icon::MORE,
        "batch" => icon::SLIDERS,
        "view" => icon::VIEW,
        "group by" => icon::LIBRARY,
        "quick convert" | "convert to png (here, no questions)" | "convert to jpeg (here, no questions)" => icon::CONVERT,
        "show file location" => icon::FOLDER,
        "edit text in nunote" => icon::EDIT,
        "compress" | "unpack here" => icon::STACK,
        "make a pdf" => icon::PAGE,
        "rotate & flip" | "quick edit" => icon::ROTATE,
        "rating" => icon::STAR,
        _ => '\0',
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Item,
    Sub,
    Sep,
}

struct Row {
    kind: Kind,
    text: String,
    hint: String,
    icon: char,
    checked: bool,
    enabled: bool,
}

/// What the menu being shown needs while it is open.
struct Ctx {
    rows: Vec<Row>,
    scale: f32,
    font: HFONT,
    icons: HFONT,
    sk: &'static nv_text::skin::Skin,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

fn cref(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xFF) << 16) | (hex & 0xFF00) | ((hex >> 16) & 0xFF))
}

unsafe fn fill(dc: HDC, r: &RECT, hex: u32) {
    let b = CreateSolidBrush(cref(hex));
    FillRect(dc, r, b);
    let _ = DeleteObject(b.into());
}

unsafe fn fill_round(dc: HDC, r: &RECT, hex: u32, radius: i32) {
    let b = CreateSolidBrush(cref(hex));
    let rgn = CreateRoundRectRgn(r.left, r.top, r.right + 1, r.bottom + 1, radius, radius);
    let _ = FillRgn(dc, rgn, b);
    let _ = DeleteObject(rgn.into());
    let _ = DeleteObject(b.into());
}

unsafe fn make_font(face: PCWSTR, px: i32) -> HFONT {
    CreateFontW(-px, 0, 0, 0, 400, 0, 0, 0, FONT_CHARSET(1), FONT_OUTPUT_PRECISION(0), FONT_CLIP_PRECISION(0), FONT_QUALITY(5), 0, face)
}

unsafe fn text_w(dc: HDC, s: &str) -> i32 {
    let wide: Vec<u16> = s.encode_utf16().collect();
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
    size.cx
}

unsafe fn draw_text(dc: HDC, s: &str, r: &mut RECT, flags: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    DrawTextW(dc, &mut wide, r, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
}

impl Ctx {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    unsafe fn measure(&self, idx: usize) -> (u32, u32) {
        let Some(row) = self.rows.get(idx) else { return (120, 24) };
        if row.kind == Kind::Sep {
            return (self.px(40.0) as u32, self.px(9.0) as u32);
        }
        let dc = GetDC(None);
        let old = SelectObject(dc, self.font.into());
        let tw = text_w(dc, &row.text);
        let hw = if row.hint.is_empty() { 0 } else { text_w(dc, &row.hint) + self.px(28.0) };
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        let w = self.px(8.0) + self.px(28.0) + self.px(8.0) + tw + hw + self.px(12.0) + self.px(22.0);
        (w.max(self.px(236.0)) as u32, self.px(30.0) as u32)
    }

    unsafe fn draw(&self, d: &DRAWITEMSTRUCT) {
        let Some(row) = self.rows.get(d.itemData) else { return };
        let (dc, rc, sk) = (d.hDC, d.rcItem, self.sk);
        fill(dc, &rc, sk.panel);
        if row.kind == Kind::Sep {
            let y = (rc.top + rc.bottom) / 2;
            fill(dc, &RECT { left: rc.left + self.px(12.0), top: y, right: rc.right - self.px(12.0), bottom: y + 1.max(self.px(1.0)) }, sk.edge);
            return;
        }
        let hot = row.enabled && (d.itemState.0 & ODS_SELECTED.0) != 0;
        if hot {
            let r = RECT { left: rc.left + self.px(4.0), top: rc.top + self.px(1.0), right: rc.right - self.px(4.0), bottom: rc.bottom - self.px(1.0) };
            fill_round(dc, &r, sk.selection(), self.px(8.0));
            let bar = RECT { left: r.left, top: r.top + self.px(7.0), right: r.left + self.px(3.0).max(2), bottom: r.bottom - self.px(7.0) };
            fill_round(dc, &bar, sk.accent, 2);
        }
        let ink = if !row.enabled { sk.disabled() } else { sk.text };
        let soft = if !row.enabled { sk.disabled() } else if hot { sk.text } else { sk.dim };
        SetBkMode(dc, TRANSPARENT);
        // Icon column: the picture, or a tick; a line that is switched on gets its picture on the accent colour.
        let col = RECT { left: rc.left + self.px(8.0), top: rc.top, right: rc.left + self.px(8.0 + 28.0), bottom: rc.bottom };
        let old = SelectObject(dc, self.icons.into());
        let mut glyph = row.icon;
        if row.checked {
            let side = self.px(22.0);
            let (cx, cy) = ((col.left + col.right) / 2, (col.top + col.bottom) / 2);
            let chip = RECT { left: cx - side / 2, top: cy - side / 2, right: cx - side / 2 + side, bottom: cy - side / 2 + side };
            fill_round(dc, &chip, if row.enabled { sk.accent } else { sk.disabled() }, self.px(6.0));
            SetTextColor(dc, cref(sk.on_accent()));
            if glyph == '\0' {
                glyph = icon::CHECK;
            }
            let mut r = chip;
            draw_text(dc, &glyph.to_string(), &mut r, DT_CENTER);
        } else if glyph != '\0' {
            SetTextColor(dc, cref(soft));
            let mut r = col;
            draw_text(dc, &glyph.to_string(), &mut r, DT_CENTER);
        }
        if row.kind == Kind::Sub {
            SetTextColor(dc, cref(soft));
            let mut r = RECT { left: rc.right - self.px(26.0), top: rc.top, right: rc.right - self.px(8.0), bottom: rc.bottom };
            draw_text(dc, &icon::CHEVRON.to_string(), &mut r, DT_CENTER);
        }
        SelectObject(dc, self.font.into());
        let right = rc.right - self.px(12.0 + 22.0);
        if !row.hint.is_empty() {
            SetTextColor(dc, cref(soft));
            let mut r = RECT { left: rc.left, top: rc.top, right, bottom: rc.bottom };
            draw_text(dc, &row.hint, &mut r, DT_RIGHT);
        }
        SetTextColor(dc, cref(ink));
        let hint_w = if row.hint.is_empty() { 0 } else { text_w(dc, &row.hint) + self.px(20.0) };
        let mut r = RECT { left: rc.left + self.px(8.0 + 28.0 + 8.0), top: rc.top, right: right - hint_w, bottom: rc.bottom };
        draw_text(dc, &row.text, &mut r, DT_LEFT | DT_END_ELLIPSIS);
        SelectObject(dc, old);
        // Windows draws its own black arrow on a submenu line after this returns (over ours); keeping that corner out of the
        // clip of the DC it paints with is the way to stop it.
        if row.kind == Kind::Sub {
            let _ = ExcludeClipRect(dc, rc.right - self.px(34.0), rc.top, rc.right, rc.bottom);
        }
    }
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_MEASUREITEM if wp.0 == 0 && lp.0 != 0 => {
            let m = &mut *(lp.0 as *mut MEASUREITEMSTRUCT);
            if m.CtlType.0 == 1 {
                CTX.with(|c| {
                    if let Some(c) = c.borrow().as_ref() {
                        let (w, h) = c.measure(m.itemData);
                        m.itemWidth = w;
                        m.itemHeight = h;
                    }
                });
                return LRESULT(1);
            }
        }
        WM_DRAWITEM if wp.0 == 0 && lp.0 != 0 => {
            let d = &*(lp.0 as *const DRAWITEMSTRUCT);
            if d.CtlType.0 == 1 {
                CTX.with(|c| {
                    if let Some(c) = c.borrow().as_ref() {
                        c.draw(d);
                    }
                });
                return LRESULT(1);
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

/// Ask Windows to draw the menu frame (border, shadow, scroll arrows) dark or light to match the skin. Undocumented but stable
/// since Windows 10 1903; if the entry points are not there nothing happens.
fn frame_mode(dark: Option<bool>) {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        let set = GetProcAddress(lib, PCSTR(135 as *const u8));
        let flush = GetProcAddress(lib, PCSTR(136 as *const u8));
        if let Some(set) = set {
            let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set);
            set(match dark {
                None => 0,
                Some(true) => 2,
                Some(false) => 3,
            });
        }
        if let Some(flush) = flush {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

/// Show the menu at a point given in client pixels of `hwnd` and wait. Returns the chosen item's id, or None if dismissed.
pub fn popup(hwnd: HWND, x: i32, y: i32, entries: &[Entry]) -> Option<u32> {
    unsafe {
        let mut rows = Vec::new();
        let menu = build(entries, &mut rows)?;
        let sk = nv_text::skin::current();
        let dpi = GetDpiForWindow(hwnd).max(96);
        // NV_MENU_SCALE: tests draw the menu at another scale (1.5, 2.0) to check the layout without changing the screen's.
        let scale = std::env::var("NV_MENU_SCALE").ok().and_then(|s| s.parse::<f32>().ok()).filter(|s| (0.5..=4.0).contains(s)).unwrap_or(dpi as f32 / 96.0);
        let font = make_font(w!("Segoe UI"), (14.0 * scale).round() as i32);
        let icons = make_font(w!("Segoe MDL2 Assets"), (16.0 * scale).round() as i32);
        CTX.with(|c| *c.borrow_mut() = Some(Ctx { rows, scale, font, icons, sk }));
        // The strip around the lines (the menu's own margin) is the panel colour too.
        let back = CreateSolidBrush(cref(sk.panel));
        let info = MENUINFO { cbSize: std::mem::size_of::<MENUINFO>() as u32, fMask: MIM_BACKGROUND | MIM_APPLYTOSUBMENUS, hbrBack: back, ..Default::default() };
        let _ = SetMenuInfo(menu, &info);
        let _ = SetWindowSubclass(hwnd, Some(subclass), 0x4E4D, 0);
        frame_mode(Some(sk.dark));
        let mut pt = POINT { x, y };
        let _ = ClientToScreen(hwnd, &mut pt);
        // The window must be in front or the menu won't close when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_TOPALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)); // documented workaround so the menu dismisses cleanly
        frame_mode(None);
        let _ = RemoveWindowSubclass(hwnd, Some(subclass), 0x4E4D);
        let _ = DestroyMenu(menu); // also destroys the submenus
        let _ = DeleteObject(back.into());
        CTX.with(|c| {
            if let Some(c) = c.borrow_mut().take() {
                let _ = DeleteObject(c.font.into());
                let _ = DeleteObject(c.icons.into());
            }
        });
        let id = picked.0 as u32;
        (id != 0).then_some(id)
    }
}

use windows::Win32::Graphics::Gdi::ClientToScreen;

/// The text the system keeps for a line: the words as they read (a real `&` doubled so it is not taken for a marker), the hint
/// after a tab. With the words kept, a screen reader reads each line, a test can read them, and a letter typed in the open menu
/// goes to the line that starts with it, as in every Windows menu.
fn system_text(label: &str, hint: &str) -> Vec<u16> {
    let mut s = String::new();
    for c in label.chars() {
        if c == '&' {
            s.push('&');
        }
        s.push(c);
    }
    if !hint.is_empty() {
        s.push('\t');
        s.push_str(hint);
    }
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Add a line with its text (so screen readers and Alt+letter see it), then make it owner-drawn with row `idx` as its data.
unsafe fn add_line(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: &[u16], idx: usize) {
    let _ = AppendMenuW(menu, MF_STRING | flags, id, PCWSTR(text.as_ptr()));
    let pos = (GetMenuItemCount(Some(menu)) - 1).max(0) as u32;
    let info = MENUITEMINFOW { cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32, fMask: MIIM_FTYPE | MIIM_DATA, fType: MFT_OWNERDRAW, dwItemData: idx, ..Default::default() };
    let _ = SetMenuItemInfoW(menu, pos, true, &info);
}

unsafe fn build(entries: &[Entry], rows: &mut Vec<Row>) -> Option<HMENU> {
    let menu = CreatePopupMenu().ok()?;
    for e in entries {
        match e {
            Entry::Separator => {
                rows.push(Row { kind: Kind::Sep, text: String::new(), hint: String::new(), icon: '\0', checked: false, enabled: false });
                let _ = AppendMenuW(menu, MF_OWNERDRAW | MF_SEPARATOR, 0, PCWSTR((rows.len() - 1) as *const u16));
            }
            Entry::Item { id, text, checked, enabled, icon } => {
                let t = nv_text::tr(text).into_owned();
                let (label, hint) = match t.split_once('\t') {
                    Some((a, b)) => (a.to_string(), b.to_string()),
                    None => (t, String::new()),
                };
                let icon = if *icon == '\0' { icon_for(text) } else { *icon };
                let wide = system_text(&label, &hint);
                rows.push(Row { kind: Kind::Item, text: label, hint, icon, checked: *checked, enabled: *enabled });
                let mut flags = MENU_ITEM_FLAGS(0);
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                add_line(menu, flags, *id as usize, &wide, rows.len() - 1);
            }
            Entry::Submenu { text, items, enabled, icon } => {
                let t = nv_text::tr(text).into_owned();
                let icon = if *icon == '\0' { icon_for(text) } else { *icon };
                let wide = system_text(&t, "");
                rows.push(Row { kind: Kind::Sub, text: t, hint: String::new(), icon, checked: false, enabled: *enabled });
                let at = rows.len() - 1;
                if let Some(sub) = build(items, rows) {
                    let mut flags = MF_POPUP;
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    add_line(menu, flags, sub.0 as usize, &wide, at);
                }
            }
        }
    }
    Some(menu)
}
