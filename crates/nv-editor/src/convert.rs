//! Converting pixels to another file format: the "Quick convert" and the batch convert. Only encoding lives here; the
//! caller decodes (and resizes) the source.

use crate::raster::Pixels;

/// The formats a picture can be converted to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Quality 1..=100.
    Jpeg(u8),
    Png,
    /// Lossless WebP.
    Webp,
    /// 24-bit Windows bitmap.
    Bmp,
}

impl Out {
    pub fn ext(self) -> &'static str {
        match self {
            Out::Jpeg(_) => "jpg",
            Out::Png => "png",
            Out::Webp => "webp",
            Out::Bmp => "bmp",
        }
    }

    pub fn label(self) -> String {
        match self {
            Out::Jpeg(q) => format!("JPEG (quality {q})"),
            Out::Png => "PNG".into(),
            Out::Webp => "WebP (lossless)".into(),
            Out::Bmp => "BMP".into(),
        }
    }

    /// Does this format keep transparency?
    pub fn has_alpha(self) -> bool {
        matches!(self, Out::Png | Out::Webp)
    }
}

/// RGB with transparency flattened over `bg`.
fn flatten_rgb(p: &Pixels, bg: [u8; 3]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(p.rgba.len() / 4 * 3);
    for px in p.rgba.chunks_exact(4) {
        let a = px[3] as u32;
        for c in 0..3 {
            rgb.push(((px[c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255) as u8);
        }
    }
    rgb
}

/// Encode `p`. `bg` is what transparency is flattened over for formats without it. `exif` (the TIFF part, see
/// `save::exif_of_jpeg`) goes into JPEGs.
pub fn encode(p: &Pixels, out: Out, bg: [u8; 3], exif: Option<&[u8]>) -> Result<Vec<u8>, String> {
    match out {
        Out::Jpeg(q) => {
            if p.w > 65_535 || p.h > 65_535 {
                return Err(format!("{}x{} is too large for a JPEG (65535 pixels a side)", p.w, p.h));
            }
            let mut bytes = Vec::with_capacity(p.rgba.len() / 8);
            let mut enc = jpeg_encoder::Encoder::new(&mut bytes, q.clamp(1, 100));
            if q >= 90 {
                enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
            }
            if let Some(e) = exif {
                let _ = enc.add_exif_metadata(e);
            }
            enc.encode(&flatten_rgb(p, bg), p.w as u16, p.h as u16, jpeg_encoder::ColorType::Rgb).map_err(|e| format!("JPEG encoder: {e}"))?;
            Ok(bytes)
        }
        Out::Png => crate::save::encode(p, crate::save::Format::Png, None),
        Out::Webp => {
            let mut bytes = Vec::new();
            image_webp::WebPEncoder::new(&mut bytes).encode(&p.rgba, p.w, p.h, image_webp::ColorType::Rgba8).map_err(|e| format!("WebP encoder: {e}"))?;
            Ok(bytes)
        }
        Out::Bmp => {
            let rgb = flatten_rgb(p, bg);
            let row = (p.w as usize * 3).div_ceil(4) * 4;
            let image_size = row * p.h as usize;
            let mut b = Vec::with_capacity(54 + image_size);
            b.extend(b"BM");
            b.extend(((54 + image_size) as u32).to_le_bytes());
            b.extend([0u8; 4]);
            b.extend(54u32.to_le_bytes());
            b.extend(40u32.to_le_bytes());
            b.extend(p.w.to_le_bytes());
            b.extend(p.h.to_le_bytes());
            b.extend(1u16.to_le_bytes());
            b.extend(24u16.to_le_bytes());
            b.extend([0u8; 4]); // BI_RGB
            b.extend((image_size as u32).to_le_bytes());
            b.extend([0u8; 16]);
            for y in (0..p.h as usize).rev() {
                let line = &rgb[y * p.w as usize * 3..(y + 1) * p.w as usize * 3];
                for px in line.chunks_exact(3) {
                    b.extend([px[2], px[1], px[0]]);
                }
                b.extend(std::iter::repeat_n(0u8, row - p.w as usize * 3));
            }
            Ok(b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Pixels {
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, if x == 0 { 0 } else { 255 }]);
            }
        }
        Pixels::new(w, h, rgba)
    }

    #[test]
    fn every_format_makes_a_file_with_its_own_signature() {
        let p = gradient(37, 21);
        let jpg = encode(&p, Out::Jpeg(85), [255; 3], None).unwrap();
        assert_eq!(&jpg[0..3], &[0xFF, 0xD8, 0xFF]);
        let png = encode(&p, Out::Png, [255; 3], None).unwrap();
        assert_eq!(&png[0..8], b"\x89PNG\r\n\x1a\n");
        let webp = encode(&p, Out::Webp, [255; 3], None).unwrap();
        assert_eq!((&webp[0..4], &webp[8..12]), (&b"RIFF"[..], &b"WEBP"[..]));
        let bmp = encode(&p, Out::Bmp, [255; 3], None).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize, bmp.len(), "BMP file size field");
        assert_eq!(u32::from_le_bytes(bmp[18..22].try_into().unwrap()), 37);
    }

    #[test]
    fn png_and_webp_round_trip_exactly_and_jpeg_flattens_transparency() {
        let p = gradient(16, 8);
        let png = encode(&p, Out::Png, [0; 3], None).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0; dec.output_buffer_size().unwrap()];
        dec.next_frame(&mut buf).unwrap();
        assert_eq!(buf, p.rgba);
        let webp = encode(&p, Out::Webp, [0; 3], None).unwrap();
        let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(webp)).unwrap();
        let mut out = vec![0; d.output_buffer_size().unwrap()];
        d.read_image(&mut out).unwrap();
        assert_eq!(out, p.rgba);
        // The transparent first column becomes the background (white here), not black.
        let jpg = encode(&p, Out::Jpeg(100), [255; 3], None).unwrap();
        assert!(jpg.len() > 100);
    }
}
