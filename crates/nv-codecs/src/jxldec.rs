//! JPEG XL (`.jxl`) through the pure-Rust `jxl-oxide` decoder: stills and animations, converted to sRGB by the decoder,
//! with the file's own orientation already applied.

use crate::{Frame, Image};
use jxl_oxide::{EnumColourEncoding, JxlImage, RenderingIntent};

/// A bare codestream (`FF 0A`) or the ISO container (`....JXL ....`).
pub fn is_jxl(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0x0A]) || b.starts_with(&[0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A])
}

pub fn decode(bytes: &[u8], first_only: bool) -> Result<Image, String> {
    let mut image = JxlImage::builder().read(std::io::Cursor::new(bytes)).map_err(|e| format!("jxl: {e}"))?;
    crate::check_size(image.width() as u64, image.height() as u64)?;
    // Ask for sRGB: the decoder converts whatever the file is in (wide gamut, HDR) between well-known encodings itself.
    image.request_color_encoding(EnumColourEncoding::srgb(RenderingIntent::Relative));
    let tick_ms = image.image_header().metadata.animation.as_ref().map(|a| 1000.0 * a.tps_denominator as f64 / a.tps_numerator.max(1) as f64).unwrap_or(0.0);
    crate::progress::decoding();
    let count = if first_only { 1 } else { image.num_loaded_keyframes().max(1) };
    let mut frames: Vec<(Vec<u8>, u32)> = Vec::new();
    let (mut w, mut h) = (0u32, 0u32);
    let mut bytes_used = 0usize;
    for k in 0..count {
        let render = image.render_frame(k).map_err(|e| format!("jxl: {e}"))?;
        let delay = (render.duration() as f64 * tick_ms).round().max(0.0) as u32;
        let mut stream = render.stream();
        let (fw, fh, ch) = (stream.width(), stream.height(), stream.channels() as usize);
        if fw == 0 || fh == 0 || ch == 0 {
            return Err("jxl: empty picture".into());
        }
        let mut samples = vec![0u8; fw as usize * fh as usize * ch];
        stream.write_to_buffer::<u8>(&mut samples);
        let mut rgba = Vec::with_capacity(fw as usize * fh as usize * 4);
        for p in samples.chunks_exact(ch) {
            match ch {
                1 => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
                2 => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
                3 => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
                4 => rgba.extend_from_slice(&[p[0], p[1], p[2], p[3]]),
                _ => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]), // CMYK and spot colours: show the first three
            }
        }
        if k > 0 && ((fw, fh) != (w, h) || bytes_used + rgba.len() > crate::MAX_ANIM_BYTES) {
            break;
        }
        (w, h) = (fw, fh);
        bytes_used += rgba.len();
        frames.push((rgba, delay.max(20)));
    }
    let mut it = frames.into_iter();
    let (rgba, delay_ms) = it.next().ok_or("jxl: the file holds no picture")?;
    let mut img = Image::new(w, h, rgba);
    img.delay_ms = delay_ms;
    img.extra_frames = it.map(|(rgba, delay_ms)| Frame { rgba, delay_ms }).collect();
    img.profile_assumable = false; // already sRGB
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        assert!(is_jxl(&[0xFF, 0x0A, 0x00]));
        assert!(is_jxl(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A]));
        assert!(!is_jxl(&[0xFF, 0xD8]));
    }

    #[test]
    fn a_known_tiny_file_decodes() {
        // The 1x1 white-ish codestream from jxl-oxide's own documentation (an 8x6 / 1x1 sample): whatever it holds, it must decode.
        let bytes: [u8; 42] = [
            0xff, 0x0a, 0x30, 0x54, 0x10, 0x09, 0x08, 0x06, 0x01, 0x00, 0x78, 0x00, 0x4b, 0x38, 0x41, 0x3c, 0xb6, 0x3a, 0x51, 0xfe, 0x00, 0x47, 0x1e, 0xa0, 0x85, 0xb8, 0x27, 0x1a, 0x48, 0x45, 0x84, 0x1b, 0x71, 0x4f,
            0xa8, 0x3e, 0x8e, 0x30, 0x03, 0x92, 0x84, 0x01,
        ];
        let img = decode(&bytes, false).expect("the documentation's sample file");
        assert!(img.width > 0 && img.height > 0);
        assert_eq!(img.rgba.len(), img.width as usize * img.height as usize * 4);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        assert!(decode(b"definitely not jpeg xl", false).is_err());
        assert!(decode(&[0xFF, 0x0A, 1, 2, 3], false).is_err());
    }
}
