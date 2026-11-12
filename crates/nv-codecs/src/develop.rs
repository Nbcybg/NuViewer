//! Half-size "development" of camera RAW sensor data, for RAWs whose embedded preview is too small to be useful.
//!
//! `rawler` decodes the file (all the vendor compression schemes, tiles decoded in parallel); here every 2x2 Bayer
//! block becomes one output pixel (so no demosaic is needed and the result is a clean 1/4-area picture), followed by
//! black/white level, camera white balance, the camera -> sRGB matrix and the sRGB transfer curve. Colour is "plain",
//! without the camera maker's tone curve, but sharp and complete, which beats a 1 MP thumbnail.

use crate::Image;
use rawler::decoders::RawDecodeParams;
use rawler::rawimage::RawImageData;
use rawler::rawsource::RawSource;
use std::path::Path;

/// sRGB (D65) -> XYZ.
const SRGB_TO_XYZ: [[f32; 3]; 3] = [[0.412_456_4, 0.357_576_1, 0.180_437_5], [0.212_672_9, 0.715_152_2, 0.072_175_0], [0.019_333_9, 0.119_192_0, 0.950_304_1]];

type M3 = [[f32; 3]; 3];

fn mul(a: &M3, b: &M3) -> M3 {
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    o
}

fn invert(m: &M3) -> Option<M3> {
    let c = |i: usize, j: usize| m[(i + 1) % 3][(j + 1) % 3] * m[(i + 2) % 3][(j + 2) % 3] - m[(i + 1) % 3][(j + 2) % 3] * m[(i + 2) % 3][(j + 1) % 3];
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    if !det.is_finite() || det.abs() < 1e-6 {
        return None;
    }
    let mut o = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            o[i][j] = c(j, i) / det; // adjugate = transposed cofactors
        }
    }
    Some(o)
}

/// Camera RGB (white balanced, 0..1) -> linear sRGB, from the DNG-style XYZ -> camera matrix. Rows of the
/// sRGB -> camera matrix are normalised so that white stays white (the same recipe as dcraw).
pub(crate) fn camera_to_srgb(xyz_to_cam: &[[f32; 3]]) -> M3 {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if xyz_to_cam.len() < 3 {
        return identity;
    }
    let x2c: M3 = [xyz_to_cam[0], xyz_to_cam[1], xyz_to_cam[2]];
    let mut rgb_to_cam = mul(&x2c, &SRGB_TO_XYZ);
    for row in rgb_to_cam.iter_mut() {
        let s: f32 = row.iter().sum();
        if !s.is_finite() || s.abs() < 1e-6 {
            return identity;
        }
        row.iter_mut().for_each(|v| *v /= s);
    }
    invert(&rgb_to_cam).unwrap_or(identity)
}

/// Linear 0..1 -> sRGB byte, through a lookup table.
fn gamma_table() -> Vec<u8> {
    (0..4096)
        .map(|i| {
            let l = i as f32 / 4095.0;
            let s = if l <= 0.003_130_8 { 12.92 * l } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5).clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Develop `path` at half the sensor resolution. Errors (unsupported layout, rawler failure) mean "use the preview".
pub fn develop_half(path: &Path) -> Result<Image, String> {
    let src = RawSource::new(path).map_err(|e| format!("open: {e}"))?;
    let dec = rawler::get_decoder(&src).map_err(|e| format!("raw: {e}"))?;
    let params = RawDecodeParams::default();
    let raw = dec.raw_image(&src, &params, false).map_err(|e| format!("raw: {e}"))?;
    let orientation = dec.raw_metadata(&src, &params).ok().and_then(|m| m.exif.orientation).filter(|o| (1..=8).contains(o)).unwrap_or(1) as u8;

    if raw.cpp != 1 {
        return Err("RAW is not a single-channel mosaic".into());
    }
    let cfa = &raw.camera.cfa;
    if cfa.width != 2 || cfa.height != 2 || !cfa.is_valid() {
        return Err(format!("unsupported colour filter layout {}", cfa.name));
    }
    let RawImageData::Integer(data) = &raw.data else { return Err("floating point RAW data is not supported".into()) };
    let (rw, rh) = (raw.width, raw.height);
    if data.len() < rw * rh {
        return Err("short RAW data".into());
    }

    // Crop to the recommended area, starting on an even pixel so the Bayer phase is unchanged.
    let (cx, cy, cw, ch) = match raw.crop_area.or(raw.active_area) {
        Some(r) if r.d.w >= 2 && r.d.h >= 2 && r.p.x + r.d.w <= rw && r.p.y + r.d.h <= rh => (r.p.x, r.p.y, r.d.w, r.d.h),
        _ => (0, 0, rw, rh),
    };
    let (x0, y0) = ((cx + 1) & !1, (cy + 1) & !1);
    let ow = (cx + cw).saturating_sub(x0) / 2;
    let oh = (cy + ch).saturating_sub(y0) / 2;
    if ow == 0 || oh == 0 {
        return Err("empty RAW area".into());
    }
    crate::check_size(ow as u64, oh as u64)?;

    // Levels, white balance, colour matrix.
    let black = raw.blacklevel.as_bayer_array();
    let white_of = |color: usize| -> f32 {
        let w = &raw.whitelevel.0;
        (if w.len() >= 4 { w[color.min(3)] } else { w.first().copied().unwrap_or(65535) }) as f32
    };
    let mut color_at = [[0usize; 2]; 2];
    for (dy, row) in color_at.iter_mut().enumerate() {
        for (dx, c) in row.iter_mut().enumerate() {
            *c = cfa.color_at(dy, dx).min(2); // a 4th colour (emerald) is treated as green
        }
    }
    let scale: [f32; 4] = std::array::from_fn(|i| {
        let color = color_at[i / 2][i % 2];
        1.0 / (white_of(color) - black[i]).max(1.0)
    });
    let wb = {
        let w = raw.wb_coeffs;
        let ok = w[..3].iter().all(|v| v.is_finite() && *v > 0.0);
        if ok { [w[0] / w[1], 1.0, w[2] / w[1]] } else { [1.0, 1.0, 1.0] }
    };
    let xyz_to_cam: Vec<[f32; 3]> = raw
        .develop_params()
        .map(|p| p.color_matrices.first().map(|m| m.matrix[..3].to_vec()).unwrap_or_default())
        .unwrap_or_default();
    let m = camera_to_srgb(&xyz_to_cam);
    let lut = gamma_table();

    // One 2x2 block -> linear camera-white-balanced sRGB (unclamped).
    let block = |sx: usize, sy: usize| -> [f32; 3] {
        let (top, bottom) = (&data[sy * rw + sx..sy * rw + sx + 2], &data[(sy + 1) * rw + sx..(sy + 1) * rw + sx + 2]);
        let mut cam = [0.0f32; 3];
        let mut n = [0.0f32; 3];
        for (i, v) in [top[0], top[1], bottom[0], bottom[1]].into_iter().enumerate() {
            let c = color_at[i / 2][i % 2];
            cam[c] += ((v as f32 - black[i]) * scale[i]).max(0.0);
            n[c] += 1.0;
        }
        for c in 0..3 {
            cam[c] = cam[c] / n[c].max(1.0) * wb[c];
        }
        std::array::from_fn(|c| m[c][0] * cam[0] + m[c][1] * cam[1] + m[c][2] * cam[2])
    };

    // Auto exposure: bring the log-average luminance to a bit above middle grey, but never darken and never brighten by more
    // than 2.5 stops (a night shot should stay dark). Then roll highlights off so the extra gain doesn't clip.
    let gain = {
        let (mut sum, mut cnt) = (0.0f64, 0u32);
        for y in (0..oh).step_by(8) {
            for x in (0..ow).step_by(8) {
                let p = block(x0 + x * 2, y0 + y * 2);
                let lum = (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).max(1e-4);
                sum += (lum as f64).ln();
                cnt += 1;
            }
        }
        let geo = if cnt > 0 { (sum / cnt as f64).exp() as f32 } else { 0.18 };
        (0.22 / geo).clamp(1.0, 6.0)
    };
    let white2 = (4.0f32 * 4.0).max(gain * gain); // extended-Reinhard white point squared: a pixel at 1.0 * gain still lands <= 1
    let tone = |x: f32| -> f32 {
        let x = x.max(0.0) * gain;
        (x * (1.0 + x / white2) / (1.0 + x)).clamp(0.0, 1.0)
    };

    let mut rgba = vec![255u8; ow * oh * 4];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8);
    let rows_per = oh.div_ceil(threads);
    std::thread::scope(|s| {
        for (t, chunk) in rgba.chunks_mut(rows_per * ow * 4).enumerate() {
            let (lut, block, tone) = (&lut[..], &block, &tone);
            s.spawn(move || {
                let first = t * rows_per;
                for (r, line) in chunk.chunks_mut(ow * 4).enumerate() {
                    let sy = y0 + (first + r) * 2;
                    for (x, px) in line.chunks_exact_mut(4).enumerate() {
                        let lin = block(x0 + x * 2, sy);
                        for c in 0..3 {
                            px[c] = lut[(tone(lin[c]) * 4095.0 + 0.5) as usize];
                        }
                    }
                }
            });
        }
    });

    let mut img = Image::new(ow as u32, oh as u32, rgba);
    img.orientation = orientation;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_like_matrix_keeps_white() {
        // A camera whose XYZ -> camera matrix is the sRGB one must map white balanced grey to grey.
        let inv = invert(&SRGB_TO_XYZ).unwrap();
        let m = camera_to_srgb(&inv);
        for v in m.iter().flatten() {
            assert!(v.is_finite());
        }
        let white: Vec<f32> = (0..3).map(|i| m[i].iter().sum()).collect();
        for w in white {
            assert!((w - 1.0).abs() < 1e-3, "row sum {w}");
        }
    }

    #[test]
    fn degenerate_matrices_fall_back_to_identity() {
        let ones = [[1.0; 3]; 3];
        assert_eq!(camera_to_srgb(&ones), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(camera_to_srgb(&[]), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn gamma_is_monotonic_and_spans_the_range() {
        let g = gamma_table();
        assert_eq!((g[0], g[4095]), (0, 255));
        assert!(g.windows(2).all(|w| w[0] <= w[1]));
    }
}
