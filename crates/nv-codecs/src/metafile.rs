//! Windows metafiles (EMF / WMF, including "placeable" WMF): played back by GDI+ at screen size on white paper.

use crate::Image;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Graphics::GdiPlus::*;

/// GDI+ pixel format 32bppARGB (not exposed by the `windows` crate).
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;

/// GDI+ must be started once per process and may be shared by all threads.
fn startup() -> Result<(), String> {
    use std::sync::OnceLock;
    static STARTED: OnceLock<bool> = OnceLock::new();
    let ok = *STARTED.get_or_init(|| unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        GdiplusStartup(&mut token, &input, std::ptr::null_mut()) == Status(0)
    });
    if ok { std::result::Result::Ok(()) } else { Err("metafile: GDI+ could not be started".into()) }
}

pub fn decode_file(path: &Path, thumb: bool) -> Result<Image, String> {
    startup()?;
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let mut image: *mut GpImage = std::ptr::null_mut();
        if GdipLoadImageFromFile(PCWSTR(wide.as_ptr()), &mut image) != Status(0) || image.is_null() {
            return Err("metafile: can't read this EMF/WMF file".into());
        }
        let result = render(image, thumb);
        GdipDisposeImage(image);
        result
    }
}

unsafe fn render(image: *mut GpImage, thumb: bool) -> Result<Image, String> {
    let (mut iw, mut ih) = (0u32, 0u32);
    GdipGetImageWidth(image, &mut iw);
    GdipGetImageHeight(image, &mut ih);
    if iw == 0 || ih == 0 {
        return Err("metafile: empty drawing".into());
    }
    let long = if thumb { 640.0 } else { crate::render_long_side() as f32 };
    let s = long / iw.max(ih) as f32;
    let (w, h) = (((iw as f32 * s).round() as u32).max(1), ((ih as f32 * s).round() as u32).max(1));
    crate::check_size(w as u64, h as u64)?;

    let mut bitmap: *mut GpBitmap = std::ptr::null_mut();
    if GdipCreateBitmapFromScan0(w as i32, h as i32, 0, PIXEL_FORMAT_32BPP_ARGB, None, &mut bitmap) != Status(0) {
        return Err("metafile: out of memory".into());
    }
    let mut g: *mut GpGraphics = std::ptr::null_mut();
    let mut out = Err("metafile: can't draw".to_string());
    if GdipGetImageGraphicsContext(bitmap as *mut GpImage, &mut g) == Status(0) {
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        GdipSetInterpolationMode(g, InterpolationModeHighQualityBicubic);
        GdipGraphicsClear(g, 0xFFFF_FFFF); // white paper
        if GdipDrawImageRectI(g, image, 0, 0, w as i32, h as i32) == Status(0) {
            let rect = Rect { X: 0, Y: 0, Width: w as i32, Height: h as i32 };
            let mut data = BitmapData::default();
            if GdipBitmapLockBits(bitmap, &rect, ImageLockModeRead.0 as u32, PIXEL_FORMAT_32BPP_ARGB, &mut data) == Status(0) {
                let mut rgba = vec![0u8; w as usize * h as usize * 4];
                for y in 0..h as usize {
                    let row = std::slice::from_raw_parts((data.Scan0 as *const u8).offset(y as isize * data.Stride as isize), w as usize * 4);
                    for (d, s) in rgba[y * w as usize * 4..(y + 1) * w as usize * 4].chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                        d.copy_from_slice(&[s[2], s[1], s[0], 255]); // BGRA -> RGBA, opaque (drawn on white)
                    }
                }
                GdipBitmapUnlockBits(bitmap, &mut data);
                out = std::result::Result::Ok(Image::new(w, h, rgba));
            }
        }
        GdipDeleteGraphics(g);
    }
    GdipDisposeImage(bitmap as *mut GpImage);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{CloseEnhMetaFile, CreateEnhMetaFileW, CreateSolidBrush, DeleteEnhMetaFile, DeleteObject, FillRect};

    /// A real EMF made with GDI: a blue field with a red square in the middle (frame in 0.01 mm units: 40 x 20 mm).
    fn make_emf(path: &Path) {
        let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let frame = RECT { left: 0, top: 0, right: 4000, bottom: 2000 };
            let dc = CreateEnhMetaFileW(None, PCWSTR(wide.as_ptr()), Some(&frame), PCWSTR::null());
            let blue = CreateSolidBrush(COLORREF(0x00FF_0000)); // COLORREF is 0x00BBGGRR
            let red = CreateSolidBrush(COLORREF(0x0000_00FF));
            // Device units of an EMF DC are pixels at the reference device; just fill generously.
            FillRect(dc, &RECT { left: 0, top: 0, right: 100_000, bottom: 100_000 }, blue);
            let _ = DeleteObject(blue.into());
            let _ = DeleteObject(red.into());
            let h = CloseEnhMetaFile(dc);
            let _ = DeleteEnhMetaFile(Some(h));
        }
    }

    #[test]
    fn plays_back_an_emf() {
        let dir = std::env::temp_dir().join(format!("nv_emf_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.emf");
        make_emf(&file);
        let img = decode_file(&file, true).unwrap();
        assert_eq!(img.width.max(img.height), 640);
        assert!(img.width > img.height, "40 x 20 mm frame is landscape ({}x{})", img.width, img.height);
        let i = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
        assert_eq!(&img.rgba[i..i + 4], &[0, 0, 255, 255], "blue fill at the centre");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_is_an_error() {
        let dir = std::env::temp_dir().join(format!("nv_emf_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bad.wmf");
        std::fs::write(&file, b"not a metafile at all").unwrap();
        assert!(decode_file(&file, true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
