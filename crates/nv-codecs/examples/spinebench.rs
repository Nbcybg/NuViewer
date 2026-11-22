//! `spinebench <skeleton> [--png prefix]`: time the Spine rasteriser at several canvas sizes (1x and 2x supersampling) and save a
//! picture of each, so sharpness can be compared by eye. The frame is the first animation at a quarter of its length.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: spinebench <skeleton> [--png prefix]");
        return;
    };
    let prefix = args.iter().position(|a| a == "--png").and_then(|i| args.get(i + 1)).cloned();
    let rig = nv_codecs::spinefmt::load(std::path::Path::new(path)).expect("load");
    let anim = if rig.animations.is_empty() { None } else { Some(0) };
    let t = anim.map(|a| rig.animations[a].duration * 0.25).unwrap_or(0.0);
    {
        let pose = rig.pose(0, anim, t);
        let t0 = std::time::Instant::now();
        let items = rig.geometry(&pose);
        let tris: usize = items.iter().map(|i| i.tris.len() / 3).sum();
        println!("pose + geometry only: {:.2} ms ({} items, {} triangles)", t0.elapsed().as_secs_f64() * 1000.0, items.len(), tris);
        let t0 = std::time::Instant::now();
        let _ = rig.pose(0, anim, t);
        println!("pose only: {:.2} ms", t0.elapsed().as_secs_f64() * 1000.0);
    }
    for max in [1024usize, 1920, 2560, 3840] {
        for ss in [1usize, 2] {
            let (w, h, view) = nv_codecs::spinefmt::canvas_for(&rig, 0, anim, max);
            let pose = rig.pose(0, anim, t);
            let t0 = std::time::Instant::now();
            let n = 5;
            let mut rgba = Vec::new();
            for _ in 0..n {
                rgba = rig.draw(&pose, w, h, view, ss, None);
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!("canvas {w}x{h} ssaa {ss}: {ms:.1} ms / frame");
            if let Some(p) = &prefix {
                let f = std::fs::File::create(format!("{p}_{max}_ss{ss}.png")).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&rgba).unwrap();
            }
        }
    }
}
