//! The tools that work on the picture or in a panel section instead of on a plain slider: the crop frame with its ratio lock and
//! the straighten line (key R), the white-balance picker (key W), the curve editor and the hue wheel (the last two panel
//! sections). The arithmetic is in `nv_editor::cropmath` and `nv_editor::engine` (tested there); this is the mouse side.

use super::*;
use nv_editor::cropmath::{self, Handle, Ratio};
use nv_editor::engine::{eval_curve, BAND_HUES};
use nv_editor::recipe::{Band, Crop, Curve, RetouchKind, BAND_NAMES};

pub const ID_WHEEL: u64 = 6;
/// Sections of the panel after the seven slider groups.
pub const SECTION_CURVES: usize = 7;
pub const SECTION_WHEEL: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    None,
    Crop,
    Picker,
    /// A heal tool is on: clone, heal, remove, patch or red-eye.
    Heal(RetouchKind),
}

pub struct Tools {
    pub mode: Mode,
    /// The frame being drawn (0..1 of the turned picture) while the crop tool is on.
    pub frame: Crop,
    pub ratio: Ratio,
    pub crop_drag: Option<(Handle, (i32, i32), Crop)>,
    /// A straighten line being drawn: from, to (window pixels).
    pub level: Option<((i32, i32), (i32, i32))>,
    /// 0 = master, 1 = R, 2 = G, 3 = B
    pub curve_ch: usize,
    pub curve_drag: Option<usize>,
    pub band: usize,
    pub band_drag: Option<usize>,
    /// Where the fitted picture was drawn last (window pixels).
    pub pic_rect: [f32; 4],
    /// Brush size of the heal tools, percent of the picture's long side.
    pub brush: f32,
    /// The pointer over the picture (window pixels), for the brush outline.
    pub cursor: Option<(i32, i32)>,
    /// The stroke being painted, in 0..1 of the original picture.
    pub stroke: Vec<(f32, f32)>,
    /// Clone / heal: the place chosen with Ctrl+click, and the distance from the painted place to it once the first stroke fixed it.
    pub clone_src: Option<(f32, f32)>,
    pub clone_offset: Option<(f32, f32)>,
    pub patch: Option<heal::PatchState>,
}

impl Default for Tools {
    fn default() -> Tools {
        Tools {
            mode: Mode::None,
            frame: Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            ratio: Ratio::Free,
            crop_drag: None,
            level: None,
            curve_ch: 0,
            curve_drag: None,
            band: 3,
            band_drag: None,
            pic_rect: [0.0; 4],
            brush: 2.0,
            cursor: None,
            stroke: Vec::new(),
            clone_src: None,
            clone_offset: None,
            patch: None,
        }
    }
}

/// Where the content of the open Curves / Hue wheel section sits (window pixels).
pub struct Special {
    pub tabs: Vec<[f32; 4]>,
    pub area: [f32; 4],
    /// The three band sliders: whole row and track.
    pub rows: Vec<([f32; 4], [f32; 4])>,
}

const BAND_ROWS: [&str; 3] = ["Hue shift", "Saturation", "Brightness"];

impl App {
    /// Layout of the section content that starts at `y`; returns it and where the next header goes.
    pub(super) fn special_layout(&self, panel: [f32; 4], y: f32, section: usize) -> (Special, f32) {
        let d = self.dpi;
        let pad = 14.0 * d;
        let w = panel[2] - panel[0] - 2.0 * pad;
        if section == SECTION_CURVES {
            let tw = (w - 3.0 * 4.0 * d) / 4.0;
            let tabs = (0..4).map(|i| [panel[0] + pad + i as f32 * (tw + 4.0 * d), y + 4.0 * d, panel[0] + pad + i as f32 * (tw + 4.0 * d) + tw, y + 28.0 * d]).collect();
            let area = [panel[0] + pad, y + 36.0 * d, panel[0] + pad + w, y + 36.0 * d + w * 0.8];
            (Special { tabs, area, rows: Vec::new() }, area[3] + 30.0 * d)
        } else {
            let side = (w).min(170.0 * d);
            let top = y + 26.0 * d;
            let area = [panel[0] + (panel[2] - panel[0] - side) / 2.0, top, panel[0] + (panel[2] - panel[0] + side) / 2.0, top + side];
            let mut rows = Vec::new();
            let mut ry = area[3] + 8.0 * d;
            for _ in 0..3 {
                rows.push(([panel[0], ry, panel[2], ry + 38.0 * d], [panel[0] + pad, ry + 26.0 * d, panel[2] - pad, ry + 30.0 * d]));
                ry += 38.0 * d;
            }
            (Special { tabs: Vec::new(), area, rows }, ry + 6.0 * d)
        }
    }

    fn special(&self) -> Option<Special> {
        if !self.panel || (self.open_group != SECTION_CURVES && self.open_group != SECTION_WHEEL) {
            return None;
        }
        self.layout().special
    }

    /// The recipe as it is shown: while the crop tool is on, the whole picture (the frame is what is being decided).
    pub(super) fn display_recipe(&self) -> Recipe {
        let mut r = self.recipe.clone();
        if self.tools.mode == Mode::Crop || self.geo_overlay() {
            r.crop = None;
        }
        r
    }

    fn curve_mut(&mut self) -> &mut Curve {
        match self.tools.curve_ch {
            1 => &mut self.recipe.curves.r,
            2 => &mut self.recipe.curves.g,
            3 => &mut self.recipe.curves.b,
            _ => &mut self.recipe.curves.master,
        }
    }

    fn curve_ref(&self) -> &Curve {
        match self.tools.curve_ch {
            1 => &self.recipe.curves.r,
            2 => &self.recipe.curves.g,
            3 => &self.recipe.curves.b,
            _ => &self.recipe.curves.master,
        }
    }

    /// A pointer position as a point of the curve square: x and y in 0..1, y up.
    fn curve_point(area: [f32; 4], x: f32, y: f32) -> (f32, f32) {
        (((x - area[0]) / (area[2] - area[0])).clamp(0.0, 1.0), (1.0 - (y - area[1]) / (area[3] - area[1])).clamp(0.0, 1.0))
    }

    /// Index of the curve point within `radius` pixels of the pointer.
    fn curve_near(&self, area: [f32; 4], x: f32, y: f32, radius: f32) -> Option<usize> {
        let (w, h) = (area[2] - area[0], area[3] - area[1]);
        self.curve_ref().points.iter().position(|p| {
            let (sx, sy) = (area[0] + p.0 * w, area[3] - p.1 * h);
            ((sx - x).powi(2) + (sy - y).powi(2)).sqrt() <= radius
        })
    }

    fn set_band_value(&mut self, k: usize, x: f32) {
        let Some(sp) = self.special() else { return };
        let Some((_, track)) = sp.rows.get(k) else { return };
        let t = ((x - track[0]) / (track[2] - track[0]).max(1.0)).clamp(0.0, 1.0);
        let mut v = (t * 200.0 - 100.0).round();
        if v.abs() <= 2.0 {
            v = 0.0;
        }
        let b = &mut self.recipe.hue_wheel[self.tools.band];
        match k {
            0 => b.hue = v,
            1 => b.sat = v,
            _ => b.lum = v,
        }
        self.changed();
    }

    // ------------------------------------------------------------------------------------------ picture mapping

    /// The point of the picture as it is shown (0..1 across and down) under a window point, fitted or zoomed; None outside it.
    pub(super) fn screen_to_display(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        self.full.as_ref()?;
        let (nx, ny) = match self.view.zoom {
            Some(z) => {
                let a = self.area();
                let mid = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
                let (sw, sh) = self.shown_dims();
                ((self.view.center.0 + (x - mid.0) / z) / sw as f32, (self.view.center.1 + (y - mid.1) / z) / sh as f32)
            }
            None => {
                let r = self.tools.pic_rect;
                if r[2] <= r[0] || r[3] <= r[1] {
                    return None;
                }
                ((x - r[0]) / (r[2] - r[0]), (y - r[1]) / (r[3] - r[1]))
            }
        };
        ((0.0..1.0).contains(&nx) && (0.0..1.0).contains(&ny)).then_some((nx, ny))
    }

    /// The window point of a point of the shown picture (0..1).
    pub(super) fn display_to_screen(&self, nx: f32, ny: f32) -> Option<(f32, f32)> {
        self.full.as_ref()?;
        match self.view.zoom {
            Some(z) => {
                let a = self.area();
                let mid = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
                let (sw, sh) = self.shown_dims();
                Some((mid.0 + (nx * sw as f32 - self.view.center.0) * z, mid.1 + (ny * sh as f32 - self.view.center.1) * z))
            }
            None => {
                let r = self.tools.pic_rect;
                (r[2] > r[0]).then(|| (r[0] + nx * (r[2] - r[0]), r[1] + ny * (r[3] - r[1])))
            }
        }
    }

    // ------------------------------------------------------------------------------------------ keys

    /// R crop, W picker, and inside the crop tool Enter / Esc / A. True if the key was used.
    pub(super) fn tool_key(&mut self, vk: u32) -> bool {
        if matches!(self.tools.mode, Mode::None | Mode::Heal(_)) && self.heal_key(vk) {
            return true;
        }
        match (self.tools.mode, vk) {
            (Mode::Crop, 0x0D) | (Mode::Crop, 0x52) => {
                self.finish_crop(true);
                true
            }
            (Mode::Crop, 0x1B) => {
                self.finish_crop(false);
                true
            }
            (Mode::Crop, 0x41) => {
                self.tools.ratio = self.tools.ratio.next();
                let aspect = self.display_aspect();
                if let Some(v) = self.tools.ratio.value(aspect) {
                    self.tools.frame = cropmath::fit_ratio(self.tools.frame, v, aspect);
                }
                self.say(&format!("Crop ratio: {}", self.tools.ratio.label()));
                invalidate(self.hwnd);
                true
            }
            (Mode::Picker, 0x1B) | (Mode::Picker, 0x57) => {
                self.tools.mode = Mode::None;
                self.say("White balance picker off");
                true
            }
            (Mode::None, 0x52) => {
                self.begin_crop();
                true
            }
            (Mode::None, 0x57) => {
                if self.full.is_some() && self.can_pick() {
                    self.tools.mode = Mode::Picker;
                    self.say("Click something that should be neutral (grey or white)");
                }
                true
            }
            _ => false,
        }
    }

    fn can_pick(&mut self) -> bool {
        if !self.recipe.perspective_identity() {
            self.say("Pick the white balance with Pitch, Yaw and Zoom at zero");
            return false;
        }
        true
    }

    /// Width / height of the picture as it is shown now (turned, uncropped).
    fn display_aspect(&self) -> f32 {
        let r = self.tools.pic_rect;
        if r[3] > r[1] && r[2] > r[0] {
            (r[2] - r[0]) / (r[3] - r[1])
        } else {
            self.full.as_ref().map(|f| f.w as f32 / f.h as f32).unwrap_or(1.5)
        }
    }

    fn begin_crop(&mut self) {
        if self.full.is_none() {
            return;
        }
        if self.view.zoom.is_some() {
            self.leave_detail();
        }
        self.tools.mode = Mode::Crop;
        self.tools.frame = self.recipe.crop.unwrap_or(Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        self.say("Crop: drag the frame · A ratio · Ctrl+drag a line to level · Enter applies · Esc cancels");
        self.rerender();
        invalidate(self.hwnd);
    }

    pub(super) fn finish_crop(&mut self, apply: bool) {
        let f = self.tools.frame;
        self.tools.mode = Mode::None;
        self.tools.crop_drag = None;
        self.tools.level = None;
        if apply {
            let whole = f.x <= 0.001 && f.y <= 0.001 && f.w >= 0.998 && f.h >= 0.998;
            let new = if whole { None } else { Some(f) };
            if new != self.recipe.crop {
                self.snapshot();
                self.recipe.crop = new;
            }
        }
        self.changed();
    }

    // ------------------------------------------------------------------------------------------ mouse

    /// A left press on a tool (crop frame, picker, curve square, hue wheel). True if used.
    pub(super) fn tool_press(&mut self, x: i32, y: i32, ctrl: bool) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        if let Some(sp) = self.special() {
            if self.open_group == SECTION_CURVES {
                if let Some(i) = sp.tabs.iter().position(|r| contains(*r, fx, fy)) {
                    self.tools.curve_ch = i;
                    invalidate(self.hwnd);
                    return true;
                }
                if contains(sp.area, fx, fy) {
                    self.snapshot();
                    let idx = match self.curve_near(sp.area, fx, fy, 12.0 * self.dpi) {
                        Some(i) => i,
                        None => {
                            let (nx, ny) = Self::curve_point(sp.area, fx, fy);
                            let c = self.curve_mut();
                            c.set_point(nx, ny);
                            c.points.iter().position(|p| (p.0 - nx).abs() < 0.011).unwrap_or(0)
                        }
                    };
                    self.tools.curve_drag = Some(idx);
                    self.tool_drag(x, y);
                    return true;
                }
            } else {
                if contains(sp.area, fx, fy) {
                    let c = ((sp.area[0] + sp.area[2]) / 2.0, (sp.area[1] + sp.area[3]) / 2.0);
                    let hue = (fx - c.0).atan2(-(fy - c.1)).to_degrees().rem_euclid(360.0);
                    let near = |h: f32| ((hue - h + 540.0).rem_euclid(360.0) - 180.0).abs();
                    self.tools.band = (0..8).min_by(|a, b| near(BAND_HUES[*a]).total_cmp(&near(BAND_HUES[*b]))).unwrap_or(0);
                    invalidate(self.hwnd);
                    return true;
                }
                if let Some(k) = sp.rows.iter().position(|(r, _)| contains(*r, fx, fy)) {
                    self.snapshot();
                    self.tools.band_drag = Some(k);
                    self.set_band_value(k, fx);
                    return true;
                }
            }
        }
        let area = self.area();
        if !contains(area, fx, fy) || self.full.is_none() {
            return false;
        }
        match self.tools.mode {
            Mode::Picker => {
                self.pick_white(fx, fy);
                true
            }
            Mode::Crop => {
                let r = self.tools.pic_rect;
                if r[2] <= r[0] {
                    return true;
                }
                if ctrl {
                    self.tools.level = Some(((x, y), (x, y)));
                    return true;
                }
                let (nx, ny) = ((fx - r[0]) / (r[2] - r[0]), (fy - r[1]) / (r[3] - r[1]));
                let tol = 14.0 * self.dpi / (r[2] - r[0]).min(r[3] - r[1]);
                let (handle, from) = match cropmath::hit(&self.tools.frame, nx, ny, tol) {
                    Some(h) => (h, self.tools.frame),
                    None => {
                        // Outside the frame: start a new one at this point and pull its far corner.
                        let (cx, cy) = (nx.clamp(0.0, 1.0 - cropmath::MIN_SIDE), ny.clamp(0.0, 1.0 - cropmath::MIN_SIDE));
                        let c = Crop { x: cx, y: cy, w: cropmath::MIN_SIDE, h: cropmath::MIN_SIDE };
                        self.tools.frame = c;
                        (Handle::SE, c)
                    }
                };
                self.tools.crop_drag = Some((handle, (x, y), from));
                invalidate(self.hwnd);
                true
            }
            Mode::Heal(kind) => self.heal_press(kind, x, y, ctrl),
            Mode::None => false,
        }
    }

    /// The pointer moved with the left button down. True if a tool took it.
    pub(super) fn tool_drag(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        if let Mode::Heal(_) = self.tools.mode {
            if contains(self.area(), fx, fy) && self.drag.is_none() {
                return self.heal_drag(x, y);
            }
        }
        if let Some(idx) = self.tools.curve_drag {
            if let Some(sp) = self.special() {
                let (nx, ny) = Self::curve_point(sp.area, fx, fy);
                let c = self.curve_mut();
                if idx < c.points.len() {
                    let lo = if idx > 0 { c.points[idx - 1].0 + 0.01 } else { 0.0 };
                    let hi = if idx + 1 < c.points.len() { c.points[idx + 1].0 - 0.01 } else { 1.0 };
                    c.points[idx] = (nx.clamp(lo, hi.max(lo)), ny);
                    self.changed();
                }
            }
            return true;
        }
        if let Some(k) = self.tools.band_drag {
            self.set_band_value(k, fx);
            return true;
        }
        if let Some((handle, start, from)) = self.tools.crop_drag {
            let r = self.tools.pic_rect;
            let (dx, dy) = ((x - start.0) as f32 / (r[2] - r[0]).max(1.0), (y - start.1) as f32 / (r[3] - r[1]).max(1.0));
            let aspect = self.display_aspect();
            self.tools.frame = cropmath::drag(from, handle, dx, dy, self.tools.ratio.value(aspect), aspect);
            invalidate(self.hwnd);
            return true;
        }
        if let Some((from, _)) = self.tools.level {
            self.tools.level = Some((from, (x, y)));
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    pub(super) fn tool_release(&mut self) {
        self.heal_release();
        self.tools.curve_drag = None;
        self.tools.band_drag = None;
        self.tools.crop_drag = None;
        if let Some((a, b)) = self.tools.level.take() {
            let len = (((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2)) as f32).sqrt();
            if len > 20.0 {
                let turn = cropmath::level_angle((a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32));
                self.snapshot();
                let ang = nv_editor::raster::normalize_degrees(self.recipe.get(P::Angle) + turn);
                self.recipe.set(P::Angle, if ang > 180.0 { ang - 360.0 } else { ang });
                self.tools.frame = Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
                self.say(&format!("Levelled: turned {turn:+.1}°"));
                self.changed();
            }
            invalidate(self.hwnd);
        }
    }

    /// Right click: remove a curve point, reset the selected band. True if used.
    pub(super) fn tool_right(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        let Some(sp) = self.special() else { return false };
        if self.open_group == SECTION_CURVES {
            if contains(sp.area, fx, fy) {
                if let Some(i) = self.curve_near(sp.area, fx, fy, 14.0 * self.dpi) {
                    self.snapshot();
                    self.curve_mut().points.remove(i);
                    self.changed();
                }
                return true;
            }
        } else {
            if contains(sp.area, fx, fy) {
                self.snapshot();
                self.recipe.hue_wheel[self.tools.band] = Band { hue: 0.0, sat: 0.0, lum: 0.0 };
                self.changed();
                return true;
            }
            if let Some(k) = sp.rows.iter().position(|(r, _)| contains(*r, fx, fy)) {
                self.snapshot();
                let b = &mut self.recipe.hue_wheel[self.tools.band];
                match k {
                    0 => b.hue = 0.0,
                    1 => b.sat = 0.0,
                    _ => b.lum = 0.0,
                }
                self.changed();
                return true;
            }
        }
        false
    }

    /// The white balance picker: average a small square of the picture, in linear light, and make it the neutral.
    fn pick_white(&mut self, x: f32, y: f32) {
        let Some((sx, sy)) = self.screen_to_source(x, y) else {
            self.say("Click on the picture");
            return;
        };
        let Some(f) = &self.full else { return };
        let (px, py) = (sx * f.w as f32, sy * f.h as f32);
        let half = 5i64;
        let (mut sum, mut n) = ([0.0f64; 3], 0.0f64);
        for yy in (py as i64 - half).max(0)..=(py as i64 + half).min(f.h as i64 - 1) {
            for xx in (px as i64 - half).max(0)..=(px as i64 + half).min(f.w as i64 - 1) {
                let i = (yy as usize * f.w as usize + xx as usize) * 4;
                for c in 0..3 {
                    let v = f.rgba[i + c] as f64 / 255.0;
                    sum[c] += if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
                }
                n += 1.0;
            }
        }
        if n == 0.0 {
            return;
        }
        let chart = [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32];
        self.tools.mode = Mode::None;
        if chart.iter().all(|c| *c < 0.002) {
            self.say("That spot is black: pick something grey or white");
            return;
        }
        self.snapshot();
        self.recipe.wb_chart = Some(chart);
        self.say("White balance set from the picked spot (reset the White balance group to clear it)");
        self.changed();
    }
}

// ---------------------------------------------------------------------------------------------------- drawing

/// The dimmed outside, the frame, the thirds, the handles and the level line of the crop tool.
pub fn draw_crop(r: &mut Renderer, text: &mut Text, t: &Tools, full: (u32, u32), angle: f32, d: f32) {
    let p = t.pic_rect;
    if p[2] <= p[0] {
        return;
    }
    let (pw, ph) = (p[2] - p[0], p[3] - p[1]);
    let f = t.frame;
    let (l, tp, rr, b) = (p[0] + f.x * pw, p[1] + f.y * ph, p[0] + (f.x + f.w) * pw, p[1] + (f.y + f.h) * ph);
    let dim = rgb(0x000000, 0.6);
    let _ = r.fill_rect([p[0], p[1], p[2], tp], dim);
    let _ = r.fill_rect([p[0], b, p[2], p[3]], dim);
    let _ = r.fill_rect([p[0], tp, l, b], dim);
    let _ = r.fill_rect([rr, tp, p[2], b], dim);
    let white = rgb(0xFFFFFF, 0.95);
    let line = rgb(0xFFFFFF, 0.35);
    for k in 1..3 {
        let (x, y) = (l + (rr - l) * k as f32 / 3.0, tp + (b - tp) * k as f32 / 3.0);
        let _ = r.fill_rect([x - 0.5, tp, x + 0.5, b], line);
        let _ = r.fill_rect([l, y - 0.5, rr, y + 0.5], line);
    }
    for e in [[l, tp, rr, tp + 1.5], [l, b - 1.5, rr, b], [l, tp, l + 1.5, b], [rr - 1.5, tp, rr, b]] {
        let _ = r.fill_rect(e, white);
    }
    let hs = 5.0 * d;
    for (x, y) in [(l, tp), (rr, tp), (l, b), (rr, b), ((l + rr) / 2.0, tp), ((l + rr) / 2.0, b), (l, (tp + b) / 2.0), (rr, (tp + b) / 2.0)] {
        let _ = r.fill_rect([x - hs, y - hs, x + hs, y + hs], white);
    }
    // Size of the result in pixels.
    let (sw, sh) = nv_editor::raster::rotated_size(full.0, full.1, angle);
    let label = format!("{} x {}   ratio {}", (f.w * sw as f32).round(), (f.h * sh as f32).round(), t.ratio.label());
    let w = text.width(&label, (12.0 * d) as i32) as f32 + 20.0 * d;
    let (x, y) = ((l + rr) / 2.0 - w / 2.0, (tp - 30.0 * d).max(p[1] + 4.0 * d));
    let _ = r.fill_rect([x, y, x + w, y + 22.0 * d], rgb(0x000000, 0.7));
    text.draw(r, &label, (12.0 * d) as i32, x + 10.0 * d, y + 3.0 * d, rgb(0xF0F0F0, 1.0));
    if let Some((a, b2)) = t.level {
        let n = (((b2.0 - a.0).pow(2) + (b2.1 - a.1).pow(2)) as f32).sqrt().max(1.0);
        let steps = (n / 3.0) as i32;
        for i in 0..=steps {
            let (x, y) = (a.0 as f32 + (b2.0 - a.0) as f32 * i as f32 / steps.max(1) as f32, a.1 as f32 + (b2.1 - a.1) as f32 * i as f32 / steps.max(1) as f32);
            let _ = r.fill_rect([x - 1.5, y - 1.5, x + 1.5, y + 1.5], rgb(0xFFD23D, 1.0));
        }
    }
}

/// The curve editor: channel tabs, the square with grid, the curve and its points.
pub fn draw_curves(r: &mut Renderer, text: &mut Text, t: &Tools, rec: &Recipe, sp: &Special, hot_tab: Option<usize>, d: f32) {
    let (text_c, dim_c, accent) = (rgb(pal::TEXT, 1.0), rgb(pal::DIM, 1.0), rgb(pal::ACCENT, 1.0));
    for (i, (rect, name)) in sp.tabs.iter().zip(["RGB", "R", "G", "B"]).enumerate() {
        let fill = if t.curve_ch == i { accent } else if hot_tab == Some(i) { rgb(pal::TILE_HOVER, 1.0) } else { rgb(pal::TILE, 1.0) };
        let _ = r.fill_rect(*rect, fill);
        text.draw_centered(r, name, (12.0 * d) as i32, (rect[0] + rect[2]) / 2.0, rect[1] + 4.0 * d, text_c);
    }
    let a = sp.area;
    let (w, h) = (a[2] - a[0], a[3] - a[1]);
    let _ = r.fill_rect(a, rgb(0x15171A, 1.0));
    for k in 1..4 {
        let f = k as f32 / 4.0;
        let _ = r.fill_rect([a[0] + w * f - 0.5, a[1], a[0] + w * f + 0.5, a[3]], rgb(0x2E3238, 1.0));
        let _ = r.fill_rect([a[0], a[1] + h * f - 0.5, a[2], a[1] + h * f + 0.5], rgb(0x2E3238, 1.0));
    }
    // The picture's own diagonal, then the curve of each channel (the chosen one bright, the others faint).
    let mut diag = 0.0;
    while diag < w {
        let _ = r.fill_rect([a[0] + diag, a[3] - h * diag / w - 1.0, a[0] + diag + 2.0, a[3] - h * diag / w + 1.0], rgb(0x4A4F58, 1.0));
        diag += 4.0 * d;
    }
    let chans: [(&Curve, u32); 4] = [(&rec.curves.master, 0xF0F0F0), (&rec.curves.r, 0xFF5A5A), (&rec.curves.g, 0x5AE06A), (&rec.curves.b, 0x6A8CFF)];
    for pass in 0..2 {
        for (i, (c, col)) in chans.iter().enumerate() {
            if (i == t.curve_ch) != (pass == 1) || c.points.is_empty() {
                continue;
            }
            let alpha = if pass == 1 { 1.0 } else { 0.35 };
            let mut x = 0.0;
            while x <= w {
                let y = eval_curve(&c.points, x / w);
                let sy = a[3] - y * h;
                let _ = r.fill_rect([a[0] + x, sy - 1.0, a[0] + x + 2.0, sy + 1.0], rgb(*col, alpha));
                x += 2.0;
            }
        }
    }
    let cur = chans[t.curve_ch];
    for p in &cur.0.points {
        let (sx, sy) = (a[0] + p.0 * w, a[3] - p.1 * h);
        let _ = r.fill_rect([sx - 4.0 * d, sy - 4.0 * d, sx + 4.0 * d, sy + 4.0 * d], rgb(0xFFFFFF, 1.0));
        let _ = r.fill_rect([sx - 2.5 * d, sy - 2.5 * d, sx + 2.5 * d, sy + 2.5 * d], rgb(cur.1, 1.0));
    }
    text.draw(r, "click adds a point · drag moves · right-click removes", (10.0 * d) as i32, a[0], a[3] + 8.0 * d, dim_c);
}

/// A ring of hues as a bitmap `n` pixels square, transparent outside the ring.
pub fn wheel_image(n: u32) -> Image {
    let mut rgba = vec![0u8; (n * n * 4) as usize];
    let c = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            let rad = (dx * dx + dy * dy).sqrt() / c;
            if !(0.6..=1.0).contains(&rad) {
                continue;
            }
            let h = dx.atan2(-dy).to_degrees().rem_euclid(360.0) / 60.0;
            let f = h - h.floor();
            let (r, g, b) = match h as u32 {
                0 => (1.0, f, 0.0),
                1 => (1.0 - f, 1.0, 0.0),
                2 => (0.0, 1.0, f),
                3 => (0.0, 1.0 - f, 1.0),
                4 => (f, 0.0, 1.0),
                _ => (1.0, 0.0, 1.0 - f),
            };
            let edge = ((1.0 - rad) * c).min((rad - 0.6) * c).clamp(0.0, 1.0); // soft edges
            let i = ((y * n + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, (edge * 255.0) as u8]);
        }
    }
    Image::new(n, n, rgba)
}

/// The hue wheel section: the ring with a marker per band, the name of the chosen band and its three sliders.
pub fn draw_wheel(r: &mut Renderer, text: &mut Text, t: &Tools, rec: &Recipe, sp: &Special, d: f32) {
    let (text_c, dim_c, accent) = (rgb(pal::TEXT, 1.0), rgb(pal::DIM, 1.0), rgb(pal::ACCENT, 1.0));
    let a = sp.area;
    let name = BAND_NAMES[t.band];
    let title = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
    text.draw(r, &format!("Band: {title}  (click the ring to choose)"), (11.0 * d) as i32, sp.rows[0].0[0] + 14.0 * d, a[1] - 20.0 * d, dim_c);
    let _ = r.draw_image(ID_WHEEL, a, false);
    let (cx, cy) = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
    let ring = (a[2] - a[0]) / 2.0 * 0.8;
    for (k, hue) in BAND_HUES.iter().enumerate() {
        let (sx, sy) = (cx + ring * hue.to_radians().sin(), cy - ring * hue.to_radians().cos());
        let b = rec.hue_wheel[k];
        let sel = k == t.band;
        let s = if sel { 6.0 * d } else { 3.5 * d };
        let _ = r.fill_rect([sx - s - 1.5, sy - s - 1.5, sx + s + 1.5, sy + s + 1.5], rgb(if sel { 0xFFFFFF } else { 0x101010 }, 1.0));
        let changed = b.hue != 0.0 || b.sat != 0.0 || b.lum != 0.0;
        let _ = r.fill_rect([sx - s, sy - s, sx + s, sy + s], rgb(if changed { 0xFFD23D } else { 0x30343A }, 1.0));
    }
    let b = rec.hue_wheel[t.band];
    for (k, ((row, track), label)) in sp.rows.iter().zip(BAND_ROWS).enumerate() {
        let v = [b.hue, b.sat, b.lum][k];
        text.draw(r, label, (12.0 * d) as i32, row[0] + 14.0 * d, row[1] + 4.0 * d, text_c);
        let vt = format!("{}{:.0}", if v > 0.0 { "+" } else { "" }, v);
        text.draw_right(r, &vt, (12.0 * d) as i32, row[2] - 14.0 * d, row[1] + 4.0 * d, if v == 0.0 { dim_c } else { accent });
        let _ = r.fill_rect(*track, rgb(0x2E3238, 1.0));
        let (mid, pos) = ((track[0] + track[2]) / 2.0, track[0] + (track[2] - track[0]) * (v + 100.0) / 200.0);
        if v != 0.0 {
            let _ = r.fill_rect([mid.min(pos), track[1], mid.max(pos), track[3]], accent);
        }
        let _ = r.fill_rect([pos - 5.0 * d, track[1] - 6.0 * d, pos + 5.0 * d, track[3] + 6.0 * d], rgb(0xE8E8E8, 1.0));
    }
}
