//! Read mode, for books and comics: two pages side by side with no gap, as one picture.
//!
//! Pages are the folder's files in their order. Which pages pair up is decided by `nv_core::spread`: from the *anchor*
//! page on they go two at a time (1+2, 3+4 ...), a page that is already a wide double-page scan stands alone, and the
//! pages before the anchor (the cover) stand alone. The anchor starts at the Settings value (page 2: the cover alone, then
//! 2+3, 4+5 ...) and `Shift+B` sets it to the page on screen, which is the fix when a book's pairs come out wrong.
//! `Ctrl+B` swaps the sides for right-to-left books (manga).
//!
//! The two pages are drawn as one wide virtual picture (both at the height of the taller one), so zoom, hold-to-zoom,
//! panning and fit all work on the pair exactly as on a single picture.

use super::*;
use nv_core::spread;
use std::cell::RefCell;

pub struct Read {
    pub on: bool,
    /// Where the pairs start (0-based page = file index).
    pub anchor: usize,
    /// The folder the anchor belongs to.
    anchor_dir: PathBuf,
    /// Anchors the user set, per folder, for this run.
    anchors: HashMap<PathBuf, usize>,
    /// Stored size of each page by path id, (0, 0) when it can't be read from the header. Filled on demand.
    dims: RefCell<HashMap<u64, (u32, u32)>>,
    /// A page known to start a spread (for this anchor and page count): the walk from the anchor can resume there, so
    /// asking about pages near the one on screen does not mean walking the whole book every time.
    checkpoint: std::cell::Cell<Option<(usize, usize, usize)>>,
}

impl Read {
    pub fn new() -> Read {
        Read { on: false, anchor: 1, anchor_dir: PathBuf::new(), anchors: HashMap::new(), dims: RefCell::new(HashMap::new()), checkpoint: std::cell::Cell::new(None) }
    }

    /// The list, the order or the pairing changed: spreads must be walked from the anchor again.
    pub fn invalidate(&self) {
        self.checkpoint.set(None);
    }

    /// Forget what was measured (the files changed, or it is a different folder).
    pub fn forget_sizes(&self) {
        self.dims.borrow_mut().clear();
        self.invalidate();
    }
}

/// Two pages laid out as one picture.
pub struct SpreadView {
    pub left: u64,
    pub right: u64,
    /// Pixel sizes of the left and right page (None = not loaded yet).
    pub left_size: Option<(u32, u32)>,
    pub right_size: Option<(u32, u32)>,
    /// The pair as one picture: both pages scaled to the taller one's height, side by side.
    pub virt: (u32, u32),
    /// The common height the pages are scaled to.
    pub height: f32,
}

impl App {
    /// Is page `i` a wide double-page scan? Read from the file's header once, then remembered.
    fn page_is_wide(&self, i: usize) -> bool {
        let Some(path) = self.folder.files.get(i) else { return false };
        let id = path_id(path);
        let known = self.read.dims.borrow().get(&id).copied();
        let (w, h) = known.unwrap_or_else(|| {
            let d = self.meta.get(&id).map(|m| (m.width, m.height)).or_else(|| nv_codecs::peek_size(path)).unwrap_or((0, 0));
            self.read.dims.borrow_mut().insert(id, d);
            d
        });
        spread::is_wide(w, h)
    }

    /// The spread (first page, page count) that holds page `page`. Without read mode every page is a spread of one.
    pub fn spread_at(&self, page: usize) -> (usize, usize) {
        if !self.read.on {
            return (page, 1);
        }
        let n = self.folder.files.len();
        // Resume from the last known spread start when the page is at or after it.
        let from = match self.read.checkpoint.get() {
            Some((anchor, count, start)) if anchor == self.read.anchor && count == n && start >= self.read.anchor && start <= page => start,
            _ => self.read.anchor,
        };
        let found = spread::spread_of(n, from, page, &|p| self.page_is_wide(p));
        if page >= self.read.anchor {
            self.read.checkpoint.set(Some((self.read.anchor, n, found.0)));
        }
        found
    }

    /// The spread on screen. Documents and 3D models are never paired.
    pub fn current_spread(&self) -> (usize, usize) {
        let i = self.folder.index;
        if !self.read.on || self.mode != Mode::Viewer || self.folder.files.is_empty() || self.is_model() || self.doc_pages().is_some() {
            return (i, 1);
        }
        self.spread_at(i)
    }

    /// Both pages of the spread on screen as one picture, or None when it is a single page (or nothing has loaded).
    pub fn spread_view(&self) -> Option<SpreadView> {
        if !self.read.on {
            return None;
        }
        let (start, len) = self.current_spread();
        if len < 2 || start + 1 >= self.folder.files.len() {
            return None;
        }
        let (first, second) = (self.id_at(start), self.id_at(start + 1));
        let first_size = self.renderer.image_size_of(first)?;
        let second_size = self.renderer.image_size_of(second);
        // Until the partner has loaded it is assumed to be as big as its neighbour, so the first page already sits in
        // its half of the screen and does not jump when the second arrives (if they are alike).
        let sz2 = second_size.unwrap_or(first_size);
        let height = first_size.1.max(sz2.1).max(1) as f32;
        let wide = |s: (u32, u32)| s.0 as f32 * height / s.1.max(1) as f32;
        let virt = ((wide(first_size) + wide(sz2)).round().max(1.0) as u32, height as u32);
        let (left, right, left_size, right_size) =
            if self.settings.read_rtl { (second, first, second_size, Some(first_size)) } else { (first, second, Some(first_size), second_size) };
        Some(SpreadView { left, right, left_size, right_size, virt, height })
    }

    /// Draw the spread into `dst` (the rectangle of the whole pair): the seam is snapped to a whole pixel so the pages
    /// touch exactly, with no gap and no overlap.
    pub fn draw_spread(&mut self, sv: &SpreadView, dst: [f32; 4], nearest: bool) {
        let total_w = dst[2] - dst[0];
        // How wide the left page is as a share of the pair (the missing partner counts as the same width as its neighbour).
        let share = |s: Option<(u32, u32)>| s.map(|s| s.0 as f32 * sv.height / s.1.max(1) as f32);
        let fallback = share(sv.left_size).or(share(sv.right_size)).unwrap_or(1.0);
        let (wl, wr) = (share(sv.left_size).unwrap_or(fallback), share(sv.right_size).unwrap_or(fallback));
        let seam = (dst[0] + total_w * wl / (wl + wr)).round();
        let right_edge = dst[2].round();
        if sv.left_size.is_some() {
            let _ = self.renderer.draw_image(sv.left, [dst[0], dst[1], seam, dst[3]], nearest);
        }
        if sv.right_size.is_some() {
            let _ = self.renderer.draw_image(sv.right, [seam, dst[1], right_edge, dst[3]], nearest);
        }
    }

    /// File indices to keep decoded in read mode: the spread on screen, then the next one and the previous one (the
    /// direction of travel first), then one more ahead. Both pages of each.
    pub fn read_wanted(&self) -> Vec<usize> {
        let n = self.folder.files.len();
        let wide = |p: usize| self.page_is_wide(p);
        let (start, _) = self.current_spread();
        let next = |s: usize| spread::next_start(n, self.read.anchor, s, &wide);
        let prev = |s: usize| spread::prev_start(n, self.read.anchor, s, &wide);
        let mut starts = vec![start];
        let (ahead, behind) = if self.dir >= 0 { (next(start), prev(start)) } else { (prev(start), next(start)) };
        starts.extend(ahead);
        starts.extend(behind);
        if let Some(a) = ahead {
            starts.extend(if self.dir >= 0 { next(a) } else { prev(a) });
        }
        let mut out = Vec::new();
        for s in starts {
            let (first, len) = self.spread_at(s);
            for p in first..first + len {
                if p < n && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// "3-4/120" in read mode with a pair on screen, "3/120" otherwise.
    pub fn position_text(&self) -> String {
        let (start, len) = self.current_spread();
        let n = self.folder.files.len();
        if len == 2 {
            format!("{}-{}/{n}", start + 1, start + 2)
        } else {
            format!("{}/{n}", self.folder.index + 1)
        }
    }

    /// The anchor a folder starts with: what the user set for it earlier this run, else the Settings page number.
    fn default_anchor(&self) -> usize {
        self.read.anchors.get(&self.folder.dir).copied().unwrap_or_else(|| self.settings.read_start.saturating_sub(1) as usize)
    }

    /// A different folder was opened (or the list changed under us): measure its pages again, take its anchor.
    pub fn read_folder_changed(&mut self) {
        self.read.forget_sizes();
        if self.read.anchor_dir != self.folder.dir {
            self.read.anchor_dir = self.folder.dir.clone();
            self.read.anchor = self.default_anchor();
        }
        self.read_settle();
    }

    /// With read mode on, the current page must be the first page of its spread.
    pub fn read_settle(&mut self) {
        if self.read.on && !self.folder.files.is_empty() {
            self.folder.index = self.spread_at(self.folder.index).0;
        }
    }

    /// `B`: read mode on / off.
    pub fn toggle_read(&mut self) {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return;
        }
        if !self.read.on && (self.is_model() || self.doc_pages().is_some()) {
            self.toast("Read mode pairs the pictures of a folder; it does not apply to a PDF or a 3D model");
            return;
        }
        if !self.settle_rotation() {
            return;
        }
        self.read.on = !self.read.on;
        if self.read.on {
            self.read.anchor_dir = self.folder.dir.clone();
            self.read.anchor = self.default_anchor();
            self.read_settle();
        }
        self.read_changed();
        if self.read.on {
            let dir = if self.settings.read_rtl { "right to left" } else { "left to right" };
            self.toast(format!("Read mode — {dir}. Shift+B: pairs start at this page · Ctrl+B: other direction · B: off"));
        } else {
            self.toast("Read mode off");
        }
    }

    /// `Shift+B`: the page on screen is the first of a pair; the pages before it stand alone.
    pub fn read_start_here(&mut self) {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return;
        }
        if !self.read.on {
            self.toggle_read();
        }
        if !self.read.on {
            return;
        }
        let page = self.folder.index;
        self.read.anchor = page;
        self.read.anchors.insert(self.folder.dir.clone(), page);
        self.read_changed();
        self.toast(format!("Pairs start at page {} — earlier pages stand alone", page + 1));
    }

    /// `Ctrl+B`: which side the first page of a pair is on.
    pub fn toggle_read_direction(&mut self) {
        self.settings.read_rtl = !self.settings.read_rtl;
        let _ = self.settings.save();
        self.toast(if self.settings.read_rtl { "Right to left: the first page of a pair is on the right" } else { "Left to right: the first page of a pair is on the left" });
        invalidate(self.hwnd);
    }

    /// The pairing changed: new view, new prefetch.
    fn read_changed(&mut self) {
        self.view = self.view.fresh();
        self.refresh();
        invalidate(self.hwnd);
    }

    /// One step through the pairs (forward +1, back -1), keeping to the spread boundaries.
    pub fn read_step(&mut self, d: isize) {
        let n = self.folder.files.len();
        let wide = |p: usize| self.page_is_wide(p);
        let (start, _) = self.current_spread();
        let target = if d > 0 { spread::next_start(n, self.read.anchor, start, &wide) } else { spread::prev_start(n, self.read.anchor, start, &wide) };
        if let Some(t) = target {
            self.navigate_with_dir(t, d.signum());
        }
    }
}
