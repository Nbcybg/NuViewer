//! Playing video and sound: `Player::open` starts a video thread and a sound thread; the program asks for the newest frame
//! and the position whenever it redraws, and is woken (the `wake` callback) when a new frame is ready. `Scrubber` gives the
//! pictures that appear over the seek bar. Everything is Media Foundation (decoding) + waveOut (sound), bound late (`load`).

mod audio;
mod load;
mod readers;

pub use readers::{audio_peaks, poster_frame, probe, Frame};
use readers::{AudioReader, Read, VideoReader, CHANNELS, RATE};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What a media file holds.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub has_video: bool,
    pub has_audio: bool,
}

/// Media time: runs with the wall clock at `speed` while playing; the sound thread nudges it when the sound card drifts.
struct Clock {
    base_ms: f64,
    since: Instant,
    running: bool,
    speed: f64,
}

struct Shared {
    quit: AtomicBool,
    playing: AtomicBool,
    ended: AtomicBool,
    looping: AtomicBool,
    volume: AtomicU32,
    /// Bumped by every seek; `target` says where to.
    gen: AtomicU64,
    target: AtomicU64,
    /// Bumped when the speed changes (the sound restarts its resampling there).
    speed_gen: AtomicU64,
    clock: Mutex<Clock>,
    frame: Mutex<Option<Frame>>,
    error: Mutex<Option<String>>,
    /// Something worth telling the user that does not stop the playing (no sound device).
    notice: Mutex<Option<String>>,
    has_video: bool,
    duration_ms: u64,
    sleep: (Mutex<()>, Condvar),
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn now_ms(&self) -> f64 {
        let c = self.clock.lock().unwrap();
        if c.running { c.base_ms + c.since.elapsed().as_secs_f64() * 1000.0 * c.speed } else { c.base_ms }
    }

    fn set_clock(&self, ms: f64, running: bool, speed: Option<f64>) {
        let mut c = self.clock.lock().unwrap();
        c.base_ms = ms;
        c.since = Instant::now();
        c.running = running;
        if let Some(s) = speed {
            c.speed = s;
        }
    }

    fn speed(&self) -> f64 {
        self.clock.lock().unwrap().speed
    }

    fn seek(&self, ms: u64) {
        let ms = if self.duration_ms > 0 { ms.min(self.duration_ms) } else { ms };
        self.target.store(ms, Ordering::SeqCst);
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.set_clock(ms as f64, self.playing.load(Ordering::SeqCst), None);
        self.ended.store(false, Ordering::SeqCst);
        self.nudge();
    }

    fn nudge(&self) {
        self.sleep.1.notify_all();
    }

    fn nap(&self, ms: u64) {
        let g = self.sleep.0.lock().unwrap();
        let _ = self.sleep.1.wait_timeout(g, Duration::from_millis(ms));
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
}

pub struct Player {
    pub info: Info,
    path: PathBuf,
    sh: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Player {
    /// Open `path` for playback, paused at the start. `wake` is called (from another thread) when there is a new frame or
    /// the state changed.
    pub fn open(path: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Player, String> {
        let p = probe(path)?;
        if !p.has_video && !p.has_audio {
            return Err("nothing in this file can be played".into());
        }
        let info = Info { duration_ms: p.duration_ms, width: p.width, height: p.height, fps: p.fps, has_video: p.has_video, has_audio: p.has_audio };
        let sh = Arc::new(Shared {
            quit: AtomicBool::new(false),
            playing: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            looping: AtomicBool::new(true),
            volume: AtomicU32::new(0.8f32.to_bits()),
            gen: AtomicU64::new(0),
            target: AtomicU64::new(0),
            speed_gen: AtomicU64::new(0),
            clock: Mutex::new(Clock { base_ms: 0.0, since: Instant::now(), running: false, speed: 1.0 }),
            frame: Mutex::new(None),
            error: Mutex::new(None),
            notice: Mutex::new(None),
            has_video: p.has_video,
            duration_ms: p.duration_ms,
            sleep: (Mutex::new(()), Condvar::new()),
            wake,
        });
        let mut threads = Vec::new();
        if p.has_video {
            let (s, path) = (sh.clone(), path.to_path_buf());
            threads.push(std::thread::spawn(move || video_loop(&path, &s)));
        }
        if p.has_audio {
            let (s, path) = (sh.clone(), path.to_path_buf());
            threads.push(std::thread::spawn(move || audio_loop(&path, &s)));
        }
        Ok(Player { info, path: path.to_path_buf(), sh, threads })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn play(&self) {
        if self.sh.ended.load(Ordering::SeqCst) {
            self.sh.seek(0);
        }
        self.sh.set_clock(self.sh.now_ms(), true, None);
        self.sh.playing.store(true, Ordering::SeqCst);
        self.sh.nudge();
    }

    pub fn pause(&self) {
        self.sh.set_clock(self.sh.now_ms(), false, None);
        self.sh.playing.store(false, Ordering::SeqCst);
        self.sh.nudge();
    }

    pub fn is_playing(&self) -> bool {
        self.sh.playing.load(Ordering::SeqCst)
    }

    pub fn ended(&self) -> bool {
        self.sh.ended.load(Ordering::SeqCst)
    }

    pub fn seek(&self, ms: u64) {
        self.sh.seek(ms);
    }

    pub fn position_ms(&self) -> u64 {
        let t = self.sh.now_ms().max(0.0) as u64;
        if self.sh.duration_ms > 0 { t.min(self.sh.duration_ms) } else { t }
    }

    pub fn set_speed(&self, s: f32) {
        let s = s.clamp(0.25, 8.0) as f64;
        self.sh.set_clock(self.sh.now_ms(), self.is_playing(), Some(s));
        self.sh.speed_gen.fetch_add(1, Ordering::SeqCst);
        self.sh.nudge();
    }

    pub fn set_volume(&self, v: f32) {
        self.sh.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_loop(&self, on: bool) {
        self.sh.looping.store(on, Ordering::SeqCst);
    }

    /// The newest frame since the last call.
    pub fn take_frame(&self) -> Option<Frame> {
        self.sh.frame.lock().unwrap().take()
    }

    pub fn error(&self) -> Option<String> {
        self.sh.error.lock().unwrap().clone()
    }

    /// A message that does not stop the playing (taken once).
    pub fn take_notice(&self) -> Option<String> {
        self.sh.notice.lock().unwrap().take()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.sh.quit.store(true, Ordering::SeqCst);
        self.sh.nudge();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn fail(sh: &Shared, msg: String) {
    *sh.error.lock().unwrap() = Some(msg);
    sh.playing.store(false, Ordering::SeqCst);
    (sh.wake)();
}

fn video_loop(path: &Path, sh: &Shared) {
    load::init_thread();
    let vr = match VideoReader::open(path) {
        Ok(v) => v,
        Err(e) => return fail(sh, e),
    };
    let mut seen = 0u64;
    let mut want_frame = true; // the first frame is shown while paused
    let mut skip_until: Option<u64> = None;
    let mut last_shown = Instant::now();
    while !sh.quit.load(Ordering::SeqCst) {
        let g = sh.gen.load(Ordering::SeqCst);
        if g != seen {
            seen = g;
            let t = sh.target.load(Ordering::SeqCst);
            vr.seek(t);
            skip_until = Some(t);
            want_frame = true;
        }
        let running = sh.playing.load(Ordering::SeqCst) && !sh.ended.load(Ordering::SeqCst);
        if !running && !want_frame {
            sh.nap(30);
            continue;
        }
        match vr.read() {
            Read::Item(f) => {
                if g != sh.gen.load(Ordering::SeqCst) {
                    continue; // a seek came in while this frame was being decoded
                }
                if let Some(t) = skip_until {
                    if f.ms + 30 < t {
                        continue; // decoding forward from the key frame to the spot that was asked for
                    }
                    skip_until = None;
                }
                // Wait for the frame's time.
                while sh.playing.load(Ordering::SeqCst) && !sh.quit.load(Ordering::SeqCst) && g == sh.gen.load(Ordering::SeqCst) {
                    let ahead = f.ms as f64 - sh.now_ms();
                    if ahead <= 3.0 {
                        break;
                    }
                    sh.nap(((ahead / sh.speed().max(0.1)) as u64).clamp(1, 20));
                }
                // Far behind: drop it, but never go longer than a quarter of a second without showing one.
                let late = sh.now_ms() - f.ms as f64;
                if sh.playing.load(Ordering::SeqCst) && late > 120.0 && last_shown.elapsed() < Duration::from_millis(250) {
                    continue;
                }
                *sh.frame.lock().unwrap() = Some(f);
                last_shown = Instant::now();
                want_frame = false;
                (sh.wake)();
            }
            Read::End => {
                if sh.looping.load(Ordering::SeqCst) && sh.playing.load(Ordering::SeqCst) {
                    sh.seek(0);
                } else {
                    sh.ended.store(true, Ordering::SeqCst);
                    sh.playing.store(false, Ordering::SeqCst);
                    sh.set_clock(sh.duration_ms as f64, false, None);
                    want_frame = false;
                    (sh.wake)();
                }
            }
            Read::Error(e) => return fail(sh, e),
        }
    }
}

fn audio_loop(path: &Path, sh: &Shared) {
    load::init_thread();
    let ar = match AudioReader::open(path) {
        Ok(a) => a,
        Err(e) => return fail(sh, e),
    };
    let mut out = match audio::AudioOut::open() {
        Ok(o) => o,
        // No sound device (or another program holds it): a video plays on silently; a sound alone runs its clock so the bar still works.
        Err(e) => {
            *sh.notice.lock().unwrap() = Some(format!("{e}: playing without sound"));
            (sh.wake)();
            if !sh.has_video {
                silent_loop(sh);
            }
            return;
        }
    };
    let (mut seen, mut seen_speed) = (0u64, 0u64);
    // The sound card counts from the last reset: `seg_ms` is the media time that was queued first, `seg_speed` how fast it was fed.
    let (mut seg_ms, mut seg_speed) = (0.0f64, 1.0f64);
    let mut frac = 0.0f64; // read position inside the input block when resampling
    let mut last_check = Instant::now();
    let alive = || !sh.quit.load(Ordering::SeqCst);
    let restart = |out: &mut audio::AudioOut, ar: &AudioReader, at: f64, seg_ms: &mut f64, seg_speed: &mut f64, frac: &mut f64| {
        out.reset();
        ar.seek(at.max(0.0) as u64);
        *seg_ms = at;
        *seg_speed = sh.speed();
        *frac = 0.0;
    };
    while alive() {
        let g = sh.gen.load(Ordering::SeqCst);
        let sg = sh.speed_gen.load(Ordering::SeqCst);
        if g != seen {
            seen = g;
            seen_speed = sg;
            restart(&mut out, &ar, sh.target.load(Ordering::SeqCst) as f64, &mut seg_ms, &mut seg_speed, &mut frac);
        } else if sg != seen_speed {
            seen_speed = sg;
            let at = sh.now_ms();
            restart(&mut out, &ar, at, &mut seg_ms, &mut seg_speed, &mut frac);
        }
        let running = sh.playing.load(Ordering::SeqCst) && !sh.ended.load(Ordering::SeqCst);
        out.set_paused(!running);
        if !running {
            sh.nap(30);
            continue;
        }
        match ar.read() {
            Read::Item(block) => {
                let speed = seg_speed;
                let vol = sh.volume();
                let gain = vol * vol; // a rough loudness curve: the lower half of the slider is gentle
                let ch = CHANNELS as usize;
                let frames_in = block.len() / ch;
                let mut mixed: Vec<i16> = Vec::with_capacity(((frames_in as f64 / speed) as usize + 2) * ch);
                // Linear resampling: speed 2 reads two input frames per output frame (higher pitch, like a tape).
                while (frac as usize) + 1 < frames_in {
                    let i = frac as usize;
                    let t = (frac - i as f64) as f32;
                    for c in 0..ch {
                        let a = block[i * ch + c] as f32;
                        let b = block[(i + 1) * ch + c] as f32;
                        mixed.push(((a + (b - a) * t) * gain).clamp(-32768.0, 32767.0) as i16);
                    }
                    frac += speed;
                }
                frac -= frames_in as f64;
                out.write(&mixed, &|| alive() && sh.gen.load(Ordering::SeqCst) == seen && sh.speed_gen.load(Ordering::SeqCst) == seen_speed);
                if last_check.elapsed() > Duration::from_millis(250) {
                    last_check = Instant::now();
                    // Where the sound card really is, in media time; the clock follows it when they drift apart.
                    let played_ms = out.played_frames() as f64 * 1000.0 / RATE as f64;
                    let media = seg_ms + played_ms * seg_speed;
                    let off = media - sh.now_ms();
                    if off.abs() > 90.0 && out.played_frames() > RATE as u64 / 4 {
                        sh.set_clock(media, true, None);
                    }
                }
            }
            Read::End => {
                if !sh.has_video {
                    // Sound only: wait until what is queued has been heard, then loop or stop.
                    sh.nap(250);
                    if sh.looping.load(Ordering::SeqCst) {
                        sh.seek(0);
                    } else {
                        sh.ended.store(true, Ordering::SeqCst);
                        sh.playing.store(false, Ordering::SeqCst);
                        sh.set_clock(sh.duration_ms as f64, false, None);
                        (sh.wake)();
                    }
                } else {
                    sh.nap(100); // the video thread decides about looping
                }
            }
            Read::Error(e) => return fail(sh, e),
        }
    }
}

// ---- seek-bar previews -----------------------------------------------------------------------------------------------

/// Small pictures of a video at any time: ask for one, take the newest answer. Runs on its own reader, so scrubbing
/// never disturbs playback.
pub struct Scrubber {
    shared: Arc<ScrubShared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct ScrubShared {
    want: Mutex<Option<u64>>,
    cv: Condvar,
    got: Mutex<Option<(u64, Frame)>>,
    quit: AtomicBool,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// Longest side of a preview picture.
const PREVIEW_EDGE: u32 = 240;

impl Scrubber {
    pub fn open(path: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Scrubber {
        let shared = Arc::new(ScrubShared { want: Mutex::new(None), cv: Condvar::new(), got: Mutex::new(None), quit: AtomicBool::new(false), wake });
        let (s, p) = (shared.clone(), path.to_path_buf());
        let thread = std::thread::spawn(move || scrub_loop(&p, &s));
        Scrubber { shared, thread: Some(thread) }
    }

    /// Ask for the picture at `ms` (the last ask wins).
    pub fn request(&self, ms: u64) {
        *self.shared.want.lock().unwrap() = Some(ms);
        self.shared.cv.notify_all();
    }

    pub fn take(&self) -> Option<(u64, Frame)> {
        self.shared.got.lock().unwrap().take()
    }
}

impl Drop for Scrubber {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn scrub_loop(path: &Path, s: &ScrubShared) {
    load::init_thread();
    let Ok(vr) = VideoReader::open(path) else { return };
    while !s.quit.load(Ordering::SeqCst) {
        let ms = {
            let mut w = s.want.lock().unwrap();
            while w.is_none() && !s.quit.load(Ordering::SeqCst) {
                w = s.cv.wait(w).unwrap();
            }
            w.take()
        };
        let Some(ms) = ms else { continue };
        vr.seek(ms);
        // The first frame after a seek is the key frame before the spot: close enough for a preview, and instant.
        if let Read::Item(f) = vr.read() {
            let small = shrink(f);
            *s.got.lock().unwrap() = Some((ms, small));
            (s.wake)();
        }
    }
}

/// Box-sample a frame down to at most `PREVIEW_EDGE` pixels on its long side.
fn shrink(f: Frame) -> Frame {
    let k = (f.width.max(f.height) as f32 / PREVIEW_EDGE as f32).ceil().max(1.0) as u32;
    if k <= 1 {
        return f;
    }
    let (w, h) = ((f.width / k).max(1), (f.height / k).max(1));
    let mut out = vec![255u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (mut b, mut g, mut r, mut n) = (0u32, 0u32, 0u32, 0u32);
            // Four samples inside the k x k block are enough for a thumbnail.
            for (dx, dy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
                let (sx, sy) = ((x * k + k * dx / 4).min(f.width - 1), (y * k + k * dy / 4).min(f.height - 1));
                let i = ((sy * f.width + sx) * 4) as usize;
                b += f.bgra[i] as u32;
                g += f.bgra[i + 1] as u32;
                r += f.bgra[i + 2] as u32;
                n += 1;
            }
            let o = ((y * w + x) * 4) as usize;
            out[o] = (b / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (r / n) as u8;
        }
    }
    Frame { width: w, height: h, bgra: out, ms: f.ms }
}

/// A sound with nowhere to play: the clock still runs so the bar, seeking and the end of the file behave.
fn silent_loop(sh: &Shared) {
    while !sh.quit.load(Ordering::SeqCst) {
        if sh.playing.load(Ordering::SeqCst) && !sh.ended.load(Ordering::SeqCst) && sh.duration_ms > 0 && sh.now_ms() >= sh.duration_ms as f64 {
            if sh.looping.load(Ordering::SeqCst) {
                sh.seek(0);
            } else {
                sh.ended.store(true, Ordering::SeqCst);
                sh.playing.store(false, Ordering::SeqCst);
                sh.set_clock(sh.duration_ms as f64, false, None);
                (sh.wake)();
            }
        }
        sh.nap(30);
    }
}
