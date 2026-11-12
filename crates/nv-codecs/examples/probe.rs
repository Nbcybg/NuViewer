//! Dev tool: decode files and print dimensions, frame count, timing and pixel statistics.
//! `cargo run --release -p nv-codecs --example probe -- <file>...`
//! Prints no file names, so it is safe to paste output from private folders.

use std::path::Path;
use std::time::Instant;

fn main() {
    for (n, arg) in std::env::args().skip(1).enumerate() {
        let path = Path::new(&arg);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("?").to_ascii_lowercase();
        // Separate raw I/O (some folders are network/phone-backed) from decode time: the read also warms the cache.
        let t_read = Instant::now();
        let bytes = std::fs::read(path).map(|b| b.len()).unwrap_or(0);
        let read_ms = t_read.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        match nv_codecs::decode_file(path) {
            Ok(img) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let px = img.rgba.len() / 4;
                let step = (px / 20_000).max(1);
                let (mut sum, mut cnt, mut amin) = ([0u64; 3], 0u64, 255u8);
                for p in img.rgba.chunks_exact(4).step_by(step) {
                    sum[0] += p[0] as u64;
                    sum[1] += p[1] as u64;
                    sum[2] += p[2] as u64;
                    amin = amin.min(p[3]);
                    cnt += 1;
                }
                let avg = |i: usize| sum[i] / cnt.max(1);
                let delays: Vec<u32> = std::iter::once(img.delay_ms).chain(img.extra_frames.iter().map(|f| f.delay_ms)).take(4).collect();
                let (dw, dh) = img.display_size();
                println!(
                    "#{n} .{ext}: {}x{} frames={} first_delays={:?} avg_rgb=({},{},{}) min_alpha={} read={read_ms:.0} ms ({bytes} B) decode={ms:.1} ms",
                    dw, dh, img.frame_count(), delays, avg(0), avg(1), avg(2), amin
                );
            }
            Err(e) => println!("#{n} .{ext}: ERROR {e}"),
        }
    }
}
