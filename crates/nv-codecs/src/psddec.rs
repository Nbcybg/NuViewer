use crate::Image;

/// Flattened composite of a PSD or PSB (what Photoshop shows), as RGBA.
///
/// PSB (version 2, "large document") and anything the `psd` crate cannot read go through `psdraw`, which reads the merged
/// image-data section itself.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    // PSD header: signature(4) version(2) reserved(6) channels(2) height(4) width(4)
    if bytes.len() >= 22 {
        let h = u32::from_be_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]) as u64;
        let w = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as u64;
        crate::check_size(w, h)?;
    }
    let version = bytes.get(4..6).map(|v| u16::from_be_bytes([v[0], v[1]])).unwrap_or(0);
    if version == 2 {
        return crate::psdraw::decode(bytes);
    }
    let via_crate = std::panic::catch_unwind(|| {
        let psd = psd::Psd::from_bytes(bytes).map_err(|e| format!("psd: {e}"))?;
        let (w, h) = (psd.width(), psd.height());
        let rgba = psd.rgba();
        if rgba.len() != w as usize * h as usize * 4 {
            return Err("psd: unexpected composite size".to_string());
        }
        Ok(Image::new(w, h, rgba))
    });
    match via_crate {
        Ok(Ok(img)) => Ok(img),
        // The crate gave up (or panicked): try our own reader before reporting a failure.
        Ok(Err(e)) => crate::psdraw::decode(bytes).map_err(|own| if own.contains("unsupported") { own } else { e }),
        Err(_) => crate::psdraw::decode(bytes),
    }
}
