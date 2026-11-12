//! Windows Imaging Component fallback: HEIC/HEIF and AVIF (when the Microsoft Store image extensions
//! are installed), TIFF, BMP, ICO, JPEG XR, and camera RAW via an installed RAW codec.

use crate::Image;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

const WINCODEC_ERR_COMPONENTNOTFOUND: i32 = 0x8898_2F50u32 as i32;
const WINCODEC_ERR_UNKNOWNIMAGEFORMAT: i32 = 0x8898_2F07u32 as i32;
/// MF_E_TOPO_CODEC_NOT_FOUND: what the HEIF decoder reports when the HEVC codec isn't installed.
const MF_E_TOPO_CODEC_NOT_FOUND: i32 = 0xC00D_5212u32 as i32;

thread_local! {
    /// COM must be initialised on every thread that uses WIC; the decode pool threads all come through here.
    static COM_READY: bool = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
}

fn describe(e: windows::core::Error) -> String {
    match e.code().0 {
        WINCODEC_ERR_COMPONENTNOTFOUND | WINCODEC_ERR_UNKNOWNIMAGEFORMAT => {
            "no Windows codec for this format (HEIC/AVIF need the Microsoft HEIF / AV1 image extensions)".to_string()
        }
        MF_E_TOPO_CODEC_NOT_FOUND => "needs the HEVC codec (install \"HEVC Video Extensions\" from the Microsoft Store)".to_string(),
        code => format!("wic: {e} (0x{:08X})", code as u32),
    }
}

pub fn decode_file(path: &Path) -> Result<Image, String> {
    if !COM_READY.with(|ok| *ok) {
        return Err("wic: COM initialisation failed".into());
    }
    // Read the file ourselves (so the viewer can show how far the disk has got) and let WIC decode from memory.
    // Huge files, and anything that can't be read that way, go through WIC's own file access as before.
    if std::fs::metadata(path).map(|m| m.len() <= 1 << 30).unwrap_or(false) {
        if let Ok(bytes) = crate::progress::read_file(path) {
            if let Ok(img) = decode_bytes(&bytes) {
                return Ok(img);
            }
        }
        if crate::progress::is_cancelled() {
            return Err(crate::progress::CANCELLED.into());
        }
    }
    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(describe)?;
        let decoder = factory
            .CreateDecoderFromFilename(PCWSTR(wide.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)
            .map_err(describe)?;
        finish(&factory, &decoder)
    }
}

/// Decode an image held in memory (e.g. the TIFF preview inside an EPS file).
pub fn decode_bytes(bytes: &[u8]) -> Result<Image, String> {
    if !COM_READY.with(|ok| *ok) {
        return Err("wic: COM initialisation failed".into());
    }
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).map_err(describe)?;
        let stream = factory.CreateStream().map_err(describe)?;
        stream.InitializeFromMemory(bytes).map_err(describe)?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).map_err(describe)?;
        finish(&factory, &decoder)
    }
}

unsafe fn finish(factory: &IWICImagingFactory, decoder: &IWICBitmapDecoder) -> Result<Image, String> {
    {

        // Multi-resolution containers (ICO) and multi-page TIFF: use the largest frame.
        let count = decoder.GetFrameCount().map_err(describe)?.max(1);
        let mut best: Option<(IWICBitmapFrameDecode, u64)> = None;
        for i in 0..count.min(64) {
            let Ok(frame) = decoder.GetFrame(i) else { continue };
            let (mut w, mut h) = (0u32, 0u32);
            if frame.GetSize(&mut w, &mut h).is_err() {
                continue;
            }
            let px = w as u64 * h as u64;
            if best.as_ref().map(|b| px > b.1).unwrap_or(true) {
                best = Some((frame, px));
            }
        }
        let (frame, _) = best.ok_or("wic: no readable frame")?;

        let converter = factory.CreateFormatConverter().map_err(describe)?;
        converter
            .Initialize(&frame, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
            .map_err(describe)?;
        let (mut w, mut h) = (0u32, 0u32);
        converter.GetSize(&mut w, &mut h).map_err(describe)?;
        crate::check_size(w as u64, h as u64)?;
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut rgba).map_err(describe)?;
        Ok(Image::new(w, h, rgba))
    }
}
