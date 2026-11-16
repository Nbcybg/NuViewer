//! Video and sound in the viewer: when the picture on screen is a media file, a `Player` (nv-media) is opened for it on a
//! worker thread, its frames are copied into the picture's own texture (so zoom, pan, fit and the film strip work on a video
//! like on any picture), and the playback bar (`controls.rs`) drives it. A sound shows its waveform as the picture.

use super::controls::{fmt_time, Controller, MediaKind};
use super::*;
use nv_codecs::media::{media_kind_of, proxy_for, MediaKind as FileKind};
use nv_media::{Player, Scrubber};

/// Overlay texture id of the picture over the seek bar.
const SCRUB_ID: u64 = 0x5C20_BB1E_0000_0001;
/// Position text and the bar refresh this often while a sound plays (a video redraws with every frame).
const TICK_MS: u32 = 120;

pub struct Media {
    pub player: Option<Player>,
    /// The file the viewer asked for, the file the player really has open (a proxy for a professional clip or for video Windows
    /// can't decode), and the picture id it draws into.
    pub origin: PathBuf,
    path: PathBuf,
    id: u64,
    opening: Option<mpsc::Receiver<(PathBuf, Result<Player, String>)>>,
    /// ffmpeg is making a playable copy of `origin`.
    proxy: Option<mpsc::Receiver<(PathBuf, Result<PathBuf, String>)>>,
    proxy_stop: Arc<std::sync::atomic::AtomicBool>,
    /// A file that could not be opened: not tried again until another one is shown.
    failed: Option<PathBuf>,
    scrub: Option<Scrubber>,
    scrub_for: PathBuf,
    scrub_size: Option<(u32, u32)>,
    scrub_asked: Option<u64>,
    pub vol_drag: bool,
    /// The frame on screen (kept for a snapshot when ffmpeg isn't there).
    pub last: Option<nv_media::Frame>,
}

impl Media {
    pub fn new() -> Media {
        Media { player: None, origin: PathBuf::new(), path: PathBuf::new(), id: 0, opening: None, proxy: None, proxy_stop: Arc::new(std::sync::atomic::AtomicBool::new(false)), failed: None, scrub: None, scrub_for: PathBuf::new(), scrub_size: None, scrub_asked: None, vol_drag: false, last: None }
    }
}

impl App {
    /// The file on screen is a video, a sound or a professional clip.
    pub fn media_current(&self) -> bool {
        self.mode == Mode::Viewer && self.folder.files.get(self.folder.index).map(|p| media_kind_of(p).is_some()).unwrap_or(false)
    }

    /// What to open for the file on screen: the file, or the proxy beside a professional clip.
    fn media_source(&self) -> Option<PathBuf> {
        let p = self.folder.files.get(self.folder.index)?;
        match media_kind_of(p)? {
            FileKind::Pro => proxy_for(p),
            _ => Some(p.clone()),
        }
    }

    /// Keep the player in step with what is on screen: open one for a media file once its picture is up, drop it when
    /// something else is shown.
    pub fn media_sync(&mut self) {
        let want = if self.media_current() && self.renderer.current() == Some(self.id_at(self.folder.index)) { self.media_source() } else { None };
        match want {
            Some(src) => {
                if self.media.origin == src && (self.media.player.is_some() || self.media.opening.is_some() || self.media.proxy.is_some()) || self.media.failed.as_ref() == Some(&src) {
                    return;
                }
                self.media_close();
                self.media.origin = src.clone();
                self.media.id = self.id_at(self.folder.index);
                self.media_open(src);
            }
            None => {
                if self.media.player.is_some() || self.media.opening.is_some() || self.media.proxy.is_some() {
                    self.media_close();
                }
                if !self.media_current() {
                    self.media.failed = None;
                }
            }
        }
    }

    pub fn media_close(&mut self) {
        // Stopping a player waits for its threads: do it off the UI thread.
        if let Some(p) = self.media.player.take() {
            std::thread::spawn(move || drop(p));
        }
        if let Some(s) = self.media.scrub.take() {
            std::thread::spawn(move || drop(s));
        }
        self.media.opening = None;
        self.media.proxy = None;
        self.media.proxy_stop.store(true, std::sync::atomic::Ordering::SeqCst); // an unfinished proxy is dropped
        self.media.scrub_size = None;
        self.media.scrub_asked = None;
        self.media.origin = PathBuf::new();
        self.media.path = PathBuf::new();
        kill_timer(self.hwnd, TIMER_MEDIA);
    }

    /// A wake-up from the player threads: take a finished open, new frames, new preview pictures, errors.
    pub fn media_pump(&mut self) {
        if let Some(rx) = &self.media.opening {
            if let Ok((src, res)) = rx.try_recv() {
                self.media.opening = None;
                match res {
                    Ok(p) if src == self.media.path => {
                        p.set_loop(self.settings.more.play_loop);
                        p.set_speed(self.anim_speed);
                        self.media.player = Some(p);
                        self.media_apply_volume();
                        if self.settings.more.autoplay {
                            if let Some(p) = &self.media.player {
                                p.play();
                            }
                        }
                        set_timer(self.hwnd, TIMER_MEDIA, TICK_MS);
                        invalidate(self.hwnd);
                    }
                    Ok(_) => {}
                    Err(e) => self.media_failed(e),
                }
            }
        }
        if let Some(rx) = &self.media.proxy {
            if let Ok((origin, res)) = rx.try_recv() {
                self.media.proxy = None;
                match res {
                    Ok(p) if origin == self.media.origin => {
                        self.toast("Playable copy ready");
                        self.media_open(p);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        self.media.failed = Some(origin);
                        self.toast(format!("Couldn't make a playable copy: {e}"));
                    }
                }
            }
        }
        let Some(p) = &self.media.player else { return };
        if let Some(f) = p.take_frame() {
            if self.renderer.upload_video(self.media.id, f.width, f.height, &f.bgra).is_ok() {
                invalidate(self.hwnd);
            }
            self.media.last = Some(f);
        }
        if let Some(n) = p.take_notice() {
            self.toast(n);
        }
        let Some(p) = &self.media.player else { return };
        if let Some(e) = p.error() {
            self.media_failed(e);
        }
        if let Some(s) = &self.media.scrub {
            if let Some((_, f)) = s.take() {
                let mut rgba = f.bgra;
                for px in rgba.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                let img = nv_codecs::Image::new(f.width, f.height, rgba);
                if self.renderer.upload_overlay(SCRUB_ID, &img).is_ok() {
                    self.media.scrub_size = Some((f.width, f.height));
                    invalidate(self.hwnd);
                }
            }
        }
    }

    /// `TIMER_MEDIA`: keep the position moving on screen while something plays and the bar is up.
    pub fn media_timer(&mut self) {
        match &self.media.player {
            Some(p) if p.is_playing() => {
                if self.controls_visible() {
                    invalidate(self.hwnd);
                }
            }
            Some(_) if self.controls_visible() => invalidate(self.hwnd),
            _ => {}
        }
    }

    pub fn media_controller(&self, id: u64) -> Option<Controller> {
        let p = self.media.player.as_ref()?;
        if self.media.id != id {
            return None;
        }
        let dur = p.info.duration_ms;
        let pos = p.position_ms();
        let (level, muted) = (self.settings.more.volume as f32 / 100.0, self.settings.more.muted);
        Some(Controller {
            kind: if p.info.has_video { MediaKind::Video } else { MediaKind::Audio },
            playing: p.is_playing(),
            progress: if dur > 0 { (pos as f32 / dur as f32).clamp(0.0, 1.0) } else { 0.0 },
            seekable: dur > 0,
            time: if dur > 0 { format!("{} / {}", fmt_time(pos), fmt_time(dur)) } else { fmt_time(pos) },
            speed: self.anim_speed,
            looping: self.settings.more.play_loop,
            volume: p.info.has_audio.then_some((level, muted)),
        })
    }

    pub fn media_duration_ms(&self) -> u64 {
        self.media.player.as_ref().map(|p| p.info.duration_ms).unwrap_or(0)
    }

    pub fn media_toggle_play(&mut self) {
        if let Some(p) = &self.media.player {
            if p.is_playing() {
                p.pause();
            } else {
                p.play();
            }
        } else if self.media.opening.is_none() && self.media_current() {
            // Not open yet (autoplay is off, or it failed before): try again.
            self.media.failed = None;
            self.media_sync();
        }
    }

    pub fn media_seek_fraction(&mut self, frac: f32) {
        if let Some(p) = &self.media.player {
            p.seek((frac as f64 * p.info.duration_ms as f64) as u64);
        }
    }

    pub fn media_jump(&mut self, delta_ms: i64) {
        if let Some(p) = &self.media.player {
            let to = (p.position_ms() as i64 + delta_ms).clamp(0, p.info.duration_ms as i64);
            p.seek(to as u64);
        }
    }

    /// One frame forward / back (`,` and `.`, the two buttons of the bar). A video steps exactly; an animation or a sequence
    /// takes one picture; a sound has no frames.
    pub fn frame_step(&mut self, forward: bool) {
        if let Some(p) = &self.media.player {
            p.step_frame(forward);
            invalidate(self.hwnd);
        } else if self.controller().is_some() {
            self.step_frame(if forward { 1 } else { -1 });
        }
    }

    pub fn media_apply_speed(&self) {
        if let Some(p) = &self.media.player {
            p.set_speed(self.anim_speed);
        }
    }

    pub fn media_apply_loop(&self) {
        if let Some(p) = &self.media.player {
            p.set_loop(self.settings.more.play_loop);
        }
    }

    pub fn media_apply_volume(&self) {
        if let Some(p) = &self.media.player {
            p.set_volume(if self.settings.more.muted { 0.0 } else { self.settings.more.volume as f32 / 100.0 });
        }
    }

    /// The pointer is over the seek track at time `ms`: ask for the picture of that spot (a video only).
    pub fn media_request_preview(&mut self, ms: u64) {
        let Some(p) = &self.media.player else { return };
        if !p.info.has_video {
            return;
        }
        if self.media.scrub.is_none() || self.media.scrub_for != self.media.path {
            self.media.scrub = Some(Scrubber::open(&self.media.path, self.wake.clone()));
            self.media.scrub_for = self.media.path.clone();
            self.media.scrub_size = None;
            self.media.scrub_asked = None;
        }
        // About 10 asks a second is plenty: the answer is a key frame anyway.
        let slot = ms / 100;
        if self.media.scrub_asked != Some(slot) {
            self.media.scrub_asked = Some(slot);
            if let Some(s) = &self.media.scrub {
                s.request(ms);
            }
        }
    }

    pub fn media_preview_size(&self) -> Option<(u32, u32)> {
        self.media.scrub_size
    }

    pub fn media_draw_preview(&mut self, r: [f32; 4]) {
        self.hud.used.insert(SCRUB_ID);
        let _ = self.renderer.draw_overlay(SCRUB_ID, r[0], r[1], [1.0; 4]);
    }

    /// Shift+Left / Shift+Right: five seconds back / forward.
    pub fn media_nudge(&mut self, forward: bool) -> bool {
        if self.media.player.is_none() {
            return false;
        }
        self.media_jump(if forward { 5000 } else { -5000 });
        invalidate(self.hwnd);
        true
    }
}

impl App {
    /// Open a player on `path` (the file itself, or its playable copy) on a worker thread.
    fn media_open(&mut self, path: PathBuf) {
        self.media.path = path.clone();
        let (tx, rx) = mpsc::channel();
        self.media.opening = Some(rx);
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let r = Player::open(&path, wake.clone());
            let _ = tx.send((path, r));
            wake();
        });
    }

    /// The player could not be opened or stopped with an error: for video Windows has no decoder for, ffmpeg (when it is
    /// there) makes a playable copy in the background and that is played instead; otherwise the message is shown.
    fn media_failed(&mut self, error: String) {
        let origin = self.media.origin.clone();
        let no_decoder = error.contains("no decoder") || error.contains("no suitable") || error.contains("can't be played") || nv_codecs::media::needs_helper_path(&origin);
        let already_proxy = self.media.path != origin;
        if let Some(p) = self.media.player.take() {
            std::thread::spawn(move || drop(p));
        }
        self.media.opening = None;
        if no_decoder && !already_proxy && nv_core::ffmpeg::find().is_some() && self.media.proxy.is_none() {
            let dest = nv_core::ffmpeg::proxy_path(&origin);
            if dest.is_file() {
                self.media_open(dest);
                return;
            }
            self.toast("Windows can't play this video: ffmpeg is making a playable copy (kept in a cache)…");
            let (tx, rx) = mpsc::channel();
            self.media.proxy = Some(rx);
            self.media.proxy_stop.store(false, std::sync::atomic::Ordering::SeqCst);
            let (stop, wake) = (self.media.proxy_stop.clone(), self.wake.clone());
            std::thread::spawn(move || {
                let r = nv_core::ffmpeg::make_proxy(&origin, &dest, &|| !stop.load(std::sync::atomic::Ordering::SeqCst)).map(|_| dest.clone());
                let _ = tx.send((origin, r));
                wake();
            });
            return;
        }
        self.media.failed = Some(origin);
        self.toast(error);
    }
}
