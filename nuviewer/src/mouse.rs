//! Mouse behaviour of the viewer: wheel = previous/next, hold left button = temporary zoom + pan,
//! Ctrl + drag = area zoom, right click = context menu, pointer at the bottom edge = Windows taskbar (fullscreen).

use super::*;
use nv_core::keys::{Action, ACTIONS};
use nv_win::menu::{self, Entry};

/// Menu ids: `ACTION_BASE + index in ACTIONS` runs that action, `EXTERNAL_BASE + n` sends the picture to the n-th
/// external program, the rest are special items.
const ACTION_BASE: u32 = 0x2000_0000;
const EXTERNAL_BASE: u32 = 0x3000_0000;
const MENU_FILE_TYPES: u32 = 0x1000_0002;
/// "Sort by" submenu: `MENU_SORT + index in SortKey::ALL`, then ascending / descending.
const MENU_SORT: u32 = 0x1000_0010;
const MENU_SORT_ASC: u32 = 0x1000_0020;
const MENU_SORT_DESC: u32 = 0x1000_0021;

impl App {
    // ---- wheel ------------------------------------------------------------------------------------

    /// One wheel notch (120) = one picture; wheel up goes back. High-resolution wheels accumulate until a notch is complete.
    pub fn wheel_step(&mut self, delta: i32) {
        if self.wheel_acc != 0 && (self.wheel_acc > 0) != (delta > 0) {
            self.wheel_acc = 0; // changed direction
        }
        self.wheel_acc += delta;
        let steps = self.wheel_acc / 120;
        if steps != 0 {
            self.wheel_acc -= steps * 120;
            for _ in 0..steps.abs() {
                self.step(if steps > 0 { -1 } else { 1 });
            }
        }
    }

    /// One step forward (+1) or back (-1): a page of the current document if there is one, else a file.
    /// Past the last page (or before the first) it carries on into the neighbouring file.
    pub fn step(&mut self, d: isize) {
        // Read mode: a step is a whole spread (one or two pages).
        if self.read.on && self.mode == Mode::Viewer && self.doc_pages().is_none() && !self.is_model() {
            self.read_step(d);
            return;
        }
        if let Some((p, n)) = self.doc_pages() {
            let q = p as isize + d;
            if (1..=n as isize).contains(&q) {
                self.set_page(q as u32);
                return;
            }
        }
        let cur = self.folder.index as isize;
        self.navigate(cur + d);
    }

    /// Show page `page` (1-based) of the current document.
    pub fn set_page(&mut self, page: u32) {
        let Some((p, n)) = self.doc_pages() else { return };
        let page = page.clamp(1, n);
        if page == p {
            return;
        }
        self.dir = if page > p { 1 } else { -1 };
        self.doc_page = page;
        self.nav_start = Some(Instant::now());
        self.view = self.view.fresh();
        self.refresh();
    }

    // ---- left button: hold to zoom, Ctrl + drag to zoom into an area -------------------------------

    pub fn viewer_press(&mut self, x: i32, y: i32, ctrl: bool) {
        if self.rename.is_some() || self.view.held.is_some() || self.view.area.is_some() {
            return;
        }
        if self.is_model() {
            self.orbit_last = Some((x, y)); // dragging turns the model; with Shift held it slides it
            self.orbit_pan = nv_win::modifiers().shift;
            return;
        }
        let Some(img) = self.view_size() else { return };
        let win = self.renderer.size();
        if ctrl {
            self.view.area = Some(((x, y), (x, y)));
        } else if !self.view.fit {
            // Already zoomed (area zoom, Ctrl+wheel, the 1-5 keys): a plain click cancels it, dragging pans it.
            self.view.click_cancel = Some((x, y));
            self.view.drag_last = Some((x, y));
        } else {
            let Some(target) = self.settings.hold_scale(img, nv_win::monitor_size(self.hwnd)) else { return };
            self.view.held = Some((self.view.fit, self.view.zoom, self.view.pan));
            self.view.zoom_to(img, win, (x, y), target);
            self.view.drag_last = Some((x, y));
        }
        invalidate(self.hwnd);
    }

    pub fn viewer_drag(&mut self, x: i32, y: i32, left_down: bool) {
        if !left_down {
            return;
        }
        if self.seek_drag {
            self.seek_to(x);
            return;
        }
        if let Some((lx, ly)) = self.orbit_last {
            if self.orbit_pan {
                let (w, h) = self.renderer.size();
                self.cam.pan_by((x - lx) as f32, (y - ly) as f32, w as f32, h as f32);
            } else {
                self.cam.rotate(-(x - lx) as f32 * 0.008, (y - ly) as f32 * 0.008);
            }
            self.orbit_last = Some((x, y));
            invalidate(self.hwnd);
            return;
        }
        if let Some((a, _)) = self.view.area {
            self.view.area = Some((a, (x, y)));
            invalidate(self.hwnd);
        } else if let Some((lx, ly)) = self.view.drag_last {
            if let Some((sx, sy)) = self.view.click_cancel {
                if (x - sx).abs() > 4 || (y - sy).abs() > 4 {
                    self.view.click_cancel = None; // it is a drag, not a click
                }
            }
            let Some(img) = self.view_size() else { return };
            self.view.pan.0 += (x - lx) as f32;
            self.view.pan.1 += (y - ly) as f32;
            self.view.clamp_pan(img, self.renderer.size()); // the picture's edge stops at the window edge
            self.view.drag_last = Some((x, y));
            invalidate(self.hwnd);
        }
    }

    /// Left button released (also sent when the mouse capture is lost): undo the hold-zoom, apply the area zoom,
    /// or, for a click on an already zoomed picture, go back to the fitted view.
    pub fn viewer_release(&mut self) {
        self.view.drag_last = None;
        self.orbit_last = None;
        self.seek_drag = false;
        if let Some((fit, zoom, pan)) = self.view.held.take() {
            self.view.fit = fit;
            self.view.zoom = zoom;
            self.view.pan = pan;
            invalidate(self.hwnd);
        } else if let Some((a, b)) = self.view.area.take() {
            if let Some(img) = self.view_size() {
                let win = self.renderer.size();
                self.view.zoom_to_rect(img, win, [a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]);
            }
            invalidate(self.hwnd);
        } else if self.view.click_cancel.take().is_some() {
            self.view.reset_to_base(); // back to the fitted picture, or to 100% when that is the base mode
            invalidate(self.hwnd);
        }
    }

    /// Rubber band for the area zoom: translucent fill and a thin outline.
    pub fn draw_area_box(&mut self, a: (i32, i32), b: (i32, i32)) {
        let (l, t, r, bt) = (a.0.min(b.0) as f32, a.1.min(b.1) as f32, a.0.max(b.0) as f32, a.1.max(b.1) as f32);
        let w = (1.5 * self.gallery.dpi).max(1.0);
        let _ = self.renderer.fill_rect([l, t, r, bt], srgb(0x3D8BFF, 0.16));
        let edge = srgb(0x7DB4FF, 0.95);
        let _ = self.renderer.fill_rect([l, t, r, t + w], edge);
        let _ = self.renderer.fill_rect([l, bt - w, r, bt], edge);
        let _ = self.renderer.fill_rect([l, t, l + w, bt], edge);
        let _ = self.renderer.fill_rect([r - w, t, r, bt], edge);
    }

    // ---- fullscreen: pointer at the bottom edge shows the taskbar ----------------------------------

    /// Start or stop the fullscreen-only timers (pointer auto-hide, bottom-edge check).
    pub fn fullscreen_timers(&mut self, on: bool) {
        if on {
            set_timer(self.hwnd, TIMER_CURSOR, CURSOR_IDLE_MS);
            set_timer(self.hwnd, TIMER_EDGE, EDGE_POLL_MS);
        } else {
            kill_timer(self.hwnd, TIMER_CURSOR);
            kill_timer(self.hwnd, TIMER_EDGE);
            nv_win::set_cursor_hidden(false);
            self.taskbar_shown = false;
            self.edge_ticks = 0;
        }
    }

    /// Polled while fullscreen (the taskbar is a different window, so once the pointer is on it we get no mouse events).
    pub fn edge_tick(&mut self) {
        if !nv_win::is_fullscreen() {
            kill_timer(self.hwnd, TIMER_EDGE);
            return;
        }
        if !self.settings.taskbar_edge {
            return;
        }
        let (cx, cy) = nv_win::cursor_pos();
        let (l, _, r, b) = nv_win::monitor_rect(self.hwnd);
        let in_x = cx >= l && cx < r;
        if !self.taskbar_shown {
            if !nv_win::is_foreground(self.hwnd) {
                return;
            }
            // A short dwell on the very bottom row, so brushing past the edge doesn't pop it up.
            if in_x && cy >= b - 1 {
                self.edge_ticks = self.edge_ticks.saturating_add(1);
                if self.edge_ticks >= 2 {
                    nv_win::set_taskbar_hidden(self.hwnd, false);
                    nv_win::set_cursor_hidden(false);
                    self.taskbar_shown = true;
                }
            } else {
                self.edge_ticks = 0;
            }
        } else if !in_x || cy < b - nv_win::taskbar_height() - 6 {
            nv_win::set_taskbar_hidden(self.hwnd, true);
            self.taskbar_shown = false;
            self.edge_ticks = 0;
        }
    }

    // ---- right click: context menu -------------------------------------------------------------------

    /// A menu line for `action`, with its current primary key as the shortcut hint.
    fn action_entry(&self, action: Action, label: &str) -> Entry {
        let idx = ACTIONS.iter().position(|i| i.action == action).unwrap_or(0);
        let hint = self.keymap.chords_of(action).first().map(|c| c.format()).unwrap_or_default();
        Entry::item(ACTION_BASE + idx as u32, if hint.is_empty() { label.to_string() } else { format!("{label}\t{hint}") })
    }

    pub fn context_menu(&mut self, x: i32, y: i32) {
        if self.rename.is_some() {
            return;
        }
        self.viewer_release(); // never leave a half-finished hold or rubber band behind the menu
        if self.mode == Mode::Gallery {
            let lay = self.layout();
            if let Some(i) = lay.hit(x as f32, y as f32, lay.clamp_scroll(self.gallery.scroll)) {
                self.gallery_select(i);
            }
        }
        nv_win::set_cursor_hidden(false);
        let has_file = self.target().is_some();
        let viewer = self.mode == Mode::Viewer;
        let many = self.folder.files.len() > 1;
        let playable = self.controller().is_some();
        let checked = |e: Entry, on: bool| match e {
            Entry::Item { id, text, enabled, .. } => Entry::Item { id, text, checked: on, enabled },
            other => other,
        };
        let externals: Vec<Entry> = self.settings.external.iter().enumerate().map(|(i, e)| Entry::item(EXTERNAL_BASE + i as u32, e.name.clone())).collect();
        let open_with = if externals.is_empty() {
            Entry::Submenu { text: "Open with".into(), items: vec![Entry::item(0x1000_0003, "(none set up: Settings > External programs)").enabled(false)], enabled: has_file }
        } else {
            Entry::Submenu { text: "Open with".into(), items: externals, enabled: has_file }
        };
        let order = self.folder.order;
        let sort_menu = {
            let mut items: Vec<Entry> = nv_core::SortKey::ALL.iter().enumerate().map(|(i, k)| Entry::checked(MENU_SORT + i as u32, k.label(), order.shuffle.is_none() && order.key == *k)).collect();
            items.push(Entry::Separator);
            items.push(Entry::checked(MENU_SORT_ASC, "Ascending", !order.descending));
            items.push(Entry::checked(MENU_SORT_DESC, "Descending", order.descending));
            Entry::Submenu { text: "Sort by".into(), items, enabled: has_file && many }
        };
        let entries = vec![
            self.action_entry(Action::Prev, "Previous picture").enabled(viewer && many),
            self.action_entry(Action::Next, "Next picture").enabled(viewer && many),
            Entry::Separator,
            self.action_entry(Action::FitReset, "Fit to screen").enabled(viewer),
            self.action_entry(Action::ActualSize, "Actual size (100%)").enabled(viewer),
            checked(self.action_entry(Action::Gallery, "Gallery").enabled(has_file), !viewer),
            checked(self.action_entry(Action::Fullscreen, "Fullscreen"), nv_win::is_fullscreen()),
            checked(self.action_entry(Action::Slideshow, "Slideshow").enabled(many), self.slide.on),
            self.action_entry(Action::PlayPause, "Play / pause").enabled(playable),
            checked(self.action_entry(Action::ToggleSequences, "Fold image sequences"), self.settings.sequences),
            checked(self.action_entry(Action::FilmStrip, "Film strip").enabled(viewer && many), self.strip.pinned),
            checked(self.action_entry(Action::ReadMode, "Read mode (two pages)").enabled(viewer && many), self.read.on),
            sort_menu,
            checked(self.action_entry(Action::Shuffle, "Shuffle").enabled(many), order.shuffle.is_some()),
            checked(self.action_entry(Action::Info, "Info / EXIF").enabled(has_file), self.hud.show_info),
            Entry::Separator,
            self.action_entry(Action::Rename, "Rename…").enabled(has_file),
            self.action_entry(Action::CopyTo, "Copy to…").enabled(has_file),
            self.action_entry(Action::MoveTo, "Move to…").enabled(has_file),
            self.action_entry(Action::CopyImage, "Copy picture").enabled(has_file),
            self.action_entry(Action::CopyPath, "Copy file path").enabled(has_file),
            self.action_entry(Action::Reveal, "Show in Explorer").enabled(has_file),
            self.action_entry(Action::Delete, "Delete (Recycle Bin)").enabled(has_file),
            self.action_entry(Action::Undo, "Undo").enabled(!self.undo.is_empty()),
            Entry::Separator,
            open_with,
            Entry::Separator,
            self.action_entry(Action::Settings, "Settings…"),
            Entry::item(MENU_FILE_TYPES, "File types…"),
            checked(self.action_entry(Action::Help, "Help"), self.hud.show_help),
            self.action_entry(Action::Quit, "Close"),
        ];
        let Some(id) = menu::popup(self.hwnd, x, y, &entries) else {
            if nv_win::is_fullscreen() {
                set_timer(self.hwnd, TIMER_CURSOR, CURSOR_IDLE_MS);
            }
            return;
        };
        match id {
            // In this process, so the chooser is owned by the viewer window like every other dialog.
            MENU_FILE_TYPES => associate_dialog(),
            MENU_SORT_ASC | MENU_SORT_DESC => {
                let o = self.folder.order;
                self.apply_order(nv_core::Order { key: o.key, descending: id == MENU_SORT_DESC, shuffle: None });
                self.settings.sort_desc = id == MENU_SORT_DESC;
                let _ = self.settings.save();
                self.toast(format!("Sorted by {}", self.order_text()));
            }
            i if (MENU_SORT..MENU_SORT + nv_core::SortKey::ALL.len() as u32).contains(&i) => {
                self.set_sort_key(nv_core::SortKey::ALL[(i - MENU_SORT) as usize]);
            }
            i if i >= EXTERNAL_BASE => {
                let program = self.settings.external.get((i - EXTERNAL_BASE) as usize).cloned();
                if let (Some(p), Some(t)) = (program, self.target()) {
                    if let Err(e) = nv_win::shell::run_external(&p.command, &self.folder.files[t]) {
                        self.toast(e);
                    }
                }
            }
            i if i >= ACTION_BASE => {
                if let Some(info) = ACTIONS.get((i - ACTION_BASE) as usize) {
                    self.perform(info.action);
                }
            }
            _ => {}
        }
    }
}
