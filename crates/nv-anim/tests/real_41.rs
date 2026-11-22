//! A real Spine 4.1.11 skeleton from a game (2218 bones, 1030 slots, 112 IK and 356 transform constraints, 100 animations, 5 MB).
//! The file is the user's own and is not in the repository: the test is skipped when it is not there (set NV_REAL_SPINE41 to another path).
use nv_anim::{binary, Atlas};

#[test]
fn a_real_4_1_skeleton_reads_to_the_last_byte_and_poses_without_blowing_up() {
    let path = std::env::var("NV_REAL_SPINE41").unwrap_or_else(|_| r"T:\Benchmark-folder\Bright_Moon\illust_dating20.skel".into());
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("{path} not found: skipped");
        return;
    };
    let atlas = Atlas::parse("page.png\nsize:8,8\nr\nbounds:0,0,1,1\n").unwrap();
    let rig = binary::parse(&bytes, atlas).expect("the real file reads");
    assert_eq!(rig.version, "4.1.11");
    assert_eq!((rig.bones.len(), rig.slots.len(), rig.ik.len(), rig.transform.len(), rig.animations.len()), (2218, 1030, 112, 356, 100));
    // every animation poses at its start, middle and end with finite numbers (parts parked 100000000 away are legitimate)
    for (i, a) in rig.animations.iter().enumerate() {
        for k in 0..3 {
            let pose = rig.pose(0, Some(i), a.duration * k as f32 / 2.0);
            assert!(pose.bones.iter().all(|b| b.a.is_finite() && b.d.is_finite() && b.wx.is_finite() && b.wy.is_finite()), "{} at {k}", a.name);
        }
    }
}
