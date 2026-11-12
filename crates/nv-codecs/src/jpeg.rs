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
    let rgba = dec.decode().map_err(|e| format!("jpeg: {e:?}"))?;
    let info = dec.info().ok_or("jpeg: no header info")?;
    let mut img = Image::new(info.width as u32, info.height as u32, rgba);
    // Colour conversion and rotation are *not* done here: both are cheaper after downscaling (ICC) or on the GPU
    // (rotation), so the image just carries what's needed to do them later.
    img.icc = dec.icc_profile();
    img.orientation = orient::exif_orientation(bytes).or(fallback_orientation).unwrap_or(1).clamp(1, 8) as u8;
    Ok(img)
}
