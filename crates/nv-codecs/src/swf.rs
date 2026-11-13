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
}
