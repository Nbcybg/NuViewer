//! Spine's JSON skeleton (3.8, 4.0, 4.1, 4.2). The versions differ in small ways this reader absorbs:
//!  * bone `transform` (<= 4.1) / `inherit` (4.2); constraint mixes `rotateMix` ... (<= 4.0) / `mixRotate` ... (>= 4.1);
//!  * curves: 3.x gives each key one curve as fractions of the segment, 4.x one per channel in absolute time / value;
//!  * rotation timelines: 3.x takes the short way round between two angles, 4.x does not; 4.2 splits translate / scale / shear in x and y;
//!  * slot colours: `color` / `twoColor` (3.x) and `rgba` / `rgb` / `alpha` / `rgba2` / `rgb2` (4.x).
//!
//! Path, physics (4.2) and, in 4.3, slider constraints and the new transform constraint are read; events, 4.3 bone inherit timelines and draw order folders are skipped.

use crate::atlas::Atlas;
use crate::model::*;
use serde_json::Value;
use std::collections::HashMap;

fn num(v: &Value, key: &str, d: f32) -> f32 {
    v.get(key).and_then(|x| x.as_f64()).map(|x| x as f32).unwrap_or(d)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

fn flag(v: &Value, key: &str, d: bool) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(d)
}

/// "rrggbbaa" or "rrggbb" as 0..1 floats (alpha 1 when missing).
pub fn hex_rgba(s: &str) -> [f32; 4] {
    let b = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).map(|v| v as f32 / 255.0);
    [b(0).unwrap_or(1.0), b(2).unwrap_or(1.0), b(4).unwrap_or(1.0), if s.len() >= 8 { b(6).unwrap_or(1.0) } else { 1.0 }]
}

fn color(v: &Value, key: &str, d: [f32; 4]) -> [f32; 4] {
    text(v, key).map(hex_rgba).unwrap_or(d)
}

fn floats(v: Option<&Value>) -> Vec<f32> {
    v.and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default()
}

// ---- curves ---------------------------------------------------------------------------------------------------------------------

enum Raw {
    Linear,
    Stepped,
    /// 4 numbers per channel, or 4 for every channel.
    Bezier(Vec<f32>),
}

fn raw_curve(o: &Value, channels: usize) -> Raw {
    match o.get("curve") {
        Some(Value::String(s)) if s == "stepped" => Raw::Stepped,
        Some(Value::Number(n)) => {
            let mut c = vec![n.as_f64().unwrap_or(0.0) as f32, num(o, "c2", 0.0), num(o, "c3", 1.0), num(o, "c4", 1.0)];
            // 4.x: the next channels continue as c5..c8, c9..c12 ...
            for ch in 1..channels {
                let base = 4 * ch + 1;
                let key = |k: usize| format!("c{}", base + k);
                if o.get(key(0)).is_some() {
                    c.extend([num(o, &key(0), 0.0), num(o, &key(1), 0.0), num(o, &key(2), 1.0), num(o, &key(3), 1.0)]);
                } else {
                    break;
                }
            }
            Raw::Bezier(c)
        }
        Some(Value::Array(_)) => {
            let c = floats(o.get("curve"));
            if c.len() >= 4 {
                Raw::Bezier(c)
            } else {
                Raw::Linear
            }
        }
        _ => Raw::Linear,
    }
}

/// Curves of one key for every channel, with 4.x's absolute control points turned into fractions of the segment.
fn finish_curves(raw: &Raw, channels: usize, absolute: bool, t0: f32, t1: f32, v0: &[f32], v1: &[f32]) -> Vec<Curve> {
    match raw {
        Raw::Linear => vec![Curve::Linear; channels],
        Raw::Stepped => vec![Curve::Stepped; channels],
        Raw::Bezier(c) => (0..channels)
            .map(|ch| {
                let p = if c.len() >= 4 * (ch + 1) { &c[4 * ch..4 * ch + 4] } else { &c[0..4] };
                if !absolute {
                    return Curve::Bezier([p[0], p[1], p[2], p[3]]);
                }
                let _ = (v0, v1);
                if t1 - t0 <= 1e-9 {
                    return Curve::Linear;
                }
                Curve::BezierAbs([p[0], p[1], p[2], p[3]])
            })
            .collect(),
    }
}

/// Build a track from keys given as (time, values, raw curve).
fn track(stride: usize, keys: Vec<(f32, Vec<f32>, Raw)>, absolute: bool) -> Track {
    let mut t = Track::new(stride);
    for i in 0..keys.len() {
        let (time, vals, raw) = (&keys[i].0, &keys[i].1, &keys[i].2);
        let curves = if i + 1 < keys.len() { finish_curves(raw, stride, absolute, *time, keys[i + 1].0, vals, &keys[i + 1].1) } else { vec![Curve::Linear; stride] };
        t.push(*time, vals, &curves);
    }
    t
}

/// Keys of a timeline array: `read` turns a frame object into its values.
fn keys_of(arr: &Value, channels: usize, read: impl Fn(&Value) -> Vec<f32>) -> Vec<(f32, Vec<f32>, Raw)> {
    let mut out: Vec<(f32, Vec<f32>, Raw)> = arr.as_array().map(|a| a.iter().map(|f| (num(f, "time", 0.0), read(f), raw_curve(f, channels))).collect()).unwrap_or_default();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

// ---- the reader -----------------------------------------------------------------------------------------------------------------

/// Parse a skeleton JSON. The atlas is attached as given (its pages' images are filled in by the caller).
pub fn parse(src: &str, atlas: Atlas) -> Result<Rig, String> {
    let root: Value = serde_json::from_str(src).map_err(|e| format!("spine json: {e}"))?;
    let head = root.get("skeleton").ok_or("spine json: no \"skeleton\" header (this is not a Spine skeleton)")?;
    let version = text(head, "spine").unwrap_or("").to_string();
    let major: u32 = version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(4);
    let legacy = major < 4;
    let absolute = !legacy;
    let unsupported: Vec<String> = Vec::new();

    // Bones.
    let mut bones: Vec<BoneData> = Vec::new();
    let mut bone_ids: HashMap<String, usize> = HashMap::new();
    for b in root.get("bones").and_then(|b| b.as_array()).ok_or("spine json: no bones")? {
        let name = text(b, "name").ok_or("spine json: a bone has no name")?.to_string();
        let parent = match text(b, "parent") {
            Some(p) => Some(*bone_ids.get(p).ok_or_else(|| format!("spine json: bone {name} has an unknown parent {p}"))?),
            None => None,
        };
        let inherit = text(b, "inherit").or_else(|| text(b, "transform")).map(Inherit::from_name).unwrap_or(Inherit::Normal);
        bone_ids.insert(name.clone(), bones.len());
        bones.push(BoneData { name, parent, length: num(b, "length", 0.0), x: num(b, "x", 0.0), y: num(b, "y", 0.0), rotation: num(b, "rotation", 0.0), scale_x: num(b, "scaleX", 1.0), scale_y: num(b, "scaleY", 1.0), shear_x: num(b, "shearX", 0.0), shear_y: num(b, "shearY", 0.0), inherit, skin_required: flag(b, "skin", false) });
    }
    if bones.is_empty() {
        return Err("spine json: the skeleton has no bones".into());
    }

    // Slots.
    let mut slots: Vec<SlotData> = Vec::new();
    let mut slot_ids: HashMap<String, usize> = HashMap::new();
    for s in root.get("slots").and_then(|s| s.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
        let name = text(s, "name").ok_or("spine json: a slot has no name")?.to_string();
        let bone = *bone_ids.get(text(s, "bone").unwrap_or("")).ok_or_else(|| format!("spine json: slot {name} has an unknown bone"))?;
        slot_ids.insert(name.clone(), slots.len());
        slots.push(SlotData { name, bone, color: color(s, "color", [1.0; 4]), dark: text(s, "dark").map(|d| {
            let c = hex_rgba(d);
            [c[0], c[1], c[2]]
        }), attachment: text(s, "attachment").map(String::from), blend: text(s, "blend").map(BlendMode::from_name).unwrap_or(BlendMode::Normal) });
    }

    // Constraints: four lists up to 4.2 (each with its own `order`), one list from 4.3 (the order of the list is the order of work).
    let mut ik: Vec<IkData> = Vec::new();
    let mut ik_ids: HashMap<String, usize> = HashMap::new();
    let mut transform: Vec<TransformData> = Vec::new();
    let mut tf_ids: HashMap<String, usize> = HashMap::new();
    let mut paths: Vec<PathData> = Vec::new();
    let mut path_ids: HashMap<String, usize> = HashMap::new();
    let mut physics: Vec<PhysicsData> = Vec::new();
    let mut phys_ids: HashMap<String, usize> = HashMap::new();
    let mut sliders: Vec<SliderData> = Vec::new();
    let mut slider_ids: HashMap<String, usize> = HashMap::new();
    let mut slider_anims: Vec<Option<String>> = Vec::new();
    let mut unified: Vec<CRef> = Vec::new();
    let mut order_guess = 0;
    let arr = |key: &str| root.get(key).and_then(|c| c.as_array()).map(|a| a.as_slice()).unwrap_or(&[]).to_vec();
    let mut add = |c: &Value, ty: &str, order_guess: &mut i32, unified: &mut Vec<CRef>, in_list: bool| {
        let order = c.get("order").and_then(|o| o.as_i64()).map(|o| o as i32).unwrap_or(*order_guess);
        let name = text(c, "name").unwrap_or("").to_string();
        match ty {
            "ik" => {
                if let Some(d) = read_ik(c, &bone_ids, order) {
                    ik_ids.insert(name, ik.len());
                    if in_list {
                        unified.push(CRef { kind: CKind::Ik, index: ik.len() });
                    }
                    ik.push(d);
                    *order_guess += 1;
                }
            }
            "transform" => {
                if let Some(d) = read_transform(c, &bone_ids, order) {
                    tf_ids.insert(name, transform.len());
                    if in_list {
                        unified.push(CRef { kind: CKind::Transform, index: transform.len() });
                    }
                    transform.push(d);
                    *order_guess += 1;
                }
            }
            "path" => {
                if let Some(d) = read_path_constraint(c, &bone_ids, &slot_ids, order) {
                    path_ids.insert(name, paths.len());
                    if in_list {
                        unified.push(CRef { kind: CKind::Path, index: paths.len() });
                    }
                    paths.push(d);
                    *order_guess += 1;
                }
            }
            "physics" => {
                if let Some(d) = read_physics(c, &bone_ids, order) {
                    phys_ids.insert(name, physics.len());
                    if in_list {
                        unified.push(CRef { kind: CKind::Physics, index: physics.len() });
                    }
                    physics.push(d);
                    *order_guess += 1;
                }
            }
            "slider" => {
                let (d, anim) = read_slider(c, &bone_ids, order);
                slider_ids.insert(name, sliders.len());
                if in_list {
                    unified.push(CRef { kind: CKind::Slider, index: sliders.len() });
                }
                sliders.push(d);
                slider_anims.push(anim);
                *order_guess += 1;
            }
            _ => {}
        }
    };
    if let Some(Value::Array(list)) = root.get("constraints") {
        for c in list {
            let ty = text(c, "type").unwrap_or("").to_string();
            add(c, &ty, &mut order_guess, &mut unified, true);
        }
    } else {
        for ty in ["ik", "transform", "path", "physics"] {
            for c in arr(ty) {
                add(&c, ty, &mut order_guess, &mut unified, false);
            }
        }
    }

    // Skins: an array (3.6 and later) or an object (older).
    let mut skins: Vec<Skin> = Vec::new();
    struct Linked {
        skin: usize,
        slot: usize,
        key: String,
        parent: String,
        parent_skin: String,
        parent_slot: usize,
        deform: bool,
        path: String,
        color: [f32; 4],
        width: f32,
        height: f32,
        seq: Option<Seq>,
    }
    let mut linked: Vec<Linked> = Vec::new();
    let skin_values: Vec<(String, &Value)> = match root.get("skins") {
        Some(Value::Array(a)) => a.iter().map(|s| (text(s, "name").unwrap_or("default").to_string(), s)).collect(),
        Some(Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    };
    for (si, (name, sv)) in skin_values.iter().enumerate() {
        let mut skin = Skin { name: name.clone(), slots: vec![HashMap::new(); slots.len()], ..Default::default() };
        let by_names = |key: &str, ids: &HashMap<String, usize>| -> Vec<usize> { sv.get(key).and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).filter_map(|n| ids.get(n).copied()).collect()).unwrap_or_default() };
        skin.bones = by_names("bones", &bone_ids);
        skin.ik = by_names("ik", &ik_ids);
        skin.transform = by_names("transform", &tf_ids);
        for (key, kind, ids) in [("path", CKind::Path, &path_ids), ("physics", CKind::Physics, &phys_ids), ("slider", CKind::Slider, &slider_ids)] {
            skin.extra.extend(by_names(key, ids).into_iter().map(|index| CRef { kind, index }));
        }
        let atts = if sv.get("attachments").is_some() { sv.get("attachments") } else { Some(*sv) };
        if let Some(Value::Object(per_slot)) = atts {
            for (slot_name, entries) in per_slot {
                let Some(&slot) = slot_ids.get(slot_name) else { continue };
                let Value::Object(entries) = entries else { continue };
                for (key, a) in entries {
                    let ty = text(a, "type").unwrap_or("region");
                    let path = text(a, "path").or_else(|| text(a, "name")).unwrap_or(key).to_string();
                    // 4.3 writes a linked mesh as a mesh with a `source` (and `timelines` for what used to be `deform`).
                    let ty = if ty == "mesh" && a.get("source").is_some() { "linkedmesh" } else { ty };
                    let att = match ty {
                        "region" => Attachment::Region(Region { path, seq: read_seq(a), x: num(a, "x", 0.0), y: num(a, "y", 0.0), rotation: num(a, "rotation", 0.0), scale_x: num(a, "scaleX", 1.0), scale_y: num(a, "scaleY", 1.0), width: num(a, "width", 32.0), height: num(a, "height", 32.0), color: color(a, "color", [1.0; 4]) }),
                        "mesh" => {
                            let uvs = floats(a.get("uvs"));
                            let verts = floats(a.get("vertices"));
                            let vertices = if verts.len() == uvs.len() { Vertices::Plain(verts) } else { weighted(&verts) };
                            let tris: Vec<u16> = floats(a.get("triangles")).iter().map(|&t| t as u16).collect();
                            Attachment::Mesh(Mesh { path, seq: read_seq(a), color: color(a, "color", [1.0; 4]), width: num(a, "width", 0.0), height: num(a, "height", 0.0), uvs, triangles: tris, vertices, hull: num(a, "hull", 0.0) as usize, deform_skin: si, deform_name: key.clone() })
                        }
                        "linkedmesh" | "weightedlinkedmesh" => {
                            let parent_slot = text(a, "slot").and_then(|n| slot_ids.get(n).copied()).unwrap_or(slot);
                            linked.push(Linked { skin: si, slot, key: key.clone(), parent: text(a, "parent").or_else(|| text(a, "source")).unwrap_or("").to_string(), parent_skin: text(a, "skin").unwrap_or("default").to_string(), parent_slot, deform: flag(a, "deform", flag(a, "timelines", true)), path, color: color(a, "color", [1.0; 4]), width: num(a, "width", 0.0), height: num(a, "height", 0.0), seq: read_seq(a) });
                            continue;
                        }
                        "clipping" => {
                            let verts = floats(a.get("vertices"));
                            let count = num(a, "vertexCount", 0.0) as usize;
                            let vertices = if verts.len() == count * 2 { Vertices::Plain(verts) } else { weighted(&verts) };
                            Attachment::Clip(Clip { end_slot: text(a, "end").and_then(|e| slot_ids.get(e).copied()).unwrap_or(slot), vertices, vertex_count: count })
                        }
                        "path" => {
                            let count = num(a, "vertexCount", 0.0) as usize;
                            let verts = floats(a.get("vertices"));
                            let vertices = if verts.len() == count * 2 { Vertices::Plain(verts) } else { weighted(&verts) };
                            Attachment::Path(PathAtt { closed: flag(a, "closed", false), constant_speed: flag(a, "constantSpeed", true), vertices, lengths: floats(a.get("lengths")) })
                        }
                        _ => Attachment::Other,
                    };
                    skin.slots[slot].insert(key.clone(), att);
                }
            }
        }
        skins.push(skin);
    }
    if skins.is_empty() {
        skins.push(Skin { name: "default".into(), slots: vec![HashMap::new(); slots.len()], ..Default::default() });
    }
    // Linked meshes take everything but the picture and colour from their parent (which may itself be linked: go until nothing changes).
    let mut pending = linked;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for l in pending {
            let psi = skins.iter().position(|s| s.name == l.parent_skin).unwrap_or(0);
            let parent = skins.get(psi).and_then(|s| s.slots.get(l.parent_slot)).and_then(|m| m.get(&l.parent)).cloned();
            match parent {
                Some(Attachment::Mesh(p)) => {
                    let (deform_skin, deform_name) = if l.deform { (p.deform_skin, p.deform_name.clone()) } else { (l.skin, l.key.clone()) };
                    skins[l.skin].slots[l.slot].insert(l.key.clone(), Attachment::Mesh(Mesh { path: l.path.clone(), seq: l.seq, color: l.color, width: if l.width > 0.0 { l.width } else { p.width }, height: if l.height > 0.0 { l.height } else { p.height }, deform_skin, deform_name, ..p }));
                }
                _ => rest.push(l),
            }
        }
        if rest.len() == before {
            break; // a parent that is not there: those meshes are simply absent
        }
        pending = rest;
    }

    // Animations.
    let mut animations: Vec<Animation> = Vec::new();
    if let Some(Value::Object(anims)) = root.get("animations") {
        for (name, a) in anims {
            animations.push(read_animation(name, a, absolute, &bone_ids, &slot_ids, &ik_ids, &tf_ids, &path_ids, &phys_ids, &slider_ids, &skins, &slots, &ik));
        }
    }
    animations.sort_by(|a, b| a.name.cmp(&b.name));
    for (sl, name) in sliders.iter_mut().zip(&slider_anims) {
        sl.animation = name.as_deref().and_then(|n| animations.iter().position(|a| a.name == n));
    }
    let reference_scale = num(head, "referenceScale", 100.0);

    Ok(Rig { version, name: String::new(), x: num(head, "x", 0.0), y: num(head, "y", 0.0), width: num(head, "width", 0.0), height: num(head, "height", 0.0), fps: num(head, "fps", 30.0), bones, slots, skins, ik, transform, path: paths, physics, slider: sliders, constraints: unified, reference_scale, animations, atlas, legacy, unsupported })
}

fn bone_list(c: &Value, key: &str, ids: &HashMap<String, usize>) -> Vec<usize> {
    c.get(key).and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).filter_map(|n| ids.get(n).copied()).collect()).unwrap_or_default()
}

fn read_ik(c: &Value, bone_ids: &HashMap<String, usize>, order: i32) -> Option<IkData> {
    let bones_of = bone_list(c, "bones", bone_ids);
    let target = bone_ids.get(text(c, "target").unwrap_or("")).copied()?;
    if bones_of.is_empty() || bones_of.len() > 2 {
        return None;
    }
    // 4.3 names how the other axis follows ("uniform", "volume"); before, a flag.
    let uniform = flag(c, "uniform", false) || text(c, "scaleY").map(|m| ScaleYMode::from_name(m) != ScaleYMode::None).unwrap_or(false);
    Some(IkData { name: text(c, "name").unwrap_or("").to_string(), order, bones: bones_of, target, mix: num(c, "mix", 1.0), softness: num(c, "softness", 0.0), bend_positive: flag(c, "bendPositive", true), compress: flag(c, "compress", false), stretch: flag(c, "stretch", false), uniform, skin_required: flag(c, "skin", false) })
}

fn read_transform(c: &Value, bone_ids: &HashMap<String, usize>, order: i32) -> Option<TransformData> {
    let name = text(c, "name").unwrap_or("").to_string();
    let mut bones_of = bone_list(c, "bones", bone_ids);
    if bones_of.is_empty() {
        if let Some(b) = text(c, "bone").and_then(|n| bone_ids.get(n)) {
            bones_of.push(*b); // Spine 3.5 and older wrote a single bone
        }
    }
    if let Some(src) = text(c, "source") {
        // 4.3: a list of "this property of the source drives that property of the bones".
        let target = *bone_ids.get(src)?;
        let mut props: Vec<FromProp> = Vec::new();
        let (mut rotate, mut x, mut y, mut sx, mut sy, mut sh) = (false, false, false, false, false, false);
        if let Some(Value::Object(from_map)) = c.get("properties") {
            for (from_name, entry) in from_map {
                let Some(prop) = Prop::from_name(from_name) else { continue };
                let mut from = FromProp { prop, offset: num(entry, "offset", 0.0), to: Vec::new() };
                if let Some(Value::Object(to_map)) = entry.get("to") {
                    for (to_name, te) in to_map {
                        let Some(to) = Prop::from_name(to_name) else { continue };
                        match to {
                            Prop::Rotate => rotate = true,
                            Prop::X => x = true,
                            Prop::Y => y = true,
                            Prop::ScaleX => sx = true,
                            Prop::ScaleY => sy = true,
                            Prop::ShearY => sh = true,
                        }
                        from.to.push(ToProp { prop: to, offset: num(te, "offset", 0.0), max: num(te, "max", 1.0), scale: num(te, "scale", 1.0) });
                    }
                }
                if !from.to.is_empty() {
                    props.push(from);
                }
            }
        }
        let mix_x = if x { num(c, "mixX", 1.0) } else { 0.0 };
        let mix_scale_x = if sx { num(c, "mixScaleX", 1.0) } else { 0.0 };
        return Some(TransformData {
            name,
            order,
            bones: bones_of,
            target,
            rotation: num(c, "rotation", 0.0),
            x: num(c, "x", 0.0),
            y: num(c, "y", 0.0),
            scale_x: num(c, "scaleX", 0.0),
            scale_y: num(c, "scaleY", 0.0),
            shear_y: num(c, "shearY", 0.0),
            mix_rotate: if rotate { num(c, "mixRotate", 1.0) } else { 0.0 },
            mix_x,
            mix_y: if y { num(c, "mixY", mix_x) } else { 0.0 },
            mix_scale_x,
            mix_scale_y: if sy { num(c, "mixScaleY", mix_scale_x) } else { 0.0 },
            mix_shear_y: if sh { num(c, "mixShearY", 1.0) } else { 0.0 },
            local: false,
            relative: false,
            skin_required: flag(c, "skin", false),
            v43: Some(Transform43 { props, local_source: flag(c, "localSource", false), local_target: flag(c, "localTarget", false), additive: flag(c, "additive", false), clamp: flag(c, "clamp", false) }),
        });
    }
    let target = bone_ids.get(text(c, "target").unwrap_or("")).copied()?;
    let first = |a: &str, b: &str, d: f32| if c.get(a).is_some() { num(c, a, d) } else { num(c, b, d) };
    let mix_rotate = first("mixRotate", "rotateMix", 1.0);
    let mix_x = first("mixX", "translateMix", 1.0);
    let mix_scale_x = first("mixScaleX", "scaleMix", 1.0);
    Some(TransformData {
        name,
        order,
        bones: bones_of,
        target,
        rotation: num(c, "rotation", 0.0),
        x: num(c, "x", 0.0),
        y: num(c, "y", 0.0),
        scale_x: num(c, "scaleX", 0.0),
        scale_y: num(c, "scaleY", 0.0),
        shear_y: num(c, "shearY", 0.0),
        mix_rotate,
        mix_x,
        mix_y: if c.get("mixY").is_some() { num(c, "mixY", mix_x) } else { mix_x },
        mix_scale_x,
        mix_scale_y: if c.get("mixScaleY").is_some() { num(c, "mixScaleY", mix_scale_x) } else { mix_scale_x },
        mix_shear_y: first("mixShearY", "shearMix", 1.0),
        local: flag(c, "local", false),
        relative: flag(c, "relative", false),
        skin_required: flag(c, "skin", false),
        v43: None,
    })
}

fn read_path_constraint(c: &Value, bone_ids: &HashMap<String, usize>, slot_ids: &HashMap<String, usize>, order: i32) -> Option<PathData> {
    let slot = *slot_ids.get(text(c, "slot").or_else(|| text(c, "target"))?)?;
    let bones = bone_list(c, "bones", bone_ids);
    if bones.is_empty() {
        return None;
    }
    let mix_x = num(c, "mixX", num(c, "translateMix", 1.0));
    Some(PathData {
        name: text(c, "name").unwrap_or("").to_string(),
        order,
        bones,
        slot,
        position_mode: if text(c, "positionMode").map(|m| m.eq_ignore_ascii_case("fixed")).unwrap_or(false) { PositionMode::Fixed } else { PositionMode::Percent },
        spacing_mode: match text(c, "spacingMode").map(|m| m.to_ascii_lowercase()).as_deref() {
            Some("fixed") => SpacingMode::Fixed,
            Some("percent") => SpacingMode::Percent,
            Some("proportional") => SpacingMode::Proportional,
            _ => SpacingMode::Length,
        },
        rotate_mode: match text(c, "rotateMode").map(|m| m.to_ascii_lowercase()).as_deref() {
            Some("chain") => RotateMode::Chain,
            Some("chainscale") => RotateMode::ChainScale,
            _ => RotateMode::Tangent,
        },
        offset_rotation: num(c, "rotation", 0.0),
        position: num(c, "position", 0.0),
        spacing: num(c, "spacing", 0.0),
        mix_rotate: num(c, "mixRotate", num(c, "rotateMix", 1.0)),
        mix_x,
        mix_y: num(c, "mixY", mix_x),
        skin_required: flag(c, "skin", false),
    })
}

fn read_physics(c: &Value, bone_ids: &HashMap<String, usize>, order: i32) -> Option<PhysicsData> {
    let bone = *bone_ids.get(text(c, "bone")?)?;
    let g = |k: &str| flag(c, k, false);
    Some(PhysicsData {
        name: text(c, "name").unwrap_or("").to_string(),
        order,
        bone,
        x: num(c, "x", 0.0),
        y: num(c, "y", 0.0),
        rotate: num(c, "rotate", 0.0),
        scale_x: num(c, "scaleX", 0.0),
        shear_x: num(c, "shearX", 0.0),
        scale_y_mode: text(c, "scaleY").map(ScaleYMode::from_name).unwrap_or(ScaleYMode::None),
        limit: num(c, "limit", 5000.0),
        step: 1.0 / num(c, "fps", 60.0).max(1.0),
        inertia: num(c, "inertia", 0.5),
        strength: num(c, "strength", 100.0),
        damping: num(c, "damping", 0.85),
        mass_inverse: 1.0 / num(c, "mass", 1.0).max(1e-6),
        wind: num(c, "wind", 0.0),
        gravity: num(c, "gravity", 0.0),
        mix: num(c, "mix", 1.0),
        global: [g("inertiaGlobal"), g("strengthGlobal"), g("dampingGlobal"), g("massGlobal"), g("windGlobal"), g("gravityGlobal"), g("mixGlobal")],
        skin_required: flag(c, "skin", false),
    })
}

/// A slider; the name of its animation is looked up once the animations are read.
fn read_slider(c: &Value, bone_ids: &HashMap<String, usize>, order: i32) -> (SliderData, Option<String>) {
    let bone = text(c, "bone").and_then(|n| bone_ids.get(n)).copied();
    let d = SliderData {
        name: text(c, "name").unwrap_or("").to_string(),
        order,
        animation: None,
        additive: flag(c, "additive", false),
        looped: flag(c, "loop", false),
        bone,
        property: text(c, "property").and_then(Prop::from_name).unwrap_or(Prop::Rotate),
        from: num(c, "from", 0.0),
        offset: num(c, "to", 0.0),
        scale: num(c, "scale", 1.0),
        local: flag(c, "local", false),
        time: num(c, "time", 0.0),
        mix: num(c, "mix", 1.0),
        skin_required: flag(c, "skin", false),
    };
    (d, text(c, "animation").map(String::from))
}

/// The `sequence` of an attachment (4.1 and later): which numbered pictures it can show.
fn read_seq(a: &Value) -> Option<Seq> {
    let s = a.get("sequence")?;
    Some(Seq { count: num(s, "count", 0.0) as usize, start: num(s, "start", 1.0) as i32, digits: num(s, "digits", 0.0) as usize, setup: num(s, "setup", 0.0) as usize })
}

pub(crate) fn weighted(v: &[f32]) -> Vertices {
    let mut out = Vec::new();
    let mut i = 0;
    while i < v.len() {
        let n = v[i] as usize;
        i += 1;
        let mut inf = Vec::with_capacity(n);
        for _ in 0..n {
            if i + 4 > v.len() {
                break;
            }
            inf.push(Influence { bone: v[i] as usize, x: v[i + 1], y: v[i + 2], weight: v[i + 3] });
            i += 4;
        }
        out.push(inf);
    }
    Vertices::Weighted(out)
}

#[allow(clippy::too_many_arguments)]
fn read_animation(name: &str, a: &Value, absolute: bool, bone_ids: &HashMap<String, usize>, slot_ids: &HashMap<String, usize>, ik_ids: &HashMap<String, usize>, tf_ids: &HashMap<String, usize>, path_ids: &HashMap<String, usize>, phys_ids: &HashMap<String, usize>, slider_ids: &HashMap<String, usize>, skins: &[Skin], slots: &[SlotData], _ik: &[IkData]) -> Animation {
    let mut anim = Animation { name: name.to_string(), ..Default::default() };
    let mut dur = 0.0f32;
    let mut note = |t: f32| dur = dur.max(t);

    if let Some(Value::Object(per_bone)) = a.get("bones") {
        for (bn, tls) in per_bone {
            let Some(&bi) = bone_ids.get(bn) else { continue };
            let Value::Object(tls) = tls else { continue };
            for (kind, arr) in tls {
                let one = |d: f32| move |f: &Value| vec![if f.get("value").is_some() { num(f, "value", d) } else if f.get("angle").is_some() { num(f, "angle", d) } else { d }];
                let two = |d: f32| move |f: &Value| vec![num(f, "x", d), num(f, "y", d)];
                let tl = match kind.as_str() {
                    "rotate" => BoneTl::Rotate(track(1, keys_of(arr, 1, one(0.0)), absolute)),
                    "translate" => BoneTl::Translate(track(2, keys_of(arr, 2, two(0.0)), absolute)),
                    "translatex" => BoneTl::TranslateX(track(1, keys_of(arr, 1, one(0.0)), absolute)),
                    "translatey" => BoneTl::TranslateY(track(1, keys_of(arr, 1, one(0.0)), absolute)),
                    "scale" => BoneTl::Scale(track(2, keys_of(arr, 2, two(1.0)), absolute)),
                    "scalex" => BoneTl::ScaleX(track(1, keys_of(arr, 1, one(1.0)), absolute)),
                    "scaley" => BoneTl::ScaleY(track(1, keys_of(arr, 1, one(1.0)), absolute)),
                    "shear" => BoneTl::Shear(track(2, keys_of(arr, 2, two(0.0)), absolute)),
                    "shearx" => BoneTl::ShearX(track(1, keys_of(arr, 1, one(0.0)), absolute)),
                    "sheary" => BoneTl::ShearY(track(1, keys_of(arr, 1, one(0.0)), absolute)),
                    _ => continue,
                };
                let t = match &tl {
                    BoneTl::Rotate(t) | BoneTl::Translate(t) | BoneTl::TranslateX(t) | BoneTl::TranslateY(t) | BoneTl::Scale(t) | BoneTl::ScaleX(t) | BoneTl::ScaleY(t) | BoneTl::Shear(t) | BoneTl::ShearX(t) | BoneTl::ShearY(t) => t.duration(),
                };
                note(t);
                anim.bones.push((bi, tl));
            }
        }
    }

    if let Some(Value::Object(per_slot)) = a.get("slots") {
        for (sn, tls) in per_slot {
            let Some(&si) = slot_ids.get(sn) else { continue };
            let Value::Object(tls) = tls else { continue };
            for (kind, arr) in tls {
                let rgba = |f: &Value, key: &str| color(f, key, [1.0; 4]).to_vec();
                let tl = match kind.as_str() {
                    "attachment" => {
                        let mut v: Vec<(f32, Option<String>)> = arr.as_array().map(|a| a.iter().map(|f| (num(f, "time", 0.0), text(f, "name").map(String::from))).collect()).unwrap_or_default();
                        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                        if let Some(l) = v.last() {
                            note(l.0);
                        }
                        anim.slots.push((si, SlotTl::Attachment(v)));
                        continue;
                    }
                    "color" | "rgba" => SlotTl::Color(track(4, keys_of(arr, 4, |f| rgba(f, "color")), absolute)),
                    "rgb" => SlotTl::Rgb(track(3, keys_of(arr, 3, |f| rgba(f, "color")[..3].to_vec()), absolute)),
                    "alpha" => SlotTl::Alpha(track(1, keys_of(arr, 1, |f| vec![num(f, "value", 0.0)]), absolute)),
                    "twoColor" | "rgba2" => SlotTl::TwoColor(track(7, keys_of(arr, 7, |f| {
                        let mut v = rgba(f, "light");
                        v.extend_from_slice(&color(f, "dark", [0.0, 0.0, 0.0, 1.0])[..3]);
                        v
                    }), absolute)),
                    "rgb2" => SlotTl::Rgb2(track(6, keys_of(arr, 6, |f| {
                        let mut v = rgba(f, "light")[..3].to_vec();
                        v.extend_from_slice(&color(f, "dark", [0.0, 0.0, 0.0, 1.0])[..3]);
                        v
                    }), absolute)),
                    _ => continue,
                };
                let t = match &tl {
                    SlotTl::Color(t) | SlotTl::Rgb(t) | SlotTl::Alpha(t) | SlotTl::TwoColor(t) | SlotTl::Rgb2(t) => t.duration(),
                    SlotTl::Attachment(_) => 0.0,
                };
                note(t);
                anim.slots.push((si, tl));
            }
        }
    }

    if let Some(Value::Object(m)) = a.get("ik") {
        for (n, arr) in m {
            let Some(&ci) = ik_ids.get(n) else { continue };
            let t = track(6, keys_of(arr, 2, |f| vec![num(f, "mix", 1.0), num(f, "softness", 0.0), if flag(f, "bendPositive", true) { 1.0 } else { -1.0 }, flag(f, "compress", false) as u8 as f32, flag(f, "stretch", false) as u8 as f32, 0.0]), absolute);
            note(t.duration());
            anim.ik.push((ci, t));
        }
    }
    if let Some(Value::Object(m)) = a.get("transform") {
        for (n, arr) in m {
            let Some(&ci) = tf_ids.get(n) else { continue };
            let t = track(6, keys_of(arr, 6, |f| {
                let pick = |a: &str, b: &str, d: f32| if f.get(a).is_some() { num(f, a, d) } else { num(f, b, d) };
                let mr = pick("mixRotate", "rotateMix", 1.0);
                let mx = pick("mixX", "translateMix", 1.0);
                let my = if f.get("mixY").is_some() { num(f, "mixY", mx) } else { mx };
                let ms = pick("mixScaleX", "scaleMix", 1.0);
                let msy = if f.get("mixScaleY").is_some() { num(f, "mixScaleY", ms) } else { ms };
                let msh = pick("mixShearY", "shearMix", 1.0);
                vec![mr, mx, my, ms, msy, msh]
            }), absolute);
            note(t.duration());
            anim.transform.push((ci, t));
        }
    }

    // 4.3 / 4.2: path, physics (a "" name is the skeleton-wide one) and, from 4.3, slider timelines.
    if let Some(Value::Object(m)) = a.get("path") {
        for (n, tls) in m {
            let (Some(&ci), Value::Object(tls)) = (path_ids.get(n), tls) else { continue };
            for (kind, arr) in tls {
                let tl = match kind.as_str() {
                    "position" => PathTl::Position(track(1, keys_of(arr, 1, |f| vec![num(f, "value", 0.0)]), absolute)),
                    "spacing" => PathTl::Spacing(track(1, keys_of(arr, 1, |f| vec![num(f, "value", 0.0)]), absolute)),
                    "mix" => PathTl::Mix(track(3, keys_of(arr, 3, |f| {
                        let mx = if f.get("mixX").is_some() { num(f, "mixX", 1.0) } else { num(f, "translateMix", 1.0) };
                        vec![if f.get("mixRotate").is_some() { num(f, "mixRotate", 1.0) } else { num(f, "rotateMix", 1.0) }, mx, num(f, "mixY", mx)]
                    }), absolute)),
                    _ => continue,
                };
                let (PathTl::Position(t) | PathTl::Spacing(t) | PathTl::Mix(t)) = &tl;
                note(t.duration());
                anim.path.push((ci, tl));
            }
        }
    }
    if let Some(Value::Object(m)) = a.get("physics") {
        for (n, tls) in m {
            let ci = if n.is_empty() { None } else if let Some(&c) = phys_ids.get(n) { Some(c) } else { continue };
            let Value::Object(tls) = tls else { continue };
            for (kind, arr) in tls {
                let prop = match kind.as_str() {
                    "inertia" => PhysProp::Inertia,
                    "strength" => PhysProp::Strength,
                    "damping" => PhysProp::Damping,
                    "mass" => PhysProp::Mass,
                    "wind" => PhysProp::Wind,
                    "gravity" => PhysProp::Gravity,
                    "mix" => PhysProp::Mix,
                    "reset" => {
                        let mut times: Vec<f32> = arr.as_array().map(|a| a.iter().map(|f| num(f, "time", 0.0)).collect()).unwrap_or_default();
                        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        if let Some(&l) = times.last() {
                            note(l);
                        }
                        anim.physics.push((ci, PhysTl::Reset(times)));
                        continue;
                    }
                    _ => continue,
                };
                let d = if prop == PhysProp::Mix { 1.0 } else { 0.0 };
                let t = track(1, keys_of(arr, 1, move |f| vec![num(f, "value", d)]), absolute);
                note(t.duration());
                anim.physics.push((ci, PhysTl::Prop(prop, t)));
            }
        }
    }
    if let Some(Value::Object(m)) = a.get("slider") {
        for (n, tls) in m {
            let (Some(&ci), Value::Object(tls)) = (slider_ids.get(n), tls) else { continue };
            for (kind, arr) in tls {
                let t = track(1, keys_of(arr, 1, |f| vec![num(f, "value", 1.0)]), absolute);
                let tl = match kind.as_str() {
                    "time" => SliderTl::Time(t),
                    "mix" => SliderTl::Mix(t),
                    _ => continue,
                };
                let (SliderTl::Time(t) | SliderTl::Mix(t)) = &tl;
                note(t.duration());
                anim.slider.push((ci, tl));
            }
        }
    }

    // Deform: skin -> slot -> attachment -> keys (4.3 files file it as attachments: skin -> slot -> attachment -> { deform: keys }).
    let mut deform_sources: Vec<(&Value, bool)> = Vec::new();
    if let Some(d) = a.get("deform") {
        deform_sources.push((d, false));
    }
    if let Some(d) = a.get("attachments") {
        deform_sources.push((d, true));
    }
    for (deform_root, nested) in deform_sources {
    if let Value::Object(per_skin) = deform_root {
        for (skin_name, per_slot) in per_skin {
            let Some(si) = skins.iter().position(|s| &s.name == skin_name) else { continue };
            let Value::Object(per_slot) = per_slot else { continue };
            for (slot_name, atts) in per_slot {
                let Some(&slot) = slot_ids.get(slot_name) else { continue };
                let Value::Object(atts) = atts else { continue };
                for (att_name, arr_or_map) in atts {
                    if nested {
                        if let Some(Value::Array(keys)) = arr_or_map.get("sequence") {
                            let mut last_delay = 0.0;
                            let mut sk: Vec<SeqKey> = keys
                                .iter()
                                .map(|f| {
                                    let delay = num(f, "delay", last_delay);
                                    last_delay = delay;
                                    SeqKey { time: num(f, "time", 0.0), mode: SeqKey::mode_from_name(text(f, "mode").unwrap_or("hold")), index: num(f, "index", 0.0) as i32, delay }
                                })
                                .collect();
                            sk.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
                            if let Some(l) = sk.last() {
                                note(l.time);
                            }
                            anim.sequence.push(SeqTl { skin: si, slot, attachment: att_name.clone(), keys: sk });
                        }
                    }
                    let arr = if nested { match arr_or_map.get("deform") { Some(d) => d, None => continue } } else { arr_or_map };
                    // The mesh's length, from whichever skin has it.
                    let len = skins.get(si).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(att_name)).or_else(|| skins.iter().find_map(|s| s.slots.get(slot).and_then(|m| m.get(att_name)))).and_then(|a| a.deform_len());
                    let Some(len) = len else { continue };
                    let frames_v: Vec<&Value> = arr.as_array().map(|a| a.iter().collect()).unwrap_or_default();
                    let mut keys: Vec<(f32, Vec<f32>, Raw)> = Vec::new();
                    for f in frames_v {
                        let mut full = vec![0.0f32; len];
                        let off = num(f, "offset", 0.0) as usize;
                        for (k, v) in floats(f.get("vertices")).into_iter().enumerate() {
                            if off + k < len {
                                full[off + k] = v;
                            }
                        }
                        keys.push((num(f, "time", 0.0), full, raw_curve(f, 1)));
                    }
                    keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    let mut tl = DeformTl { skin: si, slot, attachment: att_name.clone(), times: Vec::new(), frames: Vec::new(), curves: Vec::new() };
                    for i in 0..keys.len() {
                        // A deform's bezier is in time (absolute from 4.0) and in fractions of the change (always).
                        let curve = match &keys[i].2 {
                            _ if i + 1 >= keys.len() => Curve::Linear,
                            Raw::Bezier(c) if c.len() >= 4 => deform_fractions(c, keys[i].0, keys[i + 1].0, absolute),
                            Raw::Stepped => Curve::Stepped,
                            _ => Curve::Linear,
                        };
                        tl.times.push(keys[i].0);
                        tl.frames.push(std::mem::take(&mut keys[i].1));
                        tl.curves.push(curve);
                    }
                    if let Some(&t) = tl.times.last() {
                        note(t);
                    }
                    anim.deform.push(tl);
                }
            }
        }
    }
    }

    let order = a.get("drawOrder").or_else(|| a.get("draworder"));
    if let Some(Value::Array(frames)) = order {
        for f in frames {
            let t = num(f, "time", 0.0);
            let offsets: Vec<(usize, i32)> = f.get("offsets").and_then(|o| o.as_array()).map(|o| o.iter().filter_map(|e| Some((*slot_ids.get(text(e, "slot")?)?, num(e, "offset", 0.0) as i32))).collect()).unwrap_or_default();
            note(t);
            anim.draw_order.push((t, offsets));
        }
        anim.draw_order.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    }
    if let Some(Value::Array(frames)) = a.get("events") {
        for f in frames {
            note(num(f, "time", 0.0));
        }
    }
    let _ = slots;
    anim.duration = dur;
    anim
}

/// A deform key's bezier when the first offset does not change: use the x control points as time fractions and keep y as written.
fn deform_fractions(c: &[f32], t0: f32, t1: f32, absolute: bool) -> Curve {
    if !absolute {
        return Curve::Bezier([c[0], c[1], c[2], c[3]]);
    }
    let dt = (t1 - t0).max(1e-6);
    Curve::Bezier([(c[0] - t0) / dt, c[1].clamp(0.0, 1.0), (c[2] - t0) / dt, c[3].clamp(0.0, 1.0)])
}
