#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! NuNote: the built-in notepad of the family. Opens a text, Markdown, JSON, HTML, JavaScript ... file, edits it, and shows it as coloured
//! source or (Markdown) as the rendered page. `nunote.exe [file] [--line N]`.
//!
//! The window is a plain Win32 window with a menu, two RichEdit controls (the text; the rendered Markdown, read-only) and a status bar.
//! Colouring is character formatting applied after a short pause in typing (`highlight.rs` decides what is a comment, a string ...); the
//! rendered page is RTF made by `markdown.rs`. Files keep their encoding and line ends (`textfile.rs`).

mod highlight;
mod markdown;
mod textfile;

use highlight::Lang;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use textfile::{Encoding, LineEnd};
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, DEFAULT_GUI_FONT, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, LoadLibraryW};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Controls::RichEdit::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;

const APP: &str = "NuNote";
const BG: u32 = markdown::BACKGROUND;
const TEXT: u32 = 0xD4D7DD;
const FONT: &str = "Consolas";

// ---- command ids ---------------------------------------------------------------------------------------------------------------
const C_NEW: u16 = 1001;
const C_OPEN: u16 = 1002;
const C_SAVE: u16 = 1003;
const C_SAVE_AS: u16 = 1004;
const C_EXIT: u16 = 1005;
const C_UNDO: u16 = 1010;
const C_REDO: u16 = 1011;
const C_CUT: u16 = 1012;
const C_COPY: u16 = 1013;
const C_PASTE: u16 = 1014;
const C_SELECT_ALL: u16 = 1015;
const C_FIND: u16 = 1016;
const C_FIND_NEXT: u16 = 1017;
const C_REPLACE: u16 = 1018;
const C_GOTO: u16 = 1019;
const C_WRAP: u16 = 1020;
const C_ZOOM_IN: u16 = 1021;
const C_ZOOM_OUT: u16 = 1022;
const C_ZOOM_RESET: u16 = 1023;
const C_VIEW_SOURCE: u16 = 1030;
const C_VIEW_RENDERED: u16 = 1031;
const C_VIEW_SPLIT: u16 = 1032;
const C_ENC_UTF8: u16 = 1040;
const C_ENC_UTF8BOM: u16 = 1041;
const C_ENC_UTF16LE: u16 = 1042;
const C_ENC_UTF16BE: u16 = 1043;
const C_ENC_ANSI: u16 = 1044;
const C_EOL_CRLF: u16 = 1050;
const C_EOL_LF: u16 = 1051;
const C_EOL_CR: u16 = 1052;
const C_LANG_AUTO: u16 = 1060;
const C_LANG_BASE: u16 = 1100; // + index into Lang::ALL
const C_ABOUT: u16 = 1090;
const ID_EDIT: u16 = 201;
const ID_PREVIEW: u16 = 202;
const ID_STATUS: u16 = 203;
const TIMER_COLOUR: usize = 1;

/// How the page is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Source,
    Rendered,
    Split,
}

struct State {
    hwnd: HWND,
    edit: HWND,
    preview: HWND,
    status: HWND,
    menu: HMENU,
    path: Option<PathBuf>,
    encoding: Encoding,
    line_end: LineEnd,
    lang: Lang,
    lang_auto: bool,
    view: View,
    wrap: bool,
    pt: i32,
    find: String,
    replace: String,
    match_case: bool,
    /// Text the colours were last made for (the colouring waits until typing pauses).
    coloured_len: Option<usize>,
}

thread_local! {
    static APP_STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    APP_STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb(c: u32) -> COLORREF {
    COLORREF((c & 0xFF) << 16 | (c >> 8 & 0xFF) << 8 | (c >> 16 & 0xFF))
}

fn send(h: HWND, msg: u32, w: usize, l: isize) -> isize {
    unsafe { SendMessageW(h, msg, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}

// ---- the text in and out of the control ----------------------------------------------------------------------------------------

/// The text of the edit control with every line end a single `\n`.
fn get_text(edit: HWND) -> String {
    let n = unsafe { GetWindowTextLengthW(edit) } as usize;
    let mut buf = vec![0u16; n + 2];
    let got = unsafe { GetWindowTextW(edit, &mut buf) } as usize;
    textfile::normalise(&String::from_utf16_lossy(&buf[..got.min(buf.len())]))
}

fn set_text(edit: HWND, text: &str) {
    // The control's own paragraph mark is a lone CR.
    let units: Vec<u16> = text.replace('\n', "\r").encode_utf16().chain(std::iter::once(0)).collect();
    let st = SETTEXTEX { flags: ST_DEFAULT, codepage: 1200 };
    send(edit, EM_SETTEXTEX, &st as *const SETTEXTEX as usize, units.as_ptr() as isize);
}

fn set_rtf(preview: HWND, rtf: &str) {
    let mut bytes: Vec<u8> = rtf.bytes().collect();
    bytes.push(0);
    let st = SETTEXTEX { flags: ST_DEFAULT, codepage: 1252 };
    send(preview, EM_SETTEXTEX, &st as *const SETTEXTEX as usize, bytes.as_ptr() as isize);
}

fn selection(edit: HWND) -> CHARRANGE {
    let mut r = CHARRANGE::default();
    send(edit, EM_EXGETSEL, 0, &mut r as *mut CHARRANGE as isize);
    r
}

fn set_selection(edit: HWND, r: CHARRANGE) {
    send(edit, EM_EXSETSEL, 0, &r as *const CHARRANGE as isize);
}

fn char_format(colour: u32, pt: i32, face: Option<&str>) -> CHARFORMAT2W {
    let mut cf = CHARFORMAT2W::default();
    cf.Base.cbSize = std::mem::size_of::<CHARFORMAT2W>() as u32;
    cf.Base.dwMask = CFM_COLOR | CFM_BOLD | CFM_ITALIC | CFM_UNDERLINE | CFM_STRIKEOUT;
    cf.Base.crTextColor = rgb(colour);
    if let Some(f) = face {
        cf.Base.dwMask = cf.Base.dwMask | CFM_FACE | CFM_SIZE;
        cf.Base.yHeight = pt * 20;
        for (i, u) in f.encode_utf16().take(31).enumerate() {
            cf.Base.szFaceName[i] = u;
        }
    }
    cf
}

fn apply_format(edit: HWND, scope: u32, cf: &CHARFORMAT2W) {
    send(edit, EM_SETCHARFORMAT, scope as usize, cf as *const CHARFORMAT2W as isize);
}

/// Turn undo recording off / on (text objects model), so the colouring is not something Ctrl+Z walks back through.
fn undo_recording(edit: HWND, on: bool) {
    unsafe {
        let mut ole: *mut std::ffi::c_void = std::ptr::null_mut();
        if send(edit, EM_GETOLEINTERFACE, 0, &mut ole as *mut _ as isize) == 0 || ole.is_null() {
            return;
        }
        use windows::core::Interface;
        let ole = IRichEditOle::from_raw(ole);
        if let Ok(doc) = ole.cast::<ITextDocument>() {
            let _ = doc.Undo(if on { tomResume.0 } else { tomSuspend.0 });
        }
    }
}

// ---- colouring -----------------------------------------------------------------------------------------------------------------

fn colour_now(s: &mut State) {
    let edit = s.edit;
    let text = get_text(edit);
    let spans = highlight::spans(&text, s.lang);
    let keep = selection(edit);
    // Colouring is not an edit: the document is as modified as it was.
    let was_modified = send(edit, EM_GETMODIFY, 0, 0);
    unsafe {
        let _ = SendMessageW(edit, WM_SETREDRAW, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    send(edit, EM_SETEVENTMASK, 0, 0);
    send(edit, EM_HIDESELECTION, 1, 0);
    undo_recording(edit, false);
    set_selection(edit, CHARRANGE { cpMin: 0, cpMax: -1 });
    apply_format(edit, SCF_SELECTION, &char_format(TEXT, s.pt, Some(FONT)));
    for sp in spans.iter().take(30_000) {
        set_selection(edit, CHARRANGE { cpMin: sp.start as i32, cpMax: (sp.start + sp.len) as i32 });
        apply_format(edit, SCF_SELECTION, &char_format(highlight::colour(sp.kind), 0, None));
    }
    undo_recording(edit, true);
    set_selection(edit, keep);
    send(edit, EM_SETMODIFY, (was_modified != 0) as usize, 0);
    send(edit, EM_HIDESELECTION, 0, 0);
    send(edit, EM_SETEVENTMASK, 0, (ENM_CHANGE | ENM_SELCHANGE) as isize);
    unsafe {
        let _ = SendMessageW(edit, WM_SETREDRAW, Some(WPARAM(1)), Some(LPARAM(0)));
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(edit), None, true);
    }
    s.coloured_len = Some(text.len());
}

fn render_now(s: &State) {
    if s.view == View::Source {
        return;
    }
    let text = get_text(s.edit);
    set_rtf(s.preview, &markdown::to_rtf(&text, s.pt as u32));
}

fn schedule_colour(hwnd: HWND) {
    unsafe {
        SetTimer(Some(hwnd), TIMER_COLOUR, 350, None);
    }
}

// ---- layout, menus, status ----------------------------------------------------------------------------------------------------------

fn layout(s: &State) {
    let mut rc = RECT::default();
    unsafe {
        let _ = GetClientRect(s.hwnd, &mut rc);
    }
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    let status_h = {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(s.status, &mut r);
        }
        (r.bottom - r.top).max(0)
    };
    let body = (h - status_h).max(0);
    unsafe {
        let _ = SetWindowPos(s.status, None, 0, body, w, status_h, SWP_NOZORDER);
        let (e, p) = match s.view {
            View::Source => ((0, w), (w, 0)),
            View::Rendered => ((0, 0), (0, w)),
            View::Split => ((0, w / 2), (w / 2, w - w / 2)),
        };
        // (x, width) of each; a zero width hides it.
        let place = |hwnd: HWND, x: i32, width: i32| {
            if width <= 0 {
                let _ = ShowWindow(hwnd, SW_HIDE);
            } else {
                let _ = SetWindowPos(hwnd, None, x, 0, width, body, SWP_NOZORDER | SWP_SHOWWINDOW);
            }
        };
        place(s.edit, e.0, e.1);
        place(s.preview, p.0, p.1);
    }
}

fn menu_text(s: &str) -> Vec<u16> {
    wide(s)
}

fn build_menu() -> HMENU {
    unsafe {
        let bar = CreateMenu().unwrap_or_default();
        let add = |m: HMENU, id: u16, text: &str| {
            let _ = AppendMenuW(m, MF_STRING, id as usize, PCWSTR(menu_text(text).as_ptr()));
        };
        let sep = |m: HMENU| {
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        };
        let sub = |bar: HMENU, m: HMENU, text: &str| {
            let _ = AppendMenuW(bar, MF_POPUP, m.0 as usize, PCWSTR(menu_text(text).as_ptr()));
        };
        let file = CreatePopupMenu().unwrap_or_default();
        add(file, C_NEW, "&New\tCtrl+N");
        add(file, C_OPEN, "&Open…\tCtrl+O");
        add(file, C_SAVE, "&Save\tCtrl+S");
        add(file, C_SAVE_AS, "Save &as…\tCtrl+Shift+S");
        sep(file);
        add(file, C_EXIT, "E&xit\tAlt+F4");
        sub(bar, file, "&File");
        let edit = CreatePopupMenu().unwrap_or_default();
        add(edit, C_UNDO, "&Undo\tCtrl+Z");
        add(edit, C_REDO, "&Redo\tCtrl+Y");
        sep(edit);
        add(edit, C_CUT, "Cu&t\tCtrl+X");
        add(edit, C_COPY, "&Copy\tCtrl+C");
        add(edit, C_PASTE, "&Paste\tCtrl+V");
        add(edit, C_SELECT_ALL, "Select &all\tCtrl+A");
        sep(edit);
        add(edit, C_FIND, "&Find…\tCtrl+F");
        add(edit, C_FIND_NEXT, "Find &next\tF3");
        add(edit, C_REPLACE, "R&eplace…\tCtrl+H");
        add(edit, C_GOTO, "&Go to line…\tCtrl+G");
        sub(bar, edit, "&Edit");
        let view = CreatePopupMenu().unwrap_or_default();
        add(view, C_VIEW_SOURCE, "&Source (coloured)\tCtrl+1");
        add(view, C_VIEW_RENDERED, "&Rendered Markdown\tCtrl+2");
        add(view, C_VIEW_SPLIT, "S&plit: source and rendered\tCtrl+3");
        sep(view);
        add(view, C_WRAP, "&Word wrap\tCtrl+W");
        add(view, C_ZOOM_IN, "Zoom &in\tCtrl++");
        add(view, C_ZOOM_OUT, "Zoom &out\tCtrl+-");
        add(view, C_ZOOM_RESET, "&Normal size\tCtrl+0");
        sub(bar, view, "&View");
        let lang = CreatePopupMenu().unwrap_or_default();
        add(lang, C_LANG_AUTO, "Automatic (by the file name)");
        sep(lang);
        for (i, l) in Lang::ALL.iter().enumerate() {
            add(lang, C_LANG_BASE + i as u16, l.label());
        }
        sub(bar, lang, "&Language");
        let fmt = CreatePopupMenu().unwrap_or_default();
        for (id, t) in [(C_ENC_UTF8, "UTF-8"), (C_ENC_UTF8BOM, "UTF-8 with BOM"), (C_ENC_UTF16LE, "UTF-16 LE"), (C_ENC_UTF16BE, "UTF-16 BE"), (C_ENC_ANSI, "ANSI (Windows-1252)")] {
            add(fmt, id, t);
        }
        sep(fmt);
        for (id, t) in [(C_EOL_CRLF, "Windows (CRLF)"), (C_EOL_LF, "Unix (LF)"), (C_EOL_CR, "Mac (CR)")] {
            add(fmt, id, t);
        }
        sub(bar, fmt, "F&ormat");
        let help = CreatePopupMenu().unwrap_or_default();
        add(help, C_ABOUT, "&About NuNote");
        sub(bar, help, "&Help");
        bar
    }
}

fn accelerators() -> HACCEL {
    let key = |ch: char, ctrl: bool, shift: bool, id: u16| ACCEL { fVirt: ACCEL_VIRT_FLAGS(FVIRTKEY.0 | if ctrl { FCONTROL.0 } else { 0 } | if shift { FSHIFT.0 } else { 0 }), key: ch as u16, cmd: id };
    let vk = |k: u16, id: u16| ACCEL { fVirt: FVIRTKEY, key: k, cmd: id };
    let list = [
        key('N', true, false, C_NEW),
        key('O', true, false, C_OPEN),
        key('S', true, false, C_SAVE),
        key('S', true, true, C_SAVE_AS),
        key('F', true, false, C_FIND),
        key('H', true, false, C_REPLACE),
        key('G', true, false, C_GOTO),
        key('W', true, false, C_WRAP),
        key('1', true, false, C_VIEW_SOURCE),
        key('2', true, false, C_VIEW_RENDERED),
        key('3', true, false, C_VIEW_SPLIT),
        key('0', true, false, C_ZOOM_RESET),
        ACCEL { fVirt: ACCEL_VIRT_FLAGS(FVIRTKEY.0 | FCONTROL.0), key: 0xBB, cmd: C_ZOOM_IN },
        ACCEL { fVirt: ACCEL_VIRT_FLAGS(FVIRTKEY.0 | FCONTROL.0), key: 0xBD, cmd: C_ZOOM_OUT },
        vk(0x72, C_FIND_NEXT), // F3
    ];
    unsafe { CreateAcceleratorTableW(&list).unwrap_or_default() }
}

fn update_menu_checks(s: &State) {
    unsafe {
        let check = |id: u16, on: bool| {
            let _ = CheckMenuItem(s.menu, id as u32, (MF_BYCOMMAND.0 | if on { MF_CHECKED.0 } else { MF_UNCHECKED.0 }) as u32);
        };
        check(C_VIEW_SOURCE, s.view == View::Source);
        check(C_VIEW_RENDERED, s.view == View::Rendered);
        check(C_VIEW_SPLIT, s.view == View::Split);
        check(C_WRAP, s.wrap);
        check(C_LANG_AUTO, s.lang_auto);
        for (i, l) in Lang::ALL.iter().enumerate() {
            check(C_LANG_BASE + i as u16, !s.lang_auto && s.lang == *l);
        }
        for (id, e) in [(C_ENC_UTF8, Encoding::Utf8), (C_ENC_UTF8BOM, Encoding::Utf8Bom), (C_ENC_UTF16LE, Encoding::Utf16Le), (C_ENC_UTF16BE, Encoding::Utf16Be), (C_ENC_ANSI, Encoding::Ansi)] {
            check(id, s.encoding == e);
        }
        for (id, e) in [(C_EOL_CRLF, LineEnd::Crlf), (C_EOL_LF, LineEnd::Lf), (C_EOL_CR, LineEnd::Cr)] {
            check(id, s.line_end == e);
        }
        let _ = DrawMenuBar(s.hwnd);
    }
}

fn title(s: &State) -> String {
    let name = s.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    let dirty = send(s.edit, EM_GETMODIFY, 0, 0) != 0;
    format!("{}{name} - {APP}", if dirty { "*" } else { "" })
}

fn update_status(s: &State) {
    let sel = selection(s.edit);
    let line = send(s.edit, EM_LINEFROMCHAR, sel.cpMin as usize, 0) as i32;
    let col = sel.cpMin - send(s.edit, EM_LINEINDEX, line as usize, 0) as i32;
    let lines = send(s.edit, EM_GETLINECOUNT, 0, 0);
    let chosen = if sel.cpMax > sel.cpMin { format!("   ({} selected)", sel.cpMax - sel.cpMin) } else { String::new() };
    let parts = [format!("Ln {}, Col {}{chosen}   of {lines} lines", line + 1, col + 1), format!("{}   {}", s.encoding.label(), s.line_end.label()), format!("{}   {} pt", s.lang.label(), s.pt)];
    unsafe {
        let widths = [0i32.max(0)];
        let _ = widths;
        let mut rc = RECT::default();
        let _ = GetClientRect(s.hwnd, &mut rc);
        let w = rc.right;
        let edges = [w * 55 / 100, w * 80 / 100, -1i32];
        send(s.status, SB_SETPARTS, 3, edges.as_ptr() as isize);
        for (i, p) in parts.iter().enumerate() {
            let t = wide(p);
            send(s.status, SB_SETTEXTW, i, t.as_ptr() as isize);
        }
        let t = wide(&title(s));
        let _ = SetWindowTextW(s.hwnd, PCWSTR(t.as_ptr()));
    }
}

// ---- documents -----------------------------------------------------------------------------------------------------------------

fn auto_lang(path: &Path) -> Lang {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().trim_start_matches('.').to_ascii_lowercase()).unwrap_or_default());
    highlight::lang_of_ext(&ext)
}

fn message(hwnd: HWND, text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let (t, c) = (wide(text), wide(APP));
    unsafe { MessageBoxW(Some(hwnd), PCWSTR(t.as_ptr()), PCWSTR(c.as_ptr()), flags) }
}

fn load_into(s: &mut State, path: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > 64 << 20 {
        return Err(format!("{} is {:.0} MB: NuNote opens text files up to 64 MB.", path.display(), meta.len() as f64 / 1_048_576.0));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let loaded = textfile::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    s.path = Some(path.to_path_buf());
    s.encoding = loaded.encoding;
    s.line_end = loaded.line_end;
    if s.lang_auto {
        s.lang = auto_lang(path);
    }
    // Markdown opens in the rendered page next to the source; everything else as source.
    s.view = if s.lang == Lang::Markdown { View::Split } else { View::Source };
    set_text(s.edit, &loaded.text);
    send(s.edit, EM_SETMODIFY, 0, 0);
    send(s.edit, EM_EMPTYUNDOBUFFER, 0, 0);
    set_selection(s.edit, CHARRANGE { cpMin: 0, cpMax: 0 });
    layout(s);
    colour_now(s);
    render_now(s);
    send(s.edit, EM_SETMODIFY, 0, 0);
    update_menu_checks(s);
    update_status(s);
    Ok(())
}

fn new_doc(s: &mut State) {
    s.path = None;
    s.encoding = Encoding::Utf8;
    s.line_end = LineEnd::Crlf;
    if s.lang_auto {
        s.lang = Lang::Plain;
    }
    s.view = View::Source;
    set_text(s.edit, "");
    send(s.edit, EM_SETMODIFY, 0, 0);
    send(s.edit, EM_EMPTYUNDOBUFFER, 0, 0);
    layout(s);
    colour_now(s);
    update_menu_checks(s);
    update_status(s);
}

fn write_file(s: &mut State, path: &Path) -> Result<(), String> {
    let text = get_text(s.edit);
    let bytes = match textfile::encode(&text, s.encoding, s.line_end) {
        Ok(b) => b,
        Err(e) => {
            // The ANSI code page cannot hold it: say so and use UTF-8 instead of losing characters.
            if message(s.hwnd, &format!("{e}.\n\nSave as UTF-8 instead?"), MB_YESNO | MB_ICONWARNING) != IDYES {
                return Err("not saved".into());
            }
            s.encoding = Encoding::Utf8;
            textfile::encode(&text, s.encoding, s.line_end)?
        }
    };
    // Written beside the file and moved over it, so a failed write never leaves half a file.
    let tmp = path.with_extension(format!("{}.nunote-tmp", path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }
    s.path = Some(path.to_path_buf());
    send(s.edit, EM_SETMODIFY, 0, 0);
    if s.lang_auto {
        let l = auto_lang(path);
        if l != s.lang {
            s.lang = l;
            colour_now(s);
        }
    }
    update_status(s);
    Ok(())
}

fn file_dialog(hwnd: HWND, save: bool, initial: Option<&Path>) -> Option<PathBuf> {
    let mut buf = vec![0u16; 2048];
    if let Some(p) = initial.and_then(|p| p.file_name()) {
        for (i, u) in p.to_string_lossy().encode_utf16().take(2000).enumerate() {
            buf[i] = u;
        }
    }
    let exts: Vec<String> = nv_core::textfiles::EXTS.iter().map(|e| format!("*.{e}")).collect();
    let filter = format!("Text, Markdown, data and source files\0{}\0All files\0*.*\0\0", exts.join(";"));
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let mut ofn = OPENFILENAMEW { lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32, hwndOwner: hwnd, lpstrFilter: PCWSTR(filter.as_ptr()), lpstrFile: PWSTR(buf.as_mut_ptr()), nMaxFile: buf.len() as u32, Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST }, ..Default::default() };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn).as_bool() } else { GetOpenFileNameW(&mut ofn).as_bool() } };
    ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])))
}

/// Ask what to do with unsaved changes. True when it is fine to go on (saved, or the user says drop them).
fn settle_changes(s: &mut State) -> bool {
    if send(s.edit, EM_GETMODIFY, 0, 0) == 0 {
        return true;
    }
    let name = s.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    match message(s.hwnd, &format!("Save the changes to {name}?"), MB_YESNOCANCEL | MB_ICONQUESTION) {
        IDYES => save(s, false),
        IDNO => true,
        _ => false,
    }
}

fn save(s: &mut State, as_new: bool) -> bool {
    let path = match (&s.path, as_new) {
        (Some(p), false) => p.clone(),
        _ => match file_dialog(s.hwnd, true, s.path.as_deref()) {
            Some(p) => p,
            None => return false,
        },
    };
    match write_file(s, &path) {
        Ok(()) => true,
        Err(e) => {
            if e != "not saved" {
                message(s.hwnd, &e, MB_OK | MB_ICONERROR);
            }
            false
        }
    }
}

// ---- find / replace / go to ------------------------------------------------------------------------------------------------------

fn find_next(s: &mut State, forward: bool) -> bool {
    if s.find.is_empty() {
        return false;
    }
    let text = wide(&s.find);
    let sel = selection(s.edit);
    let mut ft = FINDTEXTEXW { chrg: if forward { CHARRANGE { cpMin: sel.cpMax, cpMax: -1 } } else { CHARRANGE { cpMin: sel.cpMin, cpMax: 0 } }, lpstrText: PCWSTR(text.as_ptr()), chrgText: CHARRANGE::default() };
    let flags = (if forward { 1 } else { 0 }) | if s.match_case { 4 } else { 0 };
    let mut found = send(s.edit, EM_FINDTEXTEXW, flags, &mut ft as *mut FINDTEXTEXW as isize);
    if found < 0 {
        // Wrap round to the other end.
        ft.chrg = if forward { CHARRANGE { cpMin: 0, cpMax: -1 } } else { CHARRANGE { cpMin: -1, cpMax: 0 } };
        found = send(s.edit, EM_FINDTEXTEXW, flags, &mut ft as *mut FINDTEXTEXW as isize);
    }
    if found >= 0 {
        let r = ft.chrgText;
        set_selection(s.edit, r);
        send(s.edit, EM_SCROLLCARET, 0, 0);
        update_status(s);
        true
    } else {
        false
    }
}

fn replace_selection(s: &State, with_text: &str) {
    let t = wide(with_text);
    send(s.edit, EM_REPLACESEL, 1, t.as_ptr() as isize);
}

fn find_dialog(replace: bool) {
    use nv_win::ui::{Form, ID_CANCEL};
    const ID_WHAT: u16 = 301;
    const ID_WITH: u16 = 302;
    const ID_CASE: u16 = 303;
    const ID_NEXT: u16 = 304;
    const ID_PREV: u16 = 305;
    const ID_REPL: u16 = 306;
    const ID_ALL: u16 = 307;
    const ID_MSG: u16 = 308;
    let (what, with_text, case) = with(|s| (s.find.clone(), s.replace.clone(), s.match_case)).unwrap_or_default();
    let h = if replace { 190 } else { 150 };
    let Ok(form) = Form::new(if replace { "Replace" } else { "Find" }, 430, h) else { return };
    form.label(14, 16, 80, 20, "Find what:");
    form.edit(ID_WHAT, 100, 12, 310, 24, &what);
    let mut y = 46;
    if replace {
        form.label(14, 50, 80, 20, "Replace with:");
        form.edit(ID_WITH, 100, 46, 310, 24, &with_text);
        y = 80;
    }
    form.check(ID_CASE, 100, y, 200, 22, "Match case", case);
    form.label_id(ID_MSG, 14, y + 28, 396, 20, "");
    let by = y + 52;
    form.button(ID_PREV, 14, by, 90, 28, "Previous", false);
    form.button(ID_NEXT, 110, by, 90, 28, "Find next", true);
    if replace {
        form.button(ID_REPL, 206, by, 90, 28, "Replace", false);
        form.button(ID_ALL, 302, by, 108, 28, "Replace all", false);
    } else {
        form.button(ID_CANCEL, 320, by, 90, 28, "Close", false);
    }
    form.run(&mut |f, id| {
        let remember = |f: &Form| {
            let (w, r, c) = (f.text(ID_WHAT), if replace { f.text(ID_WITH) } else { String::new() }, f.is_checked(ID_CASE));
            with(|s| {
                s.find = w;
                if replace {
                    s.replace = r;
                }
                s.match_case = c;
            });
        };
        match id {
            ID_NEXT | ID_PREV => {
                remember(f);
                let ok = with(|s| find_next(s, id == ID_NEXT)).unwrap_or(false);
                f.set_text(ID_MSG, if ok { "" } else { "Not found." });
                false
            }
            ID_REPL => {
                remember(f);
                with(|s| {
                    // Replace what is selected if it is the search text, then go to the next one.
                    let sel = selection(s.edit);
                    let chosen = if sel.cpMax > sel.cpMin { selected_text(s.edit, sel) } else { String::new() };
                    let same = if s.match_case { chosen == s.find } else { chosen.to_lowercase() == s.find.to_lowercase() };
                    if same {
                        let r = s.replace.clone();
                        replace_selection(s, &r);
                    }
                    let ok = find_next(s, true);
                    f.set_text(ID_MSG, if ok { "" } else { "No more matches." });
                });
                false
            }
            ID_ALL => {
                remember(f);
                let n = with(|s| replace_all(s)).unwrap_or(0);
                f.set_text(ID_MSG, &format!("{n} replaced."));
                false
            }
            ID_CANCEL => {
                remember(f);
                true
            }
            _ => false,
        }
    });
}

fn selected_text(edit: HWND, r: CHARRANGE) -> String {
    let text: Vec<u16> = get_text(edit).replace('\n', "\r").encode_utf16().collect();
    let (a, b) = (r.cpMin.max(0) as usize, (r.cpMax.max(0) as usize).min(text.len()));
    String::from_utf16_lossy(&text[a.min(b)..b]).replace('\r', "\n")
}

/// Replace every match, keeping the case rule; returns how many. One undo step.
fn replace_all(s: &mut State) -> usize {
    if s.find.is_empty() {
        return 0;
    }
    let text = get_text(s.edit);
    let (new_text, n) = replace_text(&text, &s.find, &s.replace, s.match_case);
    if n > 0 {
        let keep = selection(s.edit);
        set_selection(s.edit, CHARRANGE { cpMin: 0, cpMax: -1 });
        let t = wide(&new_text.replace('\n', "\r"));
        send(s.edit, EM_REPLACESEL, 1, t.as_ptr() as isize);
        set_selection(s.edit, CHARRANGE { cpMin: keep.cpMin.min(new_text.len() as i32), cpMax: keep.cpMin.min(new_text.len() as i32) });
        colour_now(s);
    }
    n
}

/// `text` with every `find` replaced by `with`; (result, count). Case-insensitive unless `match_case`.
fn replace_text(text: &str, find: &str, with: &str, match_case: bool) -> (String, usize) {
    if find.is_empty() {
        return (text.to_string(), 0);
    }
    let (hay, needle): (Vec<char>, Vec<char>) = if match_case { (text.chars().collect(), find.chars().collect()) } else { (text.chars().flat_map(|c| c.to_lowercase()).collect(), find.chars().flat_map(|c| c.to_lowercase()).collect()) };
    let orig: Vec<char> = text.chars().collect();
    // Lower-casing can change a text's length (rare: some Unicode letters); then the positions would not line up: fall back to a case-sensitive pass.
    if hay.len() != orig.len() && !match_case {
        return replace_text(text, find, with, true);
    }
    let mut out = String::with_capacity(text.len());
    let (mut i, mut n) = (0, 0);
    while i < orig.len() {
        if i + needle.len() <= hay.len() && hay[i..i + needle.len()] == needle[..] {
            out.push_str(with);
            i += needle.len();
            n += 1;
        } else {
            out.push(orig[i]);
            i += 1;
        }
    }
    (out, n)
}

fn goto_line() {
    use nv_win::ui::{Form, ID_CANCEL, ID_OK};
    const ID_N: u16 = 401;
    let Ok(form) = Form::new("Go to line", 300, 120) else { return };
    form.label(14, 16, 130, 20, "Line number:");
    form.edit(ID_N, 150, 12, 120, 24, "1");
    form.button(ID_OK, 100, 56, 84, 28, "Go", true);
    form.button(ID_CANCEL, 190, 56, 84, 28, "Cancel", false);
    let mut want: Option<usize> = None;
    form.run(&mut |f, id| match id {
        ID_OK => {
            want = f.text(ID_N).trim().parse().ok();
            true
        }
        ID_CANCEL => true,
        _ => false,
    });
    if let Some(n) = want {
        with(|s| {
            let lines = send(s.edit, EM_GETLINECOUNT, 0, 0).max(1) as usize;
            let line = n.clamp(1, lines) - 1;
            let at = send(s.edit, EM_LINEINDEX, line, 0) as i32;
            set_selection(s.edit, CHARRANGE { cpMin: at, cpMax: at });
            send(s.edit, EM_SCROLLCARET, 0, 0);
            unsafe {
                let _ = SetFocus(Some(s.edit));
            }
            update_status(s);
        });
    }
}

// ---- commands ------------------------------------------------------------------------------------------------------------------

fn set_view(s: &mut State, v: View) {
    s.view = v;
    layout(s);
    render_now(s);
    update_menu_checks(s);
}

fn apply_font(s: &mut State) {
    s.coloured_len = None;
    colour_now(s);
    render_now(s);
    update_status(s);
}

fn open_path(s: &mut State, path: &Path) {
    if !settle_changes(s) {
        return;
    }
    if let Err(e) = load_into(s, path) {
        message(s.hwnd, &e, MB_OK | MB_ICONERROR);
    }
}

fn command(id: u16) {
    match id {
        C_FIND => return find_dialog(false),
        C_REPLACE => return find_dialog(true),
        C_GOTO => return goto_line(),
        _ => {}
    }
    with(|s| match id {
        C_NEW => {
            if settle_changes(s) {
                new_doc(s);
            }
        }
        C_OPEN => {
            if let Some(p) = file_dialog(s.hwnd, false, None) {
                open_path(s, &p);
            }
        }
        C_SAVE => {
            save(s, false);
        }
        C_SAVE_AS => {
            save(s, true);
        }
        C_EXIT => unsafe {
            let _ = PostMessageW(Some(s.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        C_UNDO => {
            send(s.edit, EM_UNDO, 0, 0);
        }
        C_REDO => {
            send(s.edit, EM_REDO, 0, 0);
        }
        C_CUT => {
            send(s.edit, WM_CUT, 0, 0);
        }
        C_COPY => {
            send(if s.view == View::Rendered { s.preview } else { s.edit }, WM_COPY, 0, 0);
        }
        C_PASTE => {
            send(s.edit, WM_PASTE, 0, 0);
        }
        C_SELECT_ALL => {
            send(if s.view == View::Rendered { s.preview } else { s.edit }, EM_SETSEL, 0, -1);
        }
        C_FIND_NEXT => {
            if s.find.is_empty() {
                // Nothing searched yet: F3 opens the box.
                return;
            }
            find_next(s, true);
        }
        C_WRAP => {
            s.wrap = !s.wrap;
            // A target device width of 0 wraps at the window's edge; a huge one turns wrapping off.
            send(s.edit, EM_SETTARGETDEVICE, 0, if s.wrap { 0 } else { 1 });
            send(s.preview, EM_SETTARGETDEVICE, 0, 0);
            update_menu_checks(s);
        }
        C_ZOOM_IN => {
            s.pt = (s.pt + 1).min(48);
            apply_font(s);
        }
        C_ZOOM_OUT => {
            s.pt = (s.pt - 1).max(7);
            apply_font(s);
        }
        C_ZOOM_RESET => {
            s.pt = 11;
            apply_font(s);
        }
        C_VIEW_SOURCE => set_view(s, View::Source),
        C_VIEW_RENDERED => set_view(s, View::Rendered),
        C_VIEW_SPLIT => set_view(s, View::Split),
        C_ENC_UTF8 | C_ENC_UTF8BOM | C_ENC_UTF16LE | C_ENC_UTF16BE | C_ENC_ANSI => {
            s.encoding = match id {
                C_ENC_UTF8 => Encoding::Utf8,
                C_ENC_UTF8BOM => Encoding::Utf8Bom,
                C_ENC_UTF16LE => Encoding::Utf16Le,
                C_ENC_UTF16BE => Encoding::Utf16Be,
                _ => Encoding::Ansi,
            };
            send(s.edit, EM_SETMODIFY, 1, 0); // the file will be written differently
            update_menu_checks(s);
            update_status(s);
        }
        C_EOL_CRLF | C_EOL_LF | C_EOL_CR => {
            s.line_end = match id {
                C_EOL_CRLF => LineEnd::Crlf,
                C_EOL_LF => LineEnd::Lf,
                _ => LineEnd::Cr,
            };
            send(s.edit, EM_SETMODIFY, 1, 0);
            update_menu_checks(s);
            update_status(s);
        }
        C_LANG_AUTO => {
            s.lang_auto = true;
            s.lang = s.path.as_deref().map(auto_lang).unwrap_or(Lang::Plain);
            colour_now(s);
            update_menu_checks(s);
            update_status(s);
        }
        C_ABOUT => {
            message(s.hwnd, &format!("{APP} {}\n\nThe notepad of the NuViewer family: text, Markdown, JSON, HTML, JavaScript and more, as coloured source or rendered Markdown.\n\n{}", env!("CARGO_PKG_VERSION"), nv_core::about::WEBSITE), MB_OK | MB_ICONINFORMATION);
        }
        n if (C_LANG_BASE..C_LANG_BASE + Lang::ALL.len() as u16).contains(&n) => {
            s.lang = Lang::ALL[(n - C_LANG_BASE) as usize];
            s.lang_auto = false;
            if s.lang == Lang::Markdown && s.view == View::Source {
                s.view = View::Split;
                layout(s);
            }
            colour_now(s);
            render_now(s);
            update_menu_checks(s);
            update_status(s);
        }
        _ => {}
    });
}

// ---- the window ----------------------------------------------------------------------------------------------------------------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            with(|s| {
                send(s.status, WM_SIZE, 0, 0);
                layout(s);
                update_status(s);
            });
            LRESULT(0)
        }
        WM_SETFOCUS => {
            with(|s| {
                let _ = SetFocus(Some(if s.view == View::Rendered { s.preview } else { s.edit }));
            });
            LRESULT(0)
        }
        WM_COMMAND => {
            let (id, notify) = ((wp.0 & 0xFFFF) as u16, ((wp.0 >> 16) & 0xFFFF) as u32);
            if id == ID_EDIT && notify == EN_CHANGE as u32 {
                with(|s| {
                    s.coloured_len = None;
                    update_status(s);
                });
                schedule_colour(hwnd);
            } else if id != ID_EDIT && id != ID_PREVIEW {
                command(id);
            }
            LRESULT(0)
        }
        WM_NOTIFY => {
            let hdr = &*(lp.0 as *const NMHDR);
            if hdr.idFrom == ID_EDIT as usize && hdr.code == EN_SELCHANGE {
                with(|s| update_status(s));
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wp.0 == TIMER_COLOUR {
                let _ = KillTimer(Some(hwnd), TIMER_COLOUR);
                with(|s| {
                    if s.coloured_len.is_none() {
                        colour_now(s);
                        render_now(s);
                        update_status(s);
                    }
                });
            }
            LRESULT(0)
        }
        WM_DROPFILES => {
            let drop = HDROP(wp.0 as *mut _);
            let n = DragQueryFileW(drop, 0, None);
            if n > 0 {
                let mut buf = vec![0u16; n as usize + 1];
                let got = DragQueryFileW(drop, 0, Some(&mut buf)) as usize;
                let path = PathBuf::from(String::from_utf16_lossy(&buf[..got]));
                if path.is_file() {
                    with(|s| open_path(s, &path));
                }
            }
            DragFinish(drop);
            LRESULT(0)
        }
        WM_MOUSEWHEEL if (wp.0 & 0x0008) != 0 => {
            // Ctrl + wheel zooms.
            let up = ((wp.0 >> 16) as i16) > 0;
            command(if up { C_ZOOM_IN } else { C_ZOOM_OUT });
            LRESULT(0)
        }
        WM_CLOSE => {
            let ok = with(|s| settle_changes(s)).unwrap_or(true);
            if ok {
                with(|s| remember_window(s.hwnd));
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

fn remember_window(hwnd: HWND) {
    nv_win::winpos::save(APP, hwnd);
}

fn main() {
    nv_win::portable::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut file: Option<PathBuf> = None;
    let mut line: Option<usize> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--line" => line = it.next().and_then(|v| v.parse().ok()),
            s if !s.starts_with("--") && file.is_none() => file = Some(nv_win::instance::absolute(Path::new(s))),
            _ => {}
        }
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = LoadLibraryW(w!("Msftedit.dll"));
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let (icon, icon_small) = nv_win::app_icons();
        let class = w!("NuNoteWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hIcon: icon,
            hIconSm: icon_small,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(GetStockObject(windows::Win32::Graphics::Gdi::BLACK_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let menu = build_menu();
        let (x, y, w, h) = nv_win::winpos::place(APP, 1100, 760);
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("NuNote"), WS_OVERLAPPEDWINDOW, x, y, w, h, None, Some(menu), Some(hinst.into()), None) else { return };
        let rich = |id: u16, readonly: bool| {
            let style = WS_CHILD.0 | WS_VISIBLE.0 | WS_VSCROLL.0 | WS_HSCROLL.0 | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_AUTOHSCROLL as u32 | ES_NOHIDESEL as u32 | if readonly { ES_READONLY as u32 } else { ES_WANTRETURN as u32 };
            CreateWindowExW(WINDOW_EX_STYLE(0), MSFTEDIT_CLASS, PCWSTR::null(), WINDOW_STYLE(style), 0, 0, 10, 10, Some(hwnd), Some(HMENU(id as usize as *mut _)), Some(hinst.into()), None).unwrap_or_default()
        };
        let edit = rich(ID_EDIT, false);
        let preview = rich(ID_PREVIEW, true);
        let status = CreateWindowExW(WINDOW_EX_STYLE(0), STATUSCLASSNAMEW, PCWSTR::null(), WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | SBARS_SIZEGRIP as u32), 0, 0, 0, 0, Some(hwnd), Some(HMENU(ID_STATUS as usize as *mut _)), Some(hinst.into()), None).unwrap_or_default();
        for r in [edit, preview] {
            send(r, EM_SETBKGNDCOLOR, 0, rgb(BG).0 as isize);
            send(r, EM_SETLIMITTEXT, 0, 0x7FFF_FFF0);
            send(r, EM_SETMARGINS, (EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize, (14 | 14 << 16) as isize);
            let _ = SendMessageW(r, WM_SETFONT, Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)), Some(LPARAM(0)));
        }
        let _: HFONT = HFONT(GetStockObject(DEFAULT_GUI_FONT).0);
        send(edit, EM_SETEVENTMASK, 0, (ENM_CHANGE | ENM_SELCHANGE) as isize);
        send(edit, EM_SETTARGETDEVICE, 0, 1); // no wrapping to begin with
        let _ = dpi_scale(hwnd);
        DragAcceptFiles(hwnd, true);
        nv_win::set_main_window(hwnd);
        APP_STATE.with(|s| {
            *s.borrow_mut() = Some(State { hwnd, edit, preview, status, menu, path: None, encoding: Encoding::Utf8, line_end: LineEnd::Crlf, lang: Lang::Plain, lang_auto: true, view: View::Source, wrap: false, pt: 11, find: String::new(), replace: String::new(), match_case: false, coloured_len: None });
        });
        with(|s| {
            let cf = char_format(TEXT, s.pt, Some(FONT));
            apply_format(edit, SCF_ALL, &cf);
            layout(s);
            match &file {
                Some(p) => {
                    if let Err(e) = load_into(s, p) {
                        message(hwnd, &e, MB_OK | MB_ICONERROR);
                        new_doc(s);
                    }
                }
                None => new_doc(s),
            }
            if let Some(n) = line {
                let at = send(s.edit, EM_LINEINDEX, n.max(1) - 1, 0) as i32;
                set_selection(s.edit, CHARRANGE { cpMin: at, cpMax: at });
                send(s.edit, EM_SCROLLCARET, 0, 0);
            }
            update_menu_checks(s);
            update_status(s);
        });
        nv_win::winpos::show(APP, hwnd);
        let _ = SetFocus(Some(edit));
        let accel = accelerators();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Esc in the rendered page does nothing; everything else goes through the accelerators first.
            if msg.message == WM_KEYDOWN && msg.wParam.0 as u16 == VK_ESCAPE.0 {
                continue;
            }
            if TranslateAcceleratorW(hwnd, accel, &msg) == 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

fn dpi_scale(hwnd: HWND) -> f32 {
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d == 0 { 1.0 } else { d as f32 / 96.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_follows_the_case_rule() {
        assert_eq!(replace_text("Cat cat CAT", "cat", "dog", false), ("dog dog dog".to_string(), 3));
        assert_eq!(replace_text("Cat cat CAT", "cat", "dog", true), ("Cat dog CAT".to_string(), 1));
        assert_eq!(replace_text("aaa", "aa", "b", true), ("ba".to_string(), 1), "matches do not overlap");
        assert_eq!(replace_text("x", "", "y", true), ("x".to_string(), 0));
        assert_eq!(replace_text("วันนี้ วันนี้", "วัน", "คืน", false), ("คืนนี้ คืนนี้".to_string(), 2));
    }

    #[test]
    fn the_language_follows_the_file_name() {
        assert_eq!(auto_lang(Path::new("a/b/readme.md")), Lang::Markdown);
        assert_eq!(auto_lang(Path::new("x.JSON")), Lang::Json);
        assert_eq!(auto_lang(Path::new("script.ps1")), Lang::PowerShell);
        assert_eq!(auto_lang(Path::new(".gitignore")), Lang::Plain);
        assert_eq!(auto_lang(Path::new("notes")), Lang::Plain);
    }
}
