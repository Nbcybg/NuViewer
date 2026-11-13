//! More raster formats, through the `image` crate (TGA, PNM family, Radiance HDR, OpenEXR, QOI) and `image_dds`
//! (DDS textures, including the block-compressed BC1-BC7 kinds games use).
//!
//! HDR and EXR hold linear floating-point light, not display values: they are tone mapped (only when something
//! exceeds 1.0) and encoded to sRGB so they look like a picture instead of a dark, clipped one.

use crate::Image;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::io::Cursor;

/// Extensions handled here (lowercase). `icb`, `vda` and `vst` are old names of TGA files.
pub const EXTS: &[&str] = &["tga", "icb", "vda", "vst", "dds", "ppm", "pgm", "pbm", "pam", "pnm", "hdr", "rgbe", "exr", "qoi"];

/// Floating-point pictures take 16 bytes a pixel while decoding: refuse the really huge ones.
const MAX_FLOAT_PIXELS: u64 = 80_000_000;

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext)
}

fn format_of(ext: &str) -> Option<ImageFormat> {
    Some(match ext {
        "tga" | "icb" | "vda" | "vst" => ImageFormat::Tga,
        "ppm" | "pgm" | "pbm" | "pam" | "pnm" => ImageFormat::Pnm,
        "hdr" | "rgbe" => ImageFormat::Hdr, // Radiance RGBE

        "exr" => ImageFormat::OpenExr,
        "qoi" => ImageFormat::Qoi,
        _ => return None,
    })
}

pub fn decode(bytes: &[u8], ext: &str) -> Result<Image, String> {
    if ext == "dds" {
        return decode_dds(bytes);
    }
    let fmt = format_of(ext).ok_or_else(|| format!("unsupported format: .{ext}"))?;
    let reader = || ImageReader::with_format(Cursor::new(bytes), fmt);
    let (w, h) = reader().into_dimensions().map_err(|e| format!(".{ext}: {e}"))?;
    crate::check_size(w as u64, h as u64)?;
    if matches!(fmt, ImageFormat::Hdr | ImageFormat::OpenExr) && w as u64 * h as u64 > MAX_FLOAT_PIXELS {
        return Err(format!("image too large to open ({w}x{h} floating-point pixels)"));
    }
    crate::progress::decoding();
    let mut r = reader();
    r.no_limits(); // the size was checked above
    let decoded = r.decode().map_err(|e| format!(".{ext}: {e}"))?;
    Ok(from_dynamic(decoded))
}

fn decode_dds(bytes: &[u8]) -> Result<Image, String> {
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(bytes)).map_err(|e| format!("dds: {e}"))?;
    crate::check_size(dds.get_width() as u64, dds.get_height() as u64)?;
    crate::progress::decoding();
    let img = image_dds::image_from_dds(&dds, 0).map_err(|e| format!("dds: {e}"))?;
    Ok(Image::new(img.width(), img.height(), img.into_raw()))
}

fn from_dynamic(d: DynamicImage) -> Image {
    match d {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
            let f = d.to_rgba32f();
            let (w, h) = f.dimensions();
            Image::new(w, h, tone_map(f.into_raw()))
        }
        other => {
            let rgba = other.to_rgba8();
            let (w, h) = rgba.dimensions();
            Image::new(w, h, rgba.into_raw())
        }
    }
}

/// Linear float RGBA -> sRGB RGBA8. Values within 0..1 are only gamma encoded; brighter ones are compressed with the
/// extended Reinhard curve so the brightest common value (the 99.5th percentile) maps to white.
fn tone_map(px: Vec<f32>) -> Vec<u8> {
    // White point from a sample of the brightest channel of each pixel.
    let pixels = px.len() / 4;
    let step = (pixels / 200_000).max(1);
    let mut samples: Vec<f32> = (0..pixels).step_by(step).map(|i| px[i * 4].max(px[i * 4 + 1]).max(px[i * 4 + 2])).filter(|v| v.is_finite()).collect();
    samples.sort_by(|a, b| a.total_cmp(b));
    let top = samples.get(samples.len().saturating_sub(1) * 995 / 1000).copied().unwrap_or(1.0);
    let white = top.max(1.0);
    let compress = white > 1.0;

    const N: usize = 4096;
    let lut: Vec<u8> = (0..=N)
        .map(|i| {
            let v = i as f32 / N as f32;
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        })
        .collect();
    let enc = |c: f32| -> u8 {
        let c = if c.is_finite() { c.max(0.0) } else { 0.0 };
        let c = if compress { c * (1.0 + c / (white * white)) / (1.0 + c) } else { c.min(1.0) };
        lut[(c.clamp(0.0, 1.0) * N as f32 + 0.5) as usize]
    };
    let mut out = Vec::with_capacity(px.len());
    for p in px.chunks_exact(4) {
        out.extend_from_slice(&[enc(p[0]), enc(p[1]), enc(p[2]), (p[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2x2 uncompressed 24-bit TGA: red, green / blue, white, with the bottom-left origin the format defaults to.
    fn tga_2x2() -> Vec<u8> {
        let mut v = vec![0u8; 18];
        v[2] = 2; // uncompressed true colour
        v[12] = 2; // width
        v[14] = 2; // height
        v[16] = 24; // bits per pixel
        v[17] = 0; // origin bottom-left, no alpha
        // rows from the bottom: blue, white; red, green  (BGR order)
        v.extend_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0, 255, 0, 255, 0]);
        v
    }

    #[test]
    fn tga_decodes_with_the_right_orientation() {
        let img = decode(&tga_2x2(), "tga").unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        let px = |i: usize| [img.rgba[i * 4], img.rgba[i * 4 + 1], img.rgba[i * 4 + 2], img.rgba[i * 4 + 3]];
        assert_eq!(px(0), [255, 0, 0, 255], "top-left is red");
        assert_eq!(px(1), [0, 255, 0, 255], "top-right is green");
        assert_eq!(px(2), [0, 0, 255, 255], "bottom-left is blue");
        assert_eq!(px(3), [255, 255, 255, 255], "bottom-right is white");
    }

    #[test]
    fn rle_tga_decodes_too() {
        // 4x1 run-length encoded: one packet repeating (0, 128, 255 in BGR) four times.
        let mut v = vec![0u8; 18];
        v[2] = 10; // RLE true colour
        v[12] = 4;
        v[14] = 1;
        v[16] = 24;
        v[17] = 0x20; // top-left origin
        v.extend_from_slice(&[0x80 | 3, 255, 128, 0]);
        let img = decode(&v, "tga").unwrap();
        assert_eq!((img.width, img.height), (4, 1));
        assert!(img.rgba.chunks_exact(4).all(|p| p == [0, 128, 255, 255]));
    }

    #[test]
    fn ppm_and_pgm_decode() {
        let ppm = b"P6\n2 1\n255\n\xff\x00\x00\x00\x00\xff";
        let img = decode(ppm, "ppm").unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 0, 255, 255]);
        let pgm = b"P5\n1 1\n255\n\x80";
        assert_eq!(&decode(pgm, "pgm").unwrap().rgba[..4], &[128, 128, 128, 255]);
    }

    #[test]
    fn qoi_decodes() {
        // 1x1 opaque red QOI: header + one QOI_OP_RGB + end marker.
        let mut q = b"qoif".to_vec();
        q.extend_from_slice(&1u32.to_be_bytes());
        q.extend_from_slice(&1u32.to_be_bytes());
        q.extend_from_slice(&[3, 0]); // RGB, sRGB
        q.extend_from_slice(&[0xFE, 255, 0, 0]);
        q.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        let img = decode(&q, "qoi").unwrap();
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn radiance_hdr_is_tone_mapped_not_clipped_black() {
        // 2x1 flat-RGBE: a mid grey (0.5) and a bright pixel (4.0).
        let mut f = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 2\n".to_vec();
        f.extend_from_slice(&[128, 128, 128, 128]); // 0.5 each (mantissa 128, exponent 128 -> 128/256 * 2^(128-128) = 0.5)
        f.extend_from_slice(&[128, 128, 128, 131]); // 4.0
        let img = decode(&f, "hdr").unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        let (a, b) = (img.rgba[0], img.rgba[4]);
        assert!(a > 60 && a < 255, "mid grey stays a mid grey: {a}");
        assert!(b > a, "the bright pixel is brighter: {b} vs {a}");
    }

    #[test]
    fn tone_mapping_leaves_ordinary_values_alone() {
        // Everything within 0..1: only gamma encoding. Linear 0.5 -> sRGB 188.
        let out = tone_map(vec![0.5, 0.5, 0.5, 1.0, 0.0, 1.0, 0.25, 0.5]);
        assert!((out[0] as i32 - 188).abs() <= 1, "{}", out[0]);
        assert_eq!(&out[4..8], &[0, 255, 137, 128]);
    }


    #[test]
    fn uncompressed_dds_decodes() {
        // Legacy header, A8R8G8B8, 2x1 pixels: BGRA in memory.
        let mut v = b"DDS ".to_vec();
        let mut header = vec![0u32; 31];
        header[0] = 124; // size
        header[1] = 0x1 | 0x2 | 0x4 | 0x1000 | 0x8; // caps, height, width, pixelformat, pitch
        header[2] = 1; // height
        header[3] = 2; // width
        header[4] = 8; // pitch
        header[18] = 32; // pixel format size
        header[19] = 0x1 | 0x40; // alpha pixels + rgb
        header[21] = 32; // bits
        header[22] = 0x00FF_0000;
        header[23] = 0x0000_FF00;
        header[24] = 0x0000_00FF;
        header[25] = 0xFF00_0000;
        header[26] = 0x1000; // caps: texture
        for h in header {
            v.extend(h.to_le_bytes());
        }
        v.extend_from_slice(&[0, 0, 255, 255, 255, 0, 0, 128]); // red opaque, blue half transparent (BGRA)
        let img = decode(&v, "dds").unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 0, 255, 128]);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        for ext in EXTS {
            assert!(decode(b"definitely not an image", ext).is_err(), "{ext}");
        }
    }
}
