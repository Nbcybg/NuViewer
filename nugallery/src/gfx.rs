//! Small drawing helpers on top of the renderer: colours, text (GDI bitmaps uploaded as overlay textures and cached
//! by content), ellipsising, boxes and outlines.

use nv_render::Renderer;
use nv_win::text;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

/// sRGB hex (0xRRGGBB) to linear RGBA for the sRGB render target.
pub fn rgb(hex: u32, a: f32) -> [f32; 4] {
    let ch = |shift: u32| {
        let c = ((hex >> shift) & 0xFF) as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    [ch(16), ch(8), ch(0), a]
}

/// The palette (sRGB hex).
pub mod pal {
    pub const BG: u32 = 0x1A1A1A;
    pub const PANEL: u32 = 0x202225;
    pub const PANEL_EDGE: u32 = 0x34373C;
    pub const TILE: u32 = 0x2A2C30;
    pub const TILE_HOVER: u32 = 0x34373C;
    pub const TEXT: u32 = 0xF0F0F0;
    pub const DIM: u32 = 0x9AA0A6;
    pub const ACCENT: u32 = 0x3D8BFF;
    pub const FIELD: u32 = 0x15171A;
    pub const ERROR: u32 = 0xFF7B7B;
    pub const FOLDER: u32 = 0xE8B84A;
    pub const STAR: u32 = 0xFFC83D;
}

/// Text rendering state: which cached bitmaps were used this frame (the rest are dropped at the end of the frame),
/// measured widths, and ellipsised names.
#[derive(Default)]
pub struct Text {
    used: HashSet<u64>,
    widths: HashMap<(String, i32), i32>,
    clipped: HashMap<(String, i32, i32), String>,
}

fn key_of(s: &str, px: i32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (s, px).hash(&mut h);
    h.finish()
}

const PAD: i32 = 1;

impl Text {
    pub fn begin_frame(&mut self) {
        self.used.clear();
        if self.widths.len() > 4000 {
            self.widths.clear();
        }
        if self.clipped.len() > 4000 {
            self.clipped.clear();
        }
    }

    pub fn end_frame(&mut self, r: &mut Renderer) {
        let used = &self.used;
        r.retain_overlays(|id| used.contains(&id));
    }

    /// Width in pixels of `s` at font size `px` (cached).
    pub fn width(&mut self, s: &str, px: i32) -> i32 {
        if let Some(&w) = self.widths.get(&(s.to_string(), px)) {
            return w;
        }
        let w = text::measure(s, px);
        self.widths.insert((s.to_string(), px), w);
        w
    }

    /// `s` shortened with an ellipsis in the middle-end so it fits `max_w` pixels (file names keep their extension).
    pub fn fit(&mut self, s: &str, px: i32, max_w: i32) -> String {
        if let Some(c) = self.clipped.get(&(s.to_string(), px, max_w)) {
            return c.clone();
        }
        let out = if self.width(s, px) <= max_w {
            s.to_string()
        } else {
            let chars: Vec<char> = s.chars().collect();
            // Keep the extension (".jpg") visible: cut from the middle of the stem.
            let ext_len = s.rfind('.').map(|d| s[d..].chars().count()).filter(|&n| n <= 6).unwrap_or(0);
            let tail: String = chars[chars.len() - ext_len..].iter().collect();
            let (mut lo, mut hi) = (0usize, chars.len() - ext_len);
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                let cand: String = chars[..mid].iter().collect::<String>() + "…" + &tail;
                if self.width(&cand, px) <= max_w {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            chars[..lo].iter().collect::<String>() + "…" + &tail
        };
        self.clipped.insert((s.to_string(), px, max_w), out.clone());
        out
    }

    /// Draw `s` with its top-left at (x, y); returns the width drawn. Empty strings draw nothing.
    pub fn draw(&mut self, r: &mut Renderer, s: &str, px: i32, x: f32, y: f32, color: [f32; 4]) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let key = key_of(s, px);
        if !r.has_overlay(key) {
            let bmp = text::render_lines(&[s.to_string()], px, PAD);
            if r.upload_overlay(key, &nv_codecs::Image::new(bmp.width, bmp.height, bmp.rgba)).is_err() {
                return 0.0;
            }
        }
        self.used.insert(key);
        let w = r.overlay_size(key).map(|(w, _)| w as f32).unwrap_or(0.0);
        let _ = r.draw_overlay(key, x - PAD as f32, y - PAD as f32, color);
        w - 2.0 * PAD as f32
    }

    /// Draw `s` so it ends at `right`.
    pub fn draw_right(&mut self, r: &mut Renderer, s: &str, px: i32, right: f32, y: f32, color: [f32; 4]) {
        let w = self.width(s, px) as f32;
        self.draw(r, s, px, right - w, y, color);
    }

    /// Draw `s` centred on `cx`.
    pub fn draw_centered(&mut self, r: &mut Renderer, s: &str, px: i32, cx: f32, y: f32, color: [f32; 4]) {
        let w = self.width(s, px) as f32;
        self.draw(r, s, px, cx - w / 2.0, y, color);
    }
}

/// Line height used for text at `px` (matches `nv_win::text`).
pub fn line_h(px: i32) -> f32 {
    (px as f32 * 1.35).round()
}

/// Draw the shared menu bar (see `nv_win::menubar`) along the top edge, if it is showing. Call it last, so it covers what is under it.
pub fn draw_menubar(r: &mut Renderer, text: &mut Text, bar: &mut nv_win::menubar::MenuBar, w: f32, d: f32) {
    let px = (13.0 * d).round() as i32;
    bar.layout(w, d, |s| text.width(s, px) as f32);
    if !bar.visible() {
        return;
    }
    let strip = bar.strip(w, d);
    let _ = r.fill_rect(strip, rgb(0x15171A, 0.97));
    let _ = r.fill_rect([0.0, strip[3] - 1.0, w, strip[3]], rgb(pal::PANEL_EDGE, 1.0));
    let ty = (strip[3] - line_h(px)) / 2.0;
    let labels = bar.labels();
    for (i, (rect, label)) in bar.rects().iter().zip(labels.iter()).enumerate() {
        if bar.hot() == Some(i) {
            let _ = r.fill_rect([rect[0], rect[1] + 2.0 * d, rect[2], rect[3] - 1.0], rgb(if bar.keyboard() { pal::ACCENT } else { pal::TILE_HOVER }, 1.0));
        }
        let x = rect[0] + (rect[2] - rect[0] - text.width(label, px) as f32) / 2.0;
        text.draw(r, label, px, x, ty, rgb(pal::TEXT, 1.0));
        if bar.keyboard() {
            if let Some(k) = bar.mnemonic_of(i) {
                let before: String = label.chars().take(k).collect();
                let ch: String = label.chars().skip(k).take(1).collect();
                let ux = x + text.width(&before, px) as f32;
                let uw = text.width(&ch, px) as f32;
                let _ = r.fill_rect([ux, ty + line_h(px) - 3.0 * d, ux + uw, ty + line_h(px) - 2.0 * d], rgb(pal::TEXT, 1.0));
            }
        }
    }
}

/// An outline `t` pixels thick inside `r`.
pub fn outline(r: &mut Renderer, rect: [f32; 4], t: f32, color: [f32; 4]) {
    let [l, tp, rt, b] = rect;
    let _ = r.fill_rect([l, tp, rt, tp + t], color);
    let _ = r.fill_rect([l, b - t, rt, b], color);
    let _ = r.fill_rect([l, tp + t, l + t, b - t], color);
    let _ = r.fill_rect([rt - t, tp + t, rt, b - t], color);
}

pub fn contains(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_linearised() {
        assert_eq!(rgb(0xFFFFFF, 1.0), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(rgb(0x000000, 0.5), [0.0, 0.0, 0.0, 0.5]);
        let mid = rgb(0x808080, 1.0)[0];
        assert!((mid - 0.2158).abs() < 0.001, "{mid}");
    }

    #[test]
    fn rectangles_contain_their_top_left_but_not_their_bottom_right() {
        let r = [10.0, 10.0, 20.0, 20.0];
        assert!(contains(r, 10.0, 10.0) && contains(r, 19.9, 19.9));
        assert!(!contains(r, 20.0, 15.0) && !contains(r, 15.0, 20.0) && !contains(r, 9.9, 15.0));
    }
}
