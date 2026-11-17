//! Dev tool: how long the editor's engine takes on a picture for a few typical recipes (median of 5 runs).
//! `cargo run --release -p nv-editor --example bench_render [width height]`   (default 1600 x 1200, the size of a window proxy)
use nv_editor::engine;
use nv_editor::recipe::{Recipe, P};
use nv_editor::Pixels;
use std::time::Instant;

fn picture(w: u32, h: u32) -> Pixels {
    // Photo-like: mostly moderate colours with a gentle structure, and a vivid band (a saturated sky or flower) on the right fifth.
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let vivid = if fx > 0.8 { 1.0 } else { 0.0 };
            let r = 120.0 + 70.0 * (fx * 6.0).sin() + vivid * 80.0;
            let g = 70.0 + 110.0 * fy - vivid * 60.0;
            let b = 110.0 + 60.0 * ((fx + fy) * 5.0).cos() - vivid * 30.0;
            rgba[i] = r.clamp(0.0, 255.0) as u8;
            rgba[i + 1] = g.clamp(0.0, 255.0) as u8;
            rgba[i + 2] = b.clamp(0.0, 255.0) as u8;
        }
    }
    Pixels::new(w, h, rgba)
}

fn time(name: &str, src: &Pixels, r: &Recipe) {
    let mut v: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            let out = engine::render(src, r);
            std::hint::black_box(&out);
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    v.sort_by(|a, b| a.total_cmp(b));
    println!("{name:<44} {:7.1} ms (min {:.1})", v[2], v[0]);
}

fn single_thread(src: &Pixels) {
    use nv_editor::huecurves::{HueCurves, HueStage};
    let mut h = HueCurves::default();
    h.hue = vec![(30.0, 0.1), (200.0, -0.1)];
    let st = HueStage::new(&h).unwrap();
    let t = Instant::now();
    let mut acc = 0.0f32;
    for p in src.rgba.chunks_exact(4) {
        let o = st.apply_exact([p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]);
        acc += o[0] + o[1] + o[2];
    }
    let ns = t.elapsed().as_secs_f64() * 1e9 / (src.rgba.len() / 4) as f64;
    println!("HueStage::apply alone, one thread: {ns:.1} ns per pixel ({acc:.0})");
}

fn main() {
    let a: Vec<u32> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let (w, h) = if a.len() >= 2 { (a[0], a[1]) } else { (1600, 1200) };
    let src = picture(w, h);
    println!("{w} x {h} ({:.1} MP)", (w * h) as f64 / 1e6);
    single_thread(&src);
    time("no edit", &src, &Recipe::default());
    let mut r = Recipe::default();
    r.set(P::Exposure, 0.5);
    r.set(P::Contrast, 20.0);
    time("exposure + contrast (three tables)", &src, &r);
    r.set(P::Saturation, 25.0);
    r.set(P::Vibrance, 20.0);
    time("+ saturation + vibrance (general path)", &src, &r);
    let mut with_wheel = r.clone();
    with_wheel.hue_wheel[3] = nv_editor::recipe::Band { hue: 30.0, sat: -20.0, lum: 10.0 };
    time("+ hue wheel", &src, &with_wheel);
    let mut hue_only = r.clone();
    hue_only.hue_curves.hue = vec![(30.0, 0.1), (200.0, -0.1)];
    time("+ hue curve only, small (rarely out of gamut)", &src, &hue_only);
    let mut with_curves = r.clone();
    with_curves.hue_curves.hue = vec![(30.0, 0.3), (200.0, -0.2)];
    with_curves.hue_curves.sat = vec![(120.0, 0.4)];
    time("+ hue curves (OKLCH)", &src, &with_curves);
    let mut with_warp = r.clone();
    with_warp.hue_curves.set_node(2, 3, 25.0, -0.1);
    time("+ warper (OKLCH)", &src, &with_warp);
}
