//! Dev tool: what is in a media file and how fast it opens, shows a poster, plays.
//!   cargo run --release -p nv-media --example probe -- <file> [seconds to play, default 1]
//! Prints sizes and timings only (no file names, nothing from the picture).
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else { return eprintln!("usage: probe <file> [seconds]") };
    let secs: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let path = std::path::Path::new(path);
    let t = Instant::now();
    let info = nv_media::probe(path);
    println!("probe: {:.1} ms -> {:?}", t.elapsed().as_secs_f64() * 1000.0, info.as_ref().map(|i| (i.duration_ms, i.width, i.height, i.fps, i.has_video, i.has_audio)).map_err(|e| e.clone()));
    let t = Instant::now();
    match nv_media::poster_frame(path) {
        Ok(f) => println!("poster frame: {:.1} ms, {}x{}", t.elapsed().as_secs_f64() * 1000.0, f.width, f.height),
        Err(e) => println!("poster frame: {e}"),
    }
    let frames = Arc::new(AtomicUsize::new(0));
    let wake = {
        let f = frames.clone();
        Arc::new(move || {
            f.fetch_add(1, Ordering::Relaxed);
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let t = Instant::now();
    let p = match nv_media::Player::open(path, wake) {
        Ok(p) => p,
        Err(e) => return println!("open: {e}"),
    };
    println!("open: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    p.set_volume(0.0);
    p.play();
    let start = Instant::now();
    let mut got = 0;
    let mut first = None;
    while start.elapsed() < Duration::from_secs_f64(secs) {
        if p.take_frame().is_some() {
            got += 1;
            first.get_or_insert(start.elapsed());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("played {secs} s: {got} frames shown (first after {:?}), position {} ms, error {:?}", first, p.position_ms(), p.error());
    p.seek(p.info.duration_ms / 2);
    std::thread::sleep(Duration::from_millis(400));
    println!("after seek to the middle: position {} ms, frame {}", p.position_ms(), p.take_frame().is_some());
    let sc = nv_media::Scrubber::open(path, Arc::new(|| {}));
    let t = Instant::now();
    sc.request(p.info.duration_ms / 3);
    while sc.take().is_none() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("scrub preview: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
}
