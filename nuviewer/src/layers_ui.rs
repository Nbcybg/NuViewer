//! The layer panel (`Ctrl+L`): every layer of a PSD / XCF / OpenRaster file with an eye, a name and an opacity slider.
//!
//!  * click the eye or the name: layer on / off (a group switches as one); the arrow folds a group;
//!  * drag the slider: opacity 0..1 (double click: as the file has it, triple click: 0); right click on a row: only this layer;
//!  * the filter box narrows the list by name; the pointer resting on a row **highlights** that layer on the picture (the rest is
//!    dimmed, an outline marks its box), even when it is switched off;
//!  * *Save* writes what is on screen as a picture with the video-snapshot settings (transparency is kept when the format can carry it,
//!    otherwise the picture is put on the viewer's canvas colour).
//!
//! The pixels are put together on a worker thread (`nv_codecs::layerdoc::compose`); one job runs at a time and a change made while
//! it runs starts the next one when it ends, so dragging a slider never queues up work. The picture on screen is replaced by uploading
//! the flattened result under the same id; leaving the file (or closing the panel) drops that texture so the file's own picture
//! is read again.

use super::*;
use nv_codecs::layerdoc::{self, LayerDoc};
use nv_core::layers::LayerSet;

#[derive(Clone, PartialEq, Debug)]
pub enum State {
    /// The picture in front has no layers (or the panel is off).
    Idle,
    Loading,
    Failed(String),
    Ready,
}

pub struct Layers {
    pub open: bool,
    pub state: State,
    /// The file the state below belongs to.
    pub path: Option<PathBuf>,
    pub doc: Option<Arc<LayerDoc>>,
    pub set: LayerSet,
    /// First row shown.
    pub scroll: usize,
    pub filter_focus: bool,
    pub hot: Option<Hit>,
    /// The row whose slider is being dragged.
    pub drag: Option<usize>,
    /// The rows are the slots of the Spine skeleton on screen, not the layers of a file.
    pub rig_mode: bool,
    doc_rx: Option<mpsc::Receiver<(PathBuf, Result<LayerDoc, String>)>>,
    img_rx: Option<mpsc::Receiver<(u64, nv_codecs::Image)>>,
    gen: u64,
    busy: bool,
    dirty: bool,
    /// The texture on the GPU is one this panel made (not the file's own picture).
    applied: bool,
    /// Set by a test (`NUVIEWER_TEST=1`): the title says what the panel shows.
    test: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Eye(usize),
    Fold(usize),
    Name(usize),
    Slider(usize),
    Filter,
    ClearFilter,
    Close,
    AllOn,
    AllOff,
    Reset,
    Save,
    AnimPrev,
    AnimNext,
    SkinNext,
    Play,
    Speed,
    Loop,
    PingPong,
    Frames,
    Webm,
}

impl Layers {
    pub fn new() -> Layers {
        Layers { open: false, state: State::Idle, path: None, doc: None, set: LayerSet::default(), scroll: 0, filter_focus: false, hot: None, drag: None, rig_mode: false, doc_rx: None, img_rx: None, gen: 0, busy: false, dirty: false, applied: false, test: std::env::var_os("NUVIEWER_TEST").is_some() }
    }
}

fn inside(r: &[f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

/// Everything clickable, from the window size.
pub struct Lay {
    pub panel: [f32; 4],
    close: [f32; 4],
    filter: [f32; 4],
    clear: [f32; 4],
    buttons: [[f32; 4]; 4],
    /// Spine: more buttons, and the line above the list.
    extras: Vec<([f32; 4], Hit, &'static str)>,
    info: [f32; 4],
    pub list: [f32; 4],
    pub row_h: f32,
    footer: [f32; 4],
    d: f32,
}

/// The parts of one row.
struct RowGeo {
    row: [f32; 4],
    fold: [f32; 4],
    eye: [f32; 4],
    name: [f32; 4],
    slider: [f32; 4],
}

impl App {
    pub fn layers_visible(&self) -> bool {
        self.layers.open && self.mode == Mode::Viewer && !self.editor.open && !self.folder.files.is_empty() && self.layers.state != State::Idle
    }

    pub fn layers_layout(&self) -> Lay {
        let (w, h) = self.renderer.size();
        let (w, h, d) = (w as f32, h as f32, self.gallery.dpi);
        let width = (340.0 * d).min(w * 0.6);
        let panel = [w - width - 8.0 * d, 8.0 * d, w - 8.0 * d, (h - 8.0 * d).max(120.0 * d)];
        let pad = 10.0 * d;
        let (l, r) = (panel[0] + pad, panel[2] - pad);
        let close = [r - 24.0 * d, panel[1] + 8.0 * d, r, panel[1] + 32.0 * d];
        let filter = [l, panel[1] + 40.0 * d, r, panel[1] + 68.0 * d];
        let clear = [filter[2] - 24.0 * d, filter[1], filter[2], filter[3]];
        let bw = (r - l - 3.0 * 6.0 * d) / 4.0;
        let by = filter[3] + 8.0 * d;
        let buttons = std::array::from_fn(|i| [l + i as f32 * (bw + 6.0 * d), by, l + i as f32 * (bw + 6.0 * d) + bw, by + 26.0 * d]);
        let footer_h = 76.0 * d;
        let mut extras = Vec::new();
        let mut top = by + 34.0 * d;
        let mut info = [0.0; 4];
        if self.layers.rig_mode {
            let rows: [&[(Hit, &'static str)]; 3] = [&[(Hit::AnimPrev, "\u{25C2} Anim"), (Hit::AnimNext, "Anim \u{25B8}"), (Hit::SkinNext, "Skin \u{25B8}"), (Hit::Play, "Play / Pause")], &[(Hit::Speed, "Speed"), (Hit::Loop, "Loop"), (Hit::PingPong, "Ping-pong")], &[(Hit::Frames, "PNG frames"), (Hit::Webm, "WebM alpha")]];
            for row in rows {
                for (i, (hit, label)) in row.iter().enumerate() {
                    extras.push(([l + i as f32 * (bw + 6.0 * d), top, l + i as f32 * (bw + 6.0 * d) + bw, top + 26.0 * d], *hit, *label));
                }
                top += 32.0 * d;
            }
            info = [l, top, r, top + 22.0 * d];
            top += 26.0 * d;
        }
        let list = [l, top, r, panel[3] - footer_h - 6.0 * d];
        let footer = [l, panel[3] - footer_h, r, panel[3] - 6.0 * d];
        Lay { panel, close, filter, clear, buttons, extras, info, list, row_h: 26.0 * d, footer, d }
    }

    fn layers_row_geo(&self, lay: &Lay, slot: usize, depth: u32) -> RowGeo {
        let d = lay.d;
        let top = lay.list[1] + slot as f32 * lay.row_h;
        let row = [lay.list[0], top, lay.list[2], top + lay.row_h];
        let x0 = row[0] + depth as f32 * 12.0 * d;
        let fold = [x0, top, x0 + 16.0 * d, top + lay.row_h];
        let eye = [fold[2], top, fold[2] + 24.0 * d, top + lay.row_h];
        let slider = [row[2] - 92.0 * d, top + 4.0 * d, row[2] - 4.0 * d, top + lay.row_h - 4.0 * d];
        let name = [eye[2] + 2.0 * d, top, slider[0] - 8.0 * d, top + lay.row_h];
        RowGeo { row, fold, eye, name, slider }
    }

    fn layers_slots(&self, lay: &Lay) -> usize {
        ((lay.list[3] - lay.list[1]) / lay.row_h).floor().max(1.0) as usize
    }

    /// The rows now in the list, and the first one shown (kept inside what exists).
    fn layers_rows(&mut self, lay: &Lay) -> Vec<nv_core::layers::Row> {
        let rows = self.layers.set.rows();
        let slots = self.layers_slots(lay);
        self.layers.scroll = self.layers.scroll.min(rows.len().saturating_sub(slots));
        rows
    }

    // ---- opening, closing, following the picture ---------------------------------------------------------------------------

    pub fn toggle_layers(&mut self) {
        if self.layers.open {
            self.layers_close();
            return;
        }
        let Some(path) = self.folder.files.get(self.folder.index).cloned() else { return };
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !layerdoc::is_layered_ext(&ext) && !self.anim_active() {
            self.toast("This picture has no layers we can show (PSD, XCF and OpenRaster files do)");
            return;
        }
        self.layers.open = true;
        self.layers_sync();
        invalidate(self.hwnd);
    }

    pub fn layers_close(&mut self) {
        self.layers.open = false;
        self.layers.filter_focus = false;
        self.layers.set.hover = None;
        self.spine_set_highlight(None);
        self.layers_leave_file();
        self.layers.state = State::Idle;
        self.layers.path = None;
        invalidate(self.hwnd);
    }

    /// Stop what belongs to the file we were on; its edited texture must not stay on the GPU as if it were the file's picture.
    fn layers_leave_file(&mut self) {
        self.layers.gen += 1;
        self.layers.doc_rx = None;
        self.layers.img_rx = None;
        self.layers.busy = false;
        self.layers.dirty = false;
        self.layers.drag = None;
        if self.layers.applied {
            self.layers.applied = false;
            if let Some(p) = self.layers.path.clone() {
                let id = path_id(&p);
                self.renderer.forget(id);
                self.sizes.remove(&id);
                self.refresh();
            }
        }
        self.layers.doc = None;
        self.layers.set = LayerSet::default();
        self.layers.scroll = 0;
    }

    /// Called often (every redraw and wake-up): follow the picture in front, collect finished work.
    pub fn layers_sync(&mut self) {
        if !self.layers.open {
            return;
        }
        let cur = self.folder.files.get(self.folder.index).cloned();
        let rig_here = cur.is_some() && self.anim_path() == cur;
        if self.layers.path != cur || rig_here != self.layers.rig_mode {
            self.layers_leave_file();
            self.layers.path = cur.clone();
            self.layers.rig_mode = rig_here;
            if rig_here {
                // The slots of the skeleton, top of the draw order first.
                let names: Vec<(String, String)> = self.anim_nodes();
                let nodes: Vec<nv_core::layers::Node> = names.into_iter().map(|(name, mode)| nv_core::layers::Node { name, parent: None, group: false, on0: true, opacity0: 1.0, on: true, opacity: 1.0, bounds: None, mode }).collect();
                let n = nodes.len();
                self.layers.set = LayerSet { nodes, hover: None, solo: None, filter: String::new(), collapsed: vec![false; n] };
                self.layers.state = State::Ready;
                invalidate(self.hwnd);
                return;
            }
            let layered = cur.as_ref().and_then(|p| p.extension()).map(|e| layerdoc::is_layered_ext(&e.to_string_lossy())).unwrap_or(false);
            match (cur, layered) {
                (Some(p), true) => {
                    self.layers.state = State::Loading;
                    let (tx, rx) = mpsc::channel();
                    let wake = self.wake.clone();
                    self.layers.doc_rx = Some(rx);
                    std::thread::spawn(move || {
                        let r = layerdoc::read(&p);
                        let _ = tx.send((p, r));
                        wake();
                    });
                }
                _ => self.layers.state = State::Idle,
            }
            invalidate(self.hwnd);
        }
        if let Some(rx) = &self.layers.doc_rx {
            if let Ok((p, r)) = rx.try_recv() {
                self.layers.doc_rx = None;
                if self.layers.path.as_ref() == Some(&p) {
                    match r {
                        Ok(doc) => {
                            self.layers.set = LayerSet::from_doc(&doc);
                            self.layers.doc = Some(Arc::new(doc));
                            self.layers.state = State::Ready;
                        }
                        Err(e) => self.layers.state = State::Failed(e),
                    }
                    invalidate(self.hwnd);
                }
            }
        }
        let mut done = None;
        if let Some(rx) = &self.layers.img_rx {
            if let Ok(r) = rx.try_recv() {
                done = Some(r);
            }
        }
        if let Some((gen, img)) = done {
            self.layers.img_rx = None;
            self.layers.busy = false;
            if gen == self.layers.gen {
                if let Some(p) = self.layers.path.clone() {
                    let id = path_id(&p);
                    match self.renderer.upload(id, &img) {
                        Ok(()) => {
                            self.layers.applied = true;
                            self.renderer.show(id);
                            invalidate(self.hwnd);
                        }
                        Err(e) => {
                            self.layers.state = State::Failed(format!("the flattened picture does not fit the graphics card: {e}"));
                            self.layers.dirty = false;
                        }
                    }
                }
            }
            if self.layers.dirty {
                self.layers_kick();
            }
        }
    }

    /// The choice changed (or the highlight moved): put the picture together again, once the previous job is done.
    pub fn layers_changed(&mut self) {
        if self.layers.rig_mode {
            let hover = self.layers.set.hover;
            self.spine_set_highlight(hover);
            let eff: Vec<_> = self.layers.set.eff(true).into_iter().rev().collect();
            self.anim_apply(&eff);
            invalidate(self.hwnd);
            return;
        }
        self.layers.dirty = true;
        self.layers_kick();
        invalidate(self.hwnd);
    }

    fn layers_kick(&mut self) {
        if self.layers.busy || !self.layers.dirty {
            return;
        }
        let Some(doc) = self.layers.doc.clone() else { return };
        self.layers.dirty = false;
        self.layers.busy = true;
        let eff = self.layers.set.eff(true);
        let (tx, rx) = mpsc::channel();
        let wake = self.wake.clone();
        let gen = self.layers.gen;
        self.layers.img_rx = Some(rx);
        std::thread::spawn(move || {
            let mut img = layerdoc::flatten(&doc, &eff);
            img.apply_display_colour();
            let _ = tx.send((gen, img));
            wake();
        });
    }

    // ---- mouse and keys --------------------------------------------------------------------------------------------------------

    /// Is (x, y) on the panel?
    pub fn layers_blocks(&self, x: i32, y: i32) -> bool {
        self.layers_visible() && inside(&self.layers_layout().panel, x as f32, y as f32)
    }

    fn layers_hit(&mut self, x: i32, y: i32) -> Option<Hit> {
        let lay = self.layers_layout();
        let (fx, fy) = (x as f32, y as f32);
        if !inside(&lay.panel, fx, fy) {
            return None;
        }
        if inside(&lay.close, fx, fy) {
            return Some(Hit::Close);
        }
        if !self.layers.set.filter.is_empty() && inside(&lay.clear, fx, fy) {
            return Some(Hit::ClearFilter);
        }
        if inside(&lay.filter, fx, fy) {
            return Some(Hit::Filter);
        }
        for (r, hit, _) in &lay.extras {
            if inside(r, fx, fy) {
                return Some(*hit);
            }
        }
        for (i, b) in lay.buttons.iter().enumerate() {
            if inside(b, fx, fy) {
                return Some([Hit::AllOn, Hit::AllOff, Hit::Reset, Hit::Save][i]);
            }
        }
        if inside(&lay.list, fx, fy) && self.layers.state == State::Ready {
            let rows = self.layers_rows(&lay);
            let slot = ((fy - lay.list[1]) / lay.row_h) as usize;
            if let Some(r) = rows.get(self.layers.scroll + slot) {
                let g = self.layers_row_geo(&lay, slot, r.depth);
                let i = r.index;
                if inside(&g.slider, fx - 6.0 * lay.d, fy) || inside(&g.slider, fx + 6.0 * lay.d, fy) {
                    return Some(Hit::Slider(i));
                }
                if self.layers.set.nodes[i].group && inside(&g.fold, fx, fy) {
                    return Some(Hit::Fold(i));
                }
                if inside(&g.eye, fx, fy) {
                    return Some(Hit::Eye(i));
                }
                return Some(Hit::Name(i));
            }
        }
        None
    }

    /// The row a hit belongs to (what gets highlighted).
    fn layers_row_of(h: Hit) -> Option<usize> {
        match h {
            Hit::Eye(i) | Hit::Fold(i) | Hit::Name(i) | Hit::Slider(i) => Some(i),
            _ => None,
        }
    }

    /// Pointer moved over the window.
    pub fn layers_hover(&mut self, x: i32, y: i32, left_down: bool) {
        if !self.layers_visible() {
            return;
        }
        let hit = if self.layers.drag.is_some() { self.layers.hot } else { self.layers_hit(x, y) };
        let hover = hit.and_then(Self::layers_row_of);
        let changed = hit != self.layers.hot || hover != self.layers.set.hover;
        self.layers.hot = hit;
        if hover != self.layers.set.hover {
            self.layers.set.hover = hover;
            if self.layers.state == State::Ready {
                self.layers_changed();
            }
        }
        let _ = left_down;
        if changed {
            invalidate(self.hwnd);
        }
    }

    fn layers_set_slider(&mut self, i: usize, x: i32) {
        let lay = self.layers_layout();
        let rows = self.layers.set.rows();
        let Some(slot) = rows.iter().skip(self.layers.scroll).position(|r| r.index == i) else { return };
        let g = self.layers_row_geo(&lay, slot, rows[self.layers.scroll + slot].depth);
        let v = ((x as f32 - g.slider[0]) / (g.slider[2] - g.slider[0])).clamp(0.0, 1.0);
        let v = if v > 0.985 { 1.0 } else if v < 0.015 { 0.0 } else { v };
        self.layers.set.set_opacity(i, v);
        self.layers_changed();
    }

    /// Left press. True when the panel took it.
    pub fn layers_press(&mut self, x: i32, y: i32, ctrl: bool) -> bool {
        if !self.layers_visible() {
            return false;
        }
        let Some(hit) = self.layers_hit(x, y) else {
            let inside_panel = self.layers_blocks(x, y);
            if inside_panel {
                self.layers.filter_focus = false;
            }
            return inside_panel;
        };
        self.layers.filter_focus = hit == Hit::Filter;
        match hit {
            Hit::Close => self.layers_close(),
            Hit::Filter => {}
            Hit::ClearFilter => {
                self.layers.set.filter.clear();
                self.layers.scroll = 0;
            }
            Hit::AllOn => {
                self.layers.set.set_all(true);
                self.layers_changed();
            }
            Hit::AllOff => {
                self.layers.set.set_all(false);
                self.layers_changed();
            }
            Hit::Reset => {
                self.layers.set.reset_all();
                self.layers_changed();
            }
            Hit::Save => {
                self.layers_save();
            }
            Hit::AnimPrev => self.anim_step(-1),
            Hit::AnimNext => self.anim_step(1),
            Hit::SkinNext => self.anim_skin(),
            Hit::Play => self.anim_play(),
            Hit::Speed => self.cycle_speed(),
            Hit::Loop => self.toggle_loop(),
            Hit::PingPong => self.toggle_pingpong(),
            Hit::Frames => self.anim_frames(),
            Hit::Webm => self.anim_webm(),
            Hit::Fold(i) => self.layers.set.toggle_fold(i),
            Hit::Eye(i) => {
                self.layers.set.toggle(i);
                self.layers_changed();
            }
            Hit::Name(i) => {
                if ctrl {
                    self.layers.set.toggle_solo(i);
                } else {
                    self.layers.set.toggle(i);
                }
                self.layers_changed();
            }
            Hit::Slider(i) => {
                self.layers.drag = Some(i);
                self.layers.hot = Some(hit);
                self.layers_set_slider(i, x);
            }
        }
        invalidate(self.hwnd);
        true
    }

    /// Second quick press on a slider: back to what the file says. Third: nothing.
    pub fn layers_double(&mut self, x: i32, y: i32) -> bool {
        if !self.layers_visible() {
            return false;
        }
        if let Some(Hit::Slider(i)) = self.layers_hit(x, y) {
            self.layers.drag = None;
            self.layers.set.reset_one(i);
            self.layers_changed();
            return true;
        }
        self.layers_press(x, y, false)
    }

    pub fn layers_triple(&mut self, x: i32, y: i32) -> bool {
        if !self.layers_visible() {
            return false;
        }
        if let Some(Hit::Slider(i)) = self.layers_hit(x, y) {
            self.layers.drag = None;
            self.layers.set.clear_one(i);
            self.layers_changed();
            return true;
        }
        self.layers_blocks(x, y)
    }

    /// Mouse moved with a button down. True when a slider drag took it.
    pub fn layers_drag(&mut self, x: i32, left_down: bool) -> bool {
        let Some(i) = self.layers.drag else { return false };
        if !left_down {
            self.layers.drag = None;
            return true;
        }
        self.layers_set_slider(i, x);
        true
    }

    pub fn layers_release(&mut self) -> bool {
        self.layers.drag.take().is_some()
    }

    pub fn layers_right_click(&mut self, x: i32, y: i32) -> bool {
        if !self.layers_visible() {
            return false;
        }
        if let Some(h) = self.layers_hit(x, y) {
            if let Some(i) = Self::layers_row_of(h) {
                self.layers.set.toggle_solo(i);
                self.layers_changed();
            }
        }
        self.layers_blocks(x, y)
    }

    pub fn layers_wheel(&mut self, delta: i32) {
        let notches = (delta as f32 / 120.0 * 3.0).round() as isize;
        let lay = self.layers_layout();
        let rows = self.layers.set.rows().len();
        let max = rows.saturating_sub(self.layers_slots(&lay));
        self.layers.scroll = (self.layers.scroll as isize - notches).clamp(0, max as isize) as usize;
        invalidate(self.hwnd);
    }

    pub fn layers_char(&mut self, c: char) -> bool {
        if !self.layers.filter_focus || !self.layers_visible() {
            return false;
        }
        if !c.is_control() && self.layers.set.filter.chars().count() < 60 {
            self.layers.set.filter.push(c);
            self.layers.scroll = 0;
            invalidate(self.hwnd);
        }
        true
    }

    /// A key while the filter box has the keyboard. True when it was used.
    pub fn layers_key(&mut self, k: u32) -> bool {
        if !self.layers.filter_focus || !self.layers_visible() {
            return false;
        }
        match k {
            VK_BACK => {
                self.layers.set.filter.pop();
                self.layers.scroll = 0;
            }
            VK_ESCAPE => {
                if self.layers.set.filter.is_empty() {
                    self.layers.filter_focus = false;
                } else {
                    self.layers.set.filter.clear();
                    self.layers.scroll = 0;
                }
            }
            VK_RETURN => self.layers.filter_focus = false,
            _ => return false,
        }
        invalidate(self.hwnd);
        true
    }

    // ---- saving ------------------------------------------------------------------------------------------------------------------

    /// The picture as the panel has it now (no highlight), saved the way the video snapshot is. A format without transparency gets the
    /// viewer's canvas colour behind the picture.
    pub fn layers_save(&mut self) -> bool {
        if self.layers.rig_mode {
            return self.anim_snapshot();
        }
        if self.layers.state != State::Ready {
            return false;
        }
        let (Some(doc), Some(path)) = (self.layers.doc.clone(), self.layers.path.clone()) else { return false };
        let eff = self.layers.set.eff(false);
        let mut img = layerdoc::flatten(&doc, &eff);
        img.apply_display_colour();
        let prefs = self.settings.more.clone();
        if !nv_core::frames::format_keeps_alpha(&prefs.snapshot_format) {
            let c = canvas_rgb();
            let bg = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            for p in img.rgba.chunks_exact_mut(4) {
                let a = p[3] as u32;
                for k in 0..3 {
                    p[k] = ((p[k] as u32 * a + bg[k] as u32 * (255 - a) + 127) / 255) as u8;
                }
                p[3] = 255;
            }
        }
        let shown = nv_core::snapshot::Shown { width: img.width, height: img.height, rgba: img.rgba };
        match nv_core::snapshot::take_shown_labeled(&path, "layers", &prefs, shown) {
            Ok(s) => {
                let name = s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.toast(if s.note.is_empty() { format!("Saved {name}") } else { format!("Saved {name} - {}", s.note) });
            }
            Err(e) => self.toast(format!("Could not save: {e}")),
        }
        true
    }

    // ---- drawing -----------------------------------------------------------------------------------------------------------------

    fn ltext(&mut self, s: &str, x: f32, y: f32, c: [f32; 4]) {
        if s.is_empty() {
            return;
        }
        if let Some((key, _, _)) = self.overlay_small(&[s.to_string()]) {
            let _ = self.renderer.draw_overlay(key, x, y, c);
        }
    }

    /// The outline of the highlighted layer on the picture. `dst` is where the picture is drawn.
    pub fn layers_draw_outline(&mut self, dst: [f32; 4]) {
        if !self.layers_visible() || self.layers.state != State::Ready {
            return;
        }
        let (Some(h), Some(doc)) = (self.layers.set.hover, self.layers.doc.clone()) else { return };
        let (sx, sy) = ((dst[2] - dst[0]) / doc.width.max(1) as f32, (dst[3] - dst[1]) / doc.height.max(1) as f32);
        let d = self.gallery.dpi;
        let t = (2.0 * d).max(1.0);
        let col = srgb(0x35D0FF, 0.95);
        for i in self.layers.set.subtree(h) {
            let Some(b) = self.layers.set.nodes[i].bounds else { continue };
            let r = [dst[0] + b[0] as f32 * sx, dst[1] + b[1] as f32 * sy, dst[0] + b[2] as f32 * sx, dst[1] + b[3] as f32 * sy];
            if r[2] - r[0] < 1.0 || r[3] - r[1] < 1.0 {
                continue;
            }
            for e in [[r[0], r[1], r[2], r[1] + t], [r[0], r[3] - t, r[2], r[3]], [r[0], r[1], r[0] + t, r[3]], [r[2] - t, r[1], r[2], r[3]]] {
                let _ = self.renderer.fill_rect(e, col);
            }
        }
    }

    pub fn draw_layers(&mut self) {
        if !self.layers_visible() {
            return;
        }
        let lay = self.layers_layout();
        let d = lay.d;
        let (text, dim, accent) = (srgb(0xF2F2F2, 1.0), srgb(0x8A9099, 1.0), srgb(0x3D8BFF, 0.95));
        let _ = self.renderer.fill_rect(lay.panel, srgb(0x0E0E0E, 0.90));
        // Title and close.
        let ready = self.layers.state == State::Ready;
        let title = match (&self.layers.state, &self.layers.doc) {
            (State::Ready, None) if self.layers.rig_mode => self.anim_title(),
            (State::Ready, Some(doc)) => format!("Layers - {} - {} of {} shown", doc.format, self.layers.set.shown_count(), self.layers.set.nodes.iter().filter(|n| !n.group).count()),
            (State::Loading, _) => "Layers - reading...".to_string(),
            _ => "Layers".to_string(),
        };
        self.ltext(&title, lay.panel[0] + 10.0 * d, lay.panel[1] + 9.0 * d, text);
        if self.layers.hot == Some(Hit::Close) {
            let _ = self.renderer.fill_rect(lay.close, srgb(0x3A3F4A, 0.95));
        }
        if let Some((key, tw, th)) = self.overlay_glyph("\u{2715}", 14.0) {
            let _ = self.renderer.draw_overlay(key, lay.close[0] + (lay.close[2] - lay.close[0] - tw) / 2.0, lay.close[1] + (lay.close[3] - lay.close[1] - th) / 2.0, text);
        }
        // Filter box.
        let _ = self.renderer.fill_rect(lay.filter, srgb(if self.layers.filter_focus { 0x23303F } else { 0x1B1D22 }, 0.95));
        let ftxt = if self.layers.set.filter.is_empty() && !self.layers.filter_focus { "Filter by name".to_string() } else { format!("{}{}", self.layers.set.filter, if self.layers.filter_focus { "|" } else { "" }) };
        self.ltext(&ftxt, lay.filter[0] + 4.0 * d, lay.filter[1] + 3.0 * d, if self.layers.set.filter.is_empty() && !self.layers.filter_focus { dim } else { text });
        if !self.layers.set.filter.is_empty() {
            if let Some((key, tw, th)) = self.overlay_glyph("\u{2715}", 12.0) {
                let _ = self.renderer.draw_overlay(key, lay.clear[0] + (lay.clear[2] - lay.clear[0] - tw) / 2.0, lay.clear[1] + (lay.clear[3] - lay.clear[1] - th) / 2.0, dim);
            }
        }
        // Buttons.
        let labels = [("All on", Hit::AllOn), ("All off", Hit::AllOff), ("Reset", Hit::Reset), ("Save", Hit::Save)];
        for (b, (label, hit)) in lay.buttons.iter().zip(labels) {
            let hot = self.layers.hot == Some(hit) && ready;
            let _ = self.renderer.fill_rect(*b, srgb(if hot { 0x3A3F4A } else { 0x1B1D22 }, 0.95));
            if let Some((key, tw, th)) = self.overlay_small(&[label.to_string()]) {
                let c = if ready { text } else { dim };
                let _ = self.renderer.draw_overlay(key, b[0] + (b[2] - b[0] - tw) / 2.0, b[1] + (b[3] - b[1] - th) / 2.0, c);
            }
        }
        if self.layers.rig_mode {
            let extras = lay.extras.clone();
            for (b, hit, label) in extras {
                let on = match hit {
                    Hit::Play => self.anim_playing(),
                    Hit::Loop => self.settings.more.play_loop,
                    Hit::PingPong => self.settings.more.play_pingpong,
                    _ => false,
                };
                let hot = self.layers.hot == Some(hit);
                let _ = self.renderer.fill_rect(b, srgb(if on { 0x2F5FA8 } else if hot { 0x3A3F4A } else { 0x1B1D22 }, 0.95));
                if let Some((key, tw, th)) = self.overlay_small(&[label.to_string()]) {
                    let _ = self.renderer.draw_overlay(key, b[0] + (b[2] - b[0] - tw) / 2.0, b[1] + (b[3] - b[1] - th) / 2.0, text);
                }
            }
            let info = self.anim_info();
            self.renderer.set_clip(Some(lay.info));
            self.ltext(&info, lay.info[0], lay.info[1], dim);
            self.renderer.set_clip(None);
        }
        // The list.
        match self.layers.state.clone() {
            State::Ready => {
                let rows = self.layers_rows(&lay);
                let slots = self.layers_slots(&lay);
                self.renderer.set_clip(Some(lay.list));
                for slot in 0..slots {
                    let Some(r) = rows.get(self.layers.scroll + slot).copied() else { break };
                    let g = self.layers_row_geo(&lay, slot, r.depth);
                    let node = self.layers.set.nodes[r.index].clone();
                    let hot_row = self.layers.set.hover == Some(r.index);
                    if hot_row {
                        let _ = self.renderer.fill_rect(g.row, srgb(0x2A3A52, 0.9));
                    }
                    if self.layers.set.solo == Some(r.index) {
                        let _ = self.renderer.fill_rect([g.row[0], g.row[1], g.row[0] + 3.0 * d, g.row[3]], accent);
                    }
                    if node.group {
                        let glyph = if self.layers.set.collapsed.get(r.index).copied().unwrap_or(false) { "\u{25B8}" } else { "\u{25BE}" };
                        if let Some((key, tw, th)) = self.overlay_glyph(glyph, 13.0) {
                            let _ = self.renderer.draw_overlay(key, g.fold[0] + (g.fold[2] - g.fold[0] - tw) / 2.0, g.fold[1] + (g.fold[3] - g.fold[1] - th) / 2.0, dim);
                        }
                    }
                    let eye = if node.on { "\u{25CF}" } else { "\u{25CB}" };
                    if let Some((key, tw, th)) = self.overlay_glyph(eye, 14.0) {
                        let _ = self.renderer.draw_overlay(key, g.eye[0] + (g.eye[2] - g.eye[0] - tw) / 2.0, g.eye[1] + (g.eye[3] - g.eye[1] - th) / 2.0, if node.on { accent } else { dim });
                    }
                    // The name, cut at the slider.
                    self.renderer.set_clip(Some([g.name[0], g.name[1], g.name[2], g.name[3]]));
                    let label = if node.group { format!("[ {} ]", node.name) } else { node.name.clone() };
                    self.ltext(&label, g.name[0], g.name[1] + (lay.row_h - 21.0 * d) / 2.0, if node.on { text } else { dim });
                    self.renderer.set_clip(Some(lay.list));
                    // Opacity slider.
                    let (s, mid) = (g.slider, (g.slider[1] + g.slider[3]) / 2.0);
                    let _ = self.renderer.fill_rect([s[0], mid - 2.0 * d, s[2], mid + 2.0 * d], srgb(0x2B2E35, 1.0));
                    let fill = s[0] + (s[2] - s[0]) * node.opacity;
                    let _ = self.renderer.fill_rect([s[0], mid - 2.0 * d, fill, mid + 2.0 * d], if node.on { accent } else { srgb(0x4A4F5A, 1.0) });
                    let _ = self.renderer.fill_rect([fill - 3.0 * d, s[1] + 2.0 * d, fill + 3.0 * d, s[3] - 2.0 * d], srgb(0xF2F2F2, 1.0));
                    if hot_row || self.layers.drag == Some(r.index) {
                        let pct = format!("{:.0}%", node.opacity * 100.0);
                        self.renderer.set_clip(Some([g.slider[0] - 40.0 * d, g.row[1], g.slider[0] - 2.0 * d, g.row[3]]));
                        self.ltext(&pct, g.slider[0] - 38.0 * d, g.name[1] + (lay.row_h - 21.0 * d) / 2.0, dim);
                        self.renderer.set_clip(Some(lay.list));
                    }
                }
                self.renderer.set_clip(None);
                if rows.is_empty() {
                    self.ltext("No layer has that name", lay.list[0] + 4.0 * d, lay.list[1] + 6.0 * d, dim);
                }
                // Scroll bar.
                if rows.len() > slots {
                    let track = [lay.list[2] - 3.0 * d, lay.list[1], lay.list[2], lay.list[3]];
                    let (th, top) = ((track[3] - track[1]) * slots as f32 / rows.len() as f32, (track[3] - track[1]) * self.layers.scroll as f32 / rows.len() as f32);
                    let _ = self.renderer.fill_rect([track[0], track[1] + top, track[2], track[1] + top + th.max(12.0 * d)], srgb(0x5A606C, 0.9));
                }
            }
            State::Loading => self.ltext("Reading the layers...", lay.list[0] + 4.0 * d, lay.list[1] + 6.0 * d, dim),
            State::Failed(e) => {
                for (k, line) in wrap(&e, 44).into_iter().take(4).enumerate() {
                    self.ltext(&line, lay.list[0] + 4.0 * d, lay.list[1] + (6.0 + k as f32 * 20.0) * d, srgb(0xFFB454, 1.0));
                }
            }
            State::Idle => {}
        }
        // Footer: what this reader does not do, and the mouse.
        let mut lines: Vec<String> = Vec::new();
        if let Some(doc) = &self.layers.doc {
            for chunk in wrap(&doc.note, 46).into_iter().take(2) {
                lines.push(chunk);
            }
        }
        if self.layers.rig_mode {
            let note = self.anim_note();
            for chunk in wrap(&note, 46).into_iter().take(2) {
                lines.push(chunk);
            }
        }
        lines.push("Click: on / off   Ctrl+click or right click: only this".into());
        lines.push("Slider: double click = as saved, triple = 0".into());
        for (k, l) in lines.iter().enumerate() {
            self.ltext(l, lay.footer[0], lay.footer[1] + k as f32 * 15.0 * d, dim);
        }
        if self.layers.test {
            let rows = self.layers.set.rows().len();
            let title = format!("LAYERS panel={:.0},{:.0},{:.0},{:.0} list={:.0},{:.0},{:.0},{:.0} row_h={:.1} rows={rows} shown={} state={:?} hover={:?} applied={}", lay.panel[0], lay.panel[1], lay.panel[2], lay.panel[3], lay.list[0], lay.list[1], lay.list[2], lay.list[3], lay.row_h, self.layers.set.shown_count(), self.layers.state, self.layers.set.hover, self.layers.applied);
            set_title(self.hwnd, &title);
        }
    }
}

/// Words onto lines of at most `n` characters.
fn wrap(s: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > n {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wrap_breaks_between_words() {
        let l = wrap("one two three four five", 9);
        assert_eq!(l, vec!["one two", "three", "four five"]);
        assert!(wrap("", 10).is_empty());
    }
}
