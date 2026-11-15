//! Looking at the picture: fit / zoom / pan, the 100% detail renderer, the A / B split, the scopes and false colour.
//!
//! Fit mode shows the *proxy* (the picture shrunk to the window). Zoomed in, only the part of the picture that is on screen is
//! cut out of a level of the picture's pyramid (full size, half, quarter ...; always one whose scale is within a factor of 2
//! of the zoom) and run through the engine, so a zoom step costs about the same as a slider move at fit: the work follows the
//! window, not the picture. `engine::Frame` tells the engine where the cut-out sits so the vignette comes out identical.

use super::*;
use nv_editor::scopes;

pub const ID_SCOPE: u64 = 3;
pub const ID_DETAIL: u64 = 4;
pub const ID_DETAIL_ORIG: u64 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Off,
    Histogram,
    Parade,
    Vector,
}

impl Scope {
    pub fn from_setting(g: nv_core::settings::EditorGraph) -> Scope {
        use nv_core::settings::EditorGraph as G;
        match g {
            G::Histogram => Scope::Histogram,
            G::Parade => Scope::Parade,
            G::Vector => Scope::Vector,
            G::Off => Scope::Off,
        }
    }

    pub fn to_setting(self) -> nv_core::settings::EditorGraph {
        use nv_core::settings::EditorGraph as G;
        match self {
            Scope::Histogram => G::Histogram,
            Scope::Parade => G::Parade,
            Scope::Vector => G::Vector,
            Scope::Off => G::Off,
        }
    }

    pub fn next(self) -> Scope {
        match self {
            Scope::Off => Scope::Histogram,
            Scope::Histogram => Scope::Parade,
            Scope::Parade => Scope::Vector,
            Scope::Vector => Scope::Off,
        }
    }

    /// One word for the tool strip.
    pub fn short(self) -> &'static str {
        match self {
            Scope::Off => "off",
            Scope::Histogram => "histogram",
            Scope::Parade => "parade",
            Scope::Vector => "vector",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Scope::Off => "off",
            Scope::Histogram => "histogram",
            Scope::Parade => "RGB parade",
            Scope::Vector => "vector scope (orange line = skin)",
        }
    }
}

pub struct View {
    /// Screen pixels per picture pixel; None = fitted.
    pub zoom: Option<f32>,
    /// The picture point in the middle of the picture area, in full-size picture pixels.
    pub center: (f32, f32),
    /// The detail texture no longer matches the recipe / the position.
    pub detail_dirty: bool,
    /// The fit proxy was not updated while zoomed in.
    pub proxy_stale: bool,
    /// Pyramid levels below full size (index k = scale 1 / 2^(k+1)), built when first needed.
    pub levels: Vec<Option<Pixels>>,
    /// The full picture with the heal strokes, turn, flips and crop applied: what the levels are cut from whenever there is
    /// any of those. Key: what it was made for.
    pub base: Option<(String, Pixels)>,
    /// What the detail texture holds, for drawing: (left, top, right, bottom) in level pixels, and the level's scale.
    pub region: Option<([f32; 4], f32)>,
    /// Pointer position and view centre when a pan started.
    pub pan: Option<((i32, i32), (f32, f32))>,
    pub split: bool,
    pub split_at: f32,
    pub scope: Scope,
    /// The navigator is being dragged (it pans the picture).
    pub nav_drag: bool,
    pub false_colour: bool,
    /// Share of pixels at the lowest / highest level of any channel (for the label under the histogram).
    pub clipped: (f32, f32),
}

impl Default for View {
    fn default() -> View {
        View {
            zoom: None,
            center: (0.0, 0.0),
            detail_dirty: false,
            proxy_stale: false,
            levels: Vec::new(),
            base: None,
            region: None,
            pan: None,
            split: false,
            split_at: 0.5,
            // The graphs are on from the start: they are what you judge the edit by.
            scope: Scope::Histogram,
            nav_drag: false,
            false_colour: false,
            clipped: (0.0, 0.0),
        }
    }
}

impl App {
    /// Size in pixels of the picture as it is shown at full size: turned, flipped and cropped.
    pub(super) fn shown_dims(&self) -> (u32, u32) {
        let Some(f) = &self.full else { return (1, 1) };
        let angle = self.recipe.get(P::Angle) + 90.0 * (self.recipe.quarter_turns % 4) as f32;
        let (rw, rh) = nv_editor::raster::rotated_size(f.w, f.h, angle);
        match self.recipe.crop {
            Some(c) => {
                let x0 = ((c.x * rw as f32).round() as u32).min(rw.saturating_sub(1));
                let y0 = ((c.y * rh as f32).round() as u32).min(rh.saturating_sub(1));
                (((c.w * rw as f32).round() as u32).clamp(1, rw - x0), ((c.h * rh as f32).round() as u32).clamp(1, rh - y0))
            }
            None => (rw, rh),
        }
    }

    /// The zoom at which the whole picture fits the picture area (never above 100%).
    pub(super) fn fit_zoom(&self) -> f32 {
        if self.full.is_none() {
            return 1.0;
        }
        let (w, h) = self.shown_dims();
        let a = self.area();
        ((a[2] - a[0]) / w as f32).min((a[3] - a[1]) / h as f32).min(1.0)
    }

    /// The zoom in force: the chosen one, or the fit.
    pub(super) fn zoom_now(&self) -> f32 {
        self.view.zoom.unwrap_or_else(|| self.fit_zoom())
    }

    /// Zoom and pan work on the picture as it is shown (turned, flipped, cropped), except while the crop frame is being set.
    pub(super) fn can_zoom(&mut self) -> bool {
        if self.full.is_none() {
            return false;
        }
        if self.tools.mode == Mode::Crop {
            self.say("Finish the crop first (Enter applies, Esc cancels)");
            return false;
        }
        true
    }

    pub(super) fn clamp_center_pub(&mut self) {
        self.clamp_center();
    }

    /// Keep the view inside the picture (a picture smaller than the area stays centred).
    fn clamp_center(&mut self) {
        if self.full.is_none() {
            return;
        }
        let (w, h) = self.shown_dims();
        let z = self.zoom_now();
        let a = self.area();
        let (hw, hh) = ((a[2] - a[0]) / (2.0 * z), (a[3] - a[1]) / (2.0 * z));
        let clamp = |c: f32, half: f32, size: f32| if size <= 2.0 * half { size / 2.0 } else { c.clamp(half, size - half) };
        self.view.center = (clamp(self.view.center.0, hw, w as f32), clamp(self.view.center.1, hh, h as f32));
    }

    /// Zoom to `z` keeping the picture point under `anchor` (a point of the picture area, in window pixels) where it is.
    pub(super) fn set_zoom(&mut self, z: f32, anchor: (f32, f32)) {
        if !self.can_zoom() {
            return;
        }
        if self.full.is_none() {
            return;
        }
        let (w, h) = (self.shown_dims().0 as f32, self.shown_dims().1 as f32);
        let fit = self.fit_zoom();
        let z = z.clamp(fit, 16.0);
        let a = self.area();
        let mid = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        let old = self.zoom_now();
        let c0 = if self.view.zoom.is_some() { self.view.center } else { (w / 2.0, h / 2.0) };
        let point = (c0.0 + (anchor.0 - mid.0) / old, c0.1 + (anchor.1 - mid.1) / old);
        if z <= fit * 1.001 {
            self.leave_detail();
            return;
        }
        if self.view.zoom.is_none() && self.geo_overlay() && self.recipe.crop.is_some() {
            self.view.proxy_stale = true; // the fitted picture on the GPU is the uncropped one: bring it up to date on the way back
        }
        self.view.zoom = Some(z);
        self.view.center = (point.0 - (anchor.0 - mid.0) / z, point.1 - (anchor.1 - mid.1) / z);
        self.clamp_center();
        self.view.detail_dirty = true;
        self.update_title();
        invalidate(self.hwnd);
    }

    /// Back to the fitted picture.
    pub(super) fn leave_detail(&mut self) {
        self.view.zoom = None;
        self.view.region = None;
        self.view.pan = None;
        self.view.detail_dirty = false;
        if self.view.proxy_stale {
            self.view.proxy_stale = false;
            self.rerender();
        }
        self.update_title();
        invalidate(self.hwnd);
    }

    /// Keep the picture the levels are cut from in step with the heal strokes and the geometry (none of those: the picture itself).
    fn sync_base(&mut self) {
        let rec = &self.recipe;
        if rec.retouch.is_empty() && rec.geometry_identity() {
            if self.view.base.take().is_some() {
                self.view.levels.clear();
            }
            return;
        }
        let key = format!(
            "{:?}",
            (&rec.retouch, rec.get(P::Angle).to_bits(), rec.quarter_turns, rec.flip_h, rec.flip_v, rec.crop.map(|c| (c.x.to_bits(), c.y.to_bits(), c.w.to_bits(), c.h.to_bits())), rec.get(P::Pitch).to_bits(), rec.get(P::Yaw).to_bits(), rec.get(P::Zoom).to_bits())
        );
        if matches!(&self.view.base, Some((have, _)) if *have == key) {
            return;
        }
        let Some(full) = &self.full else { return };
        let retouched;
        let src = if rec.retouch.is_empty() {
            full
        } else {
            retouched = nv_editor::retouch::apply(full, &rec.retouch);
            &retouched
        };
        let g = engine::apply_geometry(src, rec);
        self.view.base = Some((key, g));
        self.view.levels.clear();
    }

    /// The pyramid level whose scale is the smallest one that is not below `z`: returns (level pixels, its scale).
    fn level_for(&mut self, z: f32) -> Option<(&Pixels, f32)> {
        self.sync_base();
        let (fw, fh) = match &self.view.base {
            Some((_, b)) => (b.w, b.h),
            None => self.full.as_ref().map(|f| (f.w, f.h))?,
        };
        let mut k = 0usize; // 0 = full size
        while k < 16 && 1.0 / (2u32.pow(k as u32 + 1) as f32) >= z {
            k += 1;
        }
        if k == 0 {
            return Some((self.view.base.as_ref().map(|b| &b.1).or(self.full.as_ref())?, 1.0));
        }
        let scale = 1.0 / 2f32.powi(k as i32);
        let long = fw.max(fh);
        let edge = ((long as f32 * scale).round() as u32).max(1);
        if self.view.levels.len() < k {
            self.view.levels.resize_with(k, || None);
        }
        if self.view.levels[k - 1].is_none() {
            let src: &mut Pixels = match &mut self.view.base {
                Some(b) => &mut b.1,
                None => self.full.as_mut()?,
            };
            let img = Image::new(src.w, src.h, std::mem::take(&mut src.rgba));
            let small = nv_codecs::downscale(&img, edge);
            src.rgba = img.rgba;
            self.view.levels[k - 1] = Some(Pixels::new(small.width, small.height, small.rgba));
        }
        let lvl = self.view.levels[k - 1].as_ref()?;
        Some((lvl, lvl.w as f32 / fw as f32))
    }

    /// Cut the visible part out of the right level, run the recipe on it and put it on the GPU.
    pub(super) fn render_detail(&mut self) {
        let Some(z) = self.view.zoom else { return };
        self.view.detail_dirty = false;
        if self.renderer.is_none() {
            return;
        }
        self.clamp_center();
        let a = self.area();
        let (aw, ah) = (a[2] - a[0], a[3] - a[1]);
        let center = self.view.center;
        let (recipe, show_original, split) = (self.recipe.clone(), self.show_original, self.view.split);
        let Some((lvl, s)) = self.level_for(z) else { return };
        let t = z / s; // screen pixels per level pixel
        let (cx, cy) = (center.0 * s, center.1 * s);
        let (vx0, vy0) = (cx - aw / (2.0 * t), cy - ah / (2.0 * t));
        let x0 = vx0.floor().max(0.0) as u32;
        let y0 = vy0.floor().max(0.0) as u32;
        let x1 = ((vx0 + aw / t).ceil().min(lvl.w as f32)).max(x0 as f32 + 1.0) as u32;
        let y1 = ((vy0 + ah / t).ceil().min(lvl.h as f32)).max(y0 as f32 + 1.0) as u32;
        let (cw, ch) = ((x1 - x0).min(lvl.w - x0), (y1 - y0).min(lvl.h - y0));
        let (lw, lh) = (lvl.w, lvl.h);
        // The neighbourhood filters need what is around the visible part: cut a little more, cut it off again afterwards.
        let m = nv_editor::spatial::margin(&recipe, &Frame { w: lw, h: lh, x0: 0, y0: 0 });
        let (ex0, ey0) = (x0.saturating_sub(m), y0.saturating_sub(m));
        let (ex1, ey1) = ((x0 + cw + m).min(lw), (y0 + ch + m).min(lh));
        let wide = cut_pixels(lvl, ex0, ey0, ex1 - ex0, ey1 - ey0);
        let started = Instant::now();
        let edited_wide = engine::apply_pixels(&wide, &recipe, Frame { w: lw, h: lh, x0: ex0 as i32, y0: ey0 as i32 });
        let (crop, edited) = if m == 0 { (wide, edited_wide) } else { (cut_pixels(&wide, x0 - ex0, y0 - ey0, cw, ch), cut_pixels(&edited_wide, x0 - ex0, y0 - ey0, cw, ch)) };
        self.edit_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.view.region = Some(([x0 as f32, y0 as f32, (x0 + cw) as f32, (y0 + ch) as f32], s));
        self.update_scope(&edited);
        let shown = if show_original { crop.clone() } else { edited };
        let shown = self.display_copy(shown);
        if let Some(r) = &mut self.renderer {
            let mut img = Image::new(shown.w, shown.h, shown.rgba);
            img.srgb_to_output();
            let _ = r.upload(ID_DETAIL, &img);
            if split {
                let orig = self.display_copy(crop);
                if let Some(r) = &mut self.renderer {
                    let mut img = Image::new(orig.w, orig.h, orig.rgba);
                    img.srgb_to_output();
                    let _ = r.upload(ID_DETAIL_ORIG, &img);
                }
            }
        }
        self.update_title();
    }

    /// False colour is a display transform: applied to what is shown, never to the picture or the scopes.
    pub(super) fn display_copy(&self, p: Pixels) -> Pixels {
        if self.view.false_colour { scopes::false_colour(&p) } else { p }
    }

    /// Redraw the scope bitmap from the pixels that are on screen.
    pub(super) fn update_scope(&mut self, shown: &Pixels) {
        if self.view.scope == Scope::Off {
            return;
        }
        let d = self.dpi;
        let (w, h) = if self.panel && !self.legacy_layout {
            // Docked at the top of the panel: as wide as the panel's graph box (272 x 134 logical pixels).
            match self.view.scope {
                Scope::Histogram => ((272.0 * d) as usize, (134.0 * d) as usize),
                Scope::Parade => ((272.0 * d) as usize, (134.0 * d) as usize),
                _ => ((134.0 * d) as usize, (134.0 * d) as usize),
            }
        } else {
            match self.view.scope {
                Scope::Histogram => ((280.0 * d) as usize, (110.0 * d) as usize),
                Scope::Parade => ((300.0 * d) as usize, (120.0 * d) as usize),
                _ => ((170.0 * d) as usize, (170.0 * d) as usize),
            }
        };
        let bmp = match self.view.scope {
            Scope::Histogram => {
                let hist = scopes::histogram(shown);
                self.view.clipped = hist.clipped();
                scopes::histogram_bitmap(&hist, w, h)
            }
            Scope::Parade => scopes::parade_bitmap(shown, w, h),
            _ => scopes::vectorscope_bitmap(shown, w),
        };
        if let Some(r) = &mut self.renderer {
            let img = Image::new(bmp.w, bmp.h, bmp.rgba);
            let _ = r.upload(ID_SCOPE, &img);
        }
    }

    // ------------------------------------------------------------------------------------------ input

    /// A press over the picture area. True if it was used (a pan or the split divider started).
    pub(super) fn press_image(&mut self, x: i32, y: i32) -> bool {
        if !contains(self.area(), x as f32, y as f32) || self.full.is_none() {
            return false;
        }
        if self.view.split {
            self.view.split_at = self.split_fraction(x);
            invalidate(self.hwnd);
            return true;
        }
        if self.view.zoom.is_some() {
            self.view.pan = Some(((x, y), self.view.center));
            return true;
        }
        false
    }

    fn split_fraction(&self, x: i32) -> f32 {
        let a = self.area();
        ((x as f32 - a[0]) / (a[2] - a[0]).max(1.0)).clamp(0.02, 0.98)
    }

    /// The pointer moved with the left button down over the picture: pan or move the divider.
    pub(super) fn drag_image(&mut self, x: i32, y: i32) -> bool {
        if self.view.split {
            self.view.split_at = self.split_fraction(x);
            invalidate(self.hwnd);
            return true;
        }
        if let (Some(((px, py), c0)), Some(z)) = (self.view.pan, self.view.zoom) {
            self.view.center = (c0.0 - (x - px) as f32 / z, c0.1 - (y - py) as f32 / z);
            self.clamp_center();
            self.view.detail_dirty = true;
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    /// Wheel over the picture area zooms around the pointer.
    pub(super) fn wheel_image(&mut self, delta: i32, x: i32, y: i32) -> bool {
        if !contains(self.area(), x as f32, y as f32) || self.full.is_none() {
            return false;
        }
        let factor = 1.25f32.powf(delta as f32 / 120.0);
        let z = self.zoom_now() * factor;
        self.set_zoom(z, (x as f32, y as f32));
        true
    }

    /// Double click on the picture: fitted <-> 100% at that point.
    pub(super) fn double_click_image(&mut self, x: i32, y: i32) -> bool {
        if !contains(self.area(), x as f32, y as f32) || self.full.is_none() {
            return false;
        }
        if self.view.zoom.is_some() {
            self.leave_detail();
        } else {
            self.set_zoom(1.0, (x as f32, y as f32));
        }
        true
    }

    /// Keys of the view: 1 / 2 / 4 = 100% / 200% / 400%, F or ` = fit, S split, Y scope, X false colour. True if used.
    pub(super) fn view_key(&mut self, vk: u32) -> bool {
        let a = self.area();
        let mid = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        match vk {
            0x31 => self.set_zoom(1.0, mid),
            0x32 => self.set_zoom(2.0, mid),
            0x34 => self.set_zoom(4.0, mid),
            0x46 | 0xC0 => {
                if self.view.zoom.is_some() {
                    self.leave_detail();
                }
            }
            0x53 => {
                self.view.split = !self.view.split;
                self.view.detail_dirty = self.view.zoom.is_some();
                invalidate(self.hwnd);
            }
            0x59 => {
                self.view.scope = self.view.scope.next();
                let name = self.view.scope.name();
                self.say(&format!("Scope: {name}"));
                self.refresh_view();
            }
            0x58 => {
                self.view.false_colour = !self.view.false_colour;
                self.say(if self.view.false_colour { "False colour: purple crushed, blue / teal shadows, green middle grey, pink skin, yellow / red clipping" } else { "False colour off" });
                self.refresh_view();
            }
            _ => return false,
        }
        true
    }

    /// Something about how the picture is shown changed (scope, false colour): render again.
    pub(super) fn refresh_view(&mut self) {
        if self.view.zoom.is_some() {
            self.view.detail_dirty = true;
        } else {
            self.rerender();
        }
        invalidate(self.hwnd);
    }

    pub(super) fn zoom_label(&self) -> String {
        match self.view.zoom {
            Some(z) => format!("{:.0}%", z * 100.0),
            None => format!("fit {:.0}%", self.fit_zoom() * 100.0),
        }
    }
}

/// The `w` x `h` part of `p` whose top-left corner is (`x0`, `y0`).
fn cut_pixels(p: &Pixels, x0: u32, y0: u32, w: u32, h: u32) -> Pixels {
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y0 + h {
        let start = (y as usize * p.w as usize + x0 as usize) * 4;
        rgba.extend_from_slice(&p.rgba[start..start + w as usize * 4]);
    }
    Pixels::new(w, h, rgba)
}
