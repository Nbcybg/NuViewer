//! Typed numbers may be far outside a slider's range: no setting, at any extreme, may panic or break the picture's size.

use nv_editor::engine;
use nv_editor::raster::Pixels;
use nv_editor::recipe::{Recipe, PARAMS, TYPED_LIMIT};

fn picture() -> Pixels {
    let (w, h) = (48u32, 32u32);
    let mut rgba = Vec::new();
    for y in 0..h {
        for x in 0..w {
            rgba.extend([(x * 5) as u8, (y * 7) as u8, ((x + y) * 3) as u8, 255]);
        }
    }
    Pixels::new(w, h, rgba)
}

#[test]
fn every_slider_survives_extreme_typed_values() {
    let p = picture();
    for i in 0..PARAMS.len() {
        for v in [TYPED_LIMIT, -TYPED_LIMIT, 1000.0, -1000.0, 0.0] {
            let mut r = Recipe::default();
            r.set_index(i, v);
            let out = std::panic::catch_unwind(|| engine::render(&p, &r)).unwrap_or_else(|_| panic!("{} = {v} panicked", PARAMS[i].key));
            // Geometry sliders (angle, crop-like) may change the size; the buffer must still be consistent.
            assert_eq!(out.rgba.len(), out.w as usize * out.h as usize * 4, "{} = {v}", PARAMS[i].key);
            assert!(out.w > 0 && out.h > 0, "{} = {v}", PARAMS[i].key);
        }
    }
}

#[test]
fn nan_and_infinity_become_the_default() {
    let mut r = Recipe::default();
    r.set_index(0, f32::NAN);
    assert_eq!(r.v[0], PARAMS[0].default);
    r.set_index(0, f32::INFINITY);
    assert_eq!(r.v[0], PARAMS[0].default);
}
