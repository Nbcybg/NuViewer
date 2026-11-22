//! Spine 4.3's binary skeleton (`.skel`).
//!
//! 4.3 changed almost every block: one list of constraints (ik, path, transform, physics, slider) in the order they work, transform
//! constraints that are lists of property mappings, a draw order that may be split in folders, weighted vertices that say how many numbers
//! they take. The layout here follows the runtime's reader and was checked against the four 4.3.75-beta example skeletons that come with
//! playcanvas-spine, each of which has its JSON export beside it (`tests/real_samples.rs` reads both and compares the poses).

use crate::atlas::Atlas;
use crate::binary::*;
use crate::model::*;
use std::collections::HashMap;

pub(crate) fn parse(bytes: &[u8], at: usize, version: &str, atlas: Atlas) -> Res<Rig> {
    let mut r = R { b: bytes, p: at, strings: None };
    let (x, y, width, height) = (r.float()?, r.float()?, r.float()?, r.float()?);
    let reference_scale = r.float()?;
    let nonessential = r.bool()?;
    let mut fps = 30.0;
    if nonessential {
        fps = r.float()?;
        r.string()?; // the pictures' folder
        r.string()?; // the audio folder
    }
    let n = r.count(1)?;
    let mut table = Vec::with_capacity(n);
    for _ in 0..n {
        table.push(r.string()?);
    }
    r.strings = Some(table);

    // Bones.
    let nb = r.count(30)?;
    let mut bones: Vec<BoneData> = Vec::with_capacity(nb);
    for i in 0..nb {
        let name = r.string()?.unwrap_or_default();
        let parent = if i == 0 { None } else { Some(r.index(i, "parent bone")?) };
        let (rotation, bx, by, scale_x, scale_y, shear_x, shear_y) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
        let inherit = Inherit::from_index(r.byte()?);
        let length = r.float()?;
        let skin_required = r.bool()?;
        if nonessential {
            r.int()?; // colour in the editor
            r.string()?; // icon
            r.float()?; // icon size
            r.float()?; // icon rotation
            r.bool()?; // visible in the editor
        }
        bones.push(BoneData { name, parent, length, x: bx, y: by, rotation, scale_x, scale_y, shear_x, shear_y, inherit, skin_required });
    }
    if bones.is_empty() {
        return Err("skel: no bones".into());
    }

    // Slots.
    let ns = r.count(8)?;
    let mut slots: Vec<SlotData> = Vec::with_capacity(ns);
    for _ in 0..ns {
        let name = r.string()?.unwrap_or_default();
        let bone = r.index(bones.len(), "slot bone")?;
        let color = r.rgba()?;
        let dark_raw = r.int()?;
        let dark = if dark_raw == -1 { None } else { Some([((dark_raw >> 16) & 0xFF) as f32 / 255.0, ((dark_raw >> 8) & 0xFF) as f32 / 255.0, (dark_raw & 0xFF) as f32 / 255.0]) };
        let attachment = r.sref()?;
        let blend = BlendMode::from_index(r.var()? as u8);
        if nonessential {
            r.bool()?; // visible in the editor
        }
        slots.push(SlotData { name, bone, color, dark, attachment, blend });
    }

    // Constraints: one list, in the order they work.
    let nc = r.count(2)?;
    let mut ik: Vec<IkData> = Vec::new();
    let mut transform: Vec<TransformData> = Vec::new();
    let mut paths: Vec<PathData> = Vec::new();
    let mut physics: Vec<PhysicsData> = Vec::new();
    let mut sliders: Vec<SliderData> = Vec::new();
    let mut unified: Vec<CRef> = Vec::with_capacity(nc);
    let bone_list = |r: &mut R, n_bones: usize| -> Res<Vec<usize>> {
        let n = r.count(1)?;
        (0..n).map(|_| r.index(n_bones, "constraint bone")).collect()
    };
    for order in 0..nc {
        let name = r.string()?.unwrap_or_default();
        let order = order as i32;
        match r.byte()? {
            0 => {
                let bs = bone_list(&mut r, bones.len())?;
                let target = r.index(bones.len(), "IK target")?;
                let flags = r.byte()?;
                let scale_mode = if flags & 2 != 0 { ScaleYMode::from_index(r.byte()?) } else { ScaleYMode::None };
                let mix = if flags & 32 != 0 { if flags & 64 != 0 { r.float()? } else { 1.0 } } else { 0.0 };
                let softness = if flags & 128 != 0 { r.float()? } else { 0.0 };
                if bs.is_empty() || bs.len() > 2 {
                    return Err("skel: an IK constraint with an unusual bone count".into());
                }
                unified.push(CRef { kind: CKind::Ik, index: ik.len() });
                ik.push(IkData { name, order, bones: bs, target, mix, softness, bend_positive: flags & 4 == 0, compress: flags & 8 != 0, stretch: flags & 16 != 0, uniform: scale_mode != ScaleYMode::None, skin_required: flags & 1 != 0 });
            }
            1 => {
                let bs = bone_list(&mut r, bones.len())?;
                let slot = r.index(slots.len(), "path slot")?;
                let flags = r.byte()?;
                let offset_rotation = if flags & 128 != 0 { r.float()? } else { 0.0 };
                let (position, spacing, mix_rotate, mix_x, mix_y) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
                unified.push(CRef { kind: CKind::Path, index: paths.len() });
                paths.push(PathData {
                    name,
                    order,
                    bones: bs,
                    slot,
                    position_mode: if (flags >> 1) & 1 == 0 { PositionMode::Fixed } else { PositionMode::Percent },
                    spacing_mode: match (flags >> 2) & 3 {
                        0 => SpacingMode::Length,
                        1 => SpacingMode::Fixed,
                        2 => SpacingMode::Percent,
                        _ => SpacingMode::Proportional,
                    },
                    rotate_mode: match (flags >> 4) & 3 {
                        0 => RotateMode::Tangent,
                        1 => RotateMode::Chain,
                        _ => RotateMode::ChainScale,
                    },
                    offset_rotation,
                    position,
                    spacing,
                    mix_rotate,
                    mix_x,
                    mix_y,
                    skin_required: flags & 1 != 0,
                });
            }
            2 => {
                let bs = bone_list(&mut r, bones.len())?;
                let source = r.index(bones.len(), "transform source")?;
                let flags = r.byte()?;
                let mut v = Transform43 { props: Vec::new(), local_source: flags & 2 != 0, local_target: flags & 4 != 0, additive: flags & 8 != 0, clamp: flags & 16 != 0 };
                for _ in 0..(flags >> 5) {
                    let from = Prop::from_index(r.byte()?).ok_or("skel: a transform constraint reads a property that does not exist")?;
                    let offset = r.float()?;
                    let mut fp = FromProp { prop: from, offset, to: Vec::new() };
                    for _ in 0..r.byte()? {
                        let to = Prop::from_index(r.byte()?).ok_or("skel: a transform constraint writes a property that does not exist")?;
                        fp.to.push(ToProp { prop: to, offset: r.float()?, max: r.float()?, scale: r.float()? });
                    }
                    v.props.push(fp);
                }
                let f = r.byte()?;
                let opt = |r: &mut R, bit: u8| -> Res<f32> { if f & bit != 0 { r.float() } else { Ok(0.0) } };
                let (rotation, ox, oy, osx, osy, osh) = (opt(&mut r, 1)?, opt(&mut r, 2)?, opt(&mut r, 4)?, opt(&mut r, 8)?, opt(&mut r, 16)?, opt(&mut r, 32)?);
                let f = r.byte()?;
                let opt = |r: &mut R, bit: u8| -> Res<f32> { if f & bit != 0 { r.float() } else { Ok(0.0) } };
                let (mr, mx, my, msx, msy, msh) = (opt(&mut r, 1)?, opt(&mut r, 2)?, opt(&mut r, 4)?, opt(&mut r, 8)?, opt(&mut r, 16)?, opt(&mut r, 32)?);
                unified.push(CRef { kind: CKind::Transform, index: transform.len() });
                transform.push(TransformData { name, order, bones: bs, target: source, rotation, x: ox, y: oy, scale_x: osx, scale_y: osy, shear_y: osh, mix_rotate: mr, mix_x: mx, mix_y: my, mix_scale_x: msx, mix_scale_y: msy, mix_shear_y: msh, local: false, relative: false, skin_required: flags & 1 != 0, v43: Some(v) });
            }
            3 => {
                let bone = r.index(bones.len(), "physics bone")?;
                let flags = r.byte()?;
                let opt = |r: &mut R, bit: u8| -> Res<f32> { if flags & bit != 0 { r.float() } else { Ok(0.0) } };
                let (px, py, rotate) = (opt(&mut r, 2)?, opt(&mut r, 4)?, opt(&mut r, 8)?);
                let mut scale_x = opt(&mut r, 16)?;
                let mut scale_y_mode = ScaleYMode::None;
                if scale_x < -2.0 {
                    scale_y_mode = ScaleYMode::Volume;
                    scale_x = -2.0 - scale_x;
                } else if scale_x < 0.0 {
                    scale_y_mode = ScaleYMode::Uniform;
                    scale_x = -1.0 - scale_x;
                }
                let shear_x = opt(&mut r, 32)?;
                let limit = if flags & 64 != 0 { r.float()? } else { 5000.0 };
                let step_byte = r.byte()?;
                let step = if step_byte == 0 { 1.0 / 60.0 } else { 1.0 / step_byte as f32 };
                let (inertia, strength, damping) = (r.float()?, r.float()?, r.float()?);
                let mass_inverse = if flags & 128 != 0 { r.float()? } else { 1.0 };
                let (wind, gravity) = (r.float()?, r.float()?);
                let f2 = r.byte()?;
                let mix = if f2 & 128 != 0 { r.float()? } else { 1.0 };
                unified.push(CRef { kind: CKind::Physics, index: physics.len() });
                physics.push(PhysicsData {
                    name,
                    order,
                    bone,
                    x: px,
                    y: py,
                    rotate,
                    scale_x,
                    shear_x,
                    scale_y_mode,
                    limit,
                    step,
                    inertia,
                    strength,
                    damping,
                    mass_inverse,
                    wind,
                    gravity,
                    mix,
                    global: [f2 & 1 != 0, f2 & 2 != 0, f2 & 4 != 0, f2 & 8 != 0, f2 & 16 != 0, f2 & 32 != 0, f2 & 64 != 0],
                    skin_required: flags & 1 != 0,
                });
            }
            4 => {
                let flags = r.byte()?;
                let mut d = SliderData { name, order, animation: None, additive: flags & 4 != 0, looped: flags & 2 != 0, bone: None, property: Prop::Rotate, from: 0.0, offset: 0.0, scale: 1.0, local: false, time: 0.0, mix: 0.0, skin_required: flags & 1 != 0 };
                if flags & 8 != 0 {
                    let value = r.float()?;
                    if !(nonessential && flags & 64 != 0) {
                        d.time = value; // (with a bone the file keeps the editor's range here instead)
                    }
                }
                if flags & 16 != 0 {
                    d.mix = if flags & 32 != 0 { r.float()? } else { 1.0 };
                }
                if flags & 64 != 0 {
                    d.local = flags & 128 != 0;
                    d.bone = Some(r.index(bones.len(), "slider bone")?);
                    d.from = r.float()?;
                    d.property = Prop::from_index(r.byte()?).ok_or("skel: a slider reads a property that does not exist")?;
                    d.offset = r.float()?;
                    d.scale = r.float()?;
                }
                unified.push(CRef { kind: CKind::Slider, index: sliders.len() });
                sliders.push(d);
            }
            t => return Err(format!("skel: unknown constraint type {t}")),
        }
    }

    // Skins.
    let mut skins: Vec<Skin> = Vec::new();
    let mut linked: Vec<LinkedRec> = Vec::new();
    let default_slots = r.var()?;
    skins.push(Skin { name: "default".into(), slots: vec![HashMap::new(); slots.len()], ..Default::default() });
    if default_slots > 0 {
        read_skin_slots43(&mut r, &mut skins, 0, default_slots, &bones, &slots, nonessential, &mut linked)?;
    }
    let nsk = r.count(2)?;
    for _ in 0..nsk {
        let name = r.string()?.unwrap_or_default();
        let mut skin = Skin { name, slots: vec![HashMap::new(); slots.len()], ..Default::default() };
        if nonessential {
            r.int()?; // the skin's colour
        }
        let nbn = r.count(1)?;
        for _ in 0..nbn {
            skin.bones.push(r.index(bones.len(), "skin bone")?);
        }
        let ncs = r.count(1)?;
        for _ in 0..ncs {
            let u = r.index(unified.len(), "skin constraint")?;
            let c = unified[u];
            match c.kind {
                CKind::Ik => skin.ik.push(c.index),
                CKind::Transform => skin.transform.push(c.index),
                _ => skin.extra.push(c),
            }
        }
        let count = r.var()?;
        skins.push(skin);
        let si = skins.len() - 1;
        read_skin_slots43(&mut r, &mut skins, si, count, &bones, &slots, nonessential, &mut linked)?;
    }
    resolve_linked(&mut skins, linked);

    // Events (only read to stay in step).
    let ne = r.count(3)?;
    let mut event_audio: Vec<bool> = Vec::new();
    for _ in 0..ne {
        r.string()?;
        r.zig()?;
        r.float()?;
        r.string()?;
        let audio = r.string()?;
        if audio.is_some() {
            r.float()?;
            r.float()?;
        }
        event_audio.push(audio.is_some());
    }

    // Animations.
    let na = r.count(2)?;
    let mut animations = Vec::with_capacity(na);
    for _ in 0..na {
        let name = r.string()?.unwrap_or_default();
        animations.push(read_animation43(&mut r, name, nonessential, &bones, &slots, &skins, &unified, &event_audio)?);
    }
    // Every slider says which animation it plays (by number, in the order of the file).
    let mut slider_anim: Vec<(usize, usize)> = Vec::new();
    for c in &unified {
        if c.kind == CKind::Slider {
            slider_anim.push((c.index, r.index(animations.len().max(1), "slider animation")?));
        }
    }
    if r.p != bytes.len() {
        return Err(format!("skel: {} bytes are left over after the animations (the reader is out of step)", bytes.len().saturating_sub(r.p)));
    }
    let file_order: Vec<String> = animations.iter().map(|a| a.name.clone()).collect();
    animations.sort_by(|a, b| a.name.cmp(&b.name));
    for (si, ai) in slider_anim {
        sliders[si].animation = file_order.get(ai).and_then(|n| animations.iter().position(|a| &a.name == n));
    }
    Ok(Rig { version: version.to_string(), name: String::new(), x, y, width, height, fps, bones, slots, skins, ik, transform, path: paths, physics, slider: sliders, constraints: unified, reference_scale, animations, atlas, legacy: false, unsupported: Vec::new() })
}

/// Vertices in 4.3: the count of vertices, then (when weighted) how many numbers the bone data takes, then per vertex its bones.
fn read_vertices43(r: &mut R, weighted: bool, bones: &[BoneData]) -> Res<(usize, Vertices)> {
    let n = r.count(2)?;
    if !weighted {
        return Ok((n, Vertices::Plain(r.floats(n * 2)?)));
    }
    let total = r.var()?;
    let mut used = 0usize;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let bc = r.count(12)?;
        used += 1 + bc;
        let mut inf = Vec::with_capacity(bc);
        for _ in 0..bc {
            let bone = r.index(bones.len(), "vertex bone")?;
            inf.push(Influence { bone, x: r.float()?, y: r.float()?, weight: r.float()? });
        }
        out.push(inf);
    }
    if used != total {
        return Err(format!("skel: weighted vertices say {total} numbers of bone data but use {used}"));
    }
    Ok((n, Vertices::Weighted(out)))
}

#[allow(clippy::too_many_arguments)]
fn read_skin_slots43(r: &mut R, skins: &mut [Skin], si: usize, slot_count: usize, bones: &[BoneData], slots: &[SlotData], nonessential: bool, linked: &mut Vec<LinkedRec>) -> Res<()> {
    for _ in 0..slot_count {
        let slot = r.index(slots.len(), "skin slot")?;
        let n = r.count(2)?;
        for _ in 0..n {
            let key = r.sref()?.unwrap_or_default();
            if let Some(att) = read_attachment43(r, &key, si, slot, bones, slots, nonessential, linked)? {
                skins[si].slots[slot].insert(key, att);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn read_attachment43(r: &mut R, key: &str, si: usize, slot: usize, bones: &[BoneData], slots: &[SlotData], nonessential: bool, linked: &mut Vec<LinkedRec>) -> Res<Option<Attachment>> {
    let flags = r.byte()?;
    let name = if flags & 8 != 0 { r.sref()?.unwrap_or_else(|| key.to_string()) } else { key.to_string() };
    let path_of = |r: &mut R, name: &str| -> Res<String> { if flags & 16 != 0 { Ok(r.sref()?.unwrap_or_else(|| name.to_string())) } else { Ok(name.to_string()) } };
    Ok(match flags & 7 {
        0 => {
            let path = path_of(r, &name)?;
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            let seq = if flags & 64 != 0 { Some(read_seq(r)?) } else { None };
            let rotation = if flags & 128 != 0 { r.float()? } else { 0.0 };
            let (x, y, sx, sy, width, height) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
            Some(Attachment::Region(Region { path, seq, x, y, rotation, scale_x: sx, scale_y: sy, width, height, color }))
        }
        1 => {
            read_vertices43(r, flags & 16 != 0, bones)?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Other)
        }
        2 => {
            let path = path_of(r, &name)?;
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            let seq = if flags & 64 != 0 { Some(read_seq(r)?) } else { None };
            let hull = r.var()?;
            let (vc, vertices) = read_vertices43(r, flags & 128 != 0, bones)?;
            let uvs = r.floats(vc * 2)?;
            let tri_count = (vc * 2).saturating_sub(hull + 2) * 3;
            if tri_count > r.b.len() {
                return Err("skel: a mesh with an implausible triangle count".into());
            }
            let mut triangles = Vec::with_capacity(tri_count);
            for _ in 0..tri_count {
                triangles.push(r.var()? as u16);
            }
            let timeline_slots = r.count(1)?;
            for _ in 0..timeline_slots {
                r.var()?; // other slots this mesh's timelines also move
            }
            let (mut width, mut height) = (0.0, 0.0);
            if nonessential {
                let ec = r.count(1)?;
                for _ in 0..ec {
                    r.var()?;
                }
                width = r.float()?;
                height = r.float()?;
            }
            if triangles.iter().any(|&t| t as usize >= vc) {
                return Err("skel: a mesh triangle points at a vertex that does not exist".into());
            }
            Some(Attachment::Mesh(Mesh { path, seq, color, width, height, uvs, triangles, vertices, hull, deform_skin: si, deform_name: key.to_string() }))
        }
        3 => {
            let path = path_of(r, &name)?;
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            let seq = if flags & 64 != 0 { Some(read_seq(r)?) } else { None };
            let inherit_timelines = flags & 128 != 0;
            let parent_slot = r.index(slots.len(), "linked mesh source slot")?;
            let skin_index = r.var()?;
            let parent = r.sref()?.unwrap_or_default();
            let (mut w, mut h) = (0.0, 0.0);
            if nonessential {
                w = r.float()?;
                h = r.float()?;
            }
            linked.push(LinkedRec { si, slot, key: key.to_string(), parent, parent_skin: format!("#{skin_index}"), parent_slot: Some(parent_slot), deform: inherit_timelines, path, color, w, h, seq });
            None
        }
        4 => {
            let closed = flags & 16 != 0;
            let constant_speed = flags & 32 != 0;
            let (vc, vertices) = read_vertices43(r, flags & 64 != 0, bones)?;
            let lengths = r.floats(vc * 2 / 6)?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Path(PathAtt { closed, constant_speed, vertices, lengths }))
        }
        5 => {
            r.float()?;
            r.float()?;
            r.float()?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Other)
        }
        6 => {
            let end_slot = r.index(slots.len(), "clipping end slot")?;
            let (vc, vertices) = read_vertices43(r, flags & 16 != 0, bones)?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Clip(Clip { end_slot, vertices, vertex_count: vc }))
        }
        t => return Err(format!("skel: unknown attachment type {t}")),
    })
}

/// Which constraint of which kind an animation's constraint number means.
fn cref(unified: &[CRef], u: usize, kind: CKind, what: &str) -> Res<usize> {
    match unified.get(u) {
        Some(c) if c.kind == kind => Ok(c.index),
        _ => Err(format!("skel: a {what} timeline points at constraint {u}, which is not one")),
    }
}

#[allow(clippy::too_many_arguments)]
fn read_animation43(r: &mut R, name: String, nonessential: bool, bones: &[BoneData], slots: &[SlotData], skins: &[Skin], unified: &[CRef], event_audio: &[bool]) -> Res<Animation> {
    let mut anim = Animation { name, ..Default::default() };
    let mut dur = 0.0f32;
    r.var()?; // the number of timelines in all (a size hint)

    // Slots.
    let n = r.count(2)?;
    for _ in 0..n {
        let slot = r.index(slots.len(), "animation slot")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            if ty == 0 {
                let mut v = Vec::with_capacity(frames);
                for _ in 0..frames {
                    v.push((r.float()?, r.sref()?));
                }
                if let Some(l) = v.last() {
                    dur = dur.max(l.0);
                }
                anim.slots.push((slot, SlotTl::Attachment(v)));
                continue;
            }
            let _bezier = r.var()?;
            let k4 = |keys: Keys| keys.track(true);
            let tl = match ty {
                1 => SlotTl::Color(k4(read_keys4(r, frames, 4, &|_| false, &mut |r| byte_color(r, 4))?)),
                2 => SlotTl::Rgb(k4(read_keys4(r, frames, 3, &|_| false, &mut |r| byte_color(r, 3))?)),
                3 => SlotTl::TwoColor(k4(read_keys4(r, frames, 7, &|_| false, &mut |r| byte_color(r, 7))?)),
                4 => SlotTl::Rgb2(k4(read_keys4(r, frames, 6, &|_| false, &mut |r| byte_color(r, 6))?)),
                5 => SlotTl::Alpha(k4(read_keys4(r, frames, 1, &|_| false, &mut |r| byte_color(r, 1))?)),
                t => return Err(format!("skel: unknown slot timeline {t}")),
            };
            if let SlotTl::Color(t) | SlotTl::Rgb(t) | SlotTl::TwoColor(t) | SlotTl::Rgb2(t) | SlotTl::Alpha(t) = &tl {
                dur = dur.max(t.duration());
            }
            anim.slots.push((slot, tl));
        }
    }

    // Bones.
    let n = r.count(2)?;
    for _ in 0..n {
        let bone = r.index(bones.len(), "animation bone")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            if ty == 10 {
                // The inherit mode over time: not played (the setup mode stays).
                for _ in 0..frames {
                    r.float()?;
                    r.byte()?;
                }
                continue;
            }
            let _bezier = r.var()?;
            let one = &mut |r: &mut R| Ok(vec![r.float()?]);
            let two = &mut |r: &mut R| Ok(vec![r.float()?, r.float()?]);
            let tl = match ty {
                0 => BoneTl::Rotate(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                1 => BoneTl::Translate(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                2 => BoneTl::TranslateX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                3 => BoneTl::TranslateY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                4 => BoneTl::Scale(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                5 => BoneTl::ScaleX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                6 => BoneTl::ScaleY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                7 => BoneTl::Shear(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                8 => BoneTl::ShearX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                9 => BoneTl::ShearY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                t => return Err(format!("skel: unknown bone timeline {t}")),
            };
            let d = match &tl {
                BoneTl::Rotate(t) | BoneTl::Translate(t) | BoneTl::TranslateX(t) | BoneTl::TranslateY(t) | BoneTl::Scale(t) | BoneTl::ScaleX(t) | BoneTl::ScaleY(t) | BoneTl::Shear(t) | BoneTl::ShearX(t) | BoneTl::ShearY(t) => t.duration(),
            };
            dur = dur.max(d);
            anim.bones.push((bone, tl));
        }
    }

    // IK constraints.
    let n = r.count(2)?;
    for _ in 0..n {
        let u = r.index(unified.len(), "animation IK")?;
        let ci = cref(unified, u, CKind::Ik, "IK")?;
        let frames = r.count(4)?;
        let _bezier = r.var()?;
        let mix_of = |r: &mut R, flags: u8| -> Res<f32> {
            if flags & 1 != 0 {
                if flags & 2 != 0 { r.float() } else { Ok(1.0) }
            } else {
                Ok(0.0)
            }
        };
        let mut keys = Keys::new(6);
        if frames > 0 {
            let mut flags = r.byte()?;
            let mut time = r.float()?;
            let mut mix = mix_of(r, flags)?;
            let mut soft = if flags & 4 != 0 { r.float()? } else { 0.0 };
            for frame in 0..frames {
                keys.push(time, &[mix, soft, if flags & 8 != 0 { 1.0 } else { -1.0 }, (flags & 16 != 0) as u8 as f32, (flags & 32 != 0) as u8 as f32, 0.0]);
                if frame + 1 == frames {
                    break;
                }
                let nf = r.byte()?;
                let t2 = r.float()?;
                let m2 = mix_of(r, nf)?;
                let s2 = if nf & 4 != 0 { r.float()? } else { 0.0 };
                if nf & 64 != 0 {
                    for c in 0..6 {
                        keys.set_raw(frame, c, Raw::Stepped);
                    }
                } else if nf & 128 != 0 {
                    for c in 0..2 {
                        keys.set_raw(frame, c, Raw::Bez([r.float()?, r.float()?, r.float()?, r.float()?]));
                    }
                }
                flags = nf;
                time = t2;
                mix = m2;
                soft = s2;
            }
        }
        let t = keys.track(true);
        dur = dur.max(t.duration());
        anim.ik.push((ci, t));
    }

    // Transform constraints.
    let n = r.count(2)?;
    for _ in 0..n {
        let u = r.index(unified.len(), "animation transform")?;
        let ci = cref(unified, u, CKind::Transform, "transform")?;
        let frames = r.count(4)?;
        let _bezier = r.var()?;
        let t = read_keys4(r, frames, 6, &|_| false, &mut |r| Ok(vec![r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?]))?.track(true);
        dur = dur.max(t.duration());
        anim.transform.push((ci, t));
    }

    // Path constraints.
    let n = r.count(2)?;
    for _ in 0..n {
        let u = r.index(unified.len(), "animation path")?;
        let ci = cref(unified, u, CKind::Path, "path")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            let _bezier = r.var()?;
            let tl = match ty {
                0 => PathTl::Position(read_keys4(r, frames, 1, &|_| false, &mut |r| Ok(vec![r.float()?]))?.track(true)),
                1 => PathTl::Spacing(read_keys4(r, frames, 1, &|_| false, &mut |r| Ok(vec![r.float()?]))?.track(true)),
                2 => PathTl::Mix(read_keys4(r, frames, 3, &|_| false, &mut |r| Ok(vec![r.float()?, r.float()?, r.float()?]))?.track(true)),
                t => return Err(format!("skel: unknown path timeline {t}")),
            };
            let (PathTl::Position(t) | PathTl::Spacing(t) | PathTl::Mix(t)) = &tl;
            dur = dur.max(t.duration());
            anim.path.push((ci, tl));
        }
    }

    // Physics constraints (a constraint number of 0 is "all that follow the skeleton-wide value").
    let n = r.count(2)?;
    for _ in 0..n {
        let u = r.var()?;
        let ci = if u == 0 { None } else { Some(cref(unified, u - 1, CKind::Physics, "physics")?) };
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            if ty == 8 {
                let mut times = Vec::with_capacity(frames);
                for _ in 0..frames {
                    times.push(r.float()?);
                }
                if let Some(&l) = times.last() {
                    dur = dur.max(l);
                }
                anim.physics.push((ci, PhysTl::Reset(times)));
                continue;
            }
            let _bezier = r.var()?;
            let prop = match ty {
                0 => PhysProp::Inertia,
                1 => PhysProp::Strength,
                2 => PhysProp::Damping,
                4 => PhysProp::Mass,
                5 => PhysProp::Wind,
                6 => PhysProp::Gravity,
                7 => PhysProp::Mix,
                t => return Err(format!("skel: unknown physics timeline {t}")),
            };
            let t = read_keys4(r, frames, 1, &|_| false, &mut |r| Ok(vec![r.float()?]))?.track(true);
            dur = dur.max(t.duration());
            anim.physics.push((ci, PhysTl::Prop(prop, t)));
        }
    }

    // Sliders.
    let n = r.count(2)?;
    for _ in 0..n {
        let u = r.index(unified.len(), "animation slider")?;
        let ci = cref(unified, u, CKind::Slider, "slider")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            let _bezier = r.var()?;
            let t = read_keys4(r, frames, 1, &|_| false, &mut |r| Ok(vec![r.float()?]))?.track(true);
            dur = dur.max(t.duration());
            anim.slider.push((ci, if ty == 0 { SliderTl::Time(t) } else if ty == 1 { SliderTl::Mix(t) } else { return Err(format!("skel: unknown slider timeline {ty}")) }));
        }
    }

    // Attachment timelines: deform and image sequences.
    let n = r.count(2)?;
    for _ in 0..n {
        let si = r.index(skins.len(), "deform skin")?;
        let ns = r.count(2)?;
        for _ in 0..ns {
            let slot = r.index(slots.len(), "deform slot")?;
            let na = r.count(2)?;
            for _ in 0..na {
                let att_name = r.sref()?.unwrap_or_default();
                let kind = r.byte()?;
                let frames = r.count(4)?;
                if kind == 1 {
                    // An image sequence over time: time, mode and index, delay per key.
                    let mut keys = Vec::with_capacity(frames);
                    for _ in 0..frames {
                        let time = r.float()?;
                        let mi = r.int()?;
                        keys.push(SeqKey { time, mode: (mi & 0xF) as u8, index: mi >> 4, delay: r.float()? });
                    }
                    if let Some(l) = keys.last() {
                        dur = dur.max(l.time);
                    }
                    anim.sequence.push(SeqTl { skin: si, slot, attachment: att_name, keys });
                    continue;
                }
                if kind != 0 {
                    return Err(format!("skel: unknown attachment timeline type {kind}"));
                }
                let _bezier = r.var()?;
                let mesh_len = skins.get(si).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(&att_name)).and_then(|a| a.deform_len());
                // (A bounding box can be deformed too: its keys are read and dropped.)
                let len = mesh_len.unwrap_or(usize::MAX);
                let mut tl = DeformTl { skin: si, slot, attachment: att_name, times: Vec::new(), frames: Vec::new(), curves: Vec::new() };
                let mut time = r.float()?;
                for frame in 0..frames {
                    let end = r.var()?;
                    let mut full = vec![0.0f32; if len == usize::MAX { 0 } else { len }];
                    if end > 0 {
                        let start = r.var()?;
                        if len == usize::MAX {
                            r.floats(end)?;
                        } else {
                            if start + end > len {
                                return Err("skel: a deform key reaches past its mesh".into());
                            }
                            for v in full.iter_mut().skip(start).take(end) {
                                *v = r.float()?;
                            }
                        }
                    }
                    tl.times.push(time);
                    tl.frames.push(full);
                    tl.curves.push(Curve::Linear);
                    if frame + 1 < frames {
                        let t2 = r.float()?;
                        let curve = match r.byte()? {
                            0 => Curve::Linear,
                            1 => Curve::Stepped,
                            2 => {
                                let c = [r.float()?, r.float()?, r.float()?, r.float()?];
                                let dt = (t2 - time).max(1e-6);
                                Curve::Bezier([(c[0] - time) / dt, c[1], (c[2] - time) / dt, c[3]])
                            }
                            t => return Err(format!("skel: unknown curve type {t}")),
                        };
                        tl.curves[frame] = curve;
                        time = t2;
                    }
                }
                if let Some(&t) = tl.times.last() {
                    dur = dur.max(t);
                }
                if len != usize::MAX {
                    anim.deform.push(tl);
                }
            }
        }
    }

    // Draw order, and the draw order of folders of slots.
    let n = r.count(2)?;
    for _ in 0..n {
        let t = r.float()?;
        let no = r.count(2)?;
        let mut offsets = Vec::with_capacity(no);
        for _ in 0..no {
            let slot = r.index(slots.len(), "draw order slot")?;
            offsets.push((slot, r.var()? as u32 as i32));
        }
        dur = dur.max(t);
        anim.draw_order.push((t, offsets));
    }
    let folders = r.count(2)?;
    for _ in 0..folders {
        let fs = r.count(1)?;
        for _ in 0..fs {
            r.var()?;
        }
        let keys = r.count(2)?;
        for _ in 0..keys {
            r.float()?;
            let changes = r.count(2)?;
            for _ in 0..changes {
                r.var()?;
                r.var()?;
            }
        }
    }

    // Events (only read to stay in step).
    let n = r.count(4)?;
    for _ in 0..n {
        let t = r.float()?;
        let e = r.index(event_audio.len().max(1), "event")?;
        r.zig()?;
        r.float()?;
        r.string()?;
        if event_audio.get(e).copied().unwrap_or(false) {
            r.float()?;
            r.float()?;
        }
        dur = dur.max(t);
    }
    if nonessential {
        r.int()?; // the animation's colour in the editor
    }
    anim.duration = dur;
    Ok(anim)
}
