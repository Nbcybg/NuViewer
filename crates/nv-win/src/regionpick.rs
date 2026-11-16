//! "Drag a rectangle on the screen": the screenshot (taken before this window appears) is shown dimmed over the whole desktop;
//! dragging reveals the part that will be kept, with its size next to it. Enter keeps everything, Esc or the right button cancels.

use crate::capture::Shot;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;

struct State {
    w: i32,
    h: i32,
    bright: HBITMAP,
    dim: HBITMAP,
    start: Option<(i32, i32)>,
    cur: (i32, i32),
    result: Option<Option<(i32, i32, i32, i32)>>,
}

fn bitmap_of(dc: HDC, shot: &Shot, dim: bool) -> HBITMAP {
    unsafe {
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: shot.width as i32, biHeight: -(shot.height as i32), biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        if !bits.is_null() {
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, shot.rgba.len());
            for (d, s) in dst.chunks_exact_mut(4).zip(shot.rgba.chunks_exact(4)) {
                let f = if dim { 0.45 } else { 1.0 };
                d.copy_from_slice(&[(s[2] as f32 * f) as u8, (s[1] as f32 * f) as u8, (s[0] as f32 * f) as u8, 255]);
            }
        }
        bmp
    }
}

fn selection(st: &State) -> Option<(i32, i32, i32, i32)> {
    let (sx, sy) = st.start?;
    let (x0, y0, x1, y1) = (sx.min(st.cur.0), sy.min(st.cur.1), sx.max(st.cur.0), sy.max(st.cur.1));
    Some((x0.clamp(0, st.w), y0.clamp(0, st.h), (x1 - x0).clamp(0, st.w), (y1 - y0).clamp(0, st.h)))
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let st = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State;
    let Some(s) = st.as_mut() else { return DefWindowProcW(hwnd, msg, wp, lp) };
    let (mx, my) = ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mem = CreateCompatibleDC(Some(dc));
            let old = SelectObject(mem, s.dim.into());
            let _ = BitBlt(dc, 0, 0, s.w, s.h, Some(mem), 0, 0, SRCCOPY);
            if let Some((x, y, w, h)) = selection(s) {
                if w > 0 && h > 0 {
                    SelectObject(mem, s.bright.into());
                    let _ = BitBlt(dc, x, y, w, h, Some(mem), x, y, SRCCOPY);
                    let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00FF_8B3D)); // BGR: accent blue
                    let oldp = SelectObject(dc, pen.into());
                    let oldb = SelectObject(dc, GetStockObject(NULL_BRUSH));
                    let _ = Rectangle(dc, x, y, x + w, y + h);
                    SelectObject(dc, oldb);
                    SelectObject(dc, oldp);
                    let _ = DeleteObject(pen.into());
                    let label: Vec<u16> = format!(" {w} × {h} ").encode_utf16().collect();
                    SetBkMode(dc, OPAQUE);
                    SetBkColor(dc, COLORREF(0x00FF_8B3D));
                    SetTextColor(dc, COLORREF(0x00FF_FFFF));
                    let _ = TextOutW(dc, x, (y - 20).max(0), &label);
                }
            }
            SelectObject(mem, old);
            let _ = DeleteDC(mem);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_LBUTTONDOWN => {
            s.start = Some((mx, my));
            s.cur = (mx, my);
            SetCapture(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if s.start.is_some() {
                s.cur = (mx, my);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            s.cur = (mx, my);
            s.result = Some(selection(s).filter(|r| r.2 >= 4 && r.3 >= 4));
            if s.result == Some(None) {
                s.start = None; // a click without a drag: start again
                s.result = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            s.result = Some(None);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wp.0 as u16 == VK_ESCAPE.0 {
                s.result = Some(None);
            } else if wp.0 as u16 == VK_RETURN.0 {
                s.result = Some(Some((0, 0, s.w, s.h)));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Show the picker over `shot` (which was captured from the virtual screen at (`left`, `top`)). Returns the chosen rectangle
/// in the picture's own pixels (x, y, w, h), or None when cancelled.
pub fn pick(shot: &Shot, left: i32, top: i32) -> Option<(i32, i32, u32, u32)> {
    unsafe {
        let hinst = GetModuleHandleW(None).ok()?;
        let class = w!("NuViewerRegionPick");
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc), hInstance: hinst.into(), hCursor: LoadCursorW(None, IDC_CROSS).ok()?, lpszClassName: class, ..Default::default() };
        RegisterClassExW(&wc);
        let (w, h) = (shot.width as i32, shot.height as i32);
        let screen = GetDC(None);
        let (bright, dim) = (bitmap_of(screen, shot, false), bitmap_of(screen, shot, true));
        ReleaseDC(None, screen);
        let mut state = State { w, h, bright, dim, start: None, cur: (0, 0), result: None };
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("NuImporter screenshot"), WS_POPUP, left, top, w, h, None, None, Some(hinst.into()), None).ok()?;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut state as *mut State as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while state.result.is_none() && GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
        let _ = DeleteObject(state.bright.into());
        let _ = DeleteObject(state.dim.into());
        let _ = RECT::default();
        state.result.flatten().map(|(x, y, w, h)| (x, y, w as u32, h as u32))
    }
}

/// Cut `r` (x, y, w, h) out of a picture.
pub fn crop(shot: &Shot, r: (i32, i32, u32, u32)) -> Shot {
    let (x, y, w, h) = (r.0.max(0) as u32, r.1.max(0) as u32, r.2.min(shot.width), r.3.min(shot.height));
    let (w, h) = (w.min(shot.width - x.min(shot.width)), h.min(shot.height - y.min(shot.height)));
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let a = ((row * shot.width + x) * 4) as usize;
        rgba.extend_from_slice(&shot.rgba[a..a + (w * 4) as usize]);
    }
    Shot { width: w, height: h, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropping_takes_the_right_pixels() {
        let rgba: Vec<u8> = (0..4 * 4).flat_map(|i| [i as u8, 0, 0, 255]).collect();
        let s = Shot { width: 4, height: 4, rgba };
        let c = crop(&s, (1, 2, 2, 2));
        assert_eq!((c.width, c.height), (2, 2));
        assert_eq!(c.rgba[0], 9, "pixel (1,2) is number 9");
        assert_eq!(c.rgba[4], 10);
        assert_eq!(c.rgba[8], 13);
        let over = crop(&s, (3, 3, 10, 10));
        assert_eq!((over.width, over.height), (1, 1));
    }
}
