//! Stamping a watermark (a ready RGBA picture: rendered text or a logo) onto a picture.

use crate::raster::Pixels;
use crate::resample::{self, Algo};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Centre,
    /// Repeated over the whole picture.
    Tile,
}

impl Place {
    pub const ALL: [Place; 6] = [Place::BottomRight, Place::BottomLeft, Place::TopRight, Place::TopLeft, Place::Centre, Place::Tile];

    pub fn label(self) -> &'static str {
        match self {
            Place::TopLeft => "top left",
            Place::TopRight => "top right",
            Place::BottomLeft => "bottom left",
            Place::BottomRight => "bottom right",
            Place::Centre => "centre",
            Place::Tile => "tiled",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Place::TopLeft => "tl",
            Place::TopRight => "tr",
            Place::BottomLeft => "bl",
            Place::BottomRight => "br",
            Place::Centre => "centre",
            Place::Tile => "tile",
        }
    }

    pub fn from_id(s: &str) -> Option<Place> {
        Place::ALL.into_iter().find(|p| p.id() == s)
    }

    pub fn next(self) -> Place {
        let i = Place::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Place::ALL[(i + 1) % Place::ALL.len()]
    }
}

fn blend(dst: &mut [u8], src: &[u8], opacity: f32) {
    let a = src[3] as f32 / 255.0 * opacity;
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    for c in 0..3 {
        let v = (src[c] as f32 * a + dst[c] as f32 * da * (1.0 - a)) / out_a.max(1e-6);
        dst[c] = (v + 0.5).clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0 + 0.5).min(255.0) as u8;
}

/// Put `mark` on `p`. The mark is scaled to `width_frac` of the picture's width (`mark` is re-sampled, not blown up from a small
/// bitmap when it is smaller than needed: render it big), `opacity` is 0..1, `margin_frac` is the gap to the edge (fraction of the
/// width).
pub fn stamp(p: &mut Pixels, mark: &Pixels, place: Place, width_frac: f32, opacity: f32, margin_frac: f32) {
    if mark.w == 0 || mark.h == 0 || p.w == 0 {
        return;
    }
    let target_w = ((p.w as f32 * width_frac.clamp(0.02, 1.0)).round() as u32).max(2);
    let target_h = ((mark.h as f32 * target_w as f32 / mark.w as f32).round() as u32).max(1);
    let m = resample::resize(mark, target_w, target_h, Algo::Lanczos);
    let margin = (p.w as f32 * margin_frac) as i32;
    let (pw, ph, mw, mh) = (p.w as i32, p.h as i32, m.w as i32, m.h as i32);
    let mut put = |ox: i32, oy: i32| {
        for y in 0..mh {
            let py = oy + y;
            if py < 0 || py >= ph {
                continue;
            }
            for x in 0..mw {
                let px = ox + x;
                if px < 0 || px >= pw {
                    continue;
                }
                let (s, d) = (((y * mw + x) * 4) as usize, ((py * pw + px) * 4) as usize);
                blend(&mut p.rgba[d..d + 4], &m.rgba[s..s + 4], opacity);
            }
        }
    };
    match place {
        Place::TopLeft => put(margin, margin),
        Place::TopRight => put(pw - mw - margin, margin),
        Place::BottomLeft => put(margin, ph - mh - margin),
        Place::BottomRight => put(pw - mw - margin, ph - mh - margin),
        Place::Centre => put((pw - mw) / 2, (ph - mh) / 2),
        Place::Tile => {
            let (sx, sy) = (mw * 2, mh * 3);
            let mut row = 0;
            let mut y = 0;
            while y < ph {
                let mut x = if row % 2 == 0 { 0 } else { sx / 2 } - mw / 2;
                while x < pw {
                    put(x, y);
                    x += sx;
                }
                y += sy;
                row += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 4]) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|_| c).collect())
    }

    #[test]
    fn a_corner_mark_lands_in_its_corner_with_its_opacity() {
        let mut p = solid(200, 100, [0, 0, 0, 255]);
        let mark = solid(20, 10, [255, 255, 255, 255]);
        stamp(&mut p, &mark, Place::BottomRight, 0.2, 0.5, 0.02);
        let at = |x: usize, y: usize| p.rgba[(y * 200 + x) * 4];
        // 40 px wide, 20 tall, 4 px from the edges.
        assert!((126..=129).contains(&at(190, 85)), "half-white: {}", at(190, 85));
        assert_eq!(at(10, 10), 0);
        assert_eq!(at(150, 85), 0, "left of the mark");
        let mut q = solid(200, 100, [0, 0, 0, 255]);
        stamp(&mut q, &mark, Place::TopLeft, 0.2, 1.0, 0.0);
        assert_eq!(q.rgba[(2 * 200 + 2) * 4], 255);
    }

    #[test]
    fn transparent_parts_of_a_mark_leave_the_picture_alone_and_tiles_repeat() {
        let mut p = solid(120, 60, [10, 20, 30, 255]);
        let clear = solid(10, 10, [255, 0, 0, 0]);
        stamp(&mut p, &clear, Place::Centre, 0.3, 1.0, 0.0);
        assert!(p.rgba.chunks_exact(4).all(|px| px == [10, 20, 30, 255]));
        let mark = solid(10, 4, [255, 255, 255, 255]);
        let mut t = solid(120, 60, [0, 0, 0, 255]);
        stamp(&mut t, &mark, Place::Tile, 0.1, 1.0, 0.0);
        let bright = t.rgba.chunks_exact(4).filter(|px| px[0] > 200).count();
        assert!(bright > 12 * 5 * 4, "many copies: {bright}");
    }
}
