//! Real skeletons of every Spine version from 3.8 to 4.3 (the example assets of github.com/playcanvas/playcanvas-spine, kept outside the
//! repository because they belong to Esoteric Software). The tests skip themselves when the folder is absent.
//!
//! Folder: `NUVIEWER_SPINE_SAMPLES`, else `T:\Benchmark-folder\spine-samples` (the layout is `spine-4.3/tank/tank-pro.json` and so on).
use nv_anim::{binary, json, Atlas, CKind, PhysicsSim, Rig};
use std::path::PathBuf;

fn root() -> Option<PathBuf> {
    let p = std::env::var_os("NUVIEWER_SPINE_SAMPLES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"T:\Benchmark-folder\spine-samples"));
    p.is_dir().then_some(p)
}

fn atlas_of(dir: &std::path::Path, stem: &str) -> Atlas {
    let text = std::fs::read_to_string(dir.join(format!("{stem}-pma.atlas"))).unwrap();
    Atlas::parse(&text).unwrap()
}

fn load_json(rel: &str, stem: &str) -> Option<Rig> {
    let dir = root()?.join(rel);
    let src = std::fs::read_to_string(dir.join(format!("{stem}-pro.json"))).ok()?;
    Some(json::parse(&src, atlas_of(&dir, stem)).unwrap_or_else(|e| panic!("{rel}: {e}")))
}

fn load_skel(rel: &str, stem: &str) -> Option<Rig> {
    let dir = root()?.join(rel);
    let bytes = std::fs::read(dir.join(format!("{stem}-pro.skel"))).ok()?;
    Some(binary::parse(&bytes, atlas_of(&dir, stem)).unwrap_or_else(|e| panic!("{rel}: {e}")))
}

const ALL: &[(&str, &str)] = &[
    ("spine-3.8/spineboy", "spineboy"),
    ("spine-4.0/owl", "owl"),
    ("spine-4.0/raptor", "raptor"),
    ("spine-4.1/raptor", "raptor"),
    ("spine-4.2/spineboy", "spineboy"),
    ("spine-4.2/celestial-circus", "celestial-circus"),
    ("spine-4.3/celestial-circus", "celestial-circus"),
    ("spine-4.3/diamond", "diamond"),
    ("spine-4.3/snowglobe", "snowglobe"),
    ("spine-4.3/tank", "tank"),
];

#[test]
fn every_version_reads_and_every_animation_poses_without_nan() {
    for (rel, stem) in ALL {
        let Some(rig) = load_json(rel, stem) else { return };
        assert!(!rig.animations.is_empty(), "{rel}: no animations");
        for (ai, anim) in rig.animations.iter().enumerate() {
            for k in 0..4 {
                let t = anim.duration * k as f32 / 3.0;
                let pose = rig.pose(0, Some(ai), t);
                for (bi, b) in pose.bones.iter().enumerate() {
                    // Game rigs park parts at 1e8; only NaN is a mistake.
                    assert!(b.wx.is_finite() && b.wy.is_finite() && b.a.is_finite() && b.d.is_finite(), "{rel}: animation {} t={t}: bone {} ({}) is not finite", anim.name, bi, rig.bones[bi].name);
                }
                let items = rig.geometry(&pose);
                // (A fade-out animation may end on nothing.)
                assert!(k > 0 || !items.is_empty(), "{rel}: animation {} draws nothing at its start", anim.name);
            }
        }
    }
}

#[test]
fn spine_43_constraints_are_all_read() {
    let Some(tank) = load_json("spine-4.3/tank", "tank") else { return };
    assert_eq!(tank.ik.len(), 2);
    assert_eq!(tank.path.len(), 1, "the tread path");
    assert_eq!(tank.transform.len(), 6);
    assert!(tank.transform.iter().all(|t| t.v43.is_some()));
    assert_eq!(tank.constraints.len(), 9);
    assert_eq!(tank.constraints[2].kind, CKind::Path);
    let tp = &tank.path[0];
    assert_eq!(tp.bones.len(), 36);
    let snow = load_json("spine-4.3/snowglobe", "snowglobe").unwrap();
    assert!(snow.physics.len() > 40, "physics constraints: {}", snow.physics.len());
    let dia = load_json("spine-4.3/diamond", "diamond").unwrap();
    assert_eq!(dia.slider.len(), 1);
    assert!(dia.slider[0].animation.is_some());
    assert!(dia.slider[0].looped && dia.slider[0].bone.is_some());
}

/// The tread bones of the tank are placed along the path: moving the path position moves them.
#[test]
fn tank_treads_follow_the_path() {
    let Some(tank) = load_json("spine-4.3/tank", "tank") else { return };
    let drive = tank.animation_index("drive").unwrap();
    let a = tank.pose(0, Some(drive), 0.0);
    let b = tank.pose(0, Some(drive), 2.0);
    let tread = tank.bone_index("tread").unwrap();
    let moved = (a.bones[tread].wx - b.bones[tread].wx).abs() + (a.bones[tread].wy - b.bones[tread].wy).abs();
    assert!(moved > 5.0, "the first tread bone moved {moved}");
    // And the treads are not all piled up in one place.
    let xs: Vec<f32> = (0..36).map(|i| a.bones[tank.path[0].bones[i]].wx).collect();
    let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
    assert!(hi - lo > 100.0, "treads span only {}", hi - lo);
}

/// Physics: a still skeleton stays still, and one whose root moves makes the hanging bones swing.
#[test]
fn physics_makes_bones_swing_when_the_skeleton_moves() {
    let Some(rig) = load_json("spine-4.3/celestial-circus", "celestial-circus") else { return };
    let swing = rig.animation_index("swing").unwrap();
    let mut sim = PhysicsSim::new();
    let mut still_moves = 0.0f32;
    let first = rig.pose_sim(0, Some(swing), 0.0, Some(&mut sim));
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let bone = rig.physics[0].bone;
    for k in 1..240 {
        sim.advance(1.0 / 60.0);
        let p = rig.pose_sim(0, Some(swing), k as f32 / 60.0, Some(&mut sim));
        let b = &p.bones[bone];
        assert!(b.wx.is_finite() && b.a.is_finite(), "physics blew up at frame {k}");
        min_x = min_x.min(b.wx);
        max_x = max_x.max(b.wx);
        still_moves = still_moves.max((b.wx - first.bones[bone].wx).abs());
    }
    assert!(max_x - min_x > 0.01 || still_moves > 0.01, "nothing moved: {min_x}..{max_x}");
}

#[test]
fn diamond_slider_turns_the_diamond() {
    let Some(rig) = load_json("spine-4.3/diamond", "diamond") else { return };
    let idle = rig.animation_index("idle-rotating").unwrap();
    let a = rig.pose(0, Some(idle), 0.0);
    let b = rig.pose(0, Some(idle), 0.9);
    let differs = rig.slots.iter().enumerate().any(|(i, _)| {
        let (pa, pb) = (&a.slots[i], &b.slots[i]);
        pa.attachment != pb.attachment
    }) || a.bones.iter().zip(&b.bones).any(|(x, y)| (x.rotation - y.rotation).abs() > 0.01 || (x.scale_x - y.scale_x).abs() > 0.001);
    assert!(differs, "the idle animation changes nothing");
}

/// The binary 4.3 files read to the same rig as the JSON export beside them.
#[test]
fn binary_43_matches_json_43() {
    for (rel, stem) in [("spine-4.3/celestial-circus", "celestial-circus"), ("spine-4.3/diamond", "diamond"), ("spine-4.3/snowglobe", "snowglobe"), ("spine-4.3/tank", "tank")] {
        let (Some(j), Some(b)) = (load_json(rel, stem), load_skel(rel, stem)) else { return };
        assert_eq!(j.bones.len(), b.bones.len(), "{rel}: bones");
        assert_eq!(j.slots.len(), b.slots.len(), "{rel}: slots");
        assert_eq!((j.ik.len(), j.transform.len(), j.path.len(), j.physics.len(), j.slider.len()), (b.ik.len(), b.transform.len(), b.path.len(), b.physics.len(), b.slider.len()), "{rel}: constraints");
        assert_eq!(j.constraints, b.constraints, "{rel}: constraint order");
        let names = |r: &Rig| r.animations.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&j), names(&b), "{rel}: animations");
        for (x, y) in j.bones.iter().zip(&b.bones) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.parent, y.parent, "{rel}: parent of {}", x.name);
            for (p, q) in [(x.x, y.x), (x.y, y.y), (x.rotation, y.rotation), (x.scale_x, y.scale_x), (x.length, y.length)] {
                // (the JSON export rounds tiny values away)
                assert!((p - q).abs() < 1e-2 * (1.0 + p.abs() * 1e-3), "{rel}: bone {} differs: {p} vs {q}", x.name);
            }
        }
        // Every animation poses the same way, at a few times.
        for ai in 0..j.animations.len() {
            for k in 0..3 {
                let t = j.animations[ai].duration * k as f32 / 2.0;
                let (mut sa, mut sb) = (PhysicsSim::new(), PhysicsSim::new());
                let pj = j.pose_sim(0, Some(ai), t, Some(&mut sa));
                let pb = b.pose_sim(0, Some(ai), t, Some(&mut sb));
                for (bi, (x, y)) in pj.bones.iter().zip(&pb.bones).enumerate() {
                    let d = (x.wx - y.wx).abs() + (x.wy - y.wy).abs() + (x.a - y.a).abs() + (x.d - y.d).abs();
                    assert!(d < 0.05 * (1.0 + x.wx.abs().min(1e5)), "{rel}: animation {} t={t}: bone {} ({}) world differs by {d}", j.animations[ai].name, bi, j.bones[bi].name);
                }
            }
        }
    }
}

/// The animation a slider plays recolours slots too (a spinning gem changes the tint of its facets): the facets are not left white.
#[test]
fn diamond_slider_animation_tints_the_facets() {
    let Some(rig) = load_json("spine-4.3/diamond", "diamond") else { return };
    let idle = rig.animation_index("idle-rotating").unwrap();
    let facet = rig.slot_index("lower-side-2").unwrap();
    let a = rig.pose(0, Some(idle), 0.0).slots[facet].color;
    let b = rig.pose(0, Some(idle), 0.4).slots[facet].color;
    assert!(a[0] < 1.0 || a[1] < 1.0, "the facet carries the slider animation's colour at t = 0: {a:?}");
    assert!((a[1] - b[1]).abs() > 0.05, "and it changes as the gem turns: {a:?} {b:?}");
}
