//! Dev tool: time decoding, per file and per mode, and print the decode statistics table.
//!
//! `cargo run --release -p nv-codecs --example bench_decode -- [--runs N] [--raw full,half,preview] [--thumb] <file>...`
//!
//! Prints no file names (output from private folders can be pasted): files are numbered. `--raw` lists the RAW modes to
//! try for RAW files (default: all three); other files are decoded once per run. `--thumb` times the thumbnail path
//! (first frame, 256 px, as the gallery does) instead of the full decode. Times are the minimum and median of the runs
//! after one warm-up (the warm-up also fills the OS file cache, so these are CPU times; reads from a cold disk differ).

use nv_codecs::RawMode;
use std::path::Path;
use std::time::Instant;

fn main() {
    let mut runs = 3usize;
    let mut modes = vec![RawMode::Preview, RawMode::Half, RawMode::Full];
    let mut thumb = false;
    let mut files = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--runs" => runs = args.next().and_then(|v| v.parse().ok()).unwrap_or(3).max(1),
            "--raw" => modes = args.next().unwrap_or_default().split(',').filter_map(RawMode::from_id).collect(),
            "--thumb" => thumb = true,
            _ => files.push(a),
        }
    }
    for (n, f) in files.iter().enumerate() {
        let path = Path::new(f);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("?").to_ascii_lowercase();
        let is_raw = ["cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "dng", "orf", "rw2", "raf", "pef", "srw", "3fr", "erf", "kdc", "mrw", "raw", "rwl", "iiq", "x3f"].contains(&ext.as_str());
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let list: Vec<Option<RawMode>> = if is_raw { modes.iter().map(|m| Some(*m)).collect() } else { vec![None] };
        for mode in list {
            if let Some(m) = mode {
                nv_codecs::set_raw_mode(m);
            }
            let run = || -> Result<(u32, u32, f64), String> {
                let t = Instant::now();
                let img = if thumb {
                    let full = nv_codecs::decode_for_thumbnail(path)?;
                    let small = nv_codecs::downscale(&full, 256).finish_for_thumb();
                    (small.width, small.height, full.width as u64 * full.height as u64)
                } else {
                    let img = nv_codecs::decode_file(path)?;
                    (img.width, img.height, img.width as u64 * img.height as u64)
                };
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let _ = img.2;
                Ok((img.0, img.1, ms))
            };
            let first = run();
            let label = mode.map(|m| m.id()).unwrap_or("-");
            match first {
                Err(e) => println!("#{n} .{ext} [{label}] ERROR {}", e.chars().take(100).collect::<String>()),
                Ok((w, h, _)) => {
                    let mut times: Vec<f64> = (0..runs).filter_map(|_| run().ok().map(|r| r.2)).collect();
                    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let med = times.get(times.len() / 2).copied().unwrap_or(0.0);
                    println!("#{n} .{ext} [{label}] {w}x{h}  file {:.1} MB  min {:.1} ms  median {med:.1} ms", size as f64 / 1e6, times.first().copied().unwrap_or(0.0));
                }
            }
        }
    }
    println!("\n{}", nv_codecs::stats::report());
}
