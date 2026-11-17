//! The colour picker (K) and the palette (Ctrl+K) on the preview picture of the right panel: on demand only. Nothing is read until one
//! is switched on; then the focused picture is read once on a worker thread (see `nv_core::colours::PickData`) and dropped again
//! when the tool is switched off. While on, the pointer over the preview shows the colour under it, a click copies it (Shift+click: the
//! other common format), a click on a palette swatch copies that swatch.

use crate::app::{App, Msg};
use crate::draw::px;
use crate::gfx::{line_h, rgb};
use nv_core::colours::{self, ColourFormat, PickData, Rgb};

#[derive(Default)]
pub struct Pick {
    pub on: bool,
    pub palette_on: bool,
    pub loaded: Option<PickData>,
    /// The item being read.
    loading: Option<u64>,
    /// The palette swatches drawn in the last frame (for clicks).
    pub bar: Vec<([f32; 4], Rgb)>,
}

impl App {
    fn pick_item(&self) -> Option<(u64, std::path::PathBuf)> {
        let id = self.pv.id?;
        let e = self.listing.iter().find(|e| e.id == id)?;
        (e.has_thumb() && e.kind != crate::model::Kind::Model && !e.is_dir()).then(|| (id, e.path.clone()))
    }

    pub fn toggle_pick(&mut self) {
        if self.pick.on {
            self.pick.on = false;
            self.pick_release();
        } else if self.pick_item().is_some() && self.chrome.right.is_some() {
            self.pick.on = true;
            self.toast("Pick a colour: click the preview picture to copy a pixel's value · Esc or K stops");
        } else {
            self.toast("Open the preview panel on a picture first (the colour is picked from the preview)");
        }
        nv_win::invalidate(self.hwnd);
    }

    pub fn toggle_palette(&mut self) {
        if self.pick.palette_on {
            self.pick.palette_on = false;
            self.pick_release();
        } else if self.pick_item().is_some() {
            self.pick.palette_on = true;
        } else {
            self.toast("Select a picture first");
        }
        nv_win::invalidate(self.hwnd);
    }

    fn pick_release(&mut self) {
        if !self.pick.on && !self.pick.palette_on {
            self.pick.loaded = None;
            self.pick.loading = None;
            self.pick.bar.clear();
        }
    }

    /// Escape stops the colour tools first. True if one was on.
    pub fn pick_escape(&mut self) -> bool {
        if self.pick.on || self.pick.palette_on {
            self.pick.on = false;
            self.pick.palette_on = false;
            self.pick_release();
            nv_win::invalidate(self.hwnd);
            return true;
        }
        false
    }

    fn pick_tick(&mut self) {
        let Some((id, path)) = self.pick_item() else { return };
        if self.pick.loaded.as_ref().is_some_and(|l| l.id == id) || self.pick.loading == Some(id) {
            return;
        }
        self.pick.loaded = None;
        self.pick.loading = Some(id);
        let (tx, wake, n) = (self.tx.clone(), self.wake.clone(), self.settings.more.palette_size as usize);
        std::thread::spawn(move || {
            let _ = tx.send(Msg::PickLoaded { id, data: Box::new(PickData::load(&path, id, n)) });
            wake();
        });
    }

    pub fn pick_loaded(&mut self, id: u64, data: Option<PickData>) {
        if self.pick.loading == Some(id) {
            self.pick.loading = None;
        }
        if (self.pick.on || self.pick.palette_on) && self.pv.id == Some(id) {
            match data {
                Some(d) => self.pick.loaded = Some(d),
                None => self.toast("Could not read this picture for its colours"),
            }
            nv_win::invalidate(self.hwnd);
        }
    }

    /// The colour of the preview picture under window point (x, y).
    pub fn pick_sample(&self, x: f32, y: f32) -> Option<Rgb> {
        let l = self.pick.loaded.as_ref()?;
        if self.pv.id != Some(l.id) {
            return None;
        }
        let (rect, vis) = self.preview_visible()?;
        if x < vis[0] || x >= vis[2] || y < vis[1] || y >= vis[3] {
            return None;
        }
        let p = crate::preview::picture_rect(rect, l.img.width, l.img.height, self.pv.zoom, self.pv.pan);
        l.at((x - p[0]) / (p[2] - p[0]), (y - p[1]) / (p[3] - p[1]))
    }

    fn copy_colour(&mut self, c: Rgb) {
        let main = ColourFormat::from_id(&self.settings.more.pick_format).unwrap_or(ColourFormat::Hex);
        let f = if nv_win::modifiers().shift { if main == ColourFormat::Hex { ColourFormat::Rgb } else { ColourFormat::Hex } } else { main };
        let text = colours::format(c, f);
        match nv_win::shell::clipboard_copy_text(self.hwnd, &text) {
            Ok(()) => self.toast(format!("Copied {text}")),
            Err(e) => self.toast(format!("Couldn't copy the colour: {e}")),
        }
    }

    /// A left press while a colour tool is on. True if it was used.
    pub fn pick_press(&mut self, x: f32, y: f32) -> bool {
        if self.pick.palette_on {
            let hit = self.pick.bar.iter().find(|(r, _)| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]).map(|(_, c)| *c);
            if let Some(c) = hit {
                self.copy_colour(c);
                return true;
            }
        }
        if self.pick.on {
            if let Some(c) = self.pick_sample(x, y) {
                self.copy_colour(c);
                return true;
            }
        }
        false
    }

    /// The pointer read-out and the palette bar, drawn over everything but the menu bar.
    pub fn draw_pick(&mut self, w: f32, h: f32, d: f32) {
        if !self.pick.on && !self.pick.palette_on {
            return;
        }
        if self.pick_item().is_none() {
            self.pick.on = false;
            self.pick.palette_on = false;
            self.pick_release();
            return;
        }
        self.pick_tick();
        let (mx, my) = self.last_mouse;
        let sample = if self.pick.on { self.pick_sample(mx as f32, my as f32) } else { None };
        let hint = if self.pick.on && sample.is_none() { Some(if self.pick.loaded.is_some() { "Move over the preview picture · click copies the colour".to_string() } else { "Reading the picture…".to_string() }) } else { None };
        let swatches = if self.pick.palette_on { self.pick.loaded.as_ref().map(|l| l.palette.clone()).unwrap_or_default() } else { Vec::new() };
        let reading = self.pick.palette_on && self.pick.loaded.is_none();
        let small = px(13.0, d);
        let font = px(15.0, d);
        let mut bar: Vec<([f32; 4], Rgb)> = Vec::new();
        let Some(r) = self.renderer.as_mut() else { return };
        let hex = |c: Rgb| ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32;
        if self.pick.on {
            let msg = match (&sample, &hint) {
                (Some(c), _) => colours::readout(*c),
                (None, Some(h)) => h.clone(),
                _ => String::new(),
            };
            if !msg.is_empty() {
                let tw = self.text.width(&msg, font) as f32 + 20.0 * d;
                let bh = line_h(font) + 10.0 * d;
                let sw = if sample.is_some() { bh } else { 0.0 };
                let (mut x, mut y) = (mx as f32 + 18.0 * d, my as f32 + 18.0 * d);
                if x + tw + sw > w {
                    x = mx as f32 - 18.0 * d - tw - sw;
                }
                if y + bh > h {
                    y = my as f32 - 18.0 * d - bh;
                }
                let _ = r.fill_rect([x, y, x + tw + sw, y + bh], rgb(0x0E0E0E, 0.93));
                if let Some(c) = sample {
                    let _ = r.fill_rect([x + 3.0 * d, y + 3.0 * d, x + sw - 3.0 * d, y + bh - 3.0 * d], rgb(hex(c), 1.0));
                }
                self.text.draw(r, &msg, font, x + sw + 10.0 * d, y + 5.0 * d, rgb(0xF2F2F2, 1.0));
            }
        }
        if self.pick.palette_on {
            if swatches.is_empty() {
                let t = if reading { "Reading the picture…" } else { "No colours found" };
                let tw = self.text.width(t, small) as f32 + 24.0 * d;
                let bh = line_h(small) + 12.0 * d;
                let (x, y) = ((w - tw) / 2.0, h - bh - 70.0 * d);
                let _ = r.fill_rect([x, y, x + tw, y + bh], rgb(0x0E0E0E, 0.93));
                self.text.draw(r, t, small, x + 12.0 * d, y + 6.0 * d, rgb(0xF2F2F2, 1.0));
            } else {
                let (sw, gap, label_h) = (60.0 * d, 8.0 * d, line_h(small) + 6.0 * d);
                let total = swatches.len() as f32 * (sw + gap) - gap;
                let (x0, y0) = ((w - total) / 2.0, h - sw - label_h - 64.0 * d);
                let _ = r.fill_rect([x0 - 10.0 * d, y0 - 10.0 * d, x0 + total + 10.0 * d, y0 + sw + label_h + 8.0 * d], rgb(0x0E0E0E, 0.93));
                for (i, s) in swatches.iter().enumerate() {
                    let x = x0 + i as f32 * (sw + gap);
                    let _ = r.fill_rect([x, y0, x + sw, y0 + sw], rgb(hex(s.rgb), 1.0));
                    let label = colours::format(s.rgb, ColourFormat::Hex);
                    let tw = self.text.width(&label, small) as f32;
                    self.text.draw(r, &label, small, x + (sw - tw) / 2.0, y0 + sw + 3.0 * d, rgb(0xF2F2F2, 1.0));
                    let share_w = (sw * (s.share * 2.0).min(1.0)).max(2.0);
                    let _ = r.fill_rect([x, y0 + sw + label_h - 3.0 * d, x + share_w, y0 + sw + label_h], rgb(0xFFFFFF, 0.55));
                    bar.push(([x, y0, x + sw, y0 + sw + label_h], s.rgb));
                }
            }
        }
        self.pick.bar = bar;
    }
}
