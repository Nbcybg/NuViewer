//! The first frames, painted on the CPU with plain GDI while the Direct3D device is still being created (~220-330 ms on
//! this PC; the thumbnails themselves are usually ready in 40-100 ms). The gallery's visible tiles - background, folder
//! glyphs, names and every thumbnail as soon as it is decoded - are drawn into a bitmap and shown a tile at a time. When the
//! GPU arrives the same layout is drawn by the renderer, so nothing jumps.
//!
//! Only the plain start-up case is handled here (thumbnail view, no panel pinned); anything else simply waits for the GPU.

use crate::app::App;
use crate::gfx::{line_h, pal};
use crate::state::ViewMode;
use nv_codecs::Image;
use nv_win::text;

/// A BGRA bitmap the size of the window.
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

fn bgr(hex: u32) -> [u8; 3] {
    [(hex & 0xFF) as u8, ((hex >> 8) & 0xFF) as u8, ((hex >> 16) & 0xFF) as u8]
}

impl Canvas {
    pub fn new(w: i32, h: i32, bg: u32) -> Canvas {
        let c = bgr(bg);
        let mut px = vec![255u8; w as usize * h as usize * 4];
        for p in px.chunks_exact_mut(4) {
            p[0] = c[0];
            p[1] = c[1];
            p[2] = c[2];
        }
        Canvas { w, h, px }
    }

    /// Fill a rectangle (clipped to the canvas) with an opaque colour.
    pub fn fill(&mut self, r: [f32; 4], hex: u32) {
        let c = bgr(hex);
        let (x0, y0) = ((r[0].round() as i32).max(0), (r[1].round() as i32).max(0));
        let (x1, y1) = ((r[2].round() as i32).min(self.w), (r[3].round() as i32).min(self.h));
        for y in y0..y1 {
            let row = (y * self.w) as usize * 4;
            for x in x0..x1 {
                let i = row + x as usize * 4;
                self.px[i] = c[0];
                self.px[i + 1] = c[1];
                self.px[i + 2] = c[2];
            }
        }
    }

    pub fn outline(&mut self, r: [f32; 4], t: f32, hex: u32) {
        self.fill([r[0], r[1], r[2], r[1] + t], hex);
        self.fill([r[0], r[3] - t, r[2], r[3]], hex);
        self.fill([r[0], r[1] + t, r[0] + t, r[3] - t], hex);
        self.fill([r[2] - t, r[1] + t, r[2], r[3] - t], hex);
    }

    /// Draw a picture scaled (bilinear) into the integer rectangle `r`, over whatever is there (alpha respected).
    pub fn blit(&mut self, r: [i32; 4], img: &Image) {
        let (dw, dh) = (r[2] - r[0], r[3] - r[1]);
        let (sw, sh) = (img.width as i32, img.height as i32);
        if dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0 || img.rgba.len() < (sw * sh * 4) as usize {
            return;
        }
        let (kx, ky) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
        for y in 0..dh {
            let dy = r[1] + y;
            if dy < 0 || dy >= self.h {
                continue;
            }
            let fy = ((y as f32 + 0.5) * ky - 0.5).clamp(0.0, (sh - 1) as f32);
            let (y0, wy) = (fy as i32, fy.fract());
            let y1 = (y0 + 1).min(sh - 1);
            for x in 0..dw {
                let dx = r[0] + x;
                if dx < 0 || dx >= self.w {
                    continue;
                }
                let fx = ((x as f32 + 0.5) * kx - 0.5).clamp(0.0, (sw - 1) as f32);
                let (x0, wx) = (fx as i32, fx.fract());
                let x1 = (x0 + 1).min(sw - 1);
                let at = |xx: i32, yy: i32, c: usize| img.rgba[((yy * sw + xx) * 4) as usize + c] as f32;
                let mut out = [0f32; 4];
                for (c, o) in out.iter_mut().enumerate() {
                    let top = at(x0, y0, c) * (1.0 - wx) + at(x1, y0, c) * wx;
                    let bot = at(x0, y1, c) * (1.0 - wx) + at(x1, y1, c) * wx;
                    *o = top * (1.0 - wy) + bot * wy;
                }
                let a = out[3] / 255.0;
                let i = ((dy * self.w + dx) as usize) * 4;
                self.px[i] = (out[2] * a + self.px[i] as f32 * (1.0 - a)) as u8;
                self.px[i + 1] = (out[1] * a + self.px[i + 1] as f32 * (1.0 - a)) as u8;
                self.px[i + 2] = (out[0] * a + self.px[i + 2] as f32 * (1.0 - a)) as u8;
            }
        }
    }

    /// Draw a white-on-transparent text bitmap tinted `hex` with its top-left at (x, y), inside `clip`.
    pub fn text(&mut self, x: i32, y: i32, bmp: &text::TextBitmap, hex: u32, clip: [i32; 4]) {
        let c = bgr(hex);
        for by in 0..bmp.height as i32 {
            let dy = y + by;
            if dy < clip[1].max(0) || dy >= clip[3].min(self.h) {
                continue;
            }
            for bx in 0..bmp.width as i32 {
                let dx = x + bx;
                if dx < clip[0].max(0) || dx >= clip[2].min(self.w) {
                    continue;
                }
                let a = bmp.rgba[((by * bmp.width as i32 + bx) * 4 + 3) as usize] as u32;
                if a == 0 {
                    continue;
                }
                let i = ((dy * self.w + dx) as usize) * 4;
                for k in 0..3 {
                    self.px[i + k] = ((c[k] as u32 * a + self.px[i + k] as u32 * (255 - a)) / 255) as u8;
                }
            }
        }
    }

    /// The pixels of a rectangle as a tight BGRA block (for painting just that part of the window).
    pub fn region(&self, r: [i32; 4]) -> Option<(i32, i32, i32, i32, Vec<u8>)> {
        let (x0, y0, x1, y1) = (r[0].max(0), r[1].max(0), r[2].min(self.w), r[3].min(self.h));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let mut out = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            let s = ((y * self.w + x0) as usize) * 4;
            out.extend_from_slice(&self.px[s..s + (x1 - x0) as usize * 4]);
        }
        Some((x0, y0, x1 - x0, y1 - y0, out))
    }
}

impl App {
    /// Can the first frames be painted on the CPU? Only the plain start-up layout is.
    pub fn cpu_possible(&self) -> bool {
        self.renderer.is_none() && self.mode == ViewMode::Thumbs && !self.panels.pin_top && !self.panels.pin_left && !self.panels.pin_right && self.listing.len() > 0
    }

    /// Paint everything that is visible (all tiles, with the thumbnails that are ready) and show it.
    pub fn cpu_full(&mut self) {
        if !self.cpu_possible() {
            return;
        }
        let mut canvas = Canvas::new(self.size.0 as i32, self.size.1 as i32, pal::BG);
        let g = self.grid();
        let scroll = g.clamp(self.scroll);
        for i in g.visible(scroll) {
            self.cpu_tile_into(&mut canvas, &g, scroll, i);
        }
        nv_win::paint_bgra_at(self.hwnd, 0, 0, canvas.w, canvas.h, &canvas.px);
        if self.timing.cpu_frame_ms.is_none() {
            self.timing.cpu_frame_ms = Some(self.ms_since_start());
        }
        self.cpu = Some(canvas);
    }

    /// A thumbnail arrived: redraw its tile and show just that tile.
    pub fn cpu_thumb(&mut self, id: u64) {
        if self.cpu.is_none() || !self.cpu_possible() {
            return;
        }
        let g = self.grid();
        let scroll = g.clamp(self.scroll);
        let Some(i) = g.visible(scroll).find(|&i| self.listing.get(i).map(|e| e.id == id).unwrap_or(false)) else { return };
        let Some(mut canvas) = self.cpu.take() else { return };
        self.cpu_tile_into(&mut canvas, &g, scroll, i);
        let rc = g.rect(i, scroll);
        if let Some((x, y, w, h, bytes)) = canvas.region([rc[0] as i32 - 1, rc[1] as i32 - 1, rc[2] as i32 + 1, rc[3] as i32 + 1]) {
            nv_win::paint_bgra_at(self.hwnd, x, y, w, h, &bytes);
        }
        self.cpu = Some(canvas);
    }

    /// One tile exactly as the renderer will draw it.
    fn cpu_tile_into(&mut self, c: &mut Canvas, g: &crate::grid::Grid, scroll: f32, i: usize) {
        let d = self.dpi;
        let (cell, label_h) = (self.cell_px(), self.label_h());
        let Some(e) = self.listing.get(i) else { return };
        let (id, is_dir, has_thumb, name) = (e.id, e.is_dir(), e.has_thumb(), e.name.clone());
        let rc = g.rect(i, scroll);
        if rc[3] < 0.0 || rc[1] > c.h as f32 {
            return;
        }
        c.fill(rc, pal::TILE);
        let pad = 8.0 * d;
        let inner = [rc[0] + pad, rc[1] + pad, rc[0] + cell - pad, rc[1] + cell - pad];
        if is_dir {
            let (w, h) = (inner[2] - inner[0], inner[3] - inner[1]);
            let s = w.min(h) * 0.62;
            let (cx, cy) = ((inner[0] + inner[2]) / 2.0, (inner[1] + inner[3]) / 2.0);
            let (x0, y0) = (cx - s / 2.0, cy - s * 0.36);
            c.fill([x0, y0, x0 + s * 0.42, y0 + s * 0.14], 0xC99A33);
            c.fill([x0, y0 + s * 0.1, x0 + s, y0 + s * 0.72], pal::FOLDER);
            c.fill([x0, y0 + s * 0.22, x0 + s, y0 + s * 0.72], 0xF2C75E);
        } else if let Some((_, img)) = self.staged.iter().find(|(sid, _)| *sid == id) {
            let (tw, th) = (img.width as f32, img.height as f32);
            let (cw, ch) = (inner[2] - inner[0], inner[3] - inner[1]);
            let s = (cw / tw).min(ch / th).min(1.0);
            let (w, h) = ((tw * s).round(), (th * s).round());
            let (l, t) = ((inner[0] + (cw - w) / 2.0).round(), (inner[1] + (ch - h) / 2.0).round());
            c.blit([l as i32, t as i32, (l + w) as i32, (t + h) as i32], img);
        } else if !has_thumb {
            // A card with the extension would need its text; the plain card is enough for the first frames.
            let (w, h) = (inner[2] - inner[0], inner[3] - inner[1]);
            let (cw, ch) = (w.min(h) * 0.56, w.min(h) * 0.7);
            let (cx, cy) = ((inner[0] + inner[2]) / 2.0, (inner[1] + inner[3]) / 2.0);
            c.fill([cx - cw / 2.0, cy - ch / 2.0, cx + cw / 2.0, cy + ch / 2.0], 0x3A3D42);
        }
        if label_h > 0.0 {
            let px = (13.0 * d).round() as i32;
            let fitted = self.text.fit(&name, px, (rc[2] - rc[0] - 12.0 * d) as i32);
            let bmp = text::render_lines(&[fitted], px, 1);
            let (tx, ty) = ((rc[0] + 6.0 * d).round() as i32 - 1, (rc[1] + cell + 3.0 * d).round() as i32 - 1);
            c.text(tx, ty, &bmp, pal::TEXT, [rc[0] as i32, rc[1] as i32, rc[2] as i32, rc[3] as i32]);
            let _ = line_h(px);
        }
        if self.sel.cursor == Some(i) && !self.sel.ids.contains(&id) {
            c.outline(rc, 1.0, 0xB8C4D8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_clip_to_the_canvas() {
        let mut c = Canvas::new(10, 10, 0x000000);
        c.fill([-5.0, -5.0, 3.0, 3.0], 0xFF0000);
        c.fill([8.0, 8.0, 20.0, 20.0], 0x00FF00);
        assert_eq!(&c.px[0..3], &[0, 0, 255], "BGR of red at (0,0)");
        assert_eq!(&c.px[(9 * 10 + 9) * 4..][..3], &[0, 255, 0]);
        assert_eq!(&c.px[(5 * 10 + 5) * 4..][..3], &[0, 0, 0]);
    }

    #[test]
    fn a_picture_is_scaled_and_blended() {
        let mut c = Canvas::new(8, 8, 0x000000);
        let mut img = Image::new(2, 2, vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 255, 0, 0, 0]);
        c.blit([0, 0, 4, 4], &img);
        assert_eq!(&c.px[(0 * 8 + 1) * 4..][..3], &[0, 0, 255], "opaque red on top");
        let bottom = &c.px[(3 * 8 + 1) * 4..][..3];
        assert!(bottom[2] < 40, "the transparent half leaves the background: {bottom:?}");
        img.width = 0;
        c.blit([0, 0, 4, 4], &img); // an empty picture is ignored
    }

    #[test]
    fn regions_are_cut_out_tightly() {
        let mut c = Canvas::new(6, 4, 0x000000);
        c.fill([2.0, 1.0, 4.0, 3.0], 0xFFFFFF);
        let (x, y, w, h, bytes) = c.region([2, 1, 4, 3]).unwrap();
        assert_eq!((x, y, w, h), (2, 1, 2, 2));
        assert!(bytes.chunks_exact(4).all(|p| p[..3] == [255, 255, 255]));
        assert!(c.region([10, 10, 12, 12]).is_none());
        let (x, _, w, _, _) = c.region([-3, 0, 2, 1]).unwrap();
        assert_eq!((x, w), (0, 2));
    }
}
