//! Heal, clone, remove, patch and red-eye: the strokes of `Recipe::retouch`, applied to the picture's own pixels (before the
//! turn, flip and crop, so a stroke stays on the same spot when those change).
//!
//! Geometry in a stroke is in 0..1 of the picture; the brush radius is a fraction of the long side. Each stroke reads the picture
//! as it is after the strokes before it, so later strokes can use earlier repairs.
//!
//! * **Clone**: copies the pixels from where `source` points (a fixed offset from the stroke's first point).
//! * **Heal**: the same texture, but its colour and brightness are brought to what is around the spot (the difference of the two
//!   low-pass pictures is added), so the copy melts in.
//! * **Remove**: fills the painted area from its surroundings (push-pull, with the surrounding grain added back).
//! * **Patch**: a lassoed area is replaced by the area `source` points to, healed.
//! * **Red-eye**: takes the red out of the pupil, inside a circle.

use crate::blur;
use crate::raster::Pixels;
use crate::recipe::{Retouch, RetouchKind};

#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Rect {
    fn w(&self) -> usize {
        (self.x1 - self.x0).max(0) as usize
    }
    fn h(&self) -> usize {
        (self.y1 - self.y0).max(0) as usize
    }
}

fn rect_of(points: &[(f32, f32)], pad: f32, w: u32, h: u32) -> Option<Rect> {
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.0, first.1, first.0, first.1);
    for p in points {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    let r = Rect { x0: ((x0 - pad).floor() as i32).max(0), y0: ((y0 - pad).floor() as i32).max(0), x1: ((x1 + pad).ceil() as i32).min(w as i32), y1: ((y1 + pad).ceil() as i32).min(h as i32) };
    (r.x1 > r.x0 && r.y1 > r.y0).then_some(r)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much each pixel of `rect` is covered by a round soft brush dragged along `pts`.
fn brush_coverage(pts: &[(f32, f32)], r: f32, rect: &Rect) -> Vec<f32> {
    let (rw, rh) = (rect.w(), rect.h());
    let mut m = vec![0.0f32; rw * rh];
    let step = (r * 0.25).max(1.0);
    let mut stamp = |cx: f32, cy: f32| {
        let x0 = ((cx - r).floor() as i32).max(rect.x0);
        let x1 = ((cx + r).ceil() as i32).min(rect.x1);
        let y0 = ((cy - r).floor() as i32).max(rect.y0);
        let y1 = ((cy + r).ceil() as i32).min(rect.y1);
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if d >= r {
                    continue;
                }
                let a = 1.0 - smoothstep(0.55 * r, r, d);
                let i = (y - rect.y0) as usize * rw + (x - rect.x0) as usize;
                if a > m[i] {
                    m[i] = a;
                }
            }
        }
    };
    for (i, p) in pts.iter().enumerate() {
        stamp(p.0, p.1);
        if i > 0 {
            let q = pts[i - 1];
            let len = ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
            let n = (len / step).floor() as usize;
            for k in 1..n {
                let t = k as f32 / n as f32;
                stamp(q.0 + (p.0 - q.0) * t, q.1 + (p.1 - q.1) * t);
            }
        }
    }
    m
}

/// A lassoed polygon filled (even-odd), with a soft edge of about `feather` pixels.
fn polygon_coverage(pts: &[(f32, f32)], feather: f32, rect: &Rect) -> Vec<f32> {
    let (rw, rh) = (rect.w(), rect.h());
    let mut plane = vec![0u8; rw * rh];
    for row in 0..rh {
        let y = rect.y0 as f32 + row as f32 + 0.5;
        let mut xs: Vec<f32> = Vec::new();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.1 <= y && b.1 > y) || (b.1 <= y && a.1 > y) {
                xs.push(a.0 + (y - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks_exact(2) {
            let x0 = ((pair[0] - rect.x0 as f32 - 0.5).ceil().max(0.0)) as usize;
            let x1 = ((pair[1] - rect.x0 as f32 - 0.5).floor() + 1.0).clamp(0.0, rw as f32) as usize;
            for x in x0..x1 {
                plane[row * rw + x] = 255;
            }
        }
    }
    let soft = blur::gaussian(&plane, rw, rh, 1, 1, (feather * 0.5).max(0.5));
    soft.iter().map(|&v| v as f32 / 255.0).collect()
}

fn px_f(p: &Pixels, x: i32, y: i32) -> [f32; 3] {
    let (x, y) = (x.clamp(0, p.w as i32 - 1) as usize, y.clamp(0, p.h as i32 - 1) as usize);
    let i = (y * p.w as usize + x) * 4;
    [p.rgba[i] as f32, p.rgba[i + 1] as f32, p.rgba[i + 2] as f32]
}

/// Bilinear sample at the continuous position (x, y), edges repeated.
fn sample(p: &Pixels, x: f32, y: f32) -> [f32; 3] {
    let (fx, fy) = ((x - 0.5).floor(), (y - 0.5).floor());
    let (tx, ty) = (x - 0.5 - fx, y - 0.5 - fy);
    let (ix, iy) = (fx as i32, fy as i32);
    let (a, b, c, d) = (px_f(p, ix, iy), px_f(p, ix + 1, iy), px_f(p, ix, iy + 1), px_f(p, ix + 1, iy + 1));
    let mut o = [0.0; 3];
    for k in 0..3 {
        o[k] = (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty;
    }
    o
}

/// The picture inside `rect` as RGBA bytes.
fn cut(p: &Pixels, rect: &Rect) -> Vec<u8> {
    let (rw, rh) = (rect.w(), rect.h());
    let mut out = Vec::with_capacity(rw * rh * 4);
    for y in 0..rh {
        let s = ((rect.y0 as usize + y) * p.w as usize + rect.x0 as usize) * 4;
        out.extend_from_slice(&p.rgba[s..s + rw * 4]);
    }
    out
}

/// The picture as seen from `offset` away, laid out like `rect` (edges repeated).
fn shifted(p: &Pixels, rect: &Rect, offset: (f32, f32)) -> Vec<u8> {
    let (rw, rh) = (rect.w(), rect.h());
    let mut out = vec![255u8; rw * rh * 4];
    for y in 0..rh {
        for x in 0..rw {
            let v = sample(p, rect.x0 as f32 + x as f32 + 0.5 + offset.0, rect.y0 as f32 + y as f32 + 0.5 + offset.1);
            let i = (y * rw + x) * 4;
            for k in 0..3 {
                out[i + k] = (v[k] + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Put `new` (RGBA, laid out like `rect`) into the picture, weighted by `cover`.
fn blend_in(p: &mut Pixels, rect: &Rect, new: &[u8], cover: &[f32]) {
    let rw = rect.w();
    for y in 0..rect.h() {
        for x in 0..rw {
            let a = cover[y * rw + x];
            if a <= 0.0 {
                continue;
            }
            let d = ((rect.y0 as usize + y) * p.w as usize + rect.x0 as usize + x) * 4;
            let s = (y * rw + x) * 4;
            for k in 0..3 {
                let v = p.rgba[d + k] as f32 + (new[s + k] as f32 - p.rgba[d + k] as f32) * a;
                p.rgba[d + k] = (v + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

fn clone_or_heal(p: &mut Pixels, pts: &[(f32, f32)], s: &Retouch, cover_of: impl Fn(&Rect) -> Vec<f32>, pad: f32, heal: bool, sigma: f32) {
    let Some(src) = s.source else { return };
    let (w, h) = (p.w, p.h);
    let offset = (src.0 * w as f32 - pts[0].0, src.1 * h as f32 - pts[0].1);
    let margin = if heal { (sigma * 3.0).ceil() } else { 0.0 };
    let Some(rect) = rect_of(pts, pad + margin, w, h) else { return };
    let cover = cover_of(&rect);
    let src_px = shifted(p, &rect, offset);
    let new = if heal {
        let (rw, rh) = (rect.w(), rect.h());
        let dst_blur = blur::gaussian(&cut(p, &rect), rw, rh, 4, 3, sigma);
        let src_blur = blur::gaussian(&src_px, rw, rh, 4, 3, sigma);
        let mut out = src_px.clone();
        for i in 0..rw * rh {
            for k in 0..3 {
                let v = src_px[i * 4 + k] as f32 + dst_blur[i * 4 + k] as f32 - src_blur[i * 4 + k] as f32;
                out[i * 4 + k] = (v + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
        out
    } else {
        src_px
    };
    blend_in(p, &rect, &new, &cover);
}

/// Fill the unknown pixels (`known` = 0) from the known ones: a pyramid of weighted averages going down, then a blend of each
/// level with the one below it going up. Smooth, and never invents colours that are not in the picture.
fn push_pull(val: &mut [f32], known: &[f32], w: usize, h: usize) {
    struct Level {
        v: Vec<f32>,
        wt: Vec<f32>,
        w: usize,
        h: usize,
    }
    let mut levels = vec![Level { v: val.to_vec(), wt: known.to_vec(), w, h }];
    while levels.last().map(|l| l.w > 1 || l.h > 1).unwrap_or(false) {
        let l = levels.last().unwrap();
        let (nw, nh) = (l.w.div_ceil(2), l.h.div_ceil(2));
        let (mut v, mut wt) = (vec![0.0f32; nw * nh * 3], vec![0.0f32; nw * nh]);
        for y in 0..nh {
            for x in 0..nw {
                let (mut sw, mut n) = (0.0f32, 0.0f32);
                let mut acc = [0.0f32; 3];
                for dy in 0..2 {
                    for dx in 0..2 {
                        let (sx, sy) = (x * 2 + dx, y * 2 + dy);
                        if sx >= l.w || sy >= l.h {
                            continue;
                        }
                        n += 1.0;
                        let q = l.wt[sy * l.w + sx];
                        sw += q;
                        for k in 0..3 {
                            acc[k] += l.v[(sy * l.w + sx) * 3 + k] * q;
                        }
                    }
                }
                if sw > 0.0 {
                    for k in 0..3 {
                        v[(y * nw + x) * 3 + k] = acc[k] / sw;
                    }
                }
                wt[y * nw + x] = (sw / n).min(1.0);
            }
        }
        levels.push(Level { v, wt, w: nw, h: nh });
        if levels.last().unwrap().wt.iter().all(|&q| q >= 1.0) {
            break;
        }
    }
    for li in (0..levels.len() - 1).rev() {
        let (fine, coarse) = {
            let (a, b) = levels.split_at_mut(li + 1);
            (&mut a[li], &b[0])
        };
        for y in 0..fine.h {
            for x in 0..fine.w {
                let q = fine.wt[y * fine.w + x];
                if q >= 1.0 {
                    continue;
                }
                // Bilinear from the coarse level, at the centre of this fine pixel.
                let (cx, cy) = ((x as f32 + 0.5) / 2.0 - 0.5, (y as f32 + 0.5) / 2.0 - 0.5);
                let (x0, y0) = (cx.floor(), cy.floor());
                let (tx, ty) = (cx - x0, cy - y0);
                let get = |xx: i32, yy: i32, k: usize| coarse.v[(yy.clamp(0, coarse.h as i32 - 1) as usize * coarse.w + xx.clamp(0, coarse.w as i32 - 1) as usize) * 3 + k];
                for k in 0..3 {
                    let up = (get(x0 as i32, y0 as i32, k) * (1.0 - tx) + get(x0 as i32 + 1, y0 as i32, k) * tx) * (1.0 - ty) + (get(x0 as i32, y0 as i32 + 1, k) * (1.0 - tx) + get(x0 as i32 + 1, y0 as i32 + 1, k) * tx) * ty;
                    let i = (y * fine.w + x) * 3 + k;
                    fine.v[i] = fine.v[i] * q + up * (1.0 - q);
                }
                fine.wt[y * fine.w + x] = 1.0;
            }
        }
    }
    val.copy_from_slice(&levels[0].v);
}

fn remove(p: &mut Pixels, pts: &[(f32, f32)], r: f32) {
    let Some(rect) = rect_of(pts, r * 2.0, p.w, p.h) else { return };
    let (rw, rh) = (rect.w(), rect.h());
    // Cover over the whole region (the brush is `r` wide; the margin gives the fill something to learn from).
    let cover = brush_coverage(pts, r, &rect);
    let known: Vec<f32> = cover.iter().map(|&c| if c > 0.45 { 0.0 } else { 1.0 }).collect();
    let mut val = vec![0.0f32; rw * rh * 3];
    for y in 0..rh {
        for x in 0..rw {
            let q = px_f(p, rect.x0 + x as i32, rect.y0 + y as i32);
            val[(y * rw + x) * 3..(y * rw + x) * 3 + 3].copy_from_slice(&q);
        }
    }
    if known.iter().all(|&k| k == 0.0) {
        return;
    }
    // Grain of the surroundings: mean absolute step between neighbouring known pixels.
    let (mut sum, mut n) = (0.0f32, 0u32);
    for y in 0..rh {
        for x in 0..rw.saturating_sub(1) {
            if known[y * rw + x] > 0.0 && known[y * rw + x + 1] > 0.0 {
                sum += (val[(y * rw + x) * 3 + 1] - val[(y * rw + x + 1) * 3 + 1]).abs();
                n += 1;
            }
        }
    }
    let grain = if n > 0 { (sum / n as f32).min(12.0) } else { 0.0 };
    push_pull(&mut val, &known, rw, rh);
    let mut new = cut(p, &rect);
    for y in 0..rh {
        for x in 0..rw {
            let i = y * rw + x;
            if cover[i] <= 0.0 {
                continue;
            }
            let hz = ((rect.x0 as usize + x).wrapping_mul(73856093) ^ (rect.y0 as usize + y).wrapping_mul(19349663)) as u32;
            let noise = ((hz >> 7) % 2001) as f32 / 1000.0 - 1.0; // -1..1
            for k in 0..3 {
                new[i * 4 + k] = (val[i * 3 + k] + noise * grain * 1.3 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
    blend_in(p, &rect, &new, &cover);
}

fn red_eye(p: &mut Pixels, c: (f32, f32), r: f32) {
    let Some(rect) = rect_of(&[c], r, p.w, p.h) else { return };
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let d = ((x as f32 + 0.5 - c.0).powi(2) + (y as f32 + 0.5 - c.1).powi(2)).sqrt();
            if d >= r {
                continue;
            }
            let i = (y as usize * p.w as usize + x as usize) * 4;
            let (rr, gg, bb) = (p.rgba[i] as f32, p.rgba[i + 1] as f32, p.rgba[i + 2] as f32);
            let redness = rr / (gg.max(bb) + 12.0);
            if redness < 1.4 || rr < 60.0 {
                continue;
            }
            let strength = (1.0 - smoothstep(0.6 * r, r, d)) * smoothstep(1.4, 2.2, redness);
            let target = (gg + bb) / 2.0 * 0.85;
            p.rgba[i] = (rr + (target.min(rr) - rr) * strength + 0.5) as u8;
        }
    }
}

fn apply_stroke(p: &mut Pixels, s: &Retouch) {
    let (w, h) = (p.w, p.h);
    let pts: Vec<(f32, f32)> = s.points.iter().map(|q| (q.0 * w as f32, q.1 * h as f32)).collect();
    if pts.is_empty() {
        return;
    }
    let r = (s.radius * w.max(h) as f32).max(1.0);
    match s.kind {
        RetouchKind::Clone => clone_or_heal(p, &pts, s, |rect| brush_coverage(&pts, r, rect), r, false, 0.0),
        RetouchKind::Heal => clone_or_heal(p, &pts, s, |rect| brush_coverage(&pts, r, rect), r, true, (r * 0.5).max(1.5)),
        RetouchKind::Remove => remove(p, &pts, r),
        RetouchKind::Patch => {
            if pts.len() >= 3 {
                clone_or_heal(p, &pts, s, |rect| polygon_coverage(&pts, r, rect), r, true, (r * 0.8).max(2.0));
            }
        }
        RetouchKind::RedEye => red_eye(p, pts[0], r),
    }
}

/// All the strokes, in order, on a copy of `src`.
pub fn apply(src: &Pixels, strokes: &[Retouch]) -> Pixels {
    let mut cur = src.clone();
    for s in strokes {
        apply_stroke(&mut cur, s);
    }
    cur
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth gradient with a dark dot (a blemish) at (40, 30) and a distinctive bright mark at (100, 30).
    fn picture() -> Pixels {
        let (w, h) = (160u32, 80u32);
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let base = 100.0 + x as f32 * 0.4 + y as f32 * 0.2;
                let d = (((x as f32 - 40.0).powi(2) + (y as f32 - 30.0).powi(2)).sqrt() - 4.0).max(0.0);
                let dot = if d == 0.0 { -70.0 } else { 0.0 };
                let m = (((x as f32 - 100.0).powi(2) + (y as f32 - 30.0).powi(2)).sqrt() < 3.0) as i32 as f32 * 90.0;
                let g = (base + dot + m).clamp(0.0, 255.0) as u8;
                v.extend_from_slice(&[g, g, g, 255]);
            }
        }
        Pixels::new(w, h, v)
    }

    fn at(p: &Pixels, x: u32, y: u32) -> u8 {
        p.rgba[((y * p.w + x) * 4) as usize]
    }

    fn stroke(kind: RetouchKind, radius: f32, source: Option<(f32, f32)>, pts: &[(f32, f32)]) -> Retouch {
        Retouch { kind, radius, source, points: pts.iter().map(|&(x, y)| (x / 160.0, y / 80.0)).collect() }
    }

    #[test]
    fn no_strokes_is_a_copy() {
        let p = picture();
        assert_eq!(apply(&p, &[]), p);
    }

    #[test]
    fn clone_copies_what_the_source_points_to() {
        let p = picture();
        // Paint at the dot, taking from the same row 20 px to the right (plain gradient).
        let s = stroke(RetouchKind::Clone, 8.0 / 160.0, Some((60.0 / 160.0, 30.0 / 80.0)), &[(40.0, 30.0)]);
        let out = apply(&p, &[s]);
        assert!(at(&p, 40, 30) < 80);
        assert!((at(&out, 40, 30) as i32 - at(&p, 60, 30) as i32).abs() <= 2, "{} vs {}", at(&out, 40, 30), at(&p, 60, 30));
        assert_eq!(at(&out, 10, 70), at(&p, 10, 70), "far away unchanged");
    }

    #[test]
    fn heal_matches_the_colour_around_the_spot_but_keeps_the_texture_of_the_source() {
        let p = picture();
        // Source is the bright mark: cloned plain it would put a bright dot into the dark spot; healed it keeps the mark's shape
        // but is brought to the level around the destination.
        let src = (100.0 / 160.0, 30.0 / 80.0);
        let clone = apply(&p, &[stroke(RetouchKind::Clone, 6.0 / 160.0, Some(src), &[(40.0, 30.0)])]);
        let heal = apply(&p, &[stroke(RetouchKind::Heal, 6.0 / 160.0, Some(src), &[(40.0, 30.0)])]);
        let around = at(&p, 40, 40) as i32;
        // The ring just inside the brush edge comes out close to its surroundings when healed.
        assert!((at(&heal, 40, 30) as i32 - around).abs() < (at(&clone, 40, 30) as i32 - around).abs(), "heal {} clone {} around {around}", at(&heal, 40, 30), at(&clone, 40, 30));
    }

    #[test]
    fn remove_fills_a_blemish_from_its_surroundings() {
        let p = picture();
        let out = apply(&p, &[stroke(RetouchKind::Remove, 8.0 / 160.0, None, &[(40.0, 30.0)])]);
        let expect = 100.0 + 40.0 * 0.4 + 30.0 * 0.2;
        assert!((at(&out, 40, 30) as f32 - expect).abs() < 12.0, "{} vs {expect}", at(&out, 40, 30));
        assert_eq!(at(&out, 100, 30), at(&p, 100, 30), "other marks stay");
    }

    #[test]
    fn a_patch_replaces_a_lassoed_area_with_the_source_area() {
        let p = picture();
        // Lasso around the bright mark's neighbour, source = a plain place: the area gets the plain gradient (healed).
        let lasso = [(94.0, 24.0), (106.0, 24.0), (106.0, 36.0), (94.0, 36.0)];
        let src_first = (60.0 / 160.0, 50.0 / 80.0); // where the first lasso point maps to
        let out = apply(&p, &[stroke(RetouchKind::Patch, 3.0 / 160.0, Some(src_first), &lasso)]);
        assert!(at(&p, 100, 30) > 150);
        assert!(at(&out, 100, 30) < at(&p, 100, 30) - 30, "the mark is gone: {}", at(&out, 100, 30));
        assert_eq!(at(&out, 20, 70), at(&p, 20, 70));
    }

    #[test]
    fn red_eye_takes_the_red_out_of_a_pupil_only() {
        let (w, h) = (60u32, 40u32);
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let inside = ((x as f32 - 30.0).powi(2) + (y as f32 - 20.0).powi(2)).sqrt() < 5.0;
                v.extend_from_slice(&if inside { [200, 40, 40, 255] } else { [200, 40, 40, 255] });
            }
        }
        // Everything is red; only inside the radius may change.
        let p = Pixels::new(w, h, v);
        let s = Retouch { kind: RetouchKind::RedEye, radius: 8.0 / 60.0, source: None, points: vec![(0.5, 0.5)] };
        let out = apply(&p, &[s]);
        let px = |q: &Pixels, x: u32, y: u32| q.rgba[((y * w + x) * 4) as usize];
        assert!(px(&out, 30, 20) < 100, "red removed in the centre: {}", px(&out, 30, 20));
        assert_eq!(px(&out, 2, 2), 200, "outside the circle untouched");
    }

    #[test]
    fn strokes_are_applied_in_order_and_survive_edges() {
        let p = picture();
        // A stroke that runs off the picture must not panic.
        let s = stroke(RetouchKind::Remove, 10.0 / 160.0, None, &[(0.0, 0.0), (5.0, 5.0), (159.0, 79.0)]);
        let c = stroke(RetouchKind::Clone, 10.0 / 160.0, Some((0.9, 0.9)), &[(155.0, 75.0), (160.0, 80.0)]);
        let out = apply(&p, &[s, c]);
        assert_eq!((out.w, out.h), (160, 80));
    }
}
