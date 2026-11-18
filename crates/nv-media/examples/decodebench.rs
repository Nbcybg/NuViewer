//! `decodebench <file> [frames] [max_edge] [cpu]`: decode speed of the video reader with nothing pacing it (the graphics
//! card path unless `cpu`), and the last frame saved as `decodebench.ppm` next to the exe's working directory.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else { return eprintln!("usage: decodebench <file> [frames] [max_edge] [cpu]") };
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let edge: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let gpu = args.get(3).map(|s| s != "cpu").unwrap_or(true);
    match nv_media::bench_decode(std::path::Path::new(file), n, edge, gpu) {
        Ok((got, ms, last)) => {
            println!("{got} frames in {ms:.0} ms = {:.1} fps ({:.1} ms/frame)", got as f64 * 1000.0 / ms, ms / got.max(1) as f64);
            if let Some(f) = last {
                println!("last frame {}x{} at {} ms", f.width, f.height, f.ms);
                let mut out = format!("P6\n{} {}\n255\n", f.width, f.height).into_bytes();
                for px in f.bgra.chunks_exact(4) {
                    out.extend_from_slice(&[px[2], px[1], px[0]]);
                }
                let _ = std::fs::write("decodebench.ppm", out);
            }
        }
        Err(e) => println!("failed: {e}"),
    }
}
