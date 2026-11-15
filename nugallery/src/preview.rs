//! The preview in the gallery's right panel, with the controls the viewer has: a still picture zooms (wheel, around the
//! pointer) and pans (drag; double click = fit ⇄ zoomed) and gets a sharper decode once you zoom in; an animated GIF / WebP /
//! APNG and a folded image sequence play, with a bar to pause, step and scrub; a 3D model turns when you drag and zooms with the
//! wheel. Everything is decoded off the UI thread and only for the item the pointer has rested on.

use crate::app::{App, Msg};
use crate::gfx::{contains, line_h, outline, pal, rgb, Text};
use crate::info;
use crate::model::Kind;
use nv_codecs::model::{Mesh, Orbit};
use nv_codecs::Image;
use nv_render::Renderer;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TIMER_PREVIEW: u32 = 6;
/// Most frames kept for a preview animation or sequence (longer runs show every n-th frame).
const MAX_FRAMES: usize = 120;
/// Longest edge of a preview frame.
const FRAME_EDGE: u32 = 480;
/// Longest edge of the sharper picture decoded when you zoom in.
const BIG_EDGE: u32 = 3072;
pub const ZOOM_MAX: f32 = 8.0;
const HEAD: f32 = 36.0;

fn frame_key(id: u64, k: usize) -> u64 {
    info::preview_key(id) ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn mesh_key(id: u64) -> u64 {
    info::preview_key(id) ^ 0x4D45_5348_4D45_5348
}

fn big_key(id: u64) -> u64 {
    info::preview_key(id) ^ 0x4249_4721_4249_4721
}

/// The frames of an animation or sequence that are on the GPU, and where playback is.
pub struct Anim {
    pub keys: Vec<u64>,
    pub delays: Vec<u32>,
    pub total: usize,
    pub index: usize,
    pub playing: bool,
    pub label: String,
}

pub struct Preview {
    /// The item this state belongs to.
    pub id: Option<u64>,
    pub zoom: f32,
    /// Offset of the picture's centre from the middle of the preview box, in pixels.
    pub pan: (f32, f32),
    /// A drag in progress: (where it started, the pan or the orbit at that moment).
    pub drag: Option<((f32, f32), (f32, f32))>,
    pub anim: Option<Anim>,
    pub scrubbing: bool,
    pub mesh: Option<Arc<Mesh>>,
    pub orbit: Orbit,
    /// A sharper decode was asked for / has arrived.
    pub big_asked: bool,
    pub big_ready: bool,
}

impl Preview {
    pub fn new(id: Option<u64>) -> Preview {
        Preview { id, zoom: 1.0, pan: (0.0, 0.0), drag: None, anim: None, scrubbing: false, mesh: None, orbit: Orbit::default(), big_asked: false, big_ready: false }
    }

    /// The texture keys this state put on the GPU (so they can be dropped when the item changes).
    fn keys(&self) -> Vec<u64> {
        let Some(id) = self.id else { return Vec::new() };
        let mut v = vec![mesh_key(id), big_key(id)];
        if let Some(a) = &self.anim {
            v.extend(a.keys.iter().copied());
        }
        v
    }
}

/// Where the controls of an animation sit in the preview box `r`: a strip along its bottom.
pub struct AnimBar {
    pub strip: [f32; 4],
    pub prev: [f32; 4],
    pub play: [f32; 4],
    pub next: [f32; 4],
    pub track: [f32; 4],
    pub label_right: f32,
}

pub fn anim_bar(r: [f32; 4], d: f32) -> AnimBar {
    let h = 30.0 * d;
    let strip = [r[0], r[3] - h, r[2], r[3]];
    let b = 24.0 * d;
    let y0 = strip[1] + (h - b) / 2.0;
    let mut x = strip[0] + 6.0 * d;
    let prev = [x, y0, x + b, y0 + b];
    x += b + 2.0 * d;
    let play = [x, y0, x + b, y0 + b];
    x += b + 2.0 * d;
    let next = [x, y0, x + b, y0 + b];
    x += b + 8.0 * d;
    let label_w = 62.0 * d;
    let track = [x, strip[1] + h / 2.0 - 2.0 * d, strip[2] - label_w - 6.0 * d, strip[1] + h / 2.0 + 2.0 * d];
    AnimBar { strip, prev, play, next, track, label_right: strip[2] - 6.0 * d }
}

/// How far the picture may be moved so it still covers the box (a picture that is not bigger than the box stays centred).
fn pan_limits(box_w: f32, box_h: f32, w: f32, h: f32) -> (f32, f32) {
    (((w - box_w) / 2.0).max(0.0), ((h - box_h) / 2.0).max(0.0))
}

/// The rectangle the picture of size (`tw`, `th`) occupies in box `r` at `zoom` with `pan`.
pub fn picture_rect(r: [f32; 4], tw: u32, th: u32, zoom: f32, pan: (f32, f32)) -> [f32; 4] {
    let (cw, ch) = (r[2] - r[0], r[3] - r[1]);
    let s = (cw / tw as f32).min(ch / th as f32) * zoom;
    let (w, h) = (tw as f32 * s, th as f32 * s);
    let (lx, ly) = pan_limits(cw, ch, w, h);
    let (px, py) = (pan.0.clamp(-lx, lx), pan.1.clamp(-ly, ly));
    let (cx, cy) = (r[0] + cw / 2.0 + px, r[1] + ch / 2.0 + py);
    [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]
}

/// Draw the preview into box `rect`. `clip` is the part of the panel that is visible (the box may be scrolled partly away).
/// `key` is the texture of the still picture (the small preview, or its thumbnail while that loads).
#[allow(clippy::too_many_arguments)]
pub fn draw(r: &mut Renderer, text: &mut Text, pv: &Preview, rect: [f32; 4], clip: [f32; 4], id: u64, key: u64, ext: &str, d: f32) {
    let vis = [rect[0].max(clip[0]), rect[1].max(clip[1]), rect[2].min(clip[2]), rect[3].min(clip[3])];
    if vis[2] <= vis[0] || vis[3] <= vis[1] {
        return;
    }
    r.set_clip(Some(vis));
    let mine = pv.id == Some(id);
    // What to show: the current frame of an animation, the turned model, the sharp decode, or the small preview.
    let shown = if let (true, Some(a)) = (mine, pv.anim.as_ref().filter(|a| !a.keys.is_empty())) {
        Some(a.keys[a.index.min(a.keys.len() - 1)])
    } else if mine && pv.mesh.is_some() && r.has_thumb(mesh_key(id)) {
        Some(mesh_key(id))
    } else if mine && pv.big_ready && r.has_thumb(big_key(id)) {
        Some(big_key(id))
    } else if r.thumb_size(key).is_some() {
        Some(key)
    } else {
        None
    };
    match shown.and_then(|k| r.thumb_size(k).map(|s| (k, s))) {
        Some((k, (tw, th))) => {
            let is_mesh = Some(k) == pv.id.map(mesh_key);
            let dst = if is_mesh { picture_rect(rect, tw, th, 1.0, (0.0, 0.0)) } else { picture_rect(rect, tw, th, if mine { pv.zoom } else { 1.0 }, if mine { pv.pan } else { (0.0, 0.0) }) };
            let _ = r.draw_thumb(k, [dst[0].round(), dst[1].round(), dst[2].round(), dst[3].round()]);
        }
        None => crate::draw::draw_card(r, text, rect, ext, false, d),
    }
    let small = (11.0 * d).round() as i32;
    // Animation controls.
    if let (true, Some(a)) = (mine, pv.anim.as_ref()) {
        let bar = anim_bar(rect, d);
        let _ = r.fill_rect(bar.strip, rgb(0x000000, 0.66));
        for (rc, glyph) in [(bar.prev, "◀"), (bar.play, if a.playing { "❚❚" } else { "▶" }), (bar.next, "▶")] {
            let _ = r.fill_rect(rc, rgb(0x2B2E33, 0.95));
            text.draw_centered(r, glyph, (11.0 * d).round() as i32, (rc[0] + rc[2]) / 2.0, rc[1] + (rc[3] - rc[1] - line_h(small)) / 2.0, rgb(pal::TEXT, 1.0));
        }
        let _ = r.fill_rect(bar.track, rgb(0xFFFFFF, 0.2));
        let frac = if a.total > 1 { a.index as f32 / (a.total - 1) as f32 } else { 0.0 };
        let knob = bar.track[0] + (bar.track[2] - bar.track[0]) * frac;
        let _ = r.fill_rect([bar.track[0], bar.track[1], knob, bar.track[3]], rgb(pal::ACCENT, 1.0));
        let _ = r.fill_rect([knob - 4.0 * d, bar.track[1] - 5.0 * d, knob + 4.0 * d, bar.track[3] + 5.0 * d], rgb(0xE8E8E8, 1.0));
        text.draw_right(r, &format!("{}/{}", a.index + 1, a.total), small, bar.label_right, bar.strip[1] + (bar.strip[3] - bar.strip[1] - line_h(small)) / 2.0, rgb(pal::DIM, 1.0));
        let tag = text.width(&a.label, small) as f32 + 12.0 * d;
        let _ = r.fill_rect([rect[0] + 6.0 * d, rect[1] + 6.0 * d, rect[0] + 6.0 * d + tag, rect[1] + 6.0 * d + line_h(small) + 4.0 * d], rgb(0x000000, 0.7));
        text.draw(r, &a.label, small, rect[0] + 12.0 * d, rect[1] + 8.0 * d, rgb(pal::TEXT, 1.0));
    } else if mine && pv.mesh.is_some() {
        let hint = "drag: turn · wheel: zoom · double-click: reset";
        let w = text.width(hint, small) as f32 + 12.0 * d;
        let _ = r.fill_rect([rect[0] + 6.0 * d, rect[3] - line_h(small) - 10.0 * d, rect[0] + 6.0 * d + w, rect[3] - 6.0 * d], rgb(0x000000, 0.6));
        text.draw(r, hint, small, rect[0] + 12.0 * d, rect[3] - line_h(small) - 8.0 * d, rgb(pal::DIM, 1.0));
    } else if mine && (pv.zoom - 1.0).abs() > 0.01 {
        let label = format!("×{:.1}", pv.zoom);
        let w = text.width(&label, small) as f32 + 12.0 * d;
        let _ = r.fill_rect([rect[2] - w - 6.0 * d, rect[1] + 6.0 * d, rect[2] - 6.0 * d, rect[1] + 6.0 * d + line_h(small) + 4.0 * d], rgb(0x000000, 0.7));
        text.draw(r, &label, small, rect[2] - w, rect[1] + 8.0 * d, rgb(pal::TEXT, 1.0));
    }
    outline(r, rect, 1.0, rgb(pal::PANEL_EDGE, 1.0));
    r.set_clip(Some(clip));
}

impl App {
    /// The part of the preview box that can be seen (the panel scrolls it under the header).
    fn preview_visible(&self) -> Option<([f32; 4], [f32; 4])> {
        let geo = self.right_geo()?;
        let d = self.dpi;
        let p = geo.panel;
        let clip = [p[0], p[1] + HEAD * d, p[2], p[3]];
        let r = geo.preview;
        let vis = [r[0].max(clip[0]), r[1].max(clip[1]), r[2].min(clip[2]), r[3].min(clip[3])];
        (vis[2] > vis[0] && vis[3] > vis[1]).then_some((r, vis))
    }

    // ---- what the focused item needs ------------------------------------------------------------------------

    /// The focus moved to a new item: forget the old preview state and start what this item needs (frames, a model).
    pub fn pv_begin(&mut self, id: u64, path: PathBuf, kind: Kind, ext: &str) {
        self.pv_drop_textures();
        self.pv = Preview::new(Some(id));
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let fps = self.settings.fps.clamp(1, 60);
        if let Some(files) = self.listing.sequence_files(id) {
            std::thread::spawn(move || {
                let n = files.len();
                let stride = n.div_ceil(MAX_FRAMES).max(1);
                let picked: Vec<&PathBuf> = files.iter().step_by(stride).collect();
                let total = picked.len();
                let label = if stride > 1 { format!("Sequence · {n} frames (every {stride}th shown)") } else { format!("Sequence · {n} frames") };
                for (i, p) in picked.into_iter().enumerate() {
                    if let Ok(img) = info::frame_decode(p, FRAME_EDGE) {
                        let _ = tx.send(Msg::AnimFrame { id, img, delay: (1000 / fps).max(10), total, label: label.clone() });
                        wake();
                    }
                    let _ = i;
                }
            });
        } else if kind == Kind::Picture && matches!(ext, "gif" | "webp" | "apng") {
            std::thread::spawn(move || {
                let Ok(img) = nv_codecs::decode_file(&path) else { return };
                if img.extra_frames.is_empty() {
                    return;
                }
                let (w, h, mut delay) = (img.width, img.height, img.delay_ms);
                let total = 1 + img.extra_frames.len();
                let label = format!("Animation · {total} frames");
                let mut frames = vec![(img.rgba, delay)];
                frames.extend(img.extra_frames.into_iter().map(|f| (f.rgba, f.delay_ms)));
                let stride = total.div_ceil(MAX_FRAMES).max(1);
                let kept = frames.len().div_ceil(stride);
                for (rgba, d) in frames.into_iter().step_by(stride) {
                    delay = d.max(20) * stride as u32;
                    let small = info::finish_frame(Image::new(w, h, rgba), FRAME_EDGE);
                    let _ = tx.send(Msg::AnimFrame { id, img: small, delay, total: kept, label: label.clone() });
                    wake();
                }
            });
        } else if kind == Kind::Model {
            std::thread::spawn(move || {
                if let Ok(img) = nv_codecs::decode_file(&path) {
                    if let Some(mesh) = img.mesh {
                        let _ = tx.send(Msg::Mesh { id, mesh });
                        wake();
                    }
                }
            });
        }
    }

    /// Drop the GPU textures of the preview state that is being replaced.
    pub fn pv_drop_textures(&mut self) {
        let keys = self.pv.keys();
        if keys.is_empty() {
            return;
        }
        if let Some(r) = self.renderer.as_mut() {
            r.retain_thumbs(|k| !keys.contains(&k));
        }
        nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
    }

    /// One frame of an animation or sequence arrived.
    pub fn pv_frame(&mut self, id: u64, img: Image, delay: u32, total: usize, label: String) {
        if self.pv.id != Some(id) {
            return; // the focus has moved on
        }
        let Some(r) = self.renderer.as_mut() else { return };
        let a = self.pv.anim.get_or_insert_with(|| Anim { keys: Vec::new(), delays: Vec::new(), total, index: 0, playing: true, label });
        let k = frame_key(id, a.keys.len());
        if r.upload_thumb(k, &img).is_err() {
            return;
        }
        a.keys.push(k);
        a.delays.push(delay);
        a.total = total;
        let first = a.keys.len() == 1;
        if first {
            nv_win::set_timer(self.hwnd, TIMER_PREVIEW, delay.max(20));
        }
        nv_win::invalidate(self.hwnd);
    }

    /// A model's geometry arrived: draw it.
    pub fn pv_mesh(&mut self, id: u64, mesh: Arc<Mesh>) {
        if self.pv.id != Some(id) {
            return;
        }
        self.pv.mesh = Some(mesh);
        self.pv_render_mesh();
    }

    /// The sharper picture (decoded after the user zoomed in) arrived.
    pub fn pv_big(&mut self, id: u64, img: Image) {
        if self.pv.id != Some(id) {
            return;
        }
        if let Some(r) = self.renderer.as_mut() {
            if r.upload_thumb(big_key(id), &img).is_ok() {
                self.pv.big_ready = true;
                nv_win::invalidate(self.hwnd);
            }
        }
    }

    /// Render the model at the box's size from where it is looked at now.
    fn pv_render_mesh(&mut self) {
        let (Some(mesh), Some(id), Some((rect, _))) = (self.pv.mesh.clone(), self.pv.id, self.preview_visible()) else { return };
        let (w, h) = (((rect[2] - rect[0]) as u32).clamp(64, 640), ((rect[3] - rect[1]) as u32).clamp(64, 640));
        let mut img = Image::new(w, h, nv_codecs::model::render_preview(&mesh, &self.pv.orbit, w, h));
        img.srgb_to_output();
        if let Some(r) = self.renderer.as_mut() {
            let _ = r.upload_thumb(mesh_key(id), &img);
        }
        nv_win::invalidate(self.hwnd);
    }

    /// Ask for a sharper decode of the focused picture (once, the first time it is zoomed in).
    fn pv_ask_big(&mut self) {
        let (Some(id), false) = (self.pv.id, self.pv.big_asked) else { return };
        let Some(e) = self.listing.iter().find(|e| e.id == id) else { return };
        if !e.has_thumb() || e.kind == Kind::Model {
            return;
        }
        self.pv.big_asked = true;
        let (path, tx, wake) = (e.path.clone(), self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            if let Ok(img) = info::frame_decode(&path, BIG_EDGE) {
                let _ = tx.send(Msg::BigPreview { id, img });
                wake();
            }
        });
    }

    /// The animation timer: next frame.
    pub fn pv_tick(&mut self) {
        let Some(a) = self.pv.anim.as_mut() else {
            nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
            return;
        };
        if !a.playing || a.keys.is_empty() {
            nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
            return;
        }
        a.index = (a.index + 1) % a.keys.len();
        let wait = a.delays.get(a.index).copied().unwrap_or(40).max(20);
        nv_win::set_timer(self.hwnd, TIMER_PREVIEW, wait);
        nv_win::invalidate(self.hwnd);
    }

    // ---- the mouse ---------------------------------------------------------------------------------------------

    /// Wheel over the preview: zoom a picture around the pointer, zoom a model. True if used.
    pub fn preview_wheel(&mut self, delta: i32, x: i32, y: i32) -> bool {
        let Some((rect, vis)) = self.preview_visible() else { return false };
        let (fx, fy) = (x as f32, y as f32);
        if !contains(vis, fx, fy) || self.pv.id.is_none() || self.pv.id != self.info_for {
            return false;
        }
        let notches = delta as f32 / 120.0;
        if self.pv.mesh.is_some() {
            self.pv.orbit.zoom_by(notches);
            self.pv_render_mesh();
            return true;
        }
        if self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| e.is_dir()).unwrap_or(true) {
            return false;
        }
        // Zoom about the pointer: the point of the picture under it stays under it.
        let old = self.pv.zoom;
        let new = (old * 1.25f32.powf(notches)).clamp(1.0, ZOOM_MAX);
        if (new - old).abs() < 1e-4 {
            return true;
        }
        let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
        let (ox, oy) = (fx - centre.0 - self.pv.pan.0, fy - centre.1 - self.pv.pan.1);
        let k = new / old;
        self.pv.pan = (self.pv.pan.0 - ox * (k - 1.0), self.pv.pan.1 - oy * (k - 1.0));
        self.pv.zoom = new;
        if new <= 1.0 + 1e-4 {
            self.pv.pan = (0.0, 0.0);
        }
        if new > 1.3 {
            self.pv_ask_big();
        }
        nv_win::invalidate(self.hwnd);
        true
    }

    /// A left press in the right panel's preview box. True if it belonged to the preview.
    pub fn preview_press(&mut self, x: f32, y: f32, double: bool) -> bool {
        let Some((rect, vis)) = self.preview_visible() else { return false };
        if !contains(vis, x, y) || self.pv.id.is_none() || self.pv.id != self.info_for {
            return false;
        }
        let d = self.dpi;
        // The animation bar.
        if let Some(a) = self.pv.anim.as_mut() {
            let bar = anim_bar(rect, d);
            if contains(bar.strip, x, y) {
                if contains(bar.play, x, y) {
                    a.playing = !a.playing;
                    if a.playing {
                        nv_win::set_timer(self.hwnd, TIMER_PREVIEW, a.delays.get(a.index).copied().unwrap_or(40).max(20));
                    } else {
                        nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
                    }
                } else if contains(bar.prev, x, y) || contains(bar.next, x, y) {
                    a.playing = false;
                    nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
                    let n = a.keys.len().max(1);
                    a.index = if contains(bar.prev, x, y) { (a.index + n - 1) % n } else { (a.index + 1) % n };
                } else if contains([bar.track[0] - 4.0 * d, bar.strip[1], bar.track[2] + 4.0 * d, bar.strip[3]], x, y) {
                    a.playing = false;
                    nv_win::kill_timer(self.hwnd, TIMER_PREVIEW);
                    self.pv.scrubbing = true;
                    self.pv_scrub(x, rect);
                }
                nv_win::invalidate(self.hwnd);
                return true;
            }
        }
        if self.pv.mesh.is_some() {
            if double {
                self.pv.orbit = Orbit::default();
                self.pv_render_mesh();
            } else {
                self.pv.drag = Some(((x, y), (self.pv.orbit.yaw, self.pv.orbit.pitch)));
            }
            return true;
        }
        if self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| e.is_dir()).unwrap_or(true) {
            return true;
        }
        if double {
            // Fit <-> 3x about the click.
            if self.pv.zoom > 1.01 {
                self.pv.zoom = 1.0;
                self.pv.pan = (0.0, 0.0);
            } else {
                let centre = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
                self.pv.zoom = 3.0;
                self.pv.pan = (-(x - centre.0) * 2.0, -(y - centre.1) * 2.0);
                self.pv_ask_big();
            }
            nv_win::invalidate(self.hwnd);
            return true;
        }
        self.pv.drag = Some(((x, y), self.pv.pan));
        true
    }

    fn pv_scrub(&mut self, x: f32, rect: [f32; 4]) {
        let bar = anim_bar(rect, self.dpi);
        if let Some(a) = self.pv.anim.as_mut() {
            let t = ((x - bar.track[0]) / (bar.track[2] - bar.track[0]).max(1.0)).clamp(0.0, 1.0);
            a.index = (t * (a.keys.len().saturating_sub(1)) as f32).round() as usize;
        }
    }

    /// The pointer moved with the left button down: pan, turn the model, or scrub.
    pub fn preview_drag(&mut self, x: i32, y: i32) -> bool {
        let Some((rect, _)) = self.preview_visible() else { return false };
        let (fx, fy) = (x as f32, y as f32);
        if self.pv.scrubbing {
            self.pv_scrub(fx, rect);
            nv_win::invalidate(self.hwnd);
            return true;
        }
        let Some(((sx, sy), (a, b))) = self.pv.drag else { return false };
        if self.pv.mesh.is_some() {
            self.pv.orbit.yaw = a - (fx - sx) * 0.012;
            self.pv.orbit.pitch = (b + (fy - sy) * 0.012).clamp(-1.52, 1.52);
            self.pv_render_mesh();
        } else {
            self.pv.pan = (a + (fx - sx), b + (fy - sy));
            nv_win::invalidate(self.hwnd);
        }
        true
    }

    pub fn preview_release(&mut self) {
        self.pv.drag = None;
        self.pv.scrubbing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_fits_the_box_and_zooms_about_its_centre() {
        let r = [0.0, 0.0, 200.0, 100.0];
        let fit = picture_rect(r, 400, 400, 1.0, (0.0, 0.0));
        assert_eq!(fit, [50.0, 0.0, 150.0, 100.0], "a square picture in a wide box: height-bound, centred");
        let z = picture_rect(r, 400, 400, 2.0, (0.0, 0.0));
        assert_eq!((z[2] - z[0], z[3] - z[1]), (200.0, 200.0));
    }

    #[test]
    fn panning_stops_at_the_picture_edges() {
        let r = [0.0, 0.0, 200.0, 100.0];
        // At fit the picture is narrower than the box: it can't be moved sideways at all.
        let fit = picture_rect(r, 400, 400, 1.0, (500.0, 0.0));
        assert_eq!(fit[0], 50.0);
        // Zoomed to 3x it is 300 wide in a 200 box: it can move 50 either way.
        let z = picture_rect(r, 400, 400, 3.0, (500.0, 0.0));
        assert_eq!((z[0], z[2]), (0.0, 300.0));
        let z = picture_rect(r, 400, 400, 3.0, (-500.0, 0.0));
        assert_eq!((z[0], z[2]), (-100.0, 200.0));
    }

    #[test]
    fn the_animation_bar_fits_inside_the_box_in_order() {
        let r = [10.0, 20.0, 330.0, 240.0];
        for d in [1.0, 1.5, 2.0] {
            let b = anim_bar(r, d);
            assert!(b.strip[0] >= r[0] && b.strip[2] <= r[2] && b.strip[3] <= r[3]);
            assert!(b.prev[2] <= b.play[0] && b.play[2] <= b.next[0] && b.next[2] <= b.track[0] && b.track[0] < b.track[2]);
        }
    }

    #[test]
    fn frame_keys_differ_per_frame_and_per_item() {
        assert_ne!(frame_key(1, 0), frame_key(1, 1));
        assert_ne!(frame_key(1, 0), frame_key(2, 0));
        assert_ne!(frame_key(1, 0), info::preview_key(1));
        assert_ne!(mesh_key(1), big_key(1));
    }
}

#[allow(dead_code)]
fn _unused(_: Duration, _: Instant) {}
