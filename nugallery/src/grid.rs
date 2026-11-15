//! Layout of the items on screen: one grid serves all three views. Thumbnails are a grid of tall cells (picture + name),
//! the list view is a grid of low, wide cells, and the details view is a grid with one column. Pure arithmetic in physical
//! pixels; `area` is the part of the window the items live in (the panels that are docked are outside it).

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub count: usize,
    pub cols: usize,
    pub rows: usize,
    pub item_w: f32,
    pub item_h: f32,
    pub gap_x: f32,
    pub gap_y: f32,
    pub margin: f32,
    /// Left, top, right, bottom of the region the items are drawn in.
    pub area: [f32; 4],
    /// X of the first column.
    left: f32,
}

impl Grid {
    /// `single_column`: one column as wide as the area (the details view).
    pub fn new(count: usize, item_w: f32, item_h: f32, gap_x: f32, gap_y: f32, margin: f32, area: [f32; 4], single_column: bool) -> Grid {
        let area_w = (area[2] - area[0]).max(1.0);
        let (item_w, cols) = if single_column {
            ((area_w - 2.0 * margin).max(1.0), 1)
        } else {
            (item_w, (((area_w - 2.0 * margin + gap_x) / (item_w + gap_x)).floor() as usize).max(1))
        };
        let rows = count.div_ceil(cols);
        let grid_w = cols as f32 * item_w + (cols as f32 - 1.0) * gap_x;
        let left = area[0] + ((area_w - grid_w) / 2.0).max(0.0).floor();
        Grid { count, cols, rows, item_w, item_h, gap_x, gap_y, margin, area, left }
    }

    fn view_h(&self) -> f32 {
        (self.area[3] - self.area[1]).max(1.0)
    }

    fn pitch_y(&self) -> f32 {
        self.item_h + self.gap_y
    }

    fn pitch_x(&self) -> f32 {
        self.item_w + self.gap_x
    }

    pub fn content_height(&self) -> f32 {
        if self.rows == 0 {
            return 0.0;
        }
        2.0 * self.margin + self.rows as f32 * self.item_h + (self.rows as f32 - 1.0) * self.gap_y
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.view_h()).max(0.0)
    }

    pub fn clamp(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }

    /// [left, top, right, bottom] of item `i` in window coordinates at the given scroll offset.
    pub fn rect(&self, i: usize, scroll: f32) -> [f32; 4] {
        let (row, col) = (i / self.cols, i % self.cols);
        let x = self.left + col as f32 * self.pitch_x();
        let y = self.area[1] + self.margin + row as f32 * self.pitch_y() - scroll;
        [x, y, x + self.item_w, y + self.item_h]
    }

    /// The item under the window point (x, y), if any (gaps and the empty end of the last row are nothing).
    pub fn hit(&self, x: f32, y: f32, scroll: f32) -> Option<usize> {
        if x < self.area[0] || x >= self.area[2] || y < self.area[1] || y >= self.area[3] {
            return None;
        }
        let (lx, ly) = (x - self.left, y + scroll - self.area[1] - self.margin);
        if lx < 0.0 || ly < 0.0 {
            return None;
        }
        let (col, row) = ((lx / self.pitch_x()) as usize, (ly / self.pitch_y()) as usize);
        let (in_x, in_y) = (lx - col as f32 * self.pitch_x(), ly - row as f32 * self.pitch_y());
        if col >= self.cols || in_x > self.item_w || in_y > self.item_h {
            return None;
        }
        let i = row * self.cols + col;
        (i < self.count).then_some(i)
    }

    fn first_row(&self, scroll: f32) -> usize {
        (((scroll - self.margin) / self.pitch_y()).floor().max(0.0)) as usize
    }

    fn last_row(&self, scroll: f32) -> usize {
        ((scroll + self.view_h() - self.margin) / self.pitch_y()).floor().max(0.0) as usize
    }

    fn rows_to_range(&self, first: usize, last: usize) -> Range<usize> {
        if self.count == 0 {
            return 0..0;
        }
        let start = (first * self.cols).min(self.count);
        let end = ((last + 1) * self.cols).min(self.count);
        start..end.max(start)
    }

    /// Items that touch the area (whole rows).
    pub fn visible(&self, scroll: f32) -> Range<usize> {
        self.rows_to_range(self.first_row(scroll), self.last_row(scroll))
    }

    pub fn visible_with_margin(&self, scroll: f32, rows: usize) -> Range<usize> {
        self.rows_to_range(self.first_row(scroll).saturating_sub(rows), self.last_row(scroll) + rows)
    }

    /// The smallest scroll change that brings item `i` fully into the area.
    pub fn scroll_to_show(&self, i: usize, scroll: f32) -> f32 {
        let r = self.rect(i, scroll);
        let pad = self.margin.min(self.gap_y.max(2.0));
        let s = if r[1] < self.area[1] + pad {
            scroll + r[1] - self.area[1] - self.margin
        } else if r[3] > self.area[3] - pad {
            scroll + r[3] - self.area[3] + self.margin
        } else {
            scroll
        };
        self.clamp(s)
    }

    /// Scroll so item `i` is in the middle of the area.
    pub fn scroll_to_center(&self, i: usize) -> f32 {
        let row_top = self.margin + (i / self.cols) as f32 * self.pitch_y();
        self.clamp(row_top + self.item_h / 2.0 - self.view_h() / 2.0)
    }

    /// The items whose cells intersect the window rectangle `r` (the rubber band), at the given scroll.
    pub fn in_rect(&self, r: [f32; 4], scroll: f32) -> Vec<usize> {
        let (x0, x1) = (r[0].min(r[2]), r[0].max(r[2]));
        let (y0, y1) = (r[1].min(r[3]), r[1].max(r[3]));
        let mut out = Vec::new();
        let vis = self.visible_with_margin(scroll, 1);
        for i in vis {
            let c = self.rect(i, scroll);
            if c[2] > x0 && c[0] < x1 && c[3] > y0 && c[1] < y1 {
                out.push(i);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(count: usize) -> Grid {
        // area 1000x600 at (0, 40); cells 100x130, gaps 10, margin 20 => (1000-40+10)/110 = 8 columns
        Grid::new(count, 100.0, 130.0, 10.0, 10.0, 20.0, [0.0, 40.0, 1000.0, 640.0], false)
    }

    #[test]
    fn columns_rows_and_docked_panels() {
        let l = g(100);
        assert_eq!((l.cols, l.rows), (8, 13));
        // A docked left panel of 300 px leaves 700: (700-40+10)/110 = 6 columns, and the grid starts after the panel.
        let narrow = Grid::new(100, 100.0, 130.0, 10.0, 10.0, 20.0, [300.0, 40.0, 1000.0, 640.0], false);
        assert_eq!(narrow.cols, 6);
        assert!(narrow.rect(0, 0.0)[0] >= 300.0);
    }

    #[test]
    fn single_column_fills_the_width() {
        let l = Grid::new(10, 100.0, 30.0, 0.0, 2.0, 8.0, [0.0, 0.0, 800.0, 600.0], true);
        assert_eq!(l.cols, 1);
        let r = l.rect(3, 0.0);
        assert_eq!(r[2] - r[0], 784.0);
        assert_eq!(l.hit(400.0, r[1] + 5.0, 0.0), Some(3));
    }

    #[test]
    fn rect_and_hit_agree_with_scroll_and_area_offset() {
        let l = g(100);
        for i in [0, 7, 8, 55, 99] {
            for scroll in [0.0, 333.0] {
                let r = l.rect(i, scroll);
                if r[3] > 40.0 && r[1] < 640.0 {
                    let (cx, cy) = ((r[0] + r[2]) / 2.0, ((r[1] + r[3]) / 2.0).clamp(41.0, 639.0));
                    assert_eq!(l.hit(cx, cy, scroll), if cy >= r[1] && cy < r[3] { Some(i) } else { l.hit(cx, cy, scroll) });
                }
            }
        }
        let r = l.rect(5, 0.0);
        assert_eq!(l.hit((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0, 0.0), Some(5));
        assert_eq!(l.hit(500.0, 10.0, 0.0), None, "the toolbar strip above the area is not a cell");
    }

    #[test]
    fn gaps_and_the_end_of_the_last_row_are_empty() {
        let l = g(10);
        let r = l.rect(0, 0.0);
        assert_eq!(l.hit(r[2] + 5.0, r[1] + 5.0, 0.0), None);
        let past = l.rect(15, 0.0);
        assert_eq!(l.hit(past[0] + 5.0, past[1] + 5.0, 0.0), None);
    }

    #[test]
    fn visible_cells_really_intersect_the_area() {
        let l = g(100);
        let v = l.visible(500.0);
        assert!(v.start > 0 && v.end <= 100);
        for i in v {
            let r = l.rect(i, 500.0);
            assert!(r[3] > 40.0 && r[1] < 640.0, "cell {i} not on screen");
        }
        let (a, b) = (l.visible(300.0), l.visible_with_margin(300.0, 2));
        assert!(b.start <= a.start && b.end >= a.end);
    }

    #[test]
    fn scrolling_to_an_item_shows_it() {
        let l = g(100);
        for i in [0, 40, 99] {
            let s = l.scroll_to_show(i, 0.0);
            let r = l.rect(i, s);
            assert!(r[1] >= 40.0 && r[3] <= 640.0, "cell {i}: {r:?}");
            let c = l.scroll_to_center(i);
            assert!((0.0..=l.max_scroll()).contains(&c));
        }
        assert_eq!(l.clamp(-5.0), 0.0);
        assert_eq!(l.clamp(1e9), l.max_scroll());
    }

    #[test]
    fn rubber_band_picks_the_cells_it_touches() {
        let l = g(100);
        let a = l.rect(0, 0.0);
        let b = l.rect(9, 0.0); // second row, second column
        let got = l.in_rect([a[0] + 10.0, a[1] + 10.0, b[0] + 10.0, b[1] + 10.0], 0.0);
        assert_eq!(got, vec![0, 1, 8, 9]);
        assert!(l.in_rect([0.0, 0.0, 5.0, 5.0], 0.0).is_empty());
    }

    #[test]
    fn empty_grid_is_safe() {
        let l = g(0);
        assert_eq!(l.visible(0.0), 0..0);
        assert_eq!(l.max_scroll(), 0.0);
        assert_eq!(l.hit(10.0, 50.0, 0.0), None);
    }
}
