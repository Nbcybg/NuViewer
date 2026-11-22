//! What is in a Unity bundle: `unityinfo <file|folder> [--png out.png] [--tex N]`. A folder is read file by file and summarised.
use nv_codecs::unityfs;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(target) = args.first() else {
        eprintln!("usage: unityinfo <bundle|folder> [--png out.png] [--tex N]");
        return;
    };
    let png = args.iter().position(|a| a == "--png").and_then(|i| args.get(i + 1)).cloned();
    let tex: usize = args.iter().position(|a| a == "--tex").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let path = Path::new(target);
    if path.is_dir() {
        let (mut ok, mut bad) = (0, 0);
        let t0 = std::time::Instant::now();
        for e in std::fs::read_dir(path).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| unityfs::is_ext(&x.to_string_lossy()))) {
            let bytes = std::fs::read(e.path()).unwrap();
            match unityfs::open(&bytes).and_then(|c| if c.textures.is_empty() { Ok(None) } else { c.texture(0).map(Some) }) {
                Ok(t) => {
                    ok += 1;
                    if let Some(t) = t {
                        let opaque = t.rgba.chunks_exact(4).filter(|p| p[3] == 255).count() * 100 / (t.width * t.height) as usize;
                        println!("{:50} {}x{} {} {}% opaque", e.file_name().to_string_lossy(), t.width, t.height, unityfs::format_name(t.format), opaque);
                    }
                }
                Err(er) => {
                    bad += 1;
                    println!("{:50} FAILED: {er}", e.file_name().to_string_lossy());
                }
            }
        }
        println!("{ok} ok, {bad} failed, {:.1} s", t0.elapsed().as_secs_f32());
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    let c = match unityfs::open(&bytes) {
        Ok(c) => c,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    println!("player {}  engine {}", c.bundle.player, c.bundle.engine);
    for f in &c.bundle.files {
        println!("  file {} ({} bytes, flags {})", f.name, f.data.len(), f.flags);
    }
    for (id, n) in &c.classes {
        println!("  {n} x {} ({id})", unityfs::class_name(*id));
    }
    for (i, n) in c.texture_names().iter().enumerate() {
        println!("  texture {i}: {n}");
    }
    if let Some(out) = png {
        let t = c.texture(tex).unwrap();
        let f = std::fs::File::create(&out).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), t.width, t.height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&t.rgba).unwrap();
        println!("wrote {out}: {}x{} {}", t.width, t.height, unityfs::format_name(t.format));
    }
}
