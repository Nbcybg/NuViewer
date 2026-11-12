//! Text for on-screen overlays, rasterised with GDI into white-on-transparent RGBA bitmaps that the
//! renderer uploads as textures and tints. (GDI gives crisp hinted text and Windows' font fallback,
//! so Thai/CJK filenames render without us shipping fonts.)

use std::ffi::c_void;
use windows::core::w;
use windows::Win32::Graphics::Gdi::*;

pub struct TextBitmap {
    pub width: u32,
    pub height: u32,
    /// Straight-alpha RGBA: RGB is white, alpha is the glyph coverage.
    pub rgba: Vec<u8>,
    /// Height of one line in pixels (rows of different bitmaps rendered at the same size line up).
    pub line_height: i32,
}

fn make_font(px: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            400, // FW_NORMAL
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY, // greyscale AA: ClearType colour fringes can't be tinted
            0,                   // DEFAULT_PITCH | FF_DONTCARE
            w!("Segoe UI"),
        )
    }
}

fn line_height_for(px: i32) -> i32 {
    (px as f32 * 1.35).round() as i32
}

/// Width in pixels of `text` at font size `px`.
pub fn measure(text: &str, px: i32) -> i32 {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let font = make_font(px);
        let old = SelectObject(dc, font.into());
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = windows::Win32::Foundation::SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        SelectObject(dc, old);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        size.cx
    }
}

/// Render `lines` (one per row) with `pad` pixels of padding around them.
pub fn render_lines(lines: &[String], px: i32, pad: i32) -> TextBitmap {
    let lh = line_height_for(px);
    unsafe {
        let dc = CreateCompatibleDC(None);
        let font = make_font(px);
        let old_font = SelectObject(dc, font.into());

        let mut max_w = 1;
        for l in lines {
            let wide: Vec<u16> = l.encode_utf16().collect();
            let mut size = windows::Win32::Foundation::SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            max_w = max_w.max(size.cx);
        }
        let (w, h) = ((max_w + 2 * pad).max(1), (lines.len() as i32 * lh + 2 * pad).max(1));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        if let (Ok(dib), false) = (dib, bits.is_null()) {
            let old_bmp = SelectObject(dc, dib.into());
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, windows::Win32::Foundation::COLORREF(0x00FF_FFFF));
            for (i, l) in lines.iter().enumerate() {
                let wide: Vec<u16> = l.encode_utf16().collect();
                let _ = TextOutW(dc, pad, pad + i as i32 * lh + (lh - px) / 4, &wide);
            }
            let _ = GdiFlush();
            let src = std::slice::from_raw_parts(bits as *const u8, w as usize * h as usize * 4);
            for (d, s) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                // White text on black: any channel is the coverage.
                d.copy_from_slice(&[255, 255, 255, s[0].max(s[1]).max(s[2])]);
            }
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(dib.into());
        }
        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        TextBitmap { width: w as u32, height: h as u32, rgba, line_height: lh }
    }
}
