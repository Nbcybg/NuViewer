//! The first page of an XPS / OpenXPS document drawn by Windows' own XPS renderer (the XPS Object Model and the XPS rasterization
//! service), on white paper. Only used when the package carries no thumbnail.

use crate::Image;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Printing::{IXpsRasterizationFactory, IXpsRasterizerNotificationCallback, XPSRAS_RENDERING_MODE_ANTIALIASED};
use windows::Win32::Storage::Xps::{IXpsOMObjectFactory, XpsOMObjectFactory};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

pub fn render_first_page(path: &Path, thumb: bool) -> Result<Image, String> {
    let err = |what: &str, e: windows::core::Error| format!("xps: {what}: {e}");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IXpsOMObjectFactory = CoCreateInstance(&XpsOMObjectFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| err("object model", e))?;
        let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let package = factory.CreatePackageFromFile(PCWSTR(wide.as_ptr()), false).map_err(|e| err("open", e))?;
        let docs = package.GetDocumentSequence().map_err(|e| err("document sequence", e))?.GetDocuments().map_err(|e| err("documents", e))?;
        if docs.GetCount().map_err(|e| err("documents", e))? == 0 {
            return Err("xps: no documents".into());
        }
        let pages = docs.GetAt(0).map_err(|e| err("document", e))?.GetPageReferences().map_err(|e| err("pages", e))?;
        if pages.GetCount().map_err(|e| err("pages", e))? == 0 {
            return Err("xps: no pages".into());
        }
        let page = pages.GetAt(0).map_err(|e| err("page reference", e))?.GetPage().map_err(|e| err("page", e))?;
        let dim = page.GetPageDimensions().map_err(|e| err("page size", e))?;
        let (pw, ph) = (dim.width.max(1.0), dim.height.max(1.0));
        let target = if thumb { 640.0 } else { 1600.0 };
        let dpi = (96.0 * target / pw.max(ph)).clamp(24.0, 400.0);
        let (w, h) = ((pw * dpi / 96.0).round() as i32, (ph * dpi / 96.0).round() as i32);
        crate::check_size(w as u64, h as u64)?;
        // The rasterization service is its own COM class (XpsRasterService.dll), not part of the object model factory.
        const XPS_RASTERIZER_FACTORY: windows::core::GUID = windows::core::GUID::from_u128(0x503e79bf_1d09_4764_9d72_1eb0c65967c6);
        let ras: IXpsRasterizationFactory = CoCreateInstance(&XPS_RASTERIZER_FACTORY, None, CLSCTX_INPROC_SERVER).map_err(|e| err("rasterizer", e))?;
        let rasterizer = ras.CreateRasterizer(&page, dpi, XPSRAS_RENDERING_MODE_ANTIALIASED, XPSRAS_RENDERING_MODE_ANTIALIASED).map_err(|e| err("rasterizer", e))?;
        let bmp = rasterizer.RasterizeRect(0, 0, w, h, None::<&IXpsRasterizerNotificationCallback>).map_err(|e| err("draw", e))?;
        let stride = (w * 4) as u32;
        let mut px = vec![0u8; (w * h * 4) as usize];
        bmp.CopyPixels(std::ptr::null(), stride, &mut px).map_err(|e| err("read pixels", e))?;
        // Premultiplied BGRA over white paper -> opaque RGBA.
        for p in px.chunks_exact_mut(4) {
            let inv = 255 - p[3] as u32;
            let (b, g, r) = (p[0] as u32 + inv, p[1] as u32 + inv, p[2] as u32 + inv);
            p[0] = r.min(255) as u8;
            p[1] = g.min(255) as u8;
            p[2] = b.min(255) as u8;
            p[3] = 255;
        }
        Ok(Image::new(w as u32, h as u32, px))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn draws_a_page_when_given_a_file() {
        // Set NV_XPS_TEST to a .xps / .oxps file to see the result; without it there is nothing to draw.
        let Some(p) = std::env::var_os("NV_XPS_TEST") else { return };
        let img = super::render_first_page(std::path::Path::new(&p), true).unwrap();
        assert!(img.width > 100);
    }
}
