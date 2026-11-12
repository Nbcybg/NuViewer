//! EPS (and Illustrator `.ai` files saved without PDF compatibility, which are PostScript too).
//!
//! PostScript can't be interpreted in-process, so, in order:
//! 1. Ghostscript, if it is installed (found on PATH or under `Program Files\gs`): a real rendering at screen size.
//! 2. The preview stored inside the file: DOS-EPS binary header (TIFF or WMF preview) or an ASCII `%%BeginPreview` bitmap.
//! 3. Otherwise an error that says so.

use crate::Image;
use std::path::{Path, PathBuf};

pub fn decode(path: &Path, thumb: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    if let Some(gs) = find_ghostscript() {
        match render_with_ghostscript(&gs, path, &bytes, thumb) {
            Ok(img) => return Ok(img),
            Err(_) => {} // fall back to the embedded preview
        }
    }
    embedded_preview(&bytes, thumb)
}

// ---- Ghostscript ---------------------------------------------------------------------------------

fn find_ghostscript() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NUVIEWER_GS").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let names = ["gswin64c.exe", "gswin32c.exe"];
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in names {
                let c = dir.join(n);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        let Some(base) = std::env::var_os(var).map(|b| PathBuf::from(b).join("gs")) else { continue };
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut versions: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        versions.sort();
        for v in versions.iter().rev() {
            for n in names {
                let c = v.join("bin").join(n);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    None
}

/// Size of the drawing in points from `%%BoundingBox` (or `%%HiResBoundingBox`) in the first 64 KB.
fn bounding_box_size(bytes: &[u8]) -> Option<(f64, f64)> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(65536)]);
    for key in ["%%HiResBoundingBox:", "%%BoundingBox:"] {
        if let Some(pos) = head.find(key) {
            let nums: Vec<f64> = head[pos + key.len()..].lines().next()?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
            if nums.len() >= 4 && nums[2] > nums[0] && nums[3] > nums[1] {
                return Some((nums[2] - nums[0], nums[3] - nums[1]));
            }
        }
    }
    None
}

fn render_with_ghostscript(gs: &Path, path: &Path, bytes: &[u8], thumb: bool) -> Result<Image, String> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let long = if thumb { 640.0 } else { crate::render_long_side() as f64 };
    let (w, h) = bounding_box_size(bytes).unwrap_or((595.0, 842.0));
    let dpi = (72.0 * long / w.max(h)).clamp(20.0, 1200.0).round();
    let mut child = Command::new(gs)
        .args(["-q", "-dSAFER", "-dBATCH", "-dNOPAUSE", "-dEPSCrop", "-sDEVICE=pngalpha"])
        .arg(format!("-r{dpi}"))
        .arg("-sOutputFile=-")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|e| format!("ghostscript: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ghostscript: no output")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    // A hostile or broken file must not hang the decode pool: give it 30 seconds.
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed().as_secs() < 30 => std::thread::sleep(std::time::Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let png = reader.join().map_err(|_| "ghostscript: output thread failed")?;
    if png.is_empty() {
        return Err("ghostscript: nothing rendered".into());
    }
    let mut img = crate::pngdec::decode(&png, true)?;
    flatten_on_white(&mut img.rgba);
    Ok(img)
}

fn flatten_on_white(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            p[3] = 255;
        }
    }
}

// ---- embedded previews ---------------------------------------------------------------------------

fn embedded_preview(bytes: &[u8], thumb: bool) -> Result<Image, String> {
    if bytes.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) && bytes.len() >= 30 {
        let u32at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]) as usize;
        let slice = |off: usize, len: usize| (len > 0 && off.checked_add(len).map(|e| e <= bytes.len()).unwrap_or(false)).then(|| &bytes[off..off + len]);
        if let Some(tiff) = slice(u32at(20), u32at(24)) {
            if let Ok(mut img) = crate::wic::decode_bytes(tiff) {
                flatten_on_white(&mut img.rgba);
                return Ok(img);
            }
        }
        if let Some(wmf) = slice(u32at(12), u32at(16)) {
            let tmp = std::env::temp_dir().join(format!("nv_eps_preview_{}_{:x}.wmf", std::process::id(), bytes.len()));
            if std::fs::write(&tmp, wmf).is_ok() {
                let r = crate::metafile::decode_file(&tmp, thumb);
                let _ = std::fs::remove_file(&tmp);
                if let Ok(img) = r {
                    return Ok(img);
                }
            }
        }
    }
    if let Some(img) = ascii_preview(bytes) {
        return Ok(img);
    }
    Err("eps: this file has no embedded preview and Ghostscript isn't installed (install Ghostscript to render EPS / old .ai files)".into())
}

/// `%%BeginPreview: w h depth lines` followed by `% <hex>` lines (EPSI). 0 = black, like PostScript's `image`.
fn ascii_preview(bytes: &[u8]) -> Option<Image> {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4 * 1024 * 1024)]);
    let start = text.find("%%BeginPreview:")?;
    let mut lines = text[start..].lines();
    let head: Vec<u32> = lines.next()?["%%BeginPreview:".len()..].split_whitespace().filter_map(|t| t.parse().ok()).collect();
    let [w, h, depth] = [*head.first()?, *head.get(1)?, *head.get(2)?];
    if w == 0 || h == 0 || w > 4096 || h > 4096 || ![1, 2, 4, 8].contains(&depth) {
        return None;
    }
    let mut data = Vec::new();
    let mut pending: Option<u8> = None;
    for line in lines {
        if line.starts_with("%%EndPreview") {
            break;
        }
        let Some(hex) = line.strip_prefix('%') else { continue };
        for c in hex.chars().filter_map(|c| c.to_digit(16)) {
            match pending.take() {
                Some(hi) => data.push(hi << 4 | c as u8),
                None => pending = Some(c as u8),
            }
        }
    }
    let row_bytes = ((w * depth + 7) / 8) as usize;
    if data.len() < row_bytes * h as usize {
        return None;
    }
    let max = (1u32 << depth) - 1;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row = &data[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..w as usize {
            let bit = x * depth as usize;
            let v = ((row[bit / 8] >> (8 - depth as usize - (bit % 8))) as u32) & max;
            let g = (v * 255 / max) as u8;
            rgba.extend_from_slice(&[g, g, g, 255]);
        }
    }
    Some(Image::new(w, h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_preview_is_decoded() {
        // 4x2, 1 bit: row 0 = 1010 (white black white black), row 1 = 0101.
        let eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 4 2\n%%BeginPreview: 4 2 1 2\n% A0\n% 50\n%%EndPreview\nshowpage\n";
        let img = ascii_preview(eps).unwrap();
        assert_eq!((img.width, img.height), (4, 2));
        let v = |x: usize, y: usize| img.rgba[(y * 4 + x) * 4];
        assert_eq!([v(0, 0), v(1, 0), v(2, 0), v(3, 0)], [255, 0, 255, 0]);
        assert_eq!([v(0, 1), v(1, 1), v(2, 1), v(3, 1)], [0, 255, 0, 255]);
        assert_eq!(bounding_box_size(eps), Some((4.0, 2.0)));
    }

    #[test]
    fn eight_bit_preview_keeps_the_grey_levels() {
        let eps = b"%%BeginPreview: 2 1 8 1\n% 0080\n%%EndPreview\n";
        let img = ascii_preview(eps).unwrap();
        assert_eq!((img.rgba[0], img.rgba[4]), (0, 128));
    }

    /// A tiny uncompressed TIFF, 2x1 RGB: red, green.
    fn tiny_tiff() -> Vec<u8> {
        let mut t = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        let entries: [(u16, u16, u32, u32); 9] = [
            (256, 3, 1, 2),   // width
            (257, 3, 1, 1),   // height
            (258, 3, 3, 122), // bits per sample -> offset of 3 shorts
            (259, 3, 1, 1),   // no compression
            (262, 3, 1, 2),   // RGB
            (273, 4, 1, 128), // strip offset
            (277, 3, 1, 3),   // samples per pixel
            (278, 3, 1, 1),   // rows per strip
            (279, 4, 1, 6),   // strip byte count
        ];
        t.extend(9u16.to_le_bytes());
        for (tag, ty, count, value) in entries {
            t.extend(tag.to_le_bytes());
            t.extend(ty.to_le_bytes());
            t.extend(count.to_le_bytes());
            t.extend(value.to_le_bytes());
        }
        t.extend(0u32.to_le_bytes()); // no next IFD
        while t.len() < 122 {
            t.push(0);
        }
        for _ in 0..3 {
            t.extend(8u16.to_le_bytes());
        }
        while t.len() < 128 {
            t.push(0);
        }
        t.extend([255, 0, 0, 0, 255, 0]);
        t
    }

    #[test]
    fn dos_eps_with_a_tiff_preview() {
        let tiff = tiny_tiff();
        let ps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 2 1\n";
        let mut eps = vec![0xC5, 0xD0, 0xD3, 0xC6];
        let ps_off = 30u32;
        let tiff_off = ps_off + ps.len() as u32;
        for v in [ps_off, ps.len() as u32, 0, 0, tiff_off, tiff.len() as u32] {
            eps.extend(v.to_le_bytes());
        }
        eps.extend([0xFF, 0xFF]); // checksum
        eps.extend(ps);
        eps.extend(&tiff);
        let img = embedded_preview(&eps, true).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn no_preview_gives_a_helpful_error() {
        let e = embedded_preview(b"%!PS-Adobe-3.0 EPSF-3.0\nnewpath 0 0 moveto showpage\n", true).err().unwrap();
        assert!(e.contains("Ghostscript"), "{e}");
    }
}
