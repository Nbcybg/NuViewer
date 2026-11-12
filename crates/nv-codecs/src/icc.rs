//! Convert RGBA8 pixels from an embedded ICC profile to sRGB (what the display path assumes).

use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};

/// Pixels converted per call, so we never need a full-image scratch copy.
const CHUNK_PIXELS: usize = 1 << 16;

/// Converts `rgba` in place. Does nothing for sRGB profiles, non-RGB profiles, or anything we can't parse.
pub fn to_srgb_in_place(rgba: &mut [u8], icc: &[u8]) {
    // Fast path: the overwhelmingly common profile is already sRGB (its description says so).
    if icc.windows(4).any(|w| w == b"sRGB") {
        return;
    }
    let Ok(src) = ColorProfile::new_from_slice(icc) else { return };
    if src.color_space != DataColorSpace::Rgb {
        return;
    }
    let dst = ColorProfile::new_srgb();
    let Ok(t) = src.create_transform_8bit(Layout::Rgba, &dst, Layout::Rgba, TransformOptions::default()) else { return };
    // Big images are split across threads (the transform is immutable and shareable); small ones stay on this thread.
    let pixels = rgba.len() / 4;
    let threads = if pixels < 1_000_000 { 1 } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 6) };
    let per_thread = (pixels.div_ceil(threads)).max(1) * 4;
    let background = crate::progress::is_background();
    std::thread::scope(|scope| {
        for part in rgba.chunks_mut(per_thread) {
            let t = &t;
            scope.spawn(move || {
                crate::progress::set_background(background);
                let mut tmp = vec![0u8; CHUNK_PIXELS * 4];
                for chunk in part.chunks_mut(CHUNK_PIXELS * 4) {
                    let n = chunk.len();
                    tmp[..n].copy_from_slice(chunk);
                    let _ = t.transform(&tmp[..n], chunk);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with_profile(rgb: [u8; 3], profile: Option<&[u8]>) -> Vec<u8> {
        let (w, h) = (16u16, 16u16);
        let data: Vec<u8> = rgb.iter().copied().cycle().take(w as usize * h as usize * 3).collect();
        let mut out = Vec::new();
        let mut enc = jpeg_encoder::Encoder::new(&mut out, 100);
        if let Some(p) = profile {
            enc.add_icc_profile(p).unwrap();
        }
        enc.encode(&data, w, h, jpeg_encoder::ColorType::Rgb).unwrap();
        out
    }

    fn centre_pixel(jpeg: &[u8]) -> [u8; 3] {
        let img = crate::decode_jpeg(jpeg).unwrap(); // the public entry point finishes the colour conversion
        let i = ((8 * img.width + 8) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }

    #[test]
    fn display_p3_is_converted_but_untagged_and_srgb_are_not() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let srgb = ColorProfile::new_srgb().encode().unwrap();
        let colour = [200u8, 60, 60];

        let untagged = centre_pixel(&jpeg_with_profile(colour, None));
        let tagged_srgb = centre_pixel(&jpeg_with_profile(colour, Some(&srgb)));
        let tagged_p3 = centre_pixel(&jpeg_with_profile(colour, Some(&p3)));

        assert_eq!(untagged, tagged_srgb, "sRGB-tagged JPEG must be left alone");
        let diff: i32 = (0..3).map(|c| (tagged_p3[c] as i32 - untagged[c] as i32).abs()).sum();
        assert!(diff > 15, "P3 colour should shift when mapped to sRGB: {untagged:?} -> {tagged_p3:?}");
    }

    #[test]
    fn neutral_grey_survives_p3_conversion() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let g = centre_pixel(&jpeg_with_profile([128, 128, 128], Some(&p3)));
        assert!(g.iter().all(|&c| (c as i32 - 128).abs() <= 3), "{g:?}");
    }

    #[test]
    fn garbage_profile_is_ignored() {
        let mut rgba = vec![10u8, 20, 30, 255];
        to_srgb_in_place(&mut rgba, &[1, 2, 3, 4]);
        assert_eq!(rgba, [10, 20, 30, 255]);
    }
}
