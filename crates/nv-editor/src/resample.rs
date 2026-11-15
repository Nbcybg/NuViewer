//! Resizing with a choice of algorithm. Separable: one pass over the rows, one over the columns, each split over all cores.
//! Shrinking stretches the filter by the scale factor, so it averages instead of skipping (no aliasing).

use crate::raster::Pixels;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algo {
    /// Fastest, blocky when enlarging, keeps hard pixels (pixel art, screenshots).
    Nearest,
    Bilinear,
    /// Keys bicubic (a = -0.75, like most editors' "bicubic").
    Bicubic,
    /// Mitchell-Netravali (B = C = 1/3): soft, no ringing.
    Mitchell,
    /// Catmull-Rom: sharp, a little ringing.
    CatmullRom,
    /// Lanczos, 3 lobes: the sharpest, the best for shrinking photographs.
    Lanczos,
    /// Exact average of the covered pixels: the best for big reductions (enlarging falls back to bilinear).
    Area,
}

impl Algo {
    pub const ALL: [Algo; 7] = [Algo::Lanczos, Algo::Bicubic, Algo::Mitchell, Algo::CatmullRom, Algo::Bilinear, Algo::Area, Algo::Nearest];

    pub fn id(self) -> &'static str {
        match self {
            Algo::Nearest => "nearest",
            Algo::Bilinear => "bilinear",
            Algo::Bicubic => "bicubic",
            Algo::Mitchell => "mitchell",
            Algo::CatmullRom => "catmull-rom",
            Algo::Lanczos => "lanczos",
            Algo::Area => "area",
        }
    }

    pub fn from_id(s: &str) -> Option<Algo> {
        Algo::ALL.into_iter().find(|a| a.id() == s.to_ascii_lowercase())
    }

    pub fn label(self) -> &'static str {
        match self {
            Algo::Nearest => "Nearest (pixels stay hard)",
            Algo::Bilinear => "Bilinear (soft, fast)",
            Algo::Bicubic => "Bicubic",
            Algo::Mitchell => "Mitchell (soft, no ringing)",
            Algo::CatmullRom => "Catmull-Rom (sharp)",
            Algo::Lanczos => "Lanczos (sharpest, best for shrinking)",
            Algo::Area => "Area (exact average, big reductions)",
        }
    }

    pub fn next(self) -> Algo {
        let i = Algo::ALL.iter().position(|a| *a == self).unwrap_or(0);
        Algo::ALL[(i + 1) % Algo::ALL.len()]
    }

    fn support(self) -> f32 {
        match self {
            Algo::Nearest => 0.5,
            Algo::Bilinear | Algo::Area => 1.0,
            Algo::Bicubic | Algo::Mitchell | Algo::CatmullRom => 2.0,
            Algo::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f32) -> f32 {
        let x = x.abs();
        match self {
            Algo::Nearest => (x < 0.5) as i32 as f32,
            Algo::Bilinear | Algo::Area => (1.0 - x).max(0.0),
            Algo::Bicubic => keys(x, -0.75),
            Algo::Mitchell => bc(x, 1.0 / 3.0, 1.0 / 3.0),
            Algo::CatmullRom => bc(x, 0.0, 0.5),
            Algo::Lanczos => {
                if x >= 3.0 {
                    0.0
                } else if x < 1e-6 {
                    1.0
                } else {
                    let px = std::f32::consts::PI * x;
                    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
                }
            }
        }
    }
}

fn keys(x: f32, a: f32) -> f32 {
    if x < 1.0 {
        (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
    } else if x < 2.0 {
        a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
    } else {
        0.0
    }
}

fn bc(x: f32, b: f32, c: f32) -> f32 {
    if x < 1.0 {
        ((12.0 - 9.0 * b - 6.0 * c) * x * x * x + (-18.0 + 12.0 * b + 6.0 * c) * x * x + (6.0 - 2.0 * b)) / 6.0
    } else if x < 2.0 {
        ((-b - 6.0 * c) * x * x * x + (6.0 * b + 30.0 * c) * x * x + (-12.0 * b - 48.0 * c) * x + (8.0 * b + 24.0 * c)) / 6.0
    } else {
        0.0
    }
}

/// For each output index: the first source index and the (normalised) weights.
fn weights(src: usize, dst: usize, algo: Algo) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f32 / dst as f32;
    // Enlarging: the filter keeps its own width. Shrinking: it is stretched to cover `scale` source pixels.
    let (stretch, algo) = if scale > 1.0 {
        (scale, algo)
    } else {
        (1.0, if algo == Algo::Area { Algo::Bilinear } else { algo })
    };
    (0..dst)
        .map(|o| {
            let centre = (o as f32 + 0.5) * scale;
            if algo == Algo::Nearest {
                let i = (centre.floor() as usize).min(src - 1);
                return (i, vec![1.0]);
            }
            let (radius, kernel_algo) = if algo == Algo::Area { (0.5 * scale, Algo::Area) } else { (algo.support() * stretch, algo) };
            let first = ((centre - radius).floor().max(0.0)) as usize;
            let last = ((centre + radius).ceil() as usize).min(src);
            let mut ws: Vec<f32> = (first..last)
                .map(|i| {
                    let d = (i as f32 + 0.5 - centre) / stretch;
                    if algo == Algo::Area {
                        // Box: the covered fraction of this source pixel.
                        let (lo, hi) = ((i as f32).max(centre - radius), ((i + 1) as f32).min(centre + radius));
                        (hi - lo).max(0.0)
                    } else {
                        kernel_algo.weight(d)
                    }
                })
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum.abs() < 1e-6 {
                let i = (centre.floor() as usize).min(src - 1);
                return (i, vec![1.0]);
            }
            ws.iter_mut().for_each(|v| *v /= sum);
            (first, ws)
        })
        .collect()
}

fn threads(units: usize, work: usize) -> usize {
    let by_size = work / (512 * 1024);
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(by_size.max(1)).min(units.max(1))
}

/// Horizontal pass: `rows` rows of `sw` pixels -> `dw` pixels.
fn pass_h(src: &[u8], dst: &mut [u8], sw: usize, dw: usize, w: &[(usize, Vec<f32>)]) {
    for (srow, drow) in src.chunks_exact(sw * 4).zip(dst.chunks_exact_mut(dw * 4)) {
        for (o, (first, ws)) in w.iter().enumerate() {
            let mut acc = [0.0f32; 4];
            for (k, wt) in ws.iter().enumerate() {
                let p = &srow[(first + k) * 4..(first + k) * 4 + 4];
                for c in 0..4 {
                    acc[c] += p[c] as f32 * wt;
                }
            }
            for c in 0..4 {
                drow[o * 4 + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Vertical pass for the output rows `y0..y1`.
fn pass_v(src: &[u8], dst: &mut [u8], width: usize, y0: usize, w: &[(usize, Vec<f32>)]) {
    let mut acc = vec![0.0f32; width * 4];
    for (r, drow) in dst.chunks_exact_mut(width * 4).enumerate() {
        let (first, ws) = &w[y0 + r];
        acc.iter_mut().for_each(|a| *a = 0.0);
        for (k, wt) in ws.iter().enumerate() {
            let srow = &src[(first + k) * width * 4..(first + k + 1) * width * 4];
            for (a, s) in acc.iter_mut().zip(srow) {
                *a += *s as f32 * wt;
            }
        }
        for (d, a) in drow.iter_mut().zip(&acc) {
            *d = (a + 0.5).clamp(0.0, 255.0) as u8;
        }
    }
}

fn has_alpha(p: &Pixels) -> bool {
    p.rgba.chunks_exact(4).any(|px| px[3] != 255)
}

/// `src` at `new_w` x `new_h`. Transparency is handled (colours are weighted by their alpha, so a soft edge does not get a halo).
pub fn resize(src: &Pixels, new_w: u32, new_h: u32, algo: Algo) -> Pixels {
    let (new_w, new_h) = (new_w.max(1), new_h.max(1));
    if (new_w, new_h) == (src.w, src.h) || src.w == 0 || src.h == 0 {
        return src.clone();
    }
    let (sw, sh, dw, dh) = (src.w as usize, src.h as usize, new_w as usize, new_h as usize);
    let alpha = has_alpha(src);
    let mut data = src.rgba.clone();
    if alpha {
        for px in data.chunks_exact_mut(4) {
            let a = px[3] as u32;
            for c in 0..3 {
                px[c] = ((px[c] as u32 * a + 127) / 255) as u8;
            }
        }
    }
    let (wx, wy) = (weights(sw, dw, algo), weights(sh, dh, algo));
    // Do the pass that shrinks the data most first.
    let h_first = dw * sh <= sw * dh;
    let out = if h_first {
        let mut mid = vec![0u8; dw * sh * 4];
        run_h(&data, &mut mid, sw, sh, dw, &wx);
        let mut out = vec![0u8; dw * dh * 4];
        run_v(&mid, &mut out, dw, dh, &wy);
        out
    } else {
        let mut mid = vec![0u8; sw * dh * 4];
        run_v(&data, &mut mid, sw, dh, &wy);
        let mut out = vec![0u8; dw * dh * 4];
        run_h(&mid, &mut out, sw, dh, dw, &wx);
        out
    };
    let mut out = out;
    if alpha {
        for px in out.chunks_exact_mut(4) {
            let a = px[3] as u32;
            if a > 0 && a < 255 {
                for c in 0..3 {
                    px[c] = ((px[c] as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
    }
    Pixels { w: new_w, h: new_h, rgba: out }
}

fn run_h(src: &[u8], dst: &mut [u8], sw: usize, rows: usize, dw: usize, w: &[(usize, Vec<f32>)]) {
    let n = threads(rows, sw * rows * 4);
    let band = rows.div_ceil(n);
    std::thread::scope(|s| {
        for (a, b) in src.chunks(band * sw * 4).zip(dst.chunks_mut(band * dw * 4)) {
            if n == 1 {
                pass_h(a, b, sw, dw, w);
            } else {
                s.spawn(move || pass_h(a, b, sw, dw, w));
            }
        }
    });
}

fn run_v(src: &[u8], dst: &mut [u8], width: usize, out_rows: usize, w: &[(usize, Vec<f32>)]) {
    let n = threads(out_rows, width * out_rows * 4 * 3);
    let band = out_rows.div_ceil(n);
    std::thread::scope(|s| {
        for (i, b) in dst.chunks_mut(band * width * 4).enumerate() {
            if n == 1 {
                pass_v(src, b, width, i * band, w);
            } else {
                s.spawn(move || pass_v(src, b, width, i * band, w));
            }
        }
    });
}

/// Shrink or enlarge so the long side is `long` pixels.
pub fn to_long_side(src: &Pixels, long: u32, algo: Algo) -> Pixels {
    let cur = src.w.max(src.h) as f64;
    let k = long.max(1) as f64 / cur;
    resize(src, ((src.w as f64 * k).round() as u32).max(1), ((src.h as f64 * k).round() as u32).max(1), algo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(w: u32, h: u32, v: u8) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|_| [v, v, v, 255]).collect())
    }

    fn gradient(w: u32, h: u32) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|i| { let x = (i % w) as f32 / (w - 1) as f32; let g = (x * 255.0) as u8; [g, g, g, 255] }).collect())
    }

    #[test]
    fn every_algorithm_keeps_a_flat_colour_and_makes_the_asked_size() {
        for algo in Algo::ALL {
            for (w, h) in [(13, 9), (100, 70), (7, 50)] {
                let out = resize(&grey(40, 30, 123), w, h, algo);
                assert_eq!((out.w, out.h), (w, h), "{algo:?}");
                assert!(out.rgba.chunks_exact(4).all(|p| p[0].abs_diff(123) <= 1 && p[3] == 255), "{algo:?} {w}x{h}");
            }
        }
    }

    #[test]
    fn a_gradient_stays_a_gradient_when_shrunk_and_enlarged() {
        for algo in Algo::ALL {
            let small = resize(&gradient(200, 4), 50, 4, algo);
            let row: Vec<u8> = small.rgba.chunks_exact(4).take(50).map(|p| p[0]).collect();
            assert!(row.windows(2).all(|w| w[1] as i32 + 3 >= w[0] as i32), "{algo:?} not monotone: {row:?}");
            assert!(row[0] <= 10 && row[49] >= 245, "{algo:?} ends: {} {}", row[0], row[49]);
            let big = resize(&gradient(20, 4), 80, 4, algo);
            let row: Vec<u8> = big.rgba.chunks_exact(4).take(80).map(|p| p[0]).collect();
            assert!(row.windows(2).all(|w| w[1] as i32 + 8 >= w[0] as i32), "{algo:?} enlarged not monotone");
        }
    }

    #[test]
    fn nearest_keeps_hard_pixels_and_area_is_the_exact_average() {
        // 2x2 checker enlarged x4 by nearest: no in-between values.
        let p = Pixels::new(2, 2, vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255]);
        let n = resize(&p, 8, 8, Algo::Nearest);
        assert!(n.rgba.chunks_exact(4).all(|p| p[0] == 0 || p[0] == 255));
        // 4x1 -> 2x1 by area: averages of pairs.
        let q = Pixels::new(4, 1, vec![0, 0, 0, 255, 100, 100, 100, 255, 200, 200, 200, 255, 50, 50, 50, 255]);
        let a = resize(&q, 2, 1, Algo::Area);
        assert_eq!((a.rgba[0], a.rgba[4]), (50, 125));
    }

    #[test]
    fn sharp_filters_overshoot_and_soft_ones_do_not() {
        let step = Pixels::new(20, 1, (0..20).flat_map(|i| if i < 10 { [50, 50, 50, 255] } else { [200, 200, 200, 255] }).collect());
        let range = |algo| {
            let o = resize(&step, 40, 1, algo);
            (o.rgba.chunks_exact(4).map(|p| p[0]).min().unwrap(), o.rgba.chunks_exact(4).map(|p| p[0]).max().unwrap())
        };
        let (lo, hi) = range(Algo::Lanczos);
        assert!(lo < 50 && hi > 200, "lanczos rings: {lo} {hi}");
        let (lo, hi) = range(Algo::Bilinear);
        assert_eq!((lo, hi), (50, 200));
        let (lo, hi) = range(Algo::Mitchell);
        assert!(lo >= 40 && hi <= 210, "mitchell stays close: {lo} {hi}");
    }

    #[test]
    fn transparent_pixels_do_not_bleed_their_colour() {
        // Opaque white left half, fully transparent *black* right half: the edge must stay white, not grey.
        let p = Pixels::new(4, 1, vec![255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        let o = resize(&p, 2, 1, Algo::Bilinear);
        assert_eq!(&o.rgba[0..3], &[255, 255, 255], "the colour stays white although the neighbour is transparent black");
        assert!(o.rgba[3] > 150 && o.rgba[7] < 100);
        let mid = resize(&p, 3, 1, Algo::Bilinear);
        let m = &mid.rgba[4..8];
        assert!(m[3] > 0 && m[3] < 255 && m[0] >= 250, "soft edge keeps white colour: {m:?}");
    }

    #[test]
    fn long_side_keeps_the_ratio() {
        let o = to_long_side(&grey(300, 200, 9), 150, Algo::Lanczos);
        assert_eq!((o.w, o.h), (150, 100));
        let o = to_long_side(&grey(100, 300, 9), 600, Algo::Bicubic);
        assert_eq!((o.w, o.h), (200, 600));
    }
}
