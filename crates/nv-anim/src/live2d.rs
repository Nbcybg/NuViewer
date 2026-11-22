//! Live2D Cubism's own files around the `.moc3`: `model3.json` (what belongs to the model), `cdi3.json` (readable names of parameters and
//! parts), `motion3.json` (curves turning parameters), `exp3.json` (expressions), and the step from the drawables the Core computes to
//! the draw list the rasteriser takes. All plain JSON and plain arithmetic, so it is tested without the Core DLL (`cubism.rs`).

use crate::cubism::Drawable;
use crate::model::BlendMode;
use crate::pose::{DrawItem, MaskGeom};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

/// What `*.model3.json` lists (paths are relative to the json's folder).
#[derive(Clone, Debug, Default)]
pub struct Model3 {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: Option<String>,
    pub pose: Option<String>,
    pub display_info: Option<String>,
    pub user_data: Option<String>,
    /// Parameter ids of the "EyeBlink" and "LipSync" groups.
    pub eye_blink: Vec<String>,
    pub lip_sync: Vec<String>,
    /// (name, file)
    pub expressions: Vec<(String, String)>,
    /// (group, file)
    pub motions: Vec<(String, String)>,
    pub version: u32,
}

pub fn parse_model3(src: &str) -> Result<Model3, String> {
    let v: Value = serde_json::from_str(src).map_err(|e| format!("model3.json: {e}"))?;
    let fr = v.get("FileReferences").ok_or("model3.json: no FileReferences (not a Live2D model file)")?;
    let moc = s(fr, "Moc").ok_or("model3.json: no Moc file named")?;
    let mut m = Model3 { moc, version: v.get("Version").and_then(|x| x.as_u64()).unwrap_or(3) as u32, ..Default::default() };
    if let Some(a) = fr.get("Textures").and_then(|t| t.as_array()) {
        m.textures = a.iter().filter_map(|t| t.as_str().map(String::from)).collect();
    }
    for g in v.get("Groups").and_then(|g| g.as_array()).into_iter().flatten() {
        let ids: Vec<String> = g.get("Ids").and_then(|i| i.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        match s(g, "Name").as_deref() {
            Some("EyeBlink") => m.eye_blink = ids,
            Some("LipSync") => m.lip_sync = ids,
            _ => {}
        }
    }
    m.physics = s(fr, "Physics");
    m.pose = s(fr, "Pose");
    m.display_info = s(fr, "DisplayInfo");
    m.user_data = s(fr, "UserData");
    if let Some(a) = fr.get("Expressions").and_then(|t| t.as_array()) {
        m.expressions = a.iter().filter_map(|e| Some((s(e, "Name")?, s(e, "File")?))).collect();
    }
    if let Some(o) = fr.get("Motions").and_then(|t| t.as_object()) {
        for (group, list) in o {
            for e in list.as_array().into_iter().flatten() {
                if let Some(f) = s(e, "File") {
                    m.motions.push((group.clone(), f));
                }
            }
        }
    }
    Ok(m)
}

/// `cdi3.json`: readable names.
#[derive(Clone, Debug, Default)]
pub struct DisplayInfo {
    pub params: HashMap<String, String>,
    pub parts: HashMap<String, String>,
}

pub fn parse_cdi3(src: &str) -> Result<DisplayInfo, String> {
    let v: Value = serde_json::from_str(src).map_err(|e| format!("cdi3.json: {e}"))?;
    let pick = |key: &str| -> HashMap<String, String> { v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|e| Some((s(e, "Id")?, s(e, "Name")?))).collect()).unwrap_or_default() };
    Ok(DisplayInfo { params: pick("Parameters"), parts: pick("Parts") })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Parameter,
    PartOpacity,
    Model,
}

#[derive(Clone, Debug)]
pub struct MotionCurve {
    pub target: Target,
    pub id: String,
    /// Flat segment list: t0, v0, then (type, points ...) repeated.
    pub segments: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct Motion {
    pub duration: f32,
    pub fps: f32,
    pub looped: bool,
    pub curves: Vec<MotionCurve>,
}

pub fn parse_motion3(src: &str) -> Result<Motion, String> {
    let v: Value = serde_json::from_str(src).map_err(|e| format!("motion3.json: {e}"))?;
    let meta = v.get("Meta").ok_or("motion3.json: no Meta")?;
    let mut curves = Vec::new();
    for c in v.get("Curves").and_then(|c| c.as_array()).into_iter().flatten() {
        let target = match s(c, "Target").as_deref() {
            Some("Parameter") => Target::Parameter,
            Some("PartOpacity") => Target::PartOpacity,
            Some("Model") => Target::Model,
            _ => continue,
        };
        let segments: Vec<f32> = c.get("Segments").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default();
        if segments.len() >= 2 {
            curves.push(MotionCurve { target, id: s(c, "Id").unwrap_or_default(), segments });
        }
    }
    Ok(Motion { duration: meta.get("Duration").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32, fps: meta.get("Fps").and_then(|x| x.as_f64()).unwrap_or(30.0) as f32, looped: meta.get("Loop").and_then(|x| x.as_bool()).unwrap_or(false), curves })
}

impl MotionCurve {
    /// The curve's value at `t` seconds (Cubism's rules: bezier segments are evaluated on their value points with time taken as linear).
    pub fn value_at(&self, t: f32) -> f32 {
        let sg = &self.segments;
        let (mut t0, mut v0) = (sg[0], sg[1]);
        if t <= t0 {
            return v0;
        }
        let mut i = 2;
        while i < sg.len() {
            let ty = sg[i] as i32;
            let n = if ty == 1 { 3 } else { 1 };
            if i + 1 + n * 2 > sg.len() {
                break;
            }
            let pt = |k: usize| (sg[i + 1 + k * 2], sg[i + 2 + k * 2]);
            let (t_end, v_end) = pt(n - 1);
            if t <= t_end {
                let u = if t_end > t0 { (t - t0) / (t_end - t0) } else { 1.0 };
                return match ty {
                    0 => v0 + (v_end - v0) * u,
                    1 => {
                        let (v1, v2) = (pt(0).1, pt(1).1);
                        let lerp = |a: f32, b: f32| a + (b - a) * u;
                        let (p01, p12, p23) = (lerp(v0, v1), lerp(v1, v2), lerp(v2, v_end));
                        lerp(lerp(p01, p12), lerp(p12, p23))
                    }
                    2 => v0,
                    _ => v_end,
                };
            }
            t0 = t_end;
            v0 = v_end;
            i += 1 + n * 2;
        }
        v0
    }
}

impl Motion {
    /// Time inside the motion for a playing time: looped motions wrap, others stop at the end.
    pub fn local_time(&self, t: f32) -> f32 {
        if self.duration <= 0.0 {
            0.0
        } else if self.looped {
            t.rem_euclid(self.duration)
        } else {
            t.clamp(0.0, self.duration)
        }
    }

    /// (target, id, value) for every curve at `t`.
    pub fn sample(&self, t: f32) -> Vec<(Target, &str, f32)> {
        let t = self.local_time(t);
        self.curves.iter().map(|c| (c.target, c.id.as_str(), c.value_at(t))).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpBlend {
    Add,
    Multiply,
    Overwrite,
}

/// `exp3.json`: (parameter id, value, how it combines).
pub fn parse_exp3(src: &str) -> Result<Vec<(String, f32, ExpBlend)>, String> {
    let v: Value = serde_json::from_str(src).map_err(|e| format!("exp3.json: {e}"))?;
    Ok(v.get("Parameters").and_then(|p| p.as_array()).into_iter().flatten().filter_map(|p| {
        let blend = match s(p, "Blend").as_deref() {
            Some("Multiply") => ExpBlend::Multiply,
            Some("Overwrite") => ExpBlend::Overwrite,
            _ => ExpBlend::Add,
        };
        Some((s(p, "Id")?, p.get("Value")?.as_f64()? as f32, blend))
    }).collect())
}

/// Combine an expression value with a parameter's current value and its default (Cubism's rule).
pub fn blend_expression(current: f32, default: f32, value: f32, blend: ExpBlend, weight: f32) -> f32 {
    let target = match blend {
        ExpBlend::Add => current + value,
        ExpBlend::Multiply => current * value,
        ExpBlend::Overwrite => value,
    };
    let _ = default;
    current + (target - current) * weight.clamp(0.0, 1.0)
}

/// How open the eyes are at `t` seconds when nothing else drives them: open, shut for a fifth of a second every 4.2 seconds (deterministic, so
/// an export blinks where the screen did).
pub fn blink_at(t: f32) -> f32 {
    let phase = t.rem_euclid(4.2);
    let close = 0.2;
    if phase >= close {
        1.0
    } else {
        // down and up again: 1 -> 0 -> 1 over the closing time
        ((phase / close) * 2.0 - 1.0).abs()
    }
}

/// The breathing value (0..1) at `t`, for the usual "ParamBreath".
pub fn breath_at(t: f32) -> f32 {
    0.5 + 0.5 * (t * 2.0).sin()
}

// ---- drawables to a draw list ------------------------------------------------------------------------------------------------------

/// The draw list for what the Core computed. Back to front by render order; hidden or empty drawables are left out; clipping masks become
/// the silhouette of the drawables they name; texture v is flipped (Cubism's v points up).
pub fn items_from_drawables(d: &[Drawable]) -> Vec<DrawItem> {
    let mut order: Vec<usize> = (0..d.len()).filter(|&i| d[i].visible && d[i].opacity > 0.0 && d[i].indices.len() >= 3 && !d[i].verts.is_empty()).collect();
    order.sort_by_key(|&i| (d[i].render_order, i));
    let mut mask_cache: HashMap<(Vec<usize>, bool), Arc<MaskGeom>> = HashMap::new();
    order
        .into_iter()
        .map(|i| {
            let dr = &d[i];
            let mask = (!dr.masks.is_empty()).then(|| {
                mask_cache
                    .entry((dr.masks.clone(), dr.inverted_mask))
                    .or_insert_with(|| {
                        let mut tris = Vec::new();
                        for &m in &dr.masks {
                            let Some(md) = d.get(m) else { continue };
                            for t in md.indices.chunks_exact(3) {
                                if let (Some(a), Some(b), Some(c)) = (md.verts.get(t[0] as usize), md.verts.get(t[1] as usize), md.verts.get(t[2] as usize)) {
                                    tris.push([*a, *b, *c]);
                                }
                            }
                        }
                        Arc::new(MaskGeom { tris, inverted: dr.inverted_mask })
                    })
                    .clone()
            });
            DrawItem {
                slot: i,
                page: dr.texture,
                blend: if dr.additive {
                    BlendMode::Additive
                } else if dr.multiply {
                    BlendMode::Multiply
                } else {
                    BlendMode::Normal
                },
                pma: false,
                color: [1.0, 1.0, 1.0, dr.opacity.clamp(0.0, 1.0)],
                dark: None,
                verts: dr.verts.clone(),
                uvs: dr.uvs.iter().map(|u| [u[0], 1.0 - u[1]]).collect(),
                tris: dr.indices.clone(),
                clip: None,
                mask,
            }
        })
        .collect()
}

/// The box around the visible drawables: `[min x, min y, max x, max y]`.
pub fn bounds_of_items(items: &[DrawItem]) -> Option<[f32; 4]> {
    let mut bb: Option<[f32; 4]> = None;
    for it in items {
        for v in &it.verts {
            let b = bb.get_or_insert([v[0], v[1], v[0], v[1]]);
            b[0] = b[0].min(v[0]);
            b[1] = b[1].min(v[1]);
            b[2] = b[2].max(v[0]);
            b[3] = b[3].max(v[1]);
        }
    }
    bb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::PageImage;
    use crate::raster::{render_pages, View};

    #[test]
    fn model3_lists_what_belongs_to_the_model() {
        let src = r#"{"Version":3,"FileReferences":{"Moc":"a.moc3","Textures":["t0.png","t1.png"],"Physics":"a.physics3.json","DisplayInfo":"a.cdi3.json",
          "Expressions":[{"Name":"smile","File":"smile.exp3.json"}],"Motions":{"Idle":[{"File":"idle.motion3.json"},{"File":"idle2.motion3.json"}],"Tap":[{"File":"tap.motion3.json"}]}}}"#;
        let m = parse_model3(src).unwrap();
        assert_eq!((m.moc.as_str(), m.textures.len(), m.motions.len(), m.expressions.len()), ("a.moc3", 2, 3, 1));
        assert_eq!(m.physics.as_deref(), Some("a.physics3.json"));
        assert!(m.pose.is_none());
        assert!(parse_model3("{}").is_err() && parse_model3(r#"{"FileReferences":{}}"#).is_err());
    }

    #[test]
    fn groups_name_the_eye_and_lip_parameters_and_the_blink_is_a_dip() {
        let m = parse_model3(r#"{"Version":3,"FileReferences":{"Moc":"a.moc3"},"Groups":[{"Target":"Parameter","Name":"EyeBlink","Ids":["ParamEyeLOpen","ParamEyeROpen"]},{"Target":"Parameter","Name":"LipSync","Ids":["ParamMouthOpenY"]}]}"#).unwrap();
        assert_eq!(m.eye_blink, vec!["ParamEyeLOpen", "ParamEyeROpen"]);
        assert_eq!(m.lip_sync, vec!["ParamMouthOpenY"]);
        assert_eq!(blink_at(1.0), 1.0, "open most of the time");
        assert!(blink_at(0.1) < 0.05, "shut in the middle of the blink: {}", blink_at(0.1));
        assert!((blink_at(0.0) - 1.0).abs() < 1e-6 && (blink_at(4.2) - 1.0).abs() < 1e-6, "and open at the start and the end of it");
        assert!((breath_at(0.0) - 0.5).abs() < 1e-6 && breath_at(0.785) > 0.99);
    }

    #[test]
    fn cdi3_gives_readable_names() {
        let d = parse_cdi3(r#"{"Parameters":[{"Id":"ParamAngleX","GroupId":"","Name":"Angle X"}],"Parts":[{"Id":"PartArmL","Name":"Left arm"}]}"#).unwrap();
        assert_eq!(d.params["ParamAngleX"], "Angle X");
        assert_eq!(d.parts["PartArmL"], "Left arm");
    }

    fn motion(segments: &str, looped: bool) -> Motion {
        parse_motion3(&format!(r#"{{"Version":3,"Meta":{{"Duration":2.0,"Fps":30,"Loop":{looped}}},"Curves":[{{"Target":"Parameter","Id":"P","Segments":{segments}}}]}}"#)).unwrap()
    }

    #[test]
    fn motion_segments_linear_stepped_inverse_stepped_and_bezier() {
        // 0 -> 10 linear over 1 s, hold 10 (stepped) until 1.5 then jump to 20, inverse stepped to 30 at 2.
        let m = motion("[0,0, 0,1,10, 2,1.5,20, 3,2,30]", false);
        let v = |t: f32| m.curves[0].value_at(t);
        assert!((v(0.5) - 5.0).abs() < 1e-4);
        assert!((v(1.0) - 10.0).abs() < 1e-4);
        assert!((v(1.2) - 10.0).abs() < 1e-4, "stepped holds the old value");
        assert!((v(1.6) - 30.0).abs() < 1e-4, "inverse stepped takes the new value at once");
        assert!((v(5.0) - 30.0).abs() < 1e-4, "after the end");
        assert!((v(-1.0) - 0.0).abs() < 1e-4);
        // bezier with value points 0, 0, 10, 10 is an ease: middle is the middle, quarter is below linear
        let b = motion("[0,0, 1,0.3,0, 0.6,10, 1,10]", false);
        assert!((b.curves[0].value_at(0.5) - 5.0).abs() < 1e-3);
        assert!(b.curves[0].value_at(0.25) < 2.5);
        // truncated data is survived
        let t = motion("[0,0, 1,0.3]", false);
        assert_eq!(t.curves[0].value_at(1.0), 0.0);
    }

    #[test]
    fn looped_motions_wrap_and_others_stop() {
        let looped = motion("[0,0, 0,2,10]", true);
        assert!((looped.local_time(2.5) - 0.5).abs() < 1e-5);
        assert!((looped.sample(2.5)[0].2 - 2.5).abs() < 1e-4);
        let once = motion("[0,0, 0,2,10]", false);
        assert!((once.local_time(9.0) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn expressions_combine_by_their_blend() {
        let e = parse_exp3(r#"{"Type":"Live2D Expression","Parameters":[{"Id":"A","Value":0.5,"Blend":"Add"},{"Id":"B","Value":2,"Blend":"Multiply"},{"Id":"C","Value":1,"Blend":"Overwrite"}]}"#).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(blend_expression(1.0, 0.0, 0.5, ExpBlend::Add, 1.0), 1.5);
        assert_eq!(blend_expression(3.0, 1.0, 2.0, ExpBlend::Multiply, 0.5), 4.5);
        assert_eq!(blend_expression(3.0, 0.0, 9.0, ExpBlend::Overwrite, 0.0), 3.0);
    }

    fn quad(x0: f32, y0: f32, x1: f32, y1: f32, order: i32) -> Drawable {
        Drawable { id: "d".into(), additive: false, multiply: false, double_sided: true, inverted_mask: false, visible: true, texture: 0, render_order: order, opacity: 1.0, masks: vec![], verts: vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], indices: vec![0, 1, 2, 0, 2, 3] }
    }

    fn white_page() -> Vec<Option<Arc<PageImage>>> {
        vec![Some(Arc::new(PageImage { width: 2, height: 2, rgba: vec![255; 16] }))]
    }

    #[test]
    fn drawables_become_items_in_render_order_and_hidden_ones_are_dropped() {
        let mut a = quad(0.0, 0.0, 1.0, 1.0, 5);
        let mut b = quad(0.0, 0.0, 1.0, 1.0, 1);
        let mut c = quad(0.0, 0.0, 1.0, 1.0, 3);
        c.visible = false;
        a.additive = true;
        b.opacity = 0.5;
        let items = items_from_drawables(&[a, b, c]);
        assert_eq!(items.iter().map(|i| i.slot).collect::<Vec<_>>(), vec![1, 0]);
        assert_eq!(items[0].color[3], 0.5);
        assert_eq!(items[1].blend, BlendMode::Additive);
        assert_eq!(items[0].uvs[0], [0.0, 0.0], "v is flipped");
        assert_eq!(bounds_of_items(&items), Some([0.0, 0.0, 1.0, 1.0]));
    }

    #[test]
    fn a_clipping_mask_cuts_a_drawable_to_the_masks_silhouette_and_inversion_flips_it() {
        let mask_quad = quad(-10.0, -10.0, 0.0, 10.0, 0);
        let mut body = quad(-10.0, -10.0, 10.0, 10.0, 1);
        body.masks = vec![0];
        let mut hidden_mask = mask_quad.clone();
        hidden_mask.visible = false; // a mask drawable itself is usually not drawn
        let pages = white_page();
        let view = View { cx: 0.0, cy: 0.0, scale: 1.0 };
        let px = render_pages(&pages, &items_from_drawables(&[hidden_mask.clone(), body.clone()]), 20, 20, view, 1, None);
        let a = |x: usize| px[(10 * 20 + x) * 4 + 3];
        assert_eq!(a(5), 255, "inside the mask (left half)");
        assert_eq!(a(15), 0, "outside the mask");
        body.inverted_mask = true;
        let px = render_pages(&pages, &items_from_drawables(&[hidden_mask, body]), 20, 20, view, 1, None);
        assert_eq!(px[(10 * 20 + 5) * 4 + 3], 0);
        assert_eq!(px[(10 * 20 + 15) * 4 + 3], 255);
    }
}
