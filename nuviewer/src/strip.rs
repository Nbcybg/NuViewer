//! The film strip: a vertical column of thumbnails on the right edge, to see what else the folder holds, jump to any
//! picture with a click, and change how the folder is ordered.
//!
//! It opens while the pointer touches the right edge of the window and closes when the pointer leaves it (`L` keeps it
//! open, Settings can switch the edge trigger off). The strip shows the same list that next / previous walk, in the same
//! order, so what it shows is exactly what the arrow keys will visit. Its header changes that order (name, date, size,
//! type, ascending / descending, shuffle); the gallery, the slideshow and next / previous follow.
//!
//! Thumbnails come from the gallery's machinery: the same worker pool, the same disk cache, the same GPU thumbnail
//! textures. Only the rows on screen (plus a few) are requested.

use super::*;
use nv_core::folder::SortKey;

/// Logical (dip) sizes.
const BAR_W: f32 = 176.0;
const PAD: f32 = 12.0;
const CELL: f32 = 152.0;
const GAP: f32 = 6.0;
const CHIP_H: f32 = 24.0;
const CHIP_GAP: f32 = 4.0;
/// Rows requested beyond the visible ones.
const MARGIN_ROWS: usize = 3;
/// How close to the right edge the pointer has to be to open the bar.
const EDGE_PX: f32 = 6.0;

/// What the pointer can be over inside the strip.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Key(SortKey),
    Direction,
    Shuffle,
    Item(usize),
}

pub struct Strip {
    /// Kept open with the `L` key.
    pub pinned: bool,
    /// Open because the pointer touched the right edge (and has not left the bar since).
    pub hover: bool,
    /// How far the list is scrolled, in pixels.
    pub scroll: f32,
    pub hot: Option<Hit>,
    /// The (first, last) rows last asked for, so a redraw does not ask again. `None` = ask again.
    pub requested: Option<(usize, usize)>,
}

impl Strip {
    pub fn new() -> Strip {
        Strip { pinned: false, hover: false, scroll: 0.0, hot: None, requested: None }
    }
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

/// Where everything is, in client pixels.
pub struct StripLayout {
    pub bar: [f32; 4],
    pub chips: Vec<(Hit, [f32; 4])>,
    /// Bottom of the header; the list starts a little below it.
    pub header_h: f32,
    pub list_top: f32,
    pub cell: f32,
    pub pitch: f32,
    pub x0: f32,
}

impl App {
    pub fn strip_layout(&self) -> StripLayout {
        let (w, _) = self.renderer.size();
        let d = self.gallery.dpi;
        let bar_w = BAR_W * d;
        let x_left = w as f32 - bar_w;
        let (pad, gap) = (PAD * d, CHIP_GAP * d);
        let chip_h = CHIP_H * d;
        let inner = bar_w - 2.0 * pad;
        let mut chips = Vec::new();
        // Row 1: the four sort keys.
        let kw = (inner - 3.0 * gap) / 4.0;
        let y1 = pad * 0.6;
        for (i, k) in SortKey::ALL.iter().enumerate() {
            let x = x_left + pad + i as f32 * (kw + gap);
            chips.push((Hit::Key(*k), [x, y1, x + kw, y1 + chip_h]));
        }
        // Row 2: direction and shuffle.
        let y2 = y1 + chip_h + gap;
        let dw = inner * 0.42;
        chips.push((Hit::Direction, [x_left + pad, y2, x_left + pad + dw, y2 + chip_h]));
        chips.push((Hit::Shuffle, [x_left + pad + dw + gap, y2, x_left + pad + inner, y2 + chip_h]));
        let header_h = y2 + chip_h + pad * 0.6;
        StripLayout { bar: [x_left, 0.0, w as f32, self.renderer.size().1 as f32], chips, header_h, list_top: header_h + GAP * d, cell: CELL * d, pitch: (CELL + GAP) * d, x0: x_left + pad }
    }

    pub fn strip_visible(&self) -> bool {
        self.mode == Mode::Viewer && !self.editor.open && !self.folder.files.is_empty() && (self.strip.pinned || self.strip.hover)
    }

    fn strip_max_scroll(&self, lay: &StripLayout) -> f32 {
        let total = lay.list_top + self.folder.files.len() as f32 * lay.pitch + GAP * self.gallery.dpi;
        (total - lay.bar[3]).max(0.0)
    }

    /// The rows on screen: (first, one past the last).
    fn strip_rows(&self, lay: &StripLayout) -> (usize, usize) {
        let n = self.folder.files.len();
        let first = ((self.strip.scroll - lay.list_top + lay.header_h) / lay.pitch).floor().max(0.0) as usize;
        let last = (((self.strip.scroll + lay.bar[3] - lay.list_top) / lay.pitch).ceil().max(0.0) as usize + 1).min(n);
        (first.min(n), last)
    }

    fn item_rect(&self, lay: &StripLayout, i: usize) -> [f32; 4] {
        let y = lay.list_top + i as f32 * lay.pitch - self.strip.scroll;
        [lay.x0, y, lay.x0 + lay.cell, y + lay.cell]
    }

    pub fn strip_hit(&self, x: f32, y: f32) -> Option<Hit> {
        let lay = self.strip_layout();
        if !inside(&lay.bar, x, y) {
            return None;
        }
        if let Some((h, _)) = lay.chips.iter().find(|(_, r)| inside(r, x, y)) {
            return Some(*h);
        }
        if y < lay.header_h {
            return None;
        }
        let (a, b) = self.strip_rows(&lay);
        (a..b).find(|&i| inside(&self.item_rect(&lay, i), x, y)).map(Hit::Item)
    }

    /// Scroll so that picture `i` is on screen: untouched if it already is, centred if it is far away.
    pub fn strip_show(&mut self, i: usize) {
        let lay = self.strip_layout();
        let r = self.item_rect(&lay, i);
        let visible = r[1] >= lay.header_h && r[3] <= lay.bar[3];
        if !visible {
            let centre = lay.list_top + i as f32 * lay.pitch + lay.cell / 2.0 - (lay.header_h + lay.bar[3]) / 2.0;
            self.strip.scroll = centre.clamp(0.0, self.strip_max_scroll(&lay));
        }
    }

    /// After anything that moves the current picture or changes the order: keep it in view and the thumbnails coming.
    pub fn strip_follow(&mut self) {
        if !self.strip_visible() {
            return;
        }
        self.strip_show(self.folder.index);
        self.strip_request_thumbs();
    }

    /// Keep thumbnails for the rows on screen (plus a few either side) and queue the missing ones, visible first.
    pub fn strip_request_thumbs(&mut self) {
        if !self.strip_visible() {
            return;
        }
        let lay = self.strip_layout();
        let (a, b) = self.strip_rows(&lay);
        if self.strip.requested == Some((a, b)) {
            return;
        }
        self.strip.requested = Some((a, b));
        let n = self.folder.files.len();
        let (ka, kb) = (a.saturating_sub(MARGIN_ROWS * 2), (b + MARGIN_ROWS * 2).min(n));
        self.gallery.keep = (ka..kb).map(|i| path_id(&self.folder.files[i])).collect();
        let keep = &self.gallery.keep;
        self.renderer.retain_thumbs(|id| keep.contains(&id));
        let order = (a..b).chain((ka..a).rev()).chain(b..kb);
        let todo: Vec<PathBuf> = order
            .filter(|&i| {
                let id = path_id(&self.folder.files[i]);
                !self.renderer.has_thumb(id) && !self.gallery.failed.contains(&id)
            })
            .map(|i| self.folder.files[i].clone())
            .collect();
        self.thumb_request(todo);
    }

    /// The pointer moved: open the strip at the right edge, close it when the pointer leaves, track what is under it.
    pub fn strip_hover(&mut self, x: i32, y: i32, left_down: bool) {
        let before = (self.strip.hover, self.strip.hot, self.strip.pinned);
        let usable = self.mode == Mode::Viewer && !self.editor.open && !self.folder.files.is_empty();
        if !usable {
            self.strip.hover = false;
            self.strip.hot = None;
        } else {
            let d = self.gallery.dpi;
            let w = self.renderer.size().0 as f32;
            let left = w - BAR_W * d;
            let fx = x as f32;
            if self.settings.film_strip && !left_down {
                let near_bar = self.strip.hover && fx >= left - 10.0 * d;
                self.strip.hover = fx >= w - EDGE_PX * d || near_bar;
            } else if !self.settings.film_strip {
                self.strip.hover = false;
            }
            self.strip.hot = if self.strip_visible() { self.strip_hit(fx, y as f32) } else { None };
        }
        if before != (self.strip.hover, self.strip.hot, self.strip.pinned) {
            if self.strip.hover && !before.0 {
                self.strip.requested = None;
                self.strip_follow();
            }
            invalidate(self.hwnd);
        }
    }

    /// Should a wheel turn or click at (x, y) go to the strip?
    pub fn strip_blocks(&self, x: i32, y: i32) -> bool {
        self.strip_visible() && inside(&self.strip_layout().bar, x as f32, y as f32)
    }

    /// The wheel over the strip scrolls it (it does not turn pages).
    pub fn strip_wheel(&mut self, delta: i32) {
        let lay = self.strip_layout();
        let step = lay.pitch * 0.9;
        self.strip.scroll = (self.strip.scroll - delta as f32 / 120.0 * step).clamp(0.0, self.strip_max_scroll(&lay));
        self.strip_request_thumbs();
        let (x, y) = self.last_mouse;
        self.strip.hot = self.strip_hit(x as f32, y as f32);
        invalidate(self.hwnd);
    }

    /// A left click at (x, y). True if the strip took it.
    pub fn strip_press(&mut self, x: i32, y: i32) -> bool {
        if !self.strip_blocks(x, y) {
            return false;
        }
        match self.strip_hit(x as f32, y as f32) {
            Some(Hit::Item(i)) => {
                self.navigate(i as isize);
                self.strip_follow();
            }
            Some(Hit::Key(k)) => self.set_sort_key(k),
            Some(Hit::Direction) => self.sort_reverse(),
            Some(Hit::Shuffle) => self.toggle_shuffle(),
            None => {}
        }
        invalidate(self.hwnd);
        true
    }

    pub fn toggle_strip(&mut self) {
        self.strip.pinned = !self.strip.pinned;
        self.strip.requested = None;
        if self.strip.pinned {
            self.strip_follow();
        }
        invalidate(self.hwnd);
    }

    pub fn draw_strip(&mut self) {
        if !self.strip_visible() {
            return;
        }
        let lay = self.strip_layout();
        // The list may have shrunk (a file was deleted or moved) or the window got taller since it was scrolled.
        self.strip.scroll = self.strip.scroll.clamp(0.0, self.strip_max_scroll(&lay));
        self.strip_request_thumbs();
        let d = self.gallery.dpi;
        let (text_c, dim_c) = (srgb(0xF2F2F2, 1.0), srgb(0x6B7078, 1.0));
        let _ = self.renderer.fill_rect(lay.bar, srgb(0x0E0E0E, 0.9));

        // The list.
        let (a, b) = self.strip_rows(&lay);
        let accent = srgb(0x3D8BFF, 1.0);
        for i in a..b {
            let r = self.item_rect(&lay, i);
            if r[3] < lay.header_h || r[1] > lay.bar[3] {
                continue;
            }
            let id = path_id(&self.folder.files[i]);
            if i == self.folder.index {
                let t = 3.0 * d;
                let _ = self.renderer.fill_rect([r[0] - t, r[1] - t, r[2] + t, r[3] + t], accent);
            } else if self.strip.hot == Some(Hit::Item(i)) {
                let t = 2.0 * d;
                let _ = self.renderer.fill_rect([r[0] - t, r[1] - t, r[2] + t, r[3] + t], srgb(0x8A93A6, 1.0));
            }
            let failed = self.gallery.failed.contains(&id);
            let _ = self.renderer.fill_rect(r, if failed { srgb(0x4A2222, 1.0) } else { srgb(0x2A2A2A, 1.0) });
            if let Some((tw, th)) = self.renderer.thumb_size(id) {
                let (cw, ch) = (r[2] - r[0], r[3] - r[1]);
                let s = (cw / tw as f32).min(ch / th as f32).min(1.0);
                let (w, h) = ((tw as f32 * s).round(), (th as f32 * s).round());
                let (l, t) = ((r[0] + (cw - w) / 2.0).round(), (r[1] + (ch - h) / 2.0).round());
                let _ = self.renderer.draw_thumb(id, [l, t, l + w, t + h]);
            }
        }
        // A thin scroll indicator.
        let max = self.strip_max_scroll(&lay);
        if max > 0.0 {
            let track = lay.bar[3] - lay.header_h;
            let total = max + lay.bar[3];
            let len = (track * lay.bar[3] / total).max(24.0 * d);
            let top = lay.header_h + (track - len) * (self.strip.scroll / max);
            let _ = self.renderer.fill_rect([lay.bar[2] - 4.0 * d, top, lay.bar[2] - 1.0 * d, top + len], srgb(0xF2F2F2, 0.35));
        }

        // The header covers whatever scrolled under it.
        let _ = self.renderer.fill_rect([lay.bar[0], 0.0, lay.bar[2], lay.header_h], srgb(0x0E0E0E, 1.0));
        let order = self.folder.order;
        for (hit, r) in &lay.chips {
            let (label, on) = match hit {
                Hit::Key(k) => (k.label().to_string(), order.shuffle.is_none() && order.key == *k),
                Hit::Direction => (if order.descending { "\u{25BC} Desc" } else { "\u{25B2} Asc" }.to_string(), false),
                Hit::Shuffle => ("Shuffle".to_string(), order.shuffle.is_some()),
                Hit::Item(_) => continue,
            };
            let dimmed = matches!(hit, Hit::Direction) && order.shuffle.is_some();
            if on {
                let _ = self.renderer.fill_rect(*r, srgb(0x3D8BFF, 0.95));
            } else if self.strip.hot == Some(*hit) && !dimmed {
                let _ = self.renderer.fill_rect(*r, srgb(0x3A3F4A, 0.95));
            } else {
                let _ = self.renderer.fill_rect(*r, srgb(0x23262C, 0.95));
            }
            if let Some((key, tw, th)) = self.overlay_glyph(&label, 11.5) {
                let _ = self.renderer.draw_overlay(key, r[0] + (r[2] - r[0] - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, if dimmed { dim_c } else { text_c });
            }
        }
        // Name of the picture under the pointer, to the left of the bar.
        if let Some(Hit::Item(i)) = self.strip.hot {
            let name = format!("{}   {}/{}", crate::hud::clip(&file_name_of(&self.folder.files[i]), 48), i + 1, self.folder.files.len());
            if let Some((key, tw, th)) = self.overlay_small(&[name]) {
                let r = self.item_rect(&lay, i);
                let (x, y) = (lay.bar[0] - tw - 8.0 * d, (r[1] + (r[3] - r[1] - th) / 2.0).clamp(0.0, lay.bar[3] - th));
                let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.92));
                let _ = self.renderer.draw_overlay(key, x, y, text_c);
            }
        }
    }
}
