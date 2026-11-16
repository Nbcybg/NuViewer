use crate::{orient, Image};
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

/// Decode a JPEG to sRGB RGBA with EXIF orientation applied.
/// `fallback_orientation` is used when the JPEG itself carries no orientation tag
/// (e.g. a RAW file's embedded preview, whose orientation lives in the container).
pub fn decode(bytes: &[u8], fallback_orientation: Option<u32>) -> Result<Image, String> {
    let first = match decode_zune(bytes, fallback_orientation) {
        Ok(img) => return Ok(img),
        Err(e) => e,
    };
    // Damaged files that every other viewer still shows (a cut-off download, a bogus length in a metadata block, junk
    // before a marker): first the same decoder on a repaired copy, then Windows' own JPEG codec, which is very forgiving.
    if let Some(fixed) = repair(bytes) {
        if let Ok(img) = decode_zune(&fixed, fallback_orientation) {
            return Ok(img);
        }
    }
    #[cfg(windows)]
    if let Ok(mut img) = crate::wic::decode_bytes(bytes) {
        if img.orientation <= 1 {
            img.orientation = orient::exif_orientation(bytes).or(fallback_orientation).unwrap_or(1).clamp(1, 8) as u8;
        }
        return Ok(img);
    }
    Err(first)
}

/// A copy of a damaged JPEG that a decoder is more likely to accept, or `None` when there is nothing to fix: metadata
/// segments (APPn / COM) whose declared length runs past the end of the file are cut out, and the end-of-image marker
/// is added when the file stops in the middle of the picture data (an interrupted copy or download).
fn repair(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.extend_from_slice(&bytes[..2]);
    let mut i = 2;
    let mut changed = false;
    let mut in_scan = false;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1; // stray byte between segments
            changed = true;
            continue;
        }
        let m = bytes[i + 1];
        if m == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if m == 0xD9 {
            break;
        }
        if m == 0xDA {
            in_scan = true;
            out.extend_from_slice(&bytes[i..]);
            break;
        }
        if (0xD0..=0xD8).contains(&m) || m == 0x01 {
            out.extend_from_slice(&bytes[i..i + 2]);
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let end = i + 2 + len;
        let is_meta = (0xE0..=0xEF).contains(&m) || m == 0xFE;
        // A sane segment is followed by the next marker; a metadata block whose length lands in the middle of other data lies.
        let lands_on_marker = end + 1 < bytes.len() && bytes[end] == 0xFF && bytes[end + 1] >= 0xC0 && bytes[end + 1] != 0xFF;
        if len >= 2 && end <= bytes.len() && (lands_on_marker || !is_meta) {
            out.extend_from_slice(&bytes[i..end]);
            i = end;
        } else if is_meta {
            // Resume at the next marker that can only be a real header.
            let next = (i + 2..bytes.len().saturating_sub(1)).find(|&k| bytes[k] == 0xFF && matches!(bytes[k + 1], 0xC0..=0xC2 | 0xC4 | 0xDB | 0xDA));
            changed = true;
            match next {
                Some(k) => i = k,
                None => return None,
            }
        } else {
            return None;
        }
    }
    // Trailing data after the EOI (phone "motion photo" tails) is fine; only a missing EOI matters.
    if in_scan && !bytes.windows(2).rev().take(1 << 20).any(|w| w == [0xFF, 0xD9]) {
        out.extend_from_slice(&[0xFF, 0xD9]);
        changed = true;
    }
    changed.then_some(out)
}

fn decode_zune(bytes: &[u8], fallback_orientation: Option<u32>) -> Result<Image, String> {
    // The decoder's default 16384 px cap is lower than what we can handle (we shrink big images afterwards).
    // Its default is *strict* mode, which refuses what every other viewer shows (extra bytes between headers, a progressive
    // file cut off in its last scans, ...): lenient mode decodes what is there.
    let opts = DecoderOptions::default().set_strict_mode(false).jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(1 << 16).set_max_height(1 << 16);
    let mut dec = JpegDecoder::new_with_options(bytes, opts);
    dec.decode_headers().map_err(|e| format!("jpeg: {e:?}"))?;
    // CMYK / YCCK (print-oriented JPEGs): hand the ink amounts over raw, the colour engine converts them (see `cmyk`).
    if let Some(space @ (ColorSpace::CMYK | ColorSpace::YCCK)) = dec.get_input_colorspace() {
        return decode_cmyk(bytes, space, fallback_orientation);
    }
    if let Some((w, h)) = dec.dimensions() {
        crate::check_size(w as u64, h as u64)?;
    }
    let pixels = dec.decode().map_err(|e| format!("jpeg: {e:?}"))?;
    let info = dec.info().ok_or("jpeg: no header info")?;
    // The decoder does not always honour the RGBA request: a greyscale JPEG (every scanned manga page, say) comes
    // back as one byte per pixel. Treating that as RGBA made the buffer 4x too short and crashed the viewer.
    let rgba = to_rgba(pixels, info.width as usize, info.height as usize)?;
    let mut img = Image::new(info.width as u32, info.height as u32, rgba);
    // Colour conversion and rotation are *not* done here: both are cheaper after downscaling (ICC) or on the GPU
    // (rotation), so the image just carries what's needed to do them later.
    img.icc = dec.icc_profile();
    img.profile_assumable = true;
    img.orientation = orient::exif_orientation(bytes).or(fallback_orientation).unwrap_or(1).clamp(1, 8) as u8;
    Ok(img)
}

/// A JPEG for a thumbnail: decoded with the DCT scaled down (1/2, 1/4 or 1/8, whichever still leaves at least `want` pixels on
/// the long side), which skips most of the inverse-transform work and, above all, the memory traffic of a full-size picture.
/// `None` when the file isn't a plain three-component or grey JPEG (CMYK, 12-bit, ...) or the scaled decode fails: the caller
/// then decodes it in full. Colour profile and orientation are carried like in `decode`.
pub fn decode_scaled(bytes: &[u8], want: u32, fallback_orientation: Option<u32>) -> Option<Image> {
    use jpeg_decoder::PixelFormat;
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    dec.read_info().ok()?;
    let info = dec.info()?;
    if !matches!(info.pixel_format, PixelFormat::RGB24 | PixelFormat::L8) {
        return None;
    }
    crate::check_size(info.width as u64, info.height as u64).ok()?;
    // Only worth it when it shrinks the work: below 2x the wanted size a full decode is as good.
    if (info.width.max(info.height) as u32) < want * 2 {
        return None;
    }
    let aspect = info.height as f64 / info.width as f64;
    let (rw, rh) = if aspect <= 1.0 { (want as f64, want as f64 * aspect) } else { (want as f64 / aspect, want as f64) };
    let (sw, sh) = dec.scale((rw.ceil() as u16).max(1), (rh.ceil() as u16).max(1)).ok()?;
    let pixels = dec.decode().ok()?;
    let rgba = match info.pixel_format {
        PixelFormat::L8 => to_rgba(pixels, sw as usize, sh as usize).ok()?,
        _ => to_rgba(pixels, sw as usize, sh as usize).ok()?,
    };
    let mut img = Image::new(sw as u32, sh as u32, rgba);
    img.icc = dec.icc_profile();
    img.profile_assumable = true;
    img.orientation = orient::exif_orientation(bytes).or(fallback_orientation).unwrap_or(1).clamp(1, 8) as u8;
    Some(img)
}

/// The first picture of a viewer window: decoded with the DCT scaled down just as far as still gives the fitted size for a window of
/// `win` pixels (several times quicker than a full decode for a big photo). `None` when the picture is too small to gain from it,
/// is not a plain JPEG, or the scaled decoder refuses the file; the caller then decodes it in full.
pub fn decode_scaled_for_window(bytes: &[u8], win: (u32, u32)) -> Option<Image> {
    // A file that stops without its end-of-image marker (a cut-off copy, a phone's trailer written after the picture) is given one.
    let ends = bytes.windows(2).rev().take(8192).any(|w| w == [0xFF, 0xD9]);
    if ends {
        return scaled_for_window(bytes, win);
    }
    let mut fixed = bytes.to_vec();
    fixed.extend_from_slice(&[0xFF, 0xD9]);
    scaled_for_window(&fixed, win)
}

fn scaled_for_window(bytes: &[u8], win: (u32, u32)) -> Option<Image> {
    use jpeg_decoder::PixelFormat;
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    dec.read_info().ok()?;
    let info = dec.info()?;
    if !matches!(info.pixel_format, PixelFormat::RGB24 | PixelFormat::L8) {
        return None;
    }
    let (w, h) = (info.width as f64, info.height as f64);
    let fit = (win.0 as f64 / w).min(win.1 as f64 / h).min(1.0);
    let edge = ((w.max(h) * fit).ceil() as u32).max(1);
    decode_scaled(bytes, edge, None)
}

/// The transform byte of the Adobe APP14 marker (0 = CMYK, 1 = YCbCr, 2 = YCCK), if the file has the marker. Its presence
/// is what tells that the four-channel samples are stored the Adobe way: **inverted**, 255 = no ink (Photoshop, and
/// everything that writes CMYK JPEGs for it). Without the marker the samples are plain ink amounts.
pub(crate) fn adobe_marker(bytes: &[u8]) -> Option<u8> {
    let mut i = 2;
    while i + 4 <= bytes.len() && bytes[i] == 0xFF {
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break; // image data starts: no more headers
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if marker == 0xEE && len >= 14 && bytes.get(i + 4..i + 9) == Some(b"Adobe") {
            return bytes.get(i + 4 + 11).copied();
        }
        i += 2 + len.max(2);
    }
    None
}

/// A four-component JPEG as raw ink amounts (0 = no ink): Adobe's inversion undone, YCCK turned back into CMYK (the
/// first three components of YCCK are YCbCr of the inverted C, M, Y).
fn decode_cmyk(bytes: &[u8], space: ColorSpace, fallback_orientation: Option<u32>) -> Result<Image, String> {
    let opts = DecoderOptions::default().set_strict_mode(false).jpeg_set_out_colorspace(space).set_max_width(1 << 16).set_max_height(1 << 16);
    let mut dec = JpegDecoder::new_with_options(bytes, opts);
    dec.decode_headers().map_err(|e| format!("jpeg: {e:?}"))?;
    if let Some((w, h)) = dec.dimensions() {
        crate::check_size(w as u64, h as u64)?;
    }
    let mut px = dec.decode().map_err(|e| format!("jpeg: {e:?}"))?;
    let info = dec.info().ok_or("jpeg: no header info")?;
    let (w, h) = (info.width as usize, info.height as usize);
    if w * h == 0 || px.len() != w * h * 4 {
        return Err(format!("jpeg: CMYK decoder returned {} bytes for {w}x{h}", px.len()));
    }
    let inverted = adobe_marker(bytes).is_some();
    if space == ColorSpace::YCCK {
        for p in px.chunks_exact_mut(4) {
            let (y, cb, cr) = (p[0] as f32, p[1] as f32 - 128.0, p[2] as f32 - 128.0);
            let r = (y + 1.402 * cr).round().clamp(0.0, 255.0) as u8;
            let g = (y - 0.344_136 * cb - 0.714_136 * cr).round().clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * cb).round().clamp(0.0, 255.0) as u8;
            // stored C, M, Y = 255 - R, G, B; K as it is.
            p[0] = 255 - r;
            p[1] = 255 - g;
            p[2] = 255 - b;
        }
    }
    if inverted {
        px.iter_mut().for_each(|v| *v = 255 - *v);
    }
    let mut img = Image::new(w as u32, h as u32, px);
    img.cmyk = true;
    img.icc = dec.icc_profile();
    img.orientation = orient::exif_orientation(bytes).or(fallback_orientation).unwrap_or(1).clamp(1, 8) as u8;
    Ok(img)
}

/// Bring decoder output of 1 (grey), 2 (grey + alpha), 3 (RGB) or 4 (RGBA) bytes per pixel to RGBA.
fn to_rgba(pixels: Vec<u8>, w: usize, h: usize) -> Result<Vec<u8>, String> {
    let n = w * h;
    if n == 0 {
        return Err("jpeg: empty picture".into());
    }
    if pixels.len() == n * 4 {
        return Ok(pixels);
    }
    let mut out = Vec::with_capacity(n * 4);
    match (pixels.len() / n, pixels.len() % n) {
        (1, 0) => pixels.iter().for_each(|&g| out.extend_from_slice(&[g, g, g, 255])),
        (2, 0) => pixels.chunks_exact(2).for_each(|p| out.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
        (3, 0) => pixels.chunks_exact(3).for_each(|p| out.extend_from_slice(&[p[0], p[1], p[2], 255])),
        _ => return Err(format!("jpeg: decoder returned {} bytes for {w}x{h}", pixels.len())),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16x8 greyscale (1 component) baseline JPEG made with PIL; the decoder hands back 1 byte per pixel for it.
    const GREY_16X8: &[u8] = include_bytes!("../tests/grey_16x8.jpg");

    #[test]
    fn greyscale_jpeg_becomes_full_rgba() {
        let img = decode(GREY_16X8, None).expect("decodes");
        assert_eq!((img.width, img.height), (16, 8));
        assert_eq!(img.rgba.len(), 16 * 8 * 4);
        assert!(img.rgba.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255));
        // and it can be shrunk without running off the end of the buffer
        let small = crate::downscale(&img, 4);
        assert_eq!(small.rgba.len(), (small.width * small.height * 4) as usize);
    }

    /// 16x8 CMYK (4 component, Adobe) JPEG of a grey ramp (x * 16), made with PIL.
    const CMYK_16X8: &[u8] = include_bytes!("../tests/cmyk_16x8.jpg");

    #[test]
    fn cmyk_jpeg_is_raw_ink_not_rgb() {
        // PIL stores CMYK JPEGs the Adobe way (inverted) and reads them back inverted too: the picture is a ramp with C, M, Y
        // empty and the black ink falling from 255 (x = 0) to 15 (x = 15). Undoing the inversion must give exactly that.
        let img = decode(CMYK_16X8, None).expect("decodes");
        assert_eq!((img.width, img.height), (16, 8));
        assert!(img.cmyk, "marked as raw CMYK");
        assert_eq!(img.rgba.len(), 16 * 8 * 4);
        for y in 0..8 {
            for x in 0..16 {
                let p = &img.rgba[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
                let want_k = 255 - 16 * x as i32;
                assert!(p[..3].iter().all(|&v| v <= 12), "no colour ink at ({x},{y}): {p:?}");
                assert!((p[3] as i32 - want_k).abs() <= 14, "black ink at ({x},{y}): {p:?}, want about {want_k}");
            }
        }
    }

    /// A 32x16 picture, left half and right half each one flat ink mix, encoded as YCCK (the CMYK transform Photoshop uses
    /// for its JPEGs) at top quality. jpeg-encoder, like libjpeg's Adobe mode, stores CMYK inverted (255 = no ink) itself.
    fn ycck_jpeg(left: [u8; 4], right: [u8; 4]) -> Vec<u8> {
        let mut px = Vec::new();
        for _y in 0..16 {
            for x in 0..32 {
                px.extend_from_slice(&if x < 16 { left } else { right });
            }
        }
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 100).encode(&px, 32, 16, jpeg_encoder::ColorType::CmykAsYcck).expect("encodes");
        out
    }

    #[test]
    fn ycck_jpeg_comes_out_as_raw_inks_too() {
        let jpg = ycck_jpeg([255, 0, 0, 0], [30, 120, 200, 90]);
        assert_eq!(adobe_marker(&jpg), Some(2), "YCCK transform in the Adobe marker");
        let img = decode(&jpg, None).expect("decodes");
        assert!(img.cmyk);
        let at = |x: usize| img.rgba[(8 * 32 + x) * 4..(8 * 32 + x) * 4 + 4].to_vec();
        for (got, want) in [(at(6), [255i32, 0, 0, 0]), (at(24), [30, 120, 200, 90])] {
            for c in 0..4 {
                assert!((got[c] as i32 - want[c]).abs() <= 8, "ink {c}: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn without_the_adobe_marker_the_samples_are_plain_ink_amounts() {
        // jpeg-encoder writes the marker for CMYK (and, like libjpeg, stores what it is given inverted for Adobe readers);
        // cut the APP14 segment out to get a marker-less file whose stored samples are (200, 0, 0, 0).
        let mut px = Vec::new();
        for _ in 0..16 * 16 {
            px.extend_from_slice(&[55u8, 255, 255, 255]);
        }
        let mut jpg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpg, 100).encode(&px, 16, 16, jpeg_encoder::ColorType::Cmyk).expect("encodes");
        assert_eq!(adobe_marker(&jpg), Some(0));
        let marker_at = jpg.windows(2).position(|w| w == [0xFF, 0xEE]).expect("APP14");
        jpg.drain(marker_at..marker_at + 2 + 14);
        assert_eq!(adobe_marker(&jpg), None);
        let img = decode(&jpg, None).expect("decodes");
        assert!(img.cmyk);
        assert!((img.rgba[0] as i32 - 200).abs() <= 6 && img.rgba[1] <= 6, "cyan 200 stays cyan 200: {:?}", &img.rgba[..4]);
    }

    fn plain_jpeg(progressive: bool) -> Vec<u8> {
        let mut px = Vec::new();
        for y in 0..64u32 {
            for x in 0..96u32 {
                px.extend_from_slice(&[(x * 2) as u8, (y * 3) as u8, ((x * y) % 251) as u8]);
            }
        }
        let mut out = Vec::new();
        let mut enc = jpeg_encoder::Encoder::new(&mut out, 85);
        enc.set_progressive(progressive);
        enc.encode(&px, 96, 64, jpeg_encoder::ColorType::Rgb).expect("encodes");
        out
    }

    #[test]
    fn cut_off_files_still_open() {
        for progressive in [false, true] {
            let jpg = plain_jpeg(progressive);
            assert_eq!(decode(&jpg, None).expect("whole file").width, 96);
            // missing end marker, and cut in the middle of the picture data
            for cut in [2, jpg.len() / 2] {
                let img = decode(&jpg[..jpg.len() - cut.min(jpg.len() - 600)], None).unwrap_or_else(|e| panic!("progressive={progressive} cut={cut}: {e}"));
                assert_eq!((img.width, img.height), (96, 64));
            }
        }
    }

    #[test]
    fn metadata_length_past_the_end_is_cut_out() {
        let jpg = plain_jpeg(false);
        // insert an APP1 block that claims to be 60000 bytes long right after SOI
        let mut bad = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xEA, 0x60, b'E', b'x', b'i', b'f', 0, 0];
        bad.extend_from_slice(&jpg[2..]);
        let fixed = repair(&bad).expect("needs a repair");
        assert!(decode_zune(&fixed, None).is_ok());
        let img = decode(&bad, None).expect("opens via the repaired copy");
        assert_eq!((img.width, img.height), (96, 64));
    }

    #[test]
    fn channel_counts() {
        assert_eq!(to_rgba(vec![7, 9], 2, 1).unwrap(), vec![7, 7, 7, 255, 9, 9, 9, 255]);
        assert_eq!(to_rgba(vec![1, 2, 3], 1, 1).unwrap(), vec![1, 2, 3, 255]);
        assert_eq!(to_rgba(vec![5, 128], 1, 1).unwrap(), vec![5, 5, 5, 128]);
        assert!(to_rgba(vec![0; 5], 2, 1).is_err());
    }
}
