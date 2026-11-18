//! `modelbench <model>`: load a model, build the light copy used for turning, and time the pictures of both
//! (saves `modelbench-full.ppm` / `modelbench-light.ppm` in the working directory).
use nv_codecs::model::{proxy_mesh, render_preview, Orbit};
use std::time::Instant;

fn save(name: &str, w: u32, h: u32, rgba: &[u8]) {
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&px[..3]);
    }
    let _ = std::fs::write(name, out);
}

fn main() {
    let Some(file) = std::env::args().nth(1) else { return eprintln!("usage: modelbench <model>") };
    let t = Instant::now();
    let img = nv_codecs::decode_file(std::path::Path::new(&file)).expect("decode");
    let Some(mesh) = img.mesh.clone() else { return eprintln!("not a model") };
    println!("load {:.0} ms: {} triangles, {} vertices, {} textures", t.elapsed().as_secs_f64() * 1000.0, mesh.triangles(), mesh.positions.len(), mesh.textures.len());
    let t = Instant::now();
    let light = proxy_mesh(&mesh, 45_000);
    let t = Instant::now();
    let fine = proxy_mesh(&mesh, 260_000);
    println!("fine copy {:.0} ms: {} triangles", t.elapsed().as_secs_f64() * 1000.0, fine.as_ref().map(|m| m.triangles()).unwrap_or(0));
    println!("light copy {:.0} ms: {} triangles", t.elapsed().as_secs_f64() * 1000.0, light.as_ref().map(|m| m.triangles()).unwrap_or(0));
    let o = Orbit { yaw: 0.8, ..Orbit::default() };
    let (w, h) = (420, 420);
    let t = Instant::now();
    let a = render_preview(light.as_ref().unwrap_or(&mesh), &o, w, h);
    println!("light picture {w}x{h}: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    save("modelbench-light.ppm", w, h, &a);
    let t = Instant::now();
    let b = render_preview(fine.as_ref().unwrap_or(&mesh), &o, 640, 640);
    println!("full picture 640x640: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    save("modelbench-full.ppm", 640, 640, &b);
}
