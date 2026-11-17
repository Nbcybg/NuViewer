//! Colour tools shared by the viewer, the gallery and the editor: the value of a picked pixel in the formats people paste (hex,
//! rgb(), hsl(), ...), and the dominant colours of a picture (k-means in OKLab, which clusters the way the eye does).
//! Nothing here runs unless the user asks for it (a picked colour, a palette): no cost while looking at pictures.

/// A colour as 8-bit sRGB.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// How a picked colour is written to the clipboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourFormat {
    /// `#RRGGBB`
    Hex,
    /// `rgb(12, 34, 56)`
    Rgb,
    /// `hsl(210, 50%, 40%)`
    Hsl,
    /// `hsv(210, 70%, 56%)`
    Hsv,
    /// `12, 34, 56`
    Triplet,
    /// `oklch(0.52 0.09 250)`
    Oklch,
}

pub const FORMATS: [(ColourFormat, &str, &str); 6] = [
    (ColourFormat::Hex, "hex", "#RRGGBB"),
    (ColourFormat::Rgb, "rgb", "rgb(R, G, B)"),
    (ColourFormat::Hsl, "hsl", "hsl(H, S%, L%)"),
    (ColourFormat::Hsv, "hsv", "hsv(H, S%, V%)"),
    (ColourFormat::Triplet, "triplet", "R, G, B"),
    (ColourFormat::Oklch, "oklch", "oklch(L C H)"),
];

impl ColourFormat {
    pub fn id(self) -> &'static str {
        FORMATS.iter().find(|f| f.0 == self).map(|f| f.1).unwrap_or("hex")
    }
    pub fn label(self) -> &'static str {
        FORMATS.iter().find(|f| f.0 == self).map(|f| f.2).unwrap_or("#RRGGBB")
    }
    pub fn from_id(s: &str) -> Option<ColourFormat> {
        FORMATS.iter().find(|f| f.1.eq_ignore_ascii_case(s.trim())).map(|f| f.0)
    }
}

pub fn rgb_to_hsl(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 } * 60.0;
    (h, s, l)
}

pub fn rgb_to_hsv(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let s = if mx <= 0.0 { 0.0 } else { d / mx };
    let h = if d < 1e-6 { 0.0 } else if mx == r { ((g - b) / d).rem_euclid(6.0) * 60.0 } else if mx == g { ((b - r) / d + 2.0) * 60.0 } else { ((r - g) / d + 4.0) * 60.0 };
    (h, s, mx)
}

fn lin(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn gamma(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0..255) to OKLab (L 0..1, a, b about -0.4..0.4).
pub fn to_oklab(c: Rgb) -> [f32; 3] {
    let (r, g, b) = (lin(c.0), lin(c.1), lin(c.2));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

pub fn from_oklab(lab: [f32; 3]) -> Rgb {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    let q = |v: f32| (gamma(v) * 255.0 + 0.5) as u8;
    Rgb(q(r), q(g), q(b))
}

/// `(lightness 0..1, chroma, hue in degrees)`.
pub fn to_oklch(c: Rgb) -> (f32, f32, f32) {
    let [l, a, b] = to_oklab(c);
    (l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0))
}

pub fn format(c: Rgb, f: ColourFormat) -> String {
    match f {
        ColourFormat::Hex => format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2),
        ColourFormat::Rgb => format!("rgb({}, {}, {})", c.0, c.1, c.2),
        ColourFormat::Triplet => format!("{}, {}, {}", c.0, c.1, c.2),
        ColourFormat::Hsl => {
            let (h, s, l) = rgb_to_hsl(c);
            format!("hsl({:.0}, {:.0}%, {:.0}%)", h, s * 100.0, l * 100.0)
        }
        ColourFormat::Hsv => {
            let (h, s, v) = rgb_to_hsv(c);
            format!("hsv({:.0}, {:.0}%, {:.0}%)", h, s * 100.0, v * 100.0)
        }
        ColourFormat::Oklch => {
            let (l, ch, h) = to_oklch(c);
            format!("oklch({:.2} {:.3} {:.0})", l, ch, h)
        }
    }
}

/// A line with the most useful formats at once, for a hover read-out.
pub fn readout(c: Rgb) -> String {
    let (h, s, l) = rgb_to_hsl(c);
    format!("#{:02X}{:02X}{:02X}   rgb({}, {}, {})   hsl({:.0}, {:.0}%, {:.0}%)", c.0, c.1, c.2, c.0, c.1, c.2, h, s * 100.0, l * 100.0)
}

/// One dominant colour and the share of the picture (0..1) it stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swatch {
    pub rgb: Rgb,
    pub share: f32,
}

/// The `n` dominant colours of a picture, biggest share first: about 20 000 pixels are sampled (an even stride, transparent pixels
/// skipped), seeded by farthest points, refined by eight k-means rounds in OKLab; clusters that land within a hair of each other
/// are merged. Takes a few milliseconds whatever the picture's size.
pub fn palette(rgba: &[u8], w: usize, h: usize, n: usize) -> Vec<Swatch> {
    let n = n.clamp(1, 16);
    let total = w * h;
    if total == 0 || rgba.len() < total * 4 {
        return Vec::new();
    }
    let stride = (total / 20_000).max(1);
    let mut pts: Vec<[f32; 3]> = Vec::with_capacity(total / stride + 1);
    let mut i = 0;
    while i < total {
        let p = &rgba[i * 4..i * 4 + 4];
        if p[3] >= 128 {
            pts.push(to_oklab(Rgb(p[0], p[1], p[2])));
        }
        i += stride;
    }
    if pts.is_empty() {
        return Vec::new();
    }
    let d2 = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // Seeds: the first point, then each time the point farthest from every seed so far.
    let mut cent: Vec<[f32; 3]> = vec![pts[0]];
    let mut nearest: Vec<f32> = pts.iter().map(|p| d2(p, &cent[0])).collect();
    while cent.len() < n.min(pts.len()) {
        let (far, fd) = nearest.iter().enumerate().fold((0usize, -1.0f32), |a, (i, &d)| if d > a.1 { (i, d) } else { a });
        if fd < 1e-6 {
            break;
        }
        cent.push(pts[far]);
        for (k, p) in pts.iter().enumerate() {
            nearest[k] = nearest[k].min(d2(p, &pts[far]));
        }
    }
    let mut counts = vec![0usize; cent.len()];
    for _ in 0..8 {
        let mut sums = vec![[0.0f32; 3]; cent.len()];
        counts.iter_mut().for_each(|c| *c = 0);
        for p in &pts {
            let (mut best, mut bd) = (0usize, f32::MAX);
            for (k, c) in cent.iter().enumerate() {
                let d = d2(p, c);
                if d < bd {
                    bd = d;
                    best = k;
                }
            }
            counts[best] += 1;
            for a in 0..3 {
                sums[best][a] += p[a];
            }
        }
        for k in 0..cent.len() {
            if counts[k] > 0 {
                cent[k] = [sums[k][0] / counts[k] as f32, sums[k][1] / counts[k] as f32, sums[k][2] / counts[k] as f32];
            }
        }
    }
    let all = pts.len() as f32;
    let mut out: Vec<(([f32; 3], usize))> = cent.into_iter().zip(counts).filter(|(_, c)| *c > 0).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    // Merge clusters that are nearly the same colour (a flat background split in two).
    let mut merged: Vec<([f32; 3], usize)> = Vec::new();
    for (c, k) in out {
        if let Some(m) = merged.iter_mut().find(|m| d2(&m.0, &c) < 0.0009) {
            let t = (m.1 + k) as f32;
            for a in 0..3 {
                m.0[a] = (m.0[a] * m.1 as f32 + c[a] * k as f32) / t;
            }
            m.1 += k;
        } else {
            merged.push((c, k));
        }
    }
    merged.sort_by(|a, b| b.1.cmp(&a.1));
    merged.into_iter().map(|(c, k)| Swatch { rgb: from_oklab(c), share: k as f32 / all }).collect()
}

/// A picture read for picking: upright, colour managed pixels and its palette.
pub struct PickData {
    pub id: u64,
    pub img: nv_codecs::Image,
    pub palette: Vec<Swatch>,
}

impl PickData {
    /// Read `path` (first frame) as the picture is shown, and work out its `n` dominant colours. Meant for a worker thread.
    pub fn load(path: &std::path::Path, id: u64, n: usize) -> Option<PickData> {
        let img = nv_codecs::decode_file_first_frame(path).map(nv_codecs::Image::finish_for_cpu).ok()?;
        let palette = palette(&img.rgba, img.width as usize, img.height as usize, n);
        Some(PickData { id, img, palette })
    }

    /// The colour at fractions (u, v) of the picture's width and height (0..1).
    pub fn at(&self, u: f32, v: f32) -> Option<Rgb> {
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) || self.img.width == 0 || self.img.height == 0 {
            return None;
        }
        let (px, py) = (((u * self.img.width as f32) as usize).min(self.img.width as usize - 1), ((v * self.img.height as f32) as usize).min(self.img.height as usize - 1));
        let o = (py * self.img.width as usize + px) * 4;
        self.img.rgba.get(o..o + 3).map(|p| Rgb(p[0], p[1], p[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_of_a_colour() {
        let c = Rgb(255, 128, 0);
        assert_eq!(format(c, ColourFormat::Hex), "#FF8000");
        assert_eq!(format(c, ColourFormat::Rgb), "rgb(255, 128, 0)");
        assert_eq!(format(c, ColourFormat::Triplet), "255, 128, 0");
        assert_eq!(format(c, ColourFormat::Hsl), "hsl(30, 100%, 50%)");
        assert_eq!(format(c, ColourFormat::Hsv), "hsv(30, 100%, 100%)");
        assert!(format(Rgb(255, 255, 255), ColourFormat::Oklch).starts_with("oklch(1.00 0.000"));
        assert_eq!(ColourFormat::from_id("HSL"), Some(ColourFormat::Hsl));
        assert_eq!(ColourFormat::from_id("nope"), None);
        assert!(readout(c).starts_with("#FF8000"));
    }

    #[test]
    fn oklab_round_trips() {
        for c in [Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(12, 200, 90), Rgb(250, 10, 40), Rgb(128, 128, 128)] {
            let back = from_oklab(to_oklab(c));
            assert!((back.0 as i32 - c.0 as i32).abs() <= 1 && (back.1 as i32 - c.1 as i32).abs() <= 1 && (back.2 as i32 - c.2 as i32).abs() <= 1, "{c:?} -> {back:?}");
        }
    }

    #[test]
    fn palette_finds_the_big_colours_in_order() {
        // 60% red, 30% green, 10% blue.
        let (w, h) = (100usize, 100usize);
        let mut px = vec![255u8; w * h * 4];
        for i in 0..w * h {
            let c = if i < 6000 { [220, 20, 20] } else if i < 9000 { [20, 200, 30] } else { [20, 30, 210] };
            px[i * 4..i * 4 + 3].copy_from_slice(&c);
        }
        let p = palette(&px, w, h, 5);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].rgb.0 > 200 && p[0].share > 0.55);
        assert!(p[1].rgb.1 > 180 && (p[1].share - 0.3).abs() < 0.05);
        assert!(p[2].rgb.2 > 190);
        assert!(palette(&[], 0, 0, 4).is_empty());
        // Transparent pixels do not count.
        assert!(palette(&vec![0u8; 400], 10, 10, 3).is_empty());
    }
}
