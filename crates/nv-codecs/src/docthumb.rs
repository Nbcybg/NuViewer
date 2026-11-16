//! Pictures for files that are not pictures, for the gallery's thumbnails (and the viewer, which shows the same picture):
//!
//!  * ZIP-based files, **one reader for all of them**: `.zip` / `.cbz` (the first page: the first image in natural name
//!    order) and `.epub` (the cover), Office (`.docx` `.xlsx` `.pptx` ...: the thumbnail Office saved in `docProps`, else the
//!    biggest picture inside), OpenDocument (`.odt` `.ods` `.odp` ...: `Thumbnails/thumbnail.png`, else the biggest picture),
//!  * Affinity files (`.afphoto` `.afdesign` `.afpub`): the preview PNG embedded near the start, when there is one,
//!  * text (`.txt` `.md` `.log` `.csv`): a card showing the first lines,
//!  * everything else that can't give a picture: a card with a badge (the extension), the name and the size.
//!
//! The ZIP reader reads the end of the file (the central directory) and the one entry it needs, not the whole archive, so
//! a 2 GB comic archive costs a few kilobytes of reading.

use crate::Image;
use std::cmp::Ordering;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const ARCHIVE_EXTS: &[&str] = &["zip", "cbz", "epub"];
/// Archives that get a card (NuGallery opens them as folders through 7-Zip or Windows' tar).
pub const OTHER_ARCHIVE_EXTS: &[&str] = &["7z", "cb7", "rar", "cbr", "tar", "tgz", "gz", "bz2", "xz", "zst"];
pub const OFFICE_EXTS: &[&str] = &["docx", "docm", "dotx", "xlsx", "xlsm", "xltx", "pptx", "pptm", "potx", "ppsx"];
pub const OPENDOC_EXTS: &[&str] = &["odt", "ods", "odp", "odg", "ott", "ots", "otp"];
pub const AFFINITY_EXTS: &[&str] = &["afphoto", "afdesign", "afpub", "aftemplate"];
pub const TEXT_EXTS: &[&str] = &["txt", "md", "log", "csv"];

/// Is this a kind of file the gallery can make a picture for (without it being a picture format)?
pub fn is_document_ext(ext: &str) -> bool {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    ARCHIVE_EXTS.contains(&e) || OTHER_ARCHIVE_EXTS.contains(&e) || OFFICE_EXTS.contains(&e) || OPENDOC_EXTS.contains(&e) || AFFINITY_EXTS.contains(&e) || TEXT_EXTS.contains(&e) || crate::codepage::is_code_ext(e)
}

// ------------------------------------------------------------------------------------------------ the ZIP reader

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub name: String,
    method: u16,
    csize: u64,
    pub size: u64,
    offset: u64,
}

pub(crate) struct Zip {
    file: File,
    pub entries: Vec<Entry>,
}

fn le16(b: &[u8], i: usize) -> Option<u16> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn le64(b: &[u8], i: usize) -> Option<u64> {
    b.get(i..i + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}

impl Zip {
    /// Open an archive and read its table of contents (central directory), including ZIP64 archives.
    pub fn open(path: &Path) -> Result<Zip, String> {
        let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        // End of central directory record: within the last 64 KB + 22 bytes (the comment can be that long).
        let tail_len = len.min(66_000) as usize;
        let mut tail = vec![0u8; tail_len];
        file.seek(SeekFrom::Start(len - tail_len as u64)).map_err(|e| e.to_string())?;
        file.read_exact(&mut tail).map_err(|e| e.to_string())?;
        let eocd = (0..tail_len.saturating_sub(21)).rev().find(|&i| tail[i..i + 4] == *b"PK\x05\x06").ok_or("not a zip archive")?;
        let (mut count, mut cd_size, mut cd_off) = (le16(&tail, eocd + 10).ok_or("short")? as u64, le32(&tail, eocd + 12).ok_or("short")? as u64, le32(&tail, eocd + 16).ok_or("short")? as u64);
        // ZIP64: the locator just before the EOCD points at a bigger record that holds the real numbers.
        if (count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_off == 0xFFFF_FFFF) && eocd >= 20 && tail[eocd - 20..eocd - 16] == *b"PK\x06\x07" {
            let rec = le64(&tail, eocd - 20 + 8).ok_or("short")?;
            let mut z = [0u8; 56];
            file.seek(SeekFrom::Start(rec)).map_err(|e| e.to_string())?;
            file.read_exact(&mut z).map_err(|e| e.to_string())?;
            if z[..4] != *b"PK\x06\x06" {
                return Err("damaged zip64 record".into());
            }
            count = le64(&z, 32).ok_or("short")?;
            cd_size = le64(&z, 40).ok_or("short")?;
            cd_off = le64(&z, 48).ok_or("short")?;
        }
        if cd_size > 256 << 20 || cd_off.checked_add(cd_size).map(|e| e > len).unwrap_or(true) {
            return Err("damaged central directory".into());
        }
        let mut cd = vec![0u8; cd_size as usize];
        file.seek(SeekFrom::Start(cd_off)).map_err(|e| e.to_string())?;
        file.read_exact(&mut cd).map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        let mut at = 0usize;
        while at + 46 <= cd.len() && entries.len() < 1_000_000 && (entries.len() as u64) < count.max(1) * 2 {
            if cd[at..at + 4] != *b"PK\x01\x02" {
                break;
            }
            let (flags, method) = (le16(&cd, at + 8).unwrap(), le16(&cd, at + 10).unwrap());
            let (mut csize, mut size, mut offset) = (le32(&cd, at + 20).unwrap() as u64, le32(&cd, at + 24).unwrap() as u64, le32(&cd, at + 42).unwrap() as u64);
            let (nlen, xlen, clen) = (le16(&cd, at + 28).unwrap() as usize, le16(&cd, at + 30).unwrap() as usize, le16(&cd, at + 32).unwrap() as usize);
            let name_bytes = cd.get(at + 46..at + 46 + nlen).ok_or("damaged entry")?;
            let extra = cd.get(at + 46 + nlen..at + 46 + nlen + xlen).ok_or("damaged entry")?;
            // The ZIP64 extra field carries the values that didn't fit in 32 bits, in this order, only for those that did not.
            let mut x = 0;
            while x + 4 <= extra.len() {
                let (id, sz) = (le16(extra, x).unwrap(), le16(extra, x + 2).unwrap() as usize);
                if id == 1 {
                    let mut p = x + 4;
                    for field in [&mut size, &mut csize, &mut offset] {
                        if *field == 0xFFFF_FFFF {
                            if let Some(v) = le64(extra, p) {
                                *field = v;
                                p += 8;
                            }
                        }
                    }
                }
                x += 4 + sz;
            }
            let name = if flags & 0x800 != 0 { String::from_utf8_lossy(name_bytes).into_owned() } else { name_bytes.iter().map(|&b| if b < 128 { b as char } else { '?' }).collect() };
            if !name.ends_with('/') {
                entries.push(Entry { name: name.replace('\\', "/"), method, csize, size, offset });
            }
            at += 46 + nlen + xlen + clen;
        }
        Ok(Zip { file, entries })
    }

    /// The contents of one entry (stored or deflated), refusing anything bigger than `limit` bytes once unpacked.
    pub fn read(&mut self, e: &Entry, limit: u64) -> Result<Vec<u8>, String> {
        if e.size > limit || e.csize > limit {
            return Err("entry too large".into());
        }
        let mut head = [0u8; 30];
        self.file.seek(SeekFrom::Start(e.offset)).map_err(|er| er.to_string())?;
        self.file.read_exact(&mut head).map_err(|er| er.to_string())?;
        if head[..4] != *b"PK\x03\x04" {
            return Err("damaged local header".into());
        }
        let skip = le16(&head, 26).unwrap() as u64 + le16(&head, 28).unwrap() as u64;
        self.file.seek(SeekFrom::Start(e.offset + 30 + skip)).map_err(|er| er.to_string())?;
        let mut raw = vec![0u8; e.csize as usize];
        self.file.read_exact(&mut raw).map_err(|er| er.to_string())?;
        match e.method {
            0 => Ok(raw),
            8 => {
                let mut out = Vec::with_capacity(e.size as usize);
                flate2::read::DeflateDecoder::new(&raw[..]).take(limit + 1).read_to_end(&mut out).map_err(|er| format!("inflate: {er}"))?;
                if out.len() as u64 > limit {
                    return Err("entry too large".into());
                }
                Ok(out)
            }
            m => Err(format!("compression method {m} is not supported")),
        }
    }
}

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "png", "webp", "gif", "bmp", "jxl", "avif", "tif", "tiff", "jp2"];

pub(crate) fn ext_of(name: &str) -> String {
    name.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub(crate) fn is_image_name(name: &str) -> bool {
    let file = name.rsplit('/').next().unwrap_or(name);
    IMAGE_EXTS.contains(&ext_of(name).as_str()) && !file.starts_with('.') && !name.starts_with("__MACOSX/")
}

/// Natural order: `page2` before `page10`, letter case ignored.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                let mut nb = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// The entries of an archive that could be the picture for it, best first.
pub(crate) fn candidates(entries: &[Entry], ext: &str) -> Vec<usize> {
    let find = |name: &str| entries.iter().position(|e| e.name.eq_ignore_ascii_case(name));
    let mut out = Vec::new();
    if OFFICE_EXTS.contains(&ext) {
        // Office writes its thumbnail here (when "save thumbnail" is on); relationships name it, but this is where it is.
        for n in ["docProps/thumbnail.jpeg", "docProps/thumbnail.jpg", "docProps/thumbnail.png"] {
            out.extend(find(n));
        }
        // Otherwise the biggest picture inside the document.
        let mut media: Vec<usize> = (0..entries.len()).filter(|&i| is_image_name(&entries[i].name) && ["word/media/", "ppt/media/", "xl/media/"].iter().any(|p| entries[i].name.to_ascii_lowercase().starts_with(p))).collect();
        media.sort_by(|&a, &b| entries[b].size.cmp(&entries[a].size));
        out.extend(media);
    } else if OPENDOC_EXTS.contains(&ext) {
        out.extend(find("Thumbnails/thumbnail.png"));
        let mut pics: Vec<usize> = (0..entries.len()).filter(|&i| is_image_name(&entries[i].name) && entries[i].name.to_ascii_lowercase().starts_with("pictures/")).collect();
        pics.sort_by(|&a, &b| entries[b].size.cmp(&entries[a].size));
        out.extend(pics);
    } else {
        // Archives of pictures: the first page. An e-book: its cover if it is named so.
        let mut pages: Vec<usize> = (0..entries.len()).filter(|&i| is_image_name(&entries[i].name)).collect();
        pages.sort_by(|&a, &b| natural_cmp(&entries[a].name, &entries[b].name));
        if ext == "epub" {
            out.extend(pages.iter().copied().filter(|&i| entries[i].name.to_ascii_lowercase().contains("cover")));
        }
        out.extend(pages);
    }
    out.dedup();
    out.truncate(4);
    out
}

fn picture_from_zip(path: &Path, ext: &str, limit: u64) -> Result<(Image, usize, String), String> {
    let mut zip = Zip::open(path)?;
    let total = zip.entries.len();
    let list = candidates(&zip.entries, ext);
    if list.is_empty() {
        return Err(format!("{total} file{}, no picture among {}", if total == 1 { "" } else { "s" }, if total == 1 { "it" } else { "them" }));
    }
    let mut last = String::new();
    for i in list {
        let e = zip.entries[i].clone();
        match zip.read(&e, limit).and_then(|bytes| crate::decode_image_bytes(&bytes, &ext_of(&e.name))) {
            Ok(img) => return Ok((img, total, e.name)),
            Err(er) => last = er,
        }
    }
    Err(last)
}

// ------------------------------------------------------------------------------------------------ Affinity

/// The first PNG inside the first 8 MB of an Affinity file: its preview.
fn affinity_preview(path: &Path) -> Option<Image> {
    let mut head = Vec::new();
    File::open(path).ok()?.take(8 << 20).read_to_end(&mut head).ok()?;
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mut from = 0;
    while let Some(p) = head[from..].windows(8).position(|w| w == SIG) {
        let start = from + p;
        // Walk the chunks to IEND so the PNG is cut out exactly.
        let mut at = start + 8;
        let mut end = None;
        while at + 12 <= head.len() {
            let len = u32::from_be_bytes(head[at..at + 4].try_into().ok()?) as usize;
            let kind = &head[at + 4..at + 8];
            at = at.checked_add(12 + len)?;
            if kind == b"IEND" {
                end = Some(at);
                break;
            }
        }
        if let Some(e) = end.filter(|&e| e <= head.len()) {
            if let Ok(img) = crate::decode_image_bytes(&head[start..e], "png") {
                return Some(img);
            }
        }
        from = start + 8;
    }
    None
}

// ------------------------------------------------------------------------------------------------ text

/// The start of a text file as text: UTF-8, UTF-16 with a byte order mark, else Latin-1-ish.
fn read_text_head(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    File::open(path).ok()?.take(4096).read_to_end(&mut buf).ok()?;
    if buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF]) {
        let be = buf[0] == 0xFE;
        let units: Vec<u16> = buf[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let buf = buf.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&buf);
    // A cut in the middle of a multi-byte character is not an error.
    match std::str::from_utf8(buf) {
        Ok(s) => Some(s.to_string()),
        Err(e) if e.error_len().is_none() => Some(String::from_utf8_lossy(&buf[..e.valid_up_to()]).into_owned()),
        Err(_) => Some(buf.iter().map(|&b| b as char).collect()),
    }
}

/// Non-empty lines of `text`, tabs turned into spaces, control characters dropped.
pub(crate) fn text_lines(text: &str, max: usize) -> Vec<String> {
    text.lines().map(|l| l.replace('\t', "    ").chars().filter(|c| !c.is_control()).collect::<String>()).map(|l| l.trim_end().to_string()).filter(|l| !l.trim().is_empty()).take(max).collect()
}

// ------------------------------------------------------------------------------------------------ cards

pub(crate) fn fmt_bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} MB", n as f64 / 1_048_576.0),
        _ => format!("{:.2} GB", n as f64 / 1_073_741_824.0),
    }
}

/// What a card says.
pub(crate) struct Card {
    /// The extension, big, in a coloured band.
    pub badge: String,
    pub accent: u32,
    /// The file name.
    pub title: String,
    /// Size and what is known about the contents.
    pub subtitle: String,
    /// A page of text: the lines shown on a paper-coloured card (portrait). Empty = the dark card with name and size.
    pub lines: Vec<String>,
}

pub(crate) fn accent_for(ext: &str) -> u32 {
    match ext {
        "zip" | "cbz" => 0xD9A400,
        "epub" => 0x6E8B3D,
        "html" | "htm" | "xhtml" | "mhtml" | "mht" => 0xE34F26,
        "swf" | "fla" | "xfl" => 0xC8272D,
        "docx" | "docm" | "dotx" => 0x2B579A,
        "xlsx" | "xlsm" | "xltx" => 0x217346,
        "pptx" | "pptm" | "potx" | "ppsx" => 0xD24726,
        e if OPENDOC_EXTS.contains(&e) => 0x18A303,
        e if AFFINITY_EXTS.contains(&e) => 0x7B4FD0,
        e if TEXT_EXTS.contains(&e) => 0x6B7280,
        _ => 0x5A6270,
    }
}

/// Draw a card (`scale` 1 = 256 x 192, or 192 x 256 for a text page; 2 = twice that, for the viewer).
#[cfg(windows)]
pub(crate) fn render_card(c: &Card, scale: i32) -> Image {
    use std::ffi::c_void;
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::*;

    let s = scale.max(1);
    let page = !c.lines.is_empty();
    let (w, h) = if page { (192 * s, 256 * s) } else { (256 * s, 192 * s) };
    let bgr = |rgb: u32| COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF));
    let font = |px: i32, weight: i32| unsafe { CreateFontW(-px * s, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI")) };
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let dc = CreateCompatibleDC(None);
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        if let (Ok(dib), false) = (dib, bits.is_null()) {
            let old_bmp = SelectObject(dc, dib.into());
            let fill = |r: RECT, rgb: u32| {
                let b = CreateSolidBrush(bgr(rgb));
                FillRect(dc, &r, b);
                let _ = DeleteObject(b.into());
            };
            let text = |t: &str, mut r: RECT, f: HFONT, rgb: u32, flags: DRAW_TEXT_FORMAT| {
                let old = SelectObject(dc, f.into());
                SetTextColor(dc, bgr(rgb));
                let mut wide: Vec<u16> = t.encode_utf16().collect();
                DrawTextW(dc, &mut wide, &mut r, flags | DT_NOPREFIX);
                SelectObject(dc, old);
            };
            SetBkMode(dc, TRANSPARENT);
            let pad = 12 * s;
            if page {
                fill(RECT { left: 0, top: 0, right: w, bottom: h }, 0xF6F5F0);
                fill(RECT { left: 0, top: 0, right: w, bottom: 8 * s }, c.accent);
                let (f_title, f_body, f_small) = (font(17, 700), font(12, 400), font(11, 600));
                let mut y = 20 * s;
                for (i, line) in c.lines.iter().enumerate() {
                    let (f, px, colour) = if i == 0 { (f_title, 17, 0x1B1B1B) } else { (f_body, 12, 0x4A4A4A) };
                    let lh = if i == 0 { 22 * s } else { 15 * s };
                    let mut rect = RECT { left: pad, top: y, right: w - pad, bottom: (h - 32 * s).min(y + 2 * lh) };
                    if i == 0 {
                        rect.bottom = (y + 3 * lh).min(h - 32 * s);
                    }
                    if rect.bottom - rect.top < lh {
                        break;
                    }
                    let flags = if i == 0 { DT_WORDBREAK | DT_END_ELLIPSIS } else { DT_SINGLELINE | DT_END_ELLIPSIS };
                    text(line, rect, f, colour, flags);
                    y += if i == 0 { ((rect.bottom - rect.top) / lh).max(1) * lh + 6 * s } else { lh };
                    let _ = px;
                }
                text(&c.badge, RECT { left: pad, top: h - 26 * s, right: w / 2, bottom: h - 6 * s }, f_small, c.accent, DT_SINGLELINE | DT_VCENTER);
                text(&c.subtitle, RECT { left: w / 2, top: h - 26 * s, right: w - pad, bottom: h - 6 * s }, f_small, 0x777777, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_END_ELLIPSIS);
                for f in [f_title, f_body, f_small] {
                    let _ = DeleteObject(f.into());
                }
            } else {
                fill(RECT { left: 0, top: 0, right: w, bottom: h }, 0x25272B);
                fill(RECT { left: 0, top: 0, right: w, bottom: 58 * s }, c.accent);
                let (f_badge, f_title, f_small) = (font(30, 700), font(16, 600), font(13, 400));
                text(&c.badge, RECT { left: pad, top: 0, right: w - pad, bottom: 58 * s }, f_badge, 0xFFFFFF, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                text(&c.title, RECT { left: pad, top: 70 * s, right: w - pad, bottom: h - 40 * s }, f_title, 0xF2F2F2, DT_WORDBREAK | DT_END_ELLIPSIS);
                text(&c.subtitle, RECT { left: pad, top: h - 34 * s, right: w - pad, bottom: h - 8 * s }, f_small, 0xA5ABB3, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                for f in [f_badge, f_title, f_small] {
                    let _ = DeleteObject(f.into());
                }
            }
            let _ = GdiFlush();
            let src = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
            for (d, p) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d.copy_from_slice(&[p[2], p[1], p[0], 255]); // BGRA -> RGBA, opaque
            }
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(dib.into());
        }
        let _ = DeleteDC(dc);
    }
    Image::new(w as u32, h as u32, rgba)
}

/// Without GDI: a flat card in the accent colour (no text).
#[cfg(not(windows))]
pub(crate) fn render_card(c: &Card, scale: i32) -> Image {
    let s = scale.max(1) as u32;
    let (w, h) = if c.lines.is_empty() { (256 * s, 192 * s) } else { (192 * s, 256 * s) };
    let px = [(c.accent >> 16) as u8, (c.accent >> 8) as u8, c.accent as u8, 255];
    Image::new(w, h, px.iter().copied().cycle().take((w * h * 4) as usize).collect())
}

/// Every picture stored in a zip-based file (Office, OpenDocument, zip / cbz / epub ...): name and bytes, in the archive's order.
pub(crate) fn pictures_in_zip(path: &Path, limit: usize) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut zip = Zip::open(path)?;
    let entries: Vec<Entry> = zip.entries.clone();
    let mut out = Vec::new();
    for e in entries {
        if out.len() >= limit {
            break;
        }
        let n = e.name.to_ascii_lowercase();
        let is_pic = !n.contains("__macosx/") && matches!(ext_of(&n).as_str(), "jpg" | "jpeg" | "jpe" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "jxl" | "avif" | "heic" | "emf" | "wmf" | "svg");
        if !is_pic || e.size == 0 {
            continue;
        }
        if let Ok(bytes) = zip.read(&e, 100 << 20) {
            out.push((e.name.rsplit('/').next().unwrap_or(&e.name).to_string(), bytes));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ entry point

/// A picture for `path`: what is inside it if that is a picture, else a card. Never fails for a readable file.
/// `first_only` = a thumbnail (cards are drawn at 1x, the picture inside is as big as it is); otherwise the viewer's.
pub(crate) fn decode(path: &Path, first_only: bool) -> Result<Image, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Data and configuration text (JSON, XML, YAML ...): a coloured page of it.
    if crate::codepage::is_code_ext(&ext) {
        return crate::codepage::decode(path, first_only);
    }
    let size = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?.len();
    let limit: u64 = if first_only { 64 << 20 } else { 256 << 20 };
    let scale = if first_only { 1 } else { 2 };
    let mut note = String::new();
    let mut about = String::new();

    if ARCHIVE_EXTS.contains(&ext.as_str()) || OFFICE_EXTS.contains(&ext.as_str()) || OPENDOC_EXTS.contains(&ext.as_str()) {
        match picture_from_zip(path, &ext, limit) {
            Ok((mut img, total, entry)) => {
                img.colour_note = Some(format!("{} preview: {entry} (of {total} files in the archive)", ext.to_uppercase()));
                img.profile_assumable = true;
                return Ok(img);
            }
            Err(e) => about = e,
        }
    } else if AFFINITY_EXTS.contains(&ext.as_str()) {
        match affinity_preview(path) {
            Some(mut img) => {
                img.colour_note = Some("Affinity preview embedded in the file".into());
                img.profile_assumable = true;
                return Ok(img);
            }
            None => about = "no preview inside".into(),
        }
    } else if TEXT_EXTS.contains(&ext.as_str()) {
        if let Some(text) = read_text_head(path) {
            let lines = text_lines(&text, 14);
            if !lines.is_empty() {
                let mut img = render_card(&Card { badge: ext.to_uppercase(), accent: accent_for(&ext), title: name, subtitle: fmt_bytes(size), lines }, scale);
                img.colour_note = Some("a card made from the first lines of the text".into());
                return Ok(img);
            }
            about = "empty".into();
        }
    }
    if !about.is_empty() {
        note = format!("no picture: {about}");
    }
    let subtitle = if about.is_empty() { fmt_bytes(size) } else { format!("{}  ·  {about}", fmt_bytes(size)) };
    let mut img = render_card(&Card { badge: ext.to_uppercase(), accent: accent_for(&ext), title: name, subtitle, lines: Vec::new() }, scale);
    img.colour_note = Some(if note.is_empty() { "a card: no picture in this file".to_string() } else { format!("a card ({note})") });
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A zip file with the given (name, bytes, deflate?) entries, written the plain way (no zip64, no data descriptors).
    fn make_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data, deflate) in entries {
            let packed: Vec<u8> = if *deflate {
                let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(data).unwrap();
                enc.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend(b"PK\x03\x04");
            out.extend([20u8, 0, 0, 0]); // version, flags
            out.extend(method.to_le_bytes());
            out.extend([0u8; 4]); // time, date
            out.extend(0u32.to_le_bytes()); // crc (not checked)
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(&packed);
            central.extend(b"PK\x01\x02");
            central.extend([20u8, 0, 20, 0, 0, 0]);
            central.extend(method.to_le_bytes());
            central.extend([0u8; 4]);
            central.extend(0u32.to_le_bytes());
            central.extend((packed.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0u8; 8]); // extra length, comment length, disk number, internal attributes
            central.extend(0u32.to_le_bytes()); // external attrs
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend(&central);
        out.extend(b"PK\x05\x06");
        out.extend([0u8; 4]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(cd_off.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&rgb.iter().copied().cycle().take((w * h * 3) as usize).collect::<Vec<u8>>()).unwrap();
        drop(wr);
        out
    }

    fn jpeg(w: u16, h: u16, rgb: [u8; 3]) -> Vec<u8> {
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 95).encode(&rgb.iter().copied().cycle().take(w as usize * h as usize * 3).collect::<Vec<u8>>(), w, h, jpeg_encoder::ColorType::Rgb).unwrap();
        out
    }

    fn temp(tag: &str, ext: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("nv_doc_{tag}_{}.{ext}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn first_pixel(img: &Image) -> [u8; 3] {
        [img.rgba[0], img.rgba[1], img.rgba[2]]
    }

    #[test]
    fn a_comic_archive_gives_its_first_page_in_natural_order() {
        let zip = make_zip(&[("page10.png", &png(8, 6, [200, 0, 0]), true), ("page2.png", &png(8, 6, [0, 200, 0]), false), ("__MACOSX/page1.png", &png(8, 6, [0, 0, 200]), false), ("readme.txt", b"hello", true)]);
        let p = temp("cbz", "cbz", &zip);
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (8, 6));
        assert_eq!(first_pixel(&img), [0, 200, 0], "page2 comes before page10, and the macOS shadow copy is skipped");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn office_thumbnails_are_preferred_then_the_biggest_picture() {
        let with_thumb = make_zip(&[("word/media/image1.png", &png(40, 40, [0, 0, 200]), true), ("docProps/thumbnail.jpeg", &jpeg(16, 12, [200, 100, 0]), false), ("word/document.xml", b"<w/>", true)]);
        let p = temp("docx", "docx", &with_thumb);
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (16, 12), "the thumbnail Word saved");
        std::fs::remove_file(p).ok();
        let without = make_zip(&[("xl/media/small.png", &png(4, 4, [1, 2, 3]), false), ("xl/media/big.png", &png(30, 20, [9, 9, 9]), false)]);
        let p = temp("xlsx", "xlsx", &without);
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (30, 20), "else the biggest picture inside");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn open_document_thumbnails() {
        let odt = make_zip(&[("mimetype", b"application/vnd.oasis.opendocument.text", false), ("Thumbnails/thumbnail.png", &png(10, 14, [5, 6, 7]), false)]);
        let p = temp("odt", "odt", &odt);
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (10, 14));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn affinity_files_give_the_png_inside_them() {
        let mut file = vec![0xAA; 300];
        file.extend(png(12, 9, [7, 8, 9]));
        file.extend(vec![0x55; 500]);
        let p = temp("af", "afphoto", &file);
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (12, 9));
        std::fs::remove_file(p).ok();
        // No preview inside: a card that says so.
        let p = temp("af2", "afdesign", &vec![1u8; 1000]);
        let card = decode(&p, true).unwrap();
        assert!(card.colour_note.as_deref().unwrap_or("").contains("no preview"), "{:?}", card.colour_note);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn text_becomes_a_card_of_its_first_lines_and_anything_else_a_card_with_name_and_size() {
        let p = temp("txt", "txt", "\n  \nFirst line\u{0E01}\nsecond\n".as_bytes());
        let img = decode(&p, true).unwrap();
        assert_eq!((img.width, img.height), (192, 256), "a text page is portrait");
        assert!(img.rgba.chunks_exact(4).all(|p| p[3] == 255));
        assert!(img.rgba.chunks_exact(4).any(|p| p[0] < 100), "there is dark text on the paper");
        let big = decode(&p, false).unwrap();
        assert_eq!((big.width, big.height), (384, 512), "the viewer gets it twice as big");
        std::fs::remove_file(p).ok();
        // A zip without pictures, a damaged zip, an empty text file.
        let p = temp("plain", "zip", &make_zip(&[("a.txt", b"x", true)]));
        let card = decode(&p, true).unwrap();
        assert_eq!((card.width, card.height), (256, 192));
        assert!(card.colour_note.as_deref().unwrap_or("").contains("card"));
        std::fs::remove_file(p).ok();
        let p = temp("bad", "docx", b"this is not a zip file at all");
        assert_eq!(decode(&p, true).unwrap().width, 256, "a card, not an error");
        std::fs::remove_file(p).ok();
        let p = temp("empty", "txt", b"");
        assert_eq!(decode(&p, true).unwrap().width, 256);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn text_decoding_handles_boms_and_cut_characters() {
        let p = temp("u16", "txt", &[0xFF, 0xFE, b'H', 0, b'i', 0]);
        assert_eq!(read_text_head(&p).as_deref(), Some("Hi"));
        std::fs::remove_file(p).ok();
        let mut bytes = "ก".repeat(2000).into_bytes(); // 3 bytes each: 4096 cuts one in half
        bytes.insert(0, b'A');
        let p = temp("cut", "txt", &bytes);
        assert!(read_text_head(&p).unwrap().starts_with('A'));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["p10.png", "p2.png", "P1.png", "p02b.png", "p100.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["P1.png", "p2.png", "p02b.png", "p10.png", "p100.png"]);
    }

    #[test]
    fn zip_reader_refuses_junk_and_big_entries() {
        let p = temp("junk", "zip", b"PK nothing like a zip");
        assert!(Zip::open(&p).is_err());
        std::fs::remove_file(p).ok();
        let z = make_zip(&[("big.bin", &vec![0u8; 5000], true)]);
        let p = temp("big", "zip", &z);
        let mut zip = Zip::open(&p).unwrap();
        let e = zip.entries[0].clone();
        assert!(zip.read(&e, 1000).is_err(), "limit applies");
        assert_eq!(zip.read(&e, 10_000).unwrap().len(), 5000);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn document_extensions() {
        for e in ["zip", "CBZ", "docx", "pptx", "xlsx", "odt", "txt", "afphoto"] {
            assert!(is_document_ext(e), "{e}");
        }
        assert!(!is_document_ext("jpg") && !is_document_ext("pdf") && !is_document_ext("exe"));
    }
}
