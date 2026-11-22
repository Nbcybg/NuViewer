//! The Spine player: a skeleton on screen is posed and drawn by `nv-anim` (on the CPU) every few milliseconds, **at the size of the window**
//! (one texel per screen pixel, so it is sharp at any window size and any zoom), and the result replaces the picture's texture. Zoom and pan
//! live in the player (wheel, drag, the zoom keys), not in the picture view, because a zoomed skeleton is drawn again at the new size.
//!
//! How much is drawn per frame adapts: while it plays the frame has a budget of about 28 ms (the resolution drops a little on a huge window
//! or a slow PC); when it stands still, or a moment after a zoom or a drag stops, it is drawn once more with 2x supersampling for smooth edges.
//!
//! Playback uses the viewer's shared settings: speed (`anim_speed`, Shift+. / the panel), loop (Shift+L) and ping-pong (Shift+K); `P` / Space
//! play and pause. The animation, skin and slot (layer) choices are in the layer panel (`Ctrl+L`, `layers_ui.rs`), which also exports a
//! snapshot, a numbered PNG sequence with transparency and a WebM with transparency (needs ffmpeg with libvpx-vp9).

use super::*;
use nv_anim::Rig;
use nv_codecs::spinefmt;
use crate::controls::fmt_speed;

pub const TIMER_SPINE: u32 = 12;
const TICK_MS: u32 = 33;

pub struct Player {
    pub id: u64,
    pub path: PathBuf,
    pub rig: Arc<Rig>,
    pub skin: usize,
    pub anim: Option<usize>,
    pub time: f32,
    pub playing: bool,
    /// +1 forward, -1 backward (ping-pong).
    pub dir: f32,
    /// The box of the whole animation (skeleton units); the view fits it to the window.
    pub bounds: [f32; 4],
    /// 1 = fitted to the window; the zoom keys and the wheel change it.
    pub zoom: f32,
    /// How far the picture is moved from the middle of the window, in window pixels.
    pub pan: (f32, f32),
    /// Milliseconds to draw one megapixel at 1x supersampling (0 = not measured yet): what the quality choice rests on.
    pub cost: f32,
    /// When to draw the crisp version (after a zoom or a drag, or at a standstill).
    pub settle: Option<Instant>,
    /// The window size the last frame was drawn for.
    pub win: (u32, u32),
    /// What the layer panel says per slot (0 = off, 1 = as the file has it).
    pub slot_alpha: Vec<f32>,
    pub last: Instant,
    pub dirty: bool,
    /// What the springs (4.2+ physics constraints) remember from frame to frame.
    pub sim: nv_anim::PhysicsSim,
    /// The animation being left (number, its time) and how far the new one has taken over (seconds of the fade so far).
    pub fade: Option<(usize, f32, f32)>,
}

/// How long one animation eases into the next.
const FADE_SECONDS: f32 = 0.25;

pub struct Spine {
    pub rigs: HashMap<u64, Arc<Rig>>,
    pub player: Option<Player>,
}

impl Spine {
    pub fn new() -> Spine {
        Spine { rigs: HashMap::new(), player: None }
    }
}

/// One frame of `rig` as straight RGBA: the pose at `time`, slots dimmed by `alpha`.
#[allow(clippy::too_many_arguments)]
pub fn render_pose(rig: &Rig, skin: usize, anim: Option<usize>, time: f32, alpha: &[f32], w: usize, h: usize, view: nv_anim::View, ssaa: usize, background: Option<[f32; 3]>, sim: Option<&mut nv_anim::PhysicsSim>) -> Vec<u8> {
    render_pose_mix(rig, skin, None, anim, time, 1.0, alpha, w, h, view, ssaa, background, sim)
}

/// `render_pose` easing from another animation (see `Rig::pose_mix`).
#[allow(clippy::too_many_arguments)]
pub fn render_pose_mix(rig: &Rig, skin: usize, from: Option<(usize, f32)>, anim: Option<usize>, time: f32, mix: f32, alpha: &[f32], w: usize, h: usize, view: nv_anim::View, ssaa: usize, background: Option<[f32; 3]>, sim: Option<&mut nv_anim::PhysicsSim>) -> Vec<u8> {
    let mut pose = rig.pose_mix(skin, from, anim, time, mix, sim);
    for (s, a) in pose.slots.iter_mut().zip(alpha) {
        s.user_alpha = *a;
    }
    rig.draw(&pose, w, h, view, ssaa, background)
}

impl Player {
    pub fn duration(&self) -> f32 {
        self.anim.and_then(|a| self.rig.animations.get(a)).map(|a| a.duration).unwrap_or(0.0)
    }

    pub fn anim_name(&self) -> String {
        self.anim.and_then(|a| self.rig.animations.get(a)).map(|a| a.name.clone()).unwrap_or_else(|| "(setup pose)".into())
    }

    /// Choose an animation (None = the setup pose) and fit the canvas to the whole of its motion.
    pub fn set_anim(&mut self, anim: Option<usize>) {
        // Ease out of the animation that was playing (a single bone pose swaps in 0.25 s instead of jumping).
        self.fade = self.anim.filter(|_| self.rig.animations.len() > 1 && anim != self.anim).map(|a| (a, self.time, 0.0));
        self.anim = anim;
        self.time = 0.0;
        self.dir = 1.0;
        self.sim.reset();
        self.bounds = spinefmt::bounds_for(&self.rig, self.skin, anim);
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
        self.dirty = true;
    }

    /// Screen pixels per skeleton unit when fitted to a `win` window (a little air round the picture).
    pub fn base_scale(&self, win: (u32, u32)) -> f32 {
        let (bw, bh) = ((self.bounds[2] - self.bounds[0]).max(1.0), (self.bounds[3] - self.bounds[1]).max(1.0));
        (win.0.max(1) as f32 / bw).min(win.1.max(1) as f32 / bh) * 0.94
    }

    /// The view for a texture of `tex` pixels standing for a `win` window.
    pub fn rig_view(&self, win: (u32, u32), tex: (usize, usize)) -> nv_anim::View {
        let k = tex.0 as f32 / win.0.max(1) as f32; // texture pixels per window pixel
        let scale = self.base_scale(win) * self.zoom * k;
        let (cx, cy) = ((self.bounds[0] + self.bounds[2]) / 2.0, (self.bounds[1] + self.bounds[3]) / 2.0);
        nv_anim::View { cx: cx - self.pan.0 * k / scale, cy: cy + self.pan.1 * k / scale, scale }
    }

    /// Zoom by `factor` about the window point `at` (the point under it stays where it is).
    pub fn zoom_about(&mut self, win: (u32, u32), at: (f32, f32), factor: f32) {
        // The point zoomed about is never outside the picture: over the empty window it becomes the nearest edge of the skeleton.
        let s = self.base_scale(win) * self.zoom;
        let (hw, hh) = ((self.bounds[2] - self.bounds[0]).max(1.0) * s / 2.0, (self.bounds[3] - self.bounds[1]).max(1.0) * s / 2.0);
        let (mx, my) = (win.0 as f32 / 2.0 + self.pan.0, win.1 as f32 / 2.0 + self.pan.1);
        let at = (at.0.clamp(mx - hw, mx + hw), at.1.clamp(my - hh, my + hh));
        let new = (self.zoom * factor).clamp(0.2, 60.0);
        let f = new / self.zoom;
        let (dx, dy) = (at.0 - win.0 as f32 / 2.0, at.1 - win.1 as f32 / 2.0);
        self.pan = (dx * (1.0 - f) + self.pan.0 * f, dy * (1.0 - f) + self.pan.1 * f);
        self.zoom = new;
        self.dirty = true;
    }
}

/// The most pixels one live frame has (a 4K window is drawn a little smaller and the graphics card stretches it).
const LIVE_MAX_MPX: f32 = 4.2;
/// Budget of a frame while it plays / while the picture is being zoomed or dragged / for the crisp frame at rest (milliseconds).
const BUDGET_PLAY: f32 = 28.0;
const BUDGET_MOVE: f32 = 70.0;
const BUDGET_REST: f32 = 450.0;

impl App {
    pub fn spine_active(&self) -> bool {
        self.mode == Mode::Viewer && self.spine.player.is_some() && !self.editor.open
    }

    /// Follow the picture in front: a skeleton gets a player (and its timer), anything else drops it. Called from `poll_background`.
    pub fn spine_sync(&mut self) {
        self.spine.rigs.retain(|id, _| self.renderer.has(*id));
        let cur = if self.folder.files.is_empty() { None } else { Some(self.id_at(self.folder.index)) };
        if let Some(p) = &self.spine.player {
            if Some(p.id) != cur || self.mode != Mode::Viewer {
                self.spine.player = None;
                kill_timer(self.hwnd, TIMER_SPINE);
            }
        }
        if self.spine.player.is_none() && self.mode == Mode::Viewer {
            if let (Some(id), Some(path)) = (cur, self.folder.files.get(self.folder.index).cloned()) {
                if let Some(rig) = self.spine.rigs.get(&id).cloned() {
                    if self.renderer.has(id) && self.renderer.current() == Some(id) {
                        let skin = 0;
                        let anim = if rig.animations.is_empty() { None } else { Some(rig.animation_index("idle").or_else(|| rig.animation_index("Idle")).unwrap_or(0)) };
                        let bounds = spinefmt::bounds_for(&rig, skin, anim);
                        let n = rig.slots.len();
                        self.view = self.view.fresh();
                        self.spine.player = Some(Player { id, path, rig, skin, anim, time: 0.0, playing: true, dir: 1.0, bounds, zoom: 1.0, pan: (0.0, 0.0), cost: 0.0, settle: None, win: (0, 0), slot_alpha: vec![1.0; n], last: Instant::now(), dirty: true, sim: nv_anim::PhysicsSim::new(), fade: None });
                        set_timer(self.hwnd, TIMER_SPINE, TICK_MS);
                        self.toast("Spine skeleton: P plays / pauses, wheel zooms, drag pans, Ctrl+L = animations, skins and layers");
                    }
                }
            }
        }
    }

    pub fn spine_tick(&mut self) {
        let loop_on = self.settings.more.play_loop;
        let ping = self.settings.more.play_pingpong;
        let speed = self.anim_speed.max(0.05);
        let Some(p) = self.spine.player.as_mut() else { return };
        let now = Instant::now();
        let dt = now.duration_since(p.last).as_secs_f32().min(0.25);
        p.last = now;
        let dur = p.duration();
        // Physics keeps swinging for as long as the skeleton plays, whatever the animation does (even a still pose).
        if p.playing && !p.rig.physics.is_empty() {
            p.sim.advance(dt * speed);
            p.dirty = true;
        }
        if let Some((fa, ft, el)) = p.fade {
            let el = el + dt;
            let old_len = p.rig.animations.get(fa).map(|a| a.duration).unwrap_or(0.0);
            // (The old animation keeps moving while it fades out, looping if it must.)
            let ft = if old_len > 0.0 { (ft + dt * speed).rem_euclid(old_len) } else { ft };
            p.fade = if el >= FADE_SECONDS { None } else { Some((fa, ft, el)) };
            p.dirty = true;
        }
        if p.playing && dur > 0.0 {
            p.time += dt * speed * p.dir;
            if ping {
                if p.time >= dur {
                    p.time = dur;
                    p.dir = -1.0;
                } else if p.time <= 0.0 {
                    p.time = 0.0;
                    p.dir = 1.0;
                }
            } else if loop_on {
                p.time = p.time.rem_euclid(dur);
            } else if p.time >= dur {
                p.time = dur;
                p.playing = false;
            }
            p.dirty = true;
        }
        // A resized window needs a frame of its own size.
        let win = self.renderer.size();
        if p.win != win {
            p.dirty = true;
        }
        let settle_due = !p.playing && p.settle.is_some_and(|t| now >= t);
        if settle_due {
            p.settle = None;
            self.spine_render_with(true);
        } else if p.dirty {
            self.spine_render_with(false);
        }
    }

    pub fn spine_render(&mut self) {
        self.spine_render_with(false);
    }

    /// Draw the skeleton for the window as it is now. `rest`: the crisp version (the quality that fits a long budget).
    pub fn spine_render_with(&mut self, rest: bool) {
        let win = self.renderer.size();
        let Some(p) = self.spine.player.as_mut() else { return };
        p.dirty = false;
        let (ww, wh) = (win.0.max(1) as f32, win.1.max(1) as f32);
        let mpx = ww * wh / 1.0e6;
        let q_cap = (LIVE_MAX_MPX / mpx).sqrt().min(1.0);
        let budget = if rest { BUDGET_REST } else if p.playing { BUDGET_PLAY } else { BUDGET_MOVE };
        // The best of these that fits the budget, by what the last frame cost (unknown: the cheap full-size one).
        let mut pick = (q_cap, 1usize);
        if p.cost > 0.0 {
            let ladder: [(f32, usize); 6] = [(1.0, 2), (1.0, 1), (0.85, 1), (0.7, 1), (0.55, 1), (0.4, 1)];
            for (i, (q, ss)) in ladder.iter().enumerate() {
                let q = q.min(q_cap);
                if p.cost * mpx * q * q * (ss * ss) as f32 <= budget || i == ladder.len() - 1 {
                    pick = (q, *ss);
                    break;
                }
            }
        }
        let (q, ss) = pick;
        let tex = (((ww * q).round() as usize).max(16), ((wh * q).round() as usize).max(16));
        let view = p.rig_view(win, tex);
        let (from, mix) = match p.fade {
            Some((fa, ft, el)) => (Some((fa, ft)), (el / FADE_SECONDS).clamp(0.0, 1.0)),
            None => (None, 1.0),
        };
        let t0 = Instant::now();
        let rgba = render_pose_mix(&p.rig, p.skin, from, p.anim, p.time, mix, &p.slot_alpha, tex.0, tex.1, view, ss, None, Some(&mut p.sim));
        let id = p.id;
        let ok = self.renderer.upload_live(id, tex.0 as u32, tex.1 as u32, &rgba).is_ok();
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        if let Some(p) = self.spine.player.as_mut() {
            p.win = win;
            let unit = ms / ((tex.0 * tex.1) as f32 / 1.0e6 * (ss * ss) as f32).max(0.01);
            p.cost = if p.cost > 0.0 { p.cost * 0.6 + unit * 0.4 } else { unit };
            // Not the crisp quality yet (a zoom or a drag, a heavy frame): draw it once more when things have been still for a moment.
            p.settle = if !rest && !p.playing && (ss < 2 || q < 1.0) { Some(Instant::now() + std::time::Duration::from_millis(160)) } else { None };
        }
        if ok {
            self.renderer.show(id);
            invalidate(self.hwnd);
        }
    }

    pub fn spine_toggle_play(&mut self) {
        if let Some(p) = self.spine.player.as_mut() {
            p.playing = !p.playing;
            if p.playing && p.duration() > 0.0 && p.time >= p.duration() && !self.settings.more.play_pingpong {
                p.time = 0.0;
            }
            p.last = Instant::now();
            let msg = if p.playing { "Play" } else { "Pause" };
            self.toast(msg);
        }
    }

    /// Next / previous animation (the setup pose is one more stop at the end of the list).
    pub fn spine_step_anim(&mut self, d: i32) {
        let Some(p) = self.spine.player.as_mut() else { return };
        let n = p.rig.animations.len() as i32 + 1;
        let cur = p.anim.map(|a| a as i32).unwrap_or(n - 1);
        let next = (cur + d).rem_euclid(n);
        p.set_anim(if next == n - 1 { None } else { Some(next as usize) });
        self.view = self.view.fresh();
        self.spine_render();
    }

    pub fn spine_next_skin(&mut self) {
        let Some(p) = self.spine.player.as_mut() else { return };
        let n = p.rig.skins.len();
        p.skin = (p.skin + 1) % n.max(1);
        p.bounds = spinefmt::bounds_for(&p.rig, p.skin, p.anim);
        p.dirty = true;
        self.spine_render();
    }

    /// (name, blend note) of every slot, top of the draw order first.
    pub fn spine_nodes(&self) -> Vec<(String, String)> {
        self.spine.player.as_ref().map(|p| p.rig.slots.iter().rev().map(|s| (s.name.clone(), if s.blend == nv_anim::BlendMode::Normal { String::new() } else { format!("{:?}", s.blend) })).collect()).unwrap_or_default()
    }

    /// The line the panel shows above its buttons.
    pub fn spine_info(&self) -> String {
        match &self.spine.player {
            Some(p) => {
                let n = p.rig.animations.len();
                let at = p.anim.map(|a| a + 1).unwrap_or(0);
                format!("{} ({at}/{n})   skin {}   {:.2}/{:.2} s   {}\u{00D7}", p.anim_name(), p.rig.skins.get(p.skin).map(|s| s.name.as_str()).unwrap_or("-"), p.time, p.duration(), fmt_speed(self.anim_speed))
            }
            None => String::new(),
        }
    }

    /// The panel's slot rows changed: dim / hide slots in the picture.
    pub fn spine_apply_slots(&mut self, eff: &[nv_codecs::layerdoc::Eff]) {
        if let Some(p) = self.spine.player.as_mut() {
            for (i, a) in p.slot_alpha.iter_mut().enumerate() {
                *a = eff.get(i).map(|e| if e.on { e.opacity } else { 0.0 }).unwrap_or(1.0);
            }
            p.dirty = true;
        }
        self.spine_render();
    }

    // ---- mouse: the picture view does the zoom; drag pans ----------------------------------------------------------------------------

    pub fn spine_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        if !self.anim_active() {
            return false;
        }
        let win = self.renderer.size();
        if let Some(p) = self.spine.player.as_mut() {
            p.zoom_about(win, (x as f32, y as f32), 1.15f32.powf(delta as f32 / 120.0));
            self.spine_render_with(false);
            return true;
        }
        if let Some(i) = self.view_size() {
            self.view.zoom_at(i, win, (x, y), delta as f32 / 120.0);
            invalidate(self.hwnd);
        }
        true
    }

    pub fn spine_press(&mut self, x: i32, y: i32) -> bool {
        if !self.anim_active() {
            return false;
        }
        self.view.drag_last = Some((x, y));
        true
    }

    pub fn spine_drag(&mut self, x: i32, y: i32, left_down: bool) -> bool {
        if !self.anim_active() {
            return false;
        }
        let Some((lx, ly)) = self.view.drag_last else { return false };
        if !left_down {
            self.view.drag_last = None;
            return true;
        }
        if let Some(p) = self.spine.player.as_mut() {
            p.pan = (p.pan.0 + (x - lx) as f32, p.pan.1 + (y - ly) as f32);
            p.dirty = true;
            self.view.drag_last = Some((x, y));
            self.spine_render_with(false);
            return true;
        }
        self.view.pan = (self.view.pan.0 + (x - lx) as f32, self.view.pan.1 + (y - ly) as f32);
        self.view.drag_last = Some((x, y));
        invalidate(self.hwnd);
        true
    }

    pub fn spine_release(&mut self) -> bool {
        self.anim_active() && self.view.drag_last.take().is_some()
    }

    pub fn spine_reset_view(&mut self) -> bool {
        if !self.anim_active() {
            return false;
        }
        if let Some(p) = self.spine.player.as_mut() {
            p.zoom = 1.0;
            p.pan = (0.0, 0.0);
            self.spine_render_with(false);
            return true;
        }
        self.view = self.view.fresh();
        invalidate(self.hwnd);
        true
    }

    /// The zoom keys while a skeleton is in front: they change the player's own zoom (the skeleton is drawn again at that size).
    /// True when the key was taken.
    pub fn spine_zoom_action(&mut self, a: nv_core::keys::Action) -> bool {
        use nv_core::keys::Action;
        if !self.spine_active() {
            return false;
        }
        let win = self.renderer.size();
        let mid = (win.0 as f32 / 2.0, win.1 as f32 / 2.0);
        let Some(p) = self.spine.player.as_mut() else { return false };
        match a {
            Action::ZoomIn => p.zoom_about(win, mid, 1.15),
            Action::ZoomOut => p.zoom_about(win, mid, 1.0 / 1.15),
            Action::Zoom1 | Action::ZoomReset | Action::FitReset => {
                p.zoom = 1.0;
                p.pan = (0.0, 0.0);
            }
            Action::Zoom2 | Action::Zoom3 | Action::Zoom4 | Action::Zoom5 => {
                let n = match a {
                    Action::Zoom2 => 2.0,
                    Action::Zoom3 => 3.0,
                    Action::Zoom4 => 4.0,
                    _ => 5.0,
                };
                p.zoom = 1.0;
                p.pan = (0.0, 0.0);
                p.zoom_about(win, mid, n);
            }
            Action::ActualSize => {
                // One skeleton unit per screen pixel.
                let f = 1.0 / p.base_scale(win);
                p.zoom = 1.0;
                p.pan = (0.0, 0.0);
                p.zoom_about(win, mid, f);
            }
            _ => return false,
        }
        self.spine_render_with(false);
        true
    }

    // ---- saving ------------------------------------------------------------------------------------------------------------------------

    /// The pose on screen as a picture, saved with the video snapshot's settings (a format without transparency gets the canvas colour).
    pub fn spine_snapshot(&mut self) -> bool {
        let Some(p) = self.spine.player.as_ref() else { return false };
        let (w, h, view) = spinefmt::canvas_for(&p.rig, p.skin, p.anim, 1536);
        let prefs = self.settings.more.clone();
        let bg = if nv_core::frames::format_keeps_alpha(&prefs.snapshot_format) {
            None
        } else {
            let c = canvas_rgb();
            Some([(c >> 16 & 0xFF) as f32 / 255.0, (c >> 8 & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0])
        };
        let mut sim = p.sim.clone(); // (the springs as they are on screen)
        let rgba = render_pose(&p.rig, p.skin, p.anim, p.time, &p.slot_alpha, w, h, view, 2, bg, Some(&mut sim));
        let label = format!("{} {:.2}s", p.anim_name(), p.time);
        let shown = nv_core::snapshot::Shown { width: w as u32, height: h as u32, rgba };
        match nv_core::snapshot::take_shown_labeled(&p.path, &label, &prefs, shown) {
            Ok(s) => {
                let name = s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(if s.note.is_empty() { format!("Saved {name}") } else { format!("Saved {name} - {}", s.note) });
            }
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
        true
    }

    fn export_setup(&self) -> Option<(Arc<Rig>, usize, Option<usize>, Vec<f32>, usize, usize, nv_anim::View, f32, usize, PathBuf)> {
        let p = self.spine.player.as_ref()?;
        let (w, h, view) = spinefmt::canvas_for(&p.rig, p.skin, p.anim, 1024);
        let (w, h) = (w + w % 2, h + h % 2); // video wants even sizes
        let fps = if p.rig.fps >= 1.0 { p.rig.fps.min(60.0) } else { 30.0 };
        let frames = ((p.duration() * fps).round() as usize).max(1);
        Some((p.rig.clone(), p.skin, p.anim, p.slot_alpha.clone(), w, h, view, fps, frames, p.path.clone()))
    }

    /// Every frame of the animation as a numbered PNG with transparency, into a new folder next to the skeleton.
    pub fn spine_export_frames(&mut self) {
        let Some((rig, skin, anim, alpha, w, h, view, fps, frames, path)) = self.export_setup() else { return };
        let name = self.spine.player.as_ref().map(|p| p.anim_name()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "spine".into());
        let dir = nv_core::fileops::unique_path(path.parent().unwrap_or(Path::new(".")), std::ffi::OsStr::new(&format!("{stem}_{}_frames", name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_"))));
        let out_dir = dir.clone();
        let stem_job = stem.clone();
        let result = nv_win::progress::run("Spine: export frames", &format!("{stem} - {name}: {frames} frames, {w}x{h}"), move |cancel, report| -> Result<usize, String> {
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
            let mut sim = nv_anim::PhysicsSim::new();
            for i in 0..frames {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                let t = i as f32 / fps;
                if i > 0 {
                    sim.advance(1.0 / fps);
                }
                let rgba = render_pose(&rig, skin, anim, t, &alpha, w, h, view, 2, None, Some(&mut sim));
                let bytes = nv_editor::convert::encode(&nv_editor::raster::Pixels::new(w as u32, h as u32, rgba), nv_editor::convert::Out::Png, [0; 3], None)?;
                let file = out_dir.join(format!("{stem_job}_{:05}.png", i + 1));
                nv_editor::save::write_atomic(&file, &bytes).map_err(|e| format!("{}: {e}", file.display()))?;
                report.set(i + 1, frames);
            }
            Ok(frames)
        });
        match result {
            Some(Ok(n)) => {
                self.toast(format!("{n} frames with transparency saved in {}", dir.display()));
                nv_win::shell::reveal_in_explorer(&dir.join(format!("{stem}_00001.png")));
            }
            Some(Err(e)) if e == "cancelled" => self.toast("Export cancelled"),
            Some(Err(e)) => nv_win::shell::message(&format!("Could not export the frames: {e}")),
            None => {}
        }
    }

    /// The animation as a WebM that keeps the transparency (VP9 with alpha through ffmpeg).
    pub fn spine_export_webm(&mut self) {
        let Some((rig, skin, anim, alpha, w, h, view, fps, frames, path)) = self.export_setup() else { return };
        if nv_core::ffmpeg::find().is_none() || !nv_core::ffmpeg::has_encoder("libvpx-vp9") {
            nv_win::shell::message("WebM with transparency needs ffmpeg with the libvpx-vp9 encoder.\n\nSettings > Archives & tools shows where ffmpeg is looked for; a full build (winget install Gyan.FFmpeg) has it. The PNG frame export needs nothing extra.");
            return;
        }
        let name = self.spine.player.as_ref().map(|p| p.anim_name()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "spine".into());
        let file = nv_core::fileops::unique_path(path.parent().unwrap_or(Path::new(".")), std::ffi::OsStr::new(&format!("{stem}_{}.webm", name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_"))));
        let out = file.clone();
        let result = nv_win::progress::run("Spine: export WebM", &format!("{stem} - {name}: {frames} frames, {w}x{h}, {fps:.0} fps"), move |cancel, report| {
            let mut sim = nv_anim::PhysicsSim::new();
            nv_core::ffmpeg::encode_webm_alpha(&out, w, h, fps, frames, cancel, |i| {
                report.set(i + 1, frames);
                if i > 0 {
                    sim.advance(1.0 / fps);
                }
                render_pose(&rig, skin, anim, i as f32 / fps, &alpha, w, h, view, 2, None, Some(&mut sim))
            })
        });
        match result {
            Some(Ok(())) => {
                self.toast(format!("Saved {}", file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                nv_win::shell::reveal_in_explorer(&file);
            }
            Some(Err(e)) if e == "cancelled" => self.toast("Export cancelled"),
            Some(Err(e)) => nv_win::shell::message(&format!("Could not export the WebM: {e}")),
            None => {}
        }
    }
}
