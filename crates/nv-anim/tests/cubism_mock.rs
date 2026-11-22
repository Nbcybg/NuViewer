//! The Cubism binding run against the mock Core (`tools/mock-cubism`): proves the calls, structs and arrays line up with the documented C API.
//! Skipped when the mock has not been built (`cargo build -p mock-cubism`, same target dir).
use nv_anim::cubism::{Core, Model};
use nv_anim::live2d::items_from_drawables;
use nv_anim::{render_pages, PageImage, View};
use std::sync::Arc;

fn mock() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|e| e.parent()?.parent().map(|p| p.to_path_buf()))).unwrap_or_default();
    [dir.join("debug").join("Live2DCubismCore.dll"), dir.join("Live2DCubismCore.dll")].into_iter().find(|p| p.is_file())
}

fn moc() -> Vec<u8> {
    let mut v = b"MOC3".to_vec();
    v.resize(128, 0);
    v
}

#[test]
fn the_binding_reads_parameters_parts_and_drawables_and_follows_changes() {
    let Some(path) = mock() else {
        eprintln!("mock Core not built: skipped");
        return;
    };
    let core = Arc::new(unsafe { Core::load_from(&path) }.expect("the mock loads"));
    assert_eq!(core.version_text(), "5.0.0");
    assert!(Model::new(core.clone(), b"not a moc").is_err());
    let mut m = Model::new(core.clone(), &moc()).expect("a model");
    assert_eq!(m.params.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["ParamAngleX", "ParamOpen"]);
    assert_eq!((m.params[0].min, m.params[0].max), (-30.0, 30.0));
    assert_eq!(m.parts, vec!["PartBody", "PartHead"]);
    assert_eq!((m.canvas_size, m.pixels_per_unit), ([100.0, 100.0], 100.0));
    let d = m.drawables();
    assert_eq!(d.len(), 4);
    assert_eq!(d[1].id, "Head");
    assert_eq!(d[3].masks, vec![2]);
    assert!(!d[2].visible && d[0].visible);
    assert_eq!(d[1].verts.len(), 4);
    assert_eq!(d[1].indices, vec![0, 1, 2, 0, 2, 3]);
    let x0 = d[1].verts[0][0];
    // a parameter moves the head; a part's opacity changes the head's opacity
    let i = m.param_index("ParamAngleX").unwrap();
    m.set_param(i, 30.0);
    m.set_param(i, 99.0); // clamped to the maximum
    m.set_part_opacity(m.part_index("PartHead").unwrap(), 0.5);
    m.update();
    let d = m.drawables();
    assert!((d[1].verts[0][0] - (x0 + 0.3)).abs() < 1e-5, "{} vs {}", d[1].verts[0][0], x0);
    assert!((d[1].opacity - 0.5).abs() < 1e-6);
}

#[test]
fn what_the_core_gives_is_drawn_with_its_mask() {
    let Some(path) = mock() else { return };
    let core = Arc::new(unsafe { Core::load_from(&path) }.unwrap());
    let mut m = Model::new(core, &moc()).unwrap();
    let items = items_from_drawables(&m.drawables());
    // Body, then Head (render orders 1 and 2), then Eye (3); the hidden Mask is not drawn.
    assert_eq!(items.iter().map(|i| i.slot).collect::<Vec<_>>(), vec![0, 1, 3]);
    let page = vec![Some(Arc::new(PageImage { width: 2, height: 2, rgba: vec![255; 16] }))];
    let px = render_pages(&page, &items[2..], 100, 100, View { cx: 0.0, cy: 0.0, scale: 100.0 }, 1, None);
    let a = |x: usize| px[(50 * 100 + x) * 4 + 3];
    assert_eq!(a(35), 255, "the eye is drawn where the mask is (left of the centre)");
    assert_eq!(a(65), 0, "and cut away where it is not");
}
