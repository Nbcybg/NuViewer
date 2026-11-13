//! Read mode: which pages of a book go on screen together.
//!
//! Pages are the folder's files in order. From the *anchor* (the page the pairs start at) they are taken two at a
//! time: 1+2, 3+4 ... A page that is wide (a scan that already holds both halves of a spread) stands alone, and the
//! pairing carries on after it. Pages before the anchor (the cover, a title page) are shown alone. Everything is a
//! pure function of the page count, the anchor and a "is this page wide" test, so it can be tested without pictures.

/// A page counts as a double-page spread when it is this much wider than it is tall.
pub const WIDE_RATIO: f32 = 1.2;

pub fn is_wide(width: u32, height: u32) -> bool {
    height > 0 && width as f32 > height as f32 * WIDE_RATIO
}

/// How many pages the spread that starts at `start` has (1 or 2). `start` must be a spread start.
fn len_at(n: usize, start: usize, wide: &dyn Fn(usize) -> bool) -> usize {
    if start + 1 >= n || wide(start) || wide(start + 1) { 1 } else { 2 }
}

/// The spread holding `page`: (first page, number of pages). `n` = page count, `anchor` = where the pairs start.
pub fn spread_of(n: usize, anchor: usize, page: usize, wide: &dyn Fn(usize) -> bool) -> (usize, usize) {
    if n == 0 {
        return (0, 1);
    }
    let page = page.min(n - 1);
    let anchor = anchor.min(n - 1);
    if page < anchor {
        return (page, 1);
    }
    let mut start = anchor;
    loop {
        let len = len_at(n, start, wide);
        if page < start + len {
            return (start, len);
        }
        start += len;
    }
}

/// The spread after the one that starts at `start`, if there is one.
pub fn next_start(n: usize, anchor: usize, start: usize, wide: &dyn Fn(usize) -> bool) -> Option<usize> {
    let (s, len) = spread_of(n, anchor, start, wide);
    (s + len < n).then_some(s + len)
}

/// The spread before the one that starts at `start`, if there is one.
pub fn prev_start(n: usize, anchor: usize, start: usize, wide: &dyn Fn(usize) -> bool) -> Option<usize> {
    let (s, _) = spread_of(n, anchor, start, wide);
    (s > 0).then(|| spread_of(n, anchor, s - 1, wide).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: usize) -> bool {
        false
    }

    /// Every spread of a book, from the first page on.
    fn all(n: usize, anchor: usize, wide: &dyn Fn(usize) -> bool) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut at = Some(0);
        while let Some(s) = at {
            out.push(spread_of(n, anchor, s, wide));
            at = next_start(n, anchor, s, wide);
        }
        out
    }

    #[test]
    fn the_cover_stands_alone_when_pairs_start_at_page_two() {
        // 7 pages (0-based): cover, then 1+2, 3+4, 5+6.
        assert_eq!(all(7, 1, &none), [(0, 1), (1, 2), (3, 2), (5, 2)]);
    }

    #[test]
    fn pairs_from_the_first_page() {
        assert_eq!(all(5, 0, &none), [(0, 2), (2, 2), (4, 1)], "an odd last page is alone");
    }

    #[test]
    fn a_later_anchor_leaves_the_front_pages_alone() {
        assert_eq!(all(8, 3, &none), [(0, 1), (1, 1), (2, 1), (3, 2), (5, 2), (7, 1)]);
    }

    #[test]
    fn a_wide_page_stands_alone_and_the_pairing_carries_on_after_it() {
        let wide = |p: usize| p == 3;
        // cover | 1+2 | 3 (wide, alone) | 4+5 | 6+7
        assert_eq!(all(8, 1, &wide), [(0, 1), (1, 2), (3, 1), (4, 2), (6, 2)]);
        // A wide partner also leaves the page before it alone: 2 is single because 3 is wide.
        let wide = |p: usize| p == 2;
        assert_eq!(all(6, 0, &wide), [(0, 2), (2, 1), (3, 2), (5, 1)]);
        let wide = |p: usize| p == 1;
        assert_eq!(all(4, 0, &wide), [(0, 1), (1, 1), (2, 2)], "page 0 cannot pair with a wide page 1");
    }

    #[test]
    fn any_page_finds_its_spread() {
        let n = 9;
        for page in 0..n {
            let (s, len) = spread_of(n, 1, page, &none);
            assert!((s..s + len).contains(&page), "page {page} is in {s}+{len}");
        }
        assert_eq!(spread_of(n, 1, 4, &none), (3, 2));
        assert_eq!(spread_of(n, 1, 100, &none), (7, 2), "past the end is the last spread");
    }

    #[test]
    fn stepping_back_mirrors_stepping_forward() {
        let wide = |p: usize| p == 5;
        let n = 12;
        let forward = all(n, 1, &wide);
        let mut back = Vec::new();
        let mut at = Some(forward.last().unwrap().0);
        while let Some(s) = at {
            back.push(spread_of(n, 1, s, &wide));
            at = prev_start(n, 1, s, &wide);
        }
        back.reverse();
        assert_eq!(back, forward);
    }

    #[test]
    fn edges() {
        assert_eq!(spread_of(0, 0, 0, &none), (0, 1));
        assert_eq!(spread_of(1, 0, 0, &none), (0, 1));
        assert_eq!(next_start(1, 0, 0, &none), None);
        assert_eq!(prev_start(5, 1, 0, &none), None);
        assert_eq!(next_start(2, 0, 0, &none), None, "one spread of two pages");
    }

    #[test]
    fn what_counts_as_wide() {
        assert!(is_wide(2000, 1000));
        assert!(!is_wide(1000, 1000));
        assert!(!is_wide(700, 1000));
        assert!(!is_wide(1200, 1000), "exactly the ratio is still a page");
        assert!(!is_wide(10, 0));
    }
}
