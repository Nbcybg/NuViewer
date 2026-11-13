//! JPEG 2000 (`.jp2`, `.j2k`, `.j2c`, `.jpf`, `.jpx`, `.jpc`) through the pure-Rust `hayro-jpeg2000` decoder, which reads both
//! the JP2 container and bare codestreams and hands back 8-bit samples.

use crate::Image;
use hayro_jpeg2000::{ColorSpace, DecodeSettings, DecoderContext};

/// True if these bytes are a JP2 file or a raw JPEG 2000 codestream.
pub fn is_jp2(b: &[u8]) -> bool {
    b.starts_with(b"\x00\x00\x00\x0C\x6A\x50\x20\x20") || b.starts_with(&[0xFF, 0x4F, 0xFF, 0x51])
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let image = hayro_jpeg2000::Image::new(bytes, &DecodeSettings::default()).map_err(|e| format!("jp2: {e}"))?;
    let (w, h) = (image.width(), image.height());
    crate::check_size(w as u64, h as u64)?;
    let space = image.color_space().clone();
    let alpha = image.has_alpha();
    crate::progress::decoding();
    let mut ctx = DecoderContext::default();
    let decoded = image.decode(&mut ctx).map_err(|e| format!("jp2: {e}"))?;
    let data = decoded.data_u8();
    let channels = space.num_channels() as usize + usize::from(alpha);
    let px = w as usize * h as usize;
    if channels == 0 || data.len() < px * channels {
        return Err(format!("jp2: decoder returned {} samples for {w}x{h} with {channels} channels", data.len()));
    }
    let mut rgba = Vec::with_capacity(px * 4);
    let colour = channels - usize::from(alpha);
    // Four colour channels without alpha are CMYK ink amounts (JP2 enumerated colour space 12, or a CMYK ICC profile):
    // they go out raw, the colour engine converts them.
    let cmyk = colour == 4;
    for p in data.chunks_exact(channels).take(px) {
        let a = if alpha { p[colour] } else { 255 };
        match colour {
            1 | 2 => rgba.extend_from_slice(&[p[0], p[0], p[0], a]),
            3 => rgba.extend_from_slice(&[p[0], p[1], p[2], a]),
            _ => rgba.extend_from_slice(&[p[0], p[1], p[2], p[3]]),
        }
    }
    let mut img = Image::new(w, h, rgba);
    if let ColorSpace::Icc { profile, num_channels } = &space {
        if *num_channels as usize == colour && (colour == 3 || colour == 4) {
            img.icc = Some(profile.clone());
        }
    }
    img.cmyk = cmyk;
    img.profile_assumable = true;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        assert!(is_jp2(b"\x00\x00\x00\x0C\x6A\x50\x20\x20rest"));
        assert!(is_jp2(&[0xFF, 0x4F, 0xFF, 0x51, 0]));
        assert!(!is_jp2(b"\xFF\xD8\xFF"));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode(b"definitely not jpeg 2000").is_err());
        assert!(decode(&[0xFF, 0x4F, 0xFF, 0x51, 0, 1, 2]).is_err());
    }
}
