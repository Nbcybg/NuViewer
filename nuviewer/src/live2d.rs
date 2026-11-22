//! The Live2D player: the same idea as `spine.rs`. The model's parameters are driven by a motion (and an expression), the Cubism Core
//! (a DLL the user downloads, see `docs/live2d.md`) computes the meshes, `nv-anim` draws them on the CPU and the result replaces the picture's
//! texture. Motions play where Spine's animations do (the panel's *Anim* buttons), expressions where Spine's skins do (*Skin*), and the
//! model's **parts** are the rows of the layer panel: their opacity is the part opacity of the model (0 to 1, not just off).

use super::*;
use crate::controls::fmt_speed;
use nv_anim::cubism::{Core, Model};
use nv_anim::live2d::{self, blend_expression, ExpBlend, Motion, Target};
use nv_codecs::live2dfmt::{self, Loaded};

pub const TIMER_L2D: u32 = 13;
const TICK_MS: u32 = 33;

pub struct L2dPlayer {
    pub id: u64,
    pub path: PathBuf,
    pub core: Arc<Core>,
    pub model: Model,
    pub loaded: Arc<Loaded>,
    /// (group, file, parsed when first needed)
    pub motions: Vec<(String, String, Option<Motion>)>,
    pub cur: Option<usize>,
    pub expr: Vec<(String, Vec<(String, f32, ExpBlend)>)>,
    pub expr_cur: Option<usize>,
    pub time: f32,
    pub playing: bool,
    pub canvas: (usize, usize),
    pub view: nv_anim::View,
    pub part_alpha: Vec<f32>,
    pub last: Instant,
    pub dirty: bool,
}

pub struct L2d {
    pub player: Option<L2dPlayer>,
    /// Why the model on screen has no player (no Core ...), shown once.
    pub failed_for: Option<PathBuf>,
}

impl L2d {
    pub fn new() -> L2d {
        L2d { player: None, failed_for: None }
    }
}

/// Parameters from a motion and an expression, then part opacities; the model is updated and its drawables become a draw list.
pub fn pose_model(model: &mut Model, motion: Option<&Motion>, expr: Option<&[(String, f32, ExpBlend)]>, eye: &[String], t: f32, part_alpha: &[f32]) -> Vec<nv_anim::DrawItem> {
    model.reset_to_defaults();
    let mut driven: Vec<String> = Vec::new();
    let mut part_motion: Vec<Option<f32>> = vec![None; model.parts.len()];
    if let Some(m) = motion {
        for (target, id, v) in m.sample(t) {
            match target {
                Target::Parameter => {
                    if let Some(i) = model.param_index(id) {
                        model.set_param(i, v);
                        driven.push(id.to_string());
                    }
                }
                Target::PartOpacity => {
                    if let Some(i) = model.part_index(id) {
                        part_motion[i] = Some(v);
                    }
                }
                Target::Model => {}
            }
        }
    }
    if let Some(e) = expr {
        for (id, value, blend) in e {
            if let Some(i) = model.param_index(id) {
                let cur = model.param_value(i);
                let def = model.params[i].default;
                model.set_param(i, blend_expression(cur, def, *value, *blend, 1.0));
            }
        }
    }
    // What a motion does not drive: the eyes blink, the breath goes.
    for id in eye {
        if !driven.contains(id) {
            if let Some(i) = model.param_index(id) {
                model.set_param(i, live2d::blink_at(t));
            }
        }
    }
    if !driven.iter().any(|d| d == "ParamBreath") {
        if let Some(i) = model.param_index("ParamBreath") {
            model.set_param(i, live2d::breath_at(t));
        }
    }
    for i in 0..model.parts.len() {
        model.set_part_opacity(i, part_motion[i].unwrap_or(1.0) * part_alpha.get(i).copied().unwrap_or(1.0));
    }
    model.update();
    live2d::items_from_drawables(&model.drawables())
}

impl App {
    pub fn l2d_active(&self) -> bool {
        self.mode == Mode::Viewer && self.l2d.player.is_some() && !self.editor.open
    }

    /// Follow the picture in front: a `.model3.json` / `.moc3` gets a player when the Core is there. Called from `poll_background`.
    pub fn l2d_sync(&mut self) {
        let cur = if self.folder.files.is_empty() { None } else { Some(self.id_at(self.folder.index)) };
        if let Some(p) = &self.l2d.player {
            if Some(p.id) != cur || self.mode != Mode::Viewer {
                self.l2d.player = None;
                kill_timer(self.hwnd, TIMER_L2D);
            }
        }
        if self.l2d.player.is_some() || self.mode != Mode::Viewer {
            return;
        }
        let (Some(id), Some(path)) = (cur, self.folder.files.get(self.folder.index).cloned()) else { return };
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !(live2dfmt::is_model3(&path) || ext == "moc3") || self.l2d.failed_for.as_ref() == Some(&path) {
            return;
        }
        if !(self.renderer.has(id) && self.renderer.current() == Some(id)) {
            return;
        }
        match self.l2d_make(id, &path) {
            Ok(p) => {
                self.l2d.player = Some(p);
                set_timer(self.hwnd, TIMER_L2D, TICK_MS);
                self.toast("Live2D model: P plays / pauses, wheel zooms, drag pans, Ctrl+L = motions, expressions and parts");
            }
            Err(e) => {
                self.l2d.failed_for = Some(path);
                self.toast(format!("Live2D: {e}"));
            }
        }
    }

    fn l2d_make(&mut self, id: u64, path: &Path) -> Result<L2dPlayer, String> {
        let loaded = Arc::new(live2dfmt::load(path)?);
        let core = live2dfmt::core()?;
        let mut model = Model::new(core.clone(), &loaded.moc)?;
        let (w, h, view) = live2dfmt::canvas_for_model(&mut model, 1024);
        let motions: Vec<(String, String, Option<Motion>)> = loaded.info.motions.iter().map(|(g, f)| (g.clone(), f.clone(), None)).collect();
        let expr: Vec<(String, Vec<(String, f32, ExpBlend)>)> = loaded.info.expressions.iter().filter_map(|(n, f)| Some((n.clone(), live2dfmt::load_expression(&loaded.dir, f).ok()?))).collect();
        let parts = model.parts.len();
        let mut p = L2dPlayer { id, path: path.to_path_buf(), core, model, loaded, motions, cur: None, expr, expr_cur: None, time: 0.0, playing: true, canvas: (w, h), view, part_alpha: vec![1.0; parts], last: Instant::now(), dirty: true };
        // Start on the first "Idle" motion if there is one.
        let start = p.motions.iter().position(|m| m.0.eq_ignore_ascii_case("idle")).or(if p.motions.is_empty() { None } else { Some(0) });
        l2d_set_motion(&mut p, start);
        Ok(p)
    }

    pub fn l2d_tick(&mut self) {
        let speed = self.anim_speed.max(0.05);
        let loop_on = self.settings.more.play_loop;
        let Some(p) = self.l2d.player.as_mut() else { return };
        let now = Instant::now();
        let dt = now.duration_since(p.last).as_secs_f32().min(0.25);
        p.last = now;
        let (dur, looped) = p.cur.and_then(|c| p.motions[c].2.as_ref()).map(|m| (m.duration, m.looped)).unwrap_or((0.0, false));
        if p.playing {
            p.time += dt * speed;
            if dur > 0.0 && p.time >= dur {
                if looped || loop_on {
                    p.time = p.time.rem_euclid(dur);
                } else {
                    p.time = dur;
                    p.playing = false;
                }
            }
            p.dirty = true;
        }
        if p.dirty {
            self.l2d_render();
        }
    }

    pub fn l2d_render(&mut self) {
        let Some(p) = self.l2d.player.as_mut() else { return };
        p.dirty = false;
        let motion = p.cur.and_then(|c| p.motions[c].2.as_ref());
        let expr = p.expr_cur.map(|e| p.expr[e].1.as_slice());
        let items = pose_model(&mut p.model, motion, expr, &p.loaded.info.eye_blink, p.time, &p.part_alpha);
        let (w, h) = p.canvas;
        let rgba = nv_anim::render_pages(&p.loaded.textures, &items, w, h, p.view, 1, None);
        let id = p.id;
        let img = nv_codecs::Image::new(w as u32, h as u32, rgba);
        if self.renderer.upload(id, &img).is_ok() {
            self.renderer.show(id);
            invalidate(self.hwnd);
        }
    }

    pub fn l2d_toggle_play(&mut self) {
        if let Some(p) = self.l2d.player.as_mut() {
            p.playing = !p.playing;
            p.last = Instant::now();
            let msg = if p.playing { "Play" } else { "Pause" };
            self.toast(msg);
        }
    }

    pub fn l2d_step_motion(&mut self, d: i32) {
        let Some(p) = self.l2d.player.as_mut() else { return };
        let n = p.motions.len() as i32 + 1; // the default pose is one more stop
        let cur = p.cur.map(|c| c as i32).unwrap_or(n - 1);
        let next = (cur + d).rem_euclid(n);
        l2d_set_motion(p, if next == n - 1 { None } else { Some(next as usize) });
        self.l2d_render();
    }

    pub fn l2d_next_expression(&mut self) {
        let Some(p) = self.l2d.player.as_mut() else { return };
        let n = p.expr.len() as i32 + 1;
        let cur = p.expr_cur.map(|c| c as i32).unwrap_or(n - 1);
        let next = (cur + 1).rem_euclid(n);
        p.expr_cur = if next == n - 1 { None } else { Some(next as usize) };
        p.dirty = true;
        self.l2d_render();
    }

    pub fn l2d_info(&self) -> String {
        let Some(p) = &self.l2d.player else { return String::new() };
        let motion = match p.cur {
            Some(c) => format!("{} / {} ({}/{})", p.motions[c].0, p.motions[c].1.trim_end_matches(".motion3.json"), c + 1, p.motions.len()),
            None => "(default pose)".to_string(),
        };
        let dur = p.cur.and_then(|c| p.motions[c].2.as_ref()).map(|m| m.duration).unwrap_or(0.0);
        let ex = p.expr_cur.map(|e| p.expr[e].0.clone()).unwrap_or_else(|| "none".into());
        format!("{motion}   expression {ex}   {:.2}/{:.2} s   {}\u{00D7}", p.time, dur, fmt_speed(self.anim_speed))
    }

    /// (name, blend note) of every part, in the order the panel shows them (the list is shown back to front).
    pub fn l2d_part_nodes(&self) -> Vec<(String, String)> {
        let Some(p) = &self.l2d.player else { return Vec::new() };
        p.model.parts.iter().rev().map(|id| (p.loaded.display.parts.get(id).cloned().unwrap_or_else(|| id.clone()), String::new())).collect()
    }

    /// The panel's rows (already reversed back to model order by the caller) changed.
    pub fn l2d_apply_parts(&mut self, eff: &[nv_codecs::layerdoc::Eff]) {
        if let Some(p) = self.l2d.player.as_mut() {
            for (i, a) in p.part_alpha.iter_mut().enumerate() {
                *a = eff.get(i).map(|e| if e.on { e.opacity } else { 0.0 }).unwrap_or(1.0);
            }
            p.dirty = true;
        }
        self.l2d_render();
    }

    // ---- saving ---------------------------------------------------------------------------------------------------------------------

    pub fn l2d_snapshot(&mut self) -> bool {
        let Some(p) = self.l2d.player.as_mut() else { return false };
        let motion = p.cur.and_then(|c| p.motions[c].2.as_ref());
        let expr = p.expr_cur.map(|e| p.expr[e].1.as_slice());
        let items = pose_model(&mut p.model, motion, expr, &p.loaded.info.eye_blink, p.time, &p.part_alpha);
        let (w, h) = (p.canvas.0 * 3 / 2, p.canvas.1 * 3 / 2);
        let prefs = self.settings.more.clone();
        let bg = if nv_core::frames::format_keeps_alpha(&prefs.snapshot_format) {
            None
        } else {
            let c = canvas_rgb();
            Some([(c >> 16 & 0xFF) as f32 / 255.0, (c >> 8 & 0xFF) as f32 / 255.0, (c & 0xFF) as f32 / 255.0])
        };
        let view = nv_anim::View { scale: p.view.scale * 1.5, ..p.view };
        let rgba = nv_anim::render_pages(&p.loaded.textures, &items, w, h, view, 2, bg);
        let label = format!("{:.2}s", p.time);
        let shown = nv_core::snapshot::Shown { width: w as u32, height: h as u32, rgba };
        match nv_core::snapshot::take_shown_labeled(&p.path, &label, &prefs, shown) {
            Ok(s) => {
                let name = s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(format!("Saved {name}"));
            }
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
        true
    }

    /// What an export needs, all owned so it can run on a worker thread: a model of its own is made there.
    #[allow(clippy::type_complexity)]
    fn l2d_export_setup(&self) -> Option<(Arc<Core>, Arc<Loaded>, Option<Motion>, Option<Vec<(String, f32, ExpBlend)>>, Vec<f32>, usize, usize, nv_anim::View, f32, usize, PathBuf, String)> {
        let p = self.l2d.player.as_ref()?;
        let motion = p.cur.and_then(|c| p.motions[c].2.clone());
        let expr = p.expr_cur.map(|e| p.expr[e].1.clone());
        let (w, h) = (p.canvas.0 + p.canvas.0 % 2, p.canvas.1 + p.canvas.1 % 2);
        let fps = motion.as_ref().map(|m| m.fps).filter(|f| *f >= 1.0).unwrap_or(30.0).min(60.0);
        let frames = motion.as_ref().map(|m| ((m.duration * fps).round() as usize).max(1)).unwrap_or(1);
        let name = p.cur.map(|c| p.motions[c].1.trim_end_matches(".motion3.json").to_string()).unwrap_or_else(|| "pose".into());
        Some((p.core.clone(), p.loaded.clone(), motion, expr, p.part_alpha.clone(), w, h, p.view, fps, frames, p.path.clone(), name))
    }

    pub fn l2d_export_frames(&mut self) {
        let Some((core, loaded, motion, expr, alpha, w, h, view, fps, frames, path, name)) = self.l2d_export_setup() else { return };
        let stem = loaded.name.clone();
        let safe = name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_");
        let dir = nv_core::fileops::unique_path(path.parent().unwrap_or(Path::new(".")), std::ffi::OsStr::new(&format!("{stem}_{safe}_frames")));
        let out_dir = dir.clone();
        let stem_job = stem.clone();
        let result = nv_win::progress::run("Live2D: export frames", &format!("{stem} - {name}: {frames} frames, {w}x{h}"), move |cancel, report| -> Result<usize, String> {
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
            let mut model = Model::new(core, &loaded.moc)?;
            for i in 0..frames {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                let items = pose_model(&mut model, motion.as_ref(), expr.as_deref(), &loaded.info.eye_blink, i as f32 / fps, &alpha);
                let rgba = nv_anim::render_pages(&loaded.textures, &items, w, h, view, 2, None);
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

    pub fn l2d_export_webm(&mut self) {
        let Some((core, loaded, motion, expr, alpha, w, h, view, fps, frames, path, name)) = self.l2d_export_setup() else { return };
        if nv_core::ffmpeg::find().is_none() || !nv_core::ffmpeg::has_encoder("libvpx-vp9") {
            nv_win::shell::message("WebM with transparency needs ffmpeg with the libvpx-vp9 encoder.\n\nSettings > Archives & tools shows where ffmpeg is looked for; a full build (winget install Gyan.FFmpeg) has it. The PNG frame export needs nothing extra.");
            return;
        }
        let safe = name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_");
        let file = nv_core::fileops::unique_path(path.parent().unwrap_or(Path::new(".")), std::ffi::OsStr::new(&format!("{}_{safe}.webm", loaded.name)));
        let out = file.clone();
        let result = nv_win::progress::run("Live2D: export WebM", &format!("{} - {name}: {frames} frames, {w}x{h}", loaded.name), move |cancel, report| -> Result<(), String> {
            let mut model = Model::new(core, &loaded.moc)?;
            nv_core::ffmpeg::encode_webm_alpha(&out, w, h, fps, frames, cancel, |i| {
                report.set(i + 1, frames);
                let items = pose_model(&mut model, motion.as_ref(), expr.as_deref(), &loaded.info.eye_blink, i as f32 / fps, &alpha);
                nv_anim::render_pages(&loaded.textures, &items, w, h, view, 2, None)
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

/// Choose a motion (None = the default pose): parse it if needed, restart, and set the canvas.
fn l2d_set_motion(p: &mut L2dPlayer, cur: Option<usize>) {
    if let Some(c) = cur {
        if p.motions[c].2.is_none() {
            p.motions[c].2 = live2dfmt::load_motion(&p.loaded.dir, &p.motions[c].1).ok();
        }
    }
    p.cur = cur;
    p.time = 0.0;
    p.dirty = true;
}
