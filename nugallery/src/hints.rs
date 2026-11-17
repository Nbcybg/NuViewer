//! Hover hints: rest the pointer on a button, a name, a folder of the tree or the address bar and a small box says what it is
//! (a file's full name with its type, size and date; the whole path of a folder; a button's name and its key). The hint comes
//! after the pointer has been still for a moment and goes away when it moves to something else, on a click, the wheel or a key.

use crate::app::App;
use crate::chrome::Btn;
use crate::draw::{fmt_bytes, fmt_date, px, type_label};
use crate::gfx::{line_h, outline, pal, rgb, wrap, Text};
use nv_render::Renderer;
use nv_win::{kill_timer, set_timer};

pub const TIMER_TIP: u32 = 7;
/// How long the pointer rests before the hint shows.
const TIP_DELAY_MS: u32 = 450;

/// What is hovered and what the hint says.
#[derive(Clone, Debug)]
pub struct Tip {
    pub key: u64,
    /// The lines broken to fit the window (done once, when the hint appears).
    pub shown: Vec<String>,
    /// Where the pointer was.
    pub at: (f32, f32),
}

fn key_of(parts: &[&str]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish() | 1
}

fn button_hint(b: Btn) -> &'static str {
    match b {
        Btn::Prev => "Previous item (←)",
        Btn::Next => "Next item (→)",
        Btn::Back => "Back to the last folder (Alt+←)",
        Btn::Forward => "Forward (Alt+→)",
        Btn::Up => "Up one folder (Backspace, Alt+↑)",
        Btn::Sort => "Sort by… (Ctrl+J)",
        Btn::Show => "Show only… (pictures, RAW, documents, all files)",
        Btn::ViewThumbs => "Thumbnails (Ctrl+1)",
        Btn::ViewDetails => "Details: name, size, type, date (Ctrl+2)",
        Btn::ViewList => "List (Ctrl+3)",
        Btn::Tools => "Tools: batch rename / convert, compress, PDF, settings",
        Btn::PinTop => "Keep the toolbar open (Ctrl+T)",
        Btn::PinLeft => "Keep the folders panel open (Ctrl+B)",
        Btn::PinRight => "Keep the details panel open (Ctrl+I)",
    }
}

impl App {
    /// What the pointer is on, if it has a hint: a key that is the same while the pointer stays on the same thing, and the lines.
    fn tip_at(&self, x: i32, y: i32) -> Option<(u64, Vec<String>)> {
        let (fx, fy) = (x as f32, y as f32);
        if self.menubar.visible() && fy < self.menubar.height(self.dpi) {
            return None;
        }
        if let Some(b) = self.chrome.button_at(fx, fy) {
            return Some((key_of(&["btn", button_hint(b)]), vec![button_hint(b).to_string()]));
        }
        if self.chrome.top.is_some() {
            if crate::gfx::contains(self.chrome.path_box, fx, fy) {
                return Some((key_of(&["path"]), vec!["Address bar (Ctrl+L)".into(), "Type or paste a folder or file path and press Enter".into(), self.display_dir().display().to_string()]));
            }
            if crate::gfx::contains(self.chrome.search_box, fx, fy) {
                return Some((key_of(&["search"]), vec!["Search this folder (Ctrl+F)".into(), "Shows only the names that contain the words".into()]));
            }
        }
        if let Some(i) = self.tree_row_at(fx, fy) {
            let n = &self.tree.nodes[i];
            return Some((key_of(&["tree", &n.path.to_string_lossy()]), vec![n.name.clone(), n.path.display().to_string()]));
        }
        if self.chrome.over_panel(fx, fy) {
            return None;
        }
        let e = self.listing.get(self.item_at(x, y)?)?;
        let mut lines = vec![e.name.clone()];
        let mut facts = vec![if e.is_dir() { "Folder".to_string() } else { type_label(e) }];
        if !e.is_dir() {
            facts.push(fmt_bytes(e.size));
        }
        facts.push(fmt_date(e.modified));
        lines.push(facts.join("  ·  "));
        if nv_core::archive_tools::is_archive(&e.path) {
            lines.push("Archive: double-click opens it like a folder".into());
        }
        Some((key_of(&["item", &e.path.to_string_lossy()]), lines))
    }

    /// The pointer moved: forget a hint that no longer fits, and wait for the pointer to rest on whatever is under it now.
    pub fn tip_move(&mut self, x: i32, y: i32, busy: bool) {
        if busy {
            self.tip_hide();
            return;
        }
        let key = self.tip_at(x, y).map(|t| t.0).unwrap_or(0);
        if key != 0 && self.tip.as_ref().map(|t| t.key) == Some(key) {
            return;
        }
        self.tip_hide();
        self.tip_wait = key;
        if key != 0 {
            set_timer(self.hwnd, TIMER_TIP, TIP_DELAY_MS);
        }
    }

    pub fn tip_hide(&mut self) {
        kill_timer(self.hwnd, TIMER_TIP);
        self.tip_wait = 0;
        if self.tip.take().is_some() {
            if self.test_mode {
                self.update_title();
            }
            nv_win::invalidate(self.hwnd);
        }
    }

    /// The pointer has rested: show the hint of what is under it.
    pub fn tip_tick(&mut self) {
        kill_timer(self.hwnd, TIMER_TIP);
        let (x, y) = self.last_mouse;
        if self.drag.is_some() || self.rubber.is_some() || self.active.is_some() && self.tip_wait == 0 {
            return;
        }
        if let Some((key, lines)) = self.tip_at(x, y) {
            if key == self.tip_wait {
                let d = self.dpi;
                let font = px(13.0, d);
                let max_w = (self.size.0 as f32 * 0.5).clamp(260.0 * d, 560.0 * d) as i32;
                let mut shown: Vec<String> = Vec::new();
                for (i, l) in lines.iter().enumerate() {
                    shown.extend(wrap(l, font, max_w).into_iter().take(if i == 0 { 4 } else { 2 }));
                }
                self.tip = Some(Tip { key, shown, at: (x as f32, y as f32) });
                if self.test_mode {
                    self.update_title();
                }
                nv_win::invalidate(self.hwnd);
            }
        }
    }

    /// Draw the hint (the last thing in the frame).
    pub fn draw_tip(&mut self, w: f32, h: f32, d: f32) {
        let Some(tip) = self.tip.clone() else { return };
        let font = px(13.0, d);
        let lines = tip.shown.clone();
        let Some(r) = self.renderer.as_mut() else { return };
        let text: &mut Text = &mut self.text;
        let lh = line_h(font);
        let tw = lines.iter().map(|l| text.width(l, font)).max().unwrap_or(0) as f32 + 20.0 * d;
        let th = lines.len() as f32 * lh + 12.0 * d;
        let (mut x, mut y) = (tip.at.0 + 14.0 * d, tip.at.1 + 20.0 * d);
        if x + tw > w - 4.0 * d {
            x = (w - tw - 4.0 * d).max(4.0 * d);
        }
        if y + th > h - 4.0 * d {
            y = (tip.at.1 - th - 8.0 * d).max(4.0 * d);
        }
        draw_box(r, text, [x, y, x + tw, y + th], &lines, font, lh, d);
    }

    /// Draw the "Move to … / Copy to …" label that follows a drag of files.
    pub fn draw_drag_label(&mut self, w: f32, h: f32, d: f32) {
        let Some(dr) = self.drag.as_ref() else { return };
        let n = dr.paths.len();
        let what = format!("{n} item{}", if n == 1 { "" } else { "s" });
        let line = match &dr.target {
            Some(t) => {
                let to = t.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| t.display().to_string());
                let copy = match dr.op {
                    Some(nv_win::files::Op::Copy) => true,
                    Some(_) => false,
                    None => t.to_string_lossy().chars().next().map(|c| c.to_ascii_lowercase()) != dr.paths[0].to_string_lossy().chars().next().map(|c| c.to_ascii_lowercase()),
                };
                format!("{} {what} to {to}", if copy { "Copy" } else { "Move" })
            }
            None => format!("{what} — drop on a folder (Ctrl copies, Shift moves)"),
        };
        let font = px(13.0, d);
        let (x, y) = (self.last_mouse.0 as f32 + 16.0 * d, self.last_mouse.1 as f32 + 18.0 * d);
        let Some(r) = self.renderer.as_mut() else { return };
        let text: &mut Text = &mut self.text;
        let lh = line_h(font);
        let tw = text.width(&line, font) as f32 + 20.0 * d;
        let th = lh + 10.0 * d;
        let x = x.min(w - tw - 4.0 * d).max(4.0 * d);
        let y = y.min(h - th - 4.0 * d).max(4.0 * d);
        draw_box(r, text, [x, y, x + tw, y + th], &[line], font, lh, d);
    }
}

fn draw_box(r: &mut Renderer, text: &mut Text, rect: [f32; 4], lines: &[String], font: i32, lh: f32, d: f32) {
    let _ = r.fill_rect([rect[0] + 2.0 * d, rect[1] + 3.0 * d, rect[2] + 2.0 * d, rect[3] + 3.0 * d], rgb(0x000000, 0.35));
    let _ = r.fill_rect(rect, rgb(0x0E0E0E, 0.96));
    outline(r, rect, 1.0, rgb(pal::PANEL_EDGE, 1.0));
    for (i, l) in lines.iter().enumerate() {
        let c = if i == 0 { rgb(pal::TEXT, 1.0) } else { rgb(pal::DIM, 1.0) };
        text.draw(r, l, font, rect[0] + 10.0 * d, rect[1] + 6.0 * d + i as f32 * lh, c);
    }
}
