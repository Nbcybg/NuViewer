//! The order of the pictures: sort by name / date / size / type (either way) and shuffle.
//!
//! There is one list per folder (`FolderList`), and next / previous, the gallery, the film strip and the slideshow all walk
//! it, so changing the order here changes all of them at once. Shuffle puts the pictures in a random order that stays the
//! same until it is switched off (then the chosen sort comes back); the picture on screen never changes.

use super::*;
use nv_core::folder::{Order, SortKey};

impl App {
    /// A random 64-bit number (xorshift64*; only used to pick shuffle seeds).
    pub fn next_rand(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// "Date, newest first" / "shuffled".
    pub fn order_text(&self) -> String {
        let o = self.folder.order;
        if o.shuffle.is_some() {
            return "shuffled".to_string();
        }
        let how = match (o.key, o.descending) {
            (SortKey::Name, false) => "A to Z",
            (SortKey::Name, true) => "Z to A",
            (SortKey::Modified, false) => "oldest first",
            (SortKey::Modified, true) => "newest first",
            (SortKey::Size, false) => "smallest first",
            (SortKey::Size, true) => "largest first",
            (SortKey::Type, false) => "type A to Z",
            (SortKey::Type, true) => "type Z to A",
        };
        format!("{}, {how}", o.key.label().to_lowercase())
    }

    /// Put the pictures in `order`. The current picture and the gallery's selection stay where they are in the sense
    /// that matters: the same files.
    pub fn apply_order(&mut self, order: Order) {
        if self.folder.files.is_empty() {
            self.folder.order = order;
            return;
        }
        if !self.settle_rotation() {
            return;
        }
        let selected = self.folder.files.get(self.gallery.selected).cloned();
        self.folder.set_order(order);
        if let Some(i) = selected.and_then(|p| self.folder.position(&p)) {
            self.gallery.selected = i;
        }
        self.slide.next = None;
        self.rebuild_items();
        if self.slide.on {
            self.pick_next();
        }
        self.strip.requested = None;
        self.hud.info_for = None;
        if self.mode == Mode::Gallery {
            let tile = self.tile_of(self.gallery.selected);
            self.gallery.scroll = self.layout().scroll_to_center(tile);
            self.request_thumbs();
            set_title(self.hwnd, &self.title());
        } else {
            self.refresh(); // the neighbours changed: prefetch them
        }
        invalidate(self.hwnd);
    }

    /// Keep the chosen sort across runs (shuffle is never kept).
    fn remember_sort(&mut self) {
        self.settings.sort = self.folder.order.key;
        self.settings.sort_desc = self.folder.order.descending;
        let _ = self.settings.save();
    }

    /// Sort by `key`. Choosing the key that is already on reverses it, like clicking a column header.
    pub fn set_sort_key(&mut self, key: SortKey) {
        let cur = self.folder.order;
        let descending = if cur.key == key && cur.shuffle.is_none() { !cur.descending } else { false };
        self.apply_order(Order { key, descending, shuffle: None });
        self.remember_sort();
        self.toast(format!("Sorted by {}", self.order_text()));
    }

    /// `J`: the next sort key (name > date > size > type).
    pub fn sort_cycle(&mut self) {
        let key = self.folder.order.key.next();
        let descending = self.folder.order.descending;
        self.apply_order(Order { key, descending, shuffle: None });
        self.remember_sort();
        self.toast(format!("Sorted by {}", self.order_text()));
    }

    /// `Shift+J`: ascending / descending.
    pub fn sort_reverse(&mut self) {
        let o = self.folder.order;
        self.apply_order(Order { key: o.key, descending: !o.descending, shuffle: None });
        self.remember_sort();
        self.toast(format!("Sorted by {}", self.order_text()));
    }

    /// `R`: random order on / off.
    pub fn toggle_shuffle(&mut self) {
        let o = self.folder.order;
        if o.shuffle.is_some() {
            self.apply_order(Order { shuffle: None, ..o });
            self.toast(format!("Shuffle off — back to {}", self.order_text()));
        } else {
            let seed = self.next_rand();
            self.apply_order(Order { shuffle: Some(seed), ..o });
            self.toast("Shuffle on — the pictures are in random order (R again restores the sort)");
        }
    }
}
