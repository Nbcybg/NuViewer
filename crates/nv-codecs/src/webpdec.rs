//! WebP (lossy, lossless, alpha, animated) via the pure-Rust `image-webp` decoder.

use crate::{Image, MAX_ANIM_BYTES};
use image_webp::WebPDecoder;
use std::io::Cursor;

fn to_rgba(buf: &[u8], has_alpha: bool) -> Vec<u8> {
    if has_alpha {
        buf.to_vec()
    } else {
        let mut out = Vec::with_capacity(buf.len() / 3 * 4);
        buf.chunks_exact(3).for_each(|c| out.extend_from_slice(&[c[0], c[1], c[2], 255]));
        out
    }
}

pub fn decode(bytes: &[u8], first_only: bool) -> Result<Image, String> {
    let mut dec = WebPDecoder::new(Cursor::new(bytes)).map_err(|e| format!("webp: {e}"))?;
    let (w, h) = dec.dimensions();
    crate::check_size(w as u64, h as u64)?;
    let has_alpha = dec.has_alpha();
    let size = dec.output_buffer_size().ok_or("webp: size overflow")?;
    let mut buf = vec![0u8; size];

    if dec.is_animated() && !first_only {
        let mut frames: Vec<(Vec<u8>, u32)> = Vec::new();
        for _ in 0..dec.num_frames() {
            match dec.read_frame(&mut buf) {
                Ok(ms) => frames.push((to_rgba(&buf, has_alpha), if ms <= 10 { 100 } else { ms })),
                Err(_) => break,
            }
            if (frames.len() + 1) * w as usize * h as usize * 4 > MAX_ANIM_BYTES {
                break;
            }
        }
        return Image::from_frames(w, h, frames).ok_or_else(|| "webp: no frames".to_string());
    }

    dec.read_image(&mut buf).map_err(|e| format!("webp: {e}"))?;
    let mut img = Image::new(w, h, to_rgba(&buf, has_alpha));
    img.icc = dec.icc_profile().ok().flatten(); // converted later, after any downscaling
    img.profile_assumable = true;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_roundtrip() {
        let (w, h) = (5u32, 3u32);
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 7 % 256) as u8).collect();
        let mut out = Vec::new();
        image_webp::WebPEncoder::new(&mut out).encode(&rgba, w, h, image_webp::ColorType::Rgba8).unwrap();
        let img = decode(&out, false).unwrap();
        assert_eq!((img.width, img.height), (w, h));
        assert_eq!(img.rgba, rgba);
        assert_eq!(img.frame_count(), 1);
    }
}
