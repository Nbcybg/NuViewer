//! The two side panels: folders on the left (tree, folder data, folder size), details on the right (preview, name, size,
//! histogram, editable properties, EXIF).

use crate::app::{Active, App, Focus};
use crate::draw::{draw_box, draw_folder, fmt_bytes, fmt_date, px, type_label, TREE_FOOT, TREE_ROW};
use crate::gfx::{contains, line_h, outline, pal, rgb};
use crate::info;
use crate::input::TextInput;
use crate::model::Kind;
use crate::tree::Icon;
use nv_win::props::Field;

const HEAD: f32 = 36.0;

/// Where things are in the right panel (window coordinates, scroll applied).
pub struct RightGeo {
    pub panel: [f32; 4],
    pub preview: [f32; 4],
    pub name_y: f32,
    pub facts_y: f32,
    pub hist: [f32; 4],
    pub meta_head_y: f32,
    /// Property rows: (field, whole row, value box).
    pub fields: Vec<(Field, [f32; 4], [f32; 4])>,
    pub exif_head_y: f32,
    pub exif_y: f32,
    pub row_h: f32,
    pub content_h: f32,
    pub view_h: f32,
}

impl App {
    // ---- left panel ---------------------------------------------------------------------------------------

    pub fn tree_rect(&self) -> Option<[f32; 4]> {
        let l = self.chrome.left?;
        let d = self.dpi;
        Some([l[0], l[1] + HEAD * d, l[2], l[3] - TREE_FOOT * d])
    }

    pub fn draw_left(&mut self) {
        let Some(l) = self.chrome.left else { return };
        let d = self.dpi;
        let tree_rect = self.tree_rect().unwrap_or(l);
        let row_h = TREE_ROW * d;
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        // Keep the scroll valid for the current size.
        let view_h = tree_rect[3] - tree_rect[1];
        self.tree.clamp(row_h, view_h);
        let font = px(14.0, d);
        let small = px(12.0, d);
        let current = self.listing.dir.clone();
        let pin = self.panels.pin_left;
        let pin_rect = self.chrome.rect_of(crate::chrome::Btn::PinLeft);
        let stats = self.listing.stats();
        let (shown, all) = (self.listing.len(), self.listing.all.len());
        let fs = self.folder_size.clone();
        let loading = self.loading.is_some();
        let sb = self.tree.scrollbar(tree_rect, row_h, d);
        let sb_hot = self.tree_sb_drag.is_some() || sb.map(|(t, _)| contains([t[0] - 4.0 * d, t[1], t[2] + 3.0 * d, t[3]], mx, my)).unwrap_or(false);
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect(l, rgb(pal::PANEL, 1.0));
        let _ = r.fill_rect([l[2] - 1.0, l[1], l[2], l[3]], rgb(pal::PANEL_EDGE, 1.0));
        self.text.draw(r, "Folders", font, l[0] + 12.0 * d, l[1] + 8.0 * d, rgb(pal::TEXT, 1.0));
        if let Some(pr) = pin_rect {
            let _ = r.fill_rect(pr, rgb(if pin { pal::ACCENT } else { 0x2B2E33 }, if pin { 0.85 } else { 1.0 }));
            self.text.draw_centered(r, "Pin", small, (pr[0] + pr[2]) / 2.0, pr[1] + (pr[3] - pr[1] - line_h(small)) / 2.0, rgb(pal::TEXT, 1.0));
        }
        // Tree rows.
        r.set_clip(Some(tree_rect));
        let first = (self.tree.scroll / row_h).floor().max(0.0) as usize;
        let rows = (view_h / row_h).ceil() as usize + 1;
        for i in first..(first + rows).min(self.tree.nodes.len()) {
            let n = &self.tree.nodes[i];
            let y = tree_rect[1] + i as f32 * row_h - self.tree.scroll;
            let row = [tree_rect[0], y, tree_rect[2], y + row_h];
            if n.header {
                self.text.draw(r, &n.name.to_uppercase(), small, l[0] + 12.0 * d, y + (row_h - line_h(small)) / 2.0 + 3.0 * d, rgb(pal::DIM, 1.0));
                continue;
            }
            let is_current = crate::app::same_dir(&n.path, &current);
            if is_current {
                let _ = r.fill_rect(row, rgb(pal::ACCENT, 0.28));
            } else if contains(row, mx, my) {
                let _ = r.fill_rect(row, rgb(pal::TILE_HOVER, 1.0));
            }
            let ix = l[0] + 10.0 * d + n.depth as f32 * 16.0 * d;
            if n.kids {
                self.text.draw(r, if n.expanded { "▾" } else { "▸" }, small, ix + 2.0 * d, y + (row_h - line_h(small)) / 2.0, rgb(pal::DIM, 1.0));
            }
            let icon = [ix + 20.0 * d, y + 5.0 * d, ix + 36.0 * d, y + row_h - 5.0 * d];
            match n.icon {
                Icon::Folder | Icon::Documents | Icon::Pictures | Icon::Downloads | Icon::Desktop => draw_folder(r, [icon[0] - 2.0 * d, icon[1] - 2.0 * d, icon[2] + 2.0 * d, icon[3] + 2.0 * d], 1.0),
                Icon::Drive | Icon::Network => {
                    let _ = r.fill_rect([icon[0], icon[1] + 4.0 * d, icon[2], icon[3] - 2.0 * d], rgb(0x8A9099, 1.0));
                    let _ = r.fill_rect([icon[2] - 5.0 * d, icon[3] - 5.0 * d, icon[2] - 2.0 * d, icon[3] - 3.0 * d], rgb(0x59E08C, 1.0));
                }
                Icon::Removable => {
                    let _ = r.fill_rect([icon[0] + 2.0 * d, icon[1] + 3.0 * d, icon[2] - 2.0 * d, icon[3] - 2.0 * d], rgb(0x6C93C9, 1.0));
                }
            }
            let name = self.text.fit(&n.name, font, (tree_rect[2] - (ix + 44.0 * d) - 8.0 * d - if sb.is_some() { 12.0 * d } else { 0.0 }) as i32);
            self.text.draw(r, &name, font, ix + 42.0 * d, y + (row_h - line_h(font)) / 2.0, rgb(pal::TEXT, 1.0));
        }
        r.set_clip(None);
        // The tree's scroll bar (the folder list is longer than the panel once a few folders are open).
        if let Some((track, thumb)) = sb {
            let _ = r.fill_rect(track, rgb(0xFFFFFF, 0.05));
            let _ = r.fill_rect(thumb, rgb(0xFFFFFF, if sb_hot { 0.55 } else { 0.28 }));
        }
        // Folder data and size.
        let foot_top = l[3] - TREE_FOOT * d;
        let _ = r.fill_rect([l[0], foot_top, l[2], foot_top + 1.0], rgb(pal::PANEL_EDGE, 1.0));
        let mut y = foot_top + 10.0 * d;
        let lh = line_h(small) + 3.0 * d;
        let x = l[0] + 12.0 * d;
        let width = (l[2] - l[0] - 24.0 * d) as i32;
        let name = self.listing.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.listing.dir.display().to_string());
        let name = self.text.fit(&name, font, width);
        self.text.draw(r, &name, font, x, y, rgb(pal::TEXT, 1.0));
        y += line_h(font) + 4.0 * d;
        let rows: Vec<(String, String)> = vec![
            ("Items".into(), if loading { "reading…".into() } else { format!("{shown} shown of {all}") }),
            ("Folders".into(), stats.folders.to_string()),
            ("Pictures".into(), stats.pictures.to_string()),
            ("Documents".into(), stats.documents.to_string()),
            ("Other files".into(), stats.other.to_string()),
            ("Files here".into(), fmt_bytes(stats.bytes)),
            ("Folder size".into(), format!("{}{}", fmt_bytes(fs.bytes), if fs.done { String::new() } else { " …".into() })),
        ];
        for (k, v) in rows {
            self.text.draw(r, &k, small, x, y, rgb(pal::DIM, 1.0));
            let v = self.text.fit(&v, small, width - 96 * d as i32);
            self.text.draw(r, &v, small, x + 96.0 * d, y, rgb(pal::TEXT, 1.0));
            y += lh;
        }
        if fs.files + fs.folders > 0 {
            let line = format!("{} files, {} folders inside", fs.files, fs.folders);
            self.text.draw(r, &line, small, x, y, rgb(pal::DIM, 1.0));
        }
    }

    /// A click in the left panel (not on its Pin button).
    pub fn left_click(&mut self, x: f32, y: f32, double: bool) {
        let Some(tr) = self.tree_rect() else { return };
        if !contains(tr, x, y) {
            return;
        }
        let d = self.dpi;
        let row_h = TREE_ROW * d;
        // On the scroll bar: grab the thumb (or jump there by clicking the track).
        if let Some((track, thumb)) = self.tree.scrollbar(tr, row_h, d) {
            if contains([track[0] - 4.0 * d, track[1], track[2] + 3.0 * d, track[3]], x, y) {
                let grab = if contains(thumb, x, y) { y - thumb[1] } else { (thumb[3] - thumb[1]) / 2.0 };
                self.tree_sb_drag = Some(grab);
                self.tree.scroll_thumb_to(tr, row_h, d, y - grab);
                nv_win::invalidate(self.hwnd);
                return;
            }
        }
        let i = ((y - tr[1] + self.tree.scroll) / row_h).floor() as usize;
        let Some(n) = self.tree.nodes.get(i) else { return };
        if n.header {
            return;
        }
        let (path, depth, kids) = (n.path.clone(), n.depth, n.kids);
        let ix = self.chrome.left.map(|l| l[0]).unwrap_or(0.0) + 10.0 * d + depth as f32 * 16.0 * d;
        if kids && x < ix + 22.0 * d {
            self.tree.toggle(i);
        } else {
            if double && kids {
                self.tree.toggle(i);
            }
            self.go(path, Vec::new(), true);
        }
        nv_win::invalidate(self.hwnd);
    }

    // ---- right panel ---------------------------------------------------------------------------------------

    /// The right panel's layout for the focused item (None when the panel is hidden).
    pub fn right_geo(&self) -> Option<RightGeo> {
        let p = self.chrome.right?;
        let d = self.dpi;
        let pad = 14.0 * d;
        let w = p[2] - p[0];
        let top = p[1] + HEAD * d - self.right_scroll;
        let mut y = top;
        let preview_h = ((w - 2.0 * pad) * 0.72).min(250.0 * d);
        let preview = [p[0] + pad, y, p[2] - pad, y + preview_h];
        y += preview_h + 10.0 * d;
        let name_y = y;
        y += line_h(px(16.0, d)) + 4.0 * d;
        let facts_y = y;
        y += 2.0 * (line_h(px(12.0, d)) + 2.0 * d) + 8.0 * d;
        let hist = [p[0] + pad, y, p[2] - pad, y + 74.0 * d];
        y += 74.0 * d + 14.0 * d;
        let meta_head_y = y;
        y += 26.0 * d;
        let editable = self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| !e.is_dir()).unwrap_or(false);
        let mut fields = Vec::new();
        if editable {
            for f in Field::ALL {
                let row = [p[0] + pad, y, p[2] - pad, y + 30.0 * d];
                let value = [row[0] + 74.0 * d, row[1], row[2], row[3]];
                fields.push((f, row, value));
                y += 34.0 * d;
            }
        }
        y += 8.0 * d;
        let exif_head_y = y;
        y += 26.0 * d;
        let exif_y = y;
        let row_h = line_h(px(12.0, d)) + 4.0 * d;
        let n_exif = self.info.as_ref().map(|i| i.exif.len()).unwrap_or(0);
        y += n_exif as f32 * row_h + 20.0 * d;
        Some(RightGeo { panel: p, preview, name_y, facts_y, hist, meta_head_y, fields, exif_head_y, exif_y, row_h, content_h: y - top, view_h: p[3] - p[1] - HEAD * d })
    }

    pub fn draw_right(&mut self) {
        let Some(mut geo) = self.right_geo() else { return };
        let d = self.dpi;
        // Clamp the scroll to the content, then recompute with it.
        let max_scroll = (geo.content_h - geo.view_h).max(0.0);
        if self.right_scroll > max_scroll {
            self.right_scroll = max_scroll;
            if let Some(g) = self.right_geo() {
                geo = g;
            }
        }
        let p = geo.panel;
        let (font, small, big) = (px(14.0, d), px(12.0, d), px(16.0, d));
        let pad = 14.0 * d;
        let pin = self.panels.pin_right;
        let pin_rect = self.chrome.rect_of(crate::chrome::Btn::PinRight);
        let picked: Vec<_> = self.sel.entries(&self.listing).into_iter().map(|e| (e.id, e.name.clone(), e.size, e.kind, e.modified, e.ext.clone(), e.path.clone())).collect();
        let n_sel = self.sel.count(&self.listing);
        let focus = self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| (e.id, e.name.clone(), e.size, e.kind, e.modified, e.ext.clone()));
        let info_ready = self.info.as_ref().filter(|i| Some(i.id) == self.info_for);
        let dims = info_ready.and_then(|i| i.dims);
        let exif: Vec<(String, String)> = info_ready.map(|i| i.exif.clone()).unwrap_or_default();
        let props: Vec<(Field, String)> = info_ready.and_then(|i| i.props.clone()).unwrap_or_default();
        let hist = focus.as_ref().and_then(|f| self.hist.get(&f.0)).cloned();
        let preview_ok = focus.as_ref().map(|f| self.preview_id == Some(f.0)).unwrap_or(false);
        let active = self.active.as_ref().map(|a| (a.focus.clone(), a.ti.clone(), a.error.is_some()));
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect(p, rgb(pal::PANEL, 1.0));
        let _ = r.fill_rect([p[0], p[1], p[0] + 1.0, p[3]], rgb(pal::PANEL_EDGE, 1.0));
        // The scrolling content, clipped under the fixed header.
        r.set_clip(Some([p[0], p[1] + HEAD * d, p[2], p[3]]));
        let width = (p[2] - p[0] - 2.0 * pad) as i32;
        match &focus {
            None => {
                self.text.draw(r, "Nothing selected", font, p[0] + pad, geo.preview[1], rgb(pal::DIM, 1.0));
            }
            Some((id, name, size, kind, modified, ext)) => {
                // Preview.
                let pv = geo.preview;
                let _ = r.fill_rect(pv, rgb(pal::FIELD, 1.0));
                let key = if preview_ok { info::preview_key(*id) } else { *id };
                if *kind == Kind::Folder {
                    draw_folder(r, pv, 1.0);
                } else {
                    // Zoom, pan, animation controls, the turning 3D model: see `preview.rs`.
                    let clip = [p[0], p[1] + HEAD * d, p[2], p[3]];
                    crate::preview::draw(r, &mut self.text, &self.pv, pv, clip, *id, key, ext, d);
                }
                // Name and facts.
                let title = if n_sel > 1 { format!("{n_sel} items selected") } else { name.clone() };
                let title = self.text.fit(&title, big, width);
                self.text.draw(r, &title, big, p[0] + pad, geo.name_y, rgb(pal::TEXT, 1.0));
                let mut l1 = Vec::new();
                let mut l2 = Vec::new();
                if n_sel > 1 {
                    let total: u64 = picked.iter().map(|e| e.2).sum();
                    l1.push(format!("{} in total", fmt_bytes(total)));
                } else if *kind == Kind::Folder {
                    l1.push("Folder".to_string());
                    l1.push(format!("changed {}", fmt_date(*modified)));
                } else {
                    l1.push(type_label(&crate::model::Entry { id: *id, path: Default::default(), name: name.clone(), ext: ext.clone(), kind: *kind, size: *size, modified: *modified }));
                    l1.push(fmt_bytes(*size));
                    if let Some((w, h)) = dims {
                        l2.push(format!("{w} × {h}  ({:.1} MP)", w as f64 * h as f64 / 1e6));
                    }
                    l2.push(format!("changed {}", fmt_date(*modified)));
                }
                self.text.draw(r, &l1.join("  ·  "), small, p[0] + pad, geo.facts_y, rgb(pal::DIM, 1.0));
                self.text.draw(r, &l2.join("  ·  "), small, p[0] + pad, geo.facts_y + line_h(small) + 2.0 * d, rgb(pal::DIM, 1.0));
                // Histogram.
                let hb = geo.hist;
                let _ = r.fill_rect(hb, rgb(pal::FIELD, 1.0));
                if let Some(h) = &hist {
                    let cols = 128usize;
                    let (bw, bh) = ((hb[2] - hb[0]) / cols as f32, hb[3] - hb[1] - 4.0 * d);
                    let peak = h.peak as f32;
                    for c in 0..cols {
                        for (arr, col) in [(&h.r, 0xFF4B4B), (&h.g, 0x3DDC6A), (&h.b, 0x4B8BFF)] {
                            let v = (arr[c * 2] + arr[c * 2 + 1]) as f32 / 2.0;
                            let hh = (v / peak).min(1.0) * bh;
                            if hh >= 1.0 {
                                let x = hb[0] + c as f32 * bw;
                                let _ = r.fill_rect([x, hb[3] - 2.0 * d - hh, x + bw, hb[3] - 2.0 * d], rgb(col, 0.55));
                            }
                        }
                    }
                } else if *kind != Kind::Folder {
                    self.text.draw_centered(r, "Histogram", small, (hb[0] + hb[2]) / 2.0, hb[1] + 26.0 * d, rgb(pal::DIM, 0.6));
                }
                outline(r, hb, 1.0, rgb(pal::PANEL_EDGE, 1.0));
                // Properties.
                if !geo.fields.is_empty() {
                    let head = if n_sel > 1 { format!("Properties — applies to all {n_sel}") } else { "Properties (editable)".to_string() };
                    self.text.draw(r, &head, font, p[0] + pad, geo.meta_head_y, rgb(pal::TEXT, 1.0));
                    for (f, row, value) in &geo.fields {
                        self.text.draw(r, f.label(), small, row[0], row[1] + (row[3] - row[1] - line_h(small)) / 2.0, rgb(pal::DIM, 1.0));
                        let v = props.iter().find(|(k, _)| k == f).map(|(_, v)| v.clone()).unwrap_or_default();
                        let editing = active.as_ref().filter(|(fo, _, _)| *fo == Focus::Meta(*f));
                        if *f == Field::Rating {
                            let stars: u32 = v.parse().unwrap_or(0);
                            let _ = r.fill_rect(*value, rgb(pal::FIELD, 1.0));
                            outline(r, *value, 1.0, rgb(if contains(*value, mx, my) { pal::ACCENT } else { pal::PANEL_EDGE }, 1.0));
                            for s in 0..5u32 {
                                let c = if s < stars { rgb(pal::STAR, 1.0) } else { rgb(0x555A62, 1.0) };
                                self.text.draw(r, "★", px(17.0, d), value[0] + 8.0 * d + s as f32 * 24.0 * d, value[1] + (value[3] - value[1] - line_h(px(17.0, d))) / 2.0, c);
                            }
                        } else {
                            let (t, ti): (String, Option<&TextInput>) = match &editing {
                                Some((_, ti, _)) => (ti.text(), Some(ti)),
                                None => (v, None),
                            };
                            draw_box(r, &mut self.text, *value, &t, ti, if info_ready.is_some() { "—" } else { "" }, small, d, editing.map(|e| e.2).unwrap_or(false));
                        }
                    }
                }
                // EXIF.
                if !exif.is_empty() {
                    self.text.draw(r, "EXIF", font, p[0] + pad, geo.exif_head_y, rgb(pal::TEXT, 1.0));
                    let label_w = (84.0 * d) as i32;
                    for (i, (k, v)) in exif.iter().enumerate() {
                        let y = geo.exif_y + i as f32 * geo.row_h;
                        self.text.draw(r, k, small, p[0] + pad, y, rgb(pal::DIM, 1.0));
                        let v = self.text.fit(v, small, width - label_w);
                        self.text.draw(r, &v, small, p[0] + pad + label_w as f32, y, rgb(pal::TEXT, 1.0));
                    }
                } else if info_ready.is_none() && *kind != Kind::Folder {
                    self.text.draw(r, "reading details…", small, p[0] + pad, geo.exif_head_y, rgb(pal::DIM, 0.7));
                }
            }
        }
        r.set_clip(None);
        // Fixed header.
        let head = [p[0], p[1], p[2], p[1] + HEAD * d];
        let _ = r.fill_rect(head, rgb(pal::PANEL, 1.0));
        let _ = r.fill_rect([head[0], head[3] - 1.0, head[2], head[3]], rgb(pal::PANEL_EDGE, 1.0));
        self.text.draw(r, "Details", font, p[0] + pad, p[1] + 8.0 * d, rgb(pal::TEXT, 1.0));
        if let Some(pr) = pin_rect {
            let _ = r.fill_rect(pr, rgb(if pin { pal::ACCENT } else { 0x2B2E33 }, if pin { 0.85 } else { 1.0 }));
            self.text.draw_centered(r, "Pin", small, (pr[0] + pr[2]) / 2.0, pr[1] + (pr[3] - pr[1] - line_h(small)) / 2.0, rgb(pal::TEXT, 1.0));
        }
    }

    /// A click in the right panel: start editing a property, or set the star rating.
    pub fn right_click_panel(&mut self, x: f32, y: f32) {
        let Some(geo) = self.right_geo() else { return };
        if y < geo.panel[1] + HEAD * self.dpi {
            return;
        }
        let d = self.dpi;
        for (f, _row, value) in &geo.fields {
            if !contains(*value, x, y) {
                continue;
            }
            if *f == Field::Rating {
                let current: u32 = self.info.as_ref().and_then(|i| i.props.as_ref()).and_then(|p| p.iter().find(|(k, _)| *k == Field::Rating).map(|(_, v)| v.parse().unwrap_or(0))).unwrap_or(0);
                let star = (((x - value[0] - 4.0 * d) / (24.0 * d)).floor() as i32 + 1).clamp(0, 5) as u32;
                // Clicking the star that is already the rating clears it.
                let stars = if star == current { 0 } else { star };
                self.write_meta(Field::Rating, &stars.to_string());
            } else {
                let current = self.info.as_ref().and_then(|i| i.props.as_ref()).and_then(|p| p.iter().find(|(k, _)| k == f).map(|(_, v)| v.clone())).unwrap_or_default();
                self.active = Some(Active { focus: Focus::Meta(*f), ti: TextInput::selected(&current), error: None });
                nv_win::invalidate(self.hwnd);
            }
            return;
        }
    }

    pub fn finish_meta(&mut self, field: Field, text: &str) {
        self.write_meta(field, text);
    }

    /// Write a property to every selected file (or the focused one).
    pub fn write_meta(&mut self, field: Field, value: &str) {
        let targets: Vec<std::path::PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| !e.is_dir()).map(|e| e.path.clone()).collect();
        if targets.is_empty() {
            return;
        }
        let mut failed: Option<String> = None;
        let mut ok = 0;
        for p in &targets {
            match nv_win::props::write(p, field, value) {
                Ok(()) => ok += 1,
                Err(e) => failed = Some(e),
            }
        }
        match failed {
            Some(e) if ok == 0 => self.toast(format!("Couldn't save {}: {e}", field.label())),
            Some(e) => self.toast(format!("Saved {ok} of {} files — {e}", targets.len())),
            None => self.toast(format!("{} saved{}", field.label(), if targets.len() > 1 { format!(" on {} files", targets.len()) } else { String::new() })),
        }
        // Read it back (also shows what the file really kept).
        self.info_for = None;
        self.info = None;
        nv_win::set_timer(self.hwnd, crate::app::TIMER_INFO, 60);
    }
}
