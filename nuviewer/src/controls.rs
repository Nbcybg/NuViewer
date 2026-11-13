//! Playback controls: one bar at the bottom of the viewer for everything that plays over time.
//!
//! ```text
//!   [|<<] [ > ] [>>|]  [=====o-----------------]   frame 12/48   [-] 24 fps [+]
//! ```
//!
//! It appears while the pointer is near the bottom edge (or always, if the setting asks for that, or pinned with `C`),
//! and only when the current picture is something that plays: a **slideshow**, an **animation** (GIF / APNG / WebP) or
//! a folded **image sequence**. `MediaKind` also reserves **Video** and **Audio**: when a decoder for them exists they
//! use this very bar (play/pause, seek, time, speed, plus a volume button on the right); nothing decodes them yet.

use super::*;

const FPS_STEPS: [u32; 11] = [1, 2, 4, 6, 8, 12, 15, 24, 30, 48, 60];
const SPEED_STEPS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Slideshow,
    Animation,
    Sequence,
    /// Reserved: needs a video decoder (Media Foundation is the natural fit).
    #[allow(dead_code)]
    Video,
    /// Reserved: needs an audio decoder; would show the cover art or a waveform as the picture.
    #[allow(dead_code)]
    Audio,
}

/// What the bar shows right now.
pub struct Controller {
    pub kind: MediaKind,
    pub playing: bool,
    /// 0..=1 along the seek bar.
    pub progress: f32,
    pub seekable: bool,
    pub left: String,
    pub right: String,
}

/// Rectangles (client pixels) of the bar and its parts.
pub struct Bar {
    pub rect: [f32; 4],
    pub prev: [f32; 4],
    pub play: [f32; 4],
    pub next: [f32; 4],
    pub seek: [f32; 4],
    pub slower: [f32; 4],
    pub faster: [f32; 4],
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

impl App {
    /// Something that plays over time is in front of the user.
    pub fn controller(&self) -> Option<Controller> {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return None;
        }
        let i = self.folder.index;
        let id = self.id_at(i);
        if let Some(run) = self.run_of(i) {
            let n = run.len();
            let pos = i - run.start;
            return Some(Controller {
                kind: MediaKind::Sequence,
                playing: self.seq_playing,
                progress: if n > 1 { pos as f32 / (n - 1) as f32 } else { 0.0 },
                seekable: true,
                left: format!("frame {}/{n}", pos + 1),
                right: format!("{} fps", self.fps),
            });
        }
        let frames = self.renderer.frame_count(id);
        if frames > 1 && self.renderer.current() == Some(id) {
            let f = self.renderer.frame_index(id);
            return Some(Controller {
                kind: MediaKind::Animation,
                playing: !self.anim_paused,
                progress: f as f32 / (frames - 1) as f32,
                seekable: true,
                left: format!("frame {}/{frames}", f + 1),
                right: format!("{}×", fmt_speed(self.anim_speed)),
            });
        }
        if self.slide.on {
            return Some(Controller {
                kind: MediaKind::Slideshow,
                playing: true,
                progress: (self.slide.since.elapsed().as_secs_f32() / self.slide.secs).clamp(0.0, 1.0),
                seekable: false,
                left: format!("{}/{}", i + 1, self.folder.files.len()),
                right: format!("{:.0} s", self.slide.secs),
            });
        }
        None
    }

    pub fn bar_layout(&self) -> Bar {
        let (w, h) = self.renderer.size();
        let (w, h, d) = (w as f32, h as f32, self.gallery.dpi);
        let bar_w = (w * 0.7).clamp(460.0 * d, 920.0 * d).min(w - 24.0 * d);
        let bar_h = 48.0 * d;
        let (x0, y0) = ((w - bar_w) / 2.0, h - bar_h - 30.0 * d); // the very bottom row is kept for the taskbar reveal
        let btn = 34.0 * d;
        let by = y0 + (bar_h - btn) / 2.0;
        let mut x = x0 + 8.0 * d;
        let next_btn = |x: &mut f32| {
            let r = [*x, by, *x + btn, by + btn];
            *x += btn + 4.0 * d;
            r
        };
        let (prev, play, next) = (next_btn(&mut x), next_btn(&mut x), next_btn(&mut x));
        let right = x0 + bar_w - 8.0 * d;
        let faster = [right - 30.0 * d, by, right, by + btn];
        let slower = [faster[0] - 30.0 * d - 64.0 * d, by, faster[0] - 64.0 * d, by + btn];
        let label_w = 96.0 * d;
        let seek = [x + 6.0 * d, y0 + bar_h / 2.0 - 5.0 * d, slower[0] - label_w - 8.0 * d, y0 + bar_h / 2.0 + 5.0 * d];
        Bar { rect: [x0, y0, x0 + bar_w, y0 + bar_h], prev, play, next, seek, slower, faster }
    }

    /// Show the bar? Always for "always", otherwise when the pointer is in the lower part of the window.
    pub fn controls_visible(&self) -> bool {
        if self.controller().is_none() {
            return false;
        }
        if self.settings.controls_always || self.controls_pinned || self.seek_drag {
            return true;
        }
        let bar = self.bar_layout();
        let h = self.renderer.size().1 as f32;
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let _ = mx;
        my >= bar.rect[1] - 40.0 * self.gallery.dpi && my < h
    }

    pub fn draw_controls(&mut self) {
        let Some(c) = self.controller() else { return };
        if !self.controls_visible() {
            return;
        }
        let bar = self.bar_layout();
        let d = self.gallery.dpi;
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let (text_c, dim_c) = (srgb(0xF2F2F2, 1.0), srgb(0x9AA0A6, 1.0));
        let _ = self.renderer.fill_rect(bar.rect, srgb(0x0E0E0E, 0.86));
        let glyph = |this: &mut App, r: [f32; 4], text: &str, enabled: bool| {
            if inside(&r, mx, my) && enabled {
                let _ = this.renderer.fill_rect(r, srgb(0x3A3F4A, 0.9));
            }
            if let Some((key, tw, th)) = this.overlay_small(&[text.to_string()]) {
                let _ = this.renderer.draw_overlay(key, r[0] + (r[2] - r[0] - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, if enabled { text_c } else { dim_c });
            }
        };
        let stepping = c.kind != MediaKind::Slideshow;
        glyph(self, bar.prev, "\u{25C0}\u{25C0}", stepping);
        glyph(self, bar.play, if c.playing { "\u{275A}\u{275A}" } else { "\u{25B6}" }, true);
        glyph(self, bar.next, "\u{25B6}\u{25B6}", stepping);
        glyph(self, bar.slower, "\u{2212}", true);
        glyph(self, bar.faster, "+", true);
        // Seek track: grey, with the played part in the accent colour and a knob.
        let s = bar.seek;
        let _ = self.renderer.fill_rect(s, srgb(0x3A3F4A, 1.0));
        let x = s[0] + (s[2] - s[0]) * c.progress;
        let _ = self.renderer.fill_rect([s[0], s[1], x, s[3]], srgb(0x3D8BFF, 1.0));
        if c.seekable {
            let k = 7.0 * d;
            let _ = self.renderer.fill_rect([x - k / 2.0, (s[1] + s[3]) / 2.0 - k, x + k / 2.0, (s[1] + s[3]) / 2.0 + k], text_c);
        }
        // Labels: position on the left of the +/- buttons, speed between them.
        if let Some((key, tw, th)) = self.overlay_small(&[c.left.clone()]) {
            let _ = self.renderer.draw_overlay(key, bar.slower[0] - 64.0 * d - 8.0 * d - tw + 36.0 * d, bar.rect[1] + (bar.rect[3] - bar.rect[1] - th) / 2.0, dim_c);
        }
        if let Some((key, tw, th)) = self.overlay_small(&[c.right.clone()]) {
            let (l, r) = (bar.slower[2], bar.faster[0]);
            let _ = self.renderer.draw_overlay(key, l + (r - l - tw) / 2.0, bar.rect[1] + (bar.rect[3] - bar.rect[1] - th) / 2.0, text_c);
        }
    }

    /// A left-button press at (x, y): true if the bar took it (so no zoom / drag / page turn starts under it).
    pub fn controls_press(&mut self, x: i32, y: i32) -> bool {
        let Some(c) = self.controller() else { return false };
        if !self.controls_visible() {
            return false;
        }
        let bar = self.bar_layout();
        let (fx, fy) = (x as f32, y as f32);
        if !inside(&bar.rect, fx, fy) {
            return false;
        }
        if inside(&bar.play, fx, fy) {
            self.toggle_play();
        } else if inside(&bar.prev, fx, fy) && c.kind != MediaKind::Slideshow {
            self.step_frame(-1);
        } else if inside(&bar.next, fx, fy) && c.kind != MediaKind::Slideshow {
            self.step_frame(1);
        } else if inside(&bar.slower, fx, fy) {
            self.adjust_playback(-1);
        } else if inside(&bar.faster, fx, fy) {
            self.adjust_playback(1);
        } else if c.seekable && fx >= bar.seek[0] - 6.0 && fx <= bar.seek[2] + 6.0 {
            self.seek_drag = true;
            self.seek_to(x);
        }
        invalidate(self.hwnd);
        true
    }

    /// Fraction along the seek bar for pointer x.
    fn seek_fraction(&self, x: i32) -> f32 {
        let s = self.bar_layout().seek;
        ((x as f32 - s[0]) / (s[2] - s[0]).max(1.0)).clamp(0.0, 1.0)
    }

    pub fn seek_to(&mut self, x: i32) {
        let frac = self.seek_fraction(x);
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Sequence) => {
                if let Some(run) = self.run_of(self.folder.index) {
                    let to = run.start + (frac * (run.len() - 1) as f32).round() as usize;
                    self.goto_frame(to);
                }
            }
            Some(MediaKind::Animation) => {
                let id = self.id_at(self.folder.index);
                let n = self.renderer.frame_count(id);
                self.anim_paused = true; // scrubbing pauses, like a video player
                self.renderer.set_frame(id, (frac * (n - 1) as f32).round() as usize);
                self.sync_anim();
                set_title(self.hwnd, &self.title());
            }
            _ => {}
        }
        invalidate(self.hwnd);
    }

    /// Go to frame (file) `to` of the current sequence without resetting the zoom.
    fn goto_frame(&mut self, to: usize) {
        if to != self.folder.index && to < self.folder.files.len() {
            self.keep_view_once = true;
            let dir = if to > self.folder.index { 1 } else { -1 };
            self.navigate_with_dir(to, dir);
        }
    }

    /// One frame back or forward (sequence: stays inside the run; animation: wraps and pauses).
    pub fn step_frame(&mut self, d: isize) {
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Sequence) => {
                if let Some(run) = self.run_of(self.folder.index) {
                    let to = (self.folder.index as isize + d).clamp(run.start as isize, run.end as isize) as usize;
                    self.goto_frame(to);
                }
            }
            Some(MediaKind::Animation) => {
                let id = self.id_at(self.folder.index);
                let n = self.renderer.frame_count(id) as isize;
                let f = (self.renderer.frame_index(id) as isize + d).rem_euclid(n) as usize;
                self.anim_paused = true;
                self.renderer.set_frame(id, f);
                self.sync_anim();
                set_title(self.hwnd, &self.title());
            }
            _ => {}
        }
        invalidate(self.hwnd);
    }

    pub fn toggle_play(&mut self) {
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Sequence) => {
                self.seq_playing = !self.seq_playing;
                if self.seq_playing {
                    set_timer(self.hwnd, TIMER_SEQ, 1000 / self.fps.max(1));
                } else {
                    kill_timer(self.hwnd, TIMER_SEQ);
                }
            }
            Some(MediaKind::Animation) => {
                self.anim_paused = !self.anim_paused;
                self.sync_anim();
            }
            Some(MediaKind::Slideshow) => self.stop_slideshow(true),
            _ => self.toast(if self.settings.sequences { "Nothing to play here" } else { "Nothing to play here (Q folds numbered pictures into sequences)" }),
        }
        invalidate(self.hwnd);
    }

    /// Faster / slower: sequence frame rate, animation speed, or the slideshow interval.
    pub fn adjust_playback(&mut self, step: i32) {
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Sequence) => {
                let cur = FPS_STEPS.iter().position(|&f| f >= self.fps).unwrap_or(FPS_STEPS.len() - 1) as i32;
                self.fps = FPS_STEPS[(cur + step).clamp(0, FPS_STEPS.len() as i32 - 1) as usize];
                if self.seq_playing {
                    set_timer(self.hwnd, TIMER_SEQ, 1000 / self.fps.max(1));
                }
                self.toast(format!("{} frames per second", self.fps));
            }
            Some(MediaKind::Animation) => {
                let cur = SPEED_STEPS.iter().position(|&s| (s - self.anim_speed).abs() < 0.01).unwrap_or(3) as i32;
                self.anim_speed = SPEED_STEPS[(cur + step).clamp(0, SPEED_STEPS.len() as i32 - 1) as usize];
                self.sync_anim();
                self.toast(format!("Animation speed {}×", fmt_speed(self.anim_speed)));
            }
            // A slideshow, or nothing playing: the slideshow interval (what +/- always did).
            _ => self.adjust_speed(-step), // longer interval = slower, so "faster" (+1) shortens it
        }
        invalidate(self.hwnd);
    }

    /// Timer: show the next frame of the sequence (looping).
    pub fn seq_tick(&mut self) {
        if !self.seq_playing || self.mode != Mode::Viewer {
            kill_timer(self.hwnd, TIMER_SEQ);
            return;
        }
        let Some(run) = self.run_of(self.folder.index) else {
            self.seq_playing = false;
            kill_timer(self.hwnd, TIMER_SEQ);
            return;
        };
        let next = if self.folder.index >= run.end { run.start } else { self.folder.index + 1 };
        self.goto_frame(next);
    }
}

fn fmt_speed(s: f32) -> String {
    if (s.fract()).abs() < 0.001 { format!("{s:.0}") } else { format!("{s}") }
}
