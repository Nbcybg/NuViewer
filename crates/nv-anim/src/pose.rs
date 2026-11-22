//! A skeleton in a pose: the animation applied to the setup pose, bone world transforms (all five inherit modes), IK and transform
//! constraints, and the geometry (world-space triangles with texture coordinates) in draw order.
//!
//! One animation is applied at a time with full strength (no mixing between animations). Physics needs a `PhysicsSim` (frame to frame memory).

use crate::constraints::{prop_value, PhysicsSim};
use crate::model::*;

const DEG: f32 = std::f32::consts::PI / 180.0;
const RAD: f32 = 180.0 / std::f32::consts::PI;

#[derive(Clone, Debug, Default)]
pub struct BoneState {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub shear_x: f32,
    pub shear_y: f32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub wx: f32,
    pub wy: f32,
}

#[derive(Clone, Debug)]
pub struct SlotState {
    pub color: [f32; 4],
    pub dark: Option<[f32; 3]>,
    pub attachment: Option<String>,
    /// Sampled deform timelines: (skin, attachment name, offsets).
    pub deform: Vec<(usize, String, Vec<f32>)>,
    /// The user switched this slot off in the layer panel, or set its opacity (1 = as the file has it).
    pub user_alpha: f32,
    /// Which picture of an image sequence the slot shows (`None`: the one the file sets up).
    pub seq: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Pose {
    pub bones: Vec<BoneState>,
    pub slots: Vec<SlotState>,
    pub draw_order: Vec<usize>,
    pub skin: usize,
}

/// One thing to draw: a region or a mesh, in world space.
#[derive(Clone, Debug)]
pub struct DrawItem {
    pub slot: usize,
    pub page: usize,
    pub blend: BlendMode,
    pub pma: bool,
    pub color: [f32; 4],
    /// The dark colour of a two-colour tint (what black texels become), when the slot has one.
    pub dark: Option<[f32; 3]>,
    pub verts: Vec<[f32; 2]>,
    /// Fractions of the page picture.
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<u16>,
    /// A clipping polygon (world space) this item is cut to.
    pub clip: Option<std::sync::Arc<Vec<[f32; 2]>>>,
    /// Live2D clipping masks: the silhouette of other drawables this item is cut to.
    pub mask: Option<std::sync::Arc<MaskGeom>>,
}

/// World-space triangles whose union is a mask (`inverted`: everything but them).
#[derive(Clone, Debug, Default)]
pub struct MaskGeom {
    pub tris: Vec<[[f32; 2]; 3]>,
    pub inverted: bool,
}

fn wrap180(mut a: f32) -> f32 {
    while a > 180.0 {
        a -= 360.0;
    }
    while a < -180.0 {
        a += 360.0;
    }
    a
}

impl Pose {
    /// Recompute every bone's world matrix from its local values (parents come first in the list).
    pub fn update_world(&mut self, rig: &Rig) {
        for i in 0..self.bones.len() {
            self.update_bone(rig, i);
        }
    }

    /// Recompute `roots` and everything below them (bones are listed parents first), leaving the rest of the skeleton alone.
    pub(crate) fn update_subtrees(&mut self, rig: &Rig, roots: &[usize]) {
        let Some(&start) = roots.iter().min() else { return };
        let n = self.bones.len();
        let mut dirty = vec![false; n];
        for &r in roots {
            if r < n {
                dirty[r] = true;
            }
        }
        for i in start..n {
            if !dirty[i] {
                if let Some(p) = rig.bones[i].parent {
                    dirty[i] = dirty[p];
                }
            }
            if dirty[i] {
                self.update_bone(rig, i);
            }
        }
    }

    pub(crate) fn update_bone(&mut self, rig: &Rig, i: usize) {
        let (x, y, rot, sx, sy, shx, shy) = {
            let b = &self.bones[i];
            (b.x, b.y, b.rotation, b.scale_x, b.scale_y, b.shear_x, b.shear_y)
        };
        let data = &rig.bones[i];
        let Some(p) = data.parent else {
            let (rx, ry) = ((rot + shx) * DEG, (rot + 90.0 + shy) * DEG);
            let b = &mut self.bones[i];
            b.a = rx.cos() * sx;
            b.b = ry.cos() * sy;
            b.c = rx.sin() * sx;
            b.d = ry.sin() * sy;
            b.wx = x;
            b.wy = y;
            return;
        };
        let par = self.bones[p].clone();
        let (mut pa, mut pb, mut pc, mut pd) = (par.a, par.b, par.c, par.d);
        let wx = pa * x + pb * y + par.wx;
        let wy = pc * x + pd * y + par.wy;
        let (a, b, c, d);
        match data.inherit {
            Inherit::Normal => {
                let (rx, ry) = ((rot + shx) * DEG, (rot + 90.0 + shy) * DEG);
                let (la, lb, lc, ld) = (rx.cos() * sx, ry.cos() * sy, rx.sin() * sx, ry.sin() * sy);
                a = pa * la + pb * lc;
                b = pa * lb + pb * ld;
                c = pc * la + pd * lc;
                d = pc * lb + pd * ld;
            }
            Inherit::OnlyTranslation => {
                let (rx, ry) = ((rot + shx) * DEG, (rot + 90.0 + shy) * DEG);
                a = rx.cos() * sx;
                b = ry.cos() * sy;
                c = rx.sin() * sx;
                d = ry.sin() * sy;
            }
            Inherit::NoRotationOrReflection => {
                let mut s = pa * pa + pc * pc;
                let prx;
                if s > 0.0001 {
                    s = (pa * pd - pb * pc).abs() / s;
                    pb = pc * s;
                    pd = pa * s;
                    prx = pc.atan2(pa) * RAD;
                } else {
                    pa = 0.0;
                    pc = 0.0;
                    prx = 90.0 - pd.atan2(pb) * RAD;
                }
                let (rx, ry) = ((rot + shx - prx) * DEG, (rot + shy - prx + 90.0) * DEG);
                let (la, lb, lc, ld) = (rx.cos() * sx, ry.cos() * sy, rx.sin() * sx, ry.sin() * sy);
                a = pa * la - pb * lc;
                b = pa * lb - pb * ld;
                c = pc * la + pd * lc;
                d = pc * lb + pd * ld;
            }
            Inherit::NoScale | Inherit::NoScaleOrReflection => {
                let (cs, sn) = ((rot * DEG).cos(), (rot * DEG).sin());
                let mut za = pa * cs + pb * sn;
                let mut zc = pc * cs + pd * sn;
                let mut s = (za * za + zc * zc).sqrt();
                if s > 0.00001 {
                    s = 1.0 / s;
                }
                za *= s;
                zc *= s;
                s = (za * za + zc * zc).sqrt();
                if data.inherit == Inherit::NoScale && (pa * pd - pb * pc < 0.0) {
                    s = -s;
                }
                let r = std::f32::consts::FRAC_PI_2 + zc.atan2(za);
                let zb = r.cos() * s;
                let zd = r.sin() * s;
                let (la, lb, lc, ld) = ((shx * DEG).cos() * sx, ((90.0 + shy) * DEG).cos() * sy, (shx * DEG).sin() * sx, ((90.0 + shy) * DEG).sin() * sy);
                a = za * la + zb * lc;
                b = za * lb + zb * ld;
                c = zc * la + zd * lc;
                d = zc * lb + zd * ld;
            }
        }
        let bs = &mut self.bones[i];
        bs.a = a;
        bs.b = b;
        bs.c = c;
        bs.d = d;
        bs.wx = wx;
        bs.wy = wy;
    }

    /// Make the local values reproduce the world matrix as it is now (after a constraint changed it directly).
    pub(crate) fn set_local_from_world(&mut self, rig: &Rig, i: usize) {
        let (a, b, c, d, wx, wy) = {
            let s = &self.bones[i];
            (s.a, s.b, s.c, s.d, s.wx, s.wy)
        };
        let data = &rig.bones[i];
        let (mut lx, mut ly) = (wx, wy);
        let (mut ra, mut rb, mut rc, mut rd) = (a, b, c, d);
        if let Some(p) = data.parent {
            let par = &self.bones[p];
            let pid = 1.0 / (par.a * par.d - par.b * par.c);
            let (ia, ib, ic, id) = (par.d * pid, par.b * pid, par.c * pid, par.a * pid);
            let (dx, dy) = (wx - par.wx, wy - par.wy);
            lx = dx * ia - dy * ib;
            ly = dy * id - dx * ic;
            if data.inherit != Inherit::OnlyTranslation {
                ra = ia * a - ib * c;
                rb = ia * b - ib * d;
                rc = id * c - ic * a;
                rd = id * d - ic * b;
            }
        }
        let s = &mut self.bones[i];
        s.x = lx;
        s.y = ly;
        s.shear_x = 0.0;
        s.scale_x = (ra * ra + rc * rc).sqrt();
        if s.scale_x > 0.0001 {
            let det = ra * rd - rb * rc;
            s.scale_y = det / s.scale_x;
            s.shear_y = -(ra * rb + rc * rd).atan2(det) * RAD;
            s.rotation = rc.atan2(ra) * RAD;
        } else {
            s.scale_x = 0.0;
            s.scale_y = (rb * rb + rd * rd).sqrt();
            s.shear_y = 0.0;
            s.rotation = 90.0 - rd.atan2(rb) * RAD;
        }
    }

    // ---- constraints ----------------------------------------------------------------------------------------------------------

    fn ik_one(&mut self, rig: &Rig, bone: usize, tx: f32, ty: f32, compress: bool, stretch: bool, uniform: bool, alpha: f32) {
        let (px, py, pa, pb, pc, pd) = match rig.bones[bone].parent {
            Some(p) => {
                let q = &self.bones[p];
                (q.wx, q.wy, q.a, q.b, q.c, q.d)
            }
            None => (0.0, 0.0, 1.0, 0.0, 0.0, 1.0),
        };
        let b = self.bones[bone].clone();
        let det = pa * pd - pb * pc;
        if det.abs() < 1e-9 {
            return;
        }
        let (x, y) = (tx - px, ty - py);
        let (lx, ly) = ((x * pd - y * pb) / det - b.x, (y * pa - x * pc) / det - b.y);
        let mut rot = ly.atan2(lx) * RAD - b.shear_x - b.rotation;
        if b.scale_x < 0.0 {
            rot += 180.0;
        }
        rot = wrap180(rot);
        let (mut sx, mut sy) = (b.scale_x, b.scale_y);
        if compress || stretch {
            let (dx, dy) = (tx - self.bones[bone].wx, ty - self.bones[bone].wy);
            let len = rig.bones[bone].length * sx;
            let dd = (dx * dx + dy * dy).sqrt();
            if ((compress && dd < len) || (stretch && dd > len)) && len > 0.0001 {
                let s = (dd / len - 1.0) * alpha + 1.0;
                sx *= s;
                if uniform {
                    sy *= s;
                }
            }
        }
        let s = &mut self.bones[bone];
        s.rotation = b.rotation + rot * alpha;
        s.scale_x = sx;
        s.scale_y = sy;
    }

    fn ik_two(&mut self, rig: &Rig, parent: usize, child: usize, tx: f32, ty: f32, bend: f32, stretch: bool, uniform: bool, mut softness: f32, alpha: f32) {
        let p = self.bones[parent].clone();
        let c = self.bones[child].clone();
        let (px, py) = (p.x, p.y);
        let (mut psx, mut psy, mut csx) = (p.scale_x, p.scale_y, c.scale_x);
        let (os1, mut s2, os2);
        if psx < 0.0 {
            psx = -psx;
            os1 = 180.0;
            s2 = -1.0;
        } else {
            os1 = 0.0;
            s2 = 1.0;
        }
        if psy < 0.0 {
            psy = -psy;
            s2 = -s2;
        }
        if csx < 0.0 {
            csx = -csx;
            os2 = 180.0;
        } else {
            os2 = 0.0;
        }
        let cx = c.x;
        let (cy, cwx, cwy);
        let u = (psx - psy).abs() <= 0.0001;
        if !u || stretch {
            cy = 0.0;
            cwx = p.a * cx + p.wx;
            cwy = p.c * cx + p.wy;
        } else {
            cy = c.y;
            cwx = p.a * cx + p.b * cy + p.wx;
            cwy = p.c * cx + p.d * cy + p.wy;
        }
        let (pp_a, pp_b, pp_c, pp_d, pp_x, pp_y) = match rig.bones[parent].parent {
            Some(g) => {
                let q = &self.bones[g];
                (q.a, q.b, q.c, q.d, q.wx, q.wy)
            }
            None => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
        };
        let mut id = pp_a * pp_d - pp_b * pp_c;
        id = if id.abs() <= 0.0001 { 0.0 } else { 1.0 / id };
        let (x, y) = (cwx - pp_x, cwy - pp_y);
        let (dx, dy) = ((x * pp_d - y * pp_b) * id - px, (y * pp_a - x * pp_c) * id - py);
        let l1 = (dx * dx + dy * dy).sqrt();
        let mut l2 = rig.bones[child].length * csx;
        if l1 < 0.0001 {
            self.ik_one(rig, parent, tx, ty, false, stretch, false, alpha);
            let ch = &mut self.bones[child];
            ch.y = cy;
            return;
        }
        let (x, y) = (tx - pp_x, ty - pp_y);
        let (mut ttx, mut tty) = ((x * pp_d - y * pp_b) * id - px, (y * pp_a - x * pp_c) * id - py);
        let mut dd = ttx * ttx + tty * tty;
        if softness != 0.0 {
            softness *= psx * (csx + 1.0) * 0.5;
            let td = dd.sqrt();
            let sd = td - l1 - l2 * psx + softness;
            if sd > 0.0 {
                let mut q = (sd / (softness * 2.0)).min(1.0) - 1.0;
                q = (sd - softness * (1.0 - q * q)) / td;
                ttx -= q * ttx;
                tty -= q * tty;
                dd = ttx * ttx + tty * tty;
            }
        }
        // (The non-uniform scale case uses the same law of cosines: exact only for equal parent scales.)
        l2 *= psx;
        let mut cos = (dd - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
        let a2;
        if cos < -1.0 {
            cos = -1.0;
            a2 = std::f32::consts::PI * bend;
        } else if cos > 1.0 {
            cos = 1.0;
            a2 = 0.0;
            if stretch {
                let a = (dd.sqrt() / (l1 + l2) - 1.0) * alpha + 1.0;
                psx *= a;
                if uniform {
                    psy *= a;
                }
            }
        } else {
            a2 = cos.acos() * bend;
        }
        let aa = l1 + l2 * cos;
        let bb = l2 * a2.sin();
        let a1 = (tty * aa - ttx * bb).atan2(ttx * aa + tty * bb);
        let os = cy.atan2(cx) * s2;
        let mut a1d = (a1 - os) * RAD + os1 - p.rotation;
        a1d = wrap180(a1d);
        let mut a2d = ((a2 + os) * RAD - c.shear_x) * s2 + os2 - c.rotation;
        a2d = wrap180(a2d);
        let ps = &mut self.bones[parent];
        ps.rotation = p.rotation + a1d * alpha;
        ps.scale_x = psx;
        ps.scale_y = psy;
        let cs = &mut self.bones[child];
        cs.x = cx;
        cs.y = cy;
        cs.rotation = c.rotation + a2d * alpha;
    }

    fn transform_constraint(&mut self, rig: &Rig, data: &TransformData, mix: [f32; 6]) {
        let t = self.bones[data.target].clone();
        let (ta, tb, tc, td) = (t.a, t.b, t.c, t.d);
        let positive = ta * td - tb * tc > 0.0;
        let off_rot = data.rotation * if positive { DEG } else { -DEG };
        let off_shear = data.shear_y * if positive { DEG } else { -DEG };
        let [mr, mx, my, msx, msy, msh] = mix;
        for &bi in &data.bones {
            let mut b = self.bones[bi].clone();
            if mr != 0.0 {
                let mut r = tc.atan2(ta) - b.c.atan2(b.a) + off_rot;
                if r > std::f32::consts::PI {
                    r -= 2.0 * std::f32::consts::PI;
                } else if r < -std::f32::consts::PI {
                    r += 2.0 * std::f32::consts::PI;
                }
                r *= mr;
                let (cs, sn) = (r.cos(), r.sin());
                let (a, bb, c, d) = (b.a, b.b, b.c, b.d);
                b.a = cs * a - sn * c;
                b.b = cs * bb - sn * d;
                b.c = sn * a + cs * c;
                b.d = sn * bb + cs * d;
            }
            if mx != 0.0 || my != 0.0 {
                let (tx, ty) = (data.x * ta + data.y * tb + t.wx, data.x * tc + data.y * td + t.wy);
                b.wx += (tx - b.wx) * mx;
                b.wy += (ty - b.wy) * my;
            }
            if msx != 0.0 {
                let mut s = (b.a * b.a + b.c * b.c).sqrt();
                if s != 0.0 {
                    s = (s + ((ta * ta + tc * tc).sqrt() - s + data.scale_x) * msx) / s;
                }
                b.a *= s;
                b.c *= s;
            }
            if msy != 0.0 {
                let mut s = (b.b * b.b + b.d * b.d).sqrt();
                if s != 0.0 {
                    s = (s + ((tb * tb + td * td).sqrt() - s + data.scale_y) * msy) / s;
                }
                b.b *= s;
                b.d *= s;
            }
            if msh != 0.0 {
                let by = b.d.atan2(b.b);
                let mut r = td.atan2(tb) - tc.atan2(ta) - (by - b.c.atan2(b.a));
                if r > std::f32::consts::PI {
                    r -= 2.0 * std::f32::consts::PI;
                } else if r < -std::f32::consts::PI {
                    r += 2.0 * std::f32::consts::PI;
                }
                let r = by + (r + off_shear) * msh;
                let s = (b.b * b.b + b.d * b.d).sqrt();
                b.b = r.cos() * s;
                b.d = r.sin() * s;
            }
            self.bones[bi] = b;
            self.set_local_from_world(rig, bi);
        }
    }
}

// ---- sampling an animation onto the setup pose ---------------------------------------------------------------------------------------

/// The numbers constraints take from animation: what the timelines set this frame, for every constraint.
pub(crate) struct Dynamic {
    ik: Vec<[f32; 6]>,
    tf: Vec<[f32; 6]>,
    /// position, spacing, mix rotate, mix x, mix y
    path: Vec<[f32; 5]>,
    /// inertia, strength, damping, mass inverse, wind, gravity, mix
    phys: Vec<[f32; 7]>,
    /// time, mix
    slider: Vec<[f32; 2]>,
}

impl Rig {
    /// The setup pose with `animation` at `time` (seconds) applied, constraints solved. Physics constraints stay at rest (see `pose_sim`).
    pub fn pose(&self, skin: usize, animation: Option<usize>, time: f32) -> Pose {
        self.pose_sim(skin, animation, time, None)
    }

    /// The same, with physics constraints swinging: `sim` remembers where they were the frame before (and says what time it is).
    pub fn pose_sim(&self, skin: usize, animation: Option<usize>, time: f32, sim: Option<&mut PhysicsSim>) -> Pose {
        self.pose_mix(skin, None, animation, time, 1.0, sim)
    }

    /// The same, easing from another animation: `from` (its number and time) is applied first, then `animation` takes over by `alpha`
    /// (0 = still all of `from`, 1 = all of `animation`). Bones ease; slots, constraints and deform switch at once.
    pub fn pose_mix(&self, skin: usize, from: Option<(usize, f32)>, animation: Option<usize>, time: f32, alpha: f32, mut sim: Option<&mut PhysicsSim>) -> Pose {
        let skin = skin.min(self.skins.len().saturating_sub(1));
        let mut pose = Pose {
            bones: self.bones.iter().map(|b| BoneState { x: b.x, y: b.y, rotation: b.rotation, scale_x: b.scale_x, scale_y: b.scale_y, shear_x: b.shear_x, shear_y: b.shear_y, ..Default::default() }).collect(),
            slots: self.slots.iter().map(|s| SlotState { color: s.color, dark: s.dark, attachment: s.attachment.clone(), deform: Vec::new(), user_alpha: 1.0, seq: None }).collect(),
            draw_order: (0..self.slots.len()).collect(),
            skin,
        };
        let mut dynamic = Dynamic {
            ik: self.ik.iter().map(|c| [c.mix, c.softness, if c.bend_positive { 1.0 } else { -1.0 }, c.compress as u8 as f32, c.stretch as u8 as f32, 0.0]).collect(),
            tf: self.transform.iter().map(|c| [c.mix_rotate, c.mix_x, c.mix_y, c.mix_scale_x, c.mix_scale_y, c.mix_shear_y]).collect(),
            path: self.path.iter().map(|c| [c.position, c.spacing, c.mix_rotate, c.mix_x, c.mix_y]).collect(),
            phys: self.physics.iter().map(|c| [c.inertia, c.strength, c.damping, c.mass_inverse, c.wind, c.gravity, c.mix]).collect(),
            slider: self.slider.iter().map(|c| [c.time, c.mix]).collect(),
        };
        let from_anim = from.and_then(|(a, t)| self.animations.get(a).map(|an| (an, t))).filter(|_| alpha < 1.0);
        if let Some((old, ot)) = from_anim {
            self.apply_animation(old, ot, &mut pose, &mut dynamic, 1.0);
        }
        if let Some(anim) = animation.and_then(|i| self.animations.get(i)) {
            self.apply_animation(anim, time, &mut pose, &mut dynamic, if from_anim.is_some() { alpha.clamp(0.0, 1.0) } else { 1.0 });
        }
        pose.update_world(self);
        // Constraints in their order.
        let mut order: Vec<(i32, CKind, usize)> = Vec::new();
        order.extend(self.ik.iter().enumerate().map(|(i, c)| (c.order, CKind::Ik, i)));
        order.extend(self.transform.iter().enumerate().map(|(i, c)| (c.order, CKind::Transform, i)));
        order.extend(self.path.iter().enumerate().map(|(i, c)| (c.order, CKind::Path, i)));
        order.extend(self.physics.iter().enumerate().map(|(i, c)| (c.order, CKind::Physics, i)));
        order.extend(self.slider.iter().enumerate().map(|(i, c)| (c.order, CKind::Slider, i)));
        order.sort_by_key(|o| o.0);
        let sk = &self.skins[skin];
        if let Some(s) = sim.as_deref_mut() {
            s.ensure_for(self.physics.len());
        }
        for (_, kind, i) in order {
            match kind {
                CKind::Ik => {
                    let c = &self.ik[i];
                    let st = dynamic.ik[i];
                    if (c.skin_required && !sk.ik.contains(&i)) || st[0] == 0.0 {
                        continue;
                    }
                    let t = pose.bones[c.target].clone();
                    match c.bones.len() {
                        1 => pose.ik_one(self, c.bones[0], t.wx, t.wy, st[3] != 0.0, st[4] != 0.0, c.uniform, st[0]),
                        2 => pose.ik_two(self, c.bones[0], c.bones[1], t.wx, t.wy, st[2], st[4] != 0.0, c.uniform, st[1], st[0]),
                        _ => {}
                    }
                    pose.update_subtrees(self, &c.bones);
                }
                CKind::Transform => {
                    let c = &self.transform[i];
                    if c.skin_required && !sk.transform.contains(&i) {
                        continue;
                    }
                    let st = dynamic.tf[i];
                    if st.iter().all(|&m| m == 0.0) {
                        continue;
                    }
                    match &c.v43 {
                        Some(v) => pose.transform43(self, c, v, st),
                        None => {
                            pose.transform_constraint(self, c, st);
                            pose.update_subtrees(self, &c.bones);
                        }
                    }
                }
                CKind::Path => {
                    let c = &self.path[i];
                    if c.skin_required && !sk.extra.contains(&CRef { kind: CKind::Path, index: i }) {
                        continue;
                    }
                    pose.path_constraint(self, c, dynamic.path[i]);
                }
                CKind::Physics => {
                    let c = &self.physics[i];
                    if c.skin_required && !sk.extra.contains(&CRef { kind: CKind::Physics, index: i }) {
                        continue;
                    }
                    if let Some(s) = sim.as_deref_mut() {
                        let (time, wind, gravity) = (s.time, s.wind, s.gravity);
                        pose.physics_constraint(self, c, dynamic.phys[i], &mut s.states[i], time, wind, gravity);
                    }
                }
                CKind::Slider => {
                    let c = &self.slider[i];
                    if c.skin_required && !sk.extra.contains(&CRef { kind: CKind::Slider, index: i }) {
                        continue;
                    }
                    let st = dynamic.slider[i];
                    let Some(anim) = c.animation.and_then(|a| self.animations.get(a)) else { continue };
                    if st[1] == 0.0 {
                        continue;
                    }
                    let mut t = st[0];
                    if let Some(bi) = c.bone {
                        t = c.offset + (prop_value(&pose.bones[bi], c.property, c.local, &[0.0; 6]) - c.from) * c.scale;
                    }
                    t = if c.looped {
                        if anim.duration > 0.0 {
                            t.rem_euclid(anim.duration)
                        } else {
                            0.0
                        }
                    } else {
                        t.max(0.0)
                    };
                    let touched = self.apply_bone_timelines(anim, t, &mut pose, st[1], c.additive);
                    // (the animation of a slider may recolour slots and play image sequences too: that is what a spinning gem does)
                    self.apply_slot_timelines(anim, t, &mut pose, st[1]);
                    pose.update_subtrees(self, &touched);
                }
            }
        }
        pose
    }

    fn apply_animation(&self, anim: &Animation, t: f32, pose: &mut Pose, dynamic: &mut Dynamic, bone_alpha: f32) {
        self.apply_bone_timelines(anim, t, pose, bone_alpha, false);
        self.apply_slot_timelines(anim, t, pose, 1.0);
        for (ci, tr) in &anim.ik {
            if let (Some(v), Some(st)) = (tr.sample(t, false), dynamic.ik.get_mut(*ci)) {
                *st = [v[0], v[1], v[2], v[3], v[4], 0.0];
            }
        }
        for (ci, tr) in &anim.transform {
            if let (Some(v), Some(st)) = (tr.sample(t, false), dynamic.tf.get_mut(*ci)) {
                st.copy_from_slice(&v[..6]);
            }
        }
        for (ci, tl) in &anim.path {
            let Some(st) = dynamic.path.get_mut(*ci) else { continue };
            match tl {
                PathTl::Position(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        st[0] = v[0];
                    }
                }
                PathTl::Spacing(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        st[1] = v[0];
                    }
                }
                PathTl::Mix(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        st[2..5].copy_from_slice(&v[..3]);
                    }
                }
            }
        }
        for (ci, tl) in &anim.physics {
            let PhysTl::Prop(prop, tr) = tl else { continue };
            let Some(v) = tr.sample(t, false) else { continue };
            let slot = match prop {
                PhysProp::Inertia => 0,
                PhysProp::Strength => 1,
                PhysProp::Damping => 2,
                PhysProp::Mass => 3,
                PhysProp::Wind => 4,
                PhysProp::Gravity => 5,
                PhysProp::Mix => 6,
            };
            let value = if slot == 3 { 1.0 / v[0].max(1e-6) } else { v[0] };
            let range = match ci {
                Some(c) => *c..*c + 1,
                None => 0..dynamic.phys.len(),
            };
            for c in range {
                // A skeleton-wide timeline (no constraint named) moves only the constraints that follow it.
                if ci.is_none() && !self.physics.get(c).map(|p| p.global[slot.min(6) as usize]).unwrap_or(false) {
                    continue;
                }
                if let Some(st) = dynamic.phys.get_mut(c) {
                    st[slot] = value;
                }
            }
        }
        for (ci, tl) in &anim.slider {
            let Some(st) = dynamic.slider.get_mut(*ci) else { continue };
            match tl {
                SliderTl::Time(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        st[0] = v[0];
                    }
                }
                SliderTl::Mix(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        st[1] = v[0];
                    }
                }
            }
        }
        for d in &anim.deform {
            let Some(i) = d.times.partition_point(|&k| k <= t).checked_sub(1) else { continue };
            let vals: Vec<f32> = if i + 1 >= d.times.len() {
                d.frames[i].clone()
            } else {
                let (t0, t1) = (d.times[i], d.times[i + 1]);
                let frac = if t1 > t0 { ((t - t0) / (t1 - t0)).clamp(0.0, 1.0) } else { 1.0 };
                let p = match d.curves[i] {
                    Curve::Stepped => 0.0,
                    Curve::Linear => frac,
                    Curve::Bezier(c) => bezier_y(c, frac),
                    Curve::BezierAbs(_) => frac,
                };
                d.frames[i].iter().zip(&d.frames[i + 1]).map(|(a, b)| a + (b - a) * p).collect()
            };
            pose.slots[d.slot].deform.push((d.skin, d.attachment.clone(), vals));
        }
        if let Some(i) = anim.draw_order.partition_point(|k| k.0 <= t).checked_sub(1) {
            pose.draw_order = apply_draw_order(self.slots.len(), &anim.draw_order[i].1);
        }
    }

    /// The slot timelines of `anim` at `t` (colours, attachments, image-sequence frames), `alpha` of the way: colours ease, attachments switch
    /// once it is at least half.
    pub(crate) fn apply_slot_timelines(&self, anim: &Animation, t: f32, pose: &mut Pose, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        let ease = |cur: f32, new: f32| if alpha >= 1.0 { new } else { cur + (new - cur) * alpha };
        for (si, tl) in &anim.slots {
            let s = &mut pose.slots[*si];
            match tl {
                SlotTl::Attachment(keys) => {
                    if alpha >= 0.5 {
                        if let Some(i) = keys.partition_point(|k| k.0 <= t).checked_sub(1) {
                            s.attachment = keys[i].1.clone();
                        }
                    }
                }
                SlotTl::Color(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        for k in 0..4 {
                            s.color[k] = ease(s.color[k], v[k]);
                        }
                    }
                }
                SlotTl::Rgb(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        for k in 0..3 {
                            s.color[k] = ease(s.color[k], v[k]);
                        }
                    }
                }
                SlotTl::Alpha(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        s.color[3] = ease(s.color[3], v[0]);
                    }
                }
                SlotTl::TwoColor(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        for k in 0..4 {
                            s.color[k] = ease(s.color[k], v[k]);
                        }
                        let d = s.dark.unwrap_or([0.0; 3]);
                        s.dark = Some([ease(d[0], v[4]), ease(d[1], v[5]), ease(d[2], v[6])]);
                    }
                }
                SlotTl::Rgb2(tr) => {
                    if let Some(v) = tr.sample(t, false) {
                        for k in 0..3 {
                            s.color[k] = ease(s.color[k], v[k]);
                        }
                        let d = s.dark.unwrap_or([0.0; 3]);
                        s.dark = Some([ease(d[0], v[3]), ease(d[1], v[4]), ease(d[2], v[5])]);
                    }
                }
            }
        }
        if alpha >= 0.5 {
        for sq in &anim.sequence {
            let Some(s) = pose.slots.get_mut(sq.slot) else { continue };
            if s.attachment.as_deref() != Some(sq.attachment.as_str()) {
                continue;
            }
            let count = match self.find_attachment(pose.skin, sq.slot, &sq.attachment) {
                Some((Attachment::Region(r), _)) => r.seq.map(|q| q.count),
                Some((Attachment::Mesh(m), _)) => m.seq.map(|q| q.count),
                _ => None,
            };
            if let Some(count) = count {
                s.seq = sequence_frame(&sq.keys, t, count);
            }
        }
        }
    }

    /// Apply the bone timelines of `anim` at `t`, `alpha` of the way (1 = replace the pose), added to the pose when `additive`. Gives the bones touched.
    pub(crate) fn apply_bone_timelines(&self, anim: &Animation, t: f32, pose: &mut Pose, alpha: f32, additive: bool) -> Vec<usize> {
        let mut touched: Vec<usize> = Vec::new();
        // Where a value ends up: replacing the pose, easing towards the animation, or adding to it.
        let put = |cur: f32, setup: f32, v: f32, scale: bool, rotate: bool| -> f32 {
            if additive {
                if scale {
                    cur * (1.0 + (v - 1.0) * alpha)
                } else {
                    cur + v * alpha
                }
            } else {
                let target = if scale { setup * v } else { setup + v };
                if alpha >= 1.0 {
                    target
                } else if rotate {
                    cur + wrap180(target - cur) * alpha
                } else {
                    cur + (target - cur) * alpha
                }
            }
        };
        for (bi, tl) in &anim.bones {
            let data = &self.bones[*bi];
            let b = &mut pose.bones[*bi];
            let mut hit = true;
            match tl {
                BoneTl::Rotate(tr) => match tr.sample(t, self.legacy) {
                    Some(v) => b.rotation = put(b.rotation, data.rotation, v[0], false, true),
                    None => hit = false,
                },
                BoneTl::Translate(tr) => match tr.sample(t, false) {
                    Some(v) => {
                        b.x = put(b.x, data.x, v[0], false, false);
                        b.y = put(b.y, data.y, v[1], false, false);
                    }
                    None => hit = false,
                },
                BoneTl::TranslateX(tr) => match tr.sample(t, false) {
                    Some(v) => b.x = put(b.x, data.x, v[0], false, false),
                    None => hit = false,
                },
                BoneTl::TranslateY(tr) => match tr.sample(t, false) {
                    Some(v) => b.y = put(b.y, data.y, v[0], false, false),
                    None => hit = false,
                },
                BoneTl::Scale(tr) => match tr.sample(t, false) {
                    Some(v) => {
                        b.scale_x = put(b.scale_x, data.scale_x, v[0], true, false);
                        b.scale_y = put(b.scale_y, data.scale_y, v[1], true, false);
                    }
                    None => hit = false,
                },
                BoneTl::ScaleX(tr) => match tr.sample(t, false) {
                    Some(v) => b.scale_x = put(b.scale_x, data.scale_x, v[0], true, false),
                    None => hit = false,
                },
                BoneTl::ScaleY(tr) => match tr.sample(t, false) {
                    Some(v) => b.scale_y = put(b.scale_y, data.scale_y, v[0], true, false),
                    None => hit = false,
                },
                BoneTl::Shear(tr) => match tr.sample(t, false) {
                    Some(v) => {
                        b.shear_x = put(b.shear_x, data.shear_x, v[0], false, false);
                        b.shear_y = put(b.shear_y, data.shear_y, v[1], false, false);
                    }
                    None => hit = false,
                },
                BoneTl::ShearX(tr) => match tr.sample(t, false) {
                    Some(v) => b.shear_x = put(b.shear_x, data.shear_x, v[0], false, false),
                    None => hit = false,
                },
                BoneTl::ShearY(tr) => match tr.sample(t, false) {
                    Some(v) => b.shear_y = put(b.shear_y, data.shear_y, v[0], false, false),
                    None => hit = false,
                },
            }
            if hit && !touched.contains(bi) {
                touched.push(*bi);
            }
        }
        touched
    }
}

/// The setup order with each listed slot moved by its offset (Spine's algorithm).
pub fn apply_draw_order(n: usize, offsets: &[(usize, i32)]) -> Vec<usize> {
    let mut offs: Vec<(usize, i32)> = offsets.iter().copied().filter(|o| o.0 < n).collect();
    offs.sort_by_key(|o| o.0);
    let mut order: Vec<i64> = vec![-1; n];
    let mut unchanged: Vec<usize> = Vec::with_capacity(n);
    let mut original = 0usize;
    for (slot, off) in &offs {
        while original != *slot {
            unchanged.push(original);
            original += 1;
        }
        let target = original as i64 + *off as i64;
        if target >= 0 && (target as usize) < n {
            order[target as usize] = original as i64;
        }
        original += 1;
    }
    while original < n {
        unchanged.push(original);
        original += 1;
    }
    let mut out = vec![0usize; n];
    for i in (0..n).rev() {
        out[i] = if order[i] == -1 { unchanged.pop().unwrap_or(0) } else { order[i] as usize };
    }
    out
}

// ---- geometry ----------------------------------------------------------------------------------------------------------------------

impl Rig {
    pub(crate) fn world_vertices(&self, pose: &Pose, bone: usize, v: &Vertices, deform: Option<&Vec<f32>>) -> Vec<[f32; 2]> {
        match v {
            Vertices::Plain(p) => {
                let b = &pose.bones[bone];
                (0..p.len() / 2)
                    .map(|i| {
                        let (x, y) = (p[i * 2] + deform.and_then(|d| d.get(i * 2)).copied().unwrap_or(0.0), p[i * 2 + 1] + deform.and_then(|d| d.get(i * 2 + 1)).copied().unwrap_or(0.0));
                        [x * b.a + y * b.b + b.wx, x * b.c + y * b.d + b.wy]
                    })
                    .collect()
            }
            Vertices::Weighted(w) => {
                let mut k = 0usize;
                w.iter()
                    .map(|infl| {
                        let (mut wx, mut wy) = (0.0f32, 0.0f32);
                        for i in infl {
                            let (dx, dy) = (deform.and_then(|d| d.get(k)).copied().unwrap_or(0.0), deform.and_then(|d| d.get(k + 1)).copied().unwrap_or(0.0));
                            k += 2;
                            let Some(b) = pose.bones.get(i.bone) else { continue };
                            let (vx, vy) = (i.x + dx, i.y + dy);
                            wx += (vx * b.a + vy * b.b + b.wx) * i.weight;
                            wy += (vx * b.c + vy * b.d + b.wy) * i.weight;
                        }
                        [wx, wy]
                    })
                    .collect()
            }
        }
    }

    /// Everything to draw for `pose`, back to front.
    pub fn geometry(&self, pose: &Pose) -> Vec<DrawItem> {
        let mut out = Vec::new();
        let mut clip: Option<(usize, std::sync::Arc<Vec<[f32; 2]>>)> = None;
        for &si in &pose.draw_order {
            let (Some(slot), Some(state)) = (self.slots.get(si), pose.slots.get(si)) else { continue };
            let Some(name) = state.attachment.as_deref() else {
                if let Some((end, _)) = &clip {
                    if *end == si {
                        clip = None;
                    }
                }
                continue;
            };
            let Some((att, found_in)) = self.find_attachment(pose.skin, si, name) else { continue };
            let slot_color = [state.color[0], state.color[1], state.color[2], state.color[3] * state.user_alpha];
            match att {
                Attachment::Clip(c) => {
                    let deform = state.deform.iter().find(|d| d.0 == found_in && d.1 == name).map(|d| &d.2);
                    let poly = self.world_vertices(pose, slot.bone, &c.vertices, deform);
                    if poly.len() >= 3 {
                        clip = Some((c.end_slot, std::sync::Arc::new(poly)));
                    }
                    continue;
                }
                Attachment::Region(r) => {
                    if slot_color[3] * r.color[3] > 0.0 {
                        if let Some(item) = self.region_item(pose, si, slot.bone, r, slot_color, slot.blend, state.seq) {
                            out.push(DrawItem { clip: clip.as_ref().map(|c| c.1.clone()), dark: state.dark, ..item });
                        }
                    }
                }
                Attachment::Mesh(m) => {
                    if slot_color[3] * m.color[3] > 0.0 {
                        let deform = state.deform.iter().find(|d| d.0 == m.deform_skin && d.1 == m.deform_name).map(|d| &d.2);
                        if let Some(item) = self.mesh_item(pose, si, slot.bone, m, slot_color, slot.blend, deform, state.seq) {
                            out.push(DrawItem { clip: clip.as_ref().map(|c| c.1.clone()), dark: state.dark, ..item });
                        }
                    }
                }
                Attachment::Other | Attachment::Path(_) => {}
            }
            if let Some((end, _)) = &clip {
                if *end == si {
                    clip = None;
                }
            }
        }
        out
    }

    fn region_item(&self, pose: &Pose, slot: usize, bone: usize, r: &Region, slot_color: [f32; 4], blend: BlendMode, seq: Option<usize>) -> Option<DrawItem> {
        let reg = match r.seq {
            Some(q) => self.atlas.find(&q.frame_name(&r.path, seq.unwrap_or(q.setup)))?,
            None => self.atlas.find(&r.path)?,
        };
        let (pw, ph) = self.atlas.page_size(reg.page);
        let (orig_w, orig_h) = (if reg.orig_w > 0.0 { reg.orig_w } else { reg.w }, if reg.orig_h > 0.0 { reg.orig_h } else { reg.h });
        let (sx, sy) = (r.width / orig_w * r.scale_x, r.height / orig_h * r.scale_y);
        let lx = -r.width / 2.0 * r.scale_x + reg.off_x * sx;
        let ly = -r.height / 2.0 * r.scale_y + reg.off_y * sy;
        let (lx2, ly2) = (lx + reg.w * sx, ly + reg.h * sy);
        let (cs, sn) = ((r.rotation * DEG).cos(), (r.rotation * DEG).sin());
        let corner = |x: f32, y: f32| [x * cs - y * sn + r.x, y * cs + x * sn + r.y];
        let local = [corner(lx, ly), corner(lx, ly2), corner(lx2, ly2), corner(lx2, ly)]; // BL UL UR BR
        let b = &pose.bones[bone];
        let verts: Vec<[f32; 2]> = local.iter().map(|p| [p[0] * b.a + p[1] * b.b + b.wx, p[0] * b.c + p[1] * b.d + b.wy]).collect();
        let (u, v) = (reg.x / pw, reg.y / ph);
        // "rotate" says the picture was packed turned counter-clockwise by that many degrees (a quarter turn swaps width and height on the page).
        let swapped = reg.rotate == 90 || reg.rotate == 270;
        let (u2, v2) = if swapped { ((reg.x + reg.h) / pw, (reg.y + reg.w) / ph) } else { ((reg.x + reg.w) / pw, (reg.y + reg.h) / ph) };
        // Corners in the order bottom-left, top-left, top-right, bottom-right.
        let uvs = match reg.rotate {
            90 => vec![[u2, v2], [u, v2], [u, v], [u2, v]],
            180 => vec![[u2, v], [u2, v2], [u, v2], [u, v]],
            270 => vec![[u, v], [u2, v], [u2, v2], [u, v2]],
            _ => vec![[u, v2], [u, v], [u2, v], [u2, v2]],
        };
        let color = [slot_color[0] * r.color[0], slot_color[1] * r.color[1], slot_color[2] * r.color[2], slot_color[3] * r.color[3]];
        Some(DrawItem { slot, page: reg.page, blend, pma: self.atlas.pages[reg.page].pma, color, dark: None, verts, uvs, tris: vec![0, 1, 2, 2, 3, 0], clip: None, mask: None })
    }

    #[allow(clippy::too_many_arguments)]
    fn mesh_item(&self, pose: &Pose, slot: usize, bone: usize, m: &Mesh, slot_color: [f32; 4], blend: BlendMode, deform: Option<&Vec<f32>>, seq: Option<usize>) -> Option<DrawItem> {
        let reg = match m.seq {
            Some(q) => self.atlas.find(&q.frame_name(&m.path, seq.unwrap_or(q.setup)))?,
            None => self.atlas.find(&m.path)?,
        };
        let (pw, ph) = self.atlas.page_size(reg.page);
        let verts = self.world_vertices(pose, bone, &m.vertices, deform);
        if verts.len() * 2 != m.uvs.len() {
            return None;
        }
        let (orig_w, orig_h) = (if reg.orig_w > 0.0 { reg.orig_w } else { reg.w }, if reg.orig_h > 0.0 { reg.orig_h } else { reg.h });
        let uvs: Vec<[f32; 2]> = (0..verts.len())
            .map(|i| {
                let (tu, tv) = (m.uvs[i * 2], m.uvs[i * 2 + 1]);
                let (u, v) = (reg.x / pw, reg.y / ph);
                match reg.rotate {
                    90 => {
                        let u0 = u - (orig_h - reg.off_y - reg.h) / pw;
                        let v0 = v - (orig_w - reg.off_x - reg.w) / ph;
                        [u0 + tv * (orig_h / pw), v0 + (1.0 - tu) * (orig_w / ph)]
                    }
                    180 => {
                        let u0 = u - (orig_w - reg.off_x - reg.w) / pw;
                        let v0 = v - reg.off_y / ph;
                        [u0 + (1.0 - tu) * (orig_w / pw), v0 + (1.0 - tv) * (orig_h / ph)]
                    }
                    270 => {
                        let u0 = u - reg.off_y / pw;
                        let v0 = v - reg.off_x / ph;
                        [u0 + (1.0 - tv) * (orig_h / pw), v0 + tu * (orig_w / ph)]
                    }
                    _ => {
                        let u0 = u - reg.off_x / pw;
                        let v0 = v - (orig_h - reg.off_y - reg.h) / ph;
                        [u0 + tu * (orig_w / pw), v0 + tv * (orig_h / ph)]
                    }
                }
            })
            .collect();
        let color = [slot_color[0] * m.color[0], slot_color[1] * m.color[1], slot_color[2] * m.color[2], slot_color[3] * m.color[3]];
        Some(DrawItem { slot, page: reg.page, blend, pma: self.atlas.pages[reg.page].pma, color, dark: None, verts, uvs, tris: m.triangles.clone(), clip: None, mask: None })
    }

    /// The box around everything drawn in `pose`: (min x, min y, max x, max y), or None when nothing is.
    ///
    /// Items far bigger than the rest are left out of the box: game rigs often carry black "letterbox" frames tens of times the size of the
    /// picture, and fitting those would show the picture as a speck. (With fewer than 8 items nothing is left out.)
    pub fn bounds_of(&self, pose: &Pose) -> Option<[f32; 4]> {
        let items = self.geometry(pose);
        let areas: Vec<f32> = items
            .iter()
            .map(|it| {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for v in &it.verts {
                    x0 = x0.min(v[0]);
                    y0 = y0.min(v[1]);
                    x1 = x1.max(v[0]);
                    y1 = y1.max(v[1]);
                }
                ((x1 - x0) * (y1 - y0)).max(0.0)
            })
            .collect();
        let limit = if areas.len() >= 8 {
            let mut s = areas.clone();
            s.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
            s[5] * 10.0
        } else {
            f32::MAX
        };
        // Items parked far away (a common way to hide a part: move its bone by 100000000) are left out too: keep what lies within a
        // generous reach of where most of the picture is.
        let centres: Vec<[f32; 2]> = items.iter().map(|it| {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for v in &it.verts {
                x0 = x0.min(v[0]);
                y0 = y0.min(v[1]);
                x1 = x1.max(v[0]);
                y1 = y1.max(v[1]);
            }
            [(x0 + x1) / 2.0, (y0 + y1) / 2.0]
        }).collect();
        let median = |mut v: Vec<f32>| -> f32 {
            v.retain(|x| x.is_finite());
            if v.is_empty() {
                return 0.0;
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v[v.len() / 2]
        };
        let (mx, my) = (median(centres.iter().map(|c| c[0]).collect()), median(centres.iter().map(|c| c[1]).collect()));
        let spread = median(centres.iter().map(|c| ((c[0] - mx).powi(2) + (c[1] - my).powi(2)).sqrt()).collect()).max(1.0);
        let reach = if items.len() >= 8 { spread * 12.0 } else { f32::MAX };
        // Big items that are plain black (the letterbox frames of game art) never count, whatever their number.
        let median_area = median(areas.clone());
        let mut bb: Option<[f32; 4]> = None;
        for ((item, area), c) in items.iter().zip(&areas).zip(&centres) {
            if *area > limit || !c[0].is_finite() || ((c[0] - mx).powi(2) + (c[1] - my).powi(2)).sqrt() > reach {
                continue;
            }
            if items.len() >= 8 && *area > median_area * 10.0 && self.item_is_black(item) {
                continue;
            }
            for v in &item.verts {
                let b = bb.get_or_insert([v[0], v[1], v[0], v[1]]);
                b[0] = b[0].min(v[0]);
                b[1] = b[1].min(v[1]);
                b[2] = b[2].max(v[0]);
                b[3] = b[3].max(v[1]);
            }
        }
        bb
    }

    /// Is what this item shows (the texels under its texture coordinates) black or empty? Sampled on a small grid.
    fn item_is_black(&self, item: &DrawItem) -> bool {
        let Some(img) = self.atlas.pages.get(item.page).and_then(|p| p.image.as_ref()) else { return false };
        let (mut u0, mut v0, mut u1, mut v1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for uv in &item.uvs {
            u0 = u0.min(uv[0]);
            v0 = v0.min(uv[1]);
            u1 = u1.max(uv[0]);
            v1 = v1.max(uv[1]);
        }
        let (mut seen, mut lit) = (0, 0);
        for gy in 0..6 {
            for gx in 0..6 {
                let u = u0 + (u1 - u0) * (gx as f32 + 0.5) / 6.0;
                let v = v0 + (v1 - v0) * (gy as f32 + 0.5) / 6.0;
                let (x, y) = (((u * img.width as f32) as i64).clamp(0, img.width as i64 - 1) as usize, ((v * img.height as f32) as i64).clamp(0, img.height as i64 - 1) as usize);
                let o = (y * img.width as usize + x) * 4;
                if img.rgba.get(o + 3).copied().unwrap_or(0) > 16 {
                    seen += 1;
                    if img.rgba[o] as u32 + img.rgba[o + 1] as u32 + img.rgba[o + 2] as u32 > 36 {
                        lit += 1;
                    }
                }
            }
        }
        seen > 0 && lit * 8 < seen
    }

    /// The box around the animation (or the setup pose): sampled at `samples` times.
    pub fn bounds(&self, skin: usize, animation: Option<usize>, samples: usize) -> Option<[f32; 4]> {
        let dur = animation.and_then(|i| self.animations.get(i)).map(|a| a.duration).unwrap_or(0.0);
        let n = if dur > 0.0 { samples.max(2) } else { 1 };
        let mut bb: Option<[f32; 4]> = None;
        for k in 0..n {
            let t = if n > 1 { dur * k as f32 / (n - 1) as f32 } else { 0.0 };
            if let Some(b) = self.bounds_of(&self.pose(skin, animation, t)) {
                let o = bb.get_or_insert(b);
                o[0] = o[0].min(b[0]);
                o[1] = o[1].min(b[1]);
                o[2] = o[2].max(b[2]);
                o[3] = o[3].max(b[3]);
            }
        }
        bb
    }
}
