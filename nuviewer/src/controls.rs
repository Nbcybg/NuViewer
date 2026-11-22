//! Playback controls: one bar at the bottom of the viewer for everything that plays over time.
//!
//! ```text
//!   [<<] [ > ] [>>]  [=====o----------------]  01:12 / 04:30   [2x] [loop] [sound][---o--]
//! ```
//!
//! The same bar serves a **video**, a **sound**, an **animation** (GIF / APNG / WebP), a folded **image sequence** and a
//! **slideshow**: play / pause, step, a seek track with a picture of the spot under the pointer, speed (1x, 2x, 4x, 0.5x),
//! loop, and for video and sound the volume. It appears while the pointer is near the bottom edge (or always, if the
//! setting asks for that, or pinned with `C`). The playing itself is in `media.rs` (video, sound) and the timers of
//! `main.rs` / `ops.rs` (the rest).

use super::*;

const FPS_STEPS: [u32; 11] = [1, 2, 4, 6, 8, 12, 15, 24, 30, 48, 60];
const SPEED_STEPS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];
/// What the speed button cycles through: normal, double, quadruple, half.
const SPEED_CYCLE: [f32; 4] = [1.0, 2.0, 4.0, 0.5];
/// How far the step buttons jump in a video or a sound.
const JUMP_MS: u64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Slideshow,
    Animation,
    Sequence,
    Video,
    Audio,
    /// A 3D model turning by itself (see `turntable.rs`).
    Turntable,
}

/// What the bar shows right now.
pub struct Controller {
    pub kind: MediaKind,
    pub playing: bool,
    /// 0..=1 along the seek bar.
    pub progress: f32,
    pub seekable: bool,
    /// Position text ("01:12 / 04:30", "frame 12/48", "5/200").
    pub time: String,
    pub speed: f32,
    pub looping: bool,
    /// Ping-pong play (animations and sequences only; `None` = the control is not shown).
    pub pingpong: Option<bool>,
    /// Volume 0..=1 and whether it is muted; only for what has sound.
    pub volume: Option<(f32, bool)>,
}

/// Rectangles (client pixels) of the bar and its parts.
pub struct Bar {
    pub rect: [f32; 4],
    pub prev: [f32; 4],
    /// One frame back / forward: a video only.
    pub fprev: Option<[f32; 4]>,
    pub fnext: Option<[f32; 4]>,
    pub play: [f32; 4],
    pub next: [f32; 4],
    pub seek: [f32; 4],
    pub time: [f32; 4],
    pub speed: [f32; 4],
    pub looping: [f32; 4],
    pub pingpong: Option<[f32; 4]>,
    pub mute: Option<[f32; 4]>,
    pub vol: Option<[f32; 4]>,
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

pub fn fmt_time(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{:02}:{:02}", s / 60, s % 60) }
}

impl App {
    /// Something that plays over time is in front of the user.
    pub fn controller(&self) -> Option<Controller> {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return None;
        }
        let i = self.folder.index;
        let id = self.id_at(i);
        let (speed, looping) = (self.anim_speed, self.settings.more.play_loop);
        let pingpong = Some(self.settings.more.play_pingpong);
        if let Some(c) = self.media_controller(id) {
            return Some(c);
        }
        if self.is_model() {
            return Some(Controller { kind: MediaKind::Turntable, playing: self.turntable, progress: self.turntable_progress(), seekable: true, time: self.turntable_text(), speed, looping: true, pingpong: None, volume: None });
        }
        if let Some(run) = self.run_of(i) {
            let n = run.len();
            let pos = i - run.start;
            return Some(Controller {
                kind: MediaKind::Sequence,
                playing: self.seq_playing,
                progress: if n > 1 { pos as f32 / (n - 1) as f32 } else { 0.0 },
                seekable: true,
                time: format!("frame {}/{n}", pos + 1),
                speed,
                looping,
                pingpong,
                volume: None,
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
                time: format!("frame {}/{frames}", f + 1),
                speed,
                looping,
                pingpong,
                volume: None,
            });
        }
        if self.slide.on {
            let n = self.folder.files.len();
            return Some(Controller {
                kind: MediaKind::Slideshow,
                playing: true,
                progress: if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 },
                seekable: n > 1,
                time: format!("{}/{n}", i + 1),
                speed,
                looping,
                pingpong: None,
                volume: None,
            });
        }
        None
    }

    pub fn bar_layout(&self, c: Option<&Controller>) -> Bar {
        let (w, h) = self.renderer.size();
        let (w, h, d) = (w as f32, h as f32, self.gallery.dpi);
        let bar_w = (w * 0.78).clamp(520.0 * d, 1100.0 * d).min(w - 24.0 * d);
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
        let frames = matches!(c.map(|c| c.kind), Some(MediaKind::Video));
        let prev = next_btn(&mut x);
        let fprev = frames.then(|| next_btn(&mut x));
        let play = next_btn(&mut x);
        let fnext = frames.then(|| next_btn(&mut x));
        let next = next_btn(&mut x);
        let mut r = x0 + bar_w - 8.0 * d;
        let has_sound = c.map(|c| c.volume.is_some()).unwrap_or(false);
        let vol = has_sound.then(|| {
            let v = [r - 70.0 * d, y0 + bar_h / 2.0 - 5.0 * d, r, y0 + bar_h / 2.0 + 5.0 * d];
            r -= 78.0 * d;
            v
        });
        let mute = has_sound.then(|| {
            let m = [r - btn, by, r, by + btn];
            r -= btn + 2.0 * d;
            m
        });
        let looping = [r - btn, by, r, by + btn];
        r -= btn + 2.0 * d;
        let pingpong = c.and_then(|c| c.pingpong).map(|_| {
            let p = [r - btn, by, r, by + btn];
            r -= btn + 2.0 * d;
            p
        });
        let speed = [r - 46.0 * d, by, r, by + btn];
        r -= 50.0 * d;
        let time_w = if matches!(c.map(|c| c.kind), Some(MediaKind::Video | MediaKind::Audio)) { 124.0 * d } else { 92.0 * d };
        let time = [r - time_w, by, r, by + btn];
        r -= time_w + 8.0 * d;
        let seek = [x + 6.0 * d, y0 + bar_h / 2.0 - 5.0 * d, r, y0 + bar_h / 2.0 + 5.0 * d];
        Bar { rect: [x0, y0, x0 + bar_w, y0 + bar_h], prev, fprev, fnext, play, next, seek, time, speed, looping, pingpong, mute, vol }
    }

    /// Show the bar? Always for "always", otherwise when the pointer is in the lower part of the window.
    pub fn controls_visible(&self) -> bool {
        let Some(c) = self.controller() else { return false };
        if self.settings.controls_always || self.controls_pinned || self.seek_drag || self.media.vol_drag {
            return true;
        }
        let bar = self.bar_layout(Some(&c));
        let h = self.renderer.size().1 as f32;
        let my = self.last_mouse.1 as f32;
        my >= bar.rect[1] - 40.0 * self.gallery.dpi && my < h
    }

    /// The pointer moved: while it is over the bar the highlight and the picture over the track follow it.
    pub fn controls_hover(&mut self, y: i32) {
        if let Some(c) = self.controller() {
            if self.controls_visible() && y as f32 >= self.bar_layout(Some(&c)).rect[1] - 30.0 * self.gallery.dpi {
                invalidate(self.hwnd);
            }
        }
    }

    pub fn draw_controls(&mut self) {
        let Some(c) = self.controller() else { return };
        if !self.controls_visible() {
            return;
        }
        let bar = self.bar_layout(Some(&c));
        let d = self.gallery.dpi;
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let (text_c, dim_c, accent) = (srgb(0xF2F2F2, 1.0), srgb(0x9AA0A6, 1.0), srgb(0x3D8BFF, 1.0));
        let _ = self.renderer.fill_rect(bar.rect, srgb(0x0E0E0E, 0.88));
        let glyph = |this: &mut App, r: [f32; 4], text: &str, enabled: bool, on: bool| {
            if on {
                let _ = this.renderer.fill_rect(r, srgb(0x3D8BFF, 0.35));
            } else if inside(&r, mx, my) && enabled {
                let _ = this.renderer.fill_rect(r, srgb(0x3A3F4A, 0.9));
            }
            if let Some((key, tw, th)) = this.overlay_small(&[text.to_string()]) {
                let _ = this.renderer.draw_overlay(key, r[0] + (r[2] - r[0] - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, if enabled { text_c } else { dim_c });
            }
        };
        let stepping = c.kind != MediaKind::Slideshow;
        glyph(self, bar.prev, "\u{25C0}\u{25C0}", stepping, false);
        if let Some(r) = bar.fprev {
            glyph(self, r, "\u{258F}\u{25C0}", true, false);
        }
        if let Some(r) = bar.fnext {
            glyph(self, r, "\u{25B6}\u{2595}", true, false);
        }
        glyph(self, bar.play, if c.playing { "\u{275A}\u{275A}" } else { "\u{25B6}" }, true, false);
        glyph(self, bar.next, "\u{25B6}\u{25B6}", stepping, false);
        glyph(self, bar.speed, &format!("{}\u{00D7}", fmt_speed(c.speed)), true, (c.speed - 1.0).abs() > 0.01);
        glyph(self, bar.looping, "\u{21BB}", true, c.looping);
        if let (Some(r), Some(on)) = (bar.pingpong, c.pingpong) {
            glyph(self, r, "\u{21C4}", true, on);
        }
        if let (Some(m), Some((_, muted))) = (bar.mute, c.volume) {
            glyph(self, m, if muted { "\u{1F507}" } else { "\u{1F50A}" }, true, false);
        }
        // Seek track: grey, with the played part in the accent colour and a knob.
        let s = bar.seek;
        let _ = self.renderer.fill_rect(s, srgb(0x3A3F4A, 1.0));
        let x = s[0] + (s[2] - s[0]) * c.progress;
        let _ = self.renderer.fill_rect([s[0], s[1], x, s[3]], accent);
        if c.seekable {
            let k = 7.0 * d;
            let _ = self.renderer.fill_rect([x - k / 2.0, (s[1] + s[3]) / 2.0 - k, x + k / 2.0, (s[1] + s[3]) / 2.0 + k], text_c);
        }
        // Slideshow: the time left on this picture, as a thin line under the track.
        if c.kind == MediaKind::Slideshow {
            let left = (self.slide.since.elapsed().as_secs_f32() * self.anim_speed / self.slide.secs).clamp(0.0, 1.0);
            let _ = self.renderer.fill_rect([s[0], s[3] + 3.0 * d, s[0] + (s[2] - s[0]) * left, s[3] + 5.0 * d], srgb(0x7DB4FF, 0.8));
        }
        if let Some(v) = bar.vol {
            let (level, muted) = c.volume.unwrap_or((0.0, false));
            let _ = self.renderer.fill_rect(v, srgb(0x3A3F4A, 1.0));
            let vx = v[0] + (v[2] - v[0]) * if muted { 0.0 } else { level };
            let _ = self.renderer.fill_rect([v[0], v[1], vx, v[3]], if muted { dim_c } else { accent });
            let k = 6.0 * d;
            let _ = self.renderer.fill_rect([vx - k / 2.0, (v[1] + v[3]) / 2.0 - k, vx + k / 2.0, (v[1] + v[3]) / 2.0 + k], text_c);
        }
        if let Some((key, tw, th)) = self.overlay_small(&[c.time.clone()]) {
            let t = bar.time;
            let _ = self.renderer.draw_overlay(key, t[0] + (t[2] - t[0] - tw) / 2.0, bar.rect[1] + (bar.rect[3] - bar.rect[1] - th) / 2.0, dim_c);
        }
        self.draw_scrub_preview(&c, &bar);
    }

    /// A picture of the spot under the pointer, above the seek track: a frame of a video or an animation, the neighbouring
    /// picture of a sequence or slideshow.
    fn draw_scrub_preview(&mut self, c: &Controller, bar: &Bar) {
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let d = self.gallery.dpi;
        let s = bar.seek;
        if !self.settings.more.scrub_preview || !c.seekable || mx < s[0] - 4.0 || mx > s[2] + 4.0 || my < s[1] - 14.0 * d || my > s[3] + 14.0 * d {
            return;
        }
        let frac = ((mx - s[0]) / (s[2] - s[0]).max(1.0)).clamp(0.0, 1.0);
        let n = |count: usize| (frac * (count.max(1) - 1) as f32).round() as usize;
        let (win_w, _) = self.renderer.size();
        let max_edge = 200.0 * d;
        // The picture (texture) and the label.
        let label;
        let mut shown: Option<(f32, f32)> = None; // size of the preview
        let mut draw: Box<dyn Fn(&mut App, [f32; 4])> = Box::new(|_, _| {});
        match c.kind {
            MediaKind::Video | MediaKind::Audio => {
                let ms = (frac as f64 * self.media_duration_ms() as f64) as u64;
                label = fmt_time(ms);
                if c.kind == MediaKind::Video {
                    self.media_request_preview(ms);
                    if let Some((w, h)) = self.media_preview_size() {
                        shown = Some((w as f32, h as f32));
                        draw = Box::new(|app, r| app.media_draw_preview(r));
                    }
                }
            }
            MediaKind::Animation => {
                let id = self.id_at(self.folder.index);
                let frames = self.renderer.frame_count(id);
                let f = n(frames);
                label = format!("frame {}", f + 1);
                if let Some((w, h)) = self.renderer.image_size_of(id) {
                    shown = Some((w as f32, h as f32));
                    draw = Box::new(move |app, r| {
                        let _ = app.renderer.draw_frame(id, f, r);
                    });
                }
            }
            // The turntable's preview is the angle under the pointer (no picture).
            MediaKind::Turntable => {
                label = format!("{:.0}\u{00B0}", frac * 360.0);
            }
            MediaKind::Sequence | MediaKind::Slideshow => {
                let (first, count) = match (c.kind, self.run_of(self.folder.index)) {
                    (MediaKind::Sequence, Some(run)) => (run.start, run.len()),
                    _ => (0, self.folder.files.len()),
                };
                let at = first + n(count);
                label = format!("{}", at + 1);
                let id = self.id_at(at);
                if let Some((w, h)) = self.renderer.image_size_of(id).or_else(|| self.renderer.thumb_size(id)) {
                    shown = Some((w as f32, h as f32));
                    draw = Box::new(move |app, r| {
                        if app.renderer.has(id) {
                            let _ = app.renderer.draw_image(id, r, false);
                        } else {
                            let _ = app.renderer.draw_thumb(id, r);
                        }
                    });
                }
            }
        }
        let label_img = self.overlay_small(&[label]);
        let (pw, ph) = match shown {
            // A video's preview is the picture the scrubber made, drawn 1:1; the others are scaled to fit.
            Some((w, h)) if c.kind == MediaKind::Video => (w, h),
            Some((w, h)) => {
                let k = max_edge / w.max(h);
                ((w * k).round(), (h * k).round())
            }
            None => (0.0, 0.0),
        };
        let (lw, lh) = label_img.map(|l| (l.1, l.2)).unwrap_or((0.0, 0.0));
        let box_w = pw.max(lw) + 8.0 * d;
        let box_h = ph + lh + 8.0 * d;
        let left = (mx - box_w / 2.0).clamp(4.0, win_w as f32 - box_w - 4.0);
        let top = bar.rect[1] - box_h - 8.0 * d;
        let _ = self.renderer.fill_rect([left, top, left + box_w, top + box_h], srgb(0x0E0E0E, 0.92));
        if pw > 0.0 {
            let px = left + (box_w - pw) / 2.0;
            draw(self, [px.round(), (top + 4.0 * d).round(), (px + pw).round(), (top + 4.0 * d + ph).round()]);
        }
        if let Some((key, tw, _)) = label_img {
            let _ = self.renderer.draw_overlay(key, left + (box_w - tw) / 2.0, top + 4.0 * d + ph, srgb(0xF2F2F2, 1.0));
        }
        // The pointer's own spot on the track.
        let _ = self.renderer.fill_rect([mx - 1.0, s[1] - 3.0 * d, mx + 1.0, s[3] + 3.0 * d], srgb(0xF2F2F2, 0.9));
    }

    /// A left-button press at (x, y): true if the bar took it (so no zoom / drag / page turn starts under it).
    pub fn controls_press(&mut self, x: i32, y: i32) -> bool {
        let Some(c) = self.controller() else { return false };
        if !self.controls_visible() {
            return false;
        }
        let bar = self.bar_layout(Some(&c));
        let (fx, fy) = (x as f32, y as f32);
        if !inside(&bar.rect, fx, fy) {
            return false;
        }
        if inside(&bar.play, fx, fy) {
            self.toggle_play();
        } else if bar.fprev.is_some_and(|r| inside(&r, fx, fy)) {
            self.frame_step(false);
        } else if bar.fnext.is_some_and(|r| inside(&r, fx, fy)) {
            self.frame_step(true);
        } else if inside(&bar.prev, fx, fy) && c.kind != MediaKind::Slideshow {
            self.step_frame(-1);
        } else if inside(&bar.next, fx, fy) && c.kind != MediaKind::Slideshow {
            self.step_frame(1);
        } else if inside(&bar.speed, fx, fy) {
            self.cycle_speed();
        } else if inside(&bar.looping, fx, fy) {
            self.toggle_loop();
        } else if bar.pingpong.is_some_and(|r| inside(&r, fx, fy)) {
            self.toggle_pingpong();
        } else if bar.mute.is_some_and(|m| inside(&m, fx, fy)) {
            self.toggle_mute();
        } else if bar.vol.is_some_and(|v| fx >= v[0] - 6.0 && fx <= v[2] + 6.0) {
            self.media.vol_drag = true;
            self.set_volume_at(x);
        } else if c.seekable && fx >= bar.seek[0] - 6.0 && fx <= bar.seek[2] + 6.0 {
            self.seek_drag = true;
            self.seek_to(x);
        }
        invalidate(self.hwnd);
        true
    }

    /// Fraction along the seek bar for pointer x.
    fn seek_fraction(&self, x: i32) -> f32 {
        let s = self.bar_layout(self.controller().as_ref()).seek;
        ((x as f32 - s[0]) / (s[2] - s[0]).max(1.0)).clamp(0.0, 1.0)
    }

    pub fn seek_to(&mut self, x: i32) {
        let frac = self.seek_fraction(x);
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Video | MediaKind::Audio) => self.media_seek_fraction(frac),
            Some(MediaKind::Turntable) => self.turntable_seek(frac),
            Some(MediaKind::Sequence) => {
                if let Some(run) = self.run_of(self.folder.index) {
                    let to = run.start + (frac * (run.len() - 1) as f32).round() as usize;
                    self.goto_frame(to);
                }
            }
            Some(MediaKind::Slideshow) => {
                let to = (frac * (self.folder.files.len() - 1) as f32).round() as usize;
                self.goto_frame(to);
            }
            Some(MediaKind::Animation) => {
                let id = self.id_at(self.folder.index);
                let n = self.renderer.frame_count(id);
                self.anim_paused = true; // scrubbing pauses, like a video player
                self.renderer.set_frame(id, (frac * (n - 1) as f32).round() as usize);
                self.sync_anim();
                set_title(self.hwnd, &self.title());
            }
            None => {}
        }
        invalidate(self.hwnd);
    }

    /// Dragging on the volume track.
    pub fn set_volume_at(&mut self, x: i32) {
        let Some(v) = self.bar_layout(self.controller().as_ref()).vol else { return };
        let frac = ((x as f32 - v[0]) / (v[2] - v[0]).max(1.0)).clamp(0.0, 1.0);
        self.settings.more.volume = (frac * 100.0).round() as u8;
        self.settings.more.muted = false;
        self.media_apply_volume();
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

    /// One step back or forward (sequence: one frame, stays inside the run; animation: one frame, wraps and pauses;
    /// video and sound: ten seconds).
    pub fn step_frame(&mut self, d: isize) {
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Video | MediaKind::Audio) => self.media_jump(d as i64 * JUMP_MS as i64),
            Some(MediaKind::Turntable) => self.turntable_step(d),
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
        if self.anim_active() {
            self.anim_play();
            return;
        }
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Video | MediaKind::Audio) => self.media_toggle_play(),
            Some(MediaKind::Turntable) => self.turntable_toggle(),
            Some(MediaKind::Sequence) => {
                self.seq_playing = !self.seq_playing;
                if self.seq_playing {
                    set_timer(self.hwnd, TIMER_SEQ, self.seq_ms());
                } else {
                    kill_timer(self.hwnd, TIMER_SEQ);
                }
            }
            Some(MediaKind::Animation) => {
                self.anim_paused = !self.anim_paused;
                if !self.anim_paused && self.renderer.frame_index(self.id_at(self.folder.index)) == 0 {
                    self.pp_back = false;
                }
                self.sync_anim();
            }
            Some(MediaKind::Slideshow) => self.stop_slideshow(true),
            None => self.toast(if self.settings.sequences { "Nothing to play here" } else { "Nothing to play here (Q folds numbered pictures into sequences)" }),
        }
        invalidate(self.hwnd);
    }

    /// Speed button / key: 1x, 2x, 4x, 0.5x, and round again. Applies to whatever is playing.
    pub fn cycle_speed(&mut self) {
        let at = SPEED_CYCLE.iter().position(|&s| (s - self.anim_speed).abs() < 0.01);
        self.anim_speed = SPEED_CYCLE[at.map(|i| (i + 1) % SPEED_CYCLE.len()).unwrap_or(0)];
        self.apply_speed();
        self.toast(format!("Speed {}\u{00D7}", fmt_speed(self.anim_speed)));
    }

    /// The speed in `anim_speed` takes effect: re-arm the timers (or tell the player).
    fn apply_speed(&mut self) {
        self.sync_anim();
        if self.seq_playing {
            set_timer(self.hwnd, TIMER_SEQ, self.seq_ms());
        }
        if self.slide.on {
            set_timer(self.hwnd, TIMER_SLIDE, self.slide_ms());
        }
        self.media_apply_speed();
        invalidate(self.hwnd);
    }

    /// Frame interval of a playing image sequence.
    pub fn seq_ms(&self) -> u32 {
        ((1000.0 / (self.fps.max(1) as f32 * self.anim_speed)).round() as u32).max(5)
    }

    /// Interval of the slideshow at the current speed.
    pub fn slide_ms(&self) -> u32 {
        ((self.slide.secs * 1000.0 / self.anim_speed).round() as u32).max(100)
    }

    pub fn toggle_loop(&mut self) {
        self.settings.more.play_loop = !self.settings.more.play_loop;
        let _ = self.settings.save();
        self.media_apply_loop();
        self.toast(if self.settings.more.play_loop { "Loop on" } else { "Loop off" });
        invalidate(self.hwnd);
    }

    /// RAW files: the camera's own look (its embedded JPEG) or the sensor data; every RAW is decoded again.
    pub fn toggle_raw_look(&mut self) {
        let mut s = self.settings.clone();
        s.raw_mode = s.raw_mode.flipped();
        let text = s.raw_mode.look_name();
        self.apply_settings(s);
        self.toast(text);
    }

    /// Ping-pong: an animation or sequence plays forward, then backward, and so on.
    pub fn toggle_pingpong(&mut self) {
        self.settings.more.play_pingpong = !self.settings.more.play_pingpong;
        let _ = self.settings.save();
        self.pp_back = false;
        self.toast(if self.settings.more.play_pingpong { "Ping-pong on (forward, then backward)" } else { "Ping-pong off" });
        invalidate(self.hwnd);
    }

    pub fn toggle_mute(&mut self) {
        self.settings.more.muted = !self.settings.more.muted;
        let _ = self.settings.save();
        self.media_apply_volume();
        self.toast(if self.settings.more.muted { "Sound off" } else { "Sound on" });
        invalidate(self.hwnd);
    }

    /// Faster / slower with + and -: sequence frame rate, animation speed, the slideshow interval, video speed.
    pub fn adjust_playback(&mut self, step: i32) {
        match self.controller().map(|c| c.kind) {
            Some(MediaKind::Sequence) => {
                let cur = FPS_STEPS.iter().position(|&f| f >= self.fps).unwrap_or(FPS_STEPS.len() - 1) as i32;
                self.fps = FPS_STEPS[(cur + step).clamp(0, FPS_STEPS.len() as i32 - 1) as usize];
                if self.seq_playing {
                    set_timer(self.hwnd, TIMER_SEQ, self.seq_ms());
                }
                self.toast(format!("{} frames per second", self.fps));
            }
            Some(MediaKind::Animation | MediaKind::Video | MediaKind::Audio | MediaKind::Turntable) => {
                let cur = SPEED_STEPS.iter().position(|&s| (s - self.anim_speed).abs() < 0.01).unwrap_or(3) as i32;
                self.anim_speed = SPEED_STEPS[(cur + step).clamp(0, SPEED_STEPS.len() as i32 - 1) as usize];
                self.apply_speed();
                self.toast(format!("Speed {}\u{00D7}", fmt_speed(self.anim_speed)));
            }
            // A slideshow, or nothing playing: the slideshow interval (what +/- always did).
            _ => self.adjust_speed(-step), // longer interval = slower, so "faster" (+1) shortens it
        }
        invalidate(self.hwnd);
    }

    /// Timer: show the next frame of the sequence (looping, or stopping at the end).
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
        let (i, looping) = (self.folder.index, self.settings.more.play_loop);
        if self.settings.more.play_pingpong && run.end > run.start {
            // Forward to the last frame, then back to the first; with loop off the play ends there (one trip).
            if !self.pp_back && i >= run.end {
                self.pp_back = true;
            } else if self.pp_back && i <= run.start {
                if !looping {
                    self.pp_back = false;
                    self.seq_playing = false;
                    kill_timer(self.hwnd, TIMER_SEQ);
                    invalidate(self.hwnd);
                    return;
                }
                self.pp_back = false;
            }
            self.goto_frame(if self.pp_back { i - 1 } else { i + 1 });
            return;
        }
        if i >= run.end && !looping {
            self.seq_playing = false;
            kill_timer(self.hwnd, TIMER_SEQ);
            invalidate(self.hwnd);
            return;
        }
        let next = if i >= run.end { run.start } else { i + 1 };
        self.goto_frame(next);
    }
}

pub(crate) fn fmt_speed(s: f32) -> String {
    if (s.fract()).abs() < 0.001 { format!("{s:.0}") } else { format!("{s}") }
}
