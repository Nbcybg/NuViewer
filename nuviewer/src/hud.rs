//! On-screen text: toasts, info panel, help, the rename box and the gallery status line.
//! Text is rasterised with GDI (`nv_win::text`) into overlay textures that are cached per frame.

use super::*;
use nv_win::text;
use std::hash::{Hash, Hasher};

const TOAST_MS: u32 = 3200;

const HELP: &[&str] = &[
    concat!("NuViewer ", env!("CARGO_PKG_VERSION"), " — keys and mouse"),
    "(these are the defaults: Settings, Ctrl+, , changes keys, mouse and more)",
    "",
    "←  →  Space  Backspace  PgUp  PgDn  Home  End     previous / next / first / last",
    "Ctrl+←  Ctrl+→     previous / next file (documents: skips the pages)",
    "Mouse wheel previous / next  ·  hold left button: zoom 100% (200% if small), drag to pan",
    "Ctrl + drag zoom into an area  ·  Ctrl + wheel zoom  ·  click while zoomed: back to the base view",
    "F fit  ·  A 100%  (pressing the one that is on switches to the other)  ·  1 2 3 4 5 = that many times fit or 100%  ·  ` resets",
    "[  ]  rotate −45° / +45°  ·  Shift+[  Shift+]  rotate 90°  ·  Ctrl+S save the rotation  ·  Ctrl+Z take it back",
    "V  viewer controls (they also open when the pointer touches the left edge)  ·  E  NuEditor (mock-up)",
    "3D: drag turns, Shift+drag moves, wheel zooms, F resets  ·  T texture  N normal map  W wireframe  Shift+S shadow  Shift+T shading  O ortho",
    "3D: X Y Z turn the model 90° about that axis (Shift = back)  ·  U other axis is up  ·  more in Settings > 3D",
    "Right click: menu (Open with…, Settings…)  ·  middle click: close  ·  top edge: info bar",
    "G  Tab  Enter     gallery  ·  arrows select, wheel scrolls, Ctrl+wheel sets the size",
    "Q  fold numbered pictures into image sequences (gallery)  ·  P play / pause  ·  C pin the controls  ·  D disk speed",
    "F11 / Alt+Enter fullscreen   I info/EXIF   S slideshow   + / − faster / slower   R shuffle",
    "F2 rename   Del Recycle Bin   Shift+Del delete permanently   Ctrl+Z undo   F5 copy to…   F6 / M move to…",
    "Ctrl+C copy image + file   Ctrl+Shift+C copy path   Ctrl+E show in Explorer",
    "Ctrl+,  /  F9  settings        Esc  close this / stop slideshow / back / quit        F1 / H  this help",
];

pub struct Hud {
    pub toast: Option<(String, Instant)>,
    pub show_info: bool,
    pub show_help: bool,
    pub info_rows: Vec<(String, String)>,
    /// Which file `info_rows` describes.
    pub info_for: Option<u64>,
    /// Overlay textures drawn this frame (others are dropped afterwards).
    pub used: HashSet<u64>,
    /// File size text of the current picture (id, text) so the top bar doesn't stat the file every frame.
    pub size_cache: Option<(u64, String)>,
}

impl Hud {
    pub fn new() -> Self {
        Hud { toast: None, show_info: false, show_help: false, info_rows: Vec::new(), info_for: None, used: HashSet::new(), size_cache: None }
    }
}

/// Keep long values (paths) readable: "…tail".
pub fn clip(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max { s.to_string() } else { format!("…{}", s.chars().skip(n - (max - 1)).collect::<String>()) }
}

impl App {
    pub fn toast(&mut self, msg: impl Into<String>) {
        self.hud.toast = Some((msg.into(), Instant::now()));
        set_timer(self.hwnd, TIMER_TOAST, TOAST_MS);
        invalidate(self.hwnd);
    }

    pub fn on_toast_timer(&mut self) {
        kill_timer(self.hwnd, TIMER_TOAST);
        self.hud.toast = None;
        invalidate(self.hwnd);
    }

    fn text_px(&self) -> i32 {
        (15.0 * self.gallery.dpi).round() as i32
    }

    /// Upload (once) and mark as used the bitmap for these lines. Returns (key, width, height).
    pub fn overlay(&mut self, lines: &[String]) -> Option<(u64, f32, f32)> {
        let px = self.text_px();
        let pad = (7.0 * self.gallery.dpi).round() as i32;
        self.overlay_px(lines, px, pad)
    }

    /// Smaller text (playback bar, badges).
    pub fn overlay_small(&mut self, lines: &[String]) -> Option<(u64, f32, f32)> {
        let d = self.gallery.dpi;
        self.overlay_px(lines, (13.0 * d).round() as i32, (4.0 * d).round() as i32)
    }

    /// One symbol or short label at `size` logical pixels (the viewer controls and editor palette).
    pub fn overlay_glyph(&mut self, text: &str, size: f32) -> Option<(u64, f32, f32)> {
        let d = self.gallery.dpi;
        self.overlay_px(&[text.to_string()], (size * d).round() as i32, (2.0 * d).round() as i32)
    }

    fn overlay_px(&mut self, lines: &[String], px: i32, pad: i32) -> Option<(u64, f32, f32)> {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (lines, px, pad).hash(&mut h);
        let key = h.finish();
        if !self.renderer.has_overlay(key) {
            let bmp = text::render_lines(lines, px, pad);
            self.renderer.upload_overlay(key, &nv_codecs::Image::new(bmp.width, bmp.height, bmp.rgba)).ok()?;
        }
        self.hud.used.insert(key);
        self.renderer.overlay_size(key).map(|(w, h)| (key, w as f32, h as f32))
    }

    fn refresh_info(&mut self) {
        let Some(i) = self.target() else {
            self.hud.info_rows.clear();
            return;
        };
        let id = self.id_at(i);
        if self.hud.info_for == Some(id) {
            return;
        }
        let path = self.folder.files[i].clone();
        let mut rows = nv_codecs::metadata::describe(&path, nv_win::utc_offset_minutes());
        if let Some(m) = self.meta.get(&id) {
            let at = rows.iter().position(|(k, _)| k == "Size").unwrap_or(rows.len());
            let dims = match m.reduced_from {
                Some((ow, oh)) => format!("{ow} × {oh}  (shown at {} × {}: reduced to fit the GPU)", m.width, m.height),
                None => format!("{} × {}", m.width, m.height),
            };
            rows.insert(at, ("Dimensions".to_string(), dims));
        }
        rows.push(("Position".to_string(), format!("{} of {}", i + 1, self.folder.files.len())));
        self.hud.info_rows = rows;
        self.hud.info_for = Some(id);
    }

    /// Draw every HUD element on top of the frame that `begin()` started. Call right before `end()`.
    pub fn draw_hud(&mut self) {
        self.hud.used.clear();
        let (w, h) = self.renderer.size();
        let (w, h) = (w as f32, h as f32);
        let d = self.gallery.dpi;
        let margin = 14.0 * d;
        let (text_c, dim_c, back_c) = (srgb(0xF2F2F2, 1.0), srgb(0x9AA0A6, 1.0), srgb(0x0E0E0E, 0.84));

        // Gallery: a status line with the selected file's name.
        if self.mode == Mode::Gallery && !self.folder.files.is_empty() {
            let sel = self.gallery.selected.min(self.folder.files.len() - 1);
            let name = file_name_of(&self.folder.files[sel]);
            let line = format!("{name}    {}/{}", sel + 1, self.folder.files.len());
            if let Some((key, tw, th)) = self.overlay(&[clip(&line, 90)]) {
                let (x, y) = ((w - tw) / 2.0, h - th - margin * 0.6);
                let _ = self.renderer.fill_rect([x, y, x + tw, y + th], back_c);
                let _ = self.renderer.draw_overlay(key, x, y, text_c);
            }
        }

        // Info / EXIF panel (top-left): labels in grey, values in white, as two aligned columns.
        if self.hud.show_info {
            self.refresh_info();
            let labels: Vec<String> = self.hud.info_rows.iter().map(|(k, _)| k.clone()).collect();
            let values: Vec<String> = self.hud.info_rows.iter().map(|(_, v)| clip(v, 80)).collect();
            if !labels.is_empty() {
                if let (Some((lk, lw, lh)), Some((vk, vw, _))) = (self.overlay(&labels), self.overlay(&values)) {
                    let (x, y, gap) = (margin, margin, 6.0 * d);
                    let _ = self.renderer.fill_rect([x, y, x + lw + gap + vw, y + lh], back_c);
                    let _ = self.renderer.draw_overlay(lk, x, y, dim_c);
                    let _ = self.renderer.draw_overlay(vk, x + lw + gap, y, text_c);
                }
            }
        }

        // Help (centred).
        if self.hud.show_help {
            let lines: Vec<String> = HELP.iter().map(|s| s.to_string()).collect();
            if let Some((key, tw, th)) = self.overlay(&lines) {
                let (x, y) = (((w - tw) / 2.0).max(0.0), ((h - th) / 2.0).max(0.0));
                let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.92));
                let _ = self.renderer.draw_overlay(key, x, y, text_c);
            }
        }

        // Rename box.
        if let Some(r) = &self.rename {
            let name: String = r.chars.iter().collect();
            let (cursor, sel, error) = (r.cursor, r.sel, r.error.clone());
            let prefix = |n: usize| -> String { name.chars().take(n).collect() };
            let px = self.text_px();
            let pad = (7.0 * d).round();
            let (sel_x, cur_x) = (sel.map(|(a, b)| (text::measure(&prefix(a), px) as f32, text::measure(&prefix(b), px) as f32)), text::measure(&prefix(cursor), px) as f32);
            if let (Some((tk, tw, th)), Some((hk, _, hh))) = (self.overlay(&[name.clone()]), self.overlay(&["Rename — Enter applies, Esc cancels".to_string()])) {
                let err = error.and_then(|e| self.overlay(&[e]));
                let box_w = (w * 0.5).max(tw + 2.0 * margin).min(w - 2.0 * margin);
                let total_h = hh + th + err.map(|(_, _, eh)| eh).unwrap_or(0.0);
                let (x, y) = ((w - box_w) / 2.0, h * 0.38);
                let _ = self.renderer.fill_rect([x, y, x + box_w, y + total_h], srgb(0x0E0E0E, 0.94));
                let _ = self.renderer.draw_overlay(hk, x, y, dim_c);
                let ty = y + hh;
                let tx = x + margin;
                if let Some((a, b)) = sel_x {
                    let _ = self.renderer.fill_rect([tx + pad + a, ty + pad * 0.6, tx + pad + b, ty + th - pad * 0.6], srgb(0x3D8BFF, 0.55));
                }
                let _ = self.renderer.draw_overlay(tk, tx, ty, text_c);
                let _ = self.renderer.fill_rect([tx + pad + cur_x, ty + pad * 0.6, tx + pad + cur_x + 2.0 * d, ty + th - pad * 0.6], text_c);
                if let Some((ek, _, _)) = err {
                    let _ = self.renderer.draw_overlay(ek, x, ty + th, srgb(0xFF7B7B, 1.0));
                }
            }
        }

        // Top bar: shown while the pointer is on the top edge (viewer only), like a title bar made of the picture's facts.
        if self.top_bar && self.mode == Mode::Viewer {
            if let Some(text) = self.top_bar_text() {
                if let Some((key, tw, th)) = self.overlay(&[clip(&text, 220)]) {
                    self.top_bar_h = th;
                    let _ = self.renderer.fill_rect([0.0, 0.0, w, th], srgb(0x0E0E0E, 0.88));
                    let _ = self.renderer.draw_overlay(key, ((w - tw) / 2.0).max(0.0), 0.0, text_c);
                }
            }
        }

        self.draw_disk_badge();

        // Playback bar, the viewer controls on the left edge, the NuEditor mock-up.
        self.draw_controls();
        self.draw_sidebar();
        self.draw_editor();

        // Toast (bottom-left).
        if let Some((msg, _)) = self.hud.toast.clone() {
            if let Some((key, tw, th)) = self.overlay(&[clip(&msg, 110)]) {
                let (x, y) = (margin, h - th - margin);
                let _ = self.renderer.fill_rect([x, y, x + tw, y + th], back_c);
                let _ = self.renderer.draw_overlay(key, x, y, text_c);
            }
        }

        let used = &self.hud.used;
        self.renderer.retain_overlays(|id| used.contains(&id));
    }
}

/// "3.2 MB" style size.
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// Megapixels the way cameras quote them ("12.2 MP"; "0.08 MP" for tiny pictures).
fn fmt_megapixels(w: u32, h: u32) -> String {
    let mp = w as f64 * h as f64 / 1_000_000.0;
    if mp >= 0.095 { format!("{mp:.1} MP") } else { format!("{mp:.2} MP") }
}

impl App {
    /// The one-line summary shown when the pointer touches the top edge: name, size in pixels and megapixels, file size,
    /// position in the folder, how fast it appeared, and the current zoom.
    pub fn top_bar_text(&mut self) -> Option<String> {
        let i = self.folder.index;
        let path = self.folder.files.get(i)?.clone();
        let id = self.id_at(i);
        let disk = match &self.hud.size_cache {
            Some((cid, s)) if *cid == id => s.clone(),
            _ => {
                let s = std::fs::metadata(&path).map(|m| fmt_bytes(m.len())).unwrap_or_default();
                self.hud.size_cache = Some((id, s.clone()));
                s
            }
        };
        let mut parts = vec![file_name_of(&path)];
        match self.meta.get(&id) {
            Some(m) => {
                let (w, h) = m.reduced_from.unwrap_or((m.width, m.height));
                match m.tris {
                    Some(t) if m.textures > 0 => parts.push(format!("3D model, {t} triangles, {} texture{}", m.textures, if m.textures == 1 { "" } else { "s" })),
                    Some(t) => parts.push(format!("3D model, {t} triangles")),
                    None => parts.push(format!("{w} × {h} ({})", fmt_megapixels(w, h))),
                }
                if let Some((p, n)) = m.page {
                    parts.push(format!("page {p}/{n}"));
                }
            }
            None => parts.push("loading…".into()),
        }
        if !disk.is_empty() {
            parts.push(disk);
        }
        parts.push(format!("{}/{}", i + 1, self.folder.files.len()));
        let mut speed = Vec::new();
        if let Some(m) = self.meta.get(&id) {
            speed.push(format!("decode {:.0} ms", m.decode_ms));
            if let Some((bytes, secs)) = m.read.filter(|&(b, s)| b >= 256 * 1024 && s > 0.02) {
                speed.push(format!("read {} in {:.2} s ({}/s)", fmt_bytes(bytes), secs, fmt_bytes((bytes as f64 / secs) as u64)));
            }
        }
        if let Some(ms) = self.last_nav_ms {
            speed.push(format!("shown in {ms:.0} ms"));
        }
        if !speed.is_empty() {
            parts.push(speed.join(", "));
        }
        if self.is_model() {
            parts.push(format!("zoom ×{:.1}", self.cam.zoom));
        } else if let Some(img) = self.view_size() {
            let scale = self.view.scale(img, self.renderer.size());
            parts.push(format!("{:.0}%", scale * 100.0));
        }
        Some(parts.join("  —  "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_shows_the_version_number() {
        assert!(HELP[0].starts_with("NuViewer "));
        assert!(HELP[0].contains(env!("CARGO_PKG_VERSION")), "{}", HELP[0]);
    }

    #[test]
    fn sizes_read_like_a_camera_and_like_explorer() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(fmt_megapixels(4000, 3000), "12.0 MP");
        assert_eq!(fmt_megapixels(640, 480), "0.3 MP");
        assert_eq!(fmt_megapixels(100, 100), "0.01 MP");
    }
}
