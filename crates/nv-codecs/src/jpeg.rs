use crate::{orient, Image};
use zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_jpeg::JpegDecoder;

/// Decode a JPEG to sRGB RGBA with EXIF orientation applied.
/// `fallback_orientation` is used when the JPEG itself carries no orientation tag
/// (e.g. a RAW file's embedded preview, whose orientation lives in the container).
pub fn decode(bytes: &[u8], fallback_orientation: Option<u32>) -> Result<Image, String> {
    // The decoder's default 16384 px cap is lower than what we can handle (we shrink big images afterwards).
    let opts = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(1 << 16).set_max_height(1 << 16);
    let mut dec = JpegDecoder::new_with_options(bytes, opts);
    dec.decode_headers().map_err(|e| format!("jpeg: {e:?}"))?;
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

    #[test]
    fn channel_counts() {
        assert_eq!(to_rgba(vec![7, 9], 2, 1).unwrap(), vec![7, 7, 7, 255, 9, 9, 9, 255]);
        assert_eq!(to_rgba(vec![1, 2, 3], 1, 1).unwrap(), vec![1, 2, 3, 255]);
        assert_eq!(to_rgba(vec![5, 128], 1, 1).unwrap(), vec![5, 5, 5, 128]);
        assert!(to_rgba(vec![0; 5], 2, 1).is_err());
    }
}
