//! The *recipe*: everything the user did to a picture, as plain data. Nothing here touches pixels.
//!
//! * Every slider is one row of [`PARAMS`] (group, key, label, range, default). The UI builds its sliders from the table,
//!   "reset group" / "is anything changed" / the XML writer all walk the same table, so a new slider is one line.
//! * What is not a slider (curves, crop, heal strokes, the LUT, flags) is a normal field.
//! * A recipe is small (a few hundred bytes), so undo is just a list of recipes.
//! * [`crate::sidecar`] writes it as the XML pair file; [`crate::engine`] applies it.

use crate::ops::Op;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    WhiteBalance,
    Light,
    Color,
    Detail,
    Composition,
    Heal,
    Film,
}

impl Group {
    pub const ALL: [Group; 7] = [Group::WhiteBalance, Group::Light, Group::Color, Group::Detail, Group::Composition, Group::Heal, Group::Film];

    pub fn key(self) -> &'static str {
        match self {
            Group::WhiteBalance => "wb",
            Group::Light => "light",
            Group::Color => "color",
            Group::Detail => "detail",
            Group::Composition => "composition",
            Group::Heal => "heal",
            Group::Film => "film",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::WhiteBalance => "White balance",
            Group::Light => "Light",
            Group::Color => "Color",
            Group::Detail => "Detail",
            Group::Composition => "Composition",
            Group::Heal => "Heal",
            Group::Film => "Film",
        }
    }
}

/// The largest size of a number typed into a slider.
pub const TYPED_LIMIT: f32 = 1.0e6;

#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    pub group: Group,
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

macro_rules! params {
    ($($id:ident, $group:ident, $key:expr, $label:expr, $min:expr, $max:expr, $def:expr;)*) => {
        /// One slider. `P::Exposure as usize` is its index in [`PARAMS`] and in `Recipe::v`.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(usize)]
        pub enum P { $($id),* }
        pub const PARAMS: &[ParamInfo] = &[$(ParamInfo { group: Group::$group, key: $key, label: $label, min: $min, max: $max, default: $def }),*];
    };
}

params! {
    // White balance: temp / tint are relative (JPEG has no "as shot" kelvin). The four shifts are a split-tone with two axes
    // for the shadows and two for the highlights (green <-> magenta and blue <-> yellow each), for correcting a camera's cast.
    Temp, WhiteBalance, "temp", "Temperature", -100.0, 100.0, 0.0;
    Tint, WhiteBalance, "tint", "Tint", -100.0, 100.0, 0.0;
    ShiftShadowsA, WhiteBalance, "shift_shadows_a", "Shift shadows: green / magenta", -100.0, 100.0, 0.0;
    ShiftShadowsB, WhiteBalance, "shift_shadows_b", "Shift shadows: blue / yellow", -100.0, 100.0, 0.0;
    ShiftHighlightsA, WhiteBalance, "shift_highlights_a", "Shift highlights: green / magenta", -100.0, 100.0, 0.0;
    ShiftHighlightsB, WhiteBalance, "shift_highlights_b", "Shift highlights: blue / yellow", -100.0, 100.0, 0.0;
    // Light.
    Exposure, Light, "exposure", "Exposure (EV)", -5.0, 5.0, 0.0;
    Brightness, Light, "brightness", "Brightness", -100.0, 100.0, 0.0;
    Contrast, Light, "contrast", "Contrast", -100.0, 100.0, 0.0;
    Highlights, Light, "highlights", "Highlights", -100.0, 100.0, 0.0;
    Midtones, Light, "midtones", "Midtones", -100.0, 100.0, 0.0;
    Shadows, Light, "shadows", "Shadows", -100.0, 100.0, 0.0;
    Whites, Light, "whites", "White point", -100.0, 100.0, 0.0;
    Blacks, Light, "blacks", "Black point", -100.0, 100.0, 0.0;
    Gamma, Light, "gamma", "Gamma", 0.2, 5.0, 1.0;
    // Colour.
    Saturation, Color, "saturation", "Saturation", -100.0, 100.0, 0.0;
    Vibrance, Color, "vibrance", "Vibrance", -100.0, 100.0, 0.0;
    Hue, Color, "hue", "Hue", -180.0, 180.0, 0.0;
    // Detail.
    Sharpen, Detail, "sharpen", "Sharpen", 0.0, 100.0, 0.0;
    SharpenRadius, Detail, "sharpen_radius", "Sharpen radius", 0.3, 3.0, 1.0;
    Clarity, Detail, "clarity", "Clarity", -100.0, 100.0, 0.0;
    Vignette, Detail, "vignette", "Vignette", -100.0, 100.0, 0.0;
    VignetteMid, Detail, "vignette_mid", "Vignette midpoint", 0.0, 100.0, 50.0;
    VignetteFeather, Detail, "vignette_feather", "Vignette feather", 0.0, 100.0, 50.0;
    Bloom, Detail, "bloom", "Bloom", 0.0, 100.0, 0.0;
    BloomThreshold, Detail, "bloom_threshold", "Bloom threshold", 0.0, 100.0, 70.0;
    BloomRadius, Detail, "bloom_radius", "Bloom radius", 0.0, 100.0, 30.0;
    // Composition (the free angle is in degrees; quarter turns, flips and the crop are fields of the recipe).
    Angle, Composition, "angle", "Rotate", -180.0, 180.0, 0.0;
    Pitch, Composition, "pitch", "Pitch", -100.0, 100.0, 0.0;
    Yaw, Composition, "yaw", "Yaw", -100.0, 100.0, 0.0;
    Zoom, Composition, "zoom", "Zoom (%)", 100.0, 400.0, 100.0;
    // Heal: the strokes are a list; this is the whole-picture skin smoothing amount.
    SkinSmooth, Heal, "skin_smooth", "Skin smooth", 0.0, 100.0, 0.0;
    // Film.
    LutStrength, Film, "lut_strength", "LUT strength", 0.0, 100.0, 100.0;
    NoiseLuma, Film, "nr_luma", "Noise reduction (luma)", 0.0, 100.0, 0.0;
    NoiseChroma, Film, "nr_chroma", "Noise reduction (colour)", 0.0, 100.0, 0.0;
    Grain, Film, "grain", "Gain / grain", 0.0, 100.0, 0.0;
    GrainSize, Film, "grain_size", "Grain size", 0.0, 100.0, 25.0;
    GrainRough, Film, "grain_rough", "Grain roughness", 0.0, 100.0, 50.0;
    GrainColor, Film, "grain_color", "Grain colour (0 = mono)", 0.0, 100.0, 0.0;
    FilmBlur, Film, "film_blur", "Film blur", 0.0, 100.0, 0.0;
}

pub const PARAM_COUNT: usize = PARAMS.len();

pub fn param_by_key(key: &str) -> Option<usize> {
    PARAMS.iter().position(|p| p.key == key)
}

/// The sliders of one group, as indices into [`PARAMS`].
pub fn params_of(group: Group) -> impl Iterator<Item = usize> {
    (0..PARAM_COUNT).filter(move |&i| PARAMS[i].group == group)
}

/// A tone curve: control points in 0..1, sorted by x. Empty = straight line.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Curve {
    pub points: Vec<(f32, f32)>,
}

impl Curve {
    pub fn is_identity(&self) -> bool {
        self.points.iter().all(|&(x, y)| (x - y).abs() < 1e-6)
    }

    /// Insert or move a point and keep the list sorted; x is kept apart from its neighbours so the curve stays a function.
    pub fn set_point(&mut self, x: f32, y: f32) {
        let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        if let Some(p) = self.points.iter_mut().find(|p| (p.0 - x).abs() < 0.01) {
            p.1 = y;
        } else {
            self.points.push((x, y));
            self.points.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
}

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Curves {
    pub master: Curve,
    pub r: Curve,
    pub g: Curve,
    pub b: Curve,
}

impl Curves {
    pub fn is_identity(&self) -> bool {
        self.master.is_identity() && self.r.is_identity() && self.g.is_identity() && self.b.is_identity()
    }
}

/// Crop rectangle in 0..1 of the (turned) picture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetouchKind {
    Clone,
    Heal,
    Remove,
    Patch,
    RedEye,
}

impl RetouchKind {
    pub fn key(self) -> &'static str {
        match self {
            RetouchKind::Clone => "clone",
            RetouchKind::Heal => "heal",
            RetouchKind::Remove => "remove",
            RetouchKind::Patch => "patch",
            RetouchKind::RedEye => "redeye",
        }
    }

    pub fn from_key(k: &str) -> Option<RetouchKind> {
        [RetouchKind::Clone, RetouchKind::Heal, RetouchKind::Remove, RetouchKind::Patch, RetouchKind::RedEye].into_iter().find(|r| r.key() == k)
    }
}

/// One heal / clone / remove / patch / red-eye action. Geometry is in 0..1 of the *uncropped, unturned* picture, so the
/// stroke stays on the same pixels when the crop or the angle changes.
#[derive(Clone, PartialEq, Debug)]
pub struct Retouch {
    pub kind: RetouchKind,
    /// Brush radius as a fraction of the picture's longer side.
    pub radius: f32,
    /// Clone / heal: where the pixels are taken from (the stroke is the destination).
    pub source: Option<(f32, f32)>,
    pub points: Vec<(f32, f32)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Band {
    pub hue: f32,
    pub sat: f32,
    pub lum: f32,
}

/// The eight colour bands of the hue wheel.
pub const BAND_NAMES: [&str; 8] = ["red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta"];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AutoMode {
    /// The user's own values only (default).
    #[default]
    Off,
    Tone,
    Color,
    Both,
}

impl AutoMode {
    pub fn key(self) -> &'static str {
        match self {
            AutoMode::Off => "off",
            AutoMode::Tone => "tone",
            AutoMode::Color => "color",
            AutoMode::Both => "both",
        }
    }

    pub fn from_key(k: &str) -> AutoMode {
        match k {
            "tone" => AutoMode::Tone,
            "color" => AutoMode::Color,
            "both" => AutoMode::Both,
            _ => AutoMode::Off,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Recipe {
    /// Slider values, indexed by `P`.
    pub v: [f32; PARAM_COUNT],
    pub invert: bool,
    /// Neutral point picked with the colour chart / eyedropper (linear RGB of a grey patch); None = not used.
    pub wb_chart: Option<[f32; 3]>,
    pub curves: Curves,
    pub hue_wheel: [Band; 8],
    pub quarter_turns: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    pub crop: Option<Crop>,
    pub retouch: Vec<Retouch>,
    /// Path of a .cube LUT file (kept as the user gave it), applied at `LutStrength`.
    pub lut: Option<String>,
    /// Reserved: the automatic mode is an option, nothing computes it yet.
    pub auto: AutoMode,
}

impl Default for Recipe {
    fn default() -> Recipe {
        let mut v = [0.0; PARAM_COUNT];
        for (i, p) in PARAMS.iter().enumerate() {
            v[i] = p.default;
        }
        Recipe {
            v,
            invert: false,
            wb_chart: None,
            curves: Curves::default(),
            hue_wheel: [Band { hue: 0.0, sat: 0.0, lum: 0.0 }; 8],
            quarter_turns: 0,
            flip_h: false,
            flip_v: false,
            crop: None,
            retouch: Vec::new(),
            lut: None,
            auto: AutoMode::Off,
        }
    }
}

impl Recipe {
    pub fn get(&self, p: P) -> f32 {
        self.v[p as usize]
    }

    pub fn set(&mut self, p: P, value: f32) {
        self.set_index(p as usize, value);
    }

    /// Set a slider by index. The slider itself moves inside `min..=max`, but a number typed by hand may go beyond it, up to
    /// `TYPED_LIMIT` (each step of the engine clamps what it computes, so any such value is safe).
    pub fn set_index(&mut self, i: usize, value: f32) {
        self.v[i] = if value.is_finite() { value.clamp(-TYPED_LIMIT, TYPED_LIMIT) } else { PARAMS[i].default };
    }

    pub fn is_default(&self, i: usize) -> bool {
        (self.v[i] - PARAMS[i].default).abs() < 1e-6
    }

    /// True when applying the recipe would change nothing (so there is nothing to save, and the engine can be skipped).
    pub fn is_identity(&self) -> bool {
        self.pixel_identity() && self.geometry_identity() && self.retouch.is_empty() && self.lut.is_none() && self.spatial_identity()
    }

    /// Nothing for the per-pixel stage (colour and tone) to do.
    pub fn pixel_identity(&self) -> bool {
        let per_pixel = [Group::WhiteBalance, Group::Light, Group::Color];
        (0..PARAM_COUNT).filter(|&i| per_pixel.contains(&PARAMS[i].group)).all(|i| self.is_default(i))
            && self.vignette_identity()
            && !self.invert
            && self.wb_chart.is_none()
            && self.curves.is_identity()
            && self.hue_wheel.iter().all(|b| b.hue == 0.0 && b.sat == 0.0 && b.lum == 0.0)
    }

    pub fn vignette_identity(&self) -> bool {
        self.get(P::Vignette) == 0.0
    }

    /// Nothing for the neighbourhood filters (sharpen, clarity, bloom, noise, grain, blur, skin) to do.
    pub fn spatial_identity(&self) -> bool {
        [P::Sharpen, P::Clarity, P::Bloom, P::SkinSmooth, P::NoiseLuma, P::NoiseChroma, P::Grain, P::FilmBlur].iter().all(|&p| self.get(p) == 0.0)
    }

    /// No keystone and no zoom.
    pub fn perspective_identity(&self) -> bool {
        self.get(P::Pitch) == 0.0 && self.get(P::Yaw) == 0.0 && self.get(P::Zoom) == 100.0
    }

    pub fn geometry_identity(&self) -> bool {
        self.get(P::Angle) == 0.0
            && self.perspective_identity()
            && self.quarter_turns % 4 == 0
            && !self.flip_h
            && !self.flip_v
            && self.crop.is_none()
    }

    /// Put one slider back to its default.
    pub fn reset_param(&mut self, i: usize) {
        self.v[i] = PARAMS[i].default;
    }

    /// Put a whole group back (sliders and the group's own extras).
    pub fn reset_group(&mut self, g: Group) {
        for i in params_of(g) {
            self.reset_param(i);
        }
        match g {
            Group::WhiteBalance => self.wb_chart = None,
            Group::Light => self.curves = Curves::default(),
            Group::Color => {
                self.invert = false;
                self.hue_wheel = Recipe::default().hue_wheel;
            }
            Group::Composition => {
                self.quarter_turns = 0;
                self.flip_h = false;
                self.flip_v = false;
                self.crop = None;
            }
            Group::Heal => self.retouch.clear(),
            Group::Film => self.lut = None,
            Group::Detail => {}
        }
    }

    /// Does a group differ from the default? (A dot next to the group's name.)
    pub fn group_changed(&self, g: Group) -> bool {
        let mut probe = self.clone();
        probe.reset_group(g);
        probe != *self
    }

    /// The viewer's simple edit list (turns and flips) as a recipe, so a turn made in NuViewer opens in NuEditor as the
    /// same edit. The recipe turns first and flips last; a turn made *after* a flip goes the other way round in that
    /// order, and a vertical flip is a horizontal flip plus a half turn. The angle ends up in -180..180.
    pub fn from_ops(ops: &[Op]) -> Recipe {
        let mut r = Recipe::default();
        let mut angle = 0.0f32;
        let mut flipped = false;
        let mut turn = |d: f32, flipped: bool| angle += if flipped { -d } else { d };
        for op in ops {
            match *op {
                Op::Rotate(d) => turn(d, flipped),
                Op::FlipH => flipped = !flipped,
                Op::FlipV => {
                    flipped = !flipped;
                    turn(180.0, flipped);
                }
            }
        }
        r.flip_h = flipped;
        let mut a = crate::raster::normalize_degrees(angle);
        if a > 180.0 {
            a -= 360.0;
        }
        r.set(P::Angle, a);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_enum_agree() {
        assert_eq!(PARAMS[P::Exposure as usize].key, "exposure");
        assert_eq!(PARAMS[P::FilmBlur as usize].key, "film_blur");
        assert_eq!(PARAMS.len(), P::FilmBlur as usize + 1);
        for (i, a) in PARAMS.iter().enumerate() {
            assert!(PARAMS[i + 1..].iter().all(|b| b.key != a.key), "key {} twice", a.key);
            assert!(a.min <= a.default && a.default <= a.max, "{} default outside its range", a.key);
        }
    }

    #[test]
    fn default_recipe_is_identity_and_each_slider_breaks_it() {
        assert!(Recipe::default().is_identity());
        // Settings of another effect: on their own they change nothing, so they are not "an edit".
        let modifiers = ["sharpen_radius", "vignette_mid", "vignette_feather", "bloom_threshold", "bloom_radius", "lut_strength", "grain_size", "grain_rough", "grain_color"];
        for i in 0..PARAM_COUNT {
            if modifiers.contains(&PARAMS[i].key) {
                continue;
            }
            let mut r = Recipe::default();
            let nudge = if PARAMS[i].default < PARAMS[i].max { PARAMS[i].max } else { PARAMS[i].min };
            r.set_index(i, nudge);
            assert!(!r.is_identity(), "{} did not count as a change", PARAMS[i].key);
            r.reset_param(i);
            assert!(r.is_identity(), "{} did not reset", PARAMS[i].key);
        }
    }

    #[test]
    fn typed_values_may_pass_the_slider_range_up_to_the_limit_and_groups_reset() {
        let mut r = Recipe::default();
        r.set(P::Exposure, 99.0);
        assert_eq!(r.get(P::Exposure), 99.0, "a typed number beyond the slider is kept");
        r.set(P::Exposure, 1.0e9);
        assert_eq!(r.get(P::Exposure), TYPED_LIMIT);
        r.set(P::Contrast, 40.0);
        r.invert = true;
        r.curves.master.set_point(0.5, 0.7);
        assert!(r.group_changed(Group::Light) && r.group_changed(Group::Color) && !r.group_changed(Group::Film));
        r.reset_group(Group::Light);
        assert_eq!(r.get(P::Exposure), 0.0);
        assert!(r.curves.is_identity());
        assert!(r.invert, "colour group untouched");
        r.reset_group(Group::Color);
        assert!(r.is_identity());
    }

    #[test]
    fn viewer_turns_become_an_angle() {
        let r = Recipe::from_ops(&[Op::Rotate(90.0), Op::Rotate(45.0)]);
        assert_eq!(r.get(P::Angle), 135.0);
        let r = Recipe::from_ops(&[Op::Rotate(270.0)]);
        assert_eq!(r.get(P::Angle), -90.0);
        let r = Recipe::from_ops(&[Op::FlipH, Op::Rotate(45.0)]);
        assert!(r.flip_h);
        assert_eq!(r.get(P::Angle), -45.0);
        assert!(Recipe::from_ops(&[]).is_identity());
    }
}
