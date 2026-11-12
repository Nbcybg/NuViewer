//! The "loading" panel: while the picture the user asked for is still on its way, say so (and what is slow).
//!
//! A big picture can take seconds on a hard disk, and the viewer keeps showing the previous one meanwhile, which
//! looks exactly like a hang. After a short delay the old picture is dimmed and a panel shows the stage (waiting,
//! reading from disk, decoding, sending to the graphics card), how many bytes have been read and at what speed,
//! and the drive's overall read / write speed and busy percentage, so it is clear whether the disk is the
//! bottleneck. The same disk readout can stay on screen (setting / `D`).

use super::*;
use crate::hud::{clip, fmt_bytes};
use nv_codecs::progress::{STAGE_DECODING, STAGE_READING};

/// The panel only appears when loading takes longer than this (fast loads never flash it).
const LOAD_DELAY: Duration = Duration::from_millis(150);
/// Redraw rate of the panel while loading, and of the always-on disk readout.
const LOAD_TICK_MS: u32 = 100;
const DISK_TICK_MS: u32 = 500;
/// Drive busy percentage from which reading is called disk-bound.
const BUSY_BOTTLENECK: f64 = 85.0;

/// "37.2 MB/s".
fn rate(bytes_per_sec: f64) -> String {
    format!("{}/s", fmt_bytes(bytes_per_sec.max(0.0) as u64))
}

impl App {
    /// The picture on screen is not the one asked for yet (it is still being loaded).
    pub fn current_loading(&self) -> bool {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return false;
        }
        let id = self.id_at(self.folder.index);
        !self.renderer.has(id) && !self.failed.contains_key(&id)
    }

    /// A decoded picture is queued for, or being copied to, the GPU.
    pub fn upload_busy_with(&self, id: u64) -> bool {
        self.renderer.upload_progress().map(|(p, _)| p == id).unwrap_or(false) || self.staged.iter().any(|s| s.id == id)
    }

    /// Start / stop the loading clock and the timer that redraws the panel; tell the disk sampler which drive matters.
    pub fn sync_loading(&mut self) {
        let loading = self.current_loading();
        if loading {
            let id = self.id_at(self.folder.index);
            if self.loading_since.map(|(i, _)| i) != Some(id) {
                self.loading_since = Some((id, Instant::now()));
            }
        } else {
            self.loading_since = None;
        }
        let show_disk = self.settings.disk_monitor && self.mode == Mode::Viewer && !self.folder.files.is_empty();
        if loading || show_disk {
            if let Some(p) = self.folder.files.get(self.folder.index) {
                nv_win::diskstat::watch(p);
            }
        }
        if loading {
            set_timer(self.hwnd, TIMER_LOAD, LOAD_TICK_MS);
        } else if show_disk {
            set_timer(self.hwnd, TIMER_LOAD, DISK_TICK_MS);
        } else {
            kill_timer(self.hwnd, TIMER_LOAD);
        }
    }

    /// `TIMER_LOAD`: redraw (progress numbers change), and notice when loading ended without us being told.
    pub fn load_tick(&mut self) {
        self.sync_loading();
        if self.loading_since.is_some() || (self.settings.disk_monitor && self.mode == Mode::Viewer) {
            if self.loading_since.is_some() {
                set_title(self.hwnd, &self.title());
            }
            invalidate(self.hwnd);
        }
    }

    /// The textures of 3D models were switched on / off: pictures cached under the old setting are stale.
    pub fn reload_models(&mut self) {
        self.renderer.clear_cache();
        self.meta.clear();
        self.failed.clear();
        self.sizes.clear();
        self.staged.clear();
        self.uploading = None;
        let _ = self.renderer.cancel_upload();
        if self.mode == Mode::Viewer {
            self.refresh();
        }
    }

    /// One short phrase for the window title: "reading 45% (12.0 MB of 26.6 MB, 37.2 MB/s) · disk F: read 37.0 MB/s write 0 B/s busy 98%".
    pub fn loading_summary(&self, id: u64) -> String {
        if self.folder.files.is_empty() {
            return String::new();
        }
        let mut s = if let Some((_, f)) = self.renderer.upload_progress().filter(|(p, _)| *p == id) {
            format!("sending to the graphics card {:.0}%", f * 100.0)
        } else if self.staged.iter().any(|st| st.id == id) {
            "waiting to send to the graphics card".to_string()
        } else if let Some(st) = self.loader.state_of(&self.load_path(self.folder.index)) {
            let p = st.progress;
            if st.queued {
                "waiting for a free decoder".to_string()
            } else if p.stage == STAGE_READING && p.total > 0 {
                format!("reading {:.0}% ({} of {}, {})", p.read as f64 * 100.0 / p.total as f64, fmt_bytes(p.read), fmt_bytes(p.total), rate(p.bytes_per_sec))
            } else if p.stage == STAGE_DECODING {
                format!("decoding {:.1} s", p.decode_secs)
            } else {
                format!("opening {:.1} s", p.elapsed_secs)
            }
        } else {
            String::new()
        };
        if let Some(d) = nv_win::diskstat::stats() {
            s += &format!(" · disk {}: read {} write {} busy {:.0}%", d.drive, rate(d.read_bps), rate(d.write_bps), d.busy_pct);
        }
        s
    }

    /// The text lines of the panel and how far along the stage is (None = no measurable progress).
    fn loading_lines(&self, id: u64) -> (Vec<String>, Option<f32>, bool) {
        let name = file_name_of(&self.folder.files[self.folder.index]);
        let mut lines = vec![format!("Loading  {}", clip(&name, 56))];
        let mut fraction = None;
        let mut disk_bound = false;
        let busy = nv_win::diskstat::stats().map(|d| d.busy_pct).unwrap_or(0.0);
        if let Some((_, f)) = self.renderer.upload_progress().filter(|(p, _)| *p == id) {
            lines.push(format!("Sending to the graphics card…  {:.0}%", f * 100.0));
            fraction = Some(f);
        } else if self.staged.iter().any(|s| s.id == id) {
            lines.push("Waiting to send to the graphics card…".to_string());
        } else if let Some(st) = self.loader.state_of(&self.load_path(self.folder.index)) {
            let p = st.progress;
            if st.queued {
                lines.push("Waiting for a free decoder…".to_string());
            } else if p.stage == STAGE_READING && p.total > 0 {
                let f = (p.read as f64 / p.total as f64).clamp(0.0, 1.0) as f32;
                fraction = Some(f);
                lines.push(format!("Reading from disk   {} of {}   ({:.0}%)", fmt_bytes(p.read), fmt_bytes(p.total), f * 100.0));
                lines.push(format!("{}   ·   {:.1} s", if p.bytes_per_sec > 0.0 { rate(p.bytes_per_sec) } else { "measuring…".into() }, p.read_secs));
                disk_bound = busy >= BUSY_BOTTLENECK;
            } else if p.stage == STAGE_DECODING {
                let read = if p.total > 0 && p.read >= p.total && p.bytes_per_sec > 0.0 {
                    format!("   (file read: {} in {:.1} s, {})", fmt_bytes(p.total), p.read_secs, rate(p.bytes_per_sec))
                } else {
                    String::new()
                };
                lines.push(format!("Decoding…  {:.1} s{read}", p.decode_secs));
            } else {
                lines.push(format!("Opening…  {:.1} s", p.elapsed_secs));
            }
        } else {
            lines.push("Starting…".to_string());
        }
        if let Some(d) = nv_win::diskstat::stats() {
            lines.push(format!("Disk {}:   read {}   write {}   busy {:.0}%", d.drive, rate(d.read_bps), rate(d.write_bps), d.busy_pct));
        }
        if disk_bound {
            lines.push("The disk is the bottleneck: it is already busy reading.".to_string());
        }
        (lines, fraction, disk_bound)
    }

    /// Dim the old picture and draw the progress panel (viewer only; called between `begin` and `end`).
    pub fn draw_loading(&mut self) {
        let Some((id, since)) = self.loading_since else { return };
        if since.elapsed() < LOAD_DELAY || !self.current_loading() || self.folder.files.is_empty() {
            return;
        }
        let (w, h) = self.renderer.size();
        let (w, h) = (w as f32, h as f32);
        let d = self.gallery.dpi;
        // The previous picture is still up: make it obvious that it is not the one that was asked for.
        if self.renderer.image_size().is_some() {
            let _ = self.renderer.fill_rect([0.0, 0.0, w, h], srgb(0x000000, 0.7));
        }
        let (lines, fraction, disk_bound) = self.loading_lines(id);
        let Some((key, tw, th)) = self.overlay(&lines) else { return };
        let bar_h = 8.0 * d;
        let gap = 8.0 * d;
        let panel_w = tw.max(380.0 * d);
        let panel_h = th + gap + bar_h + gap;
        let (x, y) = (((w - panel_w) / 2.0).round(), ((h - panel_h) / 2.0).round());
        let _ = self.renderer.fill_rect([x, y, x + panel_w, y + panel_h], srgb(0x0E0E0E, 0.93));
        let _ = self.renderer.draw_overlay(key, x + (panel_w - tw) / 2.0, y, srgb(0xF2F2F2, 1.0));
        // Progress bar: a filled track when we know how far along it is, a sweeping block when we don't.
        let (bx0, bx1, by) = (x + gap, x + panel_w - gap, y + th + gap * 0.5);
        let accent = if disk_bound { srgb(0xFFA94D, 1.0) } else { srgb(0x3D8BFF, 1.0) };
        let _ = self.renderer.fill_rect([bx0, by, bx1, by + bar_h], srgb(0x34383F, 1.0));
        let span = bx1 - bx0;
        match fraction {
            Some(f) => {
                let _ = self.renderer.fill_rect([bx0, by, bx0 + span * f, by + bar_h], accent);
            }
            None => {
                let t = since.elapsed().as_secs_f32();
                let head = (t * 0.9).fract();
                let block = span * 0.28;
                let l = bx0 + (span + block) * head - block;
                let (l, r) = (l.max(bx0), (l + block).min(bx1));
                if r > l {
                    let _ = self.renderer.fill_rect([l, by, r, by + bar_h], accent);
                }
            }
        }
    }

    /// Bottom-right readout of the drive's read / write speed (the setting / `D`). Called from `draw_hud`.
    pub fn draw_disk_badge(&mut self) {
        if !self.settings.disk_monitor || self.mode != Mode::Viewer {
            return;
        }
        let Some(ds) = nv_win::diskstat::stats() else { return };
        let text = format!("Disk {}:   read {}   write {}   busy {:.0}%", ds.drive, rate(ds.read_bps), rate(ds.write_bps), ds.busy_pct);
        let (w, h) = self.renderer.size();
        let d = self.gallery.dpi;
        if let Some((key, tw, th)) = self.overlay_small(&[text]) {
            let (x, y) = (w as f32 - tw - 14.0 * d, h as f32 - th - 36.0 * d); // above the row kept for the taskbar reveal
            let hot = if ds.busy_pct >= BUSY_BOTTLENECK { srgb(0xFFA94D, 1.0) } else { srgb(0xF2F2F2, 1.0) };
            let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.8));
            let _ = self.renderer.draw_overlay(key, x, y, hot);
        }
    }
}
