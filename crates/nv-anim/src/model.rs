//! What a Spine skeleton file holds, independent of the file version (3.8, 4.0, 4.1, 4.2) it came from.

use crate::atlas::Atlas;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inherit {
    Normal,
    OnlyTranslation,
    NoRotationOrReflection,
    NoScale,
    NoScaleOrReflection,
}

impl Inherit {
    /// From the JSON names of 3.8 - 4.2 ("transform" / "inherit").
    pub fn from_name(s: &str) -> Inherit {
        match s {
            "onlyTranslation" => Inherit::OnlyTranslation,
            "noRotationOrReflection" => Inherit::NoRotationOrReflection,
            "noScale" => Inherit::NoScale,
            "noScaleOrReflection" => Inherit::NoScaleOrReflection,
            _ => Inherit::Normal,
        }
    }

    /// From the number in a binary file (the same order in every version).
    pub fn from_index(i: u8) -> Inherit {
        match i {
            1 => Inherit::OnlyTranslation,
            2 => Inherit::NoRotationOrReflection,
            3 => Inherit::NoScale,
            4 => Inherit::NoScaleOrReflection,
            _ => Inherit::Normal,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    Normal,
    Additive,
    Multiply,
    Screen,
}

impl BlendMode {
    pub fn from_name(s: &str) -> BlendMode {
        match s {
            "additive" => BlendMode::Additive,
            "multiply" => BlendMode::Multiply,
            "screen" => BlendMode::Screen,
            _ => BlendMode::Normal,
        }
    }
    pub fn from_index(i: u8) -> BlendMode {
        match i {
            1 => BlendMode::Additive,
            2 => BlendMode::Multiply,
            3 => BlendMode::Screen,
            _ => BlendMode::Normal,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BoneData {
    pub name: String,
    pub parent: Option<usize>,
    pub length: f32,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,
    pub inherit: Inherit,
    pub skin_required: bool,
}

#[derive(Clone, Debug)]
pub struct SlotData {
    pub name: String,
    pub bone: usize,
    pub color: [f32; 4],
    pub dark: Option<[f32; 3]>,
    pub attachment: Option<String>,
    pub blend: BlendMode,
}

/// One bone's pull on a vertex.
#[derive(Clone, Copy, Debug)]
pub struct Influence {
    pub bone: usize,
    pub x: f32,
    pub y: f32,
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub enum Vertices {
    /// x, y pairs in the slot's bone space.
    Plain(Vec<f32>),
    /// Per vertex: the bones that move it.
    Weighted(Vec<Vec<Influence>>),
}

impl Vertices {
    pub fn count(&self) -> usize {
        match self {
            Vertices::Plain(v) => v.len() / 2,
            Vertices::Weighted(w) => w.len(),
        }
    }
    /// How many floats a deform timeline gives for these vertices (x, y per vertex, or per influence).
    pub fn deform_len(&self) -> usize {
        match self {
            Vertices::Plain(v) => v.len(),
            Vertices::Weighted(w) => w.iter().map(|i| i.len() * 2).sum(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Region {
    pub path: String,
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub width: f32,
    pub height: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub path: String,
    pub color: [f32; 4],
    pub width: f32,
    pub height: f32,
    pub uvs: Vec<f32>,
    pub triangles: Vec<u16>,
    pub vertices: Vertices,
    pub hull: usize,
    /// Which deform timelines move this mesh: the skin and attachment they are filed under (a linked mesh with `deform` on uses its parent's).
    pub deform_skin: usize,
    pub deform_name: String,
}

#[derive(Clone, Debug)]
pub struct Clip {
    pub end_slot: usize,
    pub vertices: Vertices,
    pub vertex_count: usize,
}

#[derive(Clone, Debug)]
pub enum Attachment {
    Region(Region),
    Mesh(Mesh),
    Clip(Clip),
    /// bounding box, path, point: nothing to draw.
    Other,
}

#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub name: String,
    /// Per slot index: attachment name -> attachment.
    pub slots: Vec<HashMap<String, Attachment>>,
    /// Bones / constraints that exist only with this skin (4.x "skin required").
    pub bones: Vec<usize>,
    pub ik: Vec<usize>,
    pub transform: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct IkData {
    pub name: String,
    pub order: i32,
    pub bones: Vec<usize>,
    pub target: usize,
    pub mix: f32,
    pub softness: f32,
    pub bend_positive: bool,
    pub compress: bool,
    pub stretch: bool,
    pub uniform: bool,
    pub skin_required: bool,
}

#[derive(Clone, Debug)]
pub struct TransformData {
    pub name: String,
    pub order: i32,
    pub bones: Vec<usize>,
    pub target: usize,
    pub rotation: f32,
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_y: f32,
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub mix_scale_x: f32,
    pub mix_scale_y: f32,
    pub mix_shear_y: f32,
    pub local: bool,
    pub relative: bool,
    pub skin_required: bool,
}

// ---- animation -------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    Stepped,
    /// Control points as fractions of the segment: x of the time, y of the value change (0 = this key's value, 1 = the next one's).
    Bezier([f32; 4]),
    /// 4.x: control points as written, in seconds and in the value's own units (so a curve may overshoot even where two keys are equal).
    BezierAbs([f32; 4]),
}

/// Keys of one timeline: `stride` numbers per key.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub stride: usize,
    pub times: Vec<f32>,
    pub values: Vec<f32>,
    /// `times.len() * stride` entries: the curve leaving each key, per channel.
    pub curves: Vec<Curve>,
}

/// Where in a track a time falls: key index, and how far to the next key (0..1) per channel.
pub struct Cursor {
    pub frame: usize,
    pub next: usize,
    pub last: bool,
}

impl Track {
    pub fn new(stride: usize) -> Track {
        Track { stride, ..Default::default() }
    }

    pub fn push(&mut self, time: f32, values: &[f32], curves: &[Curve]) {
        self.times.push(time);
        for k in 0..self.stride {
            self.values.push(values.get(k).copied().unwrap_or(0.0));
            self.curves.push(curves.get(k).copied().unwrap_or(Curve::Linear));
        }
    }

    pub fn len(&self) -> usize {
        self.times.len()
    }

    pub fn is_empty(&self) -> bool {
        self.times.is_empty()
    }

    pub fn duration(&self) -> f32 {
        self.times.last().copied().unwrap_or(0.0)
    }

    /// The value of every channel at `t`; `None` before the first key. `wrap` turns the difference of two angles into the short way round (3.x rotate).
    pub fn sample(&self, t: f32, wrap: bool) -> Option<Vec<f32>> {
        let n = self.times.len();
        if n == 0 || t < self.times[0] {
            return None;
        }
        // The last key at or before t.
        let i = match self.times.partition_point(|&x| x <= t) {
            0 => return None,
            k => k - 1,
        };
        let s = self.stride;
        let mut out = Vec::with_capacity(s);
        if i + 1 >= n {
            out.extend_from_slice(&self.values[i * s..i * s + s]);
            return Some(out);
        }
        let (t0, t1) = (self.times[i], self.times[i + 1]);
        let frac = if t1 > t0 { ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 1.0 };
        for ch in 0..s {
            let (v0, v1) = (self.values[i * s + ch], self.values[(i + 1) * s + ch]);
            let p = match self.curves[i * s + ch] {
                Curve::Stepped => 0.0,
                Curve::Linear => frac,
                Curve::Bezier(c) => bezier_y(c, frac),
                Curve::BezierAbs(c) => {
                    out.push(bezier_abs(t0, v0, c, t1, v1, t));
                    continue;
                }
            };
            let mut d = v1 - v0;
            if wrap {
                d -= (16384.0 - (16384.5 - d / 360.0) as i32 as f32) * 360.0;
            }
            out.push(v0 + d * p);
        }
        Some(out)
    }
}

/// The value of the cubic (t0, v0) (c0, c1) (c2, c3) (t1, v1) at time `t` (4.x curves: the control points are absolute).
pub fn bezier_abs(t0: f32, v0: f32, c: [f32; 4], t1: f32, v1: f32, t: f32) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
    for _ in 0..24 {
        let w = 1.0 - u;
        let x = w * w * w * t0 + 3.0 * w * w * u * c[0] + 3.0 * w * u * u * c[2] + u * u * u * t1;
        if (x - t).abs() < 1e-6 {
            break;
        }
        if x < t {
            lo = u;
        } else {
            hi = u;
        }
        u = (lo + hi) * 0.5;
    }
    let w = 1.0 - u;
    w * w * w * v0 + 3.0 * w * w * u * c[1] + 3.0 * w * u * u * c[3] + u * u * u * v1
}

/// y of the cubic (0,0) (cx1,cy1) (cx2,cy2) (1,1) at the point where its x is `x`.
pub fn bezier_y(c: [f32; 4], x: f32) -> f32 {
    let [cx1, cy1, cx2, cy2] = c;
    // Solve x(t) = x by bisection (x(t) is monotonic for sane control points; a few steps are plenty for animation).
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut t = x;
    for _ in 0..24 {
        let u = 1.0 - t;
        let xt = 3.0 * u * u * t * cx1 + 3.0 * u * t * t * cx2 + t * t * t;
        if (xt - x).abs() < 1e-5 {
            break;
        }
        if xt < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) * 0.5;
    }
    let u = 1.0 - t;
    3.0 * u * u * t * cy1 + 3.0 * u * t * t * cy2 + t * t * t
}

#[derive(Clone, Debug)]
pub enum BoneTl {
    Rotate(Track),
    /// stride 2
    Translate(Track),
    TranslateX(Track),
    TranslateY(Track),
    /// stride 2; values multiply the setup scale
    Scale(Track),
    ScaleX(Track),
    ScaleY(Track),
    Shear(Track),
    ShearX(Track),
    ShearY(Track),
}

#[derive(Clone, Debug)]
pub enum SlotTl {
    /// time -> attachment name (None = hide)
    Attachment(Vec<(f32, Option<String>)>),
    /// stride 4 (r g b a)
    Color(Track),
    /// stride 3
    Rgb(Track),
    /// stride 1
    Alpha(Track),
    /// stride 7: light rgba + dark rgb
    TwoColor(Track),
    /// stride 6: light rgb + dark rgb
    Rgb2(Track),
}

#[derive(Clone, Debug)]
pub struct DeformTl {
    pub skin: usize,
    pub slot: usize,
    pub attachment: String,
    pub times: Vec<f32>,
    /// One full-length offset list per key (already expanded from the file's offset + partial list).
    pub frames: Vec<Vec<f32>>,
    /// Curve leaving each key (one channel).
    pub curves: Vec<Curve>,
}

#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub bones: Vec<(usize, BoneTl)>,
    pub slots: Vec<(usize, SlotTl)>,
    /// (constraint index into `Rig::ik`, track stride 6: mix, softness, bend (+1 / -1), compress, stretch, unused)
    pub ik: Vec<(usize, Track)>,
    /// (index into `Rig::transform`, stride 6: mixRotate, mixX, mixY, mixScaleX, mixScaleY, mixShearY)
    pub transform: Vec<(usize, Track)>,
    pub deform: Vec<DeformTl>,
    /// (time, slot offsets): the new draw order is the setup order with each listed slot moved by its offset.
    pub draw_order: Vec<(f32, Vec<(usize, i32)>)>,
}

/// A whole skeleton.
#[derive(Clone, Debug)]
pub struct Rig {
    /// "4.1.24"
    pub version: String,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub fps: f32,
    pub bones: Vec<BoneData>,
    pub slots: Vec<SlotData>,
    pub skins: Vec<Skin>,
    pub ik: Vec<IkData>,
    pub transform: Vec<TransformData>,
    pub animations: Vec<Animation>,
    pub atlas: Atlas,
    /// The file is from Spine 3.x: curves are fractions, rotation takes the short way round.
    pub legacy: bool,
    /// What this reader leaves out for this file ("path constraints", "physics"), for the panel.
    pub unsupported: Vec<String>,
}

impl Rig {
    pub fn skin_index(&self, name: &str) -> Option<usize> {
        self.skins.iter().position(|s| s.name == name)
    }
    pub fn animation_index(&self, name: &str) -> Option<usize> {
        self.animations.iter().position(|a| a.name == name)
    }
    pub fn slot_index(&self, name: &str) -> Option<usize> {
        self.slots.iter().position(|s| s.name == name)
    }
    pub fn bone_index(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
    /// The attachment `name` in slot `slot` as `skin` sees it: its own, else the default skin's. Also the skin it was found in.
    pub fn find_attachment(&self, skin: usize, slot: usize, name: &str) -> Option<(&Attachment, usize)> {
        if let Some(a) = self.skins.get(skin).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(name)) {
            return Some((a, skin));
        }
        let default = self.skin_index("default")?;
        self.skins.get(default).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(name)).map(|a| (a, default))
    }
}
