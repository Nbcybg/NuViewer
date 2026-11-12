//! PDF (and PDF-compatible Adobe Illustrator `.ai`) pages, rendered by Windows' own `Windows.Data.Pdf` engine:
//! no extra binary to ship, vector text and graphics stay sharp at the size we render.
//!
//! A multi-page document is addressed one page at a time through a pseudo path `file.pdf?page=3` (see `split_page`;
//! `?` can't occur in a Windows file name, so it never collides with a real file). Page 1 is just the plain path.

use crate::Image;
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::Storage::StorageFile;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

thread_local! {
    /// The WinRT runtime must be initialised on every decode-pool thread that touches it (S_FALSE = already done).
    static WINRT_READY: bool = unsafe { RoInitialize(RO_INIT_MULTITHREADED).is_ok() };
}

/// `"C:\x\doc.pdf?page=3"` -> (`"C:\x\doc.pdf"`, 3). Anything else -> (path, 1).
pub fn split_page(path: &Path) -> (PathBuf, u32) {
    let s = path.as_os_str().to_string_lossy();
    if let Some((real, page)) = s.rsplit_once("?page=") {
        if let Ok(n) = page.parse::<u32>() {
            return (PathBuf::from(real), n.max(1));
        }
    }
    (path.to_path_buf(), 1)
}

/// The path that addresses `page` (1-based) of the document `path`.
pub fn page_path(path: &Path, page: u32) -> PathBuf {
    if page <= 1 {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}?page={page}", path.display()))
    }
}

fn werr(what: &str, e: windows::core::Error) -> String {
    format!("pdf: {what}: {e}")
}

/// Render page `page` (1-based, clamped to the document) of the PDF at `path`. `thumb` = small, fast render.
pub fn render(path: &Path, page: u32, thumb: bool) -> Result<Image, String> {
    if !WINRT_READY.with(|ok| *ok) {
        return Err("pdf: the Windows runtime could not be initialised".into());
    }
    let wide = HSTRING::from(path.as_os_str().to_string_lossy().as_ref());
    let file = StorageFile::GetFileFromPathAsync(&wide).map_err(|e| werr("open", e))?.join().map_err(|e| werr("open", e))?;
    let doc = PdfDocument::LoadFromFileAsync(&file).map_err(|e| werr("load", e))?.join().map_err(|e| {
        // Both a damaged file and an unsupported encryption end up here.
        format!("pdf: can't open this document ({e})")
    })?;
    if doc.IsPasswordProtected().unwrap_or(false) {
        return Err("pdf: the document is password protected".into());
    }
    let count = doc.PageCount().map_err(|e| werr("page count", e))?.max(1);
    let index = page.clamp(1, count) - 1;
    let pg = doc.GetPage(index).map_err(|e| werr("page", e))?;
    let size = pg.Size().map_err(|e| werr("page size", e))?;
    let (pw, ph) = (size.Width.max(1.0), size.Height.max(1.0));
    let long = if thumb { 640.0 } else { crate::render_long_side() as f32 };
    let s = long / pw.max(ph);
    let (w, h) = (((pw * s).round() as u32).max(1), ((ph * s).round() as u32).max(1));
    crate::check_size(w as u64, h as u64)?;

    let opts = PdfPageRenderOptions::new().map_err(|e| werr("options", e))?;
    opts.SetDestinationWidth(w).map_err(|e| werr("options", e))?;
    opts.SetDestinationHeight(h).map_err(|e| werr("options", e))?;
    let stream = InMemoryRandomAccessStream::new().map_err(|e| werr("stream", e))?;
    pg.RenderWithOptionsToStreamAsync(&stream, &opts).map_err(|e| werr("render", e))?.join().map_err(|e| werr("render", e))?;

    // The engine hands back an encoded image (PNG by default).
    let len = stream.Size().map_err(|e| werr("stream", e))? as u32;
    let input = stream.GetInputStreamAt(0).map_err(|e| werr("stream", e))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| werr("stream", e))?;
    reader.LoadAsync(len).map_err(|e| werr("stream", e))?.join().map_err(|e| werr("stream", e))?;
    let mut bytes = vec![0u8; len as usize];
    reader.ReadBytes(&mut bytes).map_err(|e| werr("stream", e))?;

    let mut img = crate::pngdec::decode(&bytes, true)?;
    flatten_on_white(&mut img.rgba);
    img.page = Some((index + 1, count));
    Ok(img)
}

/// Pages are drawn on transparent where nothing is painted: show them on white paper, like every PDF reader.
fn flatten_on_white(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a == 255 {
            continue;
        }
        for c in &mut p[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_paths_round_trip() {
        let p = Path::new("C:\\docs\\a b.pdf");
        assert_eq!(page_path(p, 1), p);
        let p3 = page_path(p, 3);
        assert_eq!(p3.to_string_lossy(), "C:\\docs\\a b.pdf?page=3");
        assert_eq!(split_page(&p3), (p.to_path_buf(), 3));
        assert_eq!(split_page(p), (p.to_path_buf(), 1));
        assert_eq!(split_page(Path::new("x.pdf?page=zero")), (PathBuf::from("x.pdf?page=zero"), 1));
    }

    /// A valid 3-page PDF (page 1 red, 2 green, 3 blue; the pages are 200x100 and 100x200 points) built by hand.
    fn three_page_pdf() -> Vec<u8> {
        let fills = ["1 0 0 rg", "0 1 0 rg", "0 0 1 rg"];
        let boxes = ["200 100", "200 100", "100 200"];
        let mut objs: Vec<String> = vec![
            "<</Type/Catalog/Pages 2 0 R>>".into(),
            "<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3>>".into(),
        ];
        for i in 0..3 {
            objs.push(format!("<</Type/Page/Parent 2 0 R/MediaBox[0 0 {}]/Contents {} 0 R>>", boxes[i], 6 + i));
        }
        for i in 0..3 {
            let body = format!("{} 0 0 1000 1000 re f", fills[i]);
            objs.push(format!("<</Length {}>>\nstream\n{body}\nendstream", body.len()));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn renders_the_requested_page_of_a_generated_pdf() {
        let dir = std::env::temp_dir().join(format!("nv_pdf_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("three pages.pdf");
        std::fs::write(&file, three_page_pdf()).unwrap();

        let centre = |img: &Image| {
            let i = ((img.height / 2 * img.width + img.width / 2) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
        };
        let p1 = render(&file, 1, true).unwrap();
        let p2 = render(&file, 2, true).unwrap();
        let p3 = render(&file, 3, true).unwrap();
        assert_eq!(p1.page, Some((1, 3)));
        assert_eq!(p3.page, Some((3, 3)));
        assert_eq!(centre(&p1), [255, 0, 0, 255], "page 1 is red");
        assert_eq!(centre(&p2), [0, 255, 0, 255], "page 2 is green");
        assert_eq!(centre(&p3), [0, 0, 255, 255], "page 3 is blue");
        assert!(p1.width > p1.height, "page 1 is landscape ({}x{})", p1.width, p1.height);
        assert!(p3.height > p3.width, "page 3 is portrait ({}x{})", p3.width, p3.height);
        assert_eq!(p1.width.max(p1.height), 640, "thumbnail render uses 640 px on the long side");
        // Out-of-range pages clamp to the last one; the pseudo path goes through the normal entry point.
        assert_eq!(render(&file, 99, true).unwrap().page, Some((3, 3)));
        let via_entry = crate::decode_file(&page_path(&file, 2)).unwrap();
        assert_eq!(via_entry.page, Some((2, 3)));
        assert_eq!(centre(&via_entry), [0, 255, 0, 255]);
        assert!(via_entry.width.max(via_entry.height) >= 2048, "full render is screen sized");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_pdf_is_an_error_not_a_crash() {
        let dir = std::env::temp_dir().join(format!("nv_pdf_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bad.pdf");
        std::fs::write(&file, b"%PDF-1.4\nthis is not really a pdf").unwrap();
        assert!(render(&file, 1, true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transparent_pixels_become_white_paper() {
        let mut px = vec![0, 0, 0, 0, 10, 20, 30, 255, 0, 0, 0, 128];
        flatten_on_white(&mut px);
        assert_eq!(&px[0..4], &[255, 255, 255, 255]);
        assert_eq!(&px[4..8], &[10, 20, 30, 255]);
        assert!(px[8] > 120 && px[8] < 135 && px[11] == 255);
    }
}
