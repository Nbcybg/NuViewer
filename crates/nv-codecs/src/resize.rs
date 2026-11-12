//! Exact area-average downscaling (box filter with fractional coverage). Used for thumbnails,
//! where it gives clean, alias-free results without needing a heavier resampling library.

use crate::Image;

/// For each destination index, the first source index and the coverage weights (summing to 1).
fn spans(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let ratio = src as f64 / dst as f64;
    (0..dst)
        .map(|i| {
            let a = i as f64 * ratio;
            let b = ((i + 1) as f64 * ratio).min(src as f64);
            let s = a.floor() as usize;
            let e = (b.ceil() as usize).min(src).max(s + 1);
            let ws = (s..e)
                .map(|k| {
                    let lo = (k as f64).max(a);
                    let hi = ((k + 1) as f64).min(b);
                    (((hi - lo) / ratio).max(0.0)) as f32
                })
                .collect();
            (s, ws)
        })
        .collect()
}

/// Below this many source pixels the thread start-up cost outweighs the gain.
const PARALLEL_MIN_PIXELS: usize = 1_500_000;

fn worker_count(pixels: usize) -> usize {
    if pixels < PARALLEL_MIN_PIXELS {
        return 1;
    }
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 6)
}

/// Scale `img` down so its longest edge is at most `max_edge`. Smaller images are returned unchanged.
///
/// One fused pass: for each destination row, the few source rows it covers are reduced horizontally on the
/// fly and accumulated vertically, so no full-size intermediate image is allocated. Large inputs split the
/// destination rows across threads.
pub fn downscale(img: &Image, max_edge: u32) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    if img.width <= max_edge && img.height <= max_edge {
        return Image::new(img.width, img.height, img.rgba.clone());
    }
    let scale = max_edge as f64 / w.max(h) as f64;
    let dw = ((w as f64 * scale).round() as usize).max(1);
    let dh = ((h as f64 * scale).round() as usize).max(1);
    let threads = worker_count(w * h).min(dh);

    let xs = spans(w, dw);
    let ys = spans(h, dh);
    let mut out = vec![0u8; dw * dh * 4];
    let rows_per = dh.div_ceil(threads);
    let background = crate::progress::is_background();
    std::thread::scope(|sc| {
        for (chunk_idx, dst_rows) in out.chunks_mut(rows_per * dw * 4).enumerate() {
            let (xs, ys, src) = (&xs, &ys, &img.rgba);
            sc.spawn(move || {
                crate::progress::set_background(background);
                let mut hrow = vec![0f32; dw * 4]; // one source row, reduced to destination width
                let mut acc = vec![0f32; dw * 4]; // weighted sum of those rows
                let y0 = chunk_idx * rows_per;
                for (r, dst) in dst_rows.chunks_mut(dw * 4).enumerate() {
                    let (sy, wys) = &ys[y0 + r];
                    acc.iter_mut().for_each(|v| *v = 0.0);
                    for (j, &wy) in wys.iter().enumerate() {
                        let y = sy + j;
                        let src_row = &src[y * w * 4..(y + 1) * w * 4];
                        for (x, (sx, wxs)) in xs.iter().enumerate() {
                            let mut a = [0f32; 4];
                            for (k, &wx) in wxs.iter().enumerate() {
                                let p = &src_row[(sx + k) * 4..(sx + k) * 4 + 4];
                                a[0] += wx * p[0] as f32;
                                a[1] += wx * p[1] as f32;
                                a[2] += wx * p[2] as f32;
                                a[3] += wx * p[3] as f32;
                            }
                            hrow[x * 4..x * 4 + 4].copy_from_slice(&a);
                        }
                        for (a, &v) in acc.iter_mut().zip(&hrow) {
                            *a += wy * v;
                        }
                    }
                    for (d, &a) in dst.iter_mut().zip(&acc) {
                        *d = (a + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            });
        }
    });
    let mut result = Image::new(dw as u32, dh as u32, out);
    // Orientation and pending colour conversion describe the picture, not its size: keep them.
    result.orientation = img.orientation;
    result.icc = img.icc.clone();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Image {
        Image::new(w, h, px.iter().copied().cycle().take((w * h * 4) as usize).collect())
    }

    #[test]
    fn large_parallel_path_matches_expectations() {
        // Above the parallel threshold: gradient image, check dimensions and that a solid-ish region stays put.
        let (w, h) = (2000u32, 1000u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 90, 255]);
            }
        }
        let r = downscale(&Image::new(w, h, rgba), 500);
        assert_eq!((r.width, r.height), (500, 250));
        // Top-left pixel ~ (0,0,90); bottom-right ~ (255,255,90); monotonic along the first row.
        let px = |x: usize, y: usize| &r.rgba[(y * 500 + x) * 4..(y * 500 + x) * 4 + 3];
        assert!(px(0, 0)[0] <= 2 && px(0, 0)[1] <= 2 && px(0, 0)[2] == 90);
        assert!(px(499, 249)[0] >= 253 && px(499, 249)[1] >= 253);
        assert!((1..500).all(|x| px(x, 0)[0] >= px(x - 1, 0)[0]));
    }

    #[test]
    fn small_image_untouched() {
        let r = downscale(&solid(10, 5, [1, 2, 3, 255]), 256);
        assert_eq!((r.width, r.height), (10, 5));
    }

    #[test]
    fn keeps_aspect_and_limits_edge() {
        let r = downscale(&solid(4000, 3000, [10, 20, 30, 255]), 256);
        assert_eq!((r.width, r.height), (256, 192));
    }

    #[test]
    fn solid_colour_is_preserved() {
        let r = downscale(&solid(1000, 700, [200, 100, 50, 255]), 100);
        assert!(r.rgba.chunks_exact(4).all(|p| p == [200, 100, 50, 255]));
    }

    #[test]
    fn fine_stripes_average_to_grey() {
        // 1px black/white vertical stripes shrunk 2x must average to ~127.
        let (w, h) = (512u32, 16u32);
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                let v = if x % 2 == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let r = downscale(&Image::new(w, h, rgba), 256);
        assert!(r.rgba.chunks_exact(4).all(|p| (p[0] as i32 - 127).abs() <= 1));
    }
}
