//! A CPU rasteriser for the draw list of a pose: textured triangles with vertex colour, the four Spine blend modes, premultiplied
//! pages, and clipping polygons. Output is straight RGBA8 with real transparency (nothing is composed onto black or white).

use crate::model::BlendMode;
use crate::pose::DrawItem;
use crate::Rig;
use std::collections::HashMap;

/// Where the world is on the canvas: the world point `(cx, cy)` is the canvas centre, `scale` canvas pixels per world unit, y up in the world.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub cx: f32,
    pub cy: f32,
    pub scale: f32,
}

impl View {
    /// A view that shows the box `[min x, min y, max x, max y]` whole in a canvas of `w` x `h` with `margin` (fraction of the size) around it.
    pub fn fit(b: [f32; 4], w: usize, h: usize, margin: f32) -> View {
        let (bw, bh) = ((b[2] - b[0]).max(1e-3), (b[3] - b[1]).max(1e-3));
        let s = ((w as f32) / bw).min((h as f32) / bh) * (1.0 - margin * 2.0).max(0.1);
        View { cx: (b[0] + b[2]) / 2.0, cy: (b[1] + b[3]) / 2.0, scale: s }
    }

    pub fn to_canvas(&self, w: usize, h: usize, p: [f32; 2]) -> [f32; 2] {
        [(p[0] - self.cx) * self.scale + w as f32 / 2.0, (self.cy - p[1]) * self.scale + h as f32 / 2.0]
    }
}

struct Page<'a> {
    w: usize,
    h: usize,
    rgba: &'a [u8],
}

fn sample(p: &Page, u: f32, v: f32) -> [f32; 4] {
    let (x, y) = (u * p.w as f32 - 0.5, v * p.h as f32 - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (ix, iy) = (x0 as i64, y0 as i64);
    let (w, h) = (p.w as i64, p.h as i64);
    // Four texels with the edge clamped; the common case (all inside) needs no clamping.
    let (xa, xb, ya, yb) = if ix >= 0 && iy >= 0 && ix + 1 < w && iy + 1 < h { (ix as usize, ix as usize + 1, iy as usize, iy as usize + 1) } else { (ix.clamp(0, w - 1) as usize, (ix + 1).clamp(0, w - 1) as usize, iy.clamp(0, h - 1) as usize, (iy + 1).clamp(0, h - 1) as usize) };
    let row_a = &p.rgba[ya * p.w * 4..(ya + 1) * p.w * 4];
    let row_b = &p.rgba[yb * p.w * 4..(yb + 1) * p.w * 4];
    let (w00, w10, w01, w11) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy);
    let k = 1.0 / 255.0;
    let mut out = [0.0f32; 4];
    for c in 0..4 {
        out[c] = (row_a[xa * 4 + c] as f32 * w00 + row_a[xb * 4 + c] as f32 * w10 + row_b[xa * 4 + c] as f32 * w01 + row_b[xb * 4 + c] as f32 * w11) * k;
    }
    out
}

/// Even-odd coverage of a polygon (canvas coordinates) as a full-canvas mask.
fn polygon_mask(poly: &[[f32; 2]], w: usize, h: usize) -> Vec<u8> {
    let mut mask = vec![0u8; w * h];
    if poly.len() < 3 {
        return mask;
    }
    let (ymin, ymax) = poly.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let (y0, y1) = ((ymin.floor().max(0.0)) as usize, (ymax.ceil().min(h as f32)) as usize);
    let mut xs: Vec<f32> = Vec::new();
    for y in y0..y1 {
        let yc = y as f32 + 0.5;
        xs.clear();
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            if (p[1] <= yc && q[1] > yc) || (q[1] <= yc && p[1] > yc) {
                xs.push(p[0] + (yc - p[1]) / (q[1] - p[1]) * (q[0] - p[0]));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks_exact(2) {
            let (xa, xb) = ((pair[0] - 0.5).ceil().max(0.0) as usize, ((pair[1] - 0.5).floor() + 1.0).min(w as f32).max(0.0) as usize);
            for x in xa..xb.min(w) {
                mask[y * w + x] = 255;
            }
        }
    }
    mask
}

/// Coverage of a set of triangles (union), inverted when asked.
fn triangle_mask(g: &crate::pose::MaskGeom, w: usize, h: usize, view: View) -> Vec<u8> {
    let mut m = vec![0u8; w * h];
    for t in &g.tris {
        let p: Vec<[f32; 2]> = t.iter().map(|v| view.to_canvas(w, h, *v)).collect();
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
        if area.abs() < 1e-6 {
            continue;
        }
        let (x0, x1) = ((p.iter().map(|q| q[0]).fold(f32::MAX, f32::min).floor().max(0.0)) as usize, (p.iter().map(|q| q[0]).fold(f32::MIN, f32::max).ceil().min(w as f32)) as usize);
        let (y0, y1) = ((p.iter().map(|q| q[1]).fold(f32::MAX, f32::min).floor().max(0.0)) as usize, (p.iter().map(|q| q[1]).fold(f32::MIN, f32::max).ceil().min(h as f32)) as usize);
        let inv = 1.0 / area;
        for y in y0..y1 {
            for x in x0..x1 {
                let (xc, yc) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p[1][0] - xc) * (p[2][1] - yc) - (p[2][0] - xc) * (p[1][1] - yc)) * inv;
                let w1 = ((p[2][0] - xc) * (p[0][1] - yc) - (p[0][0] - xc) * (p[2][1] - yc)) * inv;
                if w0 >= -1e-4 && w1 >= -1e-4 && 1.0 - w0 - w1 >= -1e-4 {
                    m[y * w + x] = 255;
                }
            }
        }
    }
    if g.inverted {
        for v in &mut m {
            *v = 255 - *v;
        }
    }
    m
}

/// Draw `items` (back to front) into straight RGBA8 of `w` x `h`. `ssaa` = 1 or 2 (2 renders at double size and averages: smoother edges).
/// `background`, when given, is put behind everything (and the result is opaque).
pub fn render(rig: &Rig, items: &[DrawItem], w: usize, h: usize, view: View, ssaa: usize, background: Option<[f32; 3]>) -> Vec<u8> {
    let pages: Vec<Option<std::sync::Arc<crate::atlas::PageImage>>> = rig.atlas.pages.iter().map(|p| p.image.clone()).collect();
    render_pages(&pages, items, w, h, view, ssaa, background)
}

/// `render` with the page pictures given directly (Live2D has textures, not an atlas).
pub fn render_pages(page_images: &[Option<std::sync::Arc<crate::atlas::PageImage>>], items: &[DrawItem], w: usize, h: usize, view: View, ssaa: usize, background: Option<[f32; 3]>) -> Vec<u8> {
    let ss = ssaa.clamp(1, 3);
    let (rw, rh) = (w * ss, h * ss);
    let rv = View { scale: view.scale * ss as f32, ..view };
    let pages: Vec<Option<Page>> = page_images.iter().map(|p| p.as_ref().map(|i| Page { w: i.width as usize, h: i.height as usize, rgba: &i.rgba })).collect();
    // Masks first (each distinct one once), then the triangles of every item in screen space.
    let mut mask_store: Vec<Vec<u8>> = Vec::new();
    let mut clip_ids: HashMap<*const Vec<[f32; 2]>, usize> = HashMap::new();
    let mut geom_ids: HashMap<*const crate::pose::MaskGeom, usize> = HashMap::new();
    struct Prep {
        item: usize,
        sv: Vec<[f32; 2]>,
        clip: Option<usize>,
        geom: Option<usize>,
        /// Rows this item can touch.
        rows: (usize, usize),
    }
    let mut preps: Vec<Prep> = Vec::with_capacity(items.len());
    for (ii, item) in items.iter().enumerate() {
        if !matches!(pages.get(item.page), Some(Some(_))) {
            continue;
        }
        let clip = item.clip.as_ref().map(|poly| {
            *clip_ids.entry(std::sync::Arc::as_ptr(poly)).or_insert_with(|| {
                let cp: Vec<[f32; 2]> = poly.iter().map(|p| rv.to_canvas(rw, rh, *p)).collect();
                mask_store.push(polygon_mask(&cp, rw, rh));
                mask_store.len() - 1
            })
        });
        let geom = item.mask.as_ref().map(|g| {
            *geom_ids.entry(std::sync::Arc::as_ptr(g)).or_insert_with(|| {
                mask_store.push(triangle_mask(g, rw, rh, rv));
                mask_store.len() - 1
            })
        });
        let sv: Vec<[f32; 2]> = item.verts.iter().map(|p| rv.to_canvas(rw, rh, *p)).collect();
        let (ymin, ymax) = sv.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        if ymax < 0.0 || ymin > rh as f32 {
            continue;
        }
        preps.push(Prep { item: ii, sv, clip, geom, rows: (ymin.floor().max(0.0) as usize, (ymax.ceil().max(0.0) as usize).min(rh)) });
    }
    // Only the rectangle that holds something is drawn (a skeleton in a big window leaves most of it empty): memory, drawing and the
    // final conversion all stay inside it. Its corners sit on whole output pixels so the averaging of supersamples lines up.
    let (mut rx0, mut ry0, mut rx1, mut ry1) = (rw as f32, rh as f32, 0f32, 0f32);
    for pr in &preps {
        for p in &pr.sv {
            if p[0].is_finite() && p[1].is_finite() {
                rx0 = rx0.min(p[0]);
                rx1 = rx1.max(p[0]);
                ry0 = ry0.min(p[1]);
                ry1 = ry1.max(p[1]);
            }
        }
    }
    let snap_down = |v: f32, max: usize| (v.floor().max(0.0) as usize).min(max) / ss * ss;
    let snap_up = |v: f32, max: usize| ((v.ceil().max(0.0) as usize).min(max)).div_ceil(ss) * ss;
    let (rx0, ry0, rx1, ry1) = (snap_down(rx0 - 1.0, rw), snap_down(ry0 - 1.0, rh), snap_up(rx1 + 1.0, rw).min(rw), snap_up(ry1 + 1.0, rh).min(rh));
    let (rect_w, rect_h) = (rx1.saturating_sub(rx0), ry1.saturating_sub(ry0));
    let mut px = vec![0f32; rect_w * rect_h * 4]; // premultiplied
    // Bands of rows on all cores: every band draws every item that reaches it, in order, so the result is the same as one pass.
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 12);
    let band_rows = rect_h.div_ceil(threads * 3).max(4);
    let masks = &mask_store;
    let (preps_ref, pages_ref) = (&preps, &pages);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let bands: Vec<std::sync::Mutex<&mut [f32]>> = if rect_w * rect_h == 0 { Vec::new() } else { px.chunks_mut(band_rows * rect_w * 4).map(std::sync::Mutex::new).collect() };
    let (next, bands) = (&next, &bands);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(move || loop {
                let b = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if b >= bands.len() {
                    break;
                }
                let (y_lo, y_hi) = (ry0 + b * band_rows, (ry0 + (b + 1) * band_rows).min(ry1));
                let mut guard = bands[b].lock().unwrap();
                for pr in preps_ref.iter() {
                    if pr.rows.1 <= y_lo || pr.rows.0 >= y_hi {
                        continue;
                    }
                    let item = &items[pr.item];
                    let Some(Some(page)) = pages_ref.get(item.page) else { continue };
                    let (mask, mask2) = (pr.clip.map(|i| &masks[i]), pr.geom.map(|i| &masks[i]));
                    for t in item.tris.chunks_exact(3) {
                        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
                        if i0 >= pr.sv.len() || i1 >= pr.sv.len() || i2 >= pr.sv.len() {
                            continue;
                        }
                        fill_triangle(&mut guard, rw, (rx0, rect_w), (y_lo, y_hi), [pr.sv[i0], pr.sv[i1], pr.sv[i2]], [item.uvs[i0], item.uvs[i1], item.uvs[i2]], page, item, mask, mask2);
                    }
                }
            });
        }
    });
    // Down to the output size, to straight RGBA8 (inside the drawn rectangle; the rest is the background or transparent).
    let mut out = vec![0u8; w * h * 4];
    if let Some(bg) = background {
        let v = [(bg[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (bg[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (bg[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, 255];
        for o in out.chunks_exact_mut(4) {
            o.copy_from_slice(&v);
        }
    }
    let n = 1.0 / (ss * ss) as f32;
    let (ox, oy) = (rx0 / ss, ry0 / ss);
    let out_rows = rect_h / ss;
    if out_rows > 0 && rect_w > 0 {
        // Rows of the output on all cores (the average of the supersamples, premultiplied -> straight).
        let per = out_rows.div_ceil(threads * 2).max(4);
        let px = &px;
        std::thread::scope(|sc| {
            for (ci, chunk) in out[oy * w * 4..(oy + out_rows) * w * 4].chunks_mut(per * w * 4).enumerate() {
                sc.spawn(move || {
                    for (ry, row) in chunk.chunks_exact_mut(w * 4).enumerate() {
                        let y = ci * per + ry;
                        for x in 0..rect_w / ss {
                            let mut acc = [0f32; 4];
                            for sy in 0..ss {
                                let base = ((y * ss + sy) * rect_w + x * ss) * 4;
                                for sx in 0..ss {
                                    let o = base + sx * 4;
                                    acc[0] += px[o];
                                    acc[1] += px[o + 1];
                                    acc[2] += px[o + 2];
                                    acc[3] += px[o + 3];
                                }
                            }
                            let o = (ox + x) * 4;
                            if let Some(bg) = background {
                                for a in &mut acc {
                                    *a *= n;
                                }
                                let inv = 1.0 - acc[3];
                                for k in 0..3 {
                                    row[o + k] = ((acc[k] + bg[k] * inv).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                                }
                                row[o + 3] = 255;
                            } else if acc[3] > 0.0 {
                                let a = (acc[3] * n).clamp(0.0, 1.0);
                                for c in 0..3 {
                                    row[o + c] = ((acc[c] * n / a).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                                }
                                row[o + 3] = (a * 255.0 + 0.5) as u8;
                            }
                        }
                    }
                });
            }
        });
    }
    out
}

#[allow(clippy::too_many_arguments)]
/// `w` = canvas width (the masks are canvas-sized); `rect` = (left column, width) of the drawn rectangle `px` holds, `band` = its rows.
fn fill_triangle(px: &mut [f32], w: usize, rect: (usize, usize), band: (usize, usize), p: [[f32; 2]; 3], uv: [[f32; 2]; 3], page: &Page, item: &DrawItem, mask: Option<&Vec<u8>>, mask2: Option<&Vec<u8>>) {
    let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    if area.abs() < 1e-6 {
        return;
    }
    let (minx, maxx) = (p.iter().map(|q| q[0]).fold(f32::MAX, f32::min), p.iter().map(|q| q[0]).fold(f32::MIN, f32::max));
    let (miny, maxy) = (p.iter().map(|q| q[1]).fold(f32::MAX, f32::min), p.iter().map(|q| q[1]).fold(f32::MIN, f32::max));
    let (cx0, cx1) = (rect.0, rect.0 + rect.1);
    let (bx0, bx1) = (((minx.floor().max(0.0)) as usize).max(cx0), ((maxx.ceil().min(cx1 as f32)) as usize).min(cx1));
    let (y0, y1) = (((miny.floor().max(0.0)) as usize).max(band.0), ((maxy.ceil().max(0.0)) as usize).min(band.1));
    let inv = 1.0 / area;
    let [cr, cg, cb, ca] = item.color;
    for y in y0..y1 {
        let yc = y as f32 + 0.5;
        // Where the row meets the triangle: only those pixels (a pixel or two wider) are tested, not the whole bounding box.
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for e in 0..3 {
            let (a, b) = (p[e], p[(e + 1) % 3]);
            if (a[1] <= yc && b[1] > yc) || (b[1] <= yc && a[1] > yc) {
                let x = a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                lo = lo.min(x);
                hi = hi.max(x);
            }
        }
        if lo > hi {
            continue;
        }
        let x0 = (((lo - 1.5).floor().max(0.0)) as usize).max(bx0);
        let x1 = (((hi + 1.5).ceil().max(0.0)) as usize).min(bx1);
        for x in x0..x1 {
            let xc = x as f32 + 0.5;
            // Barycentric weights of the pixel centre.
            let w0 = ((p[1][0] - xc) * (p[2][1] - yc) - (p[2][0] - xc) * (p[1][1] - yc)) * inv;
            let w1 = ((p[2][0] - xc) * (p[0][1] - yc) - (p[0][0] - xc) * (p[2][1] - yc)) * inv;
            let w2 = 1.0 - w0 - w1;
            // A small tolerance keeps the seams between neighbouring triangles closed.
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let cover = mask.map(|m| m[y * w + x] as f32 / 255.0).unwrap_or(1.0) * mask2.map(|m| m[y * w + x] as f32 / 255.0).unwrap_or(1.0);
            if cover <= 0.0 {
                continue;
            }
            let u = w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0];
            let v = w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1];
            let t = sample(page, u, v);
            // Premultiplied source.
            let (sr, sg, sb, sa);
            match (item.pma, item.dark) {
                // Two-colour tint: what would be black becomes the dark colour, what is white the light one.
                (true, Some(d)) => {
                    sr = t[0] * cr * ca + (t[3] - t[0]) * d[0] * ca;
                    sg = t[1] * cg * ca + (t[3] - t[1]) * d[1] * ca;
                    sb = t[2] * cb * ca + (t[3] - t[2]) * d[2] * ca;
                    sa = t[3] * ca;
                }
                (false, Some(d)) => {
                    sa = t[3] * ca;
                    sr = ((1.0 - t[0]) * d[0] + t[0] * cr) * sa;
                    sg = ((1.0 - t[1]) * d[1] + t[1] * cg) * sa;
                    sb = ((1.0 - t[2]) * d[2] + t[2] * cb) * sa;
                }
                (true, None) => {
                    sr = t[0] * cr * ca;
                    sg = t[1] * cg * ca;
                    sb = t[2] * cb * ca;
                    sa = t[3] * ca;
                }
                (false, None) => {
                    sa = t[3] * ca;
                    sr = t[0] * cr * sa;
                    sg = t[1] * cg * sa;
                    sb = t[2] * cb * sa;
                }
            }
            let (sr, sg, sb, sa) = (sr * cover, sg * cover, sb * cover, sa * cover);
            if sa <= 0.0 && sr <= 0.0 && sg <= 0.0 && sb <= 0.0 {
                continue;
            }
            let at = ((y - band.0) * rect.1 + (x - rect.0)) * 4;
            let d = &mut px[at..at + 4];
            match item.blend {
                BlendMode::Normal => {
                    let k = 1.0 - sa;
                    d[0] = sr + d[0] * k;
                    d[1] = sg + d[1] * k;
                    d[2] = sb + d[2] * k;
                    d[3] = sa + d[3] * k;
                }
                BlendMode::Additive => {
                    d[0] += sr;
                    d[1] += sg;
                    d[2] += sb;
                    d[3] += sa * (1.0 - d[3]);
                }
                BlendMode::Multiply => {
                    let k = 1.0 - sa;
                    d[0] *= sr + k;
                    d[1] *= sg + k;
                    d[2] *= sb + k;
                }
                BlendMode::Screen => {
                    d[0] = sr + d[0] * (1.0 - sr);
                    d[1] = sg + d[1] * (1.0 - sg);
                    d[2] = sb + d[2] * (1.0 - sb);
                    d[3] += sa * (1.0 - d[3]);
                }
            }
        }
    }
}
