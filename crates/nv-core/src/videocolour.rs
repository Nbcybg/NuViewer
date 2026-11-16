//! Colour of video frames, for snapshots. A video is YUV in some matrix (BT.601 / 709 / 2020), some range (limited / full),
//! with some transfer curve (BT.709 / sRGB / PQ for HDR10 / HLG) and primaries (709 / 601 / 2020 / P3). ffmpeg does the YUV to
//! RGB step (it is told matrix and range); this module does the rest in float: undo the transfer curve, tone-map HDR into the
//! SDR range, move the primaries to sRGB's, and encode sRGB. Frames come as 16-bit RGB (`rgb48le`) so a 10- or 12-bit video
//! loses nothing on the way.
//!
//! "Rec.709-A" is the Apple / QuickTime flavour of Rec.709: the picture was graded for a display with a power law of about
//! 1.96 (QuickTime's `gama` 1.961) instead of the 2.4 of BT.1886; it is offered as a choice of how SDR numbers become light.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Matrix {
    Bt601,
    Bt709,
    Bt2020,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Primaries {
    Bt709,
    /// SMPTE 170M (NTSC SD) - what most BT.601 material uses.
    Smpte170m,
    /// BT.470BG (PAL SD).
    Bt470bg,
    Bt2020,
    P3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transfer {
    /// BT.709 / BT.601 / BT.2020 SDR "camera" curve; shown on a display it is taken as `SdrAssume` says.
    Sdr,
    Srgb,
    /// SMPTE ST 2084, HDR10.
    Pq,
    /// ARIB STD-B67, HLG.
    Hlg,
    Linear,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct VideoColour {
    pub matrix: Matrix,
    pub primaries: Primaries,
    pub transfer: Transfer,
    pub full_range: bool,
    /// Bits per component of the source (8, 10, 12).
    pub bits: u8,
}

impl VideoColour {
    pub fn is_hdr(&self) -> bool {
        matches!(self.transfer, Transfer::Pq | Transfer::Hlg)
    }

    /// From what ffprobe printed (`color_space`, `color_primaries`, `color_transfer`, `color_range`; "unknown" or empty when
    /// the file doesn't say). Untagged video is guessed by size, as players do: HD and up is 709, below that 601 (PAL for 576 lines).
    pub fn from_probe(space: &str, primaries: &str, transfer: &str, range: &str, width: u32, height: u32, bits: u8) -> VideoColour {
        let known = |s: &str| !s.is_empty() && s != "unknown" && s != "unspecified" && s != "reserved";
        let hd = width >= 1280 || height > 576;
        let matrix = match space {
            "bt709" | "bt709a" => Matrix::Bt709,
            "bt2020nc" | "bt2020c" | "bt2020_ncl" | "bt2020_cl" => Matrix::Bt2020,
            "smpte170m" | "bt470bg" | "smpte240m" => Matrix::Bt601,
            _ if known(transfer) && matches!(transfer, "smpte2084" | "arib-std-b67") => Matrix::Bt2020,
            _ => {
                if hd {
                    Matrix::Bt709
                } else {
                    Matrix::Bt601
                }
            }
        };
        let prim = match primaries {
            "bt709" => Primaries::Bt709,
            "smpte170m" | "smpte240m" => Primaries::Smpte170m,
            "bt470bg" => Primaries::Bt470bg,
            "bt2020" => Primaries::Bt2020,
            "smpte432" | "smpte431" => Primaries::P3,
            _ => match matrix {
                Matrix::Bt2020 => Primaries::Bt2020,
                Matrix::Bt709 => Primaries::Bt709,
                Matrix::Bt601 => {
                    if height == 576 {
                        Primaries::Bt470bg
                    } else {
                        Primaries::Smpte170m
                    }
                }
            },
        };
        let tr = match transfer {
            "smpte2084" => Transfer::Pq,
            "arib-std-b67" => Transfer::Hlg,
            "iec61966-2-1" => Transfer::Srgb,
            "linear" => Transfer::Linear,
            _ => Transfer::Sdr,
        };
        VideoColour { matrix, primaries: prim, transfer: tr, full_range: range == "pc" || range == "full" || range == "jpeg", bits: bits.clamp(8, 16) }
    }

    /// The `scale` filter arguments that make ffmpeg do the YUV to RGB step with the right matrix and range.
    pub fn scale_args(&self) -> String {
        let m = match self.matrix {
            Matrix::Bt601 => "bt601",
            Matrix::Bt709 => "bt709",
            Matrix::Bt2020 => "bt2020",
        };
        format!("in_color_matrix={m}:in_range={}:out_range=full", if self.full_range { "full" } else { "limited" })
    }
}

/// How SDR numbers turn into light (and so how they are re-encoded for sRGB).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SdrAssume {
    /// Keep the numbers: what a player shows on an sRGB screen (only matrix, range and primaries are handled).
    AsShown,
    /// BT.1886, gamma 2.4: the standard Rec.709 display.
    Bt1886,
    /// QuickTime Rec.709-A, gamma 1.961.
    Rec709A,
    /// A plain 2.2 display.
    Gamma22,
}

impl SdrAssume {
    pub fn id(self) -> &'static str {
        match self {
            SdrAssume::AsShown => "asis",
            SdrAssume::Bt1886 => "bt1886",
            SdrAssume::Rec709A => "rec709a",
            SdrAssume::Gamma22 => "gamma22",
        }
    }
    pub fn from_id(s: &str) -> Option<SdrAssume> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "asis" | "as_shown" | "keep" => SdrAssume::AsShown,
            "bt1886" | "rec709" | "gamma24" => SdrAssume::Bt1886,
            "rec709a" | "rec709-a" | "quicktime" => SdrAssume::Rec709A,
            "gamma22" | "srgb" => SdrAssume::Gamma22,
            _ => return None,
        })
    }
    fn exponent(self) -> f32 {
        match self {
            SdrAssume::Bt1886 => 2.4,
            SdrAssume::Rec709A => 1.961,
            SdrAssume::Gamma22 => 2.2,
            SdrAssume::AsShown => 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToneMap {
    /// Extended Reinhard on luminance: keeps colours, rolls highlights off smoothly.
    Reinhard,
    /// John Hable's filmic curve: more contrast, darker mid-tones.
    Hable,
}

impl ToneMap {
    pub fn id(self) -> &'static str {
        match self {
            ToneMap::Reinhard => "reinhard",
            ToneMap::Hable => "hable",
        }
    }
    pub fn from_id(s: &str) -> Option<ToneMap> {
        match s.trim().to_ascii_lowercase().as_str() {
            "reinhard" | "bt2390" | "mobius" => Some(ToneMap::Reinhard),
            "hable" | "filmic" => Some(ToneMap::Hable),
            _ => None,
        }
    }
}

/// What the picture should be.
#[derive(Clone, Copy, Debug)]
pub struct Output {
    pub sdr: SdrAssume,
    pub tonemap: ToneMap,
    /// Peak brightness of the HDR master in nits (HDR10 is mostly 1000, sometimes 4000 or 10000).
    pub peak_nits: f32,
    /// HDR video stays HDR: the 16-bit numbers are kept as they are (BT.2020, PQ) and tagged, nothing is tone-mapped.
    pub keep_hdr: bool,
}

impl Default for Output {
    fn default() -> Self {
        Output { sdr: SdrAssume::AsShown, tonemap: ToneMap::Reinhard, peak_nits: 1000.0, keep_hdr: false }
    }
}

/// Diffuse white of an SDR picture in nits (BT.2408): HDR light is scaled so this becomes 1.0.
pub const REFERENCE_WHITE: f32 = 203.0;

pub fn pq_eotf(v: f32) -> f32 {
    // ST 2084; result in nits.
    let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32);
    let p = v.clamp(0.0, 1.0).powf(1.0 / m2);
    10000.0 * ((p - c1).max(0.0) / (c2 - c3 * p)).powf(1.0 / m1)
}

pub fn pq_oetf(nits: f32) -> f32 {
    let (m1, m2, c1, c2, c3) = (0.1593017578125f32, 78.84375f32, 0.8359375f32, 18.8515625f32, 18.6875f32);
    let y = (nits / 10000.0).clamp(0.0, 1.0).powf(m1);
    ((c1 + c2 * y) / (1.0 + c3 * y)).powf(m2)
}

/// HLG inverse OETF: signal 0..1 to scene light 0..1.
pub fn hlg_inverse_oetf(v: f32) -> f32 {
    let (a, b, c) = (0.17883277f32, 0.28466892f32, 0.55991073f32);
    let v = v.clamp(0.0, 1.0);
    if v <= 0.5 {
        v * v / 3.0
    } else {
        (((v - c) / a).exp() + b) / 12.0
    }
}

pub fn srgb_oetf(l: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_eotf(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light of the source primaries to linear BT.709 / sRGB primaries.
fn to_709(p: Primaries) -> [[f32; 3]; 3] {
    match p {
        Primaries::Bt709 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Primaries::Smpte170m => [[0.9395, 0.0502, 0.0103], [0.0178, 0.9658, 0.0164], [-0.0016, -0.0044, 1.0060]],
        Primaries::Bt470bg => [[1.0440, -0.0440, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0118, 0.9882]],
        Primaries::Bt2020 => [[1.6605, -0.5876, -0.0728], [-0.1246, 1.1329, -0.0083], [-0.0182, -0.1006, 1.1187]],
        Primaries::P3 => [[1.2249, -0.2247, 0.0], [-0.0420, 1.0419, 0.0], [-0.0197, -0.0786, 1.0979]],
    }
}

fn hable(x: f32) -> f32 {
    let (a, b, c, d, e, f) = (0.15f32, 0.50f32, 0.10f32, 0.20f32, 0.02f32, 0.30f32);
    ((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f
}

fn tone_map(rgb: [f32; 3], luma: [f32; 3], peak: f32, kind: ToneMap) -> [f32; 3] {
    let y = rgb[0] * luma[0] + rgb[1] * luma[1] + rgb[2] * luma[2];
    if y <= 1e-6 {
        return rgb;
    }
    let mapped = match kind {
        ToneMap::Reinhard => y * (1.0 + y / (peak * peak)) / (1.0 + y),
        ToneMap::Hable => hable(y * 2.0) / hable(peak * 2.0).max(1e-6),
    };
    let k = mapped / y;
    [rgb[0] * k, rgb[1] * k, rgb[2] * k]
}

/// Convert one frame to sRGB. `rgb16` is `rgb48` as u16 values, `w * h * 3` long. `bits` is 8 or 16: the result holds levels
/// 0..=255 or 0..=65535 (a 10- or 12-bit video keeps its fine steps in the 16-bit result). For `keep_hdr` on an HDR source
/// the caller doesn't convert at all (see [`keeps_hdr`]) and writes the input numbers with a PQ tag.
pub fn to_srgb(rgb16: &[u16], src: &VideoColour, out: &Output, bits: u8) -> Vec<u16> {
    let m = to_709(src.primaries);
    let top = if bits >= 16 { 65535.0f32 } else { 255.0 };
    let mut res: Vec<u16> = Vec::with_capacity(rgb16.len());
    let sdr_lin: Vec<f32> = match (src.transfer, out.sdr) {
        (Transfer::Sdr, SdrAssume::AsShown) | (Transfer::Srgb, _) => (0..=65535).map(|i| srgb_eotf(i as f32 / 65535.0)).collect(), // numbers kept = sRGB decode, re-encoded below
        (Transfer::Sdr, a) => (0..=65535).map(|i| (i as f32 / 65535.0).powf(a.exponent())).collect(),
        (Transfer::Linear, _) => (0..=65535).map(|i| i as f32 / 65535.0).collect(),
        _ => Vec::new(),
    };
    let lut: Vec<u16> = if bits >= 16 { Vec::new() } else { (0..=4095).map(|i| (srgb_oetf(i as f32 / 4095.0) * top + 0.5) as u16).collect() };
    let enc = |l: f32| -> u16 {
        if bits >= 16 {
            (srgb_oetf(l) * top + 0.5) as u16
        } else {
            lut[(l.clamp(0.0, 1.0) * 4095.0 + 0.5) as usize]
        }
    };
    let luma2020 = [0.2627f32, 0.6780, 0.0593];
    let peak = (out.peak_nits / REFERENCE_WHITE).max(1.0);
    let identity = !src.is_hdr() && src.transfer == Transfer::Sdr && out.sdr == SdrAssume::AsShown && src.primaries == Primaries::Bt709;
    for px in rgb16.chunks_exact(3) {
        if identity {
            // The common case: the numbers stay exactly as they are.
            res.extend(px.iter().map(|&v| if bits >= 16 { v } else { ((v as u32 * 255 + 32767) / 65535) as u16 }));
            continue;
        }
        let mut lin = match src.transfer {
            Transfer::Pq => [pq_eotf(px[0] as f32 / 65535.0) / REFERENCE_WHITE, pq_eotf(px[1] as f32 / 65535.0) / REFERENCE_WHITE, pq_eotf(px[2] as f32 / 65535.0) / REFERENCE_WHITE],
            Transfer::Hlg => {
                // Scene light, then the system gamma of the 1000-nit reference display (1.2) applied on luminance.
                let e = [hlg_inverse_oetf(px[0] as f32 / 65535.0), hlg_inverse_oetf(px[1] as f32 / 65535.0), hlg_inverse_oetf(px[2] as f32 / 65535.0)];
                let ys = e[0] * luma2020[0] + e[1] * luma2020[1] + e[2] * luma2020[2];
                let g = ys.max(1e-6).powf(0.2) * 1000.0 / REFERENCE_WHITE;
                [e[0] * g, e[1] * g, e[2] * g]
            }
            _ => [sdr_lin[px[0] as usize], sdr_lin[px[1] as usize], sdr_lin[px[2] as usize]],
        };
        if src.is_hdr() {
            lin = tone_map(lin, luma2020, peak, out.tonemap);
        }
        let c = [
            m[0][0] * lin[0] + m[0][1] * lin[1] + m[0][2] * lin[2],
            m[1][0] * lin[0] + m[1][1] * lin[1] + m[1][2] * lin[2],
            m[2][0] * lin[0] + m[2][1] * lin[1] + m[2][2] * lin[2],
        ];
        res.extend([enc(c[0]), enc(c[1]), enc(c[2])]);
    }
    res
}

/// 8-bit sRGB RGB bytes (see [`to_srgb`]).
pub fn to_srgb8(rgb16: &[u16], src: &VideoColour, out: &Output) -> Vec<u8> {
    to_srgb(rgb16, src, out, 8).into_iter().map(|v| v as u8).collect()
}

/// Is `out.keep_hdr` meaningful for this source?
pub fn keeps_hdr(src: &VideoColour, out: &Output) -> bool {
    out.keep_hdr && src.is_hdr()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdr709() -> VideoColour {
        VideoColour { matrix: Matrix::Bt709, primaries: Primaries::Bt709, transfer: Transfer::Sdr, full_range: false, bits: 8 }
    }

    #[test]
    fn pq_round_trips_and_has_the_known_points() {
        for n in [0.0f32, 0.01, 1.0, 100.0, 203.0, 1000.0, 10000.0] {
            let back = pq_eotf(pq_oetf(n));
            assert!((back - n).abs() <= n * 0.002 + 0.01, "{n} -> {back}");
        }
        assert!((pq_oetf(10000.0) - 1.0).abs() < 1e-4);
        assert!((pq_eotf(0.5081) - 100.0).abs() < 2.0, "PQ 0.508 is about 100 nits, got {}", pq_eotf(0.5081));
    }

    #[test]
    fn hlg_reference_points() {
        assert!((hlg_inverse_oetf(0.5) - 1.0 / 12.0).abs() < 1e-4);
        assert!((hlg_inverse_oetf(1.0) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn sdr_709_as_shown_keeps_the_numbers() {
        let rgb: Vec<u16> = vec![0, 0, 0, 65535, 65535, 65535, 32768, 16384, 8192];
        let out = to_srgb8(&rgb, &sdr709(), &Output::default());
        assert_eq!(&out[..6], &[0, 0, 0, 255, 255, 255]);
        assert_eq!(out[6], 128);
        assert_eq!(out[7], 64);
    }

    #[test]
    fn rec709a_is_brighter_in_the_shadows_than_bt1886() {
        let rgb: Vec<u16> = vec![16000, 16000, 16000];
        let a = to_srgb8(&rgb, &sdr709(), &Output { sdr: SdrAssume::Rec709A, ..Output::default() });
        let b = to_srgb8(&rgb, &sdr709(), &Output { sdr: SdrAssume::Bt1886, ..Output::default() });
        assert!(a[0] > b[0], "gamma 1.96 leaves more light than 2.4 ({} vs {})", a[0], b[0]);
    }

    #[test]
    fn hdr10_white_and_black_land_in_sdr_range() {
        let hdr = VideoColour { matrix: Matrix::Bt2020, primaries: Primaries::Bt2020, transfer: Transfer::Pq, full_range: false, bits: 10 };
        // PQ code value of diffuse white (203 nits) is about 0.58; 1000 nits is about 0.75; black is 0.
        let v = |n: f32| (pq_oetf(n) * 65535.0) as u16;
        let rgb = vec![0, 0, 0, v(REFERENCE_WHITE), v(REFERENCE_WHITE), v(REFERENCE_WHITE), v(1000.0), v(1000.0), v(1000.0)];
        let out = to_srgb8(&rgb, &hdr, &Output::default());
        assert_eq!(&out[..3], &[0, 0, 0]);
        assert!(out[3] > 150 && out[3] < 245, "diffuse white {}", out[3]);
        assert!(out[6] >= out[3], "brighter HDR stays brighter");
        // Neutral stays neutral through the primaries change.
        assert!((out[3] as i32 - out[4] as i32).abs() <= 2 && (out[4] as i32 - out[5] as i32).abs() <= 2);
    }

    #[test]
    fn probe_tags_and_untagged_guesses() {
        let hdr = VideoColour::from_probe("bt2020nc", "bt2020", "smpte2084", "tv", 3840, 2160, 10);
        assert!(hdr.is_hdr() && hdr.matrix == Matrix::Bt2020 && !hdr.full_range);
        let sd = VideoColour::from_probe("unknown", "unknown", "unknown", "", 720, 576, 8);
        assert_eq!((sd.matrix, sd.primaries), (Matrix::Bt601, Primaries::Bt470bg));
        let hd = VideoColour::from_probe("", "", "", "pc", 1920, 1080, 8);
        assert_eq!((hd.matrix, hd.full_range), (Matrix::Bt709, true));
        assert!(hd.scale_args().contains("in_color_matrix=bt709"));
    }
}
