//! What a camera RAW file says about itself, as the starting point of the sliders.
//!
//! A RAW file records how the camera balanced the picture (the colour temperature and tint it chose), its ISO and the exposure
//! compensation that was dialled in. NuEditor's Temperature and Tint sliders are relative (0 = as shot); for a RAW file they are
//! shown in the units the camera used, around those as-shot values, so "5230 K" is what the camera chose and 6500 K is "what it
//! would look like if the light had been 6500 K". The engine keeps working on the relative numbers; this module only converts.

/// The as-shot values of a RAW file (read from the file by `nv_codecs::camera_data`, never estimated).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawBase {
    /// Colour temperature (K) of the as-shot white balance.
    pub kelvin: f32,
    /// Tint of the as-shot white balance (positive = magenta, the sign of the Tint slider).
    pub tint: f32,
    pub iso: Option<u32>,
    /// Exposure compensation (EV) the camera had dialled in.
    pub ev_bias: Option<f32>,
}

/// One Temperature slider unit is this many mired (1e6 / K): the slider scales red and blue by 2^(+-t/200), which is what a
/// change of that many mired does to the white of a blackbody in sRGB (R/B changes by ~1.0 % of its logarithm per mired).
pub const MIRED_PER_UNIT: f32 = 0.665;

/// One Tint slider unit in tint units (Duv x 3000): the slider moves the green gain by 2^(-0.35 t/100), which shifts the white
/// point by 0.0092 Duv per 100 units.
pub const TINT_PER_UNIT: f32 = 0.28;

/// The kelvin range a typed number is taken in.
pub const KELVIN_RANGE: (f32, f32) = (1500.0, 40000.0);

impl RawBase {
    /// The colour temperature the Temperature slider stands for at `slider` (a positive slider is warmer = a higher kelvin
    /// setting, as in every RAW editor).
    pub fn kelvin_at(&self, slider: f32) -> f32 {
        let mired = (1e6 / self.kelvin - slider * MIRED_PER_UNIT).max(25.0);
        (1e6 / mired).clamp(KELVIN_RANGE.0, KELVIN_RANGE.1)
    }

    /// The slider position of a colour temperature (the inverse of `kelvin_at`).
    pub fn slider_for_kelvin(&self, k: f32) -> f32 {
        let k = k.clamp(KELVIN_RANGE.0, KELVIN_RANGE.1);
        (1e6 / self.kelvin - 1e6 / k) / MIRED_PER_UNIT
    }

    pub fn tint_at(&self, slider: f32) -> f32 {
        self.tint + slider * TINT_PER_UNIT
    }

    pub fn slider_for_tint(&self, t: f32) -> f32 {
        (t - self.tint) / TINT_PER_UNIT
    }

    /// "5230 K" for the value column of the Temperature row.
    pub fn kelvin_text(&self, slider: f32) -> String {
        format!("{:.0} K", self.kelvin_at(slider))
    }

    /// "+4" for the value column of the Tint row.
    pub fn tint_text(&self, slider: f32) -> String {
        format!("{:+.0}", self.tint_at(slider))
    }

    /// The label of the Temperature row: what the camera chose.
    pub fn kelvin_label(&self) -> String {
        format!("Temperature (as shot {:.0} K)", self.kelvin)
    }

    pub fn tint_label(&self) -> String {
        format!("Tint (as shot {:+.0})", self.tint)
    }

    /// From what `nv_codecs::camera_data` found: needs the colour temperature and the tint (two colour matrices in the file).
    pub fn from_values(kelvin: Option<f32>, tint: Option<f32>, iso: Option<u32>, ev_bias: Option<f32>) -> Option<RawBase> {
        let kelvin = kelvin.filter(|k| k.is_finite() && (KELVIN_RANGE.0..=KELVIN_RANGE.1).contains(k))?;
        Some(RawBase { kelvin, tint: tint.filter(|t| t.is_finite()).unwrap_or(0.0), iso, ev_bias })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RawBase {
        RawBase { kelvin: 5230.0, tint: 4.0, iso: Some(400), ev_bias: Some(0.3) }
    }

    #[test]
    fn zero_is_what_the_camera_chose() {
        let b = base();
        assert!((b.kelvin_at(0.0) - 5230.0).abs() < 1.0);
        assert_eq!(b.tint_at(0.0), 4.0);
        assert_eq!(b.kelvin_text(0.0), "5230 K");
        assert_eq!(b.tint_text(0.0), "+4");
    }

    #[test]
    fn warmer_slider_means_higher_kelvin_setting() {
        let b = base();
        assert!(b.kelvin_at(50.0) > 5230.0);
        assert!(b.kelvin_at(-50.0) < 5230.0);
        // Magenta direction raises the tint number.
        assert!(b.tint_at(50.0) > 4.0);
    }

    #[test]
    fn typing_a_kelvin_gives_the_slider_that_shows_it() {
        let b = base();
        for k in [2800.0, 4000.0, 5230.0, 6500.0, 9000.0] {
            let s = b.slider_for_kelvin(k);
            assert!((b.kelvin_at(s) - k).abs() < k * 0.002, "{k}: slider {s}");
        }
        assert!(b.slider_for_kelvin(5230.0).abs() < 0.01);
        for t in [-30.0f32, 0.0, 4.0, 25.0] {
            assert!((b.tint_at(b.slider_for_tint(t)) - t).abs() < 1e-3);
        }
    }

    #[test]
    fn the_slider_ends_stay_within_sane_kelvins() {
        let cold = RawBase { kelvin: 2500.0, ..base() };
        assert!(cold.kelvin_at(100.0) <= KELVIN_RANGE.1);
        let warm = RawBase { kelvin: 12000.0, ..base() };
        assert!(warm.kelvin_at(-100.0) >= KELVIN_RANGE.0);
        assert!(cold.kelvin_at(100.0).is_finite());
    }

    #[test]
    fn the_unit_matches_the_engine() {
        // +100 on the slider scales red/blue by 2 (ln 2 = 0.693); a blackbody's ln(R/B) moves ~0.0104 per mired.
        let mired = 100.0 * MIRED_PER_UNIT;
        let ln_rb = mired * 0.0104;
        assert!((ln_rb - 2f32.ln()).abs() < 0.05, "{ln_rb}");
    }

    #[test]
    fn unusable_values_give_no_base() {
        assert_eq!(RawBase::from_values(None, Some(1.0), None, None), None);
        assert_eq!(RawBase::from_values(Some(f32::NAN), None, None, None), None);
        assert_eq!(RawBase::from_values(Some(50.0), None, None, None), None);
        let b = RawBase::from_values(Some(6000.0), None, Some(100), None).unwrap();
        assert_eq!(b.tint, 0.0);
    }
}
