//! The colour picker (K) and the palette (Ctrl+K) of the editor: on demand only. Nothing is kept or worked out until one is switched
//! on; then every finished render also keeps a copy of the edited picture (a window-sized proxy, a few MB) and, for the palette,
//! works out the dominant colours from it (a few milliseconds). Both read what is on screen, edits included. A click copies the
//! colour (Settings: the format; Shift+click: the other common one); a click on a palette swatch copies that swatch.

use super::*;
use nv_core::colours::{self, ColourFormat, Rgb, Swatch};

#[derive(Default)]
pub struct ColourTool {
    pub on: bool,
    pub palette_on: bool,
    /// The edited proxy as last rendered (only kept while a tool is on).
    pub edited: Option<Arc<Pixels>>,
    pub palette: Vec<Swatch>,
    pub bar: Vec<([f32; 4], Rgb)>,
    pub cursor: Option<(i32, i32)>,
}

fn size(base: f32, d: f32) -> i32 {
    (base * d).round() as i32
}

impl App {
    pub(super) fn colour_wanted(&self) -> bool {
        self.colour.on || self.colour.palette_on
    }

    fn colour_release(&mut self) {
        if !self.colour_wanted() {
            self.colour.edited = None;
            self.colour.palette.clear();
            self.colour.bar.clear();
        }
    }

    pub(super) fn toggle_colour(&mut self) {
        if self.colour.on {
            self.colour.on = false;
            self.colour_release();
            self.say("Colour picker off");
        } else if self.full.is_some() {
            self.colour.on = true;
            self.say("Pick a colour: click the picture to copy its value · Shift+click the other format · K or Esc stops");
            self.rerender(); // fills the edited copy
        }
        invalidate(self.hwnd);
    }

    pub(super) fn toggle_palette(&mut self) {
        if self.colour.palette_on {
            self.colour.palette_on = false;
            self.colour_release();
        } else if self.full.is_some() {
            self.colour.palette_on = true;
            self.rerender();
        }
        invalidate(self.hwnd);
    }

    /// Esc: stop the colour tools. True if one was on.
    pub(super) fn colour_escape(&mut self) -> bool {
        if self.colour_wanted() {
            self.colour.on = false;
            self.colour.palette_on = false;
            self.colour_release();
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    /// A render finished: keep the copy and make the palette while a tool is on.
    pub(super) fn colour_after_render(&mut self, out: &Pixels) {
        if !self.colour_wanted() {
            return;
        }
        if self.colour.palette_on {
            self.colour.palette = colours::palette(&out.rgba, out.w as usize, out.h as usize, self.settings.more.palette_size as usize);
        }
        self.colour.edited = Some(Arc::new(Pixels::new(out.w, out.h, out.rgba.clone())));
    }

    /// The colour of the picture under window point (x, y), as it is edited.
    fn colour_sample(&self, x: f32, y: f32) -> Option<Rgb> {
        let e = self.colour.edited.as_ref()?;
        if !contains(self.area(), x, y) {
            return None;
        }
        let (nx, ny) = self.screen_to_display(x, y)?;
        if !(0.0..1.0).contains(&nx) || !(0.0..1.0).contains(&ny) {
            return None;
        }
        let (px, py) = (((nx * e.w as f32) as usize).min(e.w as usize - 1), ((ny * e.h as f32) as usize).min(e.h as usize - 1));
        let o = (py * e.w as usize + px) * 4;
        e.rgba.get(o..o + 3).map(|p| Rgb(p[0], p[1], p[2]))
    }

    fn colour_copy(&mut self, c: Rgb) {
        let main = ColourFormat::from_id(&self.settings.more.pick_format).unwrap_or(ColourFormat::Hex);
        let f = if nv_win::modifiers().shift { if main == ColourFormat::Hex { ColourFormat::Rgb } else { ColourFormat::Hex } } else { main };
        let text = colours::format(c, f);
        match nv_win::shell::clipboard_copy_text(self.hwnd, &text) {
            Ok(()) => self.say(&format!("Copied {text}")),
            Err(e) => self.say(&format!("Couldn't copy the colour: {e}")),
        }
    }

    /// A left press with a colour tool on. True if it was used.
    pub(super) fn colour_press(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        if self.colour.palette_on {
            let hit = self.colour.bar.iter().find(|(r, _)| fx >= r[0] && fx < r[2] && fy >= r[1] && fy < r[3]).map(|(_, c)| *c);
            if let Some(c) = hit {
                self.colour_copy(c);
                return true;
            }
        }
        if self.colour.on && contains(self.area(), fx, fy) {
            if let Some(c) = self.colour_sample(fx, fy) {
                self.colour_copy(c);
            }
            return true;
        }
        false
    }

    pub(super) fn colour_move(&mut self, x: i32, y: i32) {
        if self.colour.on {
            self.colour.cursor = contains(self.area(), x as f32, y as f32).then_some((x, y));
            invalidate(self.hwnd);
        }
    }

    /// What the read-out and the palette bar need for this frame (worked out before the renderer is borrowed).
    pub(super) fn colour_hud(&self) -> Option<Hud> {
        if !self.colour_wanted() {
            return None;
        }
        Some(Hud {
            sample: self.colour.cursor.and_then(|(x, y)| if self.colour.on { self.colour_sample(x as f32, y as f32) } else { None }),
            cursor: self.colour.cursor,
            swatches: if self.colour.palette_on { self.colour.palette.clone() } else { Vec::new() },
            palette_on: self.colour.palette_on,
            area: self.area(),
            size: (self.size.0 as f32, self.size.1 as f32),
        })
    }
}

pub struct Hud {
    sample: Option<Rgb>,
    cursor: Option<(i32, i32)>,
    swatches: Vec<Swatch>,
    palette_on: bool,
    area: [f32; 4],
    size: (f32, f32),
}

/// The pointer read-out and the palette bar. Fills `bar` with the swatch rectangles (for clicks).
pub fn draw(r: &mut Renderer, text: &mut Text, hud: &Hud, bar: &mut Vec<([f32; 4], Rgb)>, d: f32) {
    let (w, h) = hud.size;
    let area = hud.area;
    let (font, small) = (size(14.0, d), size(12.0, d));
    bar.clear();
    let hex = |c: Rgb| ((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32;
    if let (Some(c), Some((mx, my))) = (hud.sample, hud.cursor) {
        let msg = colours::readout(c);
        let tw = text.width(&msg, font) as f32 + 20.0 * d;
        let bh = 28.0 * d;
        let (mut x, mut y) = (mx as f32 + 18.0 * d, my as f32 + 18.0 * d);
        if x + tw + bh > w {
            x = mx as f32 - 18.0 * d - tw - bh;
        }
        if y + bh > h {
            y = my as f32 - 18.0 * d - bh;
        }
        let _ = r.fill_rect([x, y, x + tw + bh, y + bh], rgb(0x0E0E0E, 0.93));
        let _ = r.fill_rect([x + 3.0 * d, y + 3.0 * d, x + bh - 3.0 * d, y + bh - 3.0 * d], rgb(hex(c), 1.0));
        text.draw(r, &msg, font, x + bh + 10.0 * d, y + 5.0 * d, rgb(0xF2F2F2, 1.0));
    }
    if hud.palette_on {
        if hud.swatches.is_empty() {
            let t = "Working out the palette…";
            let tw = text.width(t, small) as f32 + 24.0 * d;
            let (x, y) = ((area[0] + area[2] - tw) / 2.0, area[3] - 70.0 * d);
            let _ = r.fill_rect([x, y, x + tw, y + 26.0 * d], rgb(0x0E0E0E, 0.93));
            text.draw(r, t, small, x + 12.0 * d, y + 5.0 * d, rgb(0xF2F2F2, 1.0));
        } else {
            let (sw, gap, label_h) = (60.0 * d, 8.0 * d, 22.0 * d);
            let total = hud.swatches.len() as f32 * (sw + gap) - gap;
            let (x0, y0) = ((area[0] + area[2] - total) / 2.0, area[3] - sw - label_h - 60.0 * d);
            let _ = r.fill_rect([x0 - 10.0 * d, y0 - 10.0 * d, x0 + total + 10.0 * d, y0 + sw + label_h + 8.0 * d], rgb(0x0E0E0E, 0.93));
            for (i, s) in hud.swatches.iter().enumerate() {
                let x = x0 + i as f32 * (sw + gap);
                let _ = r.fill_rect([x, y0, x + sw, y0 + sw], rgb(hex(s.rgb), 1.0));
                let label = colours::format(s.rgb, ColourFormat::Hex);
                let tw = text.width(&label, small) as f32;
                text.draw(r, &label, small, x + (sw - tw) / 2.0, y0 + sw + 3.0 * d, rgb(0xF2F2F2, 1.0));
                let share_w = (sw * (s.share * 2.0).min(1.0)).max(2.0);
                let _ = r.fill_rect([x, y0 + sw + label_h - 3.0 * d, x + share_w, y0 + sw + label_h], rgb(0xFFFFFF, 0.55));
                bar.push(([x, y0, x + sw, y0 + sw + label_h], s.rgb));
            }
        }
    }
}
