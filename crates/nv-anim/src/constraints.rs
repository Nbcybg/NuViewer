//! The constraints Spine 4.2 and 4.3 added to the older IK and transform kinds: the new transform constraint (any property of a source bone
//! drives any property of other bones), path constraints (bones follow a curve), physics constraints (spring-driven bones) and sliders
//! (an animation played by a bone property).
//!
//! Physics is the one kind that needs memory from frame to frame: `PhysicsSim` carries it (the player owns one, an export makes a fresh one).

use crate::model::*;
use crate::pose::*;

const DEG: f32 = std::f32::consts::PI / 180.0;
const RAD: f32 = 180.0 / std::f32::consts::PI;
const PI: f32 = std::f32::consts::PI;
const PI2: f32 = std::f32::consts::PI * 2.0;

/// What one physics constraint remembers between frames.
#[derive(Clone, Debug)]
pub struct PhysState {
    fresh: bool,
    ux: f32,
    uy: f32,
    cx: f32,
    cy: f32,
    tx: f32,
    ty: f32,
    x_offset: f32,
    x_lag: f32,
    x_velocity: f32,
    y_offset: f32,
    y_lag: f32,
    y_velocity: f32,
    rotate_offset: f32,
    rotate_lag: f32,
    rotate_velocity: f32,
    scale_offset: f32,
    scale_lag: f32,
    scale_velocity: f32,
    remaining: f32,
    last_time: f32,
}

impl Default for PhysState {
    fn default() -> Self {
        PhysState { fresh: true, ux: 0.0, uy: 0.0, cx: 0.0, cy: 0.0, tx: 0.0, ty: 0.0, x_offset: 0.0, x_lag: 0.0, x_velocity: 0.0, y_offset: 0.0, y_lag: 0.0, y_velocity: 0.0, rotate_offset: 0.0, rotate_lag: 0.0, rotate_velocity: 0.0, scale_offset: 0.0, scale_lag: 0.0, scale_velocity: 0.0, remaining: 0.0, last_time: 0.0 }
    }
}

/// The memory of all physics constraints of a rig, and the clock they run on.
#[derive(Clone, Debug)]
pub struct PhysicsSim {
    /// Seconds the simulation has been running (advance it with `advance`).
    pub time: f32,
    pub(crate) states: Vec<PhysState>,
    /// The wind and gravity every constraint feels (Spine's defaults: wind to the right, gravity down).
    pub wind: [f32; 2],
    pub gravity: [f32; 2],
}

impl Default for PhysicsSim {
    fn default() -> Self {
        PhysicsSim { time: 0.0, states: Vec::new(), wind: [1.0, 0.0], gravity: [0.0, 1.0] }
    }
}

impl PhysicsSim {
    pub fn new() -> PhysicsSim {
        PhysicsSim::default()
    }

    /// Let `dt` seconds pass (long gaps are cut to a tenth of a second so a stall does not make the springs explode).
    pub fn advance(&mut self, dt: f32) {
        self.time += dt.clamp(0.0, 0.1);
    }

    /// Start the springs over (a new animation, a jump in time).
    pub fn reset(&mut self) {
        self.states.clear();
    }

    pub(crate) fn ensure_for(&mut self, n: usize) {
        if self.states.len() != n {
            self.states = vec![PhysState::default(); n];
            for s in &mut self.states {
                s.last_time = self.time;
            }
        }
    }
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn mix_of(mix: &[f32; 6], p: Prop) -> f32 {
    match p {
        Prop::Rotate => mix[0],
        Prop::X => mix[1],
        Prop::Y => mix[2],
        Prop::ScaleX => mix[3],
        Prop::ScaleY => mix[4],
        Prop::ShearY => mix[5],
    }
}

/// The value of `prop` of bone `b`: its local value (plus the offset), or read off its world matrix.
pub(crate) fn prop_value(b: &BoneState, prop: Prop, local: bool, off: &[f32; 6]) -> f32 {
    match prop {
        Prop::Rotate => {
            if local {
                return b.rotation + off[0];
            }
            let det = b.a * b.d - b.b * b.c;
            let v = b.c.atan2(b.a) * RAD + if det > 0.0 { off[0] } else { -off[0] };
            if v < 0.0 {
                v + 360.0
            } else {
                v
            }
        }
        Prop::X => {
            if local {
                b.x + off[1]
            } else {
                off[1] * b.a + off[2] * b.b + b.wx
            }
        }
        Prop::Y => {
            if local {
                b.y + off[2]
            } else {
                off[1] * b.c + off[2] * b.d + b.wy
            }
        }
        Prop::ScaleX => {
            if local {
                b.scale_x + off[3]
            } else {
                (b.a * b.a + b.c * b.c).sqrt() + off[3]
            }
        }
        Prop::ScaleY => {
            if local {
                b.scale_y + off[4]
            } else {
                (b.b * b.b + b.d * b.d).sqrt() + off[4]
            }
        }
        Prop::ShearY => {
            if local {
                b.shear_y + off[5]
            } else {
                (b.d.atan2(b.b) - b.c.atan2(b.a)) * RAD - 90.0 + off[5]
            }
        }
    }
}

/// Write `value` into `prop` of `bone`, `mix` of the way.
fn apply_prop(bone: &mut BoneState, prop: Prop, mut value: f32, mix: f32, local: bool, additive: bool) {
    match prop {
        Prop::Rotate => {
            if local {
                bone.rotation += (if additive { value } else { value - bone.rotation }) * mix;
            } else {
                let (a, b, c, d) = (bone.a, bone.b, bone.c, bone.d);
                value *= DEG;
                if !additive {
                    value -= c.atan2(a);
                }
                if value > PI {
                    value -= PI2;
                } else if value < -PI {
                    value += PI2;
                }
                value *= mix;
                let (cos, sin) = (value.cos(), value.sin());
                bone.a = cos * a - sin * c;
                bone.b = cos * b - sin * d;
                bone.c = sin * a + cos * c;
                bone.d = sin * b + cos * d;
            }
        }
        Prop::X => {
            if local {
                bone.x += (if additive { value } else { value - bone.x }) * mix;
            } else {
                if !additive {
                    value -= bone.wx;
                }
                bone.wx += value * mix;
            }
        }
        Prop::Y => {
            if local {
                bone.y += (if additive { value } else { value - bone.y }) * mix;
            } else {
                if !additive {
                    value -= bone.wy;
                }
                bone.wy += value * mix;
            }
        }
        Prop::ScaleX => {
            if local {
                if additive {
                    bone.scale_x *= 1.0 + (value - 1.0) * mix;
                } else if bone.scale_x != 0.0 {
                    bone.scale_x += (value - bone.scale_x) * mix;
                }
            } else if additive {
                let s = 1.0 + (value - 1.0) * mix;
                bone.a *= s;
                bone.c *= s;
            } else {
                let mut s = (bone.a * bone.a + bone.c * bone.c).sqrt();
                if s != 0.0 {
                    s = 1.0 + (value - s) * mix / s;
                    bone.a *= s;
                    bone.c *= s;
                }
            }
        }
        Prop::ScaleY => {
            if local {
                if additive {
                    bone.scale_y *= 1.0 + (value - 1.0) * mix;
                } else if bone.scale_y != 0.0 {
                    bone.scale_y += (value - bone.scale_y) * mix;
                }
            } else if additive {
                let s = 1.0 + (value - 1.0) * mix;
                bone.b *= s;
                bone.d *= s;
            } else {
                let mut s = (bone.b * bone.b + bone.d * bone.d).sqrt();
                if s != 0.0 {
                    s = 1.0 + (value - s) * mix / s;
                    bone.b *= s;
                    bone.d *= s;
                }
            }
        }
        Prop::ShearY => {
            if local {
                if !additive {
                    value -= bone.shear_y;
                }
                bone.shear_y += value * mix;
            } else {
                let (b, d) = (bone.b, bone.d);
                let by = d.atan2(b);
                value = (value + 90.0) * DEG;
                if additive {
                    value -= PI / 2.0;
                } else {
                    value -= by - bone.c.atan2(bone.a);
                    if value > PI {
                        value -= PI2;
                    } else if value < -PI {
                        value += PI2;
                    }
                }
                value = by + value * mix;
                let s = (b * b + d * d).sqrt();
                bone.b = value.cos() * s;
                bone.d = value.sin() * s;
            }
        }
    }
}

impl Pose {
    /// 4.3 transform constraint.
    pub(crate) fn transform43(&mut self, rig: &Rig, d: &TransformData, v: &Transform43, mix: [f32; 6]) {
        if mix.iter().all(|&m| m == 0.0) {
            return;
        }
        let src = self.bones[d.target].clone();
        let off = [d.rotation, d.x, d.y, d.scale_x, d.scale_y, d.shear_y];
        for &bi in &d.bones {
            for from in &v.props {
                let value = prop_value(&src, from.prop, v.local_source, &off) - from.offset;
                for to in &from.to {
                    let m = mix_of(&mix, to.prop);
                    if m == 0.0 {
                        continue;
                    }
                    let mut value = to.offset + value * to.scale;
                    if v.clamp {
                        value = if to.offset < to.max { clamp(value, to.offset, to.max) } else { clamp(value, to.max, to.offset) };
                    }
                    apply_prop(&mut self.bones[bi], to.prop, value, m, v.local_target, v.additive);
                }
            }
        }
        if v.local_target {
            self.update_subtrees(rig, &d.bones);
        } else {
            for &bi in &d.bones {
                self.set_local_from_world(rig, bi);
            }
            self.update_below(rig, &d.bones);
        }
    }

    /// Recompute everything below `roots` but not the roots themselves (their world matrix was set directly).
    pub(crate) fn update_below(&mut self, rig: &Rig, roots: &[usize]) {
        let Some(&start) = roots.iter().min() else { return };
        let n = self.bones.len();
        let mut is_root = vec![false; n];
        for &r in roots {
            if r < n {
                is_root[r] = true;
            }
        }
        let mut dirty = is_root.clone();
        for i in start..n {
            if is_root[i] {
                continue;
            }
            if let Some(p) = rig.bones[i].parent {
                dirty[i] = dirty[p];
            }
            if dirty[i] {
                self.update_bone(rig, i);
            }
        }
    }

    /// The path constraint `d`, `st` = position, spacing, mix rotate, mix x, mix y.
    pub(crate) fn path_constraint(&mut self, rig: &Rig, d: &PathData, st: [f32; 5]) {
        let [position, spacing, mix_rotate, mix_x, mix_y] = st;
        if mix_rotate == 0.0 && mix_x == 0.0 && mix_y == 0.0 {
            return;
        }
        let Some(name) = self.slots.get(d.slot).and_then(|s| s.attachment.clone()) else { return };
        let Some((Attachment::Path(path), found_in)) = rig.find_attachment(self.skin, d.slot, &name) else { return };
        let slot_bone = rig.slots[d.slot].bone;
        let deform = self.slots[d.slot].deform.iter().find(|x| x.0 == found_in && x.1 == name).map(|x| &x.2);
        let world: Vec<f32> = rig.world_vertices(self, slot_bone, &path.vertices, deform).iter().flat_map(|p| [p[0], p[1]]).collect();
        if world.len() < 12 {
            return;
        }
        let tangents = d.rotate_mode == RotateMode::Tangent;
        let scale = d.rotate_mode == RotateMode::ChainScale;
        let bone_count = d.bones.len();
        let spaces_count = if tangents { bone_count } else { bone_count + 1 };
        let mut spaces = vec![0.0f32; spaces_count];
        let mut lengths = vec![0.0f32; if scale { bone_count } else { 0 }];
        let setup_len = |i: usize| rig.bones[d.bones[i]].length;
        match d.spacing_mode {
            SpacingMode::Percent => {
                if scale {
                    for i in 0..spaces_count.saturating_sub(1) {
                        let b = &self.bones[d.bones[i]];
                        let (x, y) = (setup_len(i) * b.a, setup_len(i) * b.c);
                        lengths[i] = (x * x + y * y).sqrt();
                    }
                }
                for s in spaces.iter_mut().skip(1) {
                    *s = spacing;
                }
            }
            SpacingMode::Proportional => {
                let mut sum = 0.0;
                let mut i = 0;
                while i + 1 < spaces_count {
                    let b = &self.bones[d.bones[i]];
                    let sl = setup_len(i);
                    if sl < 1e-5 {
                        if scale {
                            lengths[i] = 0.0;
                        }
                        i += 1;
                        spaces[i] = spacing;
                    } else {
                        let (x, y) = (sl * b.a, sl * b.c);
                        let length = (x * x + y * y).sqrt();
                        if scale {
                            lengths[i] = length;
                        }
                        i += 1;
                        spaces[i] = length;
                        sum += length;
                    }
                }
                if sum > 0.0 {
                    let k = spaces_count as f32 / sum * spacing;
                    for s in spaces.iter_mut().skip(1) {
                        *s *= k;
                    }
                }
            }
            _ => {
                let length_spacing = d.spacing_mode == SpacingMode::Length;
                let mut i = 0;
                while i + 1 < spaces_count {
                    let b = &self.bones[d.bones[i]];
                    let sl = setup_len(i);
                    if sl < 1e-5 {
                        if scale {
                            lengths[i] = 0.0;
                        }
                        i += 1;
                        spaces[i] = spacing;
                    } else {
                        let (x, y) = (sl * b.a, sl * b.c);
                        let length = (x * x + y * y).sqrt();
                        if scale {
                            lengths[i] = length;
                        }
                        i += 1;
                        spaces[i] = (if length_spacing { (sl + spacing).max(0.0) } else { spacing }) * length / sl;
                    }
                }
            }
        }
        let positions = path_positions(d, path, &world, &spaces, spaces_count, tangents, position);
        if positions.len() < 3 {
            return;
        }
        let (mut bone_x, mut bone_y) = (positions[0], positions[1]);
        let mut offset_rotation = d.offset_rotation;
        let tip;
        if offset_rotation == 0.0 {
            tip = d.rotate_mode == RotateMode::Chain;
        } else {
            tip = false;
            let b = &self.bones[slot_bone];
            offset_rotation *= if b.a * b.d - b.b * b.c > 0.0 { DEG } else { -DEG };
        }
        let mut ip = 3;
        for i in 0..bone_count {
            let bi = d.bones[i];
            {
                let b = &mut self.bones[bi];
                b.wx += (bone_x - b.wx) * mix_x;
                b.wy += (bone_y - b.wy) * mix_y;
            }
            let (x, y) = (positions[ip], positions[ip + 1]);
            let (dx, dy) = (x - bone_x, y - bone_y);
            if scale {
                let length = lengths[i];
                if length != 0.0 {
                    let s = ((dx * dx + dy * dy).sqrt() / length - 1.0) * mix_rotate + 1.0;
                    let b = &mut self.bones[bi];
                    b.a *= s;
                    b.c *= s;
                }
            }
            bone_x = x;
            bone_y = y;
            if mix_rotate > 0.0 {
                let (a, b, c, dd) = (self.bones[bi].a, self.bones[bi].b, self.bones[bi].c, self.bones[bi].d);
                let mut r = if tangents {
                    positions[ip - 1]
                } else if spaces[i + 1] == 0.0 {
                    positions[ip + 2]
                } else {
                    dy.atan2(dx)
                };
                r -= c.atan2(a);
                if tip {
                    let (cos, sin) = (r.cos(), r.sin());
                    let length = rig.bones[bi].length;
                    bone_x += (length * (cos * a - sin * c) - dx) * mix_rotate;
                    bone_y += (length * (sin * a + cos * c) - dy) * mix_rotate;
                } else {
                    r += offset_rotation;
                }
                if r > PI {
                    r -= PI2;
                } else if r < -PI {
                    r += PI2;
                }
                r *= mix_rotate;
                let (cos, sin) = (r.cos(), r.sin());
                let bs = &mut self.bones[bi];
                bs.a = cos * a - sin * c;
                bs.b = cos * b - sin * dd;
                bs.c = sin * a + cos * c;
                bs.d = sin * b + cos * dd;
            }
            ip += 3;
        }
        for &bi in &d.bones {
            self.set_local_from_world(rig, bi);
        }
        self.update_below(rig, &d.bones);
    }

    /// One physics constraint step. `st` = inertia, strength, damping, mass inverse, wind, gravity, mix.
    pub(crate) fn physics_constraint(&mut self, rig: &Rig, d: &PhysicsData, st: [f32; 7], ps: &mut PhysState, time: f32, wind: [f32; 2], gravity: [f32; 2]) {
        let mix = st[6];
        if mix == 0.0 {
            return;
        }
        let (inertia, strength, damping, mass_inverse, wind_v, gravity_v) = (st[0], st[1], st[2], st[3], st[4], st[5]);
        let x = d.x > 0.0;
        let y = d.y > 0.0;
        let rotate_or_shear_x = d.rotate > 0.0 || d.shear_x > 0.0;
        let scale_x = d.scale_x > 0.0;
        let bi = d.bone;
        let l = rig.bones[bi].length;
        let t = d.step;
        let mut z: f32;
        {
            let delta = (time - ps.last_time).max(0.0);
            let aa = ps.remaining;
            ps.remaining += delta;
            ps.last_time = time;
            let (bx, by) = (self.bones[bi].wx, self.bones[bi].wy);
            if ps.fresh {
                ps.fresh = false;
                ps.ux = bx;
                ps.uy = by;
                z = 0.0;
            } else {
                let mut a = ps.remaining;
                let i = inertia;
                let f = rig.reference_scale;
                let (mut dd, mut m, mut e) = (-1.0f32, 0.0f32, 0.0f32);
                let qx = d.limit * delta;
                let qy = qx;
                z = 0.0;
                if x || y {
                    if x {
                        let u = (ps.ux - bx) * i;
                        ps.x_offset += clamp(u, -qx, qx);
                        ps.ux = bx;
                    }
                    if y {
                        let u = (ps.uy - by) * i;
                        ps.y_offset += clamp(u, -qy, qy);
                        ps.uy = by;
                    }
                    if a >= t {
                        let (xs, ys) = (ps.x_offset, ps.y_offset);
                        dd = damping.powf(60.0 * t);
                        m = t * mass_inverse;
                        e = strength;
                        let (w, g) = (f * wind_v, f * gravity_v);
                        let ax = w * wind[0] + g * gravity[0];
                        let ay = w * wind[1] + g * gravity[1];
                        loop {
                            if x {
                                ps.x_velocity += (ax - ps.x_offset * e) * m;
                                ps.x_offset += ps.x_velocity * t;
                                ps.x_velocity *= dd;
                            }
                            if y {
                                ps.y_velocity -= (ay + ps.y_offset * e) * m;
                                ps.y_offset += ps.y_velocity * t;
                                ps.y_velocity *= dd;
                            }
                            a -= t;
                            if a < t {
                                break;
                            }
                        }
                        ps.x_lag = ps.x_offset - xs;
                        ps.y_lag = ps.y_offset - ys;
                    }
                    z = (1.0 - a / t).max(0.0);
                    let b = &mut self.bones[bi];
                    if x {
                        b.wx += (ps.x_offset - ps.x_lag * z) * mix * d.x;
                    }
                    if y {
                        b.wy += (ps.y_offset - ps.y_lag * z) * mix * d.y;
                    }
                }
                if rotate_or_shear_x || scale_x {
                    let b = self.bones[bi].clone();
                    let ca = b.c.atan2(b.a);
                    let (mut c, mut s, mut mr) = (0.0f32, 0.0f32, 0.0f32);
                    let _ = (c, s, mr);
                    let (mut dx, mut dy) = (ps.cx - b.wx, ps.cy - b.wy);
                    dx = clamp(dx, -qx, qx);
                    dy = clamp(dy, -qy, qy);
                    a = ps.remaining;
                    let world_scale_x = (b.a * b.a + b.c * b.c).sqrt();
                    if rotate_or_shear_x {
                        mr = (d.rotate + d.shear_x) * mix;
                        z = ps.rotate_lag * (1.0 - aa / t).max(0.0);
                        let r = (dy + ps.ty).atan2(dx + ps.tx) - ca - (ps.rotate_offset - z) * mr;
                        ps.rotate_offset += (r - (r / PI2 - 0.5).ceil() * PI2) * i;
                        let r = (ps.rotate_offset - z) * mr + ca;
                        c = r.cos();
                        s = r.sin();
                        if scale_x {
                            let r = l * world_scale_x;
                            if r > 0.0 {
                                ps.scale_offset += (dx * c + dy * s) * i / r;
                            }
                        }
                    } else {
                        c = ca.cos();
                        s = ca.sin();
                        let r = l * world_scale_x - ps.scale_lag * (1.0 - aa / t).max(0.0);
                        if r > 0.0 {
                            ps.scale_offset += (dx * c + dy * s) * i / r;
                        }
                    }
                    if a >= t {
                        if dd == -1.0 {
                            dd = damping.powf(60.0 * t);
                            m = t * mass_inverse;
                            e = strength;
                        }
                        let ax = wind_v * wind[0] + gravity_v * gravity[0];
                        let ay = wind_v * wind[1] + gravity_v * gravity[1];
                        let (rs, ss, h) = (ps.rotate_offset, ps.scale_offset, l / f);
                        loop {
                            a -= t;
                            if scale_x {
                                ps.scale_velocity += (ax * c - ay * s - ps.scale_offset * e) * m;
                                ps.scale_offset += ps.scale_velocity * t;
                                ps.scale_velocity *= dd;
                            }
                            if rotate_or_shear_x {
                                ps.rotate_velocity -= ((ax * s + ay * c) * h + ps.rotate_offset * e) * m;
                                ps.rotate_offset += ps.rotate_velocity * t;
                                ps.rotate_velocity *= dd;
                                if a < t {
                                    break;
                                }
                                let r = ps.rotate_offset * mr + ca;
                                c = r.cos();
                                s = r.sin();
                            } else if a < t {
                                break;
                            }
                        }
                        ps.rotate_lag = ps.rotate_offset - rs;
                        ps.scale_lag = ps.scale_offset - ss;
                    }
                    z = (1.0 - a / t).max(0.0);
                }
                ps.remaining = a;
            }
            ps.cx = self.bones[bi].wx;
            ps.cy = self.bones[bi].wy;
        }
        let bone = &mut self.bones[bi];
        if rotate_or_shear_x {
            let mut o = (ps.rotate_offset - ps.rotate_lag * z) * mix;
            if d.shear_x > 0.0 {
                let mut r = 0.0;
                if d.rotate > 0.0 {
                    r = o * d.rotate;
                    let (s, c) = (r.sin(), r.cos());
                    let a = bone.b;
                    bone.b = c * a - s * bone.d;
                    bone.d = s * a + c * bone.d;
                }
                r += o * d.shear_x;
                let (s, c) = (r.sin(), r.cos());
                let a = bone.a;
                bone.a = c * a - s * bone.c;
                bone.c = s * a + c * bone.c;
            } else {
                o *= d.rotate;
                let (s, c) = (o.sin(), o.cos());
                let a = bone.a;
                bone.a = c * a - s * bone.c;
                bone.c = s * a + c * bone.c;
                let a = bone.b;
                bone.b = c * a - s * bone.d;
                bone.d = s * a + c * bone.d;
            }
        }
        if scale_x {
            let mut s = 1.0 + (ps.scale_offset - ps.scale_lag * z) * mix * d.scale_x;
            bone.a *= s;
            bone.c *= s;
            match d.scale_y_mode {
                ScaleYMode::Uniform => {
                    bone.b *= s;
                    bone.d *= s;
                }
                ScaleYMode::Volume => {
                    s = s.abs();
                    s = if s >= 0.7 { 1.0 / s } else { 4.0 - 3.67347 * s };
                    bone.b *= s;
                    bone.d *= s;
                }
                ScaleYMode::None => {}
            }
        }
        ps.tx = l * bone.a;
        ps.ty = l * bone.c;
        self.set_local_from_world(rig, bi);
        self.update_below(rig, &[bi]);
    }
}

/// Where along the path each bone goes: x, y and the tangent angle per space (3 numbers each, after 2 spare ones at the end).
fn path_positions(d: &PathData, path: &PathAtt, world_all: &[f32], spaces: &[f32], spaces_count: usize, tangents: bool, mut position: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; spaces_count * 3 + 2];
    let closed = path.closed;
    let vertices_length0 = world_all.len();
    let mut curve_count = vertices_length0 / 6;
    // `computeWorldVertices(start, count)`: world coordinates of the vertices that start at float `start`.
    let wv = |start: usize, count: usize| -> Vec<f32> { (start..start + count).map(|i| world_all.get(i).copied().unwrap_or(0.0)).collect() };
    if !path.constant_speed {
        let lengths = &path.lengths;
        curve_count = curve_count.saturating_sub(if closed { 1 } else { 2 });
        let path_length = lengths.get(curve_count).copied().unwrap_or(0.0);
        if d.position_mode == PositionMode::Percent {
            position *= path_length;
        }
        let multiplier = match d.spacing_mode {
            SpacingMode::Percent => path_length,
            SpacingMode::Proportional => path_length / spaces_count as f32,
            _ => 1.0,
        };
        let mut world: Vec<f32> = vec![0.0; 8];
        let (mut prev_curve, mut curve) = (-1i32, 0usize);
        for i in 0..spaces_count {
            let o = i * 3;
            let space = spaces[i] * multiplier;
            position += space;
            let mut p = position;
            if closed {
                p %= path_length;
                if p < 0.0 {
                    p += path_length;
                }
                curve = 0;
            } else if p < 0.0 {
                if prev_curve != -2 {
                    prev_curve = -2;
                    world = wv(2, 4);
                }
                add_before(p, &world, 0, &mut out, o);
                continue;
            } else if p > path_length {
                if prev_curve != -3 {
                    prev_curve = -3;
                    world = wv(vertices_length0 - 6, 4);
                }
                add_after(p - path_length, &world, 0, &mut out, o);
                continue;
            }
            loop {
                let Some(&length) = lengths.get(curve) else { break };
                if p > length && curve + 1 < lengths.len() {
                    curve += 1;
                    continue;
                }
                if curve == 0 {
                    p /= length;
                } else {
                    let prev = lengths[curve - 1];
                    p = (p - prev) / (length - prev);
                }
                break;
            }
            if curve as i32 != prev_curve {
                prev_curve = curve as i32;
                if closed && curve == curve_count {
                    let mut w = wv(vertices_length0 - 4, 4);
                    w.extend(wv(0, 4));
                    world = w;
                } else {
                    world = wv(curve * 6 + 2, 8);
                }
            }
            add_curve(p, world[0], world[1], world[2], world[3], world[4], world[5], world[6], world[7], &mut out, o, tangents || (i > 0 && space == 0.0));
        }
        return out;
    }
    // Constant speed: measure the curves.
    let mut vertices_length = vertices_length0;
    let world: Vec<f32>;
    if closed {
        vertices_length += 2;
        let mut w = wv(2, vertices_length - 4);
        w.extend(wv(0, 2));
        w.push(w[0]);
        w.push(w[1]);
        world = w;
    } else {
        curve_count = curve_count.saturating_sub(1);
        vertices_length -= 4;
        world = wv(2, vertices_length);
    }
    if world.len() < 6 * curve_count + 2 {
        return out;
    }
    let mut curves = vec![0.0f32; curve_count];
    let mut path_length = 0.0f32;
    let (mut x1, mut y1) = (world[0], world[1]);
    let mut w = 2;
    for i in 0..curve_count {
        let (cx1, cy1, cx2, cy2, x2, y2) = (world[w], world[w + 1], world[w + 2], world[w + 3], world[w + 4], world[w + 5]);
        let tmpx = (x1 - cx1 * 2.0 + cx2) * 0.1875;
        let tmpy = (y1 - cy1 * 2.0 + cy2) * 0.1875;
        let dddfx = ((cx1 - cx2) * 3.0 - x1 + x2) * 0.09375;
        let dddfy = ((cy1 - cy2) * 3.0 - y1 + y2) * 0.09375;
        let mut ddfx = tmpx * 2.0 + dddfx;
        let mut ddfy = tmpy * 2.0 + dddfy;
        let mut dfx = (cx1 - x1) * 0.75 + tmpx + dddfx * 0.16666667;
        let mut dfy = (cy1 - y1) * 0.75 + tmpy + dddfy * 0.16666667;
        path_length += (dfx * dfx + dfy * dfy).sqrt();
        dfx += ddfx;
        dfy += ddfy;
        ddfx += dddfx;
        ddfy += dddfy;
        path_length += (dfx * dfx + dfy * dfy).sqrt();
        dfx += ddfx;
        dfy += ddfy;
        path_length += (dfx * dfx + dfy * dfy).sqrt();
        dfx += ddfx + dddfx;
        dfy += ddfy + dddfy;
        path_length += (dfx * dfx + dfy * dfy).sqrt();
        curves[i] = path_length;
        x1 = x2;
        y1 = y2;
        w += 6;
    }
    if d.position_mode == PositionMode::Percent {
        position *= path_length;
    }
    let multiplier = match d.spacing_mode {
        SpacingMode::Percent => path_length,
        SpacingMode::Proportional => path_length / spaces_count as f32,
        _ => 1.0,
    };
    let mut segments = [0.0f32; 10];
    let (mut curve_length, mut prev_curve, mut curve, mut segment) = (0.0f32, -1i32, 0usize, 0usize);
    let (mut sx1, mut sy1, mut scx1, mut scy1, mut scx2, mut scy2, mut sx2, mut sy2) = (0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..spaces_count {
        let o = i * 3;
        let space = spaces[i] * multiplier;
        position += space;
        let mut p = position;
        if closed {
            p %= path_length;
            if p < 0.0 {
                p += path_length;
            }
            curve = 0;
            segment = 0;
        } else if p < 0.0 {
            add_before(p, &world, 0, &mut out, o);
            continue;
        } else if p > path_length {
            add_after(p - path_length, &world, vertices_length - 4, &mut out, o);
            continue;
        }
        loop {
            let Some(&length) = curves.get(curve) else { break };
            if p > length && curve + 1 < curves.len() {
                curve += 1;
                continue;
            }
            if curve == 0 {
                p /= length;
            } else {
                let prev = curves[curve - 1];
                p = (p - prev) / (length - prev);
            }
            break;
        }
        if curve as i32 != prev_curve {
            prev_curve = curve as i32;
            let ii = curve * 6;
            sx1 = world[ii];
            sy1 = world[ii + 1];
            scx1 = world[ii + 2];
            scy1 = world[ii + 3];
            scx2 = world[ii + 4];
            scy2 = world[ii + 5];
            sx2 = world[ii + 6];
            sy2 = world[ii + 7];
            let tmpx = (sx1 - scx1 * 2.0 + scx2) * 0.03;
            let tmpy = (sy1 - scy1 * 2.0 + scy2) * 0.03;
            let dddfx = ((scx1 - scx2) * 3.0 - sx1 + sx2) * 0.006;
            let dddfy = ((scy1 - scy2) * 3.0 - sy1 + sy2) * 0.006;
            let mut ddfx = tmpx * 2.0 + dddfx;
            let mut ddfy = tmpy * 2.0 + dddfy;
            let mut dfx = (scx1 - sx1) * 0.3 + tmpx + dddfx * 0.16666667;
            let mut dfy = (scy1 - sy1) * 0.3 + tmpy + dddfy * 0.16666667;
            curve_length = (dfx * dfx + dfy * dfy).sqrt();
            segments[0] = curve_length;
            for seg in segments.iter_mut().take(8).skip(1) {
                dfx += ddfx;
                dfy += ddfy;
                ddfx += dddfx;
                ddfy += dddfy;
                curve_length += (dfx * dfx + dfy * dfy).sqrt();
                *seg = curve_length;
            }
            dfx += ddfx;
            dfy += ddfy;
            curve_length += (dfx * dfx + dfy * dfy).sqrt();
            segments[8] = curve_length;
            dfx += ddfx + dddfx;
            dfy += ddfy + dddfy;
            curve_length += (dfx * dfx + dfy * dfy).sqrt();
            segments[9] = curve_length;
            segment = 0;
        }
        p *= curve_length;
        loop {
            let length = segments[segment];
            if p > length && segment + 1 < segments.len() {
                segment += 1;
                continue;
            }
            if segment == 0 {
                p /= length;
            } else {
                let prev = segments[segment - 1];
                p = segment as f32 + (p - prev) / (length - prev);
            }
            break;
        }
        add_curve(p * 0.1, sx1, sy1, scx1, scy1, scx2, scy2, sx2, sy2, &mut out, o, tangents || (i > 0 && space == 0.0));
    }
    out
}

fn add_before(p: f32, temp: &[f32], i: usize, out: &mut [f32], o: usize) {
    let (x1, y1) = (temp[i], temp[i + 1]);
    let (dx, dy) = (temp[i + 2] - x1, temp[i + 3] - y1);
    let r = dy.atan2(dx);
    out[o] = x1 + p * r.cos();
    out[o + 1] = y1 + p * r.sin();
    out[o + 2] = r;
}

fn add_after(p: f32, temp: &[f32], i: usize, out: &mut [f32], o: usize) {
    let (x1, y1) = (temp[i + 2], temp[i + 3]);
    let (dx, dy) = (x1 - temp[i], y1 - temp[i + 1]);
    let r = dy.atan2(dx);
    out[o] = x1 + p * r.cos();
    out[o + 1] = y1 + p * r.sin();
    out[o + 2] = r;
}

#[allow(clippy::too_many_arguments)]
fn add_curve(p: f32, x1: f32, y1: f32, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x2: f32, y2: f32, out: &mut [f32], o: usize, tangents: bool) {
    if p == 0.0 || p.is_nan() {
        out[o] = x1;
        out[o + 1] = y1;
        out[o + 2] = (cy1 - y1).atan2(cx1 - x1);
        return;
    }
    let (tt, ttt, u) = (p * p, p * p * p, 1.0 - p);
    let (uu, uuu) = (u * u, u * u * u);
    let (ut, ut3) = (u * p, u * p * 3.0);
    let (uut3, utt3) = (u * ut3, ut3 * p);
    let x = x1 * uuu + cx1 * uut3 + cx2 * utt3 + x2 * ttt;
    let y = y1 * uuu + cy1 * uut3 + cy2 * utt3 + y2 * ttt;
    out[o] = x;
    out[o + 1] = y;
    if tangents {
        out[o + 2] = if p < 0.001 { (cy1 - y1).atan2(cx1 - x1) } else { (y - (y1 * uu + cy1 * ut * 2.0 + cy2 * tt)).atan2(x - (x1 * uu + cx1 * ut * 2.0 + cx2 * tt)) };
    }
}
