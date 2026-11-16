//! The menu bar shared by NuViewer, NuGallery and NuEditor: a strip along the top edge that appears when the pointer
//! touches that edge (and goes the moment the pointer leaves it) or when Alt / F10 is tapped. Each program draws it with
//! its own text renderer; everything else lives here: where the labels are, which one is under the pointer, the Alt+letter
//! mnemonics ("&File" = Alt+F), keyboard navigation and showing the popup menu under a label.
//!
//! Typical use in a program:
//! 1. `MenuBar::new(&["&File", "&Edit", "&View", "&Tools", "&Help"])`;
//! 2. on resize / every frame that draws it: `bar.layout(width, dpi, |s| text_width(s))`;
//! 3. `bar.pointer(x, y, dpi)` from mouse moves (returns true if the bar needs a redraw); `bar.alt_tap()` from `Event::AltTap`;
//! 4. when `bar.visible()`, draw `bar.rects()` / `bar.labels()` (underline `bar.mnemonic_of(i)`), highlight `bar.hot()`;
//! 5. a click on a label, or `bar.key(vk, alt)` returning `Some(i)`, → build that menu's entries and call `bar.run(hwnd, i, &entries)`.

use crate::menu::{popup as popup_at, Entry};
use windows::Win32::Foundation::HWND;

/// Height of the bar in logical pixels.
pub const BAR_H: f32 = 28.0;
/// How far from the top edge the pointer reveals the bar (logical pixels).
pub const EDGE: f32 = 4.0;
/// Width of the "back" button at the left end of the bar and of the "close" button at the right end (logical pixels).
pub const NAV_W: f32 = 40.0;

/// The two buttons at the ends of the bar: `<` goes back to the previous program (or into the gallery), `x` closes the program.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    Back,
    Close,
}

pub struct MenuBar {
    /// Labels as written, with `&` before the mnemonic letter.
    labels: Vec<String>,
    rects: Vec<[f32; 4]>,
    /// The bar was opened with the keyboard (Alt / F10): it stays until Esc, a click elsewhere or another Alt.
    keyboard: bool,
    /// The pointer is on the bar or on the top edge.
    pointer: bool,
    hot: Option<usize>,
    nav_hot: Option<Nav>,
    /// A popup is open under this label.
    open: Option<usize>,
    width: f32,
}

impl MenuBar {
    pub fn new(labels: &[&str]) -> MenuBar {
        MenuBar { labels: labels.iter().map(|s| nv_text::tr(s).into_owned()).collect(), rects: Vec::new(), keyboard: false, pointer: false, hot: None, nav_hot: None, open: None, width: 0.0 }
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// The label with the `&` taken out (what is drawn).
    pub fn plain(label: &str) -> String {
        label.replace('&', "")
    }

    pub fn labels(&self) -> Vec<String> {
        self.labels.iter().map(|l| Self::plain(l)).collect()
    }

    /// The letter to underline in label `i`, as its index among the drawn characters.
    pub fn mnemonic_of(&self, i: usize) -> Option<usize> {
        let l = self.labels.get(i)?;
        let at = l.find('&')?;
        Some(l[..at].chars().count())
    }

    pub fn visible(&self) -> bool {
        self.keyboard || self.pointer || self.open.is_some()
    }

    /// Was it opened with the keyboard (then the first label is highlighted until the pointer says otherwise)?
    pub fn keyboard(&self) -> bool {
        self.keyboard
    }

    pub fn hot(&self) -> Option<usize> {
        self.hot
    }

    pub fn rects(&self) -> &[[f32; 4]] {
        &self.rects
    }

    pub fn height(&self, dpi: f32) -> f32 {
        BAR_H * dpi
    }

    /// The strip itself, for the background.
    pub fn strip(&self, width: f32, dpi: f32) -> [f32; 4] {
        [0.0, 0.0, width, self.height(dpi)]
    }

    /// Place the labels left to right; `measure` gives the width of a plain label in pixels.
    pub fn layout(&mut self, width: f32, dpi: f32, mut measure: impl FnMut(&str) -> f32) {
        self.width = width;
        self.rects.clear();
        let (pad, mut x) = (12.0 * dpi, NAV_W * dpi);
        let h = self.height(dpi);
        for l in &self.labels {
            let w = measure(&Self::plain(l)) + 2.0 * pad;
            self.rects.push([x, 0.0, x + w, h]);
            x += w;
        }
    }

    /// The `<` button (left end) and the `x` button (right end) of the bar, in client pixels.
    pub fn nav_rects(&self, width: f32, dpi: f32) -> (Nav, [f32; 4], Nav, [f32; 4]) {
        let (w, h) = (NAV_W * dpi, self.height(dpi));
        (Nav::Back, [0.0, 0.0, w, h], Nav::Close, [width - w, 0.0, width, h])
    }

    /// Which end button is at the point (only while the bar shows).
    pub fn nav_hit(&self, x: f32, y: f32, dpi: f32) -> Option<Nav> {
        if !(self.pointer || self.keyboard) || self.width <= 0.0 {
            return None;
        }
        let (a, ra, b, rb) = self.nav_rects(self.width, dpi);
        let inside = |r: [f32; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        if inside(ra) {
            Some(a)
        } else if inside(rb) {
            Some(b)
        } else {
            None
        }
    }

    pub fn nav_hot(&self) -> Option<Nav> {
        self.nav_hot
    }

    /// Which label is at the point.
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        self.rects.iter().position(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3])
    }

    /// The pointer moved (client pixels). The bar shows while the pointer is on the top edge or on the bar, and goes at once
    /// when it leaves (unless a key opened it). True if what is drawn changed.
    pub fn pointer(&mut self, x: i32, y: i32, dpi: f32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        let was = (self.pointer, self.hot, self.nav_hot);
        let on_bar = self.pointer && fy < self.height(dpi) + 2.0 * dpi;
        self.pointer = fy < EDGE * dpi || on_bar;
        self.hot = if self.pointer || self.keyboard { self.hit(fx, fy) } else { None };
        self.nav_hot = self.nav_hit(fx, fy, dpi);
        if self.hot.is_some() && self.keyboard {
            // The pointer has taken over from the keyboard highlight.
        }
        (self.pointer, self.hot, self.nav_hot) != was
    }

    /// The pointer left the window.
    pub fn pointer_gone(&mut self) -> bool {
        let changed = self.pointer || self.nav_hot.is_some();
        self.pointer = false;
        self.hot = None;
        self.nav_hot = None;
        changed
    }

    /// Alt (or F10) was tapped: show the bar for the keyboard, or put it away again. Returns true if now shown.
    pub fn alt_tap(&mut self) -> bool {
        self.keyboard = !self.keyboard;
        self.hot = if self.keyboard && !self.labels.is_empty() { Some(0) } else { None };
        self.pointer = false;
        self.keyboard
    }

    /// Esc, or a click somewhere else: the keyboard lets go.
    pub fn close(&mut self) {
        self.keyboard = false;
        self.pointer = false;
        self.hot = None;
        self.open = None;
    }

    /// A key while the bar owns the keyboard: Left / Right move the highlight; returns `Some(i)` when label `i` should open
    /// (Enter, Down, or Alt + its mnemonic letter, which also works with the bar hidden). Esc closes it.
    pub fn key(&mut self, vk: u32, alt: bool) -> Option<usize> {
        if alt || self.keyboard {
            if vk >= 0x30 && (vk <= 0x39 || (0x41..=0x5A).contains(&vk)) {
                let ch = char::from_u32(vk).unwrap_or(' ').to_ascii_lowercase();
                let found = self.labels.iter().position(|l| l.find('&').and_then(|a| l[a + 1..].chars().next()).map(|c| c.to_ascii_lowercase()) == Some(ch));
                if let Some(i) = found {
                    self.hot = Some(i);
                    return Some(i);
                }
            }
        }
        if !self.keyboard {
            return None;
        }
        let n = self.labels.len();
        match vk {
            0x1B => self.close(),
            0x25 if n > 0 => self.hot = Some(self.hot.map(|h| (h + n - 1) % n).unwrap_or(0)),
            0x27 if n > 0 => self.hot = Some(self.hot.map(|h| (h + 1) % n).unwrap_or(0)),
            0x0D | 0x28 if self.hot.is_some() => return self.hot,
            _ => {}
        }
        None
    }

    /// Does a key press belong to the bar (so the program should not use it for something else)?
    pub fn wants_key(&self, vk: u32, alt: bool) -> bool {
        if alt {
            return vk >= 0x30 && (vk <= 0x39 || (0x41..=0x5A).contains(&vk));
        }
        self.keyboard
    }

    /// Show the popup of label `i` under it and wait for the choice. The bar stays visible while the menu is open; the
    /// keyboard let go of it afterwards. Returns the id of the entry picked.
    pub fn run(&mut self, hwnd: HWND, i: usize, entries: &[Entry]) -> Option<u32> {
        let r = *self.rects.get(i)?;
        self.open = Some(i);
        let picked = popup_at(hwnd, r[0] as i32, r[3] as i32, entries);
        self.open = None;
        self.keyboard = false;
        self.pointer = false;
        self.hot = None;
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> MenuBar {
        let mut b = MenuBar::new(&["&File", "&Edit", "&View"]);
        b.layout(1000.0, 1.0, |s| s.chars().count() as f32 * 8.0);
        b
    }

    #[test]
    fn labels_lose_their_ampersand_and_keep_the_mnemonic() {
        let b = bar();
        assert_eq!(b.labels(), ["File", "Edit", "View"]);
        assert_eq!(b.mnemonic_of(1), Some(0));
        assert_eq!(MenuBar::new(&["Fi&le"]).mnemonic_of(0), Some(2));
    }

    #[test]
    fn labels_are_laid_out_side_by_side_and_found_by_position() {
        let b = bar();
        for w in b.rects().windows(2) {
            assert!(w[0][2] <= w[1][0] + 0.01);
        }
        let r = b.rects()[1];
        assert_eq!(b.hit((r[0] + r[2]) / 2.0, 10.0), Some(1));
        assert_eq!(b.hit(900.0, 10.0), None);
    }

    #[test]
    fn the_top_edge_reveals_the_bar_and_leaving_hides_it_at_once() {
        let mut b = bar();
        assert!(!b.visible());
        assert!(b.pointer(500, 0, 1.0) && b.visible());
        assert!(b.pointer(500, 100, 1.0), "going away is a change to draw");
        assert!(!b.visible());
        // Once shown, moving down onto the bar keeps it.
        b.pointer(500, 0, 1.0);
        b.pointer(500, 20, 1.0);
        assert!(b.visible());
    }

    #[test]
    fn alt_opens_and_closes_it_and_letters_find_their_menu() {
        let mut b = bar();
        assert!(b.alt_tap() && b.visible() && b.hot() == Some(0));
        assert_eq!(b.key(0x27, false), None);
        assert_eq!(b.hot(), Some(1));
        assert_eq!(b.key(0x0D, false), Some(1));
        assert_eq!(b.key(0x56, false), Some(2), "V = &View while the bar is open");
        assert!(!b.alt_tap() && !b.visible());
        // Alt+F works with the bar hidden; a bare letter does not.
        assert_eq!(b.key(0x46, true), Some(0));
        assert_eq!(b.key(0x46, false), None);
    }

    #[test]
    fn escape_lets_go() {
        let mut b = bar();
        b.alt_tap();
        b.key(0x1B, false);
        assert!(!b.visible());
    }
}
