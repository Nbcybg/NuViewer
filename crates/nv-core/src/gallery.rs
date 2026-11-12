//! Gallery grid layout: pure math, no rendering. All values are in physical pixels.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub count: usize,
    pub cols: usize,
    pub rows: usize,
    pub cell: f32,
    pub gap: f32,
    pub margin: f32,
    /// X of the first column (grid is centred horizontally).
    pub left: f32,
    pub view_w: f32,
    pub view_h: f32,
}

impl Layout {
    pub fn new(count: usize, cell: f32, gap: f32, margin: f32, view_w: f32, view_h: f32) -> Self {
        let pitch = cell + gap;
        let cols = (((view_w - 2.0 * margin + gap) / pitch).floor() as usize).max(1);
        let rows = count.div_ceil(cols);
        let grid_w = cols as f32 * cell + (cols as f32 - 1.0) * gap;
        let left = ((view_w - grid_w) / 2.0).max(0.0).floor();
        Layout { count, cols, rows, cell, gap, margin, left, view_w, view_h }
    }

    fn pitch(&self) -> f32 {
        self.cell + self.gap
    }

    pub fn content_height(&self) -> f32 {
        if self.rows == 0 {
            return 0.0;
        }
        2.0 * self.margin + self.rows as f32 * self.cell + (self.rows as f32 - 1.0) * self.gap
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.view_h).max(0.0)
    }

    pub fn clamp_scroll(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }

    /// [left, top, right, bottom] of cell `i` on screen at the given scroll offset.
    pub fn rect(&self, i: usize, scroll: f32) -> [f32; 4] {
        let (row, col) = (i / self.cols, i % self.cols);
        let x = self.left + col as f32 * self.pitch();
        let y = self.margin + row as f32 * self.pitch() - scroll;
        [x, y, x + self.cell, y + self.cell]
    }

    /// Index of the cell under (x, y), if any.
    pub fn hit(&self, x: f32, y: f32, scroll: f32) -> Option<usize> {
        let (lx, ly) = (x - self.left, y + scroll - self.margin);
        if lx < 0.0 || ly < 0.0 {
            return None;
        }
        let (col, row) = ((lx / self.pitch()) as usize, (ly / self.pitch()) as usize);
        let (in_x, in_y) = (lx - col as f32 * self.pitch(), ly - row as f32 * self.pitch());
        if col >= self.cols || in_x > self.cell || in_y > self.cell {
            return None;
        }
        let i = row * self.cols + col;
        (i < self.count).then_some(i)
    }

    /// Indices of cells that intersect the viewport (whole rows).
    pub fn visible(&self, scroll: f32) -> Range<usize> {
        self.rows_to_range(self.first_row(scroll), self.last_row(scroll))
    }

    fn first_row(&self, scroll: f32) -> usize {
        (((scroll - self.margin) / self.pitch()).floor().max(0.0)) as usize
    }

    fn last_row(&self, scroll: f32) -> usize {
        ((scroll + self.view_h - self.margin) / self.pitch()).floor().max(0.0) as usize
    }

    fn rows_to_range(&self, first: usize, last: usize) -> Range<usize> {
        if self.count == 0 {
            return 0..0;
        }
        let start = (first * self.cols).min(self.count);
        let end = ((last + 1) * self.cols).min(self.count);
        start..end.max(start)
    }

    /// Visible range extended by `rows` rows on both sides.
    pub fn visible_with_margin(&self, scroll: f32, rows: usize) -> Range<usize> {
        let first = self.first_row(scroll).saturating_sub(rows);
        let last = self.last_row(scroll) + rows;
        self.rows_to_range(first, last)
    }

    /// Smallest scroll change that brings cell `i` fully into view.
    pub fn scroll_to_show(&self, i: usize, scroll: f32) -> f32 {
        let r = self.rect(i, scroll);
        let s = if r[1] < self.margin.min(self.gap) {
            scroll + r[1] - self.margin
        } else if r[3] > self.view_h - self.margin.min(self.gap) {
            scroll + r[3] - self.view_h + self.margin
        } else {
            scroll
        };
        self.clamp_scroll(s)
    }

    /// Scroll so cell `i` is vertically centred (used when entering the gallery).
    pub fn scroll_to_center(&self, i: usize) -> f32 {
        let row_top = self.margin + (i / self.cols) as f32 * self.pitch();
        self.clamp_scroll(row_top + self.cell / 2.0 - self.view_h / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lay(count: usize) -> Layout {
        // 1000x600 view, 100px cells, 10px gap, 20px margin => (1000-40+10)/110 = 8 cols
        Layout::new(count, 100.0, 10.0, 20.0, 1000.0, 600.0)
    }

    #[test]
    fn columns_and_rows() {
        let l = lay(100);
        assert_eq!(l.cols, 8);
        assert_eq!(l.rows, 13);
    }

    #[test]
    fn narrow_window_has_one_column() {
        let l = Layout::new(10, 100.0, 10.0, 20.0, 50.0, 600.0);
        assert_eq!(l.cols, 1);
    }

    #[test]
    fn rect_and_hit_agree() {
        let l = lay(100);
        for i in [0, 7, 8, 55, 99] {
            let r = l.rect(i, 0.0);
            assert_eq!(l.hit((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0, 0.0), Some(i));
        }
    }

    #[test]
    fn hit_in_gap_or_past_end_is_none() {
        let l = lay(10);
        let r = l.rect(0, 0.0);
        assert_eq!(l.hit(r[2] + 5.0, r[1] + 5.0, 0.0), None); // gap
        let past = l.rect(15, 0.0);
        assert_eq!(l.hit(past[0] + 5.0, past[1] + 5.0, 0.0), None); // cell 15 doesn't exist
    }

    #[test]
    fn visible_range_follows_scroll() {
        let l = lay(100);
        let v0 = l.visible(0.0);
        assert_eq!(v0.start, 0);
        assert!(v0.end >= 5 * 8 && v0.end <= 6 * 8);
        let v = l.visible(500.0);
        assert!(v.start > 0 && v.end <= 100);
        // Every index in the range really intersects the viewport.
        for i in v.clone() {
            let r = l.rect(i, 500.0);
            assert!(r[3] > 0.0 && r[1] < 600.0, "cell {i} not on screen");
        }
    }

    #[test]
    fn empty_gallery_is_safe() {
        let l = lay(0);
        assert_eq!(l.visible(0.0), 0..0);
        assert_eq!(l.max_scroll(), 0.0);
        assert_eq!(l.hit(10.0, 10.0, 0.0), None);
    }

    #[test]
    fn scroll_to_show_makes_cell_visible() {
        let l = lay(100);
        for i in [0, 40, 99] {
            let s = l.scroll_to_show(i, 0.0);
            let r = l.rect(i, s);
            assert!(r[1] >= 0.0 && r[3] <= 600.0, "cell {i} not fully visible after scroll");
        }
    }

    #[test]
    fn scroll_is_clamped() {
        let l = lay(100);
        assert_eq!(l.clamp_scroll(-50.0), 0.0);
        assert_eq!(l.clamp_scroll(1e9), l.max_scroll());
    }

    #[test]
    fn margin_range_is_superset() {
        let l = lay(100);
        let (a, b) = (l.visible(300.0), l.visible_with_margin(300.0, 2));
        assert!(b.start <= a.start && b.end >= a.end);
    }
}
