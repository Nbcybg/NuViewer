//! The heal tools on the picture: clone, heal, remove, patch and red-eye. Strokes are made with the mouse (keys K J M P E, or the
//! strip in the Heal group) and kept in the recipe as `Retouch` entries in 0..1 of the original picture, so they stay on the same
//! pixels when the turn, flip or crop changes. The engine (`nv_editor::retouch`) does the pixel work; the picture is updated
//! when a stroke is finished (the brush outline and the path show while it is being made).

use super::panel::TOOL_KINDS;
use super::*;
use nv_editor::recipe::{Retouch, RetouchKind};

/// A patch in progress: the lassoed area and how far it has been dragged towards the good area.
pub struct PatchState {
    pub poly: Vec<(f32, f32)>,
    /// Drag from this point (original-picture coordinates), if the area is being moved.
    pub from: Option<(f32, f32)>,
    pub offset: (f32, f32),
}

/// What the picture area shows on top of the picture while a heal tool is on (window pixels).
pub struct HealOverlay {
    pub brush: Option<(f32, f32, f32)>,
    pub stroke: Vec<(f32, f32)>,
    pub source: Option<(f32, f32)>,
    pub poly: Vec<(f32, f32)>,
    pub ghost: Vec<(f32, f32)>,
}

impl App {
    /// The heal tool strip or a key: toggle a tool.
    pub(super) fn set_heal_tool(&mut self, kind: RetouchKind) {
        if self.full.is_none() {
            return;
        }
        if !self.recipe.perspective_identity() {
            self.say("The heal tools need Pitch, Yaw and Zoom at zero");
            return;
        }
        if self.tools.mode == Mode::Crop {
            self.finish_crop(true);
        }
        if self.tools.mode == Mode::Heal(kind) {
            self.end_heal();
            return;
        }
        self.tools.mode = Mode::Heal(kind);
        self.tools.stroke.clear();
        self.tools.patch = None;
        let msg = match kind {
            RetouchKind::Clone => "Clone: Ctrl+click the place to copy from, then paint · [ ] brush size · Esc leaves",
            RetouchKind::Heal => "Heal: Ctrl+click the good place, then paint over the flaw · [ ] brush size · Esc leaves",
            RetouchKind::Remove => "Remove: paint over what should go · [ ] brush size · Esc leaves",
            RetouchKind::Patch => "Patch: draw around the flaw, then drag the outlined area onto good skin / sky and let go · Esc cancels",
            RetouchKind::RedEye => "Red-eye: click on each eye · [ ] brush size · Esc leaves",
        };
        self.say(msg);
        self.update_title();
        invalidate(self.hwnd);
    }

    fn end_heal(&mut self) {
        self.tools.mode = Mode::None;
        self.tools.stroke.clear();
        self.tools.patch = None;
        self.tools.cursor = None;
        self.update_title();
        invalidate(self.hwnd);
    }

    /// Keys while a heal tool is on or to start one. True if used.
    pub(super) fn heal_key(&mut self, vk: u32) -> bool {
        for (kind, _, _) in TOOL_KINDS {
            let key = match kind {
                RetouchKind::Clone => 0x4B,
                RetouchKind::Heal => 0x4A,
                RetouchKind::Remove => 0x4D,
                RetouchKind::Patch => 0x50,
                RetouchKind::RedEye => 0x45,
            };
            if vk == key && matches!(self.tools.mode, Mode::None | Mode::Heal(_)) {
                self.set_heal_tool(kind);
                return true;
            }
        }
        let Mode::Heal(_) = self.tools.mode else { return false };
        match vk {
            0x1B => {
                if self.tools.patch.is_some() || !self.tools.stroke.is_empty() {
                    self.tools.patch = None;
                    self.tools.stroke.clear();
                    self.say("Cancelled");
                    invalidate(self.hwnd);
                } else {
                    self.end_heal();
                }
                true
            }
            0xDB | 0xDD => {
                let b = (self.tools.brush * if vk == 0xDD { 1.25 } else { 0.8 }).clamp(0.2, 12.0);
                self.num_set(panel::Num::Brush, b);
                true
            }
            _ => false,
        }
    }

    /// The original-picture point (0..1) under a window point; None outside the picture, or with a keystone on (the engine's
    /// mapping does not cover Pitch / Yaw / Zoom).
    pub(super) fn screen_to_source(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let f = self.full.as_ref()?;
        if !self.recipe.perspective_identity() {
            return None;
        }
        let (nx, ny) = self.screen_to_display(x, y)?;
        let (sx, sy) = engine::display_to_source(&self.recipe, f.w, f.h, nx, ny);
        ((0.0..1.0).contains(&sx) && (0.0..1.0).contains(&sy)).then_some((sx, sy))
    }

    /// The window point of an original-picture point; None when it can't be told.
    fn source_to_screen(&self, sx: f32, sy: f32) -> Option<(f32, f32)> {
        let f = self.full.as_ref()?;
        let (nx, ny) = engine::source_to_display(&self.recipe, f.w, f.h, sx, sy);
        self.display_to_screen(nx, ny)
    }

    /// Window pixels per pixel of the original picture.
    fn screen_per_source(&self) -> f32 {
        if let Some(z) = self.view.zoom {
            return z;
        }
        let r = self.tools.pic_rect;
        let shown = self.shown_dims().0 as f32;
        if r[2] > r[0] && shown > 0.0 { (r[2] - r[0]) / shown } else { 1.0 }
    }

    /// The brush radius as a fraction of the picture's long side.
    fn brush_fraction(&self) -> f32 {
        self.tools.brush / 100.0
    }

    // ------------------------------------------------------------------------------------------ mouse

    pub(super) fn heal_press(&mut self, kind: RetouchKind, x: i32, y: i32, ctrl: bool) -> bool {
        let Some(p) = self.screen_to_source(x as f32, y as f32) else { return true };
        self.tools.cursor = Some((x, y));
        match kind {
            RetouchKind::Clone | RetouchKind::Heal if ctrl => {
                self.tools.clone_src = Some(p);
                self.tools.clone_offset = None;
                self.say("Source set: now paint where it should be copied to");
            }
            RetouchKind::Clone | RetouchKind::Heal => {
                if self.tools.clone_src.is_none() {
                    self.say("Ctrl+click the place to copy from first");
                } else {
                    self.tools.stroke = vec![p];
                }
            }
            RetouchKind::Remove => self.tools.stroke = vec![p],
            RetouchKind::RedEye => {
                self.push_stroke(Retouch { kind, radius: self.brush_fraction(), source: None, points: vec![p] });
            }
            RetouchKind::Patch => {
                let inside = self.tools.patch.as_ref().map(|s| point_in_polygon(&s.poly, p)).unwrap_or(false);
                if inside {
                    if let Some(s) = &mut self.tools.patch {
                        s.from = Some(p);
                    }
                } else {
                    self.tools.patch = None;
                    self.tools.stroke = vec![p];
                }
            }
        }
        invalidate(self.hwnd);
        true
    }

    pub(super) fn heal_drag(&mut self, x: i32, y: i32) -> bool {
        let Mode::Heal(kind) = self.tools.mode else { return false };
        self.tools.cursor = Some((x, y));
        if let Some(p) = self.screen_to_source(x as f32, y as f32) {
            if kind == RetouchKind::Patch {
                if let Some(s) = &mut self.tools.patch {
                    if let Some(from) = s.from {
                        s.offset = (p.0 - from.0, p.1 - from.1);
                        invalidate(self.hwnd);
                        return true;
                    }
                }
            }
            if !self.tools.stroke.is_empty() {
                let last = *self.tools.stroke.last().unwrap();
                let (a, b) = (self.source_to_screen(last.0, last.1), (x as f32, y as f32));
                let far = a.map(|a| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() >= 2.0 * self.dpi).unwrap_or(true);
                if far {
                    self.tools.stroke.push(p);
                }
            }
        }
        invalidate(self.hwnd);
        true
    }

    pub(super) fn heal_release(&mut self) {
        let Mode::Heal(kind) = self.tools.mode else { return };
        if kind == RetouchKind::Patch {
            if let Some(s) = &mut self.tools.patch {
                if s.from.take().is_some() {
                    let (poly, off) = (s.poly.clone(), s.offset);
                    self.tools.patch = None;
                    if off.0.abs() + off.1.abs() > 0.002 {
                        let src = (poly[0].0 + off.0, poly[0].1 + off.1);
                        self.push_stroke(Retouch { kind, radius: self.brush_fraction() * 0.5, source: Some(src), points: poly });
                    }
                    return;
                }
            }
            let stroke = std::mem::take(&mut self.tools.stroke);
            if stroke.len() >= 3 {
                self.tools.patch = Some(PatchState { poly: stroke, from: None, offset: (0.0, 0.0) });
                self.say("Now drag the outlined area onto a good place and let go · Esc cancels");
            }
            invalidate(self.hwnd);
            return;
        }
        let stroke = std::mem::take(&mut self.tools.stroke);
        if stroke.is_empty() {
            return;
        }
        let source = match kind {
            RetouchKind::Clone | RetouchKind::Heal => {
                let Some(src) = self.tools.clone_src else { return };
                // The first stroke fixes how far the source is from where we paint; later strokes keep that distance.
                let off = *self.tools.clone_offset.get_or_insert((src.0 - stroke[0].0, src.1 - stroke[0].1));
                Some((stroke[0].0 + off.0, stroke[0].1 + off.1))
            }
            _ => None,
        };
        self.push_stroke(Retouch { kind, radius: self.brush_fraction(), source, points: stroke });
    }

    fn push_stroke(&mut self, s: Retouch) {
        self.snapshot();
        self.recipe.retouch.push(s);
        self.changed();
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ overlay

    /// The brush, the path being painted, the clone source and the patch outlines, in window pixels.
    pub(super) fn heal_overlay(&self) -> Option<HealOverlay> {
        let Mode::Heal(kind) = self.tools.mode else { return None };
        let k = self.screen_per_source();
        let long = self.full.as_ref().map(|f| f.w.max(f.h) as f32).unwrap_or(1.0);
        let radius = self.brush_fraction() * long * k;
        let map = |pts: &[(f32, f32)], off: (f32, f32)| -> Vec<(f32, f32)> { pts.iter().filter_map(|p| self.source_to_screen(p.0 + off.0, p.1 + off.1)).collect() };
        let stroke = map(&self.tools.stroke, (0.0, 0.0));
        let (mut poly, mut ghost) = (Vec::new(), Vec::new());
        if let Some(s) = &self.tools.patch {
            poly = map(&s.poly, (0.0, 0.0));
            ghost = map(&s.poly, s.offset);
        } else if kind == RetouchKind::Patch {
            poly = stroke.clone();
        }
        // Where the pixels come from: the chosen source, moved along with the brush once the offset is known.
        let source = match (kind, self.tools.clone_src, self.tools.clone_offset, self.tools.cursor, self.tools.stroke.first()) {
            (RetouchKind::Clone | RetouchKind::Heal, Some(src), None, _, None) => self.source_to_screen(src.0, src.1),
            (RetouchKind::Clone | RetouchKind::Heal, Some(_), Some(off), Some(c), _) => {
                self.screen_to_source(c.0 as f32, c.1 as f32).and_then(|p| self.source_to_screen(p.0 + off.0, p.1 + off.1))
            }
            (RetouchKind::Clone | RetouchKind::Heal, Some(src), None, Some(c), Some(first)) => {
                self.screen_to_source(c.0 as f32, c.1 as f32).and_then(|p| self.source_to_screen(src.0 + (p.0 - first.0), src.1 + (p.1 - first.1)))
            }
            _ => None,
        };
        let brush = self.tools.cursor.map(|c| (c.0 as f32, c.1 as f32, radius.max(2.0)));
        Some(HealOverlay { brush, stroke, source, poly, ghost })
    }
}

fn point_in_polygon(poly: &[(f32, f32)], p: (f32, f32)) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            inside = !inside;
        }
    }
    inside
}

/// Draw the overlay: a ring for the brush, the path, the source cross and the patch outlines.
pub fn draw_heal(r: &mut Renderer, o: &HealOverlay, d: f32) {
    let ring = |r: &mut Renderer, cx: f32, cy: f32, rad: f32, col: [f32; 4]| {
        let n = ((rad * 2.0 * std::f32::consts::PI) / 3.0).clamp(16.0, 120.0) as i32;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (x, y) = (cx + rad * a.cos(), cy + rad * a.sin());
            let _ = r.fill_rect([x - 1.0, y - 1.0, x + 1.0, y + 1.0], col);
        }
    };
    let line = |r: &mut Renderer, pts: &[(f32, f32)], close: bool, col: [f32; 4]| {
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            if i + 1 == n && !close {
                break;
            }
            let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let steps = (len / 2.0).ceil().max(1.0) as i32;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                let _ = r.fill_rect([x - 1.0, y - 1.0, x + 1.0, y + 1.0], col);
            }
        }
    };
    if let Some((x, y, rad)) = o.brush {
        ring(r, x, y, rad + 1.0, rgb(0x000000, 0.7));
        ring(r, x, y, rad, rgb(0xFFFFFF, 0.95));
    }
    if o.stroke.len() > 1 {
        line(r, &o.stroke, false, rgb(0xFFD23D, 0.9));
    }
    if !o.poly.is_empty() {
        line(r, &o.poly, true, rgb(0xFFFFFF, 0.95));
    }
    if !o.ghost.is_empty() {
        line(r, &o.ghost, true, rgb(0x6AE0FF, 0.95));
    }
    if let Some((x, y)) = o.source {
        let s = 7.0 * d;
        let _ = r.fill_rect([x - s, y - 1.0, x + s, y + 1.0], rgb(0x6AE0FF, 1.0));
        let _ = r.fill_rect([x - 1.0, y - s, x + 1.0, y + s], rgb(0x6AE0FF, 1.0));
    }
}
