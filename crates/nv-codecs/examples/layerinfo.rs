//! `layerinfo <file>`: the layers `nv_codecs::layerdoc::read` finds, the time it took, and a flattened PNG (`--png out.png`).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: layerinfo <file> [--png out.png]");
        return;
    };
    let t = std::time::Instant::now();
    let doc = match nv_codecs::layerdoc::read(std::path::Path::new(path)) {
        Ok(d) => d,
        Err(e) => {
            println!("no layers: {e}");
            return;
        }
    };
    println!("{} {}x{}: {} layers read in {:.0} ms, {:.1} MB of pixels", doc.format, doc.width, doc.height, doc.layers.len(), t.elapsed().as_secs_f64() * 1000.0, doc.bytes() as f64 / 1e6);
    println!("note: {}", doc.note);
    for (i, l) in doc.layers.iter().enumerate() {
        println!("{i:3} parent={:?} {} {} op={:.2} {} at {},{} {}x{} {}", l.parent, if l.group { "group" } else { "layer" }, if l.visible { "on " } else { "off" }, l.opacity, l.mode.name(), l.x, l.y, l.w, l.h, l.name);
    }
    let eff = doc.default_eff();
    let t = std::time::Instant::now();
    let img = nv_codecs::layerdoc::flatten(&doc, &eff);
    println!("flattened in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    // How close is it to the picture the file itself stores?
    if let Ok(merged) = nv_codecs::decode_file(std::path::Path::new(path)) {
        if merged.width == img.width && merged.height == img.height {
            let mut sum = 0u64;
            let mut n = 0u64;
            for (a, b) in merged.rgba.chunks_exact(4).zip(img.rgba.chunks_exact(4)) {
                if a[3] == 0 && b[3] == 0 {
                    continue;
                }
                n += 1;
                for k in 0..4 {
                    sum += (a[k] as i32 - b[k] as i32).unsigned_abs() as u64;
                }
            }
            let mut worst = (0u32, 0usize);
            for (i, (a, b)) in merged.rgba.chunks_exact(4).zip(img.rgba.chunks_exact(4)).enumerate() {
                let d: u32 = (0..4).map(|k| (a[k] as i32 - b[k] as i32).unsigned_abs()).sum();
                if d > worst.0 {
                    worst = (d, i);
                }
            }
            let (x, y) = (worst.1 % img.width as usize, worst.1 / img.width as usize);
            println!("worst pixel ({x},{y}): stored {:?} ours {:?}", &merged.rgba[worst.1 * 4..worst.1 * 4 + 4], &img.rgba[worst.1 * 4..worst.1 * 4 + 4]);
            let (mut mx, mut ox) = ([0u64; 4], [0u64; 4]);
            let mut cnt = 0u64;
            for (a, b) in merged.rgba.chunks_exact(4).zip(img.rgba.chunks_exact(4)) {
                if a[3] > 0 {
                    cnt += 1;
                    for k in 0..4 {
                        mx[k] += a[k] as u64;
                        ox[k] += b[k] as u64;
                    }
                }
            }
            println!("mean over stored-opaque pixels: stored {:?} ours {:?}", mx.map(|v| v / cnt.max(1)), ox.map(|v| v / cnt.max(1)));
            println!("mean difference to the stored merged picture: {:.2} levels over {n} pixels with content", sum as f64 / (n.max(1) * 4) as f64);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--png") {
        if let Some(out) = args.get(i + 1) {
            let f = std::fs::File::create(out).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.width, img.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&img.rgba).unwrap();
            println!("wrote {out}");
        }
    }
}
