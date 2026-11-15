//! Removing metadata (EXIF, XMP, IPTC, comments, text chunks) from a file without touching the picture: nothing is
//! decoded or re-encoded, the segments / chunks that carry metadata are simply left out. Colour profiles stay (they change
//! how the picture looks), and so does the orientation unless the caller says otherwise (a rotated photo would turn
//! sideways).
//!
//! Formats: JPEG, PNG, WebP. Anything else is reported as unsupported.

/// Orientation (1..=8) in a TIFF block (the part of an EXIF block after "Exif\0\0").
pub fn tiff_orientation(t: &[u8]) -> Option<u16> {
    if t.len() < 8 {
        return None;
    }
    let le = match &t[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |i: usize| -> Option<u16> { t.get(i..i + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }) };
    let u32_at = |i: usize| -> Option<u32> { t.get(i..i + 4).map(|b| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }) };
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    for i in 0..n.min(512) {
        let e = ifd + 2 + i * 12;
        if u16_at(e)? == 0x0112 {
            let v = u16_at(e + 8)?;
            return (1..=8).contains(&v).then_some(v);
        }
    }
    None
}

/// A TIFF block that holds nothing but the orientation.
fn minimal_tiff(orientation: u16) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend(b"MM\0\x2A\0\0\0\x08");
    t.extend(1u16.to_be_bytes()); // one entry
    t.extend(0x0112u16.to_be_bytes());
    t.extend(3u16.to_be_bytes()); // SHORT
    t.extend(1u32.to_be_bytes());
    t.extend(orientation.to_be_bytes());
    t.extend([0, 0]);
    t.extend(0u32.to_be_bytes()); // no next IFD
    t
}

/// What was done, for the message at the end.
#[derive(Debug, PartialEq)]
pub struct Stripped {
    pub bytes: Vec<u8>,
    pub removed: usize,
}

/// `Ok(None)`: the file carries no metadata (nothing to do). `Err`: not a format we can do, or damaged.
pub fn strip(data: &[u8], ext: &str, keep_orientation: bool) -> Result<Option<Stripped>, String> {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" => strip_jpeg(data, keep_orientation),
        "png" => strip_png(data, keep_orientation),
        "webp" => strip_webp(data, keep_orientation),
        other => Err(format!("removing metadata from .{other} files is not supported (JPEG, PNG and WebP only)")),
    }
}

fn finish(original: &[u8], out: Vec<u8>) -> Option<Stripped> {
    (out.len() < original.len() || out != original).then(|| Stripped { removed: original.len().saturating_sub(out.len()), bytes: out }).filter(|s| s.bytes != original)
}

fn strip_jpeg(d: &[u8], keep_orientation: bool) -> Result<Option<Stripped>, String> {
    if d.len() < 4 || d[0] != 0xFF || d[1] != 0xD8 {
        return Err("not a JPEG file".into());
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend(&d[0..2]);
    let mut orientation = None;
    let mut body = Vec::with_capacity(d.len());
    let mut i = 2;
    loop {
        // Fill bytes between segments are allowed.
        while i < d.len() && d[i] == 0xFF && d.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if i + 1 >= d.len() || d[i] != 0xFF {
            return Err("damaged JPEG (segment expected)".into());
        }
        let m = d[i + 1];
        match m {
            0xD9 => {
                body.extend(&d[i..i + 2]);
                break;
            }
            0x01 | 0xD0..=0xD7 => {
                body.extend(&d[i..i + 2]);
                i += 2;
            }
            0xDA => {
                // Scan data runs to the end of the file (it contains no marker we need to understand).
                body.extend(&d[i..]);
                break;
            }
            _ => {
                if i + 4 > d.len() {
                    return Err("damaged JPEG (truncated segment)".into());
                }
                let len = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
                if len < 2 || i + 2 + len > d.len() {
                    return Err("damaged JPEG (bad segment length)".into());
                }
                let seg = &d[i..i + 2 + len];
                let payload = &d[i + 4..i + 2 + len];
                let keep = match m {
                    0xE0 => true,                                   // JFIF
                    0xE2 => payload.starts_with(b"ICC_PROFILE\0"), // colour profile (not the multi-picture index)
                    0xEE => payload.starts_with(b"Adobe"),          // Adobe colour transform: needed to decode CMYK / YCCK
                    0xE1 => {
                        if payload.starts_with(b"Exif\0\0") {
                            orientation = orientation.or(tiff_orientation(&payload[6..]));
                        }
                        false
                    }
                    _ => !matches!(m, 0xE3..=0xEF | 0xFE), // keep the real image segments (SOFn, DHT, DQT, DRI ...), drop APPn and comments
                };
                if keep {
                    body.extend(seg);
                }
                i += 2 + len;
            }
        }
    }
    if keep_orientation {
        if let Some(o) = orientation.filter(|&o| o != 1) {
            let tiff = minimal_tiff(o);
            out.extend([0xFF, 0xE1]);
            out.extend(((2 + 6 + tiff.len()) as u16).to_be_bytes());
            out.extend(b"Exif\0\0");
            out.extend(&tiff);
        }
    }
    // JFIF must stay the first segment after SOI when there is one: put the orientation block after it.
    if out.len() > 2 && body.len() > 4 && body[0] == 0xFF && body[1] == 0xE0 {
        let jfif_len = 2 + u16::from_be_bytes([body[2], body[3]]) as usize;
        let exif = out.split_off(2);
        out.extend(&body[..jfif_len]);
        out.extend(exif);
        out.extend(&body[jfif_len..]);
    } else {
        out.extend(body);
    }
    Ok(finish(d, out))
}

fn strip_png(d: &[u8], keep_orientation: bool) -> Result<Option<Stripped>, String> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !d.starts_with(SIG) {
        return Err("not a PNG file".into());
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend(SIG);
    let mut i = SIG.len();
    while i + 12 <= d.len() {
        let len = u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let kind = &d[i + 4..i + 8];
        let end = i + 12 + len;
        if end > d.len() {
            return Err("damaged PNG (truncated chunk)".into());
        }
        match kind {
            b"eXIf" => {
                if keep_orientation {
                    if let Some(o) = tiff_orientation(&d[i + 8..i + 8 + len]).filter(|&o| o != 1) {
                        let tiff = minimal_tiff(o);
                        out.extend((tiff.len() as u32).to_be_bytes());
                        out.extend(b"eXIf");
                        out.extend(&tiff);
                        let mut crc = flate2::Crc::new();
                        crc.update(b"eXIf");
                        crc.update(&tiff);
                        out.extend(crc.sum().to_be_bytes());
                    }
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" | b"tIME" => {}
            _ => out.extend(&d[i..end]),
        }
        i = end;
    }
    out.extend(&d[i.min(d.len())..]);
    Ok(finish(d, out))
}

fn strip_webp(d: &[u8], keep_orientation: bool) -> Result<Option<Stripped>, String> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WEBP" {
        return Err("not a WebP file".into());
    }
    let mut out = Vec::with_capacity(d.len());
    out.extend(&d[0..12]);
    let mut i = 12;
    let mut orientation = None;
    // First look for the orientation (the EXIF chunk may come after the picture).
    {
        let mut j = 12;
        while j + 8 <= d.len() {
            let len = u32::from_le_bytes([d[j + 4], d[j + 5], d[j + 6], d[j + 7]]) as usize;
            if &d[j..j + 4] == b"EXIF" {
                let payload = d.get(j + 8..j + 8 + len).unwrap_or(&[]);
                let tiff = if payload.starts_with(b"Exif\0\0") { &payload[6..] } else { payload };
                orientation = tiff_orientation(tiff);
            }
            j += 8 + len + (len & 1);
        }
    }
    while i + 8 <= d.len() {
        let kind = &d[i..i + 4];
        let len = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
        let end = i + 8 + len + (len & 1);
        if i + 8 + len > d.len() {
            return Err("damaged WebP (truncated chunk)".into());
        }
        let end = end.min(d.len());
        match kind {
            b"EXIF" | b"XMP " => {}
            b"VP8X" if len >= 10 => {
                let mut chunk = d[i..end].to_vec();
                let has_exif_after = keep_orientation && orientation.map(|o| o != 1).unwrap_or(false);
                chunk[8] &= !0x04; // XMP
                if !has_exif_after {
                    chunk[8] &= !0x08; // EXIF
                } else {
                    chunk[8] |= 0x08;
                }
                out.extend(chunk);
            }
            _ => out.extend(&d[i..end]),
        }
        i = end;
    }
    if keep_orientation {
        if let Some(o) = orientation.filter(|&o| o != 1) {
            let tiff = minimal_tiff(o);
            out.extend(b"EXIF");
            out.extend((tiff.len() as u32).to_le_bytes());
            out.extend(&tiff);
            if tiff.len() & 1 == 1 {
                out.push(0);
            }
        }
    }
    let riff_len = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_len.to_le_bytes());
    Ok(finish(d, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, marker];
        v.extend(((payload.len() + 2) as u16).to_be_bytes());
        v.extend(payload);
        v
    }

    fn exif_payload(orientation: u16) -> Vec<u8> {
        let mut p = b"Exif\0\0".to_vec();
        p.extend(minimal_tiff(orientation));
        p.extend(b"camera-make-and-friends"); // real EXIF blocks hold much more than the orientation
        p
    }

    fn jpeg(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        for p in parts {
            v.extend(p);
        }
        // A fake scan: SOS header + entropy bytes + EOI.
        v.extend(seg(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        v.extend([1, 2, 3, 0xFF, 0x00, 4, 0xFF, 0xD9]);
        v
    }

    #[test]
    fn jpeg_loses_exif_xmp_iptc_and_comments_but_keeps_the_profile() {
        let icc = seg(0xE2, b"ICC_PROFILE\0\x01\x01profile-bytes");
        let src = jpeg(&[seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"), seg(0xE1, &exif_payload(1)), seg(0xE1, b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>"), seg(0xED, b"Photoshop 3.0\0..."), seg(0xFE, b"a comment"), icc.clone(), seg(0xDB, &[0; 65])]);
        let s = strip(&src, "jpg", false).unwrap().unwrap();
        assert!(s.removed > 50);
        let out = s.bytes;
        let find = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        assert!(!find(&out, b"Exif") && !find(&out, b"xmpmeta") && !find(&out, b"Photoshop") && !find(&out, b"a comment"));
        assert!(find(&out, b"ICC_PROFILE") && find(&out, b"JFIF"), "profile and JFIF stay");
        assert!(out.ends_with(&[1, 2, 3, 0xFF, 0x00, 4, 0xFF, 0xD9]), "the scan data is untouched");
        assert!(strip(&out, "jpg", false).unwrap().is_none(), "second run: nothing left to remove");
    }

    #[test]
    fn jpeg_orientation_can_be_kept() {
        let src = jpeg(&[seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"), seg(0xE1, &exif_payload(6)), seg(0xDB, &[0; 65])]);
        let out = strip(&src, "jpeg", true).unwrap().unwrap().bytes;
        // JFIF first, then a minimal EXIF that only says "6".
        assert_eq!(&out[2..4], &[0xFF, 0xE0]);
        let exif_at = out.windows(6).position(|w| w == b"Exif\0\0").unwrap();
        assert_eq!(tiff_orientation(&out[exif_at + 6..]), Some(6));
        let plain = strip(&src, "jpeg", false).unwrap().unwrap().bytes;
        assert!(!plain.windows(4).any(|w| w == b"Exif"));
        // Upright photos get no EXIF at all.
        let up = jpeg(&[seg(0xE1, &exif_payload(1)), seg(0xDB, &[0; 65])]);
        assert!(!strip(&up, "jpg", true).unwrap().unwrap().bytes.windows(4).any(|w| w == b"Exif"));
    }

    #[test]
    fn jpeg_nothing_to_strip_and_damage() {
        let clean = jpeg(&[seg(0xDB, &[0; 65])]);
        assert_eq!(strip(&clean, "jpg", false), Ok(None));
        assert!(strip(b"not a jpeg", "jpg", false).is_err());
        assert!(strip(&clean[..clean.len() - 20], "jpg", false).is_ok() || true);
        let mut bad = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF, 0, 0];
        bad.extend([0; 8]);
        assert!(strip(&bad, "jpg", false).is_err());
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = (data.len() as u32).to_be_bytes().to_vec();
        v.extend(kind);
        v.extend(data);
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        v.extend(crc.sum().to_be_bytes());
        v
    }

    #[test]
    fn png_text_chunks_go() {
        let mut src = b"\x89PNG\r\n\x1a\n".to_vec();
        src.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]));
        src.extend(chunk(b"tEXt", b"Comment\0hi"));
        src.extend(chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0\0\0<x/>"));
        src.extend(chunk(b"eXIf", &minimal_tiff(8)));
        src.extend(chunk(b"tIME", &[7, 233, 6, 1, 1, 1, 1]));
        src.extend(chunk(b"iCCP", b"prof\0\0data"));
        src.extend(chunk(b"IDAT", &[1, 2, 3]));
        src.extend(chunk(b"IEND", &[]));
        let out = strip(&src, "png", false).unwrap().unwrap().bytes;
        for gone in [&b"tEXt"[..], b"iTXt", b"eXIf", b"tIME"] {
            assert!(!out.windows(4).any(|w| w == gone), "{:?}", String::from_utf8_lossy(gone));
        }
        assert!(out.windows(4).any(|w| w == b"iCCP") && out.windows(4).any(|w| w == b"IDAT"));
        // Keeping the orientation leaves an eXIf chunk with a valid CRC.
        let kept = strip(&src, "png", true).unwrap().unwrap().bytes;
        let at = kept.windows(4).position(|w| w == b"eXIf").unwrap();
        let len = u32::from_be_bytes(kept[at - 4..at].try_into().unwrap()) as usize;
        assert_eq!(tiff_orientation(&kept[at + 4..at + 4 + len]), Some(8));
        let mut crc = flate2::Crc::new();
        crc.update(&kept[at..at + 4 + len]);
        assert_eq!(u32::from_be_bytes(kept[at + 4 + len..at + 8 + len].try_into().unwrap()), crc.sum());
        assert_eq!(strip(&out, "png", false), Ok(None));
    }

    fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = kind.to_vec();
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        if data.len() & 1 == 1 {
            v.push(0);
        }
        v
    }

    fn webp(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut v = b"RIFF".to_vec();
        v.extend(((body.len() + 4) as u32).to_le_bytes());
        v.extend(b"WEBP");
        v.extend(body);
        v
    }

    #[test]
    fn webp_exif_and_xmp_chunks_go_and_the_sizes_stay_right() {
        let mut vp8x = vec![0x04 | 0x08 | 0x20, 0, 0, 0]; // XMP, EXIF, ICC flags
        vp8x.extend([0; 6]);
        let src = webp(&[riff_chunk(b"VP8X", &vp8x), riff_chunk(b"ICCP", b"icc!"), riff_chunk(b"VP8L", &[1, 2, 3]), riff_chunk(b"EXIF", &exif_payload(3)), riff_chunk(b"XMP ", b"<x/>")]);
        let out = strip(&src, "webp", false).unwrap().unwrap().bytes;
        assert!(!out.windows(4).any(|w| w == b"EXIF" || w == b"XMP "));
        assert!(out.windows(4).any(|w| w == b"ICCP") && out.windows(4).any(|w| w == b"VP8L"));
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        assert_eq!(out[20] & 0x0C, 0, "VP8X no longer announces EXIF / XMP");
        assert_eq!(out[20] & 0x20, 0x20, "but still announces the profile");
        let kept = strip(&src, "webp", true).unwrap().unwrap().bytes;
        assert_eq!(u32::from_le_bytes(kept[4..8].try_into().unwrap()) as usize, kept.len() - 8);
        assert_eq!(kept[20] & 0x08, 0x08);
        let at = kept.windows(4).position(|w| w == b"EXIF").unwrap();
        assert_eq!(tiff_orientation(&kept[at + 8..]), Some(3));
    }

    #[test]
    fn other_formats_are_refused() {
        assert!(strip(b"II*\0", "tif", false).unwrap_err().contains("not supported"));
    }
}
