use super::*;
use std::sync::Arc;

const ATLAS: &str = "t.png\nsize:4,4\npma:false\nbox\nbounds:0,0,4,4\n";

fn with_page(mut rig: Rig, rgba: [u8; 4]) -> Rig {
    let px: Vec<u8> = (0..16).flat_map(|_| rgba).collect();
    rig.atlas.pages[0].image = Some(Arc::new(PageImage { width: 4, height: 4, rgba: px }));
    rig
}

/// A skeleton in the dialect of `version`: a root, an arm (length 50) and a 100 x 100 box on the arm; "turn" rotates the arm 0 -> 90 degrees in a second.
fn skeleton(version: &str) -> String {
    let legacy = version.starts_with("3.");
    let inherit_key = if version.starts_with("4.2") { "inherit" } else { "transform" };
    let key = if legacy || version.starts_with("4.0") { "angle" } else { "value" };
    format!(
        r#"{{"skeleton":{{"hash":"x","spine":"{version}","x":-60,"y":-60,"width":120,"height":120,"fps":30}},
        "bones":[{{"name":"root"}},{{"name":"arm","parent":"root","length":50,"x":10,"{inherit_key}":"normal"}}],
        "slots":[{{"name":"s","bone":"arm","attachment":"box"}}],
        "skins":[{{"name":"default","attachments":{{"s":{{"box":{{"width":100,"height":100}}}}}}}}],
        "animations":{{"turn":{{"bones":{{"arm":{{"rotate":[{{"time":0,"{key}":0}},{{"time":1,"{key}":90}}]}}}}}}}}}}"#
    )
}

fn angle(p: &Pose, bone: usize) -> f32 {
    p.bones[bone].c.atan2(p.bones[bone].a).to_degrees()
}

#[test]
fn every_version_reads_the_same_skeleton_and_turns_the_arm() {
    for v in ["3.8.99", "4.0.64", "4.1.24", "4.2.33"] {
        let rig = Rig::from_json(&skeleton(v), ATLAS).unwrap_or_else(|e| panic!("{v}: {e}"));
        assert_eq!(rig.bones.len(), 2, "{v}");
        assert_eq!(rig.legacy, v.starts_with("3."), "{v}");
        let a = rig.animation_index("turn").unwrap();
        assert!((rig.animations[a].duration - 1.0).abs() < 1e-6);
        let p = rig.pose(0, Some(a), 0.5);
        assert!((angle(&p, 1) - 45.0).abs() < 0.01, "{v}: {}", angle(&p, 1));
        // the arm sits at x = 10 on the root
        assert!((p.bones[1].wx - 10.0).abs() < 1e-4 && p.bones[1].wy.abs() < 1e-4);
        let end = rig.pose(0, Some(a), 5.0);
        assert!((angle(&end, 1) - 90.0).abs() < 0.01, "{v}");
        let setup = rig.pose(0, None, 0.0);
        assert!(angle(&setup, 1).abs() < 0.01);
    }
}

#[test]
fn old_curves_are_fractions_and_new_curves_are_absolute_and_both_give_one_shape() {
    // The same ease: 3.8 writes control points as fractions of the segment, 4.x in seconds and degrees.
    let old = r#"{"skeleton":{"spine":"3.8.99"},"bones":[{"name":"root"}],"slots":[],"animations":{"a":{"bones":{"root":{"rotate":[{"time":0,"angle":0,"curve":[0.25,0,0.75,1]},{"time":2,"angle":90}]}}}}}"#;
    let new = r#"{"skeleton":{"spine":"4.1.24"},"bones":[{"name":"root"}],"slots":[],"animations":{"a":{"bones":{"root":{"rotate":[{"time":0,"value":0,"curve":[0.5,0,1.5,90]},{"time":2,"value":90}]}}}}}"#;
    let (a, b) = (Rig::from_json(old, ATLAS).unwrap(), Rig::from_json(new, ATLAS).unwrap());
    for t in [0.1f32, 0.5, 1.0, 1.5, 1.9] {
        let (x, y) = (angle(&a.pose(0, Some(0), t), 0), angle(&b.pose(0, Some(0), t), 0));
        assert!((x - y).abs() < 0.05, "t={t}: {x} vs {y}");
    }
    let mid = angle(&a.pose(0, Some(0), 1.0), 0);
    assert!((mid - 45.0).abs() < 0.5, "an ease-in-out is at the middle in the middle: {mid}");
    let early = angle(&a.pose(0, Some(0), 0.3), 0);
    assert!(early < 0.15 * 90.0, "slow at the start: {early}");
}

#[test]
fn stepped_keys_hold_and_3x_rotation_takes_the_short_way() {
    let old = r#"{"skeleton":{"spine":"3.8.99"},"bones":[{"name":"root"}],"slots":[],"animations":{"a":{"bones":{"root":{"rotate":[{"time":0,"angle":170},{"time":1,"angle":-170}]}}}}}"#;
    let new = old.replace("3.8.99", "4.1.24").replace("angle", "value");
    let (a, b) = (Rig::from_json(old, ATLAS).unwrap(), Rig::from_json(&new, ATLAS).unwrap());
    let ra = angle(&a.pose(0, Some(0), 0.5), 0);
    assert!(ra.abs() > 175.0, "3.x crosses 180: {ra}");
    let rb = angle(&b.pose(0, Some(0), 0.5), 0);
    assert!(rb.abs() < 1.0, "4.x goes through 0: {rb}");
    let st = r#"{"skeleton":{"spine":"4.2.33"},"bones":[{"name":"root"}],"slots":[],"animations":{"a":{"bones":{"root":{"translatex":[{"time":0,"value":5,"curve":"stepped"},{"time":1,"value":50}]}}}}}"#;
    let r = Rig::from_json(st, ATLAS).unwrap();
    assert!((r.pose(0, Some(0), 0.9).bones[0].wx - 5.0).abs() < 1e-4);
    assert!((r.pose(0, Some(0), 1.0).bones[0].wx - 50.0).abs() < 1e-4);
}

#[test]
fn draw_order_offsets_follow_spines_rule() {
    use crate::pose::apply_draw_order;
    // 4 slots; slot 0 moves 2 places later: 1 2 0 3
    assert_eq!(apply_draw_order(4, &[(0, 2)]), vec![1, 2, 0, 3]);
    // swap 1 and 2
    assert_eq!(apply_draw_order(4, &[(1, 1), (2, -1)]), vec![0, 2, 1, 3]);
    assert_eq!(apply_draw_order(3, &[]), vec![0, 1, 2]);
}

#[test]
fn a_two_bone_ik_chain_reaches_its_target() {
    let src = r#"{"skeleton":{"spine":"4.1.24"},
      "bones":[{"name":"root"},{"name":"b1","parent":"root","length":100},{"name":"b2","parent":"b1","x":100,"length":100},{"name":"tgt","parent":"root","x":100,"y":100}],
      "slots":[],"ik":[{"name":"ik","bones":["b1","b2"],"target":"tgt","bendPositive":true}]}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    let p = rig.pose(0, None, 0.0);
    let b2 = &p.bones[2];
    let tip = (b2.wx + b2.a * 100.0, b2.wy + b2.c * 100.0);
    assert!((tip.0 - 100.0).abs() < 0.5 && (tip.1 - 100.0).abs() < 0.5, "{tip:?}");
    // the other bend gives a different elbow, same tip
    let src2 = src.replace("\"bendPositive\":true", "\"bendPositive\":false");
    let q = Rig::from_json(&src2, ATLAS).unwrap().pose(0, None, 0.0);
    let tip2 = (q.bones[2].wx + q.bones[2].a * 100.0, q.bones[2].wy + q.bones[2].c * 100.0);
    assert!((tip2.0 - 100.0).abs() < 0.5 && (tip2.1 - 100.0).abs() < 0.5);
    assert!((q.bones[2].wy - p.bones[2].wy).abs() > 10.0, "bend direction matters");
    // too far: the chain stretches out straight towards the target, without breaking
    let far = src.replace("\"x\":100,\"y\":100}", "\"x\":900,\"y\":0}");
    let f = Rig::from_json(&far, ATLAS).unwrap().pose(0, None, 0.0);
    assert!(f.bones[1].c.atan2(f.bones[1].a).abs() < 0.01 && f.bones[2].wx > 99.0);
}

#[test]
fn a_one_bone_ik_points_at_the_target_and_a_transform_constraint_copies_rotation() {
    let src = r#"{"skeleton":{"spine":"3.8.99"},
      "bones":[{"name":"root"},{"name":"a","parent":"root","length":50},{"name":"tgt","parent":"root","x":0,"y":100}],
      "slots":[],"ik":[{"name":"ik","bones":["a"],"target":"tgt"}]}"#;
    let p = Rig::from_json(src, ATLAS).unwrap().pose(0, None, 0.0);
    assert!((angle(&p, 1) - 90.0).abs() < 0.01);
    let tf = r#"{"skeleton":{"spine":"4.1.24"},
      "bones":[{"name":"root"},{"name":"driver","parent":"root","rotation":90},{"name":"f","parent":"root","x":50}],
      "slots":[],"transform":[{"name":"t","bones":["f"],"target":"driver","mixRotate":1,"mixX":0,"mixScaleX":0,"mixShearY":0}]}"#;
    let p = Rig::from_json(tf, ATLAS).unwrap().pose(0, None, 0.0);
    assert!((angle(&p, 2) - 90.0).abs() < 0.01, "{}", angle(&p, 2));
    assert!((p.bones[2].wx - 50.0).abs() < 1e-3, "position untouched");
    // the 3.8 spelling of the mixes
    let old = tf.replace("4.1.24", "3.8.99").replace("mixRotate", "rotateMix").replace("mixX", "translateMix").replace("mixScaleX", "scaleMix").replace("mixShearY", "shearMix");
    let p = Rig::from_json(&old, ATLAS).unwrap().pose(0, None, 0.0);
    assert!((angle(&p, 2) - 90.0).abs() < 0.01);
}

#[test]
fn inherit_modes_differ_when_the_parent_turns_and_scales() {
    let mk = |mode: &str| format!(r#"{{"skeleton":{{"spine":"4.2.33"}},"bones":[{{"name":"root","rotation":90,"scaleX":2}},{{"name":"c","parent":"root","x":10,"inherit":"{mode}"}}],"slots":[]}}"#);
    let get = |mode: &str| Rig::from_json(&mk(mode), ATLAS).unwrap().pose(0, None, 0.0);
    let normal = get("normal");
    assert!((angle(&normal, 1) - 90.0).abs() < 0.01);
    assert!(((normal.bones[1].a.powi(2) + normal.bones[1].c.powi(2)).sqrt() - 2.0).abs() < 1e-3);
    let only_t = get("onlyTranslation");
    assert!(angle(&only_t, 1).abs() < 0.01, "does not turn with the parent");
    assert!((only_t.bones[1].wy - 20.0).abs() < 1e-3, "but is carried by it: {}", only_t.bones[1].wy);
    let no_scale = get("noScale");
    assert!(((no_scale.bones[1].a.powi(2) + no_scale.bones[1].c.powi(2)).sqrt() - 1.0).abs() < 1e-3, "scale not inherited");
    assert!((angle(&no_scale, 1) - 90.0).abs() < 0.01, "rotation is");
    let no_rot = get("noRotationOrReflection");
    assert!(angle(&no_rot, 1).abs() < 0.01);
}

fn quad_mesh_rig(extra: &str, anim: &str) -> String {
    let t = r#"{"skeleton":{"spine":"4.1.24"},
        "bones":[{"name":"root"},{"name":"a","parent":"root"},{"name":"b","parent":"root","x":20}],
        "slots":[{"name":"s","bone":"root","attachment":"m"}],
        "skins":[{"name":"default","attachments":{"s":{"m":{"type":"mesh","path":"box","uvs":[0,0,1,0,1,1,0,1],"triangles":[0,1,2,0,2,3],"vertices":[2,1,0,0,0.5,2,0,0,0.5, 2,1,0,0,0.5,2,0,0,0.5, 2,1,0,10,0.5,2,0,10,0.5, 2,1,0,10,0.5,2,0,10,0.5],"hull":4,"width":10,"height":10}EXTRA}}}],
        "animations":ANIM}"#;
    t.replace("EXTRA", extra).replace("ANIM", &format!("{{{anim}}}"))
}

#[test]
fn weighted_vertices_blend_their_bones_and_deform_adds_offsets() {
    let rig = Rig::from_json(&quad_mesh_rig("", ""), ATLAS).unwrap();
    let p = rig.pose(0, None, 0.0);
    let item = &rig.geometry(&p)[0];
    // each vertex: half on bone a (at 0,0), half on bone b (at 20,0), local (0,0)/(0,10) on a and (0,0)/(0,10) on b: x = 0.5*0 + 0.5*20 = 10
    assert_eq!(item.verts.len(), 4);
    assert!((item.verts[0][0] - 10.0).abs() < 1e-4 && item.verts[0][1].abs() < 1e-4);
    assert!((item.verts[2][1] - 10.0).abs() < 1e-4);
    // 4 vertices x 2 influences x 2 floats = 16 deform floats; the first 4 move vertex 0's two influences by (+1, +2)
    let anim = r#""d":{"deform":{"default":{"s":{"m":[{"time":0,"vertices":[1,2,1,2]},{"time":1,"offset":0,"vertices":[3,4,3,4]}]}}}}"#;
    let rig = Rig::from_json(&quad_mesh_rig("", anim), ATLAS).unwrap();
    let p = rig.pose(0, Some(0), 0.5);
    let v0 = rig.geometry(&p)[0].verts[0];
    assert!((v0[0] - 12.0).abs() < 1e-3 && (v0[1] - 3.0).abs() < 1e-3, "{v0:?}");
    // 3.x / 4.x both keep the file's key time of 1 as the animation length
    assert!((rig.animations[0].duration - 1.0).abs() < 1e-6);
}

#[test]
fn a_linked_mesh_takes_its_parents_shape_and_deform() {
    let linked = r#","lm":{"type":"linkedmesh","parent":"m","path":"box","deform":true}"#;
    let rig = Rig::from_json(&quad_mesh_rig(linked, ""), ATLAS).unwrap();
    let Some((Attachment::Mesh(m), _)) = rig.find_attachment(0, 0, "lm") else { panic!("linked mesh missing") };
    assert_eq!(m.triangles.len(), 6);
    assert_eq!(m.deform_name, "m");
    let own = r#","lm":{"type":"linkedmesh","parent":"m","path":"box","deform":false}"#;
    let rig = Rig::from_json(&quad_mesh_rig(own, ""), ATLAS).unwrap();
    let Some((Attachment::Mesh(m), _)) = rig.find_attachment(0, 0, "lm") else { panic!() };
    assert_eq!(m.deform_name, "lm");
}

#[test]
fn skins_and_the_default_skin_fall_back() {
    let src = r#"{"skeleton":{"spine":"4.1.24"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root","attachment":"a"}],
      "skins":[{"name":"default","attachments":{"s":{"a":{"width":10,"height":10},"b":{"width":10,"height":10}}}},{"name":"red","attachments":{"s":{"a":{"width":99,"height":99}}}}]}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    assert_eq!(rig.skin_index("red"), Some(1));
    let Some((Attachment::Region(r), from)) = rig.find_attachment(1, 0, "a") else { panic!() };
    assert_eq!((r.width, from), (99.0, 1));
    let Some((Attachment::Region(r), from)) = rig.find_attachment(1, 0, "b") else { panic!() };
    assert_eq!((r.width, from), (10.0, 0));
    // the old object form of skins
    let old = r#"{"skeleton":{"spine":"3.4.02"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root"}],"skins":{"default":{"s":{"a":{"width":7,"height":7}}}}}"#;
    let rig = Rig::from_json(old, ATLAS).unwrap();
    assert!(matches!(rig.find_attachment(0, 0, "a"), Some((Attachment::Region(r), _)) if r.width == 7.0));
}

#[test]
fn rendering_puts_pixels_where_the_geometry_says_and_leaves_the_rest_transparent() {
    let rig = with_page(Rig::from_json(&skeleton("4.1.24"), ATLAS).unwrap(), [255, 255, 255, 255]);
    let p = rig.pose(0, None, 0.0);
    // The box is 100 x 100 centred on the arm at x = 10: spans x -40..60, y -50..50.
    let b = rig.bounds_of(&p).unwrap();
    assert!((b[0] + 40.0).abs() < 1e-3 && (b[2] - 60.0).abs() < 1e-3 && (b[1] + 50.0).abs() < 1e-3 && (b[3] - 50.0).abs() < 1e-3, "{b:?}");
    let view = View { cx: 0.0, cy: 0.0, scale: 1.0 };
    let px = rig.draw(&p, 200, 200, view, 1, None);
    let at = |x: usize, y: usize| &px[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4];
    assert_eq!(at(110, 100), &[255, 255, 255, 255], "inside");
    assert_eq!(at(20, 20)[3], 0, "outside is transparent, not black");
    assert_eq!(at(180, 100)[3], 0, "right of x = 60");
    // y is up in the world: the world point (0, 40) is above the centre
    assert_eq!(at(100, 100 - 60)[3], 0, "above y = 50 is outside");
    assert_eq!(at(100, 100 - 45)[3], 255, "y is up: 45 above the centre is inside");
}

#[test]
fn vertex_colour_blend_modes_and_a_background() {
    let mut rig = with_page(Rig::from_json(&skeleton("4.1.24"), ATLAS).unwrap(), [200, 100, 50, 255]);
    rig.slots[0].color = [0.5, 1.0, 1.0, 0.5];
    let view = View { cx: 10.0, cy: 0.0, scale: 1.0 };
    let px = rig.draw(&rig.pose(0, None, 0.0), 100, 100, view, 1, None);
    let c = &px[(50 * 100 + 50) * 4..(50 * 100 + 50) * 4 + 4];
    assert!((c[0] as i32 - 100).abs() <= 1 && (c[1] as i32 - 100).abs() <= 1 && (c[3] as i32 - 128).abs() <= 1, "{c:?}: straight colour, half alpha");
    // on a white background the half-transparent colour mixes
    let px = rig.draw(&rig.pose(0, None, 0.0), 100, 100, view, 1, Some([1.0, 1.0, 1.0]));
    let c = &px[(50 * 100 + 50) * 4..(50 * 100 + 50) * 4 + 4];
    assert_eq!(c[3], 255);
    assert!(c[0] > 150 && c[0] < 200, "{c:?}");
    // additive over a transparent canvas still shows the colour; multiply over nothing shows nothing
    rig.slots[0].blend = BlendMode::Additive;
    rig.slots[0].color = [1.0; 4];
    let px = rig.draw(&rig.pose(0, None, 0.0), 100, 100, view, 1, None);
    assert!(px[(50 * 100 + 50) * 4 + 3] > 200);
    rig.slots[0].blend = BlendMode::Multiply;
    let px = rig.draw(&rig.pose(0, None, 0.0), 100, 100, view, 1, None);
    assert_eq!(px[(50 * 100 + 50) * 4 + 3], 0);
}

#[test]
fn clipping_cuts_what_is_drawn_after_it() {
    // A clip polygon covering the left half of the box (x < 10) between slot c and slot s.
    let src = r#"{"skeleton":{"spine":"4.1.24"},"bones":[{"name":"root"}],
      "slots":[{"name":"c","bone":"root","attachment":"clip"},{"name":"s","bone":"root","attachment":"box"}],
      "skins":[{"name":"default","attachments":{"c":{"clip":{"type":"clipping","end":"s","vertexCount":4,"vertices":[-100,-100,10,-100,10,100,-100,100]}},"s":{"box":{"width":100,"height":100}}}}]}"#;
    let rig = with_page(Rig::from_json(src, ATLAS).unwrap(), [255, 255, 255, 255]);
    let px = rig.draw(&rig.pose(0, None, 0.0), 200, 200, View { cx: 0.0, cy: 0.0, scale: 1.0 }, 1, None);
    let a = |x: usize| px[(100 * 200 + x) * 4 + 3];
    assert_eq!(a(80), 255, "inside the clip and the box");
    assert_eq!(a(130), 0, "box, but right of the clip");
}

#[test]
fn rotated_and_trimmed_atlas_regions_map_to_the_right_texels() {
    // A 4x2 page: left half red, right half blue. The region "box" is the blue half stored rotated.
    let atlas = "t.png\nsize:4,2\nbox\nbounds:2,0,1,2\nrotate:90\n";
    let src = r#"{"skeleton":{"spine":"4.1.24"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root","attachment":"box"}],
      "skins":[{"name":"default","attachments":{"s":{"box":{"width":20,"height":10}}}}]}"#;
    let mut rig = Rig::from_json(src, atlas).unwrap();
    let mut px = Vec::new();
    for _y in 0..2 {
        for x in 0..4 {
            px.extend(if x < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
        }
    }
    rig.atlas.pages[0].image = Some(Arc::new(PageImage { width: 4, height: 2, rgba: px }));
    let out = rig.draw(&rig.pose(0, None, 0.0), 40, 20, View { cx: 0.0, cy: 0.0, scale: 1.0 }, 1, None);
    let c = &out[(10 * 40 + 20) * 4..(10 * 40 + 20) * 4 + 4];
    assert_eq!(c, &[0, 0, 255, 255], "only blue texels are sampled");
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let src = r#"{"skeleton":{"spine":"4.2.33"},"bones":[{"name":"root"}],"slots":[],"path":[{"name":"p"}],"physics":[{"name":"ph","bone":"root"}]}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    assert_eq!(rig.physics.len(), 1, "a physics constraint of 4.2 is read now");
    assert!(rig.path.is_empty(), "a path constraint without slot and bones is dropped, not an error");
    assert!(Rig::from_json("{}", ATLAS).is_err());
    assert!(Rig::from_json("not json", ATLAS).is_err());
    assert!(Rig::from_json(r#"{"skeleton":{},"bones":[{"name":"a","parent":"zzz"}]}"#, ATLAS).is_err());
}

/// A picture with four coloured quarters, packed into an atlas turned `degrees` counter-clockwise, comes out upright.
#[test]
fn regions_packed_at_every_quarter_turn_come_out_upright() {
    const R: [u8; 4] = [255, 0, 0, 255];
    const G: [u8; 4] = [0, 255, 0, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const Y: [u8; 4] = [255, 255, 0, 255];
    // (degrees, page size, page pixels row by row). The original is 4 x 2: top row R R G G, bottom row B B Y Y.
    let cases: [(u32, (usize, usize), Vec<[u8; 4]>); 4] = [
        (0, (4, 2), vec![R, R, G, G, B, B, Y, Y]),
        (90, (2, 4), vec![G, Y, G, Y, R, B, R, B]),
        (180, (4, 2), vec![Y, Y, B, B, G, G, R, R]),
        (270, (2, 4), vec![B, R, B, R, Y, G, Y, G]),
    ];
    for (deg, (pw, ph), px) in cases {
        let atlas = format!("t.png\nsize:{pw},{ph}\npma:false\nbox\nbounds:0,0,4,2\n{}", if deg == 0 { String::new() } else { format!("rotate:{deg}\n") });
        let src = r#"{"skeleton":{"spine":"4.2.33"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root","attachment":"box"}],
          "skins":[{"name":"default","attachments":{"s":{"box":{"width":40,"height":20}}}}]}"#;
        let mut rig = Rig::from_json(src, &atlas).unwrap();
        rig.atlas.pages[0].image = Some(Arc::new(PageImage { width: pw as u32, height: ph as u32, rgba: px.iter().flatten().copied().collect() }));
        let out = rig.draw(&rig.pose(0, None, 0.0), 40, 20, View { cx: 0.0, cy: 0.0, scale: 1.0 }, 1, None);
        let at = |x: usize, y: usize| [out[(y * 40 + x) * 4], out[(y * 40 + x) * 4 + 1], out[(y * 40 + x) * 4 + 2], out[(y * 40 + x) * 4 + 3]];
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() < 40);
        let got = (at(5, 5), at(35, 5), at(5, 15), at(35, 15));
        assert!(near(got.0, R) && near(got.1, G) && near(got.2, B) && near(got.3, Y), "packed {deg} degrees: top-left, top-right, bottom-left, bottom-right are {got:?}");
    }
}

/// The same picture as a mesh.
#[test]
fn meshes_on_regions_packed_at_every_quarter_turn_come_out_upright() {
    const R: [u8; 4] = [255, 0, 0, 255];
    const G: [u8; 4] = [0, 255, 0, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const Y: [u8; 4] = [255, 255, 0, 255];
    let cases: [(u32, (usize, usize), Vec<[u8; 4]>); 4] = [
        (0, (4, 2), vec![R, R, G, G, B, B, Y, Y]),
        (90, (2, 4), vec![G, Y, G, Y, R, B, R, B]),
        (180, (4, 2), vec![Y, Y, B, B, G, G, R, R]),
        (270, (2, 4), vec![B, R, B, R, Y, G, Y, G]),
    ];
    for (deg, (pw, ph), px) in cases {
        let atlas = format!("t.png\nsize:{pw},{ph}\npma:false\nbox\nbounds:0,0,4,2\n{}", if deg == 0 { String::new() } else { format!("rotate:{deg}\n") });
        // A quad (uvs 0..1 over the picture, v down) 40 x 20 around the origin.
        let src = r#"{"skeleton":{"spine":"4.2.33"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root","attachment":"box"}],
          "skins":[{"name":"default","attachments":{"s":{"box":{"type":"mesh","uvs":[0,1,0,0,1,0,1,1],"triangles":[0,1,2,2,3,0],"vertices":[-20,-10,-20,10,20,10,20,-10],"hull":4,"width":40,"height":20}}}}]}"#;
        let mut rig = Rig::from_json(src, &atlas).unwrap();
        rig.atlas.pages[0].image = Some(Arc::new(PageImage { width: pw as u32, height: ph as u32, rgba: px.iter().flatten().copied().collect() }));
        let out = rig.draw(&rig.pose(0, None, 0.0), 40, 20, View { cx: 0.0, cy: 0.0, scale: 1.0 }, 1, None);
        let at = |x: usize, y: usize| [out[(y * 40 + x) * 4], out[(y * 40 + x) * 4 + 1], out[(y * 40 + x) * 4 + 2], out[(y * 40 + x) * 4 + 3]];
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(x, y)| (*x as i32 - y as i32).abs() < 40);
        let got = (at(5, 5), at(35, 5), at(5, 15), at(35, 15));
        assert!(near(got.0, R) && near(got.1, G) && near(got.2, B) && near(got.3, Y), "mesh, packed {deg} degrees: {got:?}");
    }
}

#[test]
fn easing_from_another_animation_lands_between_the_two_poses() {
    let rig = Rig::from_json(&skeleton("4.2.33"), ATLAS).unwrap();
    let a = rig.animation_index("turn").unwrap();
    // From the arm at 0 degrees (turn at 0 s) to the arm at 90 (turn at 1 s): half way is 45, a full alpha is 90, none is 0.
    let at = |alpha: f32| angle(&rig.pose_mix(0, Some((a, 0.0)), Some(a), 1.0, alpha, None), 1);
    assert!((at(0.5) - 45.0).abs() < 0.01, "{}", at(0.5));
    assert!((at(0.0) - 0.0).abs() < 0.01);
    assert!((at(1.0) - 90.0).abs() < 0.01);
}

#[test]
fn a_two_colour_tint_turns_black_texels_into_the_dark_colour() {
    // A page that is half black, half white; slot light colour white, dark colour red: the black half comes out red, the white half white.
    let atlas = "t.png\nsize:2,1\npma:false\nbox\nbounds:0,0,2,1\n";
    let src = r#"{"skeleton":{"spine":"4.2.33"},"bones":[{"name":"root"}],"slots":[{"name":"s","bone":"root","attachment":"box","dark":"ff0000"}],
      "skins":[{"name":"default","attachments":{"s":{"box":{"width":40,"height":20}}}}]}"#;
    let mut rig = Rig::from_json(src, atlas).unwrap();
    rig.atlas.pages[0].image = Some(Arc::new(PageImage { width: 2, height: 1, rgba: vec![0, 0, 0, 255, 255, 255, 255, 255] }));
    let out = rig.draw(&rig.pose(0, None, 0.0), 40, 20, View { cx: 0.0, cy: 0.0, scale: 1.0 }, 1, None);
    let at = |x: usize| [out[(10 * 40 + x) * 4], out[(10 * 40 + x) * 4 + 1], out[(10 * 40 + x) * 4 + 2]];
    let (left, right) = (at(4), at(35));
    assert!(left[0] > 200 && left[1] < 60 && left[2] < 60, "black texels are the dark colour: {left:?}");
    assert!(right[0] > 200 && right[1] > 200 && right[2] > 200, "white texels stay light: {right:?}");
}

/// A 4.3 transform constraint: one bone's x copied to another's (a property mapping, no fixed "translate mix").
#[test]
fn a_43_transform_constraint_copies_the_mapped_property() {
    let src = r#"{"skeleton":{"spine":"4.3.40"},"bones":[{"name":"root"},{"name":"a","parent":"root","x":100,"y":30},{"name":"b","parent":"root"}],"slots":[],
      "constraints":[{"type":"transform","name":"t","bones":["b"],"source":"a","properties":{"x":{"to":{"x":{}}}}}]}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    assert_eq!(rig.transform.len(), 1);
    assert!(rig.transform[0].v43.is_some());
    let p = rig.pose(0, None, 0.0);
    let (b, a) = (rig.bone_index("b").unwrap(), rig.bone_index("a").unwrap());
    assert!((p.bones[b].wx - 100.0).abs() < 1e-3, "x copied: {}", p.bones[b].wx);
    assert!(p.bones[b].wy.abs() < 1e-3, "y was not mapped: {}", p.bones[b].wy);
    assert!((p.bones[a].wx - 100.0).abs() < 1e-3);
}

/// A 4.3 slider turns a bone property into the time of an animation.
#[test]
fn a_slider_plays_its_animation_at_the_time_a_bone_property_gives() {
    let src = r#"{"skeleton":{"spine":"4.3.40"},"bones":[{"name":"root"},{"name":"ctl","parent":"root","rotation":45},{"name":"target","parent":"root"}],"slots":[],
      "constraints":[{"type":"slider","name":"s","animation":"follow","bone":"ctl","property":"rotate","scale":0.0111111111,"local":true}],
      "animations":{"follow":{"bones":{"target":{"rotate":[{"time":0,"value":0},{"time":1,"value":90}]}}}}}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    assert_eq!(rig.slider.len(), 1);
    assert!(rig.slider[0].animation.is_some());
    let p = rig.pose(0, None, 0.0);
    let t = rig.bone_index("target").unwrap();
    assert!((angle(&p, t) - 45.0).abs() < 0.1, "half way through the animation: {}", angle(&p, t));
}

/// A physics constraint lets a bone lag behind its parent's move and settle again; without a simulation it stays rigid.
#[test]
fn physics_lags_and_settles() {
    let src = r#"{"skeleton":{"spine":"4.2.40"},"bones":[{"name":"root"},{"name":"child","parent":"root","x":50}],"slots":[],
      "physics":[{"name":"p","bone":"child","x":1,"inertia":1,"strength":100,"damping":0.85}],
      "animations":{"go":{"bones":{"root":{"translate":[{"time":0,"x":0},{"time":0.5,"x":200}]}}}}}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    assert_eq!(rig.physics.len(), 1);
    let go = rig.animation_index("go").unwrap();
    let child = rig.bone_index("child").unwrap();
    let mut sim = PhysicsSim::new();
    let (mut lag_seen, mut last) = (0.0f32, 0.0f32);
    for k in 0..=240 {
        let t = (k as f32 / 60.0).min(0.5);
        sim.advance(1.0 / 60.0);
        let p = rig.pose_sim(0, Some(go), t, Some(&mut sim));
        let rigid = rig.pose(0, Some(go), t).bones[child].wx;
        lag_seen = lag_seen.max((rigid - p.bones[child].wx).abs());
        last = (p.bones[child].wx - rigid).abs();
        assert!(p.bones[child].wx.is_finite());
    }
    assert!(lag_seen > 3.0, "the bone should lag behind while the parent moves (max {lag_seen})");
    assert!(last < 1.0, "and settle after the parent stops (still {last} away after 4 s)");
}

#[test]
fn an_ik_stretch_measures_the_distance_in_the_parents_space() {
    // The root is scaled by 2; the target is 200 units away in the root's own space (400 in the world). The bone is 50 long, so it must
    // stretch by 200 / 50 = 4 in its local scale (the parent's 2 comes on top), not by 400 / 50.
    let src = r#"{"skeleton":{"spine":"4.1.24"},
      "bones":[{"name":"root","scaleX":2,"scaleY":2},{"name":"a","parent":"root","length":50},{"name":"tgt","parent":"root","x":200}],
      "slots":[],"ik":[{"name":"ik","bones":["a"],"target":"tgt","stretch":true}]}"#;
    let p = Rig::from_json(src, ATLAS).unwrap().pose(0, None, 0.0);
    assert!((p.bones[1].scale_x - 4.0).abs() < 1e-3, "local scale {}", p.bones[1].scale_x);
}

#[test]
fn local_and_relative_transform_constraints_work_on_the_bones_own_values() {
    // f hangs on a parent that sits at x = 100; the driver is at x = 30 on the root and f itself at x = 10 on its parent.
    let rig = |flags: &str| {
        let src = format!(
            r#"{{"skeleton":{{"spine":"4.1.24"}},
          "bones":[{{"name":"root"}},{{"name":"p","parent":"root","x":100}},{{"name":"driver","parent":"root","x":30}},{{"name":"f","parent":"p","x":10}}],
          "slots":[],"transform":[{{"name":"t","bones":["f"],"target":"driver","mixRotate":0,"mixX":1,"mixScaleX":0,"mixShearY":0{flags}}}]}}"#
        );
        Rig::from_json(&src, ATLAS).unwrap().pose(0, None, 0.0)
    };
    // World, absolute: f goes to where the driver is.
    assert!((rig("").bones[3].wx - 30.0).abs() < 1e-3);
    // Local, absolute: f takes the driver's x as its own x on its parent (30 on top of the parent's 100).
    assert!((rig(r#","local":true"#).bones[3].wx - 130.0).abs() < 1e-3);
    // Local, relative: the driver's x is added to f's (10 + 30).
    assert!((rig(r#","local":true,"relative":true"#).bones[3].wx - 140.0).abs() < 1e-3);
    // World, relative: the driver's world x is added to f's world position (110 + 30).
    assert!((rig(r#","relative":true"#).bones[3].wx - 140.0).abs() < 1e-3);
}

#[test]
fn a_mix_animation_plays_over_the_idle_with_the_same_number() {
    let src = r#"{"skeleton":{"spine":"4.1.24"},
      "bones":[{"name":"root"},{"name":"a","parent":"root","length":10},{"name":"b","parent":"root","length":10}],
      "slots":[],
      "animations":{
        "idle1":{"bones":{"a":{"rotate":[{"time":0,"value":0},{"time":1,"value":90},{"time":2,"value":0}]}}},
        "mix1_2_1":{"bones":{"b":{"rotate":[{"time":0,"value":45}]}}},
        "mix2_1_1":{"bones":{"b":{"rotate":[{"time":0,"value":45}]}}}}}"#;
    let rig = Rig::from_json(src, ATLAS).unwrap();
    let (idle, mix, lone) = (rig.animation_index("idle1").unwrap(), rig.animation_index("mix1_2_1").unwrap(), rig.animation_index("mix2_1_1").unwrap());
    assert_eq!(rig.base_layer(mix), Some(idle));
    assert_eq!(rig.base_layer(idle), None);
    assert_eq!(rig.base_layer(lone), None, "no idle2: nothing underneath");
    let p = rig.pose(0, Some(mix), 0.5);
    assert!((angle(&p, 2) - 45.0).abs() < 0.01, "the mix turns b");
    assert!((angle(&p, 1) - 45.0).abs() < 0.01, "the idle underneath turns a: {}", angle(&p, 1));
    // The idle loops while the mix is held: at t = 2.5 it is where it is at 0.5.
    let later = rig.pose(0, Some(mix), 2.5);
    assert!((angle(&later, 1) - 45.0).abs() < 0.01, "{}", angle(&later, 1));
    let p = rig.pose(0, Some(lone), 0.5);
    assert!(angle(&p, 1).abs() < 0.01);
}
