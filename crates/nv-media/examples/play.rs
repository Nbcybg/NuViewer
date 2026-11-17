//! `play <file> [seconds]`: open a video or sound the way the viewer does, play it, and print what the player reports.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else { return eprintln!("usage: play <file> [seconds]") };
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let wakes = Arc::new(AtomicU64::new(0));
    let w = wakes.clone();
    let t0 = Instant::now();
    let p = match nv_media::Player::open(std::path::Path::new(file), Arc::new(move || { w.fetch_add(1, Ordering::Relaxed); })) {
        Ok(p) => p,
        Err(e) => return println!("open failed: {e}"),
    };
    println!("opened in {:.0} ms: {:?}", t0.elapsed().as_secs_f64() * 1000.0, p.info);
    p.play();
    let mut frames = 0;
    let mut last_ms = 0;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Some(f) = p.take_frame() {
            frames += 1;
            last_ms = f.ms;
        }
        if let Some(n) = p.take_notice() {
            println!("notice: {n}");
        }
        if let Some(e) = p.error() {
            println!("error: {e}");
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("after {secs} s: {frames} frames taken, last frame at {last_ms} ms, position {} ms, playing {}, wakes {}", p.position_ms(), p.is_playing(), wakes.load(Ordering::Relaxed));
}
