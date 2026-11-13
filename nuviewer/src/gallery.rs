//! Gallery mode: the grid of thumbnails. Everything that is only about the gallery lives here, so it can grow (and later
//! move into its own program) without touching the viewer.
//!
//! What the gallery needs from the rest of the app, and nothing else:
//! * the folder listing (`folder`) and which file is current (`folder.index`);
//! * the renderer's thumbnail textures (`renderer.upload_thumb`, `draw_thumb`, ...) and 2D drawing calls;
//! * a pool of thumbnail workers (`nv_core::Loader::thumbs`), started the first time the gallery opens, so a viewer
//!   that never shows the gallery never starts them;
//! * `hud` for text. The only thing it hands back is "open file N" (`leave_gallery(true)`).
//!
//! `docs/gallery-split.md` describes the plan for turning this into a separate gallery process.

use super::*;

/// Gallery metrics in logical pixels (multiplied by the monitor's DPI scale).
pub(crate) const GALLERY_CELL: f32 = 176.0;
pub(crate) const GALLERY_CELL_MIN: f32 = 96.0;
pub(crate) const GALLERY_CELL_MAX: f32 = 264.0;
pub(crate) const GALLERY_GAP: f32 = 10.0;
pub(crate) const GALLERY_MARGIN: f32 = 16.0;
/// Rows of thumbnails requested beyond the visible ones (and kept on the GPU, plus a bit more).
pub(crate) const PREFETCH_ROWS: usize = 2;
pub(crate) const KEEP_ROWS: usize = 4;


pub(crate) struct Gallery {
    pub(crate) cell: f32,
    pub(crate) selected: usize,
    pub(crate) scroll: f32,
    pub(crate) dpi: f32,
    /// Thumbnails that failed to generate (shown as a dark-red tile).
    pub(crate) failed: HashSet<u64>,
    /// Ids of thumbnails currently worth keeping on the GPU.
    pub(crate) keep: HashSet<u64>,
    pub(crate) req_start: Option<Instant>,
    pub(crate) ready_ms: Option<f64>,
}

impl App {
    // ---- gallery ---------------------------------------------------------------------------------

    pub(crate) fn layout(&self) -> Layout {
        let (w, h) = self.renderer.size();
        let d = self.gallery.dpi;
        Layout::new(self.items.len(), self.gallery.cell, GALLERY_GAP * d, GALLERY_MARGIN * d, w as f32, h as f32)
    }

    /// File index shown by gallery tile `tile`.
    pub(crate) fn file_of(&self, tile: usize) -> usize {
        self.items.get(tile).copied().unwrap_or(0)
    }

    /// Gallery tile that contains file `file` (a frame in the middle of a folded sequence belongs to its first frame's tile).
    pub(crate) fn tile_of(&self, file: usize) -> usize {
        match self.items.binary_search(&file) {
            Ok(t) => t,
            Err(t) => t.saturating_sub(1),
        }
    }

    /// The folded sequence containing file `file`, if sequences are folded.
    pub(crate) fn run_of(&self, file: usize) -> Option<Run> {
        self.runs.iter().find(|r| (r.start..=r.end).contains(&file)).cloned()
    }

    /// Recompute which gallery tiles exist (after the folder or the sequence setting changed); keeps the selection on a tile.
    pub(crate) fn rebuild_items(&mut self) {
        self.read.invalidate(); // the list changed: read mode's spreads must be worked out from the anchor again
        self.runs = if self.settings.sequences { sequence::find_runs(&self.folder.files, self.settings.sequence_min as usize) } else { Vec::new() };
        self.items = sequence::gallery_items(self.folder.files.len(), &self.runs);
        if !self.items.is_empty() {
            let sel = self.gallery.selected.min(self.folder.files.len() - 1);
            self.gallery.selected = self.items[self.tile_of(sel)];
        }
    }

    pub(crate) fn gallery_title(&self) -> String {
        let n = self.items.len();
        if n == 0 {
            return "NuViewer — no images".into();
        }
        let sel = self.gallery.selected.min(self.folder.files.len() - 1);
        let name = self.folder.files[sel].file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let seq = self.run_of(sel).map(|r| format!(" (sequence, {} frames)", r.len())).unwrap_or_default();
        let vis = self.layout().visible(self.gallery.scroll);
        let loaded = vis.clone().filter(|&i| self.renderer.has_thumb(self.id_at(self.file_of(i)))).count();
        let ready = self.gallery.ready_ms.map(|m| format!(" in {m:.0} ms")).unwrap_or_default();
        format!("{name}{seq} — {}/{n} — gallery — thumbs {loaded}/{}{ready} — NuViewer", self.tile_of(sel) + 1, vis.len())
    }

    /// Keep thumbnails for the visible rows (plus margin) on the GPU, and queue the missing ones, visible first.
    pub(crate) fn request_thumbs(&mut self) {
        if self.folder.files.is_empty() {
            return;
        }
        let lay = self.layout();
        let scroll = self.gallery.scroll;
        let vis = lay.visible(scroll);
        let near = lay.visible_with_margin(scroll, PREFETCH_ROWS);
        let (items, files) = (&self.items, &self.folder.files);
        self.gallery.keep = lay.visible_with_margin(scroll, KEEP_ROWS).map(|i| path_id(&files[items[i]])).collect();
        let keep = &self.gallery.keep;
        self.renderer.retain_thumbs(|id| keep.contains(&id));

        let order = vis.clone().chain(near.filter(|i| !vis.contains(i)));
        let todo: Vec<PathBuf> = order
            .filter(|&i| {
                let id = path_id(&self.folder.files[self.items[i]]);
                !self.renderer.has_thumb(id) && !self.gallery.failed.contains(&id)
            })
            .map(|i| self.folder.files[self.items[i]].clone())
            .collect();
        self.thumb_request(todo);
    }

    pub(crate) fn enter_gallery(&mut self) {
        if self.folder.files.is_empty() || !self.settle_rotation() {
            return;
        }
        self.editor.open = false;
        self.stop_slideshow(false);
        self.mode = Mode::Gallery;
        self.sync_anim(); // stops the timer
        self.doc_page = 1; // the gallery shows first pages
        self.rebuild_items();
        self.seq_playing = false;
        kill_timer(self.hwnd, TIMER_SEQ);
        self.gallery.selected = self.items.get(self.tile_of(self.folder.index)).copied().unwrap_or(0);
        self.gallery.scroll = self.layout().scroll_to_center(self.tile_of(self.gallery.selected));
        self.gallery.req_start = Some(Instant::now());
        self.gallery.ready_ms = None;
        self.loader.request(Vec::new()); // stop prefetching full images while browsing
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    /// Back to the viewer; `open` shows the gallery's selection, otherwise the image stays as it was.
    pub(crate) fn leave_gallery(&mut self, open: bool) {
        self.mode = Mode::Viewer;
        self.thumb_request(Vec::new());
        self.strip.requested = None; // the film strip asks for its own thumbnails again
        if open && self.gallery.selected != self.folder.index {
            // Don't leave the previously viewed picture on screen while the chosen one decodes.
            if !self.renderer.has(self.id_at(self.gallery.selected)) {
                self.renderer.hide_current();
            }
            self.navigate(self.gallery.selected as isize);
        } else {
            self.refresh();
        }
    }

    /// Select gallery tile `tile` (clamped) and scroll it into view.
    pub(crate) fn gallery_select(&mut self, tile: usize) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let tile = tile.min(n - 1);
        self.gallery.selected = self.items[tile];
        let lay = self.layout();
        self.gallery.scroll = lay.scroll_to_show(tile, lay.clamp_scroll(self.gallery.scroll));
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    pub(crate) fn gallery_scroll_by(&mut self, pixels: f32) {
        let lay = self.layout();
        self.gallery.scroll = lay.clamp_scroll(self.gallery.scroll + pixels);
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    pub(crate) fn gallery_zoom(&mut self, notches: f32) {
        let d = self.gallery.dpi;
        self.gallery.cell = (self.gallery.cell * 1.1f32.powf(notches)).clamp(GALLERY_CELL_MIN * d, GALLERY_CELL_MAX * d);
        let lay = self.layout();
        self.gallery.scroll = lay.scroll_to_show(self.tile_of(self.gallery.selected), lay.clamp_scroll(self.gallery.scroll));
        self.request_thumbs();
        invalidate(self.hwnd);
    }

    /// Keys that move by whole rows in the gallery (not remappable: they only make sense in a grid).
    pub(crate) fn gallery_key(&mut self, k: u32) {
        let lay = self.layout();
        let (sel, cols) = (self.tile_of(self.gallery.selected), lay.cols);
        match k {
            VK_DOWN => self.gallery_select(sel + cols),
            VK_UP => self.gallery_select(sel.saturating_sub(cols)),
            _ => {}
        }
    }

    /// One screen of tiles (for the Page keys in the gallery).
    pub(crate) fn gallery_page(&self) -> usize {
        let lay = self.layout();
        ((lay.view_h / (lay.cell + lay.gap)) as usize).saturating_sub(1).max(1) * lay.cols
    }

    pub(crate) fn draw_gallery(&mut self) {
        if !self.renderer.begin(clear_color()) {
            return;
        }
        let lay = self.layout();
        let scroll = lay.clamp_scroll(self.gallery.scroll);
        let d = self.gallery.dpi;
        let tile = srgb(0x2A2A2A, 1.0);
        let tile_failed = srgb(0x4A2222, 1.0);
        let accent = srgb(0x3D8BFF, 1.0);
        for i in lay.visible(scroll) {
            let r = lay.rect(i, scroll);
            let fi = self.items[i];
            let id = self.id_at(fi);
            if fi == self.gallery.selected {
                let b = 4.0 * d;
                let _ = self.renderer.fill_rect([r[0] - b, r[1] - b, r[2] + b, r[3] + b], accent);
            }
            let _ = self.renderer.fill_rect(r, if self.gallery.failed.contains(&id) { tile_failed } else { tile });
            if let Some((tw, th)) = self.renderer.thumb_size(id) {
                // Fit inside the cell, never enlarging beyond the thumbnail's own pixels; snap to whole pixels.
                let (cw, ch) = (r[2] - r[0], r[3] - r[1]);
                let s = (cw / tw as f32).min(ch / th as f32).min(1.0);
                let (w, h) = ((tw as f32 * s).round(), (th as f32 * s).round());
                let (l, t) = ((r[0] + (cw - w) / 2.0).round(), (r[1] + (ch - h) / 2.0).round());
                let _ = self.renderer.draw_thumb(id, [l, t, l + w, t + h]);
            }
            // A folded image sequence: show how many frames it holds.
            if let Some(run) = self.runs.iter().find(|r| r.start == fi).cloned() {
                if let Some((key, tw, th)) = self.overlay_small(&[format!("\u{25B6} {}", run.len())]) {
                    let (x, y) = (r[2] - tw - 4.0 * d, r[3] - th - 4.0 * d);
                    let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.82));
                    let _ = self.renderer.draw_overlay(key, x, y, srgb(0xF2F2F2, 1.0));
                }
            }
        }
        self.draw_hud();
        let _ = self.renderer.end();
    }
}

impl App {
    /// The thumbnail workers, started on first use.
    fn thumb_pool(&mut self) -> &Loader {
        if self.thumb_loader.is_none() {
            self.thumb_loader = Some(Loader::thumbs(worker_threads(), self.wake.clone()));
        }
        self.thumb_loader.as_ref().expect("just created")
    }

    /// Ask for exactly these thumbnails (an empty list cancels everything queued). Never starts workers just to cancel.
    pub(crate) fn thumb_request(&mut self, todo: Vec<PathBuf>) {
        if todo.is_empty() && self.thumb_loader.is_none() {
            return;
        }
        self.thumb_pool().request(todo);
    }

    /// A finished thumbnail, if one is waiting.
    pub(crate) fn next_thumb(&mut self) -> Option<nv_core::Loaded> {
        self.thumb_loader.as_ref().and_then(|l| l.try_recv())
    }
}
