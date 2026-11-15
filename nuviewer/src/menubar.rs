//! The viewer's menu bar (File, Edit, View, Go, Tools, Help): the strip along the top edge that appears when the pointer
//! touches that edge or Alt / F10 is tapped (`nv_win::menubar`). The entries are the right-click menu's own builders in
//! `mouse.rs`, so every item runs the same action as its key.

use super::*;
use nv_win::{menu, menubar::MenuBar, text};

pub const LABELS: [&str; 6] = ["&File", "&Edit", "&View", "&Go", "&Tools", "&Help"];

pub fn new_bar() -> MenuBar {
    MenuBar::new(&LABELS)
}

impl App {
    /// The pointer moved: the bar shows on the top edge and goes the moment the pointer leaves it.
    pub fn menubar_hover(&mut self, x: i32, y: i32) {
        if self.rename.is_some() || self.mode != Mode::Viewer {
            return;
        }
        if self.menubar.pointer(x, y, self.gallery.dpi) {
            invalidate(self.hwnd);
        }
    }

    /// Alt (or F10) was tapped.
    pub fn menubar_alt(&mut self) {
        if self.rename.is_some() {
            return;
        }
        self.menubar.alt_tap();
        invalidate(self.hwnd);
    }

    /// A left press: on a label it opens that menu; anywhere else it just lets go of a keyboard-opened bar. True if the press was the bar's.
    pub fn menubar_press(&mut self, x: i32, y: i32) -> bool {
        let bar_h = self.menubar.height(self.gallery.dpi);
        if self.menubar.visible() && (y as f32) < bar_h {
            match self.menubar.hit(x as f32, y as f32) {
                Some(i) => self.open_menubar_menu(i),
                None => {
                    self.menubar.close();
                    invalidate(self.hwnd);
                }
            }
            return true;
        }
        if self.menubar.keyboard() {
            self.menubar.close();
            invalidate(self.hwnd);
        }
        false
    }

    /// A key: Alt+letter opens a menu; while the bar has the keyboard its arrows / Enter / Esc are its own. True if used.
    pub fn menubar_key(&mut self, k: u32) -> bool {
        let m = nv_win::modifiers();
        if m.alt && !m.ctrl {
            if let Some(i) = self.menubar.key(k, true) {
                self.open_menubar_menu(i);
                return true;
            }
            return false; // Alt+Enter and friends stay with the key map
        }
        if self.menubar.keyboard() {
            if let Some(i) = self.menubar.key(k, false) {
                self.open_menubar_menu(i);
            }
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    pub fn open_menubar_menu(&mut self, i: usize) {
        let entries = self.menu_entries(i);
        nv_win::set_cursor_hidden(false);
        invalidate(self.hwnd);
        let picked = self.menubar.run(self.hwnd, i, &entries);
        invalidate(self.hwnd);
        if let Some(id) = picked {
            self.menu_pick(id);
        }
    }

    /// Draw the bar if it is showing. Called last among the overlays (above the info bar, below the toast).
    pub fn draw_menubar(&mut self) {
        let (w, _) = self.renderer.size();
        let d = self.gallery.dpi;
        let px = (13.0 * d).round() as i32;
        self.menubar.layout(w as f32, d, |s| text::measure(s, px) as f32);
        if !self.menubar.visible() {
            return;
        }
        let strip = self.menubar.strip(w as f32, d);
        let _ = self.renderer.fill_rect(strip, srgb(0x15171A, 0.97));
        let _ = self.renderer.fill_rect([0.0, strip[3] - 1.0, w as f32, strip[3]], srgb(0x34373C, 1.0));
        let labels = self.menubar.labels();
        let rects: Vec<[f32; 4]> = self.menubar.rects().to_vec();
        let (hot, keyboard) = (self.menubar.hot(), self.menubar.keyboard());
        for (i, (rect, label)) in rects.iter().zip(labels.iter()).enumerate() {
            if hot == Some(i) {
                let _ = self.renderer.fill_rect([rect[0], rect[1] + 2.0 * d, rect[2], rect[3] - 1.0], srgb(if keyboard { 0x3D8BFF } else { 0x34373C }, 1.0));
            }
            if let Some((key, tw, th)) = self.overlay_small(&[label.clone()]) {
                let (x, y) = ((rect[0] + rect[2] - tw) / 2.0, (rect[1] + rect[3] - th) / 2.0);
                let _ = self.renderer.draw_overlay(key, x, y, srgb(0xF2F2F2, 1.0));
                if keyboard {
                    if let Some(k) = self.menubar.mnemonic_of(i) {
                        let pad = 4.0 * d;
                        let before: String = label.chars().take(k).collect();
                        let ch: String = label.chars().skip(k).take(1).collect();
                        let ux = x + pad + text::measure(&before, px) as f32;
                        let uw = text::measure(&ch, px) as f32;
                        let _ = self.renderer.fill_rect([ux, y + th - pad - 1.0 * d, ux + uw, y + th - pad], srgb(0xF2F2F2, 1.0));
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn _keep(_: menu::Entry) {}
