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
    // PLAY_EDGE=720 PLAY_FF=1: the options the gallery's preview uses. ffmpeg is looked up on PATH.
    nv_media::set_ffmpeg(|| std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("ffmpeg.exe")).find(|p| p.is_file())));
    let opts = nv_media::Opts { max_edge: std::env::var("PLAY_EDGE").ok().and_then(|v| v.parse().ok()).unwrap_or(nv_media::DEFAULT_EDGE), ffmpeg_fallback: std::env::var("PLAY_FF").is_ok() };
    // PLAY_POSTER=1: a poster is taken on another thread at the same moment (the viewer does that).
    let poster = std::env::var("PLAY_POSTER").is_ok().then(|| { let f = file.clone(); std::thread::spawn(move || { let r = nv_media::poster_frame(std::path::Path::new(&f)); println!("poster: {:?}", r.map(|f| (f.width, f.height))); }) });
    let p = match nv_media::Player::open_with(std::path::Path::new(file), Arc::new(move || { w.fetch_add(1, Ordering::Relaxed); }), opts) {
        Ok(p) => p,
        Err(e) => return println!("open failed: {e}"),
    };
    println!("opened in {:.0} ms: {:?}", t0.elapsed().as_secs_f64() * 1000.0, p.info);
    p.play();
    let mut frames = 0;
    let mut last_ms = 0;
    let mut dims = (0, 0, 0u8);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(secs) {
        if let Some(f) = p.take_frame() {
            frames += 1;
            last_ms = f.ms;
            dims = (f.width, f.height, f.bgra.chunks_exact(4).step_by(997).map(|p| p[3]).min().unwrap_or(255));
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
    println!("frame {}x{}, smallest alpha {}", dims.0, dims.1, dims.2);
    println!("after {secs} s: {frames} frames taken, last frame at {last_ms} ms, position {} ms, playing {}, wakes {}", p.position_ms(), p.is_playing(), wakes.load(Ordering::Relaxed));
    if let Some(h) = poster { let _ = h.join(); }
}
