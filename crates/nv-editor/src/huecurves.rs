//! Hue curves and the colour warper: two ways to move colours by where they sit on the colour wheel, worked in OKLCH (lightness,
//! chroma, hue), where a hue shift keeps the brightness the eye sees (HSL does not: a blue and a yellow of the same HSL lightness
//! differ a lot).
//!
//!  * Three **curves over the hue circle** (Hue vs Hue, Hue vs Sat, Hue vs Lum): free points (hue in degrees on x, a change from -1 to +1
//!    on y), joined by a smooth curve that wraps around from 360 to 0. At y = +1 a hue moves by 90 degrees, the chroma doubles,
//!    the lightness rises by 0.25.
//!  * The **warper**: 12 spokes (every 30 degrees of hue) by 5 rings (chroma 0, 1/4, 1/2, 3/4, full) of nodes; each node says "the colour
//!    here should go there" as a hue offset in degrees and a chroma offset (in units of the largest chroma), and every colour between
//!    nodes follows by bilinear blending around the wheel.
//!
//! Greys (no chroma) are never touched. Out-of-range results are brought back by lowering the chroma, not by clipping channels, so the
//! hue does not turn.

use std::sync::OnceLock;

pub const SPOKES: usize = 12;
pub const RINGS: usize = 5;
/// Degrees of hue a curve value of 1 means.
pub const HUE_RANGE: f32 = 90.0;
/// Lightness (OKLab L) a curve value of 1 means.
pub const LUM_RANGE: f32 = 0.25;
/// The chroma the outermost warper ring stands for (about the most chroma an sRGB colour has).
pub const CHROMA_MAX: f32 = 0.32;

/// The settings of both tools; everything empty / zero is "no change".
#[derive(Clone, PartialEq, Debug, Default)]
pub struct HueCurves {
    /// Points (hue in degrees 0..360, change -1..1) sorted by hue.
    pub hue: Vec<(f32, f32)>,
    pub sat: Vec<(f32, f32)>,
    pub lum: Vec<(f32, f32)>,
    /// `SPOKES * RINGS` nodes of (hue offset in degrees, chroma offset in units of `CHROMA_MAX`), spoke-major; empty = none.
    pub warp: Vec<[f32; 2]>,
}

impl HueCurves {
    pub fn is_identity(&self) -> bool {
        let flat = |p: &[(f32, f32)]| p.iter().all(|q| q.1 == 0.0);
        flat(&self.hue) && flat(&self.sat) && flat(&self.lum) && self.warp.iter().all(|n| n[0] == 0.0 && n[1] == 0.0)
    }

    pub fn curve(&self, i: usize) -> &Vec<(f32, f32)> {
        match i {
            0 => &self.hue,
            1 => &self.sat,
            _ => &self.lum,
        }
    }

    pub fn curve_mut(&mut self, i: usize) -> &mut Vec<(f32, f32)> {
        match i {
            0 => &mut self.hue,
            1 => &mut self.sat,
            _ => &mut self.lum,
        }
    }

    pub fn node(&self, spoke: usize, ring: usize) -> [f32; 2] {
        self.warp.get(spoke * RINGS + ring).copied().unwrap_or([0.0, 0.0])
    }

    pub fn set_node(&mut self, spoke: usize, ring: usize, dh: f32, ds: f32) {
        if self.warp.len() != SPOKES * RINGS {
            self.warp = vec![[0.0, 0.0]; SPOKES * RINGS];
        }
        self.warp[spoke * RINGS + ring] = [dh, ds];
        if self.warp.iter().all(|n| n[0] == 0.0 && n[1] == 0.0) {
            self.warp.clear();
        }
    }
}

/// Add a point (or move the one within 2 degrees) keeping the list sorted by hue.
pub fn set_point(points: &mut Vec<(f32, f32)>, hue: f32, change: f32) -> usize {
    let hue = hue.rem_euclid(360.0);
    let change = change.clamp(-1.0, 1.0);
    if let Some(i) = points.iter().position(|p| (p.0 - hue).abs() < 2.0) {
        points[i] = (points[i].0, change);
        return i;
    }
    let at = points.iter().position(|p| p.0 > hue).unwrap_or(points.len());
    points.insert(at, (hue, change));
    at
}

fn catmull(p0: f32, p1: f32, p2: f32, p3: f32, u: f32) -> f32 {
    let (u2, u3) = (u * u, u * u * u);
    0.5 * ((2.0 * p1) + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u2 + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u3)
}

/// One value per degree of hue from the points: a Catmull-Rom spline closed around the circle (one point = a constant).
pub fn curve_lut(points: &[(f32, f32)]) -> [f32; 360] {
    let mut lut = [0.0f32; 360];
    let n = points.len();
    if n == 0 {
        return lut;
    }
    if n == 1 {
        lut.fill(points[0].1);
        return lut;
    }
    let mut pts: Vec<(f32, f32)> = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut p: Vec<(f32, f32)> = Vec::with_capacity(n + 4);
    p.push((pts[n - 2].0 - 360.0, pts[n - 2].1));
    p.push((pts[n - 1].0 - 360.0, pts[n - 1].1));
    p.extend(pts.iter().copied());
    p.push((pts[0].0 + 360.0, pts[0].1));
    p.push((pts[1].0 + 360.0, pts[1].1));
    for (h, out) in lut.iter_mut().enumerate() {
        let hf = h as f32;
        let mut i = 1;
        while i < p.len() - 3 && hf >= p[i + 1].0 {
            i += 1;
        }
        let span = (p[i + 1].0 - p[i].0).max(1e-3);
        let u = ((hf - p[i].0) / span).clamp(0.0, 1.0);
        *out = catmull(p[i - 1].1, p[i].1, p[i + 1].1, p[i + 2].1, u).clamp(-1.0, 1.0);
    }
    lut
}

// ---- colour maths -------------------------------------------------------------------------------------------------------------

fn tables() -> &'static (Vec<f32>, Vec<f32>) {
    static T: OnceLock<(Vec<f32>, Vec<f32>)> = OnceLock::new();
    T.get_or_init(|| {
        // sRGB value 0..1 -> linear (1024 steps); linear 0..1 -> sRGB (4096 steps, the curve is steep near 0).
        let dec = (0..=1024).map(|i| {
            let v = i as f32 / 1024.0;
            if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        });
        let enc = (0..=4096).map(|i| {
            let v = i as f32 / 4096.0;
            if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
        });
        (dec.collect(), enc.collect())
    })
}

fn lerp_table(t: &[f32], x: f32) -> f32 {
    let last = (t.len() - 1) as f32;
    let p = x.clamp(0.0, 1.0) * last;
    let i = (p as usize).min(t.len() - 2);
    let f = p - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// Cube roots by table: the cube root of x is read at sqrt(x) (4096 steps), which keeps the table smooth where the root is steep.
fn cbrt_table() -> &'static Vec<f32> {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| (0..=4096).map(|i| { let u = i as f32 / 4096.0; (u * u).cbrt() }).collect())
}

#[inline]
fn cbrt_fast(t: &[f32], x: f32) -> f32 {
    lerp_table(t, x.max(0.0).sqrt())
}

/// Sine and cosine of an angle in degrees, from a table at quarter-degree steps (linear in between: error about 2e-6).
fn sincos_table() -> &'static Vec<(f32, f32)> {
    static T: OnceLock<Vec<(f32, f32)>> = OnceLock::new();
    T.get_or_init(|| (0..=1440).map(|i| { let r = (i as f32 * 0.25).to_radians(); (r.sin(), r.cos()) }).collect())
}

#[inline]
fn sincos_deg(t: &[(f32, f32)], d: f32) -> (f32, f32) {
    let p = d.rem_euclid(360.0) * 4.0;
    let i = (p as usize).min(1439);
    let f = p - i as f32;
    (t[i].0 + (t[i + 1].0 - t[i].0) * f, t[i].1 + (t[i + 1].1 - t[i].1) * f)
}

/// atan2 in degrees (0..360) by a polynomial: error under 0.01 degrees.
#[inline]
fn atan2_deg(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let (mn, mx) = (ax.min(ay), ax.max(ay));
    if mx == 0.0 {
        return 0.0;
    }
    let a = mn / mx;
    let s = a * a;
    let mut r = (((-0.046_496_475 * s + 0.159_314_22) * s - 0.327_622_77) * s * a) + a;
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = std::f32::consts::PI - r;
    }
    if y < 0.0 {
        r = -r;
    }
    r.to_degrees().rem_euclid(360.0)
}

/// `to_lab` with the table cube root (used per pixel by `HueStage`).
#[inline]
fn to_lab_fast(v: [f32; 3], dec: &[f32], cb: &[f32]) -> [f32; 3] {
    let (r, g, b) = (lerp_table(dec, v[0]), lerp_table(dec, v[1]), lerp_table(dec, v[2]));
    let l = cbrt_fast(cb, 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b);
    let m = cbrt_fast(cb, 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b);
    let s = cbrt_fast(cb, 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b);
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn to_lab(v: [f32; 3]) -> [f32; 3] {
    let dec = &tables().0;
    let (r, g, b) = (lerp_table(dec, v[0]), lerp_table(dec, v[1]), lerp_table(dec, v[2]));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn lab_to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    [4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s, -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s, -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s]
}

fn encode(lin: [f32; 3]) -> [f32; 3] {
    let enc = &tables().1;
    [lerp_table(enc, lin[0]), lerp_table(enc, lin[1]), lerp_table(enc, lin[2])]
}

/// OKLCH to sRGB (0..1). A colour outside the sRGB gamut loses chroma until it fits (found by halving: smooth, and the hue stays).
pub fn lch_to_srgb(l: f32, c: f32, h_deg: f32) -> [f32; 3] {
    let hr = h_deg.to_radians();
    let (cs, sn) = (hr.cos(), hr.sin());
    let l = l.clamp(0.0, 1.0);
    let c = c.max(0.0);
    let lin_at = |c: f32| lab_to_linear([l, c * cs, c * sn]);
    let fits = |lin: &[f32; 3]| lin.iter().all(|&x| (-0.0005..=1.0005).contains(&x));
    let lin = lin_at(c);
    if fits(&lin) {
        return encode(lin);
    }
    let (mut lo, mut hi) = (0.0f32, c);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if fits(&lin_at(mid)) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    encode(lin_at(lo))
}

pub fn srgb_to_lch(v: [f32; 3]) -> (f32, f32, f32) {
    let [l, a, b] = to_lab(v);
    (l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0))
}

/// The tools ready to run on pixels.
pub struct HueStage {
    hue: [f32; 360],
    sat: [f32; 360],
    lum: [f32; 360],
    curves_on: bool,
    warp: Option<Box<[[[f32; 2]; RINGS]; SPOKES]>>,
    /// The whole transform sampled on a lattice of `GRID`^3 sRGB colours (what `apply` interpolates): the maths per pixel is
    /// about 100 ns, a trilinear read is about 20.
    lattice: Vec<[f32; 3]>,
    /// One flag per lattice cell: a corner of it was brought back into the gamut, so the transform bends there and the lattice would
    /// blur it: such pixels are worked out in full.
    exact_cell: Vec<bool>,
}

/// Lattice points per axis.
pub const GRID: usize = 41;

impl HueStage {
    /// `None` when there is nothing to do.
    pub fn new(h: &HueCurves) -> Option<HueStage> {
        if h.is_identity() {
            return None;
        }
        let warp = (!h.warp.is_empty()).then(|| {
            let mut m = Box::new([[[0.0f32; 2]; RINGS]; SPOKES]);
            for s in 0..SPOKES {
                for r in 0..RINGS {
                    m[s][r] = h.node(s, r);
                }
            }
            m
        });
        let mut stage = HueStage { hue: curve_lut(&h.hue), sat: curve_lut(&h.sat), lum: curve_lut(&h.lum), curves_on: !(h.hue.is_empty() && h.sat.is_empty() && h.lum.is_empty()), warp, lattice: Vec::new(), exact_cell: Vec::new() };
        (stage.lattice, stage.exact_cell) = stage.build_lattice();
        Some(stage)
    }

    fn at(lut: &[f32; 360], h: f32) -> f32 {
        let i = h.floor() as usize % 360;
        let f = h - h.floor();
        lut[i] + (lut[(i + 1) % 360] - lut[i]) * f
    }

    /// Bilinear warp offsets at (hue degrees, chroma in units of CHROMA_MAX).
    fn warp_at(m: &[[[f32; 2]; RINGS]; SPOKES], h: f32, n: f32) -> [f32; 2] {
        let hf = h.rem_euclid(360.0) / 30.0;
        let i0 = hf.floor() as usize % SPOKES;
        let i1 = (i0 + 1) % SPOKES;
        let tu = hf - hf.floor();
        let rf = (n.clamp(0.0, 0.9999)) * (RINGS - 1) as f32;
        let j0 = rf.floor() as usize;
        let j1 = (j0 + 1).min(RINGS - 1);
        let tv = rf - rf.floor();
        let mut out = [0.0f32; 2];
        for k in 0..2 {
            let a = m[i0][j0][k] * (1.0 - tu) + m[i1][j0][k] * tu;
            let b = m[i0][j1][k] * (1.0 - tu) + m[i1][j1][k] * tu;
            out[k] = a * (1.0 - tv) + b * tv;
        }
        out
    }

    /// One pixel, sRGB 0..1 in and out, worked out in full (the lattice below is made from this). Works with table lookups and a polynomial arctangent (no cbrt / atan2 / sin / cos calls),
    /// about three times quicker than the plain maths and equal to it within a hundredth of a degree of hue.
    pub fn apply_exact(&self, v: [f32; 3]) -> [f32; 3] {
        self.eval(v).0
    }

    /// The exact result and whether the colour had to be brought back into the sRGB gamut on the way.
    fn eval(&self, v: [f32; 3]) -> ([f32; 3], bool) {
        let (dec, enc) = (&tables().0, &tables().1);
        let [l0, a, b] = to_lab_fast(v, dec, cbrt_table());
        let c0 = (a * a + b * b).sqrt();
        if c0 < 1.0e-5 {
            return (v, false); // a grey has no hue to move
        }
        // Colours with hardly any chroma have a hue that means nothing: the tools fade out towards the grey axis, so the result is
        // continuous there (and a lattice can follow it).
        let w = (c0 / 0.03).min(1.0);
        let w = w * w * (3.0 - 2.0 * w);
        let h0 = atan2_deg(b, a);
        let (mut dh, mut c, mut l) = (0.0f32, c0, l0);
        if self.curves_on {
            dh = Self::at(&self.hue, h0) * HUE_RANGE * w;
            c *= 1.0 + Self::at(&self.sat, h0) * w;
            c = c.max(0.0);
            l += Self::at(&self.lum, h0) * LUM_RANGE * w;
        }
        if let Some(m) = &self.warp {
            let [wdh, wds] = Self::warp_at(m, h0 + dh, c / CHROMA_MAX);
            dh += wdh * w;
            c = (c + wds * CHROMA_MAX * w).max(0.0);
        }
        // Turn the colour's direction in the a-b plane by dh instead of going through an angle and back.
        let (sd, cd) = sincos_deg(&sincos_table()[..], dh);
        let (ua, ub) = (a / c0, b / c0);
        let (na, nb) = (ua * cd - ub * sd, ua * sd + ub * cd);
        let l = l.clamp(0.0, 1.0);
        let lin_at = |c: f32| lab_to_linear([l, c * na, c * nb]);
        let fits = |lin: &[f32; 3]| lin.iter().all(|&x| (-0.0005..=1.0005).contains(&x));
        let lin = lin_at(c);
        let (lin, mapped) = if fits(&lin) {
            (lin, false)
        } else {
            let (mut lo, mut hi) = (0.0f32, c);
            for _ in 0..8 {
                let mid = (lo + hi) / 2.0;
                if fits(&lin_at(mid)) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            (lin_at(lo), true)
        };
        ([lerp_table(enc, lin[0]), lerp_table(enc, lin[1]), lerp_table(enc, lin[2])], mapped)
    }

    /// Sample the transform at every lattice colour (red varies fastest), on as many threads as there are slabs of blue; also
    /// which cells touch a colour that had to be gamut-mapped.
    fn build_lattice(&self) -> (Vec<[f32; 3]>, Vec<bool>) {
        let total = GRID * GRID * GRID;
        let mut out = vec![[0.0f32; 3]; total];
        let mut mapped = vec![false; total];
        let n = (GRID - 1) as f32;
        let threads = std::thread::available_parallelism().map(|t| t.get()).unwrap_or(1).min(8);
        let per = (GRID * GRID).div_ceil(threads).max(1) * GRID;
        std::thread::scope(|scope| {
            for (k, (chunk, flags)) in out.chunks_mut(per).zip(mapped.chunks_mut(per)).enumerate() {
                scope.spawn(move || {
                    for (j, (px, fl)) in chunk.iter_mut().zip(flags.iter_mut()).enumerate() {
                        let idx = k * per + j;
                        let (r, g, b) = (idx % GRID, (idx / GRID) % GRID, idx / (GRID * GRID));
                        let (rgb, m) = self.eval([r as f32 / n, g as f32 / n, b as f32 / n]);
                        *px = rgb;
                        *fl = m;
                    }
                });
            }
        });
        let c = GRID - 1;
        let mut cells = vec![false; c * c * c];
        for z in 0..c {
            for y in 0..c {
                for x in 0..c {
                    let at = |dx: usize, dy: usize, dz: usize| mapped[((z + dz) * GRID + y + dy) * GRID + x + dx];
                    cells[(z * c + y) * c + x] = at(0, 0, 0) || at(1, 0, 0) || at(0, 1, 0) || at(1, 1, 0) || at(0, 0, 1) || at(1, 0, 1) || at(0, 1, 1) || at(1, 1, 1);
                }
            }
        }
        (out, cells)
    }

    /// One pixel, sRGB 0..1 in and out: a trilinear read of the lattice.
    #[inline]
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let n = (GRID - 1) as f32;
        let p = [v[0].clamp(0.0, 1.0) * n, v[1].clamp(0.0, 1.0) * n, v[2].clamp(0.0, 1.0) * n];
        let i = [(p[0] as usize).min(GRID - 2), (p[1] as usize).min(GRID - 2), (p[2] as usize).min(GRID - 2)];
        let f = [p[0] - i[0] as f32, p[1] - i[1] as f32, p[2] - i[2] as f32];
        if self.exact_cell[(i[2] * (GRID - 1) + i[1]) * (GRID - 1) + i[0]] {
            return self.eval(v).0;
        }
        let base = (i[2] * GRID + i[1]) * GRID + i[0];
        let l = &self.lattice;
        let (c000, c100, c010, c110) = (l[base], l[base + 1], l[base + GRID], l[base + GRID + 1]);
        let up = base + GRID * GRID;
        let (c001, c101, c011, c111) = (l[up], l[up + 1], l[up + GRID], l[up + GRID + 1]);
        let mut o = [0.0f32; 3];
        for k in 0..3 {
            let x00 = c000[k] + (c100[k] - c000[k]) * f[0];
            let x10 = c010[k] + (c110[k] - c010[k]) * f[0];
            let x01 = c001[k] + (c101[k] - c001[k]) * f[0];
            let x11 = c011[k] + (c111[k] - c011[k]) * f[0];
            let y0 = x00 + (x10 - x00) * f[1];
            let y1 = x01 + (x11 - x01) * f[1];
            o[k] = y0 + (y1 - y0) * f[2];
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_wraps_and_hits_its_points() {
        let lut = curve_lut(&[(0.0, 0.5), (180.0, -0.5)]);
        assert!((lut[0] - 0.5).abs() < 0.02 && (lut[180] + 0.5).abs() < 0.02, "{} {}", lut[0], lut[180]);
        // Between 180 and 360 it climbs back, so 359 is near 0.5 again (the wrap).
        assert!(lut[359] > 0.4);
        assert_eq!(curve_lut(&[(90.0, 0.25)])[10], 0.25);
        assert_eq!(curve_lut(&[])[5], 0.0);
    }

    #[test]
    fn points_stay_sorted_and_merge() {
        let mut p = Vec::new();
        set_point(&mut p, 200.0, 0.3);
        set_point(&mut p, 20.0, -0.2);
        set_point(&mut p, 21.0, 0.9);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], (20.0, 0.9));
        assert_eq!(p[1].0, 200.0);
    }

    #[test]
    fn nothing_set_means_no_stage_and_greys_are_left_alone() {
        assert!(HueStage::new(&HueCurves::default()).is_none());
        let mut h = HueCurves::default();
        h.hue = vec![(0.0, 1.0)];
        let st = HueStage::new(&h).unwrap();
        assert_eq!(st.apply([0.5, 0.5, 0.5]), [0.5, 0.5, 0.5]);
        assert_eq!(st.apply([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_hue_shift_turns_the_colour_and_keeps_its_lightness() {
        let mut h = HueCurves::default();
        h.hue = vec![(0.0, 0.5)]; // every hue moves by +45 degrees
        let st = HueStage::new(&h).unwrap();
        let red = [0.8, 0.1, 0.1];
        let out = st.apply(red);
        let (l0, c0, h0) = srgb_to_lch(red);
        let (l1, c1, h1) = srgb_to_lch(out);
        assert!(((h1 - h0).rem_euclid(360.0) - 45.0).abs() < 4.0, "hue {h0} -> {h1}");
        assert!((l1 - l0).abs() < 0.03, "lightness {l0} -> {l1}");
        assert!(c1 > c0 * 0.5);
    }

    #[test]
    fn saturation_and_lightness_curves() {
        let mut h = HueCurves::default();
        h.sat = vec![(0.0, -1.0)]; // all colours lose their chroma
        let out = HueStage::new(&h).unwrap().apply([0.8, 0.2, 0.2]);
        assert!((out[0] - out[1]).abs() < 0.02 && (out[1] - out[2]).abs() < 0.02, "{out:?}");
        let mut h = HueCurves::default();
        h.lum = vec![(0.0, 1.0)];
        let (l0, _, _) = srgb_to_lch([0.4, 0.3, 0.3]);
        let (l1, _, _) = srgb_to_lch(HueStage::new(&h).unwrap().apply([0.4, 0.3, 0.3]));
        assert!(l1 > l0 + 0.1);
    }

    #[test]
    fn a_warper_node_moves_the_colours_around_it() {
        let mut h = HueCurves::default();
        let red_hue = srgb_to_lch([0.8, 0.1, 0.1]).2;
        let spoke = (red_hue / 30.0).round() as usize % SPOKES;
        for ring in 0..RINGS {
            h.set_node(spoke, ring, 40.0, 0.0);
        }
        let st = HueStage::new(&h).unwrap();
        let out = st.apply([0.8, 0.1, 0.1]);
        let shift = (srgb_to_lch(out).2 - red_hue).rem_euclid(360.0);
        assert!(shift > 15.0 && shift < 45.0, "moved by {shift}");
        // A colour on the far side of the wheel stays put.
        let blue = [0.1, 0.2, 0.8];
        let o = st.apply(blue);
        assert!((o[0] - blue[0]).abs() < 0.03 && (o[2] - blue[2]).abs() < 0.03);
        // Clearing every node makes the tool identity again.
        let mut h2 = h.clone();
        for ring in 0..RINGS {
            h2.set_node(spoke, ring, 0.0, 0.0);
        }
        assert!(h2.is_identity() && h2.warp.is_empty());
    }

    #[test]
    fn the_fast_helpers_agree_with_the_plain_maths() {
        for (y, x) in [(1.0f32, 0.0f32), (0.3, -0.7), (-0.5, -0.5), (-0.2, 0.9), (0.0, -1.0), (0.01, 1.0)] {
            let want = y.atan2(x).to_degrees().rem_euclid(360.0);
            assert!((atan2_deg(y, x) - want).abs() < 0.02 || (atan2_deg(y, x) - want).abs() > 359.98, "{y} {x}: {} vs {want}", atan2_deg(y, x));
        }
        for d in [0.0f32, 12.3, 90.0, 179.9, 270.5, 359.9, -30.0] {
            let (s, c) = sincos_deg(&sincos_table()[..], d);
            assert!((s - d.to_radians().sin()).abs() < 1e-4 && (c - d.to_radians().cos()).abs() < 1e-4);
        }
        for x in [0.0f32, 1e-6, 1e-3, 0.05, 0.4, 1.0] {
            assert!((cbrt_fast(cbrt_table(), x) - x.cbrt()).abs() < 2e-3, "{x}");
        }
        // The pixel path equals the plain conversions: a hue shift of exactly +30 degrees.
        let mut h = HueCurves::default();
        h.hue = vec![(0.0, 1.0 / 3.0)];
        let st = HueStage::new(&h).unwrap();
        for rgb in [[0.8f32, 0.1, 0.1], [0.2, 0.6, 0.3], [0.1, 0.3, 0.9], [0.9, 0.8, 0.2]] {
            let (l0, c0, h0) = srgb_to_lch(rgb);
            let want = lch_to_srgb(l0, c0, h0 + 30.0);
            let got = st.apply(rgb);
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 0.01, "{rgb:?}: {got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn the_lattice_follows_the_exact_maths_closely() {
        let mut h = HueCurves::default();
        h.hue = vec![(30.0, 0.3), (200.0, -0.2)];
        h.sat = vec![(120.0, 0.4)];
        h.lum = vec![(300.0, -0.2)];
        h.set_node(3, 2, 20.0, 0.05);
        let st = HueStage::new(&h).unwrap();
        let (mut worst, mut sum, mut n) = (0.0f32, 0.0f32, 0u32);
        let mut at = [0u32; 3];
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(13) {
                for b in (0..=255).step_by(19) {
                    let v = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
                    let (a, e) = (st.apply(v), st.apply_exact(v));
                    for k in 0..3 {
                        let d = (a[k] - e[k]).abs() * 255.0;
                        if d > worst {
                            worst = d;
                            at = [r, g, b];
                        }
                        sum += d;
                        n += 1;
                    }
                }
            }
        }
        let mean = sum / n as f32;
        assert!(mean < 0.35, "mean error {mean} levels (worst {worst})");
        assert!(worst < 6.0, "worst error {worst} levels at {at:?}: {:?} vs {:?}", st.apply([at[0] as f32 / 255.0, at[1] as f32 / 255.0, at[2] as f32 / 255.0]), st.apply_exact([at[0] as f32 / 255.0, at[1] as f32 / 255.0, at[2] as f32 / 255.0]));
        // Greys stay grey.
        assert_eq!(st.apply([0.5, 0.5, 0.5]), [0.5, 0.5, 0.5]);
    }

    #[test]
    fn out_of_gamut_results_lose_chroma_not_hue() {
        let rgb = lch_to_srgb(0.9, 0.4, 150.0);
        assert!(rgb.iter().all(|&x| (0.0..=1.0).contains(&x)));
        let (_, _, h) = srgb_to_lch(rgb);
        assert!((h - 150.0).abs() < 6.0, "{h}");
    }
}
