//! Background removal for pictures whose background is plain or nearly plain (a studio backdrop, a product on white, a flat
//! sky): the colours along the picture's border are taken as "background", and whatever of that colour is connected to the
//! border becomes transparent, with a soft edge. This is not a subject detector: a busy background needs a hand-made mask.

use crate::blur;
use crate::raster::Pixels;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The (up to four) main colours along the border.
fn border_colours(p: &Pixels) -> Vec<[f32; 3]> {
    let (w, h) = (p.w as usize, p.h as usize);
    let t = (w.min(h) / 100).max(1);
    let mut clusters: Vec<([f32; 3], u32)> = Vec::new();
    let mut total = 0u32;
    let mut take = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        let c = [p.rgba[i] as f32, p.rgba[i + 1] as f32, p.rgba[i + 2] as f32];
        total += 1;
        if let Some(k) = clusters.iter_mut().find(|k| dist(k.0, c) < 28.0) {
            let n = k.1 as f32;
            for j in 0..3 {
                k.0[j] = (k.0[j] * n + c[j]) / (n + 1.0);
            }
            k.1 += 1;
        } else if clusters.len() < 12 {
            clusters.push((c, 1));
        }
    };
    for y in 0..h {
        for x in 0..w {
            if x < t || y < t || x + t >= w || y + t >= h {
                if (x + y) % 2 == 0 {
                    take(x, y);
                }
            }
        }
    }
    clusters.sort_by_key(|k| std::cmp::Reverse(k.1));
    clusters.into_iter().filter(|k| k.1 as f32 >= total as f32 * 0.04).take(4).map(|k| k.0).collect()
}

/// Make the background transparent. `tolerance` 0..100 is how far from the border colours a pixel may be and still count as
/// background. Returns the share of the picture that was removed.
pub fn remove_background(p: &mut Pixels, tolerance: f32) -> f32 {
    let (w, h) = (p.w as usize, p.h as usize);
    if w < 3 || h < 3 {
        return 0.0;
    }
    let cols = border_colours(p);
    if cols.is_empty() {
        return 0.0;
    }
    let tol = 8.0 + tolerance.clamp(0.0, 100.0) / 100.0 * 70.0;
    let near = |i: usize| {
        let c = [p.rgba[i * 4] as f32, p.rgba[i * 4 + 1] as f32, p.rgba[i * 4 + 2] as f32];
        cols.iter().map(|k| dist(*k, c)).fold(f32::MAX, f32::min) < tol
    };
    // Flood fill from the border through pixels that look like the background.
    let mut bg = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    for x in 0..w {
        for y in [0, h - 1] {
            stack.push(y * w + x);
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            stack.push(y * w + x);
        }
    }
    while let Some(i) = stack.pop() {
        if bg[i] || !near(i) {
            continue;
        }
        bg[i] = true;
        let (x, y) = (i % w, i / w);
        if x > 0 {
            stack.push(i - 1);
        }
        if x + 1 < w {
            stack.push(i + 1);
        }
        if y > 0 {
            stack.push(i - w);
        }
        if y + 1 < h {
            stack.push(i + w);
        }
    }
    let removed = bg.iter().filter(|&&b| b).count();
    if removed == 0 {
        return 0.0;
    }
    // Alpha plane: 0 for background; one pixel of the subject's rim is eaten (the mix of both colours), then feathered.
    let mut plane: Vec<u8> = bg.iter().map(|&b| if b { 0 } else { 255 }).collect();
    let mut eroded = plane.clone();
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            if plane[i] == 255 && (plane[i - 1] == 0 || plane[i + 1] == 0 || plane[i - w] == 0 || plane[i + w] == 0) {
                eroded[i] = 0;
            }
        }
    }
    plane = eroded;
    let soft = blur::gaussian(&plane, w, h, 1, 1, (w.max(h) as f32 / 1500.0).max(0.8));
    let main = cols[0];
    for (i, px) in p.rgba.chunks_exact_mut(4).enumerate() {
        let a = soft[i].min(px[3]);
        if a < 255 && a > 0 {
            // The rim still carries some of the old background: take it out of the colour.
            let k = a as f32 / 255.0;
            for c in 0..3 {
                px[c] = ((px[c] as f32 - main[c] * (1.0 - k)) / k).clamp(0.0, 255.0) as u8;
            }
        }
        px[3] = a;
    }
    removed as f32 / (w * h) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Blue backdrop, a red square subject, and a blue-ish hole inside the subject that is not connected to the border.
    fn studio() -> Pixels {
        let (w, h) = (60u32, 40u32);
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let subject = (15..45).contains(&x) && (8..32).contains(&y);
                let hole = (28..32).contains(&x) && (18..22).contains(&y);
                let noise = ((x * 7 + y * 13) % 5) as u8;
                v.extend_from_slice(&if hole { [30, 60, 200, 255] } else if subject { [200, 30 + noise, 30, 255] } else { [30 + noise, 60, 200, 255] });
            }
        }
        Pixels::new(w, h, v)
    }

    #[test]
    fn the_backdrop_goes_the_subject_and_enclosed_holes_stay() {
        let mut p = studio();
        let share = remove_background(&mut p, 40.0);
        let a = |x: usize, y: usize| p.rgba[(y * 60 + x) * 4 + 3];
        assert_eq!(a(2, 2), 0);
        assert_eq!(a(57, 37), 0);
        assert_eq!(a(20, 12), 255, "subject kept");
        assert_eq!(a(30, 20), 255, "a hole that does not touch the border stays");
        assert!(share > 0.62 && share < 0.74, "about the backdrop's share: {share}");
        // The rim is soft, not a staircase of 0 and 255.
        assert!((0..60).any(|x| { let v = a(x, 20); v > 0 && v < 255 }));
    }

    #[test]
    fn a_picture_without_a_plain_border_loses_little() {
        // Noise all over: nothing is close to a common border colour.
        let mut seed = 99u32;
        let mut p = Pixels::new(50, 50, (0..2500).flat_map(|_| { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); [(seed >> 24) as u8, (seed >> 16) as u8, (seed >> 8) as u8, 255] }).collect());
        let share = remove_background(&mut p, 10.0);
        assert!(share < 0.1, "{share}");
    }
}
