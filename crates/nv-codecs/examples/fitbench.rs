//! Dev tool: time the first-frame resampler. `cargo run --release -p nv-codecs --example fitbench -- <file>... [--win WxH]`
fn main() {
    let mut win = (2560f32, 1440f32);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--win" {
            let (w, h) = args[i + 1].split_once('x').unwrap();
            win = (w.parse().unwrap(), h.parse().unwrap());
            i += 2;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }
    for f in files {
        let img = nv_codecs::decode_file(std::path::Path::new(&f)).expect("decode");
        let (dw, dh) = img.display_size();
        let s = (win.0 / dw as f32).min(win.1 / dh as f32).min(1.0);
        for round in 0..3 {
            let t = std::time::Instant::now();
            let r = nv_codecs::fit_like_gpu(&img, s);
            let a = t.elapsed().as_secs_f64() * 1000.0;
            let t = std::time::Instant::now();
            let edge = ((img.width.max(img.height) as f32) * s).round() as u32;
            let _ = nv_codecs::downscale(&img, edge);
            let b = t.elapsed().as_secs_f64() * 1000.0;
            println!("{}x{} -> {}x{} (scale {s:.3}) round {round}: fit_like_gpu {a:.0} ms, old area-average {b:.0} ms", img.width, img.height, r.width, r.height);
        }
    }
}
