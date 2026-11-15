//! Where the panels and toolbar buttons are. A pure function of the window size, the DPI and which panels are shown, used
//! by drawing and by mouse hit-testing alike, so what you see is what you click.

use crate::gfx::contains;

pub const TOP_H: f32 = 44.0;
pub const LEFT_W: f32 = 300.0;
pub const RIGHT_W: f32 = 360.0;
pub const BTN: f32 = 34.0;
pub const PAD: f32 = 8.0;
/// Pointer within this many (logical) pixels of the window edge brings the panel there.
pub const EDGE_ZONE: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    Prev,
    Next,
    Back,
    Forward,
    Up,
    Sort,
    Show,
    ViewThumbs,
    ViewDetails,
    ViewList,
    Tools,
    Exit,
    PinTop,
    PinLeft,
    PinRight,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct Panels {
    pub top_open: bool,
    pub left_open: bool,
    pub right_open: bool,
    pub pin_top: bool,
    pub pin_left: bool,
    pub pin_right: bool,
}

impl Panels {
    pub fn top_shown(&self) -> bool {
        self.top_open || self.pin_top
    }
    pub fn left_shown(&self) -> bool {
        self.left_open || self.pin_left
    }
    pub fn right_shown(&self) -> bool {
        self.right_open || self.pin_right
    }
}

#[derive(Clone, Debug)]
pub struct Chrome {
    pub top: Option<[f32; 4]>,
    pub left: Option<[f32; 4]>,
    pub right: Option<[f32; 4]>,
    /// Where the items live: the window minus the panels that are pinned (open-but-unpinned panels float over it).
    pub area: [f32; 4],
    pub buttons: Vec<(Btn, [f32; 4])>,
    pub path_box: [f32; 4],
    pub search_box: [f32; 4],
}

impl Chrome {
    pub fn button_at(&self, x: f32, y: f32) -> Option<Btn> {
        self.buttons.iter().find(|(_, r)| contains(*r, x, y)).map(|(b, _)| *b)
    }

    pub fn rect_of(&self, b: Btn) -> Option<[f32; 4]> {
        self.buttons.iter().find(|(k, _)| *k == b).map(|(_, r)| *r)
    }

    /// Is the point on any visible panel or the toolbar?
    pub fn over_panel(&self, x: f32, y: f32) -> bool {
        [self.top, self.left, self.right].into_iter().flatten().any(|r| contains(r, x, y))
    }
}

pub fn layout(w: f32, h: f32, d: f32, p: &Panels) -> Chrome {
    let top_h = TOP_H * d;
    let top = p.top_shown().then_some([0.0, 0.0, w, top_h]);
    // Side panels start under the toolbar when it is shown, so the two never overlap.
    let side_top = if p.top_shown() { top_h } else { 0.0 };
    let left_w = (LEFT_W * d).min(w * 0.4);
    let right_w = (RIGHT_W * d).min(w * 0.45);
    let left = p.left_shown().then_some([0.0, side_top, left_w, h]);
    let right = p.right_shown().then_some([w - right_w, side_top, w, h]);

    let area = [
        if p.pin_left { left_w } else { 0.0 },
        if p.pin_top { top_h } else { 0.0 },
        if p.pin_right { w - right_w } else { w },
        h,
    ];

    let mut buttons = Vec::new();
    let (mut path_box, mut search_box) = ([0.0; 4], [0.0; 4]);
    if top.is_some() {
        let (b, pad, gap) = (BTN * d, PAD * d, 4.0 * d);
        let (y0, y1) = ((top_h - b) / 2.0, (top_h + b) / 2.0);
        let mut x = pad;
        let pin_w = 30.0 * d;
        buttons.push((Btn::PinTop, [x, y0, x + pin_w, y1]));
        x += pin_w + gap * 2.0;
        for k in [Btn::Prev, Btn::Next, Btn::Back, Btn::Forward, Btn::Up] {
            buttons.push((k, [x, y0, x + b, y1]));
            x += b + gap;
        }
        let left_end = x + gap;
        // Right group, from the right edge inwards.
        let mut r = w - pad;
        buttons.push((Btn::Exit, [r - b, y0, r, y1]));
        r -= b + gap * 2.0;
        let tools_w = 64.0 * d;
        buttons.push((Btn::Tools, [r - tools_w, y0, r, y1]));
        r -= tools_w + gap * 2.0;
        // What is left for the search box once the views, Show and Sort and a path box of at least 80 are placed.
        let fixed_left = 3.0 * (b + 1.0) + gap + 84.0 * d + gap + 84.0 * d + gap * 2.0;
        let room = r - left_end - fixed_left - 80.0 * d - gap * 2.0;
        let search_w = if room < 80.0 * d { 0.0 } else { room.min(190.0 * d) };
        if search_w > 0.0 {
            search_box = [r - search_w, y0, r, y1];
            r -= search_w + gap * 2.0;
        }
        for k in [Btn::ViewList, Btn::ViewDetails, Btn::ViewThumbs] {
            buttons.push((k, [r - b, y0, r, y1]));
            r -= b + 1.0;
        }
        r -= gap;
        let show_w = 84.0 * d;
        buttons.push((Btn::Show, [r - show_w, y0, r, y1]));
        r -= show_w + gap;
        let sort_w = 84.0 * d;
        buttons.push((Btn::Sort, [r - sort_w, y0, r, y1]));
        r -= sort_w + gap * 2.0;
        path_box = [left_end, y0, r.max(left_end + 80.0 * d), y1];
    }
    if let Some(l) = left {
        let pw = 40.0 * d;
        buttons.push((Btn::PinLeft, [l[2] - pw - 6.0 * d, l[1] + 4.0 * d, l[2] - 6.0 * d, l[1] + 28.0 * d]));
    }
    if let Some(r) = right {
        let pw = 40.0 * d;
        buttons.push((Btn::PinRight, [r[2] - pw - 6.0 * d, r[1] + 4.0 * d, r[2] - 6.0 * d, r[1] + 28.0 * d]));
    }
    Chrome { top, left, right, area, buttons, path_box, search_box }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_shown_means_the_whole_window_is_for_the_items() {
        let c = layout(1920.0, 1080.0, 1.0, &Panels::default());
        assert!(c.top.is_none() && c.left.is_none() && c.right.is_none());
        assert_eq!(c.area, [0.0, 0.0, 1920.0, 1080.0]);
        assert!(c.buttons.is_empty());
    }

    #[test]
    fn open_panels_float_and_pinned_panels_dock() {
        let open = Panels { top_open: true, left_open: true, right_open: true, ..Default::default() };
        let c = layout(1920.0, 1080.0, 1.0, &open);
        assert!(c.top.is_some() && c.left.is_some() && c.right.is_some());
        assert_eq!(c.area, [0.0, 0.0, 1920.0, 1080.0], "floating panels cover the items");
        let pinned = Panels { pin_top: true, pin_left: true, pin_right: true, ..Default::default() };
        let c = layout(1920.0, 1080.0, 1.0, &pinned);
        assert_eq!(c.area, [LEFT_W, TOP_H, 1920.0 - RIGHT_W, 1080.0]);
        assert_eq!(c.left.unwrap()[1], TOP_H, "side panels start under the toolbar");
    }

    #[test]
    fn toolbar_buttons_fit_and_do_not_overlap() {
        for (w, d) in [(1920.0, 1.0), (1280.0, 1.5), (900.0, 1.0)] {
            let c = layout(w, 800.0, d, &Panels { top_open: true, ..Default::default() });
            let mut rects: Vec<[f32; 4]> = c.buttons.iter().map(|(_, r)| *r).collect();
            rects.push(c.path_box);
            if c.search_box[2] > c.search_box[0] {
                rects.push(c.search_box);
            }
            for (i, a) in rects.iter().enumerate() {
                assert!(a[0] >= 0.0 && a[2] <= w && a[2] > a[0], "{a:?} in {w}");
                for b in &rects[i + 1..] {
                    assert!(a[2] <= b[0] + 0.5 || b[2] <= a[0] + 0.5, "overlap {a:?} / {b:?} at {w}");
                }
            }
            assert!(c.path_box[2] - c.path_box[0] >= 80.0 * d);
        }
    }

    #[test]
    fn buttons_are_found_by_position() {
        let c = layout(1600.0, 900.0, 1.0, &Panels { top_open: true, left_open: true, ..Default::default() });
        let up = c.rect_of(Btn::Up).unwrap();
        assert_eq!(c.button_at((up[0] + up[2]) / 2.0, (up[1] + up[3]) / 2.0), Some(Btn::Up));
        assert!(c.over_panel(10.0, 10.0) && c.over_panel(10.0, 500.0) && !c.over_panel(900.0, 500.0));
        assert!(c.rect_of(Btn::PinLeft).is_some() && c.rect_of(Btn::PinRight).is_none());
    }
}
