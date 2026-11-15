//! Decoding a picture to a screen-sized BGRA block (worker thread + small cache) and the borderless preview window.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Is the preview window up? (Read by the keyboard hook thread, so an atomic; written by `show` / `hide`.)
pub static VISIBLE: AtomicBool = AtomicBool::new(false);
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Message the worker posts to the agent window when something finished decoding.
pub const WM_DECODED: u32 = WM_APP + 5;

/// Formats shown by Space. Everything here decodes in well under 100 ms for a normal file; documents, 3D models and
/// archives are left to the full viewer (a wrong guess would make Space feel slow, which is the one thing it must not).
const EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "bmp", "dib", "ico", "tif", "tiff", "avif", "heic", "heif", "hif", "jxl", "jp2", "j2k", "psd", "psb", "tga", "dds", "svg", "qoi", "ppm", "pgm", "pnm", "hdr", "exr", "pcx",
];

pub fn previewable(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return false };
    let ext = ext.to_ascii_lowercase();
    EXTS.contains(&ext.as_str()) || nv_codecs::is_raw_ext(&ext)
}

pub struct Preview {
    pub w: i32,
    pub h: i32,
    pub bgra: Vec<u8>,
    pub name: String,
    pub info: String,
    pub decode_ms: f64,
    /// A big word in the middle when a card has no icon: "FOLDER", "DOCX".
    pub big: Option<String>,
    /// An info card's facts (label, value), drawn from `row_top` down, `row_h` apart.
    pub rows: Vec<(String, String)>,
    pub row_top: i32,
    pub row_h: i32,
}

#[derive(Clone, PartialEq)]
struct Key {
    path: PathBuf,
    mtime: u64,
    len: u64,
    maxw: u32,
    maxh: u32,
}

fn key_of(path: &Path, maxw: u32, maxh: u32) -> Key {
    let md = std::fs::metadata(path).ok();
    let mtime = md.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
    Key { path: path.to_path_buf(), mtime, len: md.map(|m| m.len()).unwrap_or(0), maxw, maxh }
}

// ---------------------------------------------------------------------------------------------------------------------
// decoding

pub fn human_size(b: u64) -> String {
    if b >= 1 << 20 { format!("{:.1} MB", b as f64 / (1 << 20) as f64) } else { format!("{} KB", (b + 1023) >> 10) }
}

/// Decode `path` and fit it into `maxw` x `maxh` (never enlarged). Orientation and colour are applied on the small copy.
fn make_preview(path: &Path, maxw: u32, maxh: u32) -> std::result::Result<Preview, String> {
    let t0 = std::time::Instant::now();
    let img = nv_codecs::decode_file_first_frame(path)?;
    let (dw, dh) = img.display_size();
    let s = (maxw as f32 / dw as f32).min(maxh as f32 / dh as f32).min(1.0);
    let edge = ((img.width.max(img.height) as f32) * s).round().max(1.0) as u32;
    let mut small = if s < 1.0 { nv_codecs::downscale(&img, edge) } else { img };
    small.apply_display_colour();
    let small = small.oriented();
    // Flatten transparency over a dark neutral.
    const BG: u32 = 0x2A;
    let mut bgra = Vec::with_capacity(small.rgba.len());
    for p in small.rgba.chunks_exact(4) {
        let a = p[3] as u32;
        if a == 255 {
            bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
        } else {
            let mix = |c: u8| ((c as u32 * a + BG * (255 - a) + 127) / 255) as u8;
            bgra.extend_from_slice(&[mix(p[2]), mix(p[1]), mix(p[0]), 255]);
        }
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_uppercase()).unwrap_or_default();
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let info = format!("{ext}   {dw} \u{d7} {dh}   {}", human_size(bytes));
    Ok(Preview { w: small.width as i32, h: small.height as i32, bgra, name, info, decode_ms: t0.elapsed().as_secs_f64() * 1000.0, big: None, rows: Vec::new(), row_top: 0, row_h: 0 })
}

// ---------------------------------------------------------------------------------------------------------------------
// worker thread + cache

struct Req {
    path: PathBuf,
    maxw: u32,
    maxh: u32,
}

struct Shared {
    slot: Mutex<Option<Req>>,
    wake: Condvar,
    cache: Mutex<Vec<(Key, Arc<Preview>)>>,
    inflight: Mutex<Option<Key>>,
    agent: isize,
}

static SHARED: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

fn shared() -> Option<Arc<Shared>> {
    SHARED.lock().ok()?.clone()
}

const CACHE_ENTRIES: usize = 4;

pub fn start_worker(agent: HWND) {
    let sh = Arc::new(Shared { slot: Mutex::new(None), wake: Condvar::new(), cache: Mutex::new(Vec::new()), inflight: Mutex::new(None), agent: agent.0 as isize });
    *SHARED.lock().unwrap() = Some(sh.clone());
    std::thread::Builder::new()
        .name("nuquick-decode".into())
        .spawn(move || {
          // The shell makes thumbnails and icons for the info cards: it needs COM on this thread.
          unsafe {
              let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
          }
          loop {
            let req = {
                let mut g = sh.slot.lock().unwrap();
                while g.is_none() {
                    g = sh.wake.wait(g).unwrap();
                }
                g.take().unwrap()
            };
            let key = key_of(&req.path, req.maxw, req.maxh);
            if sh.cache.lock().unwrap().iter().any(|(k, _)| *k == key) {
                continue;
            }
            *sh.inflight.lock().unwrap() = Some(key.clone());
            let r = std::panic::catch_unwind(|| if previewable(&req.path) { make_preview(&req.path, req.maxw, req.maxh) } else { Ok(crate::card::build(&req.path, None)) });
            *sh.inflight.lock().unwrap() = None;
            match r {
                Ok(Ok(p)) => {
                    let mut c = sh.cache.lock().unwrap();
                    c.retain(|(k, _)| k.path != key.path);
                    c.push((key, Arc::new(p)));
                    while c.len() > CACHE_ENTRIES {
                        c.remove(0);
                    }
                }
                Ok(Err(e)) => {
                    crate::log(&format!("decode failed: {e}"));
                    // A card, so the preview does not wait for a picture that will never come.
                    let mut c = sh.cache.lock().unwrap();
                    c.retain(|(k, _)| k.path != key.path);
                    c.push((key, Arc::new(crate::card::build(&req.path, Some("Can't show this picture")))));
                }
                Err(_) => crate::log("decode panicked"),
            }
            unsafe {
                let _ = PostMessageW(Some(HWND(sh.agent as *mut _)), WM_DECODED, WPARAM(0), LPARAM(0));
            }
          }
        })
        .expect("decode thread");
}

/// Ask for `path` (latest request wins; whatever was still waiting is dropped). A `prefetch` is a guess about what the
/// user will press Space on next, so it skips what would be expensive to guess wrong: big files and network paths.
pub fn request(path: &Path, maxw: u32, maxh: u32, prefetch: bool) {
    let Some(sh) = shared() else { return };
    if prefetch {
        let big = std::fs::metadata(path).map(|m| m.len() > 80 << 20).unwrap_or(true);
        if big || path.to_string_lossy().starts_with("\\\\") {
            return;
        }
    }
    if sh.inflight.lock().unwrap().as_ref() == Some(&key_of(path, maxw, maxh)) {
        return;
    }
    *sh.slot.lock().unwrap() = Some(Req { path: path.to_path_buf(), maxw, maxh });
    sh.wake.notify_one();
}

pub fn cached(path: &Path, maxw: u32, maxh: u32) -> Option<Arc<Preview>> {
    let sh = shared()?;
    let key = key_of(path, maxw, maxh);
    let c = sh.cache.lock().ok()?;
    c.iter().find(|(k, _)| *k == key).map(|(_, p)| p.clone())
}

/// Drop everything (called after a minute of not being used, together with a working-set trim).
pub fn clear_cache() {
    if let Some(sh) = shared() {
        sh.cache.lock().unwrap().clear();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// window

struct ViewState {
    hwnd: HWND,
    cur: Option<Arc<Preview>>,
    standalone: bool,
    font: HFONT,
    font_big: HFONT,
    scale: f32,
    /// Where the window currently sits (x, y, w, h), so a picture of the same size needs no move or resize.
    placed: (i32, i32, i32, i32),
    /// Where the user left the window (its centre), kept for the next time and for the next run.
    center: Option<(i32, i32)>,
    drag: Option<Drag>,
}

#[derive(Clone, Copy)]
struct Drag {
    start_screen: (i32, i32),
    start_origin: (i32, i32),
    moved: bool,
}

thread_local! {
    static VIEW: RefCell<ViewState> = const { RefCell::new(ViewState { hwnd: HWND(std::ptr::null_mut()), cur: None, standalone: false, font: HFONT(std::ptr::null_mut()), font_big: HFONT(std::ptr::null_mut()), scale: 1.0, placed: (0, 0, 0, 0), center: None, drag: None }) };
}

const CLASS: PCWSTR = w!("NuQuickView");

fn caption_h(scale: f32) -> i32 {
    (34.0 * scale).round() as i32
}

/// The biggest picture size that fits on the screen the anchor window is on (a margin all round, room for the caption).
pub fn max_size(anchor: HWND) -> (u32, u32) {
    unsafe {
        let mon = MonitorFromWindow(anchor, MONITOR_DEFAULTTOPRIMARY);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let scale = GetDpiForSystem() as f32 / 96.0;
        let w = ((mi.rcWork.right - mi.rcWork.left) as f32 * 0.9) as u32;
        let h = (((mi.rcWork.bottom - mi.rcWork.top) as f32 * 0.9) as i32 - caption_h(scale)).max(100) as u32;
        (w.max(200), h)
    }
}

unsafe extern "system" fn view_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(hwnd, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        // Never take the focus from Explorer: its arrow keys must keep working while the preview is up.
        WM_MOUSEACTIVATE if !VIEW.with(|v| v.borrow().standalone) => LRESULT(MA_NOACTIVATE as isize),
        // Drag the window anywhere; a press and release without moving is a click and closes it.
        WM_LBUTTONDOWN => {
            let (cx, cy) = (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32);
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            VIEW.with(|v| v.borrow_mut().drag = Some(Drag { start_screen: (r.left + cx, r.top + cy), start_origin: (r.left, r.top), moved: false }));
            SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let Some(d) = VIEW.with(|v| v.borrow().drag) else { return DefWindowProcW(hwnd, msg, wp, lp) };
            if wp.0 & 1 == 0 {
                return LRESULT(0); // the left button is not down: not a drag move
            }
            let (cx, cy) = (lp.0 as i16 as i32, (lp.0 >> 16) as i16 as i32);
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let (sx, sy) = (r.left + cx, r.top + cy);
            let (dx, dy) = (sx - d.start_screen.0, sy - d.start_screen.1);
            if !d.moved && dx.abs() < 4 && dy.abs() < 4 {
                return LRESULT(0);
            }
            let (nx, ny) = (d.start_origin.0 + dx, d.start_origin.1 + dy);
            let _ = SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            VIEW.with(|v| {
                let mut v = v.borrow_mut();
                v.drag = Some(Drag { moved: true, ..d });
                v.placed = (nx, ny, r.right - r.left, r.bottom - r.top);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let d = VIEW.with(|v| v.borrow_mut().drag.take());
            let _ = ReleaseCapture();
            match d {
                Some(d) if d.moved => {
                    let mut r = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut r);
                    let c = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
                    VIEW.with(|v| v.borrow_mut().center = Some(c));
                    save_center(c);
                }
                _ => {
                    if VIEW.with(|v| v.borrow().standalone) {
                        PostQuitMessage(0);
                    } else {
                        crate::close_request();
                    }
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            if VIEW.with(|v| v.borrow().standalone) {
                PostQuitMessage(0);
            } else {
                crate::close_request();
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // Only reached in standalone mode (`--preview`), where the window does take the focus.
            if matches!(wp.0 as u32, 0x1B | 0x20 | 0x0D) {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn paint(hwnd: HWND, hdc: HDC) {
    let (cur, font, font_big, scale) = VIEW.with(|v| {
        let v = v.borrow();
        (v.cur.clone(), v.font, v.font_big, v.scale)
    });
    let Some(p) = cur else { return };
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (cw, ch) = (rc.right, rc.bottom);
    let x = ((cw - p.w) / 2).max(0);
    let brush = CreateSolidBrush(COLORREF(0x002A2A2A));
    let fill = |l: i32, t: i32, r: i32, b: i32| {
        if r > l && b > t {
            FillRect(hdc, &RECT { left: l, top: t, right: r, bottom: b }, brush);
        }
    };
    fill(0, 0, x, p.h);
    fill(x + p.w, 0, cw, p.h);
    fill(0, p.h, cw, ch);
    let _ = DeleteObject(brush.into());
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: p.w, biHeight: -p.h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() },
        ..Default::default()
    };
    SetDIBitsToDevice(hdc, x, 0, p.w as u32, p.h as u32, 0, 0, 0, p.h as u32, p.bgra.as_ptr() as *const _, &bmi, DIB_RGB_COLORS);
    SetBkMode(hdc, TRANSPARENT);
    if let Some(big) = &p.big {
        let old = SelectObject(hdc, font_big.into());
        SetTextColor(hdc, COLORREF(0x00808080));
        let mut w: Vec<u16> = big.encode_utf16().collect();
        let mut r = RECT { left: x, top: 0, right: x + p.w, bottom: if p.rows.is_empty() { p.h } else { p.row_top } };
        DrawTextW(hdc, &mut w, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        SelectObject(hdc, old);
    }
    // An info card's facts: labels on the left in grey, values on the right, like Quick Look's.
    if !p.rows.is_empty() {
        let old = SelectObject(hdc, font.into());
        let (pad, mid) = ((20.0 * scale) as i32, x + p.w / 2);
        for (i, (label, value)) in p.rows.iter().enumerate() {
            let y = p.row_top + i as i32 * p.row_h;
            let mut l: Vec<u16> = label.encode_utf16().collect();
            let mut v: Vec<u16> = value.encode_utf16().collect();
            let mut lr = RECT { left: x + pad, top: y, right: mid - (8.0 * scale) as i32, bottom: y + p.row_h };
            let mut vr = RECT { left: mid + (8.0 * scale) as i32, top: y, right: x + p.w - pad, bottom: y + p.row_h };
            SetTextColor(hdc, COLORREF(0x00909090));
            DrawTextW(hdc, &mut l, &mut lr, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
            SetTextColor(hdc, COLORREF(0x00E8E8E8));
            DrawTextW(hdc, &mut v, &mut vr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_PATH_ELLIPSIS);
        }
        SelectObject(hdc, old);
    }
    // Caption: name, then the facts in a quieter colour.
    let old = SelectObject(hdc, font.into());
    let pad = (12.0 * scale) as i32;
    let mut name: Vec<u16> = p.name.encode_utf16().collect();
    let mut info: Vec<u16> = p.info.encode_utf16().collect();
    let mut irc = RECT { left: pad, top: p.h, right: cw - pad, bottom: ch };
    SetTextColor(hdc, COLORREF(0x00909090));
    DrawTextW(hdc, &mut info, &mut irc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE);
    let mut size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &info, &mut size);
    let mut nrc = RECT { left: pad, top: p.h, right: (cw - pad - size.cx - pad).max(pad + 40), bottom: ch };
    SetTextColor(hdc, COLORREF(0x00E8E8E8));
    DrawTextW(hdc, &mut name, &mut nrc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS);
    SelectObject(hdc, old);
    // A thin edge instead of a drop shadow (the shadow costs ~8 ms every time the window is shown).
    let edge = CreateSolidBrush(COLORREF(0x00606060));
    FrameRect(hdc, &RECT { left: 0, top: 0, right: cw, bottom: ch }, edge);
    let _ = DeleteObject(edge.into());
}

unsafe fn ensure_window(standalone: bool) -> HWND {
    let existing = VIEW.with(|v| v.borrow().hwnd);
    if !existing.0.is_null() {
        return existing;
    }
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: if std::env::var_os("NUQUICK_SHADOW").is_some() { CS_DROPSHADOW | CS_HREDRAW | CS_VREDRAW } else { CS_HREDRAW | CS_VREDRAW },
        lpfnWndProc: Some(view_proc),
        hInstance: hinst.into(),
        hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
        lpszClassName: CLASS,
        ..Default::default()
    };
    RegisterClassExW(&wc);
    let ex = if standalone { WS_EX_TOPMOST } else { WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW };
    let hwnd = CreateWindowExW(ex, CLASS, w!("NuQuick"), WS_POPUP, 0, 0, 100, 100, None, None, Some(hinst.into()), None).unwrap_or_default();
    let scale = GetDpiForSystem() as f32 / 96.0;
    let font = CreateFontW(-((13.0 * scale).round() as i32), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
    let font_big = CreateFontW(-((56.0 * scale).round() as i32), 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.hwnd = hwnd;
        v.center = load_center();
        v.standalone = standalone;
        v.font = font;
        v.font_big = font_big;
        v.scale = scale;
    });
    hwnd
}

fn pos_file() -> PathBuf {
    if let Some(p) = std::env::var_os("NUQUICK_POS_FILE") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("nuquick-pos.txt")
}

fn load_center() -> Option<(i32, i32)> {
    let t = std::fs::read_to_string(pos_file()).ok()?;
    let mut it = t.split_whitespace().map(|v| v.parse::<i32>());
    match (it.next(), it.next()) {
        (Some(Ok(x)), Some(Ok(y))) => Some((x, y)),
        _ => None,
    }
}

fn save_center((x, y): (i32, i32)) {
    let f = pos_file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, format!("{x} {y}
"));
}

/// Where the window goes for `p`: where the user last left it (same centre, whatever the size of the new picture), else centred
/// on the work area of the screen `anchor` is on.
unsafe fn geometry(p: &Preview, anchor: HWND, scale: f32) -> (i32, i32, i32, i32) {
    let cap = caption_h(scale);
    let ww = p.w.max((420.0 * scale) as i32);
    let wh = p.h + cap;
    let remembered = VIEW.with(|v| v.borrow().center);
    let mon = match remembered {
        Some((cx, cy)) => MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST),
        None => MonitorFromWindow(anchor, MONITOR_DEFAULTTOPRIMARY),
    };
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut mi);
    let wa = mi.rcWork;
    match remembered {
        // Same centre as last time; kept on that screen (a smaller or missing screen must not strand the window).
        Some((cx, cy)) => {
            let x = (cx - ww / 2).min(wa.right - ww).max(wa.left);
            let y = (cy - wh / 2).min(wa.bottom - wh).max(wa.top);
            (x, y, ww, wh)
        }
        None => (wa.left + ((wa.right - wa.left) - ww) / 2, wa.top + ((wa.bottom - wa.top) - wh) / 2, ww, wh),
    }
}

/// Move and size the (still hidden) window for `p` ahead of time: showing it later is then just a show. Measured: placing a
/// topmost window with a shadow costs ~10 ms, a plain show a few.
pub fn prepare(p: &Preview, anchor: HWND) {
    unsafe {
        let hwnd = ensure_window(false);
        if is_visible() {
            return;
        }
        let scale = VIEW.with(|v| v.borrow().scale);
        let g = geometry(p, anchor, scale);
        if VIEW.with(|v| v.borrow().placed) != g {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), g.0, g.1, g.2, g.3, SWP_NOACTIVATE | SWP_NOREDRAW);
            VIEW.with(|v| v.borrow_mut().placed = g);
        }
    }
}

/// Show `p` centred on the work area of the screen `anchor` is on, on top of everything, without activating it.
pub fn show(p: Arc<Preview>, anchor: HWND, standalone: bool) {
    unsafe {
        let hwnd = ensure_window(standalone);
        let scale = VIEW.with(|v| v.borrow().scale);
        let g = geometry(&p, anchor, scale);
        VIEW.with(|v| v.borrow_mut().cur = Some(p));
        let t = std::time::Instant::now();
        if VIEW.with(|v| v.borrow().placed) == g && !standalone {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        } else {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), g.0, g.1, g.2, g.3, if standalone { SWP_SHOWWINDOW } else { SWP_SHOWWINDOW | SWP_NOACTIVATE });
            VIEW.with(|v| v.borrow_mut().placed = g);
        }
        VISIBLE.store(true, Ordering::Relaxed);
        let pos_ms = t.elapsed().as_secs_f64() * 1000.0;
        // Paint now instead of waiting for the next trip through the message loop.
        let hdc = GetDC(Some(hwnd));
        paint(hwnd, hdc);
        ReleaseDC(Some(hwnd), hdc);
        let _ = ValidateRect(Some(hwnd), None);
        crate::log(&format!("show: place/show {:.1} ms, paint {:.1} ms", pos_ms, t.elapsed().as_secs_f64() * 1000.0 - pos_ms));
        if standalone {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn hide() {
    VISIBLE.store(false, Ordering::Relaxed);
    let hwnd = VIEW.with(|v| v.borrow().hwnd);
    if !hwnd.0.is_null() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

pub fn is_visible() -> bool {
    VISIBLE.load(Ordering::Relaxed)
}

/// Decode straight on this thread (`--preview`): no worker, no cache.
pub fn decode_now(path: &Path, maxw: u32, maxh: u32) -> std::result::Result<Arc<Preview>, String> {
    if previewable(path) { make_preview(path, maxw, maxh).map(Arc::new) } else { Ok(Arc::new(crate::card::build(path, None))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_and_raw_are_previewable_documents_and_models_are_not() {
        for f in ["a.jpg", "A.JPEG", "b.png", "c.webp", "d.heic", "e.CR2", "f.raf", "g.dng", "h.svg"] {
            assert!(previewable(Path::new(f)), "{f}");
        }
        for f in ["a.pdf", "b.stl", "c.zip", "d.txt", "noext", "e.docx", "f.mp4"] {
            assert!(!previewable(Path::new(f)), "{f}");
        }
    }
}
