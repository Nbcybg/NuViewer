use crate::Image;

/// Flattened composite of a PSD (what Photoshop shows), as RGBA.
pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    // PSD header: signature(4) version(2) reserved(6) channels(2) height(4) width(4)
    if bytes.len() >= 22 {
        let h = u32::from_be_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]) as u64;
        let w = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as u64;
        crate::check_size(w, h)?;
    }
    let psd = psd::Psd::from_bytes(bytes).map_err(|e| format!("psd: {e}"))?;
    let (w, h) = (psd.width(), psd.height());
    let rgba = psd.rgba();
    if rgba.len() != w as usize * h as usize * 4 {
        return Err("psd: unexpected composite size".into());
    }
    Ok(Image::new(w, h, rgba))
}
