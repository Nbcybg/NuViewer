//! FLIF (`.flif`, the Free Lossless Image Format) through the pure-Rust `flif` crate. FLIF has been superseded by JPEG XL,
//! but old collections still hold it. The first frame of an animated FLIF is shown.

use crate::Image;
use flif::components::{BytesPerChannel, ColorSpace};

pub fn is_flif(b: &[u8]) -> bool {
    b.starts_with(b"FLIF")
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if !is_flif(bytes) {
        return Err("flif: not a FLIF file (bad signature)".into());
    }
    let limits = flif::Limits { pixels: crate::MAX_DECODE_PIXELS, ..Default::default() };
    crate::progress::decoding();
    let flif = flif::Flif::decode_with_limits(std::io::Cursor::new(bytes), limits).map_err(|e| format!("flif: {e}"))?;
    let h = &flif.info().header;
    let (w, hgt) = (h.width, h.height);
    crate::check_size(w as u64, hgt as u64)?;
    let ch = match h.channels {
        ColorSpace::Monochrome => 1usize,
        ColorSpace::RGB => 3,
        ColorSpace::RGBA => 4,
    };
    let wide = matches!(h.bytes_per_channel, BytesPerChannel::Two);
    let px = w as usize * hgt as usize;
    let per = if wide { 2 } else { 1 };
    let raw = flif.raw();
    if raw.len() < px * ch * per {
        return Err(format!("flif: the decoder returned {} bytes for {w}x{hgt}", raw.len()));
    }
    let mut rgba = Vec::with_capacity(px * 4);
    for p in raw.chunks_exact(ch * per).take(px) {
        // 16-bit samples: keep the high byte (the crate stores them big-endian, like the reference decoder's PNG output).
        let s = |c: usize| p[c * per];
        match ch {
            1 => rgba.extend_from_slice(&[s(0), s(0), s(0), 255]),
            3 => rgba.extend_from_slice(&[s(0), s(1), s(2), 255]),
            _ => rgba.extend_from_slice(&[s(0), s(1), s(2), s(3)]),
        }
    }
    let mut img = Image::new(w, hgt, rgba);
    img.profile_assumable = true;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_and_garbage() {
        assert!(is_flif(b"FLIF\x44"));
        assert!(!is_flif(b"GIF89a"));
        assert!(decode(b"not flif at all").is_err());
        assert!(decode(b"FLIF\x44\x00\x00").is_err());
    }
}
