//! Drawing the items (thumbnails, details, list), the toolbar and the overlays. Panels are in `panels.rs`.
//!
//! Rule of thumb for borrowing: anything that needs `&self` methods (the grid, the chrome) is worked out first, then
//! the renderer is borrowed and only fields are touched.

use crate::app::{App, Focus};
use crate::chrome::Btn;
use crate::gfx::{contains, line_h, outline, pal, rgb, Text};
use crate::input::TextInput;
use crate::model::{Entry, Kind};
use crate::state::ViewMode;
use nv_core::folder::SortKey;
use nv_render::Renderer;

pub const TREE_ROW: f32 = 26.0;
pub const TREE_FOOT: f32 = 196.0;

pub fn px(base: f32, d: f32) -> i32 {
    (base * d).round() as i32
}

pub fn toggle_fullscreen(app: &mut App) {
    let fs = nv_win::toggle_fullscreen(app.hwnd);
    app.fullscreen_timers(fs);
    // The WM_SIZE caused by the style change arrives while we are busy: resize by hand.
    let (w, h) = nv_win::client_size(app.hwnd);
    app.size = (w, h);
    if let Some(r) = app.renderer.as_mut() {
        let _ = r.resize(w, h);
    }
    app.relayout();
    app.clamp_scroll();
    app.request_thumbs();
    nv_win::invalidate(app.hwnd);
}

/// "3.2 MB" like Explorer.
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else if v >= 100.0 { format!("{v:.0} {}", UNITS[u]) } else { format!("{v:.1} {}", UNITS[u]) }
}

/// "2025-06-01 14:30" in local time.
pub fn fmt_date(secs: i64) -> String {
    let t = nv_core::batch::format_local(secs, nv_win::utc_offset_minutes());
    t[..16.min(t.len())].to_string()
}

pub fn type_label(e: &Entry) -> String {
    match e.kind {
        Kind::Folder => "Folder".into(),
        _ if e.ext.is_empty() => "File".into(),
        _ => e.ext.to_uppercase(),
    }
}

// ---- details columns -------------------------------------------------------------------------------------------

/// x positions of the details view: (name start, size right edge, type start, date start, date end).
pub fn columns(area: [f32; 4], d: f32) -> (f32, f32, f32, f32, f32) {
    let right = area[2] - 14.0 * d;
    let date_x = right - 150.0 * d;
    let type_x = date_x - 96.0 * d;
    let size_right = type_x - 10.0 * d;
    (area[0] + 52.0 * d, size_right, type_x, date_x, right)
}

/// Which sort key a click at window x on the details header means.
pub fn header_hit(app: &App, x: f32) -> Option<SortKey> {
    let (name_x, size_right, type_x, date_x, _) = columns(app.chrome.area, app.dpi);
    Some(if x < size_right - 80.0 * app.dpi {
        let _ = name_x;
        SortKey::Name
    } else if x < type_x {
        SortKey::Size
    } else if x < date_x {
        SortKey::Type
    } else {
        SortKey::Modified
    })
}

// ---- a text box -------------------------------------------------------------------------------------------------

/// A single-line box: background, border, the text (or a dim placeholder), the selection and the caret.
#[allow(clippy::too_many_arguments)]
pub fn draw_box(r: &mut Renderer, text: &mut Text, rect: [f32; 4], s: &str, ti: Option<&TextInput>, placeholder: &str, font: i32, d: f32, error: bool) {
    let focused = ti.is_some();
    let edge = if error { pal::ERROR } else if focused { pal::ACCENT } else { pal::PANEL_EDGE };
    let _ = r.fill_rect(rect, rgb(pal::FIELD, 1.0));
    outline(r, rect, if focused { 2.0 * d.min(1.5) } else { 1.0 }, rgb(edge, 1.0));
    let pad = 8.0 * d;
    let inner = [rect[0] + 2.0, rect[1] + 2.0, rect[2] - 2.0, rect[3] - 2.0];
    let ty = rect[1] + (rect[3] - rect[1] - line_h(font)) / 2.0 + 1.0;
    let view_w = (rect[2] - rect[0]) - 2.0 * pad;
    r.set_clip(Some(inner));
    match ti {
        Some(ti) => {
            let prefix = |n: usize| -> String { ti.chars.iter().take(n).collect() };
            let caret_x = text.width(&prefix(ti.cursor), font) as f32;
            let shift = (caret_x - view_w + 2.0).max(0.0);
            let x0 = rect[0] + pad - shift;
            if let Some((a, b)) = ti.selection() {
                let (xa, xb) = (text.width(&prefix(a), font) as f32, text.width(&prefix(b), font) as f32);
                let _ = r.fill_rect([x0 + xa, ty, x0 + xb, ty + line_h(font)], rgb(pal::ACCENT, 0.55));
            }
            text.draw(r, s, font, x0, ty, rgb(pal::TEXT, 1.0));
            let _ = r.fill_rect([x0 + caret_x, ty + 1.0, x0 + caret_x + (1.5 * d).max(1.0), ty + line_h(font) - 1.0], rgb(pal::TEXT, 1.0));
        }
        None => {
            if s.is_empty() {
                text.draw(r, placeholder, font, rect[0] + pad, ty, rgb(pal::DIM, 1.0));
            } else {
                let fitted = text.fit(s, font, view_w as i32);
                text.draw(r, &fitted, font, rect[0] + pad, ty, rgb(pal::TEXT, 1.0));
            }
        }
    }
    r.set_clip(None);
}

/// Folder glyph: a tab and a body, drawn in a box.
pub fn draw_folder(r: &mut Renderer, b: [f32; 4], alpha: f32) {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let s = w.min(h) * 0.62;
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let (x0, y0) = (cx - s / 2.0, cy - s * 0.36);
    let c = rgb(pal::FOLDER, alpha);
    let _ = r.fill_rect([x0, y0, x0 + s * 0.42, y0 + s * 0.14], rgb(0xC99A33, alpha));
    let _ = r.fill_rect([x0, y0 + s * 0.1, x0 + s, y0 + s * 0.72], c);
    let _ = r.fill_rect([x0, y0 + s * 0.22, x0 + s, y0 + s * 0.72], rgb(0xF2C75E, alpha));
}

/// A folder with the first picture inside it peeking out of the front, like Explorer's folder previews.
pub fn draw_folder_cover(r: &mut Renderer, b: [f32; 4], id: u64, tw: u32, th: u32) {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let s = w.min(h) * 0.86;
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let (x0, y0) = (cx - s / 2.0, cy - s * 0.42);
    let _ = r.fill_rect([x0, y0, x0 + s * 0.4, y0 + s * 0.12], rgb(0xC99A33, 1.0));
    let _ = r.fill_rect([x0, y0 + s * 0.08, x0 + s, y0 + s * 0.8], rgb(0xC99A33, 1.0));
    // The picture, fitted into the back of the folder.
    let area = [x0 + s * 0.09, y0 + s * 0.15, x0 + s * 0.91, y0 + s * 0.66];
    let (aw, ah) = (area[2] - area[0], area[3] - area[1]);
    let k = (aw / tw as f32).min(ah / th as f32);
    let (pw, ph) = ((tw as f32 * k).round(), (th as f32 * k).round());
    let (px, py) = ((area[0] + (aw - pw) / 2.0).round(), (area[1] + (ah - ph) / 2.0).round());
    let _ = r.fill_rect([px - 2.0, py - 2.0, px + pw + 2.0, py + ph + 2.0], rgb(0xFFFFFF, 0.9));
    let _ = r.draw_thumb(id, [px, py, px + pw, py + ph]);
    // The front of the folder over the lower part of the picture.
    let _ = r.fill_rect([x0, y0 + s * 0.36, x0 + s, y0 + s * 0.8], rgb(0xF2C75E, 0.97));
}

/// A picture-less file: a card with its extension.
pub fn draw_card(r: &mut Renderer, text: &mut Text, b: [f32; 4], ext: &str, failed: bool, d: f32) {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    let (cw, ch) = (w.min(h) * 0.56, w.min(h) * 0.7);
    let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let card = [cx - cw / 2.0, cy - ch / 2.0, cx + cw / 2.0, cy + ch / 2.0];
    let _ = r.fill_rect(card, rgb(if failed { 0x5A2A2A } else { 0x3A3D42 }, 1.0));
    outline(r, card, 1.0, rgb(if failed { 0x8A4444 } else { 0x55595F }, 1.0));
    let label = if ext.is_empty() { "?".to_string() } else { ext.to_uppercase() };
    let size = (cw * 0.28).clamp(9.0 * d, 20.0 * d) as i32;
    let label = text.fit(&label, size, (cw - 6.0) as i32);
    text.draw_centered(r, &label, size, cx, cy - line_h(size) / 2.0, rgb(if failed { pal::ERROR } else { pal::DIM }, 1.0));
}

impl App {
    pub fn draw(&mut self) {
        if self.renderer.is_none() {
            return;
        }
        self.upload_staged();
        let g = self.grid();
        let scroll = g.clamp(self.scroll);
        let hover = {
            let (x, y) = self.last_mouse;
            if self.chrome.over_panel(x as f32, y as f32) { None } else { g.hit(x as f32, y as f32, scroll) }
        };
        let d = self.dpi;
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        {
            let Some(r) = self.renderer.as_mut() else { return };
            if !r.begin(rgb(pal::BG, 1.0)) {
                return;
            }
        }
        self.text.begin_frame();
        self.draw_items(&g, scroll, hover);
        self.draw_rubber();
        self.draw_scrollbar();
        self.draw_status(&g);
        self.draw_left();
        self.draw_right();
        self.draw_toolbar();
        self.draw_overlays(w, h, d);
        if let Some(r) = self.renderer.as_mut() {
            self.text.end_frame(r);
            let _ = r.end();
        }
        if self.timing.first_frame_ms.is_none() {
            self.timing.first_frame_ms = Some(self.ms_since_start());
            // Tell the idle loop: the benchmark report may be due.
            (self.wake)();
        }
    }

    fn draw_items(&mut self, g: &crate::grid::Grid, scroll: f32, hover: Option<usize>) {
        let d = self.dpi;
        let mode = self.mode;
        let label_h = self.label_h();
        let cell = self.cell_px();
        let area = self.chrome.area;
        let (name_x, size_right, type_x, date_x, _right) = columns(area, d);
        let cursor = self.sel.cursor;
        let Some(r) = self.renderer.as_mut() else { return };
        // Items never draw into the details header strip or under docked panels.
        let top = area[1] + if mode == ViewMode::Details { 28.0 * d } else { 0.0 };
        r.set_clip(Some([area[0], top, area[2], area[3]]));
        let name_px = px(13.0, d);
        for i in g.visible(scroll) {
            let e = &self.listing.all[self.listing.view[i]];
            let rc = g.rect(i, scroll);
            if rc[3] < top || rc[1] > area[3] {
                continue;
            }
            let selected = self.sel.ids.contains(&e.id);
            let failed = self.thumb_failed.contains(&e.id);
            let hovered = hover == Some(i);
            let bg = if selected {
                rgb(pal::ACCENT, 0.30)
            } else if hovered {
                rgb(pal::TILE_HOVER, 1.0)
            } else if mode == ViewMode::Details && i % 2 == 1 {
                rgb(0x1F2124, 1.0)
            } else if mode == ViewMode::Thumbs {
                rgb(pal::TILE, 1.0)
            } else {
                rgb(pal::BG, 0.0)
            };
            let _ = r.fill_rect(rc, bg);
            match mode {
                ViewMode::Thumbs => {
                    let b = [rc[0], rc[1], rc[2], rc[1] + cell];
                    draw_visual(r, &mut self.text, e, b, 8.0 * d, failed, d);
                    if label_h > 0.0 {
                        let name = self.text.fit(&e.name, name_px, (rc[2] - rc[0] - 12.0 * d) as i32);
                        self.text.draw(r, &name, name_px, rc[0] + 6.0 * d, rc[1] + cell + 3.0 * d, rgb(if selected { 0xFFFFFF } else { pal::TEXT }, 1.0));
                    }
                }
                ViewMode::Details => {
                    let icon = [rc[0] + 10.0 * d, rc[1] + 3.0 * d, rc[0] + 10.0 * d + 24.0 * d, rc[3] - 3.0 * d];
                    draw_visual(r, &mut self.text, e, icon, 0.0, failed, d);
                    let ty = rc[1] + (rc[3] - rc[1] - line_h(name_px)) / 2.0;
                    let name = self.text.fit(&e.name, name_px, (size_right - 90.0 * d - name_x) as i32);
                    self.text.draw(r, &name, name_px, name_x, ty, rgb(pal::TEXT, 1.0));
                    if !e.is_dir() {
                        self.text.draw_right(r, &fmt_bytes(e.size), name_px, size_right, ty, rgb(pal::DIM, 1.0));
                    }
                    self.text.draw(r, &type_label(e), name_px, type_x, ty, rgb(pal::DIM, 1.0));
                    self.text.draw(r, &fmt_date(e.modified), name_px, date_x, ty, rgb(pal::DIM, 1.0));
                }
                ViewMode::List => {
                    let icon = [rc[0] + 4.0 * d, rc[1] + 2.0 * d, rc[0] + 4.0 * d + 24.0 * d, rc[3] - 2.0 * d];
                    draw_visual(r, &mut self.text, e, icon, 0.0, failed, d);
                    let ty = rc[1] + (rc[3] - rc[1] - line_h(name_px)) / 2.0;
                    let name = self.text.fit(&e.name, name_px, (rc[2] - rc[0] - 40.0 * d) as i32);
                    self.text.draw(r, &name, name_px, rc[0] + 34.0 * d, ty, rgb(pal::TEXT, 1.0));
                }
            }
            if selected {
                outline(r, rc, (2.0 * d).max(1.0), rgb(pal::ACCENT, 1.0));
            } else if cursor == Some(i) {
                outline(r, rc, 1.0, rgb(0xB8C4D8, 0.8));
            }
        }
        r.set_clip(None);
        // The details header stays on top.
        if mode == ViewMode::Details {
            let hb = [area[0], area[1], area[2], area[1] + 28.0 * d];
            let _ = r.fill_rect(hb, rgb(pal::PANEL, 1.0));
            let _ = r.fill_rect([hb[0], hb[3] - 1.0, hb[2], hb[3]], rgb(pal::PANEL_EDGE, 1.0));
            let key = self.listing.order.key;
            let mark = |k: SortKey| -> &'static str {
                if self.listing.order.shuffle.is_some() || key != k { "" } else if self.listing.order.descending { " ▼" } else { " ▲" }
            };
            let ty = hb[1] + (hb[3] - hb[1] - line_h(name_px)) / 2.0;
            let dim = rgb(pal::TEXT, 0.85);
            self.text.draw(r, &format!("Name{}", mark(SortKey::Name)), name_px, name_x, ty, dim);
            self.text.draw_right(r, &format!("Size{}", mark(SortKey::Size)), name_px, size_right, ty, dim);
            self.text.draw(r, &format!("Type{}", mark(SortKey::Type)), name_px, type_x, ty, dim);
            self.text.draw(r, &format!("Modified{}", mark(SortKey::Modified)), name_px, date_x, ty, dim);
        }
        if self.listing.len() == 0 && self.loading.is_none() {
            let msg = if !self.listing.search.is_empty() { "Nothing matches the search" } else { "This folder has nothing to show" };
            let cx = (area[0] + area[2]) / 2.0;
            self.text.draw_centered(r, msg, px(16.0, d), cx, area[1] + 80.0 * d, rgb(pal::DIM, 1.0));
        }
    }

    fn draw_scrollbar(&mut self) {
        let Some((track, thumb)) = self.scrollbar() else { return };
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let hot = self.sb_drag.is_some() || contains(thumb, mx, my);
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect(track, rgb(0xFFFFFF, 0.05));
        let _ = r.fill_rect(thumb, rgb(0xFFFFFF, if hot { 0.55 } else { 0.28 }));
    }

    fn draw_rubber(&mut self) {
        let Some(rb) = &self.rubber else { return };
        let (sx, sy, nx, ny) = (rb.start.0, rb.start.1 - (self.scroll - rb.scroll0), rb.now.0, rb.now.1);
        let rect = [sx.min(nx), sy.min(ny), sx.max(nx), sy.max(ny)];
        let d = self.dpi;
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect(rect, rgb(pal::ACCENT, 0.15));
        outline(r, rect, (1.5 * d).max(1.0), rgb(0x7DB4FF, 0.95));
    }

    /// A status line at the bottom edge of the items: counts, the selection, the size of what is selected.
    fn draw_status(&mut self, g: &crate::grid::Grid) {
        let d = self.dpi;
        let n = self.listing.len();
        let sel: Vec<&Entry> = self.sel.entries(&self.listing);
        let picked = self.sel.count(&self.listing);
        let mut s = format!("{n} items");
        if picked > 0 {
            let bytes: u64 = sel.iter().map(|e| e.size).sum();
            s += &format!("  ·  {picked} selected ({})", fmt_bytes(bytes));
        }
        if let Some(ms) = self.timing.visible_ready_ms {
            s += &format!("  ·  thumbnails {} in {ms:.0} ms", self.timing.visible_count);
        }
        if self.listing.order.shuffle.is_some() {
            s += "  ·  shuffled";
        } else {
            s += &format!("  ·  by {} {}", self.listing.order.key.label().to_lowercase(), if self.listing.order.descending { "↓" } else { "↑" });
        }
        if self.listing.show != crate::model::Show::Media {
            s += &format!("  ·  {}", self.listing.show.label());
        }
        let font = px(12.0, d);
        let area = g.area;
        let w = self.text.width(&s, font) as f32 + 16.0 * d;
        let h = line_h(font) + 6.0 * d;
        let x = area[0] + 10.0 * d;
        let y = area[3] - h - 8.0 * d;
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect([x, y, x + w, y + h], rgb(0x0E0E0E, 0.78));
        self.text.draw(r, &s, font, x + 8.0 * d, y + 3.0 * d, rgb(pal::TEXT, 0.95));
    }

    fn draw_toolbar(&mut self) {
        let Some(top) = self.chrome.top else { return };
        let d = self.dpi;
        let (mx, my) = (self.last_mouse.0 as f32, self.last_mouse.1 as f32);
        let font = px(15.0, d);
        let small = px(13.0, d);
        let buttons = self.chrome.buttons.clone();
        let (path_box, search_box) = (self.chrome.path_box, self.chrome.search_box);
        let mode = self.mode;
        let can_back = !self.back.is_empty();
        let can_fwd = !self.forward.is_empty();
        let pin = self.panels.pin_top;
        let key_label = self.listing.order.key.label();
        let show_label = if self.listing.show == crate::model::Show::Media { "Show".to_string() } else { "Show •".to_string() };
        let path_text = self.listing.dir.display().to_string();
        let search_text = self.listing.search.clone();
        let Some(r) = self.renderer.as_mut() else { return };
        let _ = r.fill_rect(top, rgb(pal::PANEL, 1.0));
        let _ = r.fill_rect([top[0], top[3] - 1.0, top[2], top[3]], rgb(pal::PANEL_EDGE, 1.0));
        for (b, rc) in &buttons {
            if matches!(b, Btn::PinLeft | Btn::PinRight) {
                continue;
            }
            let hot = contains(*rc, mx, my);
            let on = match b {
                Btn::ViewThumbs => mode == ViewMode::Thumbs,
                Btn::ViewDetails => mode == ViewMode::Details,
                Btn::ViewList => mode == ViewMode::List,
                Btn::PinTop => pin,
                _ => false,
            };
            let dead = (*b == Btn::Back && !can_back) || (*b == Btn::Forward && !can_fwd);
            let bg = if on { rgb(pal::ACCENT, 0.85) } else if hot && !dead { rgb(0x3A3E44, 1.0) } else { rgb(0x2B2E33, 1.0) };
            let _ = r.fill_rect(*rc, bg);
            let label = match b {
                Btn::Prev => "◀".to_string(),
                Btn::Next => "▶".to_string(),
                Btn::Back => "←".to_string(),
                Btn::Forward => "→".to_string(),
                Btn::Up => "↑".to_string(),
                Btn::Sort => format!("{key_label} ▾"),
                Btn::Show => format!("{show_label} ▾"),
                Btn::ViewThumbs => "▦".to_string(),
                Btn::ViewDetails => "☰".to_string(),
                Btn::ViewList => "≣".to_string(),
                Btn::Tools => "Tools ▾".to_string(),
                Btn::Exit => "✕".to_string(),
                Btn::PinTop => "Pin".to_string(),
                _ => String::new(),
            };
            let f = if matches!(b, Btn::Sort | Btn::Show | Btn::Tools | Btn::PinTop) { small } else { font };
            let c = if dead { rgb(pal::DIM, 0.5) } else { rgb(pal::TEXT, 1.0) };
            self.text.draw_centered(r, &label, f, (rc[0] + rc[2]) / 2.0, rc[1] + (rc[3] - rc[1] - line_h(f)) / 2.0, c);
        }
        let path_focus = self.active.as_ref().filter(|a| a.focus == Focus::Path);
        let search_focus = self.active.as_ref().filter(|a| a.focus == Focus::Search);
        let p_text = path_focus.map(|a| a.ti.text()).unwrap_or(path_text);
        draw_box(r, &mut self.text, path_box, &p_text, path_focus.map(|a| &a.ti), "", font, d, path_focus.map(|a| a.error.is_some()).unwrap_or(false));
        let s_text = search_focus.map(|a| a.ti.text()).unwrap_or(search_text);
        draw_box(r, &mut self.text, search_box, &s_text, search_focus.map(|a| &a.ti), "Search", font, d, false);
    }

    /// Rename box, toasts, progress, help.
    fn draw_overlays(&mut self, w: f32, h: f32, d: f32) {
        let font = px(15.0, d);
        let small = px(13.0, d);
        let rename = self.active.as_ref().filter(|a| matches!(a.focus, Focus::Rename(_) | Focus::NewFolder(_))).map(|a| (a.ti.clone(), a.error.clone(), matches!(a.focus, Focus::NewFolder(_))));
        let toast = self.toast.as_ref().map(|(t, _)| t.clone());
        let job = self.job.as_ref().map(|j| format!("{}  {}  (Esc cancels)", j.label, self.job_text));
        let help = self.help;
        let Some(r) = self.renderer.as_mut() else { return };
        if let Some((ti, err, new_folder)) = rename {
            let bw = (w * 0.42).clamp(320.0 * d, 760.0 * d);
            let bh = 38.0 * d;
            let x = (w - bw) / 2.0;
            let y = h * 0.36;
            let title = if new_folder { "New folder — Enter creates it, Esc cancels" } else { "Rename — Enter applies, Esc cancels" };
            let _ = r.fill_rect([x - 12.0 * d, y - 36.0 * d, x + bw + 12.0 * d, y + bh + 14.0 * d + if err.is_some() { 24.0 * d } else { 0.0 }], rgb(0x0E0E0E, 0.92));
            self.text.draw(r, title, small, x, y - 28.0 * d, rgb(pal::DIM, 1.0));
            draw_box(r, &mut self.text, [x, y, x + bw, y + bh], &ti.text(), Some(&ti), "", font, d, err.is_some());
            if let Some(e) = err {
                self.text.draw(r, &e, small, x, y + bh + 8.0 * d, rgb(pal::ERROR, 1.0));
            }
        }
        if let Some(j) = job {
            let tw = self.text.width(&j, small) as f32 + 24.0 * d;
            let bh = line_h(small) + 12.0 * d;
            let x = (w - tw) / 2.0;
            let y = h - bh - 56.0 * d;
            let _ = r.fill_rect([x, y, x + tw, y + bh], rgb(0x0E0E0E, 0.92));
            outline(r, [x, y, x + tw, y + bh], 1.0, rgb(pal::ACCENT, 0.9));
            self.text.draw(r, &j, small, x + 12.0 * d, y + 6.0 * d, rgb(pal::TEXT, 1.0));
        }
        if let Some(t) = toast {
            let t = self.text.fit(&t, small, (w * 0.7) as i32);
            let tw = self.text.width(&t, small) as f32 + 24.0 * d;
            let bh = line_h(small) + 12.0 * d;
            let (x, y) = (14.0 * d + self.chrome.area[0], h - bh - 44.0 * d);
            let _ = r.fill_rect([x, y, x + tw, y + bh], rgb(0x0E0E0E, 0.92));
            self.text.draw(r, &t, small, x + 12.0 * d, y + 6.0 * d, rgb(pal::TEXT, 1.0));
        }
        if help {
            let lines = help_lines();
            let font = px(14.0, d);
            let lh = line_h(font);
            let tw = lines.iter().map(|l| self.text.width(l, font)).max().unwrap_or(0) as f32 + 40.0 * d;
            let th = lines.len() as f32 * lh + 30.0 * d;
            let (x, y) = (((w - tw) / 2.0).max(0.0), ((h - th) / 2.0).max(0.0));
            let _ = r.fill_rect([x, y, x + tw, y + th], rgb(0x0E0E0E, 0.95));
            for (i, l) in lines.iter().enumerate() {
                let c = if i == 0 { rgb(pal::TEXT, 1.0) } else { rgb(0xD0D4DA, 1.0) };
                self.text.draw(r, l, font, x + 20.0 * d, y + 15.0 * d + i as f32 * lh, c);
            }
        }
    }
}

/// A folder, a picture (from its thumbnail), or a card with the extension, fitted into `b` (with `pad` around it).
fn draw_visual(r: &mut Renderer, text: &mut Text, e: &Entry, b: [f32; 4], pad: f32, failed: bool, d: f32) {
    let inner = [b[0] + pad, b[1] + pad, b[2] - pad, b[3] - pad];
    if e.is_dir() {
        match r.thumb_size(e.id) {
            Some((tw, th)) if inner[2] - inner[0] > 60.0 * d => draw_folder_cover(r, inner, e.id, tw, th),
            _ => draw_folder(r, inner, 1.0),
        }
        return;
    }
    if let Some((tw, th)) = r.thumb_size(e.id) {
        let (cw, ch) = (inner[2] - inner[0], inner[3] - inner[1]);
        let s = (cw / tw as f32).min(ch / th as f32).min(1.0);
        let (w, h) = ((tw as f32 * s).round(), (th as f32 * s).round());
        let (l, t) = ((inner[0] + (cw - w) / 2.0).round(), (inner[1] + (ch - h) / 2.0).round());
        let _ = r.draw_thumb(e.id, [l, t, l + w, t + h]);
    } else if !e.has_thumb() || failed {
        draw_card(r, text, inner, &e.ext, failed && e.has_thumb(), d);
    }
}

fn help_lines() -> Vec<String> {
    [
        concat!("NuGallery ", env!("CARGO_PKG_VERSION"), " — keys and mouse"),
        "",
        "Mouse   wheel: previous / next file  ·  Shift+wheel: scroll  ·  Ctrl+wheel: thumbnail size (columns)",
        "        click: select  ·  Ctrl+click: add / remove  ·  Shift+click: range  ·  drag on empty space: select a box",
        "        double-click / middle click: view  ·  right click: menu  ·  edges: left = folders, right = details,",
        "        top = toolbar, bottom = Windows taskbar (pin a panel with its Pin button, or Ctrl+B / Ctrl+I / Ctrl+T)",
        "Keys    arrows / PgUp / PgDn / Home / End: move  ·  Shift+arrows: extend  ·  Ctrl+A: select all  ·  Esc: clear",
        "        Space: view in NuViewer  ·  Enter: next file  ·  F8: slideshow  ·  a letter: jump to the name  ·  Backspace / Alt+↑: up  ·  Alt+← / →: back / forward",
        "        1 2 3 4 5: thumbnail size  ·  `: reset size  ·  Ctrl+1 / 2 / 3: thumbnails / details / list  ·  Ctrl+J: sort  ·  Ctrl+R: shuffle",
        "        F2 rename  ·  Del Recycle Bin  ·  Shift+Del delete for good  ·  Ctrl+N new folder  ·  F5 refresh",
        "        Ctrl+C copy  ·  Ctrl+X cut  ·  Ctrl+V paste  ·  Ctrl+Shift+C copy path  ·  F6 move to  ·  F7 copy to  ·  Ctrl+P print",
        "        Ctrl+F search  ·  Ctrl+L address bar  ·  F11 fullscreen  ·  F1 this help  ·  Alt+F4 exit",
        "Many files selected: right click > Batch (rename, convert, change time stamp, remove metadata) and Compress",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_explorer() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(150 * 1024 * 1024), "150 MB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn detail_columns_are_ordered() {
        let (n, s, t, d, r) = columns([0.0, 0.0, 1200.0, 800.0], 1.0);
        assert!(n < s && s < t && t < d && d < r && r <= 1200.0);
    }

    #[test]
    fn the_help_names_the_version() {
        assert!(help_lines()[0].contains(env!("CARGO_PKG_VERSION")));
    }
}
