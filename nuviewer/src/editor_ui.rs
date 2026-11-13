//! NuEditor, first mock-up: the shape of the editor drawn over the viewer, so the layout can be judged and the code has a
//! place to grow. A tool palette on the left, an inspector for the chosen tool on the right.
//!
//! What works today: **Rotate** (±45°, ±90°, Save, Revert; the same code as the keys `[` `]` and Ctrl+S). Every other tool
//! is listed from `nv_editor::tools::TOOLS` marked "soon" and shows its description with sliders that do nothing yet.
//! The editor opens with `E` / the pencil button and closes with `E` / Esc. It never edits pixels itself: edits are recorded
//! in `nv_editor::EditStack`, shown live on the GPU, and only written by Save (see `rotate.rs`).

use super::*;
use nv_core::settings::RotateSave;
use nv_editor::tools::{ToolState, TOOLS};

pub struct Editor {
    pub open: bool,
    /// Index into `TOOLS`.
    pub tool: usize,
    /// What the pointer is over: a tool in the palette, or a button of the inspector.
    pub hot: Option<Hit>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Tool(usize),
    Turn(i32),
    Save,
    Revert,
    Close,
}

impl Editor {
    pub fn new() -> Editor {
        Editor { open: false, tool: 0, hot: None }
    }
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

/// Everything clickable, computed from the window size.
struct Layout {
    palette: [f32; 4],
    inspector: [f32; 4],
    tools: Vec<[f32; 4]>,
    turns: Vec<(i32, [f32; 4])>,
    save: [f32; 4],
    revert: [f32; 4],
    close: [f32; 4],
}

impl App {
    fn editor_layout(&self) -> Layout {
        let (w, h) = self.renderer.size();
        let (w, h, d) = (w as f32, h as f32, self.gallery.dpi);
        let palette = [0.0, 0.0, 190.0 * d, h];
        let inspector = [w - 300.0 * d, 0.0, w, h];
        let row = 46.0 * d;
        let tools = (0..TOOLS.len()).map(|i| [8.0 * d, 70.0 * d + i as f32 * row, palette[2] - 8.0 * d, 70.0 * d + (i as f32 + 1.0) * row - 4.0 * d]).collect();
        let bw = (inspector[2] - inspector[0] - 2.0 * 12.0 * d - 3.0 * 8.0 * d) / 4.0;
        let by = 150.0 * d;
        let turns = [(-90, 0), (-45, 1), (45, 2), (90, 3)].iter().map(|&(deg, i)| (deg, [inspector[0] + 12.0 * d + i as f32 * (bw + 8.0 * d), by, inspector[0] + 12.0 * d + i as f32 * (bw + 8.0 * d) + bw, by + 44.0 * d])).collect();
        let half = (inspector[2] - inspector[0] - 3.0 * 12.0 * d) / 2.0;
        let sy = by + 70.0 * d;
        let save = [inspector[0] + 12.0 * d, sy, inspector[0] + 12.0 * d + half, sy + 44.0 * d];
        let revert = [save[2] + 12.0 * d, sy, save[2] + 12.0 * d + half, sy + 44.0 * d];
        let close = [8.0 * d, h - 54.0 * d, palette[2] - 8.0 * d, h - 10.0 * d];
        Layout { palette, inspector, tools, turns, save, revert, close }
    }

    pub fn toggle_editor(&mut self) {
        if self.editor.open {
            self.editor.open = false;
            self.editor.hot = None;
        } else {
            if self.mode != Mode::Viewer || self.folder.files.is_empty() {
                return;
            }
            if self.is_model() {
                self.toast("NuEditor works on pictures, not 3D models");
                return;
            }
            self.editor.open = true;
            self.sidebar.hover = false;
        }
        invalidate(self.hwnd);
    }

    fn hit_editor(&self, x: i32, y: i32) -> Option<Hit> {
        let l = self.editor_layout();
        let (fx, fy) = (x as f32, y as f32);
        if let Some(i) = l.tools.iter().position(|r| inside(r, fx, fy)) {
            return Some(Hit::Tool(i));
        }
        if inside(&l.close, fx, fy) {
            return Some(Hit::Close);
        }
        if TOOLS[self.editor.tool].id == "rotate" {
            if let Some((deg, _)) = l.turns.iter().find(|(_, r)| inside(r, fx, fy)) {
                return Some(Hit::Turn(*deg));
            }
            if inside(&l.save, fx, fy) {
                return Some(Hit::Save);
            }
            if inside(&l.revert, fx, fy) {
                return Some(Hit::Revert);
            }
        }
        None
    }

    /// Pointer moved while the editor is open (called from the sidebar's hover hook).
    pub fn editor_hover(&mut self, x: i32, y: i32) {
        if !self.editor.open {
            return;
        }
        let hot = self.hit_editor(x, y);
        if hot != self.editor.hot {
            self.editor.hot = hot;
            invalidate(self.hwnd);
        }
    }

    /// A left click: true if the editor's panels took it (so the picture underneath is not dragged).
    pub fn editor_press(&mut self, x: i32, y: i32) -> bool {
        if !self.editor.open {
            return false;
        }
        let l = self.editor_layout();
        let (fx, fy) = (x as f32, y as f32);
        let on_panel = inside(&l.palette, fx, fy) || inside(&l.inspector, fx, fy);
        match self.hit_editor(x, y) {
            Some(Hit::Tool(i)) => self.editor.tool = i,
            Some(Hit::Turn(deg)) => self.rotate_by(deg as f32),
            Some(Hit::Save) => {
                self.save_rotation(true);
            }
            Some(Hit::Revert) => {
                if !self.undo_rotation() {
                    self.toast("Nothing to revert");
                }
            }
            Some(Hit::Close) => self.toggle_editor(),
            None => {}
        }
        if on_panel {
            invalidate(self.hwnd);
        }
        on_panel
    }

    pub fn draw_editor(&mut self) {
        if !self.editor.open || self.mode != Mode::Viewer {
            return;
        }
        let l = self.editor_layout();
        let d = self.gallery.dpi;
        let (text_c, dim_c, panel_c, accent) = (srgb(0xF2F2F2, 1.0), srgb(0x9AA0A6, 1.0), srgb(0x14161A, 0.93), srgb(0x3D8BFF, 0.9));
        let hot = self.editor.hot;
        let button = |this: &mut App, r: [f32; 4], label: &str, on: bool, enabled: bool, hover: bool| {
            let fill = if on { accent } else if hover && enabled { srgb(0x3A3F4A, 0.95) } else { srgb(0x24272D, 0.95) };
            let _ = this.renderer.fill_rect(r, fill);
            if let Some((key, tw, th)) = this.overlay_small(&[label.to_string()]) {
                let _ = this.renderer.draw_overlay(key, r[0] + (r[2] - r[0] - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, if enabled { text_c } else { srgb(0x6B7078, 1.0) });
            }
        };

        // Palette.
        let _ = self.renderer.fill_rect(l.palette, panel_c);
        if let Some((key, _, _)) = self.overlay(&["NuEditor".to_string()]) {
            let _ = self.renderer.draw_overlay(key, 12.0 * d, 12.0 * d, text_c);
        }
        if let Some((key, _, _)) = self.overlay_small(&["mock-up".to_string()]) {
            let _ = self.renderer.draw_overlay(key, 12.0 * d, 40.0 * d, srgb(0xE2A33B, 1.0));
        }
        for (i, tool) in TOOLS.iter().enumerate() {
            let r = l.tools[i];
            let selected = i == self.editor.tool;
            if selected {
                let _ = self.renderer.fill_rect(r, accent);
            } else if hot == Some(Hit::Tool(i)) {
                let _ = self.renderer.fill_rect(r, srgb(0x3A3F4A, 0.95));
            }
            let ready = tool.state == ToolState::Ready;
            let col = if ready || selected { text_c } else { dim_c };
            if let Some((key, tw, th)) = self.overlay_glyph(tool.glyph, 18.0) {
                let _ = self.renderer.draw_overlay(key, r[0] + 10.0 * d + (24.0 * d - tw) / 2.0, r[1] + (r[3] - r[1] - th) / 2.0, col);
            }
            let label = if ready { tool.name.to_string() } else { format!("{}  \u{00B7} soon", tool.name) };
            if let Some((key, _, th)) = self.overlay_small(&[label]) {
                let _ = self.renderer.draw_overlay(key, r[0] + 44.0 * d, r[1] + (r[3] - r[1] - th) / 2.0, col);
            }
        }
        button(self, l.close, "Close editor  (E / Esc)", false, true, hot == Some(Hit::Close));

        // Inspector.
        let _ = self.renderer.fill_rect(l.inspector, panel_c);
        let tool = TOOLS[self.editor.tool];
        let x = l.inspector[0] + 14.0 * d;
        if let Some((key, _, _)) = self.overlay(&[tool.name.to_string()]) {
            let _ = self.renderer.draw_overlay(key, x, 12.0 * d, text_c);
        }
        if let Some((key, _, _)) = self.overlay_small(&[hud::clip(tool.hint, 44)]) {
            let _ = self.renderer.draw_overlay(key, x, 46.0 * d, dim_c);
        }
        if tool.id == "rotate" {
            let turn = self.rotation();
            let state = if turn == 0.0 { "Upright — nothing to save".to_string() } else { format!("Turned {:.0}° — not saved yet", if turn > 180.0 { turn - 360.0 } else { turn }) };
            if let Some((key, _, _)) = self.overlay_small(&[state]) {
                let _ = self.renderer.draw_overlay(key, x, 100.0 * d, text_c);
            }
            for (deg, r) in &l.turns {
                let label = format!("{}{}\u{00B0}", if *deg > 0 { "+" } else { "\u{2212}" }, deg.abs());
                button(self, *r, &label, false, true, hot == Some(Hit::Turn(*deg)));
            }
            let pending = self.rotation_pending();
            button(self, l.save, "Save  (Ctrl+S)", pending, pending, hot == Some(Hit::Save));
            button(self, l.revert, "Revert  (Ctrl+Z)", false, pending, hot == Some(Hit::Revert));
            let note = match self.settings.rotate_save {
                RotateSave::Auto => "Moving on saves it without asking.",
                RotateSave::Ask => "Moving on asks first. (Settings > Viewer)",
            };
            if let Some((key, _, _)) = self.overlay_small(&[note.to_string()]) {
                let _ = self.renderer.draw_overlay(key, x, l.save[3] + 18.0 * d, dim_c);
            }
        } else {
            // A mock slider set: shows where the controls will go.
            for (i, name) in ["Amount", "Radius", "Strength"].iter().enumerate() {
                let y = 110.0 * d + i as f32 * 56.0 * d;
                if let Some((key, _, _)) = self.overlay_small(&[name.to_string()]) {
                    let _ = self.renderer.draw_overlay(key, x, y, dim_c);
                }
                let track = [x, y + 26.0 * d, l.inspector[2] - 14.0 * d, y + 30.0 * d];
                let _ = self.renderer.fill_rect(track, srgb(0x2E3238, 1.0));
                let knob_x = track[0] + (track[2] - track[0]) * (0.35 + 0.2 * i as f32);
                let _ = self.renderer.fill_rect([knob_x - 6.0 * d, y + 20.0 * d, knob_x + 6.0 * d, y + 36.0 * d], srgb(0x5A6068, 1.0));
            }
            if let Some((key, _, _)) = self.overlay_small(&["Not built yet — this is the layout it will have.".to_string()]) {
                let _ = self.renderer.draw_overlay(key, x, 300.0 * d, srgb(0xE2A33B, 1.0));
            }
        }
    }
}
