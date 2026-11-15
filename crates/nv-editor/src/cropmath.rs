//! The arithmetic behind the crop frame and the straighten line, with no window: which part of the frame a point is on, what
//! dragging it does (free or locked to an aspect ratio), and how much to turn the picture so a line the user drew becomes level.
//! Frames are in 0..1 of the (turned) picture, like `recipe::Crop`; `aspect` is the picture's pixel width / height.

use crate::recipe::Crop;

/// Smallest side of a frame, as a share of the picture.
pub const MIN_SIDE: f32 = 0.04;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ratio {
    Free,
    Original,
    Square,
    R3x2,
    R4x3,
    R5x4,
    R16x9,
}

impl Ratio {
    pub const ALL: [Ratio; 7] = [Ratio::Free, Ratio::Original, Ratio::Square, Ratio::R3x2, Ratio::R4x3, Ratio::R5x4, Ratio::R16x9];

    pub fn next(self) -> Ratio {
        let i = Ratio::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Ratio::ALL[(i + 1) % Ratio::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Ratio::Free => "free",
            Ratio::Original => "as the picture",
            Ratio::Square => "1:1",
            Ratio::R3x2 => "3:2",
            Ratio::R4x3 => "4:3",
            Ratio::R5x4 => "5:4",
            Ratio::R16x9 => "16:9",
        }
    }

    /// Width / height of the frame in pixels, turned to portrait when the picture is portrait. None = free.
    pub fn value(self, aspect: f32) -> Option<f32> {
        let landscape = match self {
            Ratio::Free => return None,
            Ratio::Original => return Some(aspect),
            Ratio::Square => 1.0,
            Ratio::R3x2 => 3.0 / 2.0,
            Ratio::R4x3 => 4.0 / 3.0,
            Ratio::R5x4 => 5.0 / 4.0,
            Ratio::R16x9 => 16.0 / 9.0,
        };
        Some(if aspect < 1.0 { 1.0 / landscape } else { landscape })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    Move,
    N,
    S,
    E,
    W,
    NW,
    NE,
    SW,
    SE,
}

/// The part of the frame under (x, y), `tol` away at most from an edge or corner (all in 0..1 of the picture).
pub fn hit(c: &Crop, x: f32, y: f32, tol: f32) -> Option<Handle> {
    let (l, t, r, b) = (c.x, c.y, c.x + c.w, c.y + c.h);
    let near = |a: f32, b: f32| (a - b).abs() <= tol;
    let (in_x, in_y) = (x >= l - tol && x <= r + tol, y >= t - tol && y <= b + tol);
    if !in_x || !in_y {
        return None;
    }
    let (on_l, on_r, on_t, on_b) = (near(x, l), near(x, r), near(y, t), near(y, b));
    match (on_l, on_r, on_t, on_b) {
        (true, _, true, _) => Some(Handle::NW),
        (_, true, true, _) => Some(Handle::NE),
        (true, _, _, true) => Some(Handle::SW),
        (_, true, _, true) => Some(Handle::SE),
        (true, _, _, _) => Some(Handle::W),
        (_, true, _, _) => Some(Handle::E),
        (_, _, true, _) => Some(Handle::N),
        (_, _, _, true) => Some(Handle::S),
        _ if x > l && x < r && y > t && y < b => Some(Handle::Move),
        _ => None,
    }
}

/// Shrink `(l, t, r, b)` about `anchor` until it lies inside 0..1.
fn keep_inside(mut l: f32, mut t: f32, mut r: f32, mut b: f32, anchor: (f32, f32)) -> (f32, f32, f32, f32) {
    let mut f: f32 = 1.0;
    let mut limit = |edge: f32, a: f32, bound: f32| {
        let (have, room) = ((edge - a).abs(), (bound - a).abs());
        if have > room && have > 1e-6 {
            f = f.min(room / have);
        }
    };
    if l < 0.0 {
        limit(l, anchor.0, 0.0);
    }
    if r > 1.0 {
        limit(r, anchor.0, 1.0);
    }
    if t < 0.0 {
        limit(t, anchor.1, 0.0);
    }
    if b > 1.0 {
        limit(b, anchor.1, 1.0);
    }
    if f < 1.0 {
        l = anchor.0 + (l - anchor.0) * f;
        r = anchor.0 + (r - anchor.0) * f;
        t = anchor.1 + (t - anchor.1) * f;
        b = anchor.1 + (b - anchor.1) * f;
    }
    (l, t, r, b)
}

/// The frame after dragging `h` by (dx, dy) (shares of the picture). With `ratio` (pixel width / height) the frame keeps that
/// shape: a corner keeps the opposite corner fixed, an edge keeps the frame centred on the other axis.
pub fn drag(c: Crop, h: Handle, dx: f32, dy: f32, ratio: Option<f32>, aspect: f32) -> Crop {
    let (mut l, mut t, mut r, mut b) = (c.x, c.y, c.x + c.w, c.y + c.h);
    if h == Handle::Move {
        let dx = dx.clamp(-l, 1.0 - r);
        let dy = dy.clamp(-t, 1.0 - b);
        return Crop { x: l + dx, y: t + dy, w: c.w, h: c.h };
    }
    let west = matches!(h, Handle::W | Handle::NW | Handle::SW);
    let east = matches!(h, Handle::E | Handle::NE | Handle::SE);
    let north = matches!(h, Handle::N | Handle::NW | Handle::NE);
    let south = matches!(h, Handle::S | Handle::SW | Handle::SE);
    if west {
        l = (l + dx).clamp(0.0, r - MIN_SIDE);
    }
    if east {
        r = (r + dx).clamp(l + MIN_SIDE, 1.0);
    }
    if north {
        t = (t + dy).clamp(0.0, b - MIN_SIDE);
    }
    if south {
        b = (b + dy).clamp(t + MIN_SIDE, 1.0);
    }
    if let Some(rt) = ratio {
        let k = rt / aspect.max(1e-6); // frame width / height in shares
        let corner = (west || east) && (north || south);
        if corner {
            let (mut w, mut hh) = (r - l, b - t);
            if w / hh > k { w = hh * k } else { hh = w / k }
            let anchor = (if west { c.x + c.w } else { c.x }, if north { c.y + c.h } else { c.y });
            if west { l = anchor.0 - w } else { r = anchor.0 + w }
            if north { t = anchor.1 - hh } else { b = anchor.1 + hh }
            if west { r = anchor.0 } else { l = anchor.0 }
            if north { b = anchor.1 } else { t = anchor.1 }
            (l, t, r, b) = keep_inside(l, t, r, b, anchor);
        } else if west || east {
            let w = r - l;
            let hh = w / k;
            let cy = c.y + c.h / 2.0;
            (t, b) = (cy - hh / 2.0, cy + hh / 2.0);
            let anchor = (if west { r } else { l }, cy);
            (l, t, r, b) = keep_inside(l, t, r, b, anchor);
        } else {
            let hh = b - t;
            let w = hh * k;
            let cx = c.x + c.w / 2.0;
            (l, r) = (cx - w / 2.0, cx + w / 2.0);
            let anchor = (cx, if north { b } else { t });
            (l, t, r, b) = keep_inside(l, t, r, b, anchor);
        }
    }
    Crop { x: l, y: t, w: r - l, h: b - t }
}

/// The largest frame of the shape `ratio` that fits in `c`, centred in it (what choosing a ratio does to the current frame).
pub fn fit_ratio(c: Crop, ratio: f32, aspect: f32) -> Crop {
    let k = ratio / aspect.max(1e-6);
    let (mut w, mut h) = (c.w, c.h);
    if w / h > k { w = h * k } else { h = w / k }
    Crop { x: c.x + (c.w - w) / 2.0, y: c.y + (c.h - h) / 2.0, w, h }
}

/// How many degrees to turn the picture clockwise so the line from `p0` to `p1` (screen points, y down) becomes level. A line
/// closer to vertical than to level is made vertical instead.
pub fn level_angle(p0: (f32, f32), p1: (f32, f32)) -> f32 {
    let mut a = (p1.1 - p0.1).atan2(p1.0 - p0.0).to_degrees();
    while a > 45.0 {
        a -= 90.0;
    }
    while a <= -45.0 {
        a += 90.0;
    }
    -a
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Crop = Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

    #[test]
    fn hits_find_corners_edges_and_the_inside() {
        let c = Crop { x: 0.2, y: 0.2, w: 0.6, h: 0.6 };
        assert_eq!(hit(&c, 0.2, 0.2, 0.03), Some(Handle::NW));
        assert_eq!(hit(&c, 0.8, 0.8, 0.03), Some(Handle::SE));
        assert_eq!(hit(&c, 0.5, 0.2, 0.03), Some(Handle::N));
        assert_eq!(hit(&c, 0.8, 0.5, 0.03), Some(Handle::E));
        assert_eq!(hit(&c, 0.5, 0.5, 0.03), Some(Handle::Move));
        assert_eq!(hit(&c, 0.95, 0.5, 0.03), None);
    }

    #[test]
    fn free_drag_moves_one_edge_and_stays_inside() {
        let c = drag(FULL, Handle::W, 0.25, 0.0, None, 1.5);
        assert_eq!((c.x, c.w, c.y, c.h), (0.25, 0.75, 0.0, 1.0));
        let c = drag(FULL, Handle::SE, -0.5, -0.5, None, 1.5);
        assert_eq!((c.w, c.h), (0.5, 0.5));
        // Cannot be dragged past the opposite edge or out of the picture.
        let c = drag(FULL, Handle::E, -2.0, 0.0, None, 1.5);
        assert!((c.w - MIN_SIDE).abs() < 1e-6);
        let c = drag(Crop { x: 0.1, y: 0.1, w: 0.3, h: 0.3 }, Handle::Move, -1.0, -1.0, None, 1.5);
        assert_eq!((c.x, c.y, c.w, c.h), (0.0, 0.0, 0.3, 0.3));
    }

    #[test]
    fn locked_ratio_keeps_its_shape() {
        // A 3:2 picture; a 1:1 frame must have equal pixel sides: w * 1.5 == h.
        let aspect = 1.5;
        let c = fit_ratio(FULL, 1.0, aspect);
        assert!((c.w * aspect - c.h).abs() < 1e-5 && (c.x - (1.0 - c.w) / 2.0).abs() < 1e-5, "{c:?}");
        for h in [Handle::NW, Handle::SE, Handle::E, Handle::N] {
            let d = drag(c, h, 0.07, -0.05, Some(1.0), aspect);
            assert!((d.w * aspect - d.h).abs() < 1e-4, "{h:?}: {d:?}");
            assert!(d.x >= -1e-6 && d.y >= -1e-6 && d.x + d.w <= 1.0 + 1e-6 && d.y + d.h <= 1.0 + 1e-6, "{h:?} left the picture: {d:?}");
        }
        // Corner drags keep the opposite corner where it was.
        let d = drag(c, Handle::SE, -0.1, -0.1, Some(1.0), aspect);
        assert!((d.x - c.x).abs() < 1e-6 && (d.y - c.y).abs() < 1e-6);
    }

    #[test]
    fn ratios_follow_the_picture_orientation() {
        assert_eq!(Ratio::R3x2.value(1.5), Some(1.5));
        assert_eq!(Ratio::R3x2.value(2.0 / 3.0), Some(2.0 / 3.0));
        assert_eq!(Ratio::Free.value(1.5), None);
        assert_eq!(Ratio::Original.value(1.3), Some(1.3));
        assert_eq!(Ratio::R16x9.next(), Ratio::Free);
    }

    #[test]
    fn the_level_line_turns_the_picture_to_make_it_horizontal() {
        // Slopes down to the right by 10 degrees: turn 10 degrees anticlockwise.
        let a = level_angle((0.0, 0.0), (100.0, 100.0 * 10f32.to_radians().tan()));
        assert!((a + 10.0).abs() < 0.01, "{a}");
        // Slopes up to the right: turn clockwise.
        assert!(level_angle((0.0, 0.0), (100.0, -5.0)) > 0.0);
        // A nearly vertical line is made vertical (small turn, not 80 degrees).
        let a = level_angle((0.0, 0.0), (-17.4, 100.0));
        assert!(a.abs() < 10.5 && a.abs() > 9.0, "{a}");
        assert_eq!(level_angle((0.0, 0.0), (50.0, 0.0)), 0.0);
    }
}
