use crate::Image;
use std::path::Path;

/// Flattened composite of a PSD or PSB (what Photoshop shows), as RGBA, from a file in memory.
///
/// The merged picture is read straight from the file's image-data section (`psdraw`): that is what Photoshop itself
/// shows, it is much faster than flattening the layers, and transparency follows Photoshop's rule (the first extra channel
/// is transparency only when the layer count is negative). Only a file saved without "Maximize Compatibility", whose merged
/// picture is blank, is flattened from its layers by the `psd` crate.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    // PSD header: signature(4) version(2) reserved(6) channels(2) height(4) width(4)
    if bytes.len() >= 22 {
        let h = u32::from_be_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]) as u64;
        let w = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as u64;
        crate::check_size(w, h)?;
    }
    let img = crate::psdraw::decode(bytes)?;
    Ok(if is_blank(&img) { flatten_layers(bytes).unwrap_or(img) } else { img })
}

/// The same for a file on disk: the layers (most of a big file) are never read.
pub fn decode_file(path: &Path) -> Result<Image, String> {
    let img = crate::psdraw::decode_path(path)?;
    if is_blank(&img) {
        if let Ok(bytes) = crate::progress::read_file(path) {
            return Ok(flatten_layers(&bytes).unwrap_or(img));
        }
    }
    Ok(img)
}

/// Every sampled pixel is the same opaque or fully transparent colour: no merged picture was saved.
fn is_blank(img: &Image) -> bool {
    if img.cmyk || img.rgba.len() < 16 {
        return false;
    }
    let step = ((img.rgba.len() / 4) / 4096).max(1) * 4;
    let first = [img.rgba[0], img.rgba[1], img.rgba[2], img.rgba[3]];
    img.rgba.chunks_exact(4).step_by(step / 4).all(|p| p == first) && (first[3] == 0 || first == [255, 255, 255, 255])
}

/// The picture rebuilt from the layers by the `psd` crate (a panic inside it just means "no result").
fn flatten_layers(bytes: &[u8]) -> Option<Image> {
    if bytes.get(4..6) == Some(&[0, 2]) {
        return None; // the crate does not read PSB
    }
    let rgba = std::panic::catch_unwind(|| {
        let psd = psd::Psd::from_bytes(bytes).ok()?;
        let (w, h) = (psd.width(), psd.height());
        let rgba = psd.flatten_layers_rgba(&|_| true).ok()?;
        (rgba.len() == w as usize * h as usize * 4).then_some((w, h, rgba, crate::psdraw::icc_profile(bytes)))
    })
    .ok()??;
    let mut img = Image::new(rgba.0, rgba.1, rgba.2);
    img.profile_assumable = true;
    img.icc = rgba.3;
    Some(img)
}
