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

/// An image sequence: the picture is one of `count` numbered regions (`path` + the number, `digits` wide), starting at `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seq {
    pub count: usize,
    pub start: i32,
    pub digits: usize,
    /// The frame shown in the setup pose.
    pub setup: usize,
}

impl Seq {
    /// The atlas name of frame `index` of an attachment whose picture is `path`.
    pub fn frame_name(&self, path: &str, index: usize) -> String {
        format!("{path}{:0>width$}", self.start + index.min(self.count.saturating_sub(1)) as i32, width = self.digits)
    }
}

#[derive(Clone, Debug)]
pub struct Region {
    pub path: String,
    pub seq: Option<Seq>,
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
    pub seq: Option<Seq>,
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
    /// A path other things follow (only a path constraint uses it).
    Path(PathAtt),
    /// bounding box, point: nothing to draw.
    Other,
}

impl Attachment {
    /// How many numbers a deform key of this attachment has (meshes, clipping polygons and paths can be deformed).
    pub fn deform_len(&self) -> Option<usize> {
        match self {
            Attachment::Mesh(m) => Some(m.vertices.deform_len()),
            Attachment::Clip(c) => Some(c.vertices.deform_len()),
            Attachment::Path(p) => Some(p.vertices.deform_len()),
            _ => None,
        }
    }
}

/// A path: Bezier curves as x, y triples (handle in, point, handle out), in the slot's bone space (or weighted).
#[derive(Clone, Debug)]
pub struct PathAtt {
    pub closed: bool,
    pub constant_speed: bool,
    pub vertices: Vertices,
    /// Length up to the end of each curve (the file's own numbers, used when the speed is not constant).
    pub lengths: Vec<f32>,
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
    /// 4.3: the path / physics / slider constraints (and any other kind) this skin needs.
    pub extra: Vec<CRef>,
}

/// Which list of the rig a constraint is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CKind {
    Ik,
    Transform,
    Path,
    Physics,
    Slider,
}

/// One constraint: its kind and its place in that kind's list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CRef {
    pub kind: CKind,
    pub index: usize,
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
    /// 4.3: the constraint is a list of "this property of the source drives that property of the bones" (then `rotation` ... `shear_y` are the
    /// source offsets, and `mix_*` the starting mixes). `None` for the older, fixed kind.
    pub v43: Option<Transform43>,
}

/// A bone property a 4.3 transform constraint (or slider) reads or writes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prop {
    Rotate,
    X,
    Y,
    ScaleX,
    ScaleY,
    ShearY,
}

impl Prop {
    pub fn from_name(s: &str) -> Option<Prop> {
        Some(match s {
            "rotate" => Prop::Rotate,
            "x" => Prop::X,
            "y" => Prop::Y,
            "scaleX" => Prop::ScaleX,
            "scaleY" => Prop::ScaleY,
            "shearY" => Prop::ShearY,
            _ => return None,
        })
    }
    pub fn from_index(i: u8) -> Option<Prop> {
        Some(match i {
            0 => Prop::Rotate,
            1 => Prop::X,
            2 => Prop::Y,
            3 => Prop::ScaleX,
            4 => Prop::ScaleY,
            5 => Prop::ShearY,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ToProp {
    pub prop: Prop,
    pub offset: f32,
    pub max: f32,
    pub scale: f32,
}

#[derive(Clone, Debug)]
pub struct FromProp {
    pub prop: Prop,
    pub offset: f32,
    pub to: Vec<ToProp>,
}

#[derive(Clone, Debug, Default)]
pub struct Transform43 {
    pub props: Vec<FromProp>,
    pub local_source: bool,
    pub local_target: bool,
    pub additive: bool,
    pub clamp: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScaleYMode {
    None,
    Uniform,
    Volume,
}

impl ScaleYMode {
    pub fn from_name(s: &str) -> ScaleYMode {
        match s.to_ascii_lowercase().as_str() {
            "uniform" => ScaleYMode::Uniform,
            "volume" => ScaleYMode::Volume,
            _ => ScaleYMode::None,
        }
    }
    pub fn from_index(i: u8) -> ScaleYMode {
        match i {
            1 => ScaleYMode::Uniform,
            2 => ScaleYMode::Volume,
            _ => ScaleYMode::None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PositionMode {
    Fixed,
    Percent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpacingMode {
    Length,
    Fixed,
    Percent,
    Proportional,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateMode {
    Tangent,
    Chain,
    ChainScale,
}

/// Makes bones follow a path attachment.
#[derive(Clone, Debug)]
pub struct PathData {
    pub name: String,
    pub order: i32,
    pub bones: Vec<usize>,
    /// The slot whose attachment is the path.
    pub slot: usize,
    pub position_mode: PositionMode,
    pub spacing_mode: SpacingMode,
    pub rotate_mode: RotateMode,
    pub offset_rotation: f32,
    pub position: f32,
    pub spacing: f32,
    pub mix_rotate: f32,
    pub mix_x: f32,
    pub mix_y: f32,
    pub skin_required: bool,
}

/// A bone that swings (spring physics): 4.2 and later.
#[derive(Clone, Debug)]
pub struct PhysicsData {
    pub name: String,
    pub order: i32,
    pub bone: usize,
    pub x: f32,
    pub y: f32,
    pub rotate: f32,
    pub scale_x: f32,
    pub shear_x: f32,
    pub scale_y_mode: ScaleYMode,
    pub limit: f32,
    /// Seconds per simulation step.
    pub step: f32,
    pub inertia: f32,
    pub strength: f32,
    pub damping: f32,
    pub mass_inverse: f32,
    pub wind: f32,
    pub gravity: f32,
    pub mix: f32,
    /// inertia, strength, damping, mass, wind, gravity, mix: follow the skeleton-wide timeline of that name.
    pub global: [bool; 7],
    pub skin_required: bool,
}

/// 4.3: plays an animation, its time taken from a bone property (or set by a timeline).
#[derive(Clone, Debug)]
pub struct SliderData {
    pub name: String,
    pub order: i32,
    pub animation: Option<usize>,
    pub additive: bool,
    pub looped: bool,
    pub bone: Option<usize>,
    pub property: Prop,
    /// The property value that maps to `offset`.
    pub from: f32,
    pub offset: f32,
    pub scale: f32,
    pub local: bool,
    pub time: f32,
    pub mix: f32,
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
    bezier_poly((t0, v0), (c[0], c[1]), (c[2], c[3]), (t1, v1), t)
}

/// y of the cubic (0,0) (cx1,cy1) (cx2,cy2) (1,1) at the point where its x is `x`.
pub fn bezier_y(c: [f32; 4], x: f32) -> f32 {
    bezier_poly((0.0, 0.0), (c[0], c[1]), (c[2], c[3]), (1.0, 1.0), x)
}

/// The value of a cubic at the time `x`, the way the Spine runtimes read it: the curve is tabulated at nine points (t = 0.1 ... 0.9) and
/// the value between two of them is a straight line. That is not the exact curve (an ease-in-out is off by a few percent), and the
/// players (BD2-L2D-Viewer included) show exactly this, so this does too.
fn bezier_poly(p0: (f32, f32), c1: (f32, f32), c2: (f32, f32), p1: (f32, f32), x: f32) -> f32 {
    let (mut px, mut py) = p0;
    for k in 1..=10 {
        let (qx, qy) = if k == 10 {
            p1
        } else {
            let t = k as f32 / 10.0;
            let u = 1.0 - t;
            (u * u * u * p0.0 + 3.0 * u * u * t * c1.0 + 3.0 * u * t * t * c2.0 + t * t * t * p1.0, u * u * u * p0.1 + 3.0 * u * u * t * c1.1 + 3.0 * u * t * t * c2.1 + t * t * t * p1.1)
        };
        if qx >= x {
            return if qx > px { py + (x - px) / (qx - px) * (qy - py) } else { qy };
        }
        px = qx;
        py = qy;
    }
    p1.1
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
    /// (index into `Rig::path`, what changes)
    pub path: Vec<(usize, PathTl)>,
    /// (index into `Rig::physics`, or `None` for every constraint that follows the skeleton-wide value, what changes)
    pub physics: Vec<(Option<usize>, PhysTl)>,
    /// (index into `Rig::slider`, what changes)
    pub slider: Vec<(usize, SliderTl)>,
    /// Which picture of an image sequence a slot shows over time.
    pub sequence: Vec<SeqTl>,
}

/// One key of an image-sequence timeline: from `time` the frames play in `mode` (0 hold, 1 once, 2 loop, 3 pingpong, 4 onceReverse,
/// 5 loopReverse, 6 pingpongReverse), starting at `index`, `delay` seconds apart.
#[derive(Clone, Copy, Debug)]
pub struct SeqKey {
    pub time: f32,
    pub mode: u8,
    pub index: i32,
    pub delay: f32,
}

#[derive(Clone, Debug)]
pub struct SeqTl {
    pub skin: usize,
    pub slot: usize,
    pub attachment: String,
    pub keys: Vec<SeqKey>,
}

impl SeqKey {
    pub fn mode_from_name(s: &str) -> u8 {
        match s {
            "once" => 1,
            "loop" => 2,
            "pingpong" => 3,
            "onceReverse" => 4,
            "loopReverse" => 5,
            "pingpongReverse" => 6,
            _ => 0,
        }
    }
}

/// The frame of a sequence of `count` pictures that `keys` show at `t` (`None` before the first key).
pub fn sequence_frame(keys: &[SeqKey], t: f32, count: usize) -> Option<usize> {
    let i = keys.partition_point(|k| k.time <= t).checked_sub(1)?;
    let k = keys[i];
    let count = count.max(1) as i64;
    let mut index = k.index as i64;
    if k.mode != 0 {
        index += (((t - k.time) / k.delay.max(1e-6)) + 1e-5) as i64;
        match k.mode {
            1 => index = index.min(count - 1),
            2 => index %= count,
            3 => {
                let n = (count << 1) - 2;
                index = if n == 0 { 0 } else { index % n };
                if index >= count {
                    index = n - index;
                }
            }
            4 => index = (count - 1 - index).max(0),
            5 => index = count - 1 - (index % count),
            6 => {
                let n = (count << 1) - 2;
                index = if n == 0 { 0 } else { (index + count - 1) % n };
                if index >= count {
                    index = n - index;
                }
            }
            _ => {}
        }
    }
    Some(index.clamp(0, count - 1) as usize)
}

#[derive(Clone, Debug)]
pub enum PathTl {
    Position(Track),
    Spacing(Track),
    /// stride 3: rotate, x, y
    Mix(Track),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhysProp {
    Inertia,
    Strength,
    Damping,
    Mass,
    Wind,
    Gravity,
    Mix,
}

#[derive(Clone, Debug)]
pub enum PhysTl {
    Prop(PhysProp, Track),
    /// The times at which the simulation starts over.
    Reset(Vec<f32>),
}

#[derive(Clone, Debug)]
pub enum SliderTl {
    Time(Track),
    Mix(Track),
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
    pub path: Vec<PathData>,
    pub physics: Vec<PhysicsData>,
    pub slider: Vec<SliderData>,
    /// 4.3 numbers its constraints in one list (animations and skins refer to it): where each one is kept. Empty before 4.3.
    pub constraints: Vec<CRef>,
    /// The skeleton's size the physics constraints are tuned to (4.2 and later).
    pub reference_scale: f32,
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
