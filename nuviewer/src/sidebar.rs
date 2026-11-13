//! The viewer controls: a vertical bar of buttons on the left edge.
//!
//! It appears while the pointer touches the left edge of the window and goes away when the pointer leaves the bar; the
//! `V` key (or Settings) can also keep it open. Every button is an ordinary `Action`, so it does exactly what its key does.
//! The pointer hovering a button shows what it is and its current key.

use super::*;
use nv_core::keys::Action;

struct Item {
    action: Action,
    glyph: &'static str,
    label: &'static str,
}

/// In the order the bar shows them, top to bottom.
const ITEMS: [Item; 14] = [
    Item { action: Action::Next, glyph: "\u{276F}", label: "Next picture" },
    Item { action: Action::Prev, glyph: "\u{276E}", label: "Previous picture" },
    Item { action: Action::FitReset, glyph: "\u{26F6}", label: "Fit to screen" },
    Item { action: Action::ActualSize, glyph: "1:1", label: "Actual size (100%)" },
    Item { action: Action::Fullscreen, glyph: "\u{2922}", label: "Fullscreen" },
    Item { action: Action::Edit, glyph: "\u{270E}", label: "NuEditor" },
    Item { action: Action::RotatePlus45, glyph: "+45\u{00B0}", label: "Rotate +45\u{00B0}" },
    Item { action: Action::RotateMinus45, glyph: "\u{2212}45\u{00B0}", label: "Rotate \u{2212}45\u{00B0}" },
    Item { action: Action::Slideshow, glyph: "\u{25B6}", label: "Slideshow" },
    Item { action: Action::ToggleSequences, glyph: "\u{25A4}", label: "Image sequences" },
    Item { action: Action::Gallery, glyph: "\u{25A6}", label: "Gallery" },
    Item { action: Action::Settings, glyph: "\u{2699}", label: "Settings" },
    Item { action: Action::Help, glyph: "?", label: "Help" },
    Item { action: Action::Quit, glyph: "\u{2715}", label: "Close the viewer" },
];

pub struct Sidebar {
    /// Kept open with the `V` key.
    pub pinned: bool,
    /// Open because the pointer touched the left edge (and has not left the bar since).
    pub hover: bool,
    /// The button under the pointer.
    pub hot: Option<usize>,
}

impl Sidebar {
    pub fn new() -> Sidebar {
        Sidebar { pinned: false, hover: false, hot: None }
    }
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

impl App {
    /// Bar rectangle and one rectangle per button, in client pixels.
    pub fn sidebar_layout(&self) -> ([f32; 4], Vec<[f32; 4]>) {
        let (_, h) = self.renderer.size();
        let (h, d) = (h as f32, self.gallery.dpi);
        let n = ITEMS.len() as f32;
        let (pad, gap) = (6.0 * d, 3.0 * d);
        // Buttons shrink on a short window so the whole column always fits.
        let btn = (44.0 * d).min((h - 16.0 * d - 2.0 * pad - (n - 1.0) * gap) / n).max(22.0 * d);
        let total = n * btn + (n - 1.0) * gap + 2.0 * pad;
        let (x0, y0) = (6.0 * d, ((h - total) / 2.0).max(0.0));
        let rect = [x0, y0, x0 + btn + 2.0 * pad, y0 + total];
        let buttons = (0..ITEMS.len()).map(|i| {
            let y = y0 + pad + i as f32 * (btn + gap);
            [x0 + pad, y, x0 + pad + btn, y + btn]
        });
        (rect, buttons.collect())
    }

    /// Is the bar on screen right now?
    pub fn sidebar_visible(&self) -> bool {
        self.mode == Mode::Viewer && !self.editor.open && !self.folder.files.is_empty() && (self.sidebar.pinned || self.sidebar.hover)
    }

    /// Should a click / wheel at (x, y) be left to the bar?
    pub fn sidebar_blocks(&self, x: i32, y: i32) -> bool {
        self.sidebar_visible() && inside(&self.sidebar_layout().0, x as f32, y as f32)
    }

    /// Pointer moved: open the bar at the left edge, close it when the pointer leaves, track the button under it.
    pub fn sidebar_hover(&mut self, x: i32, y: i32, left_down: bool) {
        let before = (self.sidebar.hover, self.sidebar.hot);
        let usable = self.mode == Mode::Viewer && !self.editor.open && !self.folder.files.is_empty();
        if !usable {
            self.sidebar.hover = false;
            self.sidebar.hot = None;
        } else {
            let d = self.gallery.dpi;
            let (rect, buttons) = self.sidebar_layout();
            let (fx, fy) = (x as f32, y as f32);
            if self.settings.sidebar && !left_down {
                let near_bar = self.sidebar.hover && fx <= rect[2] + 10.0 * d && fy >= rect[1] - 10.0 * d && fy <= rect[3] + 10.0 * d;
                self.sidebar.hover = fx >= 0.0 && fx <= 6.0 * d || near_bar;
            } else if !self.settings.sidebar {
                self.sidebar.hover = false;
            }
            self.sidebar.hot = if self.sidebar.pinned || self.sidebar.hover { buttons.iter().position(|b| inside(b, fx, fy)) } else { None };
        }
        if before != (self.sidebar.hover, self.sidebar.hot) {
            invalidate(self.hwnd);
        }
    }

    /// A left click at (x, y). True if the bar took it.
    pub fn sidebar_press(&mut self, x: i32, y: i32) -> bool {
        if !self.sidebar_visible() {
            return false;
        }
        let (rect, buttons) = self.sidebar_layout();
        let (fx, fy) = (x as f32, y as f32);
        if !inside(&rect, fx, fy) {
            return false;
        }
        if let Some(i) = buttons.iter().position(|b| inside(b, fx, fy)) {
            let action = ITEMS[i].action;
            if self.sidebar_enabled(action) {
                self.perform(action);
            }
        }
        invalidate(self.hwnd);
        true
    }

    /// Buttons that make no sense right now are dimmed and do nothing.
    fn sidebar_enabled(&self, a: Action) -> bool {
        let many = self.folder.files.len() > 1;
        match a {
            Action::Next | Action::Prev | Action::Slideshow => many,
            Action::FitReset | Action::ActualSize | Action::RotatePlus45 | Action::RotateMinus45 | Action::Edit => !self.is_model() || a == Action::FitReset || a == Action::ActualSize,
            _ => true,
        }
    }

    /// Toggle buttons that are "on" right now.
    fn sidebar_active(&self, a: Action) -> bool {
        match a {
            Action::FitReset => !self.view.base_actual,
            Action::ActualSize => self.view.base_actual,
            Action::Fullscreen => nv_win::is_fullscreen(),
            Action::Slideshow => self.slide.on,
            Action::ToggleSequences => self.settings.sequences,
            Action::Edit => self.editor.open,
            _ => false,
        }
    }

    pub fn draw_sidebar(&mut self) {
        if !self.sidebar_visible() {
            return;
        }
        let (rect, buttons) = self.sidebar_layout();
        let d = self.gallery.dpi;
        let (text_c, dim_c) = (srgb(0xF2F2F2, 1.0), srgb(0x6B7078, 1.0));
        let _ = self.renderer.fill_rect(rect, srgb(0x0E0E0E, 0.88));
        let hot = self.sidebar.hot;
        for (i, item) in ITEMS.iter().enumerate() {
            let r = buttons[i];
            let enabled = self.sidebar_enabled(item.action);
            let active = self.sidebar_active(item.action);
            if active {
                let _ = self.renderer.fill_rect(r, srgb(0x3D8BFF, 0.9));
            } else if hot == Some(i) && enabled {
                let _ = self.renderer.fill_rect(r, srgb(0x3A3F4A, 0.95));
            }
            let size = if item.glyph.chars().count() > 1 { 14.0 } else { 24.0 };
            if let Some((key, tw, th)) = self.overlay_glyph(item.glyph, size) {
                let _ = self.renderer.draw_overlay(key, r[0] + (r[2] - r[0] - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, if enabled { text_c } else { dim_c });
            }
        }
        // What the hovered button is, to the right of it.
        if let Some(i) = hot {
            let item = &ITEMS[i];
            let hint = self.keymap.chords_of(item.action).first().map(|c| format!("    {}", c.format())).unwrap_or_default();
            if let Some((key, tw, th)) = self.overlay_small(&[format!("{}{hint}", item.label)]) {
                let r = buttons[i];
                let (x, y) = (rect[2] + 8.0 * d, r[1] + (r[3] - r[1] - th) / 2.0);
                let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.92));
                let _ = self.renderer.draw_overlay(key, x, y, text_c);
            }
        }
    }
}
