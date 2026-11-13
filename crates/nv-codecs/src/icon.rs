//! Windows bitmaps without a file header (`.dib`), icons and cursors (`.ico`, `.cur`) and Apple icon files (`.icns`).
//!
//! A DIB is the BMP pixel format on its own: a BITMAPINFOHEADER (or the older / newer variants), an optional palette and
//! the pixels. Icons and cursors are a directory of such DIBs (or PNGs); the biggest picture is shown. In an icon's DIB the
//! height counts the picture twice, because a 1-bit AND mask (the transparency of old icons) follows the colours.

use crate::Image;

fn le16(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| "dib: unexpected end of data".to_string())
}

fn le32(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| "dib: unexpected end of data".to_string())
}

/// Bit masks of a 16 or 32 bit pixel: (mask, shift, bits) per channel R, G, B, A.
type Masks = [(u32, u32, u32); 4];

fn mask_of(m: u32) -> (u32, u32, u32) {
    if m == 0 {
        return (0, 0, 0);
    }
    let shift = m.trailing_zeros();
    (m, shift, (m >> shift).trailing_ones())
}

fn extract(px: u32, (mask, shift, bits): (u32, u32, u32)) -> u8 {
    if bits == 0 {
        return 255;
    }
    let v = (px & mask) >> shift;
    // Scale to 0..=255.
    if bits >= 8 { (v >> (bits - 8)) as u8 } else { (v * 255 / ((1 << bits) - 1)) as u8 }
}

/// Decode a DIB: `data` starts at the info header. `icon` = the height includes an AND mask (icons and cursors).
/// `pixels_at` is where the pixel rows start when the file says so (a BMP's `bfOffBits` minus its 14-byte file header).
pub fn decode_dib(data: &[u8], icon: bool, pixels_at: Option<usize>) -> Result<Image, String> {
    let hsize = le32(data, 0)? as usize;
    let (w, mut h, planes_bpp, compression, colors_used, masks_in_header): (i64, i64, (u16, u16), u32, u32, bool) = if hsize == 12 {
        (le16(data, 4)? as i64, le16(data, 6)? as i64, (le16(data, 8)?, le16(data, 10)?), 0, 0, false)
    } else if hsize >= 40 {
        (le32(data, 4)? as i32 as i64, le32(data, 8)? as i32 as i64, (le16(data, 12)?, le16(data, 14)?), le32(data, 16)?, le32(data, 32)?, hsize >= 52)
    } else {
        return Err(format!("dib: unsupported header size {hsize}"));
    };
    let bpp = planes_bpp.1 as usize;
    let top_down = h < 0;
    h = h.abs();
    if icon {
        h /= 2; // the mask counts as a second picture
    }
    let (w, h) = (w.unsigned_abs() as usize, h as usize);
    if w == 0 || h == 0 {
        return Err("dib: empty image".into());
    }
    crate::check_size(w as u64, h as u64)?;
    if !matches!(bpp, 1 | 2 | 4 | 8 | 16 | 24 | 32) {
        return Err(format!("dib: unsupported bit depth {bpp}"));
    }
    if matches!(compression, 4 | 5) {
        return Err("dib: JPEG / PNG compressed bitmaps are not supported".into());
    }
    // Where the palette and the masks are.
    let mut at = hsize;
    let mut masks: Masks = [(0xFF0000, 16, 8), (0xFF00, 8, 8), (0xFF, 0, 8), (0, 0, 0)];
    if bpp == 16 {
        masks = [(0x7C00, 10, 5), (0x03E0, 5, 5), (0x001F, 0, 5), (0, 0, 0)];
    }
    if compression == 3 || compression == 6 {
        // BITMAPINFOHEADER keeps the masks right after itself; the bigger headers hold them inside.
        let base = if masks_in_header { 40 } else { at };
        masks = [mask_of(le32(data, base)?), mask_of(le32(data, base + 4)?), mask_of(le32(data, base + 8)?), (0, 0, 0)];
        if compression == 6 || (masks_in_header && hsize >= 56) {
            masks[3] = mask_of(le32(data, base + 12)?);
        }
        if !masks_in_header {
            at += if compression == 6 { 16 } else { 12 };
        }
    } else if compression == 0 && bpp == 32 && hsize >= 108 {
        // A 32-bit BI_RGB bitmap with a V4/V5 header: alpha mask is stated in the header.
        let a = mask_of(le32(data, 52)?);
        if a.0 != 0 {
            masks[3] = a;
        }
    }
    let entry = if hsize == 12 { 3 } else { 4 };
    let ncolors = if bpp <= 8 { if colors_used > 0 { colors_used as usize } else { 1 << bpp } } else { 0 };
    let palette: Vec<[u8; 3]> = (0..ncolors).map(|i| data.get(at + i * entry..at + i * entry + 3).map(|c| [c[2], c[1], c[0]]).unwrap_or([0, 0, 0])).collect();
    let pixels_start = pixels_at.unwrap_or(at + ncolors * entry);
    let pix = data.get(pixels_start..).ok_or("dib: unexpected end of data")?;

    let mut rgba = vec![0u8; w * h * 4];
    let mut any_alpha = false;
    match compression {
        1 | 2 => rle(pix, w, h, compression == 1, &palette, top_down, &mut rgba)?,
        0 | 3 | 6 => {
            let stride = (w * bpp).div_ceil(32) * 4;
            if pix.len() < stride * (h - 1) + (w * bpp).div_ceil(8) {
                return Err("dib: unexpected end of data".into());
            }
            for y in 0..h {
                let src_row = if top_down { y } else { h - 1 - y };
                let row = &pix[src_row * stride..];
                for x in 0..w {
                    let o = (y * w + x) * 4;
                    let (r, g, b, a) = match bpp {
                        1 | 2 | 4 | 8 => {
                            let per = 8 / bpp;
                            let byte = row[x / per];
                            let idx = ((byte >> (8 - bpp * (x % per + 1))) & ((1u16 << bpp) - 1) as u8) as usize;
                            let c = palette.get(idx).copied().unwrap_or([0, 0, 0]);
                            (c[0], c[1], c[2], 255)
                        }
                        16 => {
                            let px = u16::from_le_bytes([row[x * 2], row[x * 2 + 1]]) as u32;
                            (extract(px, masks[0]), extract(px, masks[1]), extract(px, masks[2]), if masks[3].0 != 0 { extract(px, masks[3]) } else { 255 })
                        }
                        24 => (row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255),
                        _ => {
                            let px = u32::from_le_bytes([row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]]);
                            if compression == 0 && masks[3].0 == 0 {
                                (row[x * 4 + 2], row[x * 4 + 1], row[x * 4], row[x * 4 + 3])
                            } else {
                                (extract(px, masks[0]), extract(px, masks[1]), extract(px, masks[2]), if masks[3].0 != 0 { extract(px, masks[3]) } else { 255 })
                            }
                        }
                    };
                    rgba[o..o + 4].copy_from_slice(&[r, g, b, a]);
                    any_alpha |= a != 0;
                }
            }
            // A 32-bit BI_RGB bitmap whose fourth byte is unused (all zero) is opaque, not invisible.
            if bpp == 32 && !any_alpha && !icon {
                for p in rgba.chunks_exact_mut(4) {
                    p[3] = 255;
                }
            }
        }
        c => return Err(format!("dib: unsupported compression {c}")),
    }
    // Icons and cursors: the AND mask (1 = transparent) follows the colours, unless the colours carry real alpha.
    if icon {
        let colour_bytes = if matches!(compression, 1 | 2) { pix.len() } else { (w * bpp).div_ceil(32) * 4 * h };
        let has_real_alpha = bpp == 32 && any_alpha;
        if !has_real_alpha {
            let mstride = w.div_ceil(32) * 4;
            if let Some(mask) = pix.get(colour_bytes..).filter(|m| m.len() >= mstride * h) {
                for y in 0..h {
                    let src_row = if top_down { y } else { h - 1 - y };
                    for x in 0..w {
                        let transparent = (mask[src_row * mstride + x / 8] >> (7 - (x % 8))) & 1 == 1;
                        rgba[(y * w + x) * 4 + 3] = if transparent { 0 } else { 255 };
                    }
                }
            } else if bpp == 32 {
                for p in rgba.chunks_exact_mut(4) {
                    p[3] = 255;
                }
            }
        }
    }
    Ok(Image::new(w as u32, h as u32, rgba))
}

/// BI_RLE8 / BI_RLE4.
fn rle(pix: &[u8], w: usize, h: usize, eight: bool, palette: &[[u8; 3]], top_down: bool, rgba: &mut [u8]) -> Result<(), String> {
    let (mut x, mut y, mut i) = (0usize, 0usize, 0usize);
    let put = |x: usize, y: usize, idx: usize, rgba: &mut [u8]| {
        if x < w && y < h {
            let row = if top_down { y } else { h - 1 - y };
            let c = palette.get(idx).copied().unwrap_or([0, 0, 0]);
            rgba[(row * w + x) * 4..(row * w + x) * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    };
    while i + 1 < pix.len() && y < h {
        let (n, v) = (pix[i] as usize, pix[i + 1]);
        i += 2;
        if n > 0 {
            for k in 0..n {
                let idx = if eight { v as usize } else if k % 2 == 0 { (v >> 4) as usize } else { (v & 15) as usize };
                put(x, y, idx, rgba);
                x += 1;
            }
        } else {
            match v {
                0 => {
                    x = 0;
                    y += 1;
                }
                1 => break,
                2 => {
                    x += *pix.get(i).ok_or("dib: unexpected end of data")? as usize;
                    y += *pix.get(i + 1).ok_or("dib: unexpected end of data")? as usize;
                    i += 2;
                }
                count => {
                    let count = count as usize;
                    let bytes = if eight { count } else { count.div_ceil(2) };
                    let run = pix.get(i..i + bytes).ok_or("dib: unexpected end of data")?;
                    for k in 0..count {
                        let idx = if eight { run[k] as usize } else if k % 2 == 0 { (run[k / 2] >> 4) as usize } else { (run[k / 2] & 15) as usize };
                        put(x, y, idx, rgba);
                        x += 1;
                    }
                    i += bytes + (bytes & 1); // runs are padded to a 16-bit boundary
                }
            }
        }
    }
    Ok(())
}

/// A `.dib` file: usually the bare DIB, sometimes a whole BMP file with the 14-byte header.
pub fn decode_dib_file(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(b"BM") {
        let off = le32(bytes, 10)? as usize;
        let body = bytes.get(14..).ok_or("dib: unexpected end of data")?;
        return decode_dib(body, false, off.checked_sub(14).filter(|o| *o >= 12));
    }
    decode_dib(bytes, false, None)
}

/// An icon or cursor file: the largest (then most colourful) picture of its directory.
pub fn decode_ico(bytes: &[u8]) -> Result<Image, String> {
    let (reserved, kind, count) = (le16(bytes, 0)?, le16(bytes, 2)?, le16(bytes, 4)? as usize);
    if reserved != 0 || !matches!(kind, 1 | 2) || count == 0 {
        return Err("ico: not an icon or cursor file (bad header)".into());
    }
    let mut best: Option<(u64, u16, usize, usize)> = None; // (pixels, bpp, size, offset)
    for i in 0..count.min(256) {
        let e = 6 + i * 16;
        let (mut w, mut h) = (*bytes.get(e).ok_or("ico: unexpected end of data")? as u64, *bytes.get(e + 1).ok_or("ico: unexpected end of data")? as u64);
        if w == 0 {
            w = 256;
        }
        if h == 0 {
            h = 256;
        }
        let bpp = if kind == 1 { le16(bytes, e + 6)? } else { 0 };
        let (size, offset) = (le32(bytes, e + 8)? as usize, le32(bytes, e + 12)? as usize);
        let key = (w * h, bpp);
        if best.map(|b| key > (b.0, b.1)).unwrap_or(true) {
            best = Some((key.0, key.1, size, offset));
        }
    }
    let (_, _, size, offset) = best.ok_or("ico: no pictures")?;
    let data = bytes.get(offset..offset.saturating_add(size).min(bytes.len())).ok_or("ico: unexpected end of data")?;
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        return crate::pngdec::decode(data, true);
    }
    decode_dib(data, true, None)
}

/// An Apple icon file: the biggest PNG or JPEG 2000 picture inside, or else the biggest of the old RLE icon types.
pub fn decode_icns(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 8 || &bytes[0..4] != b"icns" {
        return Err("icns: not an Apple icon file (bad signature)".into());
    }
    // Elements: type (4), length including these 8 bytes (4, big-endian), data.
    let mut best: Option<&[u8]> = None;
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        if len < 8 || at + len > bytes.len() {
            break;
        }
        let data = &bytes[at + 8..at + len];
        let modern = data.starts_with(&[0x89, b'P', b'N', b'G']) || data.starts_with(b"\x00\x00\x00\x0CjP  ") || data.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]);
        if modern && best.map(|b| data.len() > b.len()).unwrap_or(true) {
            best = Some(data); // the largest payload is the largest picture
        }
        at += len;
    }
    if let Some(data) = best {
        return if data.starts_with(&[0x89, b'P', b'N', b'G']) { crate::pngdec::decode(data, true) } else { crate::jp2::decode(data) };
    }
    // Old-style icons (is32, il32, ih32, it32 + masks): the icns crate knows them.
    let family = icns::IconFamily::read(std::io::Cursor::new(bytes)).map_err(|e| format!("icns: {e}"))?;
    let kind = family
        .available_icons()
        .into_iter()
        .max_by_key(|t| t.pixel_width() as u64 * t.pixel_height() as u64 * t.pixel_density() as u64)
        .ok_or("icns: the file holds no pictures")?;
    let img = family.get_icon_with_type(kind).map_err(|e| format!("icns: {e}"))?.convert_to(icns::PixelFormat::RGBA);
    Ok(Image::new(img.width(), img.height(), img.data().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2 24-bit DIB: bottom row first, BGR. Top row red, green; bottom row blue, white.
    fn dib_24() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(40u32.to_le_bytes());
        v.extend(2i32.to_le_bytes());
        v.extend(2i32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(24u16.to_le_bytes());
        v.extend([0u8; 24]); // compression .. important colours
        // Row size 6 bytes padded to 8.
        v.extend([255, 0, 0, 255, 255, 255, 0, 0]); // bottom: blue, white
        v.extend([0, 0, 255, 0, 255, 0, 0, 0]); // top: red, green
        v
    }

    #[test]
    fn a_24_bit_dib_decodes_with_the_right_orientation() {
        let img = decode_dib_file(&dib_24()).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        let px = |i: usize| [img.rgba[i * 4], img.rgba[i * 4 + 1], img.rgba[i * 4 + 2], img.rgba[i * 4 + 3]];
        assert_eq!(px(0), [255, 0, 0, 255]);
        assert_eq!(px(1), [0, 255, 0, 255]);
        assert_eq!(px(2), [0, 0, 255, 255]);
        assert_eq!(px(3), [255, 255, 255, 255]);
    }

    #[test]
    fn a_bmp_with_its_file_header_is_accepted_as_dib() {
        let dib = dib_24();
        let mut bmp = b"BM".to_vec();
        bmp.extend((14 + dib.len() as u32).to_le_bytes());
        bmp.extend([0u8; 4]);
        bmp.extend((14u32 + 40).to_le_bytes());
        bmp.extend(&dib);
        assert_eq!(decode_dib_file(&bmp).unwrap().rgba, decode_dib_file(&dib).unwrap().rgba);
    }

    #[test]
    fn eight_bit_palette_and_rle() {
        // 4x1, palette: 0 = red, 1 = blue, BI_RLE8: 3 pixels of index 0, 1 pixel of index 1, end of bitmap.
        let mut v = Vec::new();
        v.extend(40u32.to_le_bytes());
        v.extend(4i32.to_le_bytes());
        v.extend((-1i32).to_le_bytes()); // top-down
        v.extend(1u16.to_le_bytes());
        v.extend(8u16.to_le_bytes());
        v.extend(1u32.to_le_bytes()); // BI_RLE8
        v.extend([0u8; 12]);
        v.extend(2u32.to_le_bytes()); // two colours used
        v.extend(0u32.to_le_bytes());
        v.extend([0, 0, 255, 0, 255, 0, 0, 0]); // BGRA: red, blue
        v.extend([3, 0, 1, 1, 0, 1]);
        let img = decode_dib(&v, false, None).unwrap();
        assert_eq!(&img.rgba[..16], &[255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255]);
    }

    /// An icon file with one 2x2 24-bit picture whose AND mask hides the top-left pixel.
    fn ico_2x2(kind: u16) -> Vec<u8> {
        let mut dib = Vec::new();
        dib.extend(40u32.to_le_bytes());
        dib.extend(2i32.to_le_bytes());
        dib.extend(4i32.to_le_bytes()); // height counts the mask too
        dib.extend(1u16.to_le_bytes());
        dib.extend(24u16.to_le_bytes());
        dib.extend([0u8; 24]);
        dib.extend([255, 0, 0, 255, 255, 255, 0, 0]); // bottom row
        dib.extend([0, 0, 255, 0, 255, 0, 0, 0]); // top row: red, green
        dib.extend([0, 0, 0, 0, 0x80, 0, 0, 0]); // mask: bottom row opaque; top row: pixel 0 transparent
        let mut f = vec![0, 0];
        f.extend(kind.to_le_bytes());
        f.extend(1u16.to_le_bytes());
        f.extend([2, 2, 0, 0]);
        f.extend(1u16.to_le_bytes()); // planes (or hotspot x for a cursor)
        f.extend(24u16.to_le_bytes());
        f.extend((dib.len() as u32).to_le_bytes());
        f.extend(22u32.to_le_bytes());
        f.extend(dib);
        f
    }

    #[test]
    fn icons_and_cursors_use_the_and_mask() {
        for kind in [1, 2] {
            let img = decode_ico(&ico_2x2(kind)).unwrap();
            assert_eq!((img.width, img.height), (2, 2));
            assert_eq!(img.rgba[3], 0, "the masked pixel is transparent (kind {kind})");
            assert_eq!(&img.rgba[4..8], &[0, 255, 0, 255]);
            assert_eq!(img.rgba[11], 255);
        }
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_dib(b"nonsense", false, None).is_err());
        assert!(decode_dib_file(&[]).is_err());
        assert!(decode_ico(b"\0\0\x05\0\x01\0").is_err());
        assert!(decode_icns(b"icns\0\0\0\x08").is_err());
        assert!(decode_icns(b"nope").is_err());
        let mut truncated = dib_24();
        truncated.truncate(50);
        assert!(decode_dib(&truncated, false, None).is_err());
    }
}
