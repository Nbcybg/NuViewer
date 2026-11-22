//! Layout of the items on screen: one grid serves all three views. Thumbnails are a grid of tall cells (picture + name),
//! the list view is a grid of low, wide cells, and the details view is a grid with one column. Pure arithmetic in physical
//! pixels; `area` is the part of the window the items live in (the panels that are docked are outside it).
//!
//! A grid can be cut into *bands* (group view): every band starts on a new row under a header strip, and the items of a
//! band fill its rows from the left. Without groups there is one band and no header, so the arithmetic is the old one.

use std::ops::Range;

/// One group of the view: items `start..end`, a header strip (when the grid has headers) and the rows under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub start: usize,
    pub end: usize,
    /// Top of the header strip, relative to the top of the area (content coordinates, scroll 0).
    pub head_y: f32,
    /// Top of the first row of the band, same coordinates.
    pub rows_y: f32,
    pub rows: usize,
}

#[derive(Debug, Clone, PartialEq)]
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
    /// Height of a group header strip (0 = no groups).
    pub head_h: f32,
    pub bands: Vec<Band>,
    /// X of the first column.
    left: f32,
    /// Content height (margins included).
    content: f32,
}

impl Grid {
    /// `single_column`: one column as wide as the area (the details view).
    pub fn new(count: usize, item_w: f32, item_h: f32, gap_x: f32, gap_y: f32, margin: f32, area: [f32; 4], single_column: bool) -> Grid {
        Self::grouped(count, item_w, item_h, gap_x, gap_y, margin, area, single_column, &[], 0.0)
    }

    /// Like `new`, with groups: `starts` are the first item index of every group (ascending, the first is 0) and `head_h`
    /// the height of the header strip above each of them. An empty `starts` means no groups.
    #[allow(clippy::too_many_arguments)]
    pub fn grouped(count: usize, item_w: f32, item_h: f32, gap_x: f32, gap_y: f32, margin: f32, area: [f32; 4], single_column: bool, starts: &[usize], head_h: f32) -> Grid {
        let area_w = (area[2] - area[0]).max(1.0);
        let (item_w, cols) = if single_column {
            ((area_w - 2.0 * margin).max(1.0), 1)
        } else {
            (item_w, (((area_w - 2.0 * margin + gap_x) / (item_w + gap_x)).floor() as usize).max(1))
        };
        let grid_w = cols as f32 * item_w + (cols as f32 - 1.0) * gap_x;
        let left = area[0] + ((area_w - grid_w) / 2.0).max(0.0).floor();
        let head_h = if starts.is_empty() { 0.0 } else { head_h };
        // The bands.
        let mut bands: Vec<Band> = Vec::new();
        let mut y = margin;
        let mut total_rows = 0;
        if count > 0 {
            let cuts: Vec<usize> = if starts.is_empty() { vec![0] } else { starts.iter().copied().filter(|&s| s < count).collect() };
            let cuts = if cuts.first() == Some(&0) { cuts } else { std::iter::once(0).chain(cuts).collect() };
            for (b, &start) in cuts.iter().enumerate() {
                let end = cuts.get(b + 1).copied().unwrap_or(count);
                if end <= start {
                    continue;
                }
                let rows = (end - start).div_ceil(cols);
                total_rows += rows;
                let head_y = y;
                let rows_y = y + head_h;
                bands.push(Band { start, end, head_y, rows_y, rows });
                y = rows_y + rows as f32 * item_h + (rows as f32 - 1.0) * gap_y;
                // Air between two groups.
                y += if head_h > 0.0 { gap_y * 2.0 } else { 0.0 };
            }
        }
        let content = if count == 0 { 0.0 } else { y - if head_h > 0.0 { gap_y * 2.0 } else { 0.0 } + margin };
        Grid { count, cols, rows: total_rows, item_w, item_h, gap_x, gap_y, margin, area, head_h, bands, left, content }
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

    pub fn has_groups(&self) -> bool {
        self.head_h > 0.0
    }

    pub fn content_height(&self) -> f32 {
        self.content
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height() - self.view_h()).max(0.0)
    }

    pub fn clamp(&self, scroll: f32) -> f32 {
        scroll.clamp(0.0, self.max_scroll())
    }

    /// The band item `i` belongs to.
    pub fn band_of(&self, i: usize) -> Option<usize> {
        if self.bands.is_empty() {
            return None;
        }
        // An index past the end is placed as if the last band went on (the empty end of the last row).
        Some(self.bands.partition_point(|b| b.start <= i).saturating_sub(1))
    }

    /// [left, top, right, bottom] of item `i` in window coordinates at the given scroll offset.
    pub fn rect(&self, i: usize, scroll: f32) -> [f32; 4] {
        let Some(b) = self.band_of(i).map(|b| self.bands[b]) else {
            return [self.left, self.area[1] + self.margin - scroll, self.left + self.item_w, self.area[1] + self.margin + self.item_h - scroll];
        };
        let k = i - b.start;
        let (row, col) = (k / self.cols, k % self.cols);
        let x = self.left + col as f32 * self.pitch_x();
        let y = self.area[1] + b.rows_y + row as f32 * self.pitch_y() - scroll;
        [x, y, x + self.item_w, y + self.item_h]
    }

    /// The header strip of band `b` (window coordinates); it spans the grid's width.
    pub fn header_rect(&self, b: usize, scroll: f32) -> [f32; 4] {
        let band = self.bands[b];
        let y = self.area[1] + band.head_y - scroll;
        let right = self.left + self.cols as f32 * self.item_w + (self.cols as f32 - 1.0) * self.gap_x;
        [self.left, y, right.min(self.area[2] - self.margin * 0.5), y + self.head_h]
    }

    /// The bands whose header or rows touch the area.
    pub fn visible_bands(&self, scroll: f32) -> Range<usize> {
        let (top, bottom) = (scroll, scroll + self.view_h());
        let first = self.bands.iter().position(|b| b.rows_y + b.rows as f32 * self.pitch_y() > top).unwrap_or(self.bands.len());
        let last = self.bands.iter().rposition(|b| b.head_y < bottom).map(|p| p + 1).unwrap_or(0);
        first..last.max(first)
    }

    /// The header of the band under the window point (x, y), if that is where it is.
    pub fn header_at(&self, x: f32, y: f32, scroll: f32) -> Option<usize> {
        if !self.has_groups() || x < self.area[0] || x >= self.area[2] || y < self.area[1] || y >= self.area[3] {
            return None;
        }
        let ly = y + scroll - self.area[1];
        self.bands.iter().position(|b| ly >= b.head_y && ly < b.rows_y)
    }

    /// The item under the window point (x, y), if any (gaps, headers and the empty end of the last row are nothing).
    pub fn hit(&self, x: f32, y: f32, scroll: f32) -> Option<usize> {
        if x < self.area[0] || x >= self.area[2] || y < self.area[1] || y >= self.area[3] {
            return None;
        }
        let (lx, ly) = (x - self.left, y + scroll - self.area[1]);
        if lx < 0.0 || ly < 0.0 {
            return None;
        }
        let bi = self.bands.partition_point(|b| b.rows_y <= ly).checked_sub(1)?;
        let b = self.bands[bi];
        let ly = ly - b.rows_y;
        let (col, row) = ((lx / self.pitch_x()) as usize, (ly / self.pitch_y()) as usize);
        let (in_x, in_y) = (lx - col as f32 * self.pitch_x(), ly - row as f32 * self.pitch_y());
        if col >= self.cols || row >= b.rows || in_x > self.item_w || in_y > self.item_h {
            return None;
        }
        let i = b.start + row * self.cols + col;
        (i < b.end).then_some(i)
    }

    /// Items whose rows touch the content range `top..bottom` (content coordinates; whole rows).
    fn items_between(&self, top: f32, bottom: f32) -> Range<usize> {
        let (mut lo, mut hi) = (usize::MAX, 0usize);
        for b in &self.bands {
            if b.rows_y >= bottom {
                break;
            }
            let band_bottom = b.rows_y + b.rows as f32 * self.pitch_y();
            if band_bottom <= top {
                continue;
            }
            // The first row whose bottom is below `top` (a row that ends in the gap above the area is not visible).
            let above = top - b.rows_y - self.item_h;
            let r0 = if above < 0.0 { 0 } else { (above / self.pitch_y()).floor() as usize + 1 };
            let r1 = (((bottom - b.rows_y) / self.pitch_y()).ceil() as usize).saturating_sub(1).min(b.rows - 1);
            if r0 > r1 {
                continue;
            }
            lo = lo.min((b.start + r0 * self.cols).min(b.end));
            hi = hi.max((b.start + (r1 + 1) * self.cols).min(b.end));
        }
        if lo == usize::MAX { 0..0 } else { lo..hi.max(lo) }
    }

    /// Items that touch the area (whole rows).
    pub fn visible(&self, scroll: f32) -> Range<usize> {
        self.items_between(scroll, scroll + self.view_h())
    }

    pub fn visible_with_margin(&self, scroll: f32, rows: usize) -> Range<usize> {
        let extra = rows as f32 * self.pitch_y();
        self.items_between(scroll - extra, scroll + self.view_h() + extra)
    }

    /// Top of item `i`'s row in content coordinates, taking the header along for the first row of a band.
    fn row_top(&self, i: usize, with_header: bool) -> f32 {
        match self.band_of(i).map(|b| self.bands[b]) {
            Some(b) => {
                let row = (i - b.start) / self.cols;
                if row == 0 && with_header { b.head_y } else { b.rows_y + row as f32 * self.pitch_y() }
            }
            None => self.margin,
        }
    }

    /// The smallest scroll change that brings item `i` fully into the area (its group header too, for a first row).
    pub fn scroll_to_show(&self, i: usize, scroll: f32) -> f32 {
        let r = self.rect(i, scroll);
        let pad = self.margin.min(self.gap_y.max(2.0));
        let head_top = self.area[1] + self.row_top(i, true) - scroll;
        let s = if r[1] < self.area[1] + pad || head_top < self.area[1] {
            // A header goes to the very top edge; an ordinary row keeps the margin.
            scroll + head_top.min(r[1]) - self.area[1] - if head_top < r[1] { 0.0 } else { self.margin }
        } else if r[3] > self.area[3] - pad {
            scroll + r[3] - self.area[3] + self.margin
        } else {
            scroll
        };
        self.clamp(s)
    }

    /// Scroll so item `i` is in the middle of the area.
    pub fn scroll_to_center(&self, i: usize) -> f32 {
        self.clamp(self.row_top(i, false) + self.item_h / 2.0 - self.view_h() / 2.0)
    }

    /// The item an arrow key moves to from `i`: one row up (`down == false`) or down, in the same column; across a group
    /// boundary it lands in the neighbouring group's edge row (clamped to the items it has). `None` = nowhere to go.
    pub fn vertical(&self, i: usize, down: bool) -> Option<usize> {
        if i >= self.count {
            return None;
        }
        let bi = self.band_of(i)?;
        let b = self.bands[bi];
        let k = i - b.start;
        let (row, col) = (k / self.cols, k % self.cols);
        if down {
            let want = (row + 1) * self.cols + col;
            if want < b.end - b.start {
                return Some(b.start + want);
            }
            if (row + 1) * self.cols < b.end - b.start {
                return Some(b.end - 1); // the last, shorter row
            }
            let n = self.bands.get(bi + 1)?;
            Some(n.start + col.min(n.end - n.start - 1))
        } else {
            if row > 0 {
                return Some(b.start + (row - 1) * self.cols + col);
            }
            let p = self.bands.get(bi.checked_sub(1)?)?;
            let last_row = p.rows - 1;
            Some((p.start + last_row * self.cols + col).min(p.end - 1))
        }
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

    /// Three groups of 10, 3 and 17 items, 8 columns, 24 px headers.
    fn grouped() -> Grid {
        Grid::grouped(30, 100.0, 130.0, 10.0, 10.0, 20.0, [0.0, 40.0, 1000.0, 640.0], false, &[0, 10, 13], 24.0)
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
    fn content_height_is_the_old_one_without_groups() {
        // 13 rows of 130 with 10 between, 20 margin on both sides.
        assert_eq!(g(100).content_height(), 2.0 * 20.0 + 13.0 * 130.0 + 12.0 * 10.0);
        assert_eq!(g(0).content_height(), 0.0);
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
        assert_eq!(l.vertical(0, true), None);
    }

    #[test]
    fn groups_start_on_their_own_row_under_a_header() {
        let l = grouped();
        assert_eq!(l.bands.len(), 3);
        // Group 0: items 0..10 = 2 rows; group 1: 3 items = 1 row, and it starts below the first group's rows.
        let (a, b, c) = (l.rect(9, 0.0), l.rect(10, 0.0), l.rect(13, 0.0));
        assert!(b[1] > a[3] + 24.0, "header strip sits between the groups");
        assert_eq!(b[0], l.rect(0, 0.0)[0], "a group starts at the first column");
        assert!(c[1] > b[3] + 24.0);
        // Headers are hit as headers, items as items, and the strip between is nothing.
        let h1 = l.header_rect(1, 0.0);
        assert_eq!(l.header_at(h1[0] + 5.0, h1[1] + 5.0, 0.0), Some(1));
        assert_eq!(l.hit(h1[0] + 5.0, h1[1] + 5.0, 0.0), None);
        assert_eq!(l.hit((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0, 0.0), Some(10));
        // The 4th column of group 1 does not exist.
        let (px, py) = (b[0] + 4.0 * 110.0 + 5.0, b[1] + 5.0);
        assert_eq!(l.hit(px, py, 0.0), None);
    }

    #[test]
    fn every_item_is_hit_where_it_is_drawn_in_a_grouped_grid() {
        let l = grouped();
        for scroll in [0.0, 150.0] {
            for i in 0..30 {
                let r = l.rect(i, scroll);
                let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
                if cy > 40.0 && cy < 640.0 {
                    assert_eq!(l.hit(cx, cy, scroll), Some(i), "item {i} at scroll {scroll}");
                }
            }
        }
    }

    #[test]
    fn visible_range_follows_groups() {
        let l = grouped();
        let all = l.visible(0.0);
        for i in all.clone() {
            let r = l.rect(i, 0.0);
            assert!(r[3] > 40.0 && r[1] < 640.0);
        }
        assert_eq!(all.start, 0);
        // Scrolled to the bottom the last item is visible.
        assert_eq!(l.visible(l.max_scroll()).end, 30);
        assert!(l.visible_bands(0.0).contains(&0));
        assert!(l.visible_bands(l.max_scroll()).contains(&2));
    }

    #[test]
    fn arrows_cross_group_edges_in_the_same_column() {
        let l = grouped();
        // Group 0: 8 + 2 items; down from item 1 (col 1) -> item 9? 8+1 = 9 is col 1 of row 1.
        assert_eq!(l.vertical(1, true), Some(9));
        // Down from item 5 (col 5) in the first row: row 1 has only cols 0..1, so the shorter row's last item.
        assert_eq!(l.vertical(5, true), Some(9));
        // Down from the last row goes to the next group, same column clamped (group 1 has 3 items).
        assert_eq!(l.vertical(9, true), Some(11));
        assert_eq!(l.vertical(8, true), Some(10));
        // Up from group 1 lands in the last row of group 0, same column (col 2 -> clamped to the 2 items of that row).
        assert_eq!(l.vertical(12, false), Some(9));
        assert_eq!(l.vertical(10, false), Some(8));
        assert_eq!(l.vertical(0, false), None);
        assert_eq!(l.vertical(29, true), None);
    }

    #[test]
    fn scrolling_to_the_first_item_of_a_group_shows_its_header() {
        let l = grouped();
        let s = l.scroll_to_show(13, l.max_scroll());
        let h = l.header_rect(2, s);
        assert!(h[1] >= 40.0 - 0.5, "header {h:?} is cut off at scroll {s}");
    }
}
