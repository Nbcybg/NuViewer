//! Making a PDF from pictures: one picture per page, in the order given. A JPEG goes in byte for byte (no quality lost, no
//! re-encoding time); every other format is decoded and stored lossless (Flate) when it is small or as JPEG when it is big.
//! The file is written as it goes, so a few hundred photographs never sit in memory together.

use crate::prefs::PdfPage;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Pixels per inch a "fit" page assumes for its pictures.
const FIT_DPI: f32 = 150.0;
/// Non-JPEG pictures above this many pixels are stored as JPEG instead of lossless.
const LOSSLESS_MAX_PX: u64 = 4_000_000;

struct Pdf<W: Write> {
    out: BufWriter<W>,
    pos: u64,
    offsets: Vec<u64>,
}

impl<W: Write> Pdf<W> {
    fn raw(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.pos += b.len() as u64;
        self.out.write_all(b)
    }

    /// Start object `id` (1-based, any order).
    fn begin(&mut self, id: usize) -> std::io::Result<()> {
        if self.offsets.len() < id {
            self.offsets.resize(id, 0);
        }
        self.offsets[id - 1] = self.pos;
        self.raw(format!("{id} 0 obj\n").as_bytes())
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) -> std::io::Result<()> {
        self.begin(id)?;
        self.raw(format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes())?;
        self.raw(data)?;
        self.raw(b"\nendstream\nendobj\n")
    }
}

fn pdf_text(s: &str) -> String {
    if s.is_ascii() {
        format!("({})", s.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)"))
    } else {
        let mut hex = String::from("<FEFF");
        for u in s.encode_utf16() {
            hex += &format!("{u:04X}");
        }
        hex + ">"
    }
}

/// The page size in points for a picture of `w` x `h` pixels, and the rectangle (x, y, w, h) the picture fills on it.
pub fn layout(page: PdfPage, w: u32, h: u32) -> ((f32, f32), (f32, f32, f32, f32)) {
    match page {
        PdfPage::Fit => {
            let (pw, ph) = ((w as f32 * 72.0 / FIT_DPI).clamp(10.0, 14400.0), (h as f32 * 72.0 / FIT_DPI).clamp(10.0, 14400.0));
            ((pw, ph), (0.0, 0.0, pw, ph))
        }
        PdfPage::A4 | PdfPage::Letter => {
            let (a, b) = if page == PdfPage::A4 { (595.28f32, 841.89f32) } else { (612.0, 792.0) };
            let landscape = w > h;
            let (pw, ph) = if landscape { (b, a) } else { (a, b) };
            let m = 18.0;
            let k = ((pw - 2.0 * m) / w as f32).min((ph - 2.0 * m) / h as f32);
            let (iw, ih) = (w as f32 * k, h as f32 * k);
            ((pw, ph), ((pw - iw) / 2.0, (ph - ih) / 2.0, iw, ih))
        }
    }
}

/// What goes into the PDF for one picture.
struct Prepared {
    width: u32,
    height: u32,
    /// Image dictionary entries (colour space, filter) and the stream bytes.
    dict: String,
    data: Vec<u8>,
}

fn prepare(path: &Path, quality: u8) -> Result<Prepared, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "jpg" | "jpeg" | "jpe" | "jfif") {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if let Some(i) = nv_codecs::jpeg_info(&bytes) {
            if i.orientation == 1 && i.bits == 8 && (i.components == 1 || i.components == 3) {
                let cs = if i.components == 1 { "/DeviceGray" } else { "/DeviceRGB" };
                return Ok(Prepared { width: i.width, height: i.height, dict: format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode", i.width, i.height), data: bytes });
            }
        }
    }
    // Anything else (or a JPEG that needs turning / is CMYK): decode, upright, sRGB, flattened on white.
    let img = nv_codecs::decode_file(path)?.finish_for_cpu();
    let (w, h) = (img.width, img.height);
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for p in img.rgba.chunks_exact(4) {
        let a = p[3] as u32;
        for &c in &p[..3] {
            rgb.push(((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
        }
    }
    if (w as u64) * (h as u64) > LOSSLESS_MAX_PX {
        let jpg = nv_codecs::encode_jpeg_rgb(&rgb, w, h, quality)?;
        return Ok(Prepared { width: w, height: h, dict: format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"), data: jpg });
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::new(6));
    z.write_all(&rgb).map_err(|e| e.to_string())?;
    let data = z.finish().map_err(|e| e.to_string())?;
    Ok(Prepared { width: w, height: h, dict: format!("/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode"), data })
}

/// Write `dest` with one page per picture. Pictures that can't be read are skipped; the result says how many pages there are.
/// `progress(done, total)` returns false to stop (the half-made file is deleted).
pub fn make(dest: &Path, images: &[PathBuf], page: PdfPage, quality: u8, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<usize, String> {
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut pdf = Pdf { out: BufWriter::with_capacity(1 << 20, file), pos: 0, offsets: Vec::new() };
    let io = |e: std::io::Error| e.to_string();
    pdf.raw(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n").map_err(io)?;
    let (mut pages, mut next_id) = (Vec::new(), 4usize);
    for (n, path) in images.iter().enumerate() {
        if !progress(n, images.len()) {
            drop(pdf);
            let _ = std::fs::remove_file(dest);
            return Err("cancelled".into());
        }
        let Ok(p) = prepare(path, quality) else { continue };
        let ((pw, ph), (x, y, iw, ih)) = layout(page, p.width, p.height);
        let (page_id, content_id, image_id) = (next_id, next_id + 1, next_id + 2);
        next_id += 3;
        pdf.stream(image_id, &p.dict, &p.data).map_err(io)?;
        let content = format!("q {iw:.2} 0 0 {ih:.2} {x:.2} {y:.2} cm /Im0 Do Q");
        pdf.stream(content_id, "", content.as_bytes()).map_err(io)?;
        pdf.begin(page_id).map_err(io)?;
        pdf.raw(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>\nendobj\n").as_bytes()).map_err(io)?;
        pages.push(page_id);
    }
    if pages.is_empty() {
        drop(pdf);
        let _ = std::fs::remove_file(dest);
        return Err("none of the pictures could be read".into());
    }
    pdf.begin(1).map_err(io)?;
    pdf.raw(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n").map_err(io)?;
    pdf.begin(2).map_err(io)?;
    let kids: Vec<String> = pages.iter().map(|i| format!("{i} 0 R")).collect();
    pdf.raw(format!("<< /Type /Pages /Count {} /Kids [{}] >>\nendobj\n", pages.len(), kids.join(" ")).as_bytes()).map_err(io)?;
    pdf.begin(3).map_err(io)?;
    let title = dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    pdf.raw(format!("<< /Producer (NuGallery) /Title {} >>\nendobj\n", pdf_text(&title)).as_bytes()).map_err(io)?;
    let xref = pdf.pos;
    let count = pdf.offsets.len() + 1;
    let mut table = format!("xref\n0 {count}\n0000000000 65535 f \n");
    for off in pdf.offsets.clone() {
        table += &format!("{off:010} 00000 n \n");
    }
    pdf.raw(table.as_bytes()).map_err(io)?;
    pdf.raw(format!("trailer\n<< /Size {count} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes()).map_err(io)?;
    pdf.out.flush().map_err(io)?;
    progress(images.len(), images.len());
    Ok(pages.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_layouts() {
        let ((w, h), (x, y, iw, ih)) = layout(PdfPage::Fit, 1500, 1000);
        assert_eq!((w, h, x, y, iw, ih), (720.0, 480.0, 0.0, 0.0, 720.0, 480.0));
        let ((w, h), (x, _, iw, _)) = layout(PdfPage::A4, 2000, 1000); // wide picture: landscape page
        assert!(w > h && x >= 18.0 - 0.01 && iw <= w - 36.0 + 0.01);
        let ((w, h), _) = layout(PdfPage::Letter, 1000, 2000);
        assert!(h > w);
    }

    /// Make a PDF from a PNG and a JPEG and let Windows' own PDF reader draw page 1 of it.
    #[test]
    fn windows_can_read_what_we_write() {
        let dir = std::env::temp_dir().join(format!("nv_pdf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let rgb: Vec<u8> = (0..64 * 48).flat_map(|i| [(i % 64 * 4) as u8, 80, 200]).collect();
        let jpg = nv_codecs::encode_jpeg_rgb(&rgb, 64, 48, 90).unwrap();
        std::fs::write(dir.join("a.jpg"), &jpg).unwrap();
        let mut png = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png, 40, 30);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&vec![120u8; 40 * 30 * 3]).unwrap();
        }
        std::fs::write(dir.join("b.png"), &png).unwrap();
        let out = dir.join("t.pdf");
        let n = make(&out, &[dir.join("a.jpg"), dir.join("b.png"), dir.join("missing.png")], PdfPage::Fit, 88, &mut |_, _| true).unwrap();
        assert_eq!(n, 2);
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"%PDF-1.4") && bytes.ends_with(b"%%EOF\n"));
        // The JPEG is in the file unchanged.
        assert!(bytes.windows(jpg.len()).any(|w| w == &jpg[..]));
        let img = nv_codecs::decode_file(&out).expect("Windows reads the PDF");
        assert!(img.width > 0 && img.height > 0);
        assert!((img.width as f32 / img.height as f32 - 64.0 / 48.0).abs() < 0.05, "page 1 has the JPEG's shape: {}x{}", img.width, img.height);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
