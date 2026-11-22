//! Spine's JSON skeleton (3.8, 4.0, 4.1, 4.2). The versions differ in small ways this reader absorbs:
//!  * bone `transform` (<= 4.1) / `inherit` (4.2); constraint mixes `rotateMix` ... (<= 4.0) / `mixRotate` ... (>= 4.1);
//!  * curves: 3.x gives each key one curve as fractions of the segment, 4.x one per channel in absolute time / value;
//!  * rotation timelines: 3.x takes the short way round between two angles, 4.x does not; 4.2 splits translate / scale / shear in x and y;
//!  * slot colours: `color` / `twoColor` (3.x) and `rgba` / `rgb` / `alpha` / `rgba2` / `rgb2` (4.x).
//!
//! Path constraints and 4.2's physics constraints are read as "not supported" (named in `Rig::unsupported`), events are skipped.

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
    let mut unsupported: Vec<String> = Vec::new();

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

    // Constraints.
    let mut ik: Vec<IkData> = Vec::new();
    let mut ik_ids: HashMap<String, usize> = HashMap::new();
    let mut order_guess = 0;
    for c in root.get("ik").and_then(|c| c.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
        let name = text(c, "name").unwrap_or("").to_string();
        let bones_of: Vec<usize> = c.get("bones").and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).filter_map(|n| bone_ids.get(n).copied()).collect()).unwrap_or_default();
        let target = bone_ids.get(text(c, "target").unwrap_or("")).copied();
        let (Some(target), false) = (target, bones_of.is_empty() || bones_of.len() > 2) else { continue };
        ik_ids.insert(name.clone(), ik.len());
        ik.push(IkData { name, order: c.get("order").and_then(|o| o.as_i64()).map(|o| o as i32).unwrap_or(order_guess), bones: bones_of, target, mix: num(c, "mix", 1.0), softness: num(c, "softness", 0.0), bend_positive: flag(c, "bendPositive", true), compress: flag(c, "compress", false), stretch: flag(c, "stretch", false), uniform: flag(c, "uniform", false), skin_required: flag(c, "skin", false) });
        order_guess += 1;
    }
    let mut transform: Vec<TransformData> = Vec::new();
    let mut tf_ids: HashMap<String, usize> = HashMap::new();
    for c in root.get("transform").and_then(|c| c.as_array()).map(|a| a.as_slice()).unwrap_or(&[]) {
        let name = text(c, "name").unwrap_or("").to_string();
        let mut bones_of: Vec<usize> = c.get("bones").and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).filter_map(|n| bone_ids.get(n).copied()).collect()).unwrap_or_default();
        if bones_of.is_empty() {
            if let Some(b) = text(c, "bone").and_then(|n| bone_ids.get(n)) {
                bones_of.push(*b); // Spine 3.5 and older wrote a single bone
            }
        }
        let Some(target) = bone_ids.get(text(c, "target").unwrap_or("")).copied() else { continue };
        let first = |a: &str, b: &str, d: f32| if c.get(a).is_some() { num(c, a, d) } else { num(c, b, d) };
        let mix_rotate = first("mixRotate", "rotateMix", 1.0);
        let mix_x = first("mixX", "translateMix", 1.0);
        let mix_scale_x = first("mixScaleX", "scaleMix", 1.0);
        tf_ids.insert(name.clone(), transform.len());
        transform.push(TransformData {
            name,
            order: c.get("order").and_then(|o| o.as_i64()).map(|o| o as i32).unwrap_or(order_guess),
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
        });
        order_guess += 1;
    }
    if root.get("path").and_then(|p| p.as_array()).map(|a| !a.is_empty()).unwrap_or(false) {
        unsupported.push("path constraints".into());
    }
    if root.get("physics").and_then(|p| p.as_array()).map(|a| !a.is_empty()).unwrap_or(false) {
        unsupported.push("physics constraints".into());
    }

    // Skins: an array (3.6 and later) or an object (older).
    let mut skins: Vec<Skin> = Vec::new();
    struct Linked {
        skin: usize,
        slot: usize,
        key: String,
        parent: String,
        parent_skin: String,
        deform: bool,
        path: String,
        color: [f32; 4],
        width: f32,
        height: f32,
    }
    let mut linked: Vec<Linked> = Vec::new();
    let skin_values: Vec<(String, &Value)> = match root.get("skins") {
        Some(Value::Array(a)) => a.iter().map(|s| (text(s, "name").unwrap_or("default").to_string(), s)).collect(),
        Some(Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    };
    for (si, (name, sv)) in skin_values.iter().enumerate() {
        let mut skin = Skin { name: name.clone(), slots: vec![HashMap::new(); slots.len()], bones: Vec::new(), ik: Vec::new(), transform: Vec::new() };
        let by_names = |key: &str, ids: &HashMap<String, usize>| -> Vec<usize> { sv.get(key).and_then(|b| b.as_array()).map(|a| a.iter().filter_map(|n| n.as_str()).filter_map(|n| ids.get(n).copied()).collect()).unwrap_or_default() };
        skin.bones = by_names("bones", &bone_ids);
        skin.ik = by_names("ik", &ik_ids);
        skin.transform = by_names("transform", &tf_ids);
        let atts = if sv.get("attachments").is_some() { sv.get("attachments") } else { Some(*sv) };
        if let Some(Value::Object(per_slot)) = atts {
            for (slot_name, entries) in per_slot {
                let Some(&slot) = slot_ids.get(slot_name) else { continue };
                let Value::Object(entries) = entries else { continue };
                for (key, a) in entries {
                    let ty = text(a, "type").unwrap_or("region");
                    let path = text(a, "path").or_else(|| text(a, "name")).unwrap_or(key).to_string();
                    let att = match ty {
                        "region" => Attachment::Region(Region { path, x: num(a, "x", 0.0), y: num(a, "y", 0.0), rotation: num(a, "rotation", 0.0), scale_x: num(a, "scaleX", 1.0), scale_y: num(a, "scaleY", 1.0), width: num(a, "width", 32.0), height: num(a, "height", 32.0), color: color(a, "color", [1.0; 4]) }),
                        "mesh" => {
                            let uvs = floats(a.get("uvs"));
                            let verts = floats(a.get("vertices"));
                            let vertices = if verts.len() == uvs.len() { Vertices::Plain(verts) } else { weighted(&verts) };
                            let tris: Vec<u16> = floats(a.get("triangles")).iter().map(|&t| t as u16).collect();
                            Attachment::Mesh(Mesh { path, color: color(a, "color", [1.0; 4]), width: num(a, "width", 0.0), height: num(a, "height", 0.0), uvs, triangles: tris, vertices, hull: num(a, "hull", 0.0) as usize, deform_skin: si, deform_name: key.clone() })
                        }
                        "linkedmesh" | "weightedlinkedmesh" => {
                            linked.push(Linked { skin: si, slot, key: key.clone(), parent: text(a, "parent").unwrap_or("").to_string(), parent_skin: text(a, "skin").unwrap_or("default").to_string(), deform: flag(a, "deform", true), path, color: color(a, "color", [1.0; 4]), width: num(a, "width", 0.0), height: num(a, "height", 0.0) });
                            continue;
                        }
                        "clipping" => {
                            let verts = floats(a.get("vertices"));
                            let count = num(a, "vertexCount", 0.0) as usize;
                            let vertices = if verts.len() == count * 2 { Vertices::Plain(verts) } else { weighted(&verts) };
                            Attachment::Clip(Clip { end_slot: text(a, "end").and_then(|e| slot_ids.get(e).copied()).unwrap_or(slot), vertices, vertex_count: count })
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
            let parent = skins.get(psi).and_then(|s| s.slots.get(l.slot)).and_then(|m| m.get(&l.parent)).cloned();
            match parent {
                Some(Attachment::Mesh(p)) => {
                    let (deform_skin, deform_name) = if l.deform { (p.deform_skin, p.deform_name.clone()) } else { (l.skin, l.key.clone()) };
                    skins[l.skin].slots[l.slot].insert(l.key.clone(), Attachment::Mesh(Mesh { path: l.path.clone(), color: l.color, width: if l.width > 0.0 { l.width } else { p.width }, height: if l.height > 0.0 { l.height } else { p.height }, deform_skin, deform_name, ..p }));
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
            animations.push(read_animation(name, a, absolute, &bone_ids, &slot_ids, &ik_ids, &tf_ids, &skins, &slots, &ik));
        }
    }
    animations.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Rig { version, name: String::new(), x: num(head, "x", 0.0), y: num(head, "y", 0.0), width: num(head, "width", 0.0), height: num(head, "height", 0.0), fps: num(head, "fps", 30.0), bones, slots, skins, ik, transform, animations, atlas, legacy, unsupported })
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
fn read_animation(name: &str, a: &Value, absolute: bool, bone_ids: &HashMap<String, usize>, slot_ids: &HashMap<String, usize>, ik_ids: &HashMap<String, usize>, tf_ids: &HashMap<String, usize>, skins: &[Skin], slots: &[SlotData], _ik: &[IkData]) -> Animation {
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

    // Deform: skin -> slot -> attachment -> keys.
    if let Some(Value::Object(per_skin)) = a.get("deform") {
        for (skin_name, per_slot) in per_skin {
            let Some(si) = skins.iter().position(|s| &s.name == skin_name) else { continue };
            let Value::Object(per_slot) = per_slot else { continue };
            for (slot_name, atts) in per_slot {
                let Some(&slot) = slot_ids.get(slot_name) else { continue };
                let Value::Object(atts) = atts else { continue };
                for (att_name, arr) in atts {
                    // The mesh's length, from whichever skin has it.
                    let len = skins.get(si).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(att_name)).or_else(|| skins.iter().find_map(|s| s.slots.get(slot).and_then(|m| m.get(att_name)))).and_then(|a| if let Attachment::Mesh(m) = a { Some(m.vertices.deform_len()) } else { None });
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
