//! The colour picker (K) and the palette bar (Ctrl+K): on demand only. Nothing is decoded, kept or drawn until one of them is switched
//! on; then the picture is read once more on a worker thread (upright, colour managed: the numbers the eye gets), its palette is
//! worked out there, and both are dropped again when the tool is switched off. While on: the pointer shows the colour under it, a
//! click copies it to the clipboard in the format chosen in Settings (Shift+click: the other common one), a click on a palette
//! swatch copies that swatch.

use super::*;
use nv_core::colours::{self, ColourFormat, Rgb, Swatch};
use std::sync::mpsc;

pub type Loaded = colours::PickData;

#[derive(Default)]
pub struct Pick {
    pub on: bool,
    pub palette_on: bool,
    pub loaded: Option<Loaded>,
    loading: Option<(u64, mpsc::Receiver<Option<Loaded>>)>,
    /// The palette swatches as drawn in the last frame (for clicks).
    pub bar: Vec<([f32; 4], Rgb)>,
}

impl App {
    pub fn toggle_pick(&mut self) {
        if self.pick.on {
            self.pick.on = false;
            self.pick_release();
        } else if self.pick_possible() {
            self.pick.on = true;
            self.toast("Pick a colour: click a pixel to copy its value · Shift+click the other format · Esc or K stops");
        } else {
            self.toast("Colours can be picked from a still picture (not a model, a 360 view, a turned picture or a spread)");
        }
        invalidate(self.hwnd);
    }

    pub fn toggle_palette(&mut self) {
        if self.pick.palette_on {
            self.pick.palette_on = false;
            self.pick_release();
        } else if self.pick_possible() {
            self.pick.palette_on = true;
        } else {
            self.toast("A palette is made from a still picture (not a model, a 360 view or a spread)");
        }
        invalidate(self.hwnd);
    }

    fn pick_release(&mut self) {
        if !self.pick.on && !self.pick.palette_on {
            self.pick.loaded = None;
            self.pick.loading = None;
            self.pick.bar.clear();
        }
    }

    /// A still picture is on screen in the plain viewer.
    fn pick_possible(&self) -> bool {
        self.mode == Mode::Viewer && !self.folder.files.is_empty() && !self.is_model() && !self.pano_active() && self.rotation() == 0.0 && self.spread_view().is_none() && !self.media_current()
    }

    /// Start reading the current picture when the tool is on and it is not the one held.
    fn pick_tick(&mut self) {
        let Some(i) = self.target() else { return };
        let id = self.id_at(i);
        if self.pick.loaded.as_ref().is_some_and(|l| l.id == id) || self.pick.loading.as_ref().is_some_and(|(lid, _)| *lid == id) {
            return;
        }
        self.pick.loaded = None;
        let path = self.folder.files[i].clone();
        let n = self.settings.more.palette_size as usize;
        let (tx, rx) = mpsc::channel();
        let wake = self.wake.clone();
        self.pick.loading = Some((id, rx));
        std::thread::spawn(move || {
            let loaded = Loaded::load(&path, id, n);
            let _ = tx.send(loaded);
            wake();
        });
    }

    /// Take a finished read (called with the other background work).
    pub fn pick_poll(&mut self) {
        let Some((id, rx)) = &self.pick.loading else { return };
        let id = *id;
        match rx.try_recv() {
            Ok(loaded) => {
                self.pick.loading = None;
                if (self.pick.on || self.pick.palette_on) && self.target().map(|i| self.id_at(i)) == Some(id) {
                    match loaded {
                        Some(l) => self.pick.loaded = Some(l),
                        None => self.toast("Could not read this picture for its colours"),
                    }
                    invalidate(self.hwnd);
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pick.loading = None,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }

    /// The colour of the picture under window point (x, y).
    pub fn pick_sample(&self, x: i32, y: i32) -> Option<Rgb> {
        let l = self.pick.loaded.as_ref()?;
        if !self.pick_possible() || self.target().map(|i| self.id_at(i)) != Some(l.id) {
            return None;
        }
        let win = self.renderer.size();
        let vs = self.view_size()?;
        let d = self.view.dst_rect(vs, win);
        l.at((x as f32 - d[0]) / (d[2] - d[0]), (y as f32 - d[1]) / (d[3] - d[1]))
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
    pub fn pick_press(&mut self, x: i32, y: i32) -> bool {
        if self.pick.palette_on {
            let hit = self.pick.bar.iter().find(|(r, _)| (x as f32) >= r[0] && (x as f32) < r[2] && (y as f32) >= r[1] && (y as f32) < r[3]).map(|(_, c)| *c);
            if let Some(c) = hit {
                self.copy_colour(c);
                return true;
            }
        }
        if self.pick.on {
            if let Some(c) = self.pick_sample(x, y) {
                self.copy_colour(c);
            }
            return true;
        }
        false
    }

    /// Escape: stop the colour tools first. True if one was on.
    pub fn pick_escape(&mut self) -> bool {
        if self.pick.on || self.pick.palette_on {
            self.pick.on = false;
            self.pick.palette_on = false;
            self.pick_release();
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    pub fn draw_pick(&mut self) {
        if !self.pick.on && !self.pick.palette_on {
            return;
        }
        if !self.pick_possible() {
            self.pick.on = false;
            self.pick.palette_on = false;
            self.pick_release();
            return;
        }
        self.pick_tick();
        let d = self.gallery.dpi;
        let (w, h) = (self.renderer.size().0 as f32, self.renderer.size().1 as f32);
        let (text_c, back_c) = (ui::text(1.0), ui::panel(0.88));
        let rgb_hex = |c: Rgb| ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32;
        if self.pick.on {
            let (mx, my) = self.last_mouse;
            let sample = self.pick_sample(mx, my);
            let msg = match (&sample, self.pick.loaded.is_some()) {
                (Some(c), _) => colours::readout(*c),
                (None, false) => "Reading the picture…".to_string(),
                (None, true) => "Click a pixel · Shift+click: other format · Esc stops".to_string(),
            };
            if let Some((key, tw, th)) = self.overlay(&[msg]) {
                let sw = if sample.is_some() { th } else { 0.0 };
                let (bw, bh) = (tw + sw, th);
                let (mut x, mut y) = (mx as f32 + 20.0 * d, my as f32 + 20.0 * d);
                if x + bw > w {
                    x = mx as f32 - 20.0 * d - bw;
                }
                if y + bh > h {
                    y = my as f32 - 20.0 * d - bh;
                }
                let _ = self.renderer.fill_rect([x, y, x + bw, y + bh], back_c);
                if let Some(c) = sample {
                    let _ = self.renderer.fill_rect([x + 3.0 * d, y + 3.0 * d, x + sw - 3.0 * d, y + bh - 3.0 * d], srgb(rgb_hex(c), 1.0));
                }
                let _ = self.renderer.draw_overlay(key, x + sw, y, text_c);
            }
            // A small square around the pixel under the pointer.
            if sample.is_some() {
                let r = 7.0 * d;
                let (cx, cy) = (mx as f32, my as f32);
                for (c, o) in [(srgb(0x000000, 0.9), 0.0), (srgb(0xFFFFFF, 0.95), 1.0 * d)] {
                    let (a, b) = (r - o, r - o);
                    let t = 1.0 * d;
                    let _ = self.renderer.fill_rect([cx - a, cy - b, cx + a, cy - b + t], c);
                    let _ = self.renderer.fill_rect([cx - a, cy + b - t, cx + a, cy + b], c);
                    let _ = self.renderer.fill_rect([cx - a, cy - b, cx - a + t, cy + b], c);
                    let _ = self.renderer.fill_rect([cx + a - t, cy - b, cx + a, cy + b], c);
                }
            }
        }
        self.pick.bar.clear();
        if self.pick.palette_on {
            let swatches: Vec<Swatch> = self.pick.loaded.as_ref().map(|l| l.palette.clone()).unwrap_or_default();
            if swatches.is_empty() {
                if let Some((key, tw, th)) = self.overlay(&[if self.pick.loaded.is_some() { "No colours found".to_string() } else { "Reading the picture…".to_string() }]) {
                    let (x, y) = ((w - tw) / 2.0, h - th - 40.0 * d);
                    let _ = self.renderer.fill_rect([x, y, x + tw, y + th], back_c);
                    let _ = self.renderer.draw_overlay(key, x, y, text_c);
                }
            } else {
                let (sw, gap, label_h) = (64.0 * d, 8.0 * d, 24.0 * d);
                let total = swatches.len() as f32 * (sw + gap) - gap;
                let (x0, y0) = ((w - total) / 2.0, h - sw - label_h - 36.0 * d);
                let _ = self.renderer.fill_rect([x0 - 10.0 * d, y0 - 10.0 * d, x0 + total + 10.0 * d, y0 + sw + label_h + 8.0 * d], back_c);
                for (i, s) in swatches.iter().enumerate() {
                    let x = x0 + i as f32 * (sw + gap);
                    let _ = self.renderer.fill_rect([x, y0, x + sw, y0 + sw], srgb(rgb_hex(s.rgb), 1.0));
                    let label = colours::format(s.rgb, ColourFormat::Hex);
                    if let Some((key, tw, _)) = self.overlay_small(&[label]) {
                        let _ = self.renderer.draw_overlay(key, x + (sw - tw) / 2.0, y0 + sw, text_c);
                    }
                    // The share of the picture: a thin bar under the swatch.
                    let share_w = (sw * (s.share * 2.0).min(1.0)).max(2.0);
                    let _ = self.renderer.fill_rect([x, y0 + sw + label_h - 3.0 * d, x + share_w, y0 + sw + label_h], srgb(0xFFFFFF, 0.55));
                    self.pick.bar.push(([x, y0, x + sw, y0 + sw + label_h], s.rgb));
                }
            }
        }
    }
}
