//! Pictures that arrive from the screen and the clipboard (what NuImporter takes in): a screenshot of the whole desktop, a region
//! or a window, the picture on the clipboard (PNG, or a bitmap with or without alpha) and files copied in Explorer.

use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, RECT};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::DragQueryFileW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// RGBA pixels, top-down, opaque unless the source had alpha.
pub struct Shot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The rectangle of all monitors together: (left, top, width, height).
pub fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1)) }
}

/// The screen inside (x, y, w, h) (virtual-screen coordinates), as RGBA.
pub fn capture_rect(x: i32, y: i32, w: i32, h: i32) -> Result<Shot, String> {
    if w <= 0 || h <= 0 || w > 32_000 || h > 32_000 {
        return Err("nothing to capture".into());
    }
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        let result = match dib {
            Ok(dib) if !bits.is_null() => {
                let old = SelectObject(dc, dib.into());
                // CAPTUREBLT includes layered (transparent) windows.
                let ok = BitBlt(dc, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
                let _ = GdiFlush();
                let r = if ok.is_ok() {
                    let src = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
                    let mut rgba = Vec::with_capacity(src.len());
                    for p in src.chunks_exact(4) {
                        rgba.extend([p[2], p[1], p[0], 255]);
                    }
                    Ok(Shot { width: w as u32, height: h as u32, rgba })
                } else {
                    Err("the screen could not be copied".to_string())
                };
                SelectObject(dc, old);
                let _ = DeleteObject(dib.into());
                r
            }
            _ => Err("no memory for the screenshot".to_string()),
        };
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        result
    }
}

/// All monitors.
pub fn capture_desktop() -> Result<Shot, String> {
    let (x, y, w, h) = virtual_screen();
    capture_rect(x, y, w, h)
}

/// The window under the pointer (its outer frame), or the foreground window.
pub fn window_rect_at_pointer() -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut pt = windows::Win32::Foundation::POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let hwnd = GetAncestor(WindowFromPoint(pt), GA_ROOT);
        if hwnd.0.is_null() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
}

// ---- clipboard -------------------------------------------------------------------------------------------------

const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_DIBV5: u32 = 17;

unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if OpenClipboard(Some(hwnd)).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

unsafe fn bytes_of(fmt: u32) -> Option<Vec<u8>> {
    let h = GetClipboardData(fmt).ok()?;
    let g = windows::Win32::Foundation::HGLOBAL(h.0);
    let size = GlobalSize(g);
    let p = GlobalLock(g) as *const u8;
    if p.is_null() || size == 0 {
        return None;
    }
    let v = std::slice::from_raw_parts(p, size).to_vec();
    let _ = GlobalUnlock(g);
    Some(v)
}

/// Decode a clipboard bitmap (BITMAPINFOHEADER / V5 followed by the pixels): 24- and 32-bit, bottom-up or top-down.
pub fn parse_dib(d: &[u8]) -> Option<Shot> {
    let u32at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let header = u32at(0)? as usize;
    let (w, hraw) = (u32at(4)? as i32, u32at(8)? as i32);
    let bpp = d.get(14..16).map(|b| u16::from_le_bytes(b.try_into().unwrap()))? as usize;
    let compression = u32at(16)?;
    let colours = u32at(32).unwrap_or(0) as usize;
    if w <= 0 || hraw == 0 || !(bpp == 24 || bpp == 32) || !(compression == 0 || compression == 3) || header < 40 {
        return None;
    }
    let (h, top_down) = (hraw.unsigned_abs() as usize, hraw < 0);
    let w = w as usize;
    let mut off = header + if compression == 3 && header == 40 { 12 } else { 0 } + colours * 4;
    if off > d.len() {
        off = header;
    }
    let stride = (w * bpp).div_ceil(32) * 4;
    if d.len() < off + stride * h {
        return None;
    }
    let mut rgba = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    for y in 0..h {
        let src_y = if top_down { y } else { h - 1 - y };
        let row = &d[off + src_y * stride..off + src_y * stride + w * bpp / 8];
        for x in 0..w {
            let (b, g, r, a) = if bpp == 32 { (row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]) } else { (row[x * 3], row[x * 3 + 1], row[x * 3 + 2], 255) };
            any_alpha |= a != 0 && a != 255;
            let o = (y * w + x) * 4;
            rgba[o..o + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    // Many programs write 32-bit bitmaps with an alpha byte of 0: the picture is opaque then.
    if !any_alpha {
        let first = rgba.chunks_exact(4).next().map(|p| p[3]).unwrap_or(255);
        let uniform = rgba.chunks_exact(4).all(|p| p[3] == first);
        if uniform {
            for p in rgba.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
    }
    Some(Shot { width: w as u32, height: h as u32, rgba })
}

/// The picture on the clipboard: a "PNG" entry (lossless, with alpha) first, else a bitmap.
pub fn clipboard_image(hwnd: HWND) -> Option<Shot> {
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let png_fmt = RegisterClipboardFormatW(w!("PNG"));
        let found = (|| {
            if png_fmt != 0 && IsClipboardFormatAvailable(png_fmt).is_ok() {
                if let Some(b) = bytes_of(png_fmt) {
                    if let Ok(img) = nv_codecs_decode(&b) {
                        return Some(img);
                    }
                }
            }
            for fmt in [CF_DIBV5, CF_DIB] {
                if IsClipboardFormatAvailable(fmt).is_ok() {
                    if let Some(b) = bytes_of(fmt) {
                        if let Some(s) = parse_dib(&b) {
                            return Some(s);
                        }
                    }
                }
            }
            None
        })();
        let _ = CloseClipboard();
        found
    }
}

/// Decode PNG bytes (the clipboard's "PNG" format) with the `png` crate: no dependency on the codecs here.
fn nv_codecs_decode(b: &[u8]) -> Result<Shot, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(b));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; r.output_buffer_size().ok_or("too large")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let px = &buf[..info.buffer_size()];
    let n = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    if rgba.len() != n * 4 {
        return Err("odd PNG".into());
    }
    Ok(Shot { width: info.width, height: info.height, rgba })
}

/// Files copied in Explorer (or by another program) that are on the clipboard.
pub fn clipboard_files(hwnd: HWND) -> Vec<PathBuf> {
    unsafe {
        if !open(hwnd) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if IsClipboardFormatAvailable(CF_HDROP).is_ok() {
            if let Ok(h) = GetClipboardData(CF_HDROP) {
                let hdrop = windows::Win32::UI::Shell::HDROP(h.0);
                let n = DragQueryFileW(hdrop, u32::MAX, None);
                for i in 0..n {
                    let len = DragQueryFileW(hdrop, i, None) as usize;
                    let mut buf = vec![0u16; len + 1];
                    DragQueryFileW(hdrop, i, Some(&mut buf));
                    out.push(PathBuf::from(String::from_utf16_lossy(&buf[..len])));
                }
            }
        }
        let _ = CloseClipboard();
        out
    }
}

#[allow(dead_code)]
fn _unused(_: HANDLE) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn dib(w: i32, h: i32, bpp: u16, px: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        for v in [40u32, w as u32, h as u32] {
            d.extend(v.to_le_bytes());
        }
        d.extend(1u16.to_le_bytes());
        d.extend(bpp.to_le_bytes());
        d.extend([0u8; 24]);
        d.extend(px);
        d
    }

    #[test]
    fn a_24_bit_bitmap_is_upright_rgba() {
        // 2 x 2, bottom-up: bottom row first. Rows are padded to 4 bytes (2 * 3 = 6 -> 8).
        let d = dib(2, 2, 24, &[0, 0, 255, 0, 255, 0, 0, 0, /* bottom: red, green */ 255, 0, 0, 0, 0, 0, 0, 0 /* top: blue, black */]);
        let s = parse_dib(&d).unwrap();
        assert_eq!((s.width, s.height), (2, 2));
        assert_eq!(&s.rgba[0..4], &[0, 0, 255, 255], "top-left is blue");
        assert_eq!(&s.rgba[8..12], &[255, 0, 0, 255], "bottom-left is red");
    }

    #[test]
    fn a_32_bit_bitmap_with_zero_alpha_is_taken_as_opaque_and_with_real_alpha_kept() {
        let zero = dib(1, 1, 32, &[10, 20, 30, 0]);
        assert_eq!(parse_dib(&zero).unwrap().rgba, vec![30, 20, 10, 255]);
        let real = dib(2, 1, 32, &[10, 20, 30, 128, 1, 2, 3, 255]);
        let s = parse_dib(&real).unwrap();
        assert_eq!(s.rgba[3], 128);
        assert!(parse_dib(&dib(1, 1, 8, &[0, 0, 0, 0])).is_none());
        assert!(parse_dib(&[0u8; 10]).is_none());
    }

    #[test]
    fn the_screen_can_be_captured() {
        // No window is needed; in a session with no desktop this may fail, which is not a bug of ours.
        if let Ok(s) = capture_rect(0, 0, 8, 8) {
            assert_eq!(s.rgba.len(), 8 * 8 * 4);
            assert!(s.rgba.chunks_exact(4).all(|p| p[3] == 255));
        }
    }
}

// ---- put a picture on the clipboard ---------------------------------------------------------------------------------

const CF_DIB_FMT: u32 = 8;

/// Put RGBA pixels on the clipboard (a "PNG" entry with alpha, and a plain bitmap for programs that only know that).
pub fn clipboard_set_image(hwnd: HWND, shot: &Shot) -> Result<(), String> {
    use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE, GMEM_ZEROINIT};
    let mut png_bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png_bytes, shot.width, shot.height);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::Fast);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(&shot.rgba).map_err(|e| e.to_string())?;
        wr.finish().map_err(|e| e.to_string())?;
    }
    let (w, h) = (shot.width as usize, shot.height as usize);
    let mut dib = Vec::with_capacity(40 + w * h * 4);
    for v in [40u32, shot.width, shot.height] {
        dib.extend(v.to_le_bytes());
    }
    dib.extend(1u16.to_le_bytes());
    dib.extend(32u16.to_le_bytes());
    for v in [0u32, (w * h * 4) as u32, 0, 0, 0, 0] {
        dib.extend(v.to_le_bytes());
    }
    for y in (0..h).rev() {
        for px in shot.rgba[y * w * 4..(y + 1) * w * 4].chunks_exact(4) {
            let a = px[3] as u32;
            let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            dib.extend([blend(px[2]), blend(px[1]), blend(px[0]), 255]);
        }
    }
    unsafe {
        if !open(hwnd) {
            return Err("the clipboard is busy".into());
        }
        let alloc = |bytes: &[u8]| -> Result<HANDLE, String> {
            let g = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()).map_err(|e| e.to_string())?;
            let p = GlobalLock(g) as *mut u8;
            if p.is_null() {
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            let _ = GlobalUnlock(g);
            Ok(HANDLE(g.0))
        };
        let r = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            SetClipboardData(CF_DIB_FMT, Some(alloc(&dib)?)).map_err(|e| e.to_string())?;
            let png_fmt = RegisterClipboardFormatW(w!("PNG"));
            if png_fmt != 0 {
                SetClipboardData(png_fmt, Some(alloc(&png_bytes)?)).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        r
    }
}
