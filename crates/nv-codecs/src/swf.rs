//! Flash movies (`.swf`). A Flash movie is a program (vector shapes, scripts, sound), not a picture, and there is no way to
//! "play" one here. What can be shown is the **largest bitmap stored inside it** (a JPEG or a lossless bitmap, which is what
//! most Flash banners, galleries and sprite sheets are made of). A movie without bitmaps gets an error that says so.
//!
//! Layout: `FWS` | `CWS` (zlib after the 8-byte header) | `ZWS` (LZMA: not supported), version, length, then a RECT, frame
//! rate and count, then tags: u16 `code << 6 | length` (length 0x3F = a u32 follows).

use crate::Image;
use std::io::Read;

pub fn is_swf(b: &[u8]) -> bool {
    b.len() >= 8 && matches!(&b[0..3], b"FWS" | b"CWS" | b"ZWS")
}

fn inflate(data: &[u8], limit: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).take(limit).read_to_end(&mut out).map_err(|e| format!("swf: damaged compressed data ({e})"))?;
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if !is_swf(bytes) {
        return Err("swf: not a Flash file (bad signature)".into());
    }
    if &bytes[0..3] == b"ZWS" {
        return Err("swf: this movie uses LZMA compression, which is not supported".into());
    }
    let body_owned;
    let body: &[u8] = if &bytes[0..3] == b"CWS" {
        body_owned = inflate(&bytes[8..], 512 << 20)?;
        &body_owned
    } else {
        &bytes[8..]
    };
    // Skip the RECT (5-bit field size, four fields), the frame rate and the frame count.
    let nbits = (*body.first().ok_or("swf: unexpected end of file")? >> 3) as usize;
    let mut at = (5 + 4 * nbits).div_ceil(8) + 4;
    let mut tables: Option<&[u8]> = None;
    let mut best: Option<Image> = None;
    let mut found_any = false;
    while at + 2 <= body.len() {
        let head = u16::from_le_bytes([body[at], body[at + 1]]);
        at += 2;
        let (code, mut len) = (head >> 6, (head & 0x3F) as usize);
        if len == 0x3F {
            let l = body.get(at..at + 4).ok_or("swf: unexpected end of file")?;
            len = u32::from_le_bytes([l[0], l[1], l[2], l[3]]) as usize;
            at += 4;
        }
        let Some(data) = body.get(at..at + len) else { break };
        at += len;
        if code == 0 {
            break; // End
        }
        let image = match code {
            8 => {
                tables = Some(data);
                None
            }
            6 if data.len() > 2 => jpeg_with_tables(tables, &data[2..]),
            21 if data.len() > 2 => crate::jpeg::decode(strip_leading_eoi(&data[2..]), None).ok(),
            35 | 90 if data.len() > 6 => {
                let alpha_at = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
                let start = if code == 90 { 8 } else { 6 };
                match data.get(start..start + alpha_at.min(data.len().saturating_sub(start))) {
                    Some(jpeg) => crate::jpeg::decode(strip_leading_eoi(jpeg), None).ok().map(|mut img| {
                        if let Some(rest) = data.get(start + alpha_at..) {
                            if let Ok(alpha) = inflate(rest, img.width as u64 * img.height as u64) {
                                if alpha.len() == img.width as usize * img.height as usize {
                                    for (p, a) in img.rgba.chunks_exact_mut(4).zip(alpha) {
                                        p[3] = a;
                                    }
                                }
                            }
                        }
                        img
                    }),
                    None => None,
                }
            }
            20 | 36 => lossless(data, code == 36),
            _ => None,
        };
        if let Some(img) = image {
            found_any = true;
            if best.as_ref().map(|b| img.width as u64 * img.height as u64 > b.width as u64 * b.height as u64).unwrap_or(true) {
                best = Some(img);
            }
        }
    }
    best.ok_or_else(|| {
        if found_any {
            "swf: the bitmaps in this movie could not be decoded".to_string()
        } else {
            "swf: this Flash movie holds no bitmap pictures (it is vector art or a program, which NuViewer cannot play)".to_string()
        }
    })
}

/// Flash writes some JPEGs as `FF D9 FF D8 ...`: an end marker, then the real start.
fn strip_leading_eoi(j: &[u8]) -> &[u8] {
    if j.starts_with(&[0xFF, 0xD9, 0xFF, 0xD8]) { &j[2..] } else { j }
}

/// DefineBits: the quantisation tables sit in a separate JPEGTables tag; stitch tables + image into one stream.
fn jpeg_with_tables(tables: Option<&[u8]>, image: &[u8]) -> Option<Image> {
    let image = strip_leading_eoi(image);
    let mut joined = Vec::new();
    match tables {
        Some(t) => {
            joined.extend_from_slice(t.strip_suffix(&[0xFF, 0xD9]).unwrap_or(t));
            joined.extend_from_slice(image.strip_prefix(&[0xFF, 0xD8]).unwrap_or(image));
        }
        None => joined.extend_from_slice(image),
    }
    crate::jpeg::decode(&joined, None).ok()
}

/// DefineBitsLossless / DefineBitsLossless2: zlib-compressed pixels, colour-mapped (8 bit) or true colour (15 / 24 bit).
fn lossless(d: &[u8], with_alpha: bool) -> Option<Image> {
    let format = *d.get(2)?;
    let (w, h) = (u16::from_le_bytes([*d.get(3)?, *d.get(4)?]) as usize, u16::from_le_bytes([*d.get(5)?, *d.get(6)?]) as usize);
    crate::check_size(w as u64, h as u64).ok()?;
    let (palette_len, data_at) = if format == 3 { (*d.get(7)? as usize + 1, 8) } else { (0, 7) };
    let raw = inflate(d.get(data_at..)?, 1 << 31).ok()?;
    let mut rgba = vec![255u8; w * h * 4];
    match format {
        3 => {
            let entry = if with_alpha { 4 } else { 3 };
            let pal = raw.get(..palette_len * entry)?;
            let stride = w.div_ceil(4) * 4; // rows are padded to 32 bits
            let idx = raw.get(palette_len * entry..)?;
            if idx.len() < stride * h {
                return None;
            }
            for y in 0..h {
                for x in 0..w {
                    let i = idx[y * stride + x] as usize;
                    let e = pal.get(i * entry..i * entry + entry).unwrap_or(&[0, 0, 0, 0][..entry]);
                    let o = (y * w + x) * 4;
                    rgba[o..o + 3].copy_from_slice(&e[..3]);
                    rgba[o + 3] = if with_alpha { e[3] } else { 255 };
                }
            }
        }
        5 => {
            if raw.len() < w * h * 4 {
                return None;
            }
            for (i, p) in raw.chunks_exact(4).take(w * h).enumerate() {
                // 32-bit pixels: ARGB (premultiplied in Lossless2), or an unused byte then RGB.
                let (a, r, g, b) = (p[0], p[1], p[2], p[3]);
                let un = |c: u8| if with_alpha && a != 0 { (c as u32 * 255 / a as u32).min(255) as u8 } else { c };
                rgba[i * 4..i * 4 + 4].copy_from_slice(&[un(r), un(g), un(b), if with_alpha { a } else { 255 }]);
            }
        }
        4 => {
            let stride = (w * 2).div_ceil(4) * 4;
            if raw.len() < stride * h {
                return None;
            }
            for y in 0..h {
                for x in 0..w {
                    let v = u16::from_be_bytes([raw[y * stride + x * 2], raw[y * stride + x * 2 + 1]]);
                    let c5 = |s: u16| (((v >> s) & 31) * 255 / 31) as u8;
                    let o = (y * w + x) * 4;
                    rgba[o..o + 3].copy_from_slice(&[c5(10), c5(5), c5(0)]);
                }
            }
        }
        _ => return None,
    }
    Some(Image::new(w as u32, h as u32, rgba))
}

// ---------------------------------------------------------------------------------------------------- cards and FLA

/// What the header of a movie says.
#[derive(Debug, PartialEq)]
pub struct Info {
    pub version: u8,
    /// Stage size in pixels (0 when unknown, e.g. LZMA movies whose header is compressed).
    pub width: f64,
    pub height: f64,
    pub fps: f64,
    pub frames: u16,
}

fn bits(body: &[u8], pos: usize, n: usize) -> i64 {
    let mut v: i64 = 0;
    for i in 0..n {
        let p = pos + i;
        let bit = body.get(p / 8).map(|b| (b >> (7 - p % 8)) & 1).unwrap_or(0);
        v = (v << 1) | bit as i64;
    }
    if n > 0 && v & (1 << (n - 1)) != 0 {
        v -= 1 << n; // sign-extend
    }
    v
}

pub fn info(bytes: &[u8]) -> Option<Info> {
    if !is_swf(bytes) {
        return None;
    }
    let version = bytes[3];
    let head: Vec<u8> = match &bytes[0..3] {
        b"CWS" => inflate(&bytes[8..], 64).ok()?,
        b"ZWS" => return Some(Info { version, width: 0.0, height: 0.0, fps: 0.0, frames: 0 }),
        _ => bytes[8..bytes.len().min(8 + 64)].to_vec(),
    };
    let n = (*head.first()? >> 3) as usize;
    let (x0, x1, y0, y1) = (bits(&head, 5, n), bits(&head, 5 + n, n), bits(&head, 5 + 2 * n, n), bits(&head, 5 + 3 * n, n));
    let at = (5 + 4 * n).div_ceil(8);
    let fps = u16::from_le_bytes([*head.get(at)?, *head.get(at + 1)?]) as f64 / 256.0;
    let frames = u16::from_le_bytes([*head.get(at + 2)?, *head.get(at + 3)?]);
    Some(Info { version, width: (x1 - x0) as f64 / 20.0, height: (y1 - y0) as f64 / 20.0, fps, frames })
}

fn describe(i: &Info) -> String {
    let mut s = format!("Flash {}", i.version);
    if i.width > 0.0 {
        s.push_str(&format!("  ·  {} × {}", i.width.round(), i.height.round()));
    }
    if i.fps > 0.0 {
        s.push_str(&format!("  ·  {} fps  ·  {} frames", (i.fps * 10.0).round() / 10.0, i.frames));
    }
    s
}

/// A Flash movie as a picture: the largest bitmap inside it, or, for a movie made of vector art and scripts (which this
/// program cannot play), a card with what the header says. A file that is no Flash movie at all is an error.
pub(crate) fn decode_or_card(path: &std::path::Path, bytes: &[u8], first_only: bool) -> Result<Image, String> {
    match decode(bytes) {
        Ok(mut img) => {
            img.colour_note = Some("the largest bitmap stored inside the Flash movie".into());
            Ok(img)
        }
        Err(e) => {
            let Some(i) = info(bytes) else { return Err(e) };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let sub = format!("{}  ·  {}", describe(&i), crate::docthumb::fmt_bytes(bytes.len() as u64));
            let mut img = crate::docthumb::render_card(&crate::docthumb::Card { badge: "SWF".into(), accent: crate::docthumb::accent_for("swf"), title: name, subtitle: sub, lines: Vec::new() }, if first_only { 1 } else { 2 });
            img.colour_note = Some(format!("a card ({}; Flash movies are programs, not pictures)", e.trim_start_matches("swf: ")));
            Ok(img)
        }
    }
}

/// `<DOMDocument ... width="550" height="400" frameRate="24">`: one attribute of the document's root element.
fn xml_attr(head: &str, name: &str) -> Option<String> {
    let key = format!(" {name}=\"");
    let s = head.find(&key)? + key.len();
    let e = head[s..].find('"')?;
    Some(head[s..s + e].to_string())
}

/// A Flash authoring file (`.fla`). Since CS5 it is a zip holding `DOMDocument.xml` (the stage, the timeline) and the library:
/// a picture stored in it is shown; otherwise a card with the stage size and what the library holds. The older binary
/// format (an OLE container) is only recognised.
#[allow(unused_assignments)]
pub(crate) fn decode_fla(path: &std::path::Path, first_only: bool) -> Result<Image, String> {
    use crate::docthumb::{ext_of, fmt_bytes, is_image_name, render_card, Card, Zip};
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?.len();
    let scale = if first_only { 1 } else { 2 };
    let limit: u64 = if first_only { 64 << 20 } else { 256 << 20 };
    let mut head = [0u8; 8];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
        let n = f.read(&mut head).unwrap_or(0);
        head[n..].fill(0);
    }
    let mut about = String::new();
    let mut note = String::new();
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        about = "Flash 8 or older (binary FLA)".into();
        note = "an old binary FLA holds no preview picture".into();
    } else {
        match Zip::open(path) {
            Ok(mut zip) => {
                let mut pics: Vec<usize> = (0..zip.entries.len()).filter(|&i| is_image_name(&zip.entries[i].name)).collect();
                pics.sort_by(|&a, &b| zip.entries[b].size.cmp(&zip.entries[a].size));
                for i in pics.into_iter().take(4) {
                    let e = zip.entries[i].clone();
                    if let Ok(mut img) = zip.read(&e, limit).and_then(|b| crate::decode_image_bytes(&b, &ext_of(&e.name))) {
                        img.colour_note = Some(format!("FLA library picture: {}", e.name));
                        img.profile_assumable = true;
                        return Ok(img);
                    }
                }
                let xml = zip.entries.iter().position(|e| e.name.eq_ignore_ascii_case("DOMDocument.xml")).and_then(|i| {
                    let e = zip.entries[i].clone();
                    zip.read(&e, 8 << 20).ok()
                });
                let library = zip.entries.iter().filter(|e| e.name.to_ascii_lowercase().starts_with("library/")).count();
                match xml {
                    Some(x) => {
                        let text = String::from_utf8_lossy(&x).into_owned();
                        let root = text.find("<DOMDocument").map(|s| &text[s..(s + 2048).min(text.len())]).unwrap_or("");
                        let root = root.split('>').next().unwrap_or("");
                        let mut parts = vec!["Flash document".to_string()];
                        if let (Some(w), Some(h)) = (xml_attr(root, "width").or_else(|| Some("550".into())), xml_attr(root, "height").or_else(|| Some("400".into()))) {
                            parts.push(format!("{w} × {h}"));
                        }
                        if let Some(f) = xml_attr(root, "frameRate") {
                            parts.push(format!("{f} fps"));
                        }
                        let bitmaps = text.matches("<DOMBitmapItem").count();
                        if bitmaps > 0 {
                            parts.push(format!("{bitmaps} bitmap{}", if bitmaps == 1 { "" } else { "s" }));
                        }
                        if library > 0 {
                            parts.push(format!("{library} library items"));
                        }
                        about = parts.join("  ·  ");
                        note = "an FLA keeps its bitmaps in a private format: no picture to show".into();
                    }
                    None => about = "zip without DOMDocument.xml".into(),
                }
            }
            Err(e) => about = format!("not readable as an FLA ({e})"),
        }
    }
    let mut img = render_card(&Card { badge: "FLA".into(), accent: crate::docthumb::accent_for("fla"), title: name, subtitle: format!("{}  ·  {about}", fmt_bytes(size)), lines: Vec::new() }, scale);
    img.colour_note = Some(format!("a card ({note})"));
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn tag(code: u16, data: &[u8]) -> Vec<u8> {
        let mut v = ((code << 6) | 0x3F).to_le_bytes().to_vec(); // always the long form
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    /// A movie (header + tags + End). `compressed` = CWS.
    fn movie(tags: &[Vec<u8>], compressed: bool) -> Vec<u8> {
        let mut body = vec![0b0000_1000, 0]; // RECT with 1-bit fields: 5 + 4 bits, padded to 2 bytes
        body.extend([24, 0, 1, 0]); // frame rate, frame count
        for t in tags {
            body.extend(t);
        }
        body.extend([0, 0]); // End tag
        let mut f = if compressed { b"CWS".to_vec() } else { b"FWS".to_vec() };
        f.push(9);
        f.extend((body.len() as u32 + 8).to_le_bytes());
        f.extend(if compressed { zlib(&body) } else { body });
        f
    }

    fn lossless_tag(w: u16, h: u16) -> Vec<u8> {
        // Format 5 (32-bit xRGB): a red 2x1 bitmap.
        let mut px = Vec::new();
        for _ in 0..(w as usize * h as usize) {
            px.extend([0, 255, 0, 0]);
        }
        let mut d = 7u16.to_le_bytes().to_vec(); // character id
        d.push(5);
        d.extend(w.to_le_bytes());
        d.extend(h.to_le_bytes());
        d.extend(zlib(&px));
        tag(20, &d)
    }

    #[test]
    fn the_largest_lossless_bitmap_is_shown() {
        for compressed in [false, true] {
            let swf = movie(&[lossless_tag(2, 1), lossless_tag(4, 3)], compressed);
            let img = decode(&swf).unwrap();
            assert_eq!((img.width, img.height), (4, 3), "compressed: {compressed}");
            assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
        }
    }

    #[test]
    fn colour_mapped_bitmaps_use_their_palette() {
        // 2x1, palette of 2 (RGB): 0 = green, 1 = blue; rows are padded to 4 bytes.
        let mut raw = vec![0, 255, 0, 0, 0, 255];
        raw.extend([1, 0, 0, 0]);
        let mut d = 1u16.to_le_bytes().to_vec();
        d.push(3);
        d.extend(2u16.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.push(1); // palette size - 1
        d.extend(zlib(&raw));
        let img = decode(&movie(&[tag(20, &d)], false)).unwrap();
        assert_eq!(&img.rgba[..8], &[0, 0, 255, 255, 0, 255, 0, 255]); // index 1 (blue), then index 0 (green)
    }

    #[test]
    fn vector_only_movies_and_garbage_are_errors_that_say_why() {
        let e = decode(&movie(&[tag(2, &[0; 4])], false)).err().unwrap();
        assert!(e.contains("no bitmap"), "{e}");
        assert!(decode(b"not flash").is_err());
        assert!(decode(b"ZWS\x09\0\0\0\0xx").err().unwrap().contains("LZMA"));
        assert!(decode(b"CWS\x09\0\0\0\0garbage").is_err());
    }
    #[test]
    fn header_info_and_cards() {
        let swf = movie(&[tag(2, &[0; 4])], false);
        // 1-bit RECT fields (all 0 or -1) -> 0 x 0 stage; frame rate 24, 1 frame, version 9
        let i = info(&swf).unwrap();
        assert_eq!((i.version, i.fps, i.frames), (9, 24.0 / 256.0, 1));
        assert!(info(b"nope").is_none());
        // a 550 x 400 px stage: nbits = 15 -> 11000 twips wide
        let mut head = vec![];
        let n = 15usize;
        let mut acc: Vec<bool> = vec![];
        let push = |acc: &mut Vec<bool>, v: u32, n: usize| (0..n).rev().for_each(|b| acc.push(v >> b & 1 == 1));
        push(&mut acc, n as u32, 5);
        for v in [0u32, 11000, 0, 8000] {
            push(&mut acc, v, n);
        }
        while acc.len() % 8 != 0 {
            acc.push(false);
        }
        for c in acc.chunks(8) {
            head.push(c.iter().fold(0u8, |a, &b| a << 1 | b as u8));
        }
        head.extend([0, 24, 120, 0]); // 24.0 fps (8.8), 120 frames
        let mut f = b"FWS".to_vec();
        f.push(10);
        f.extend((head.len() as u32 + 8).to_le_bytes());
        f.extend(head);
        let i = info(&f).unwrap();
        assert_eq!((i.width, i.height, i.fps, i.frames, i.version), (550.0, 400.0, 24.0, 120, 10));
        assert_eq!(describe(&i), "Flash 10  ·  550 × 400  ·  24 fps  ·  120 frames");
        // a vector-only movie becomes a card, not an error; garbage stays an error
        let p = std::env::temp_dir().join("nv_vector.swf");
        let img = decode_or_card(&p, &f, true).expect("card");
        assert!(img.width > 0 && img.colour_note.as_deref().unwrap_or("").starts_with("a card"));
        assert!(decode_or_card(&p, b"junk", true).is_err());
    }
}
