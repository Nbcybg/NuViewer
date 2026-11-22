//! Mouse behaviour of the viewer: wheel = previous/next, hold left button = temporary zoom + pan,
//! Ctrl + drag = area zoom, right click = context menu, pointer at the bottom edge = Windows taskbar (fullscreen).

use super::*;
use nv_core::keys::{Action, ACTIONS};
use nv_core::xmp::Label;
use nv_win::menu::{self, Entry};

/// Menu ids: `ACTION_BASE + index in ACTIONS` runs that action, `EXTERNAL_BASE + n` sends the picture to the n-th
/// external program, the rest are special items.
const ACTION_BASE: u32 = 0x2000_0000;
const EXTERNAL_BASE: u32 = 0x3000_0000;
const MENU_FILE_TYPES: u32 = 0x1000_0002;
const MENU_CHOOSE_PROGRAM: u32 = 0x1000_0004;
const MENU_SET_UP_PROGRAMS: u32 = 0x1000_0005;
/// "Open with": Windows' own program picker, Explorer's Properties window, and `OPEN_WITH_BASE + n` for the n-th program listed.
const MENU_OPEN_AS: u32 = 0x1000_0006;
const MENU_PROPERTIES: u32 = 0x1000_0007;
const OPEN_WITH_BASE: u32 = 0x4000_0000;
/// "Sort by" submenu: `MENU_SORT + index in SortKey::ALL`, then ascending / descending.
const MENU_SORT: u32 = 0x1000_0010;
/// Plug-ins submenu: `MENU_PLUGIN + plug-in * 64 + command` runs a command; the folder and the reload items; one that is only a note.
const MENU_PLUGIN_FOLDER: u32 = 0x1000_0030;
const MENU_PLUGIN_RELOAD: u32 = 0x1000_0031;
const MENU_PLUGIN_NOTE: u32 = 0x1000_0032;
const MENU_PLUGIN: u32 = 0x1000_1000;
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
            // A document with several pages is left only on purpose (Ctrl+Left / Ctrl+Right = previous / next file): a
            // wheel spun fast must stop at the last page, not carry on into the next file.
            if n >= 2 {
                let (key, edge) = if d > 0 { ("Ctrl+Right", "last") } else { ("Ctrl+Left", "first") };
                self.toast(format!("This is the {edge} page  ·  {key} goes to the {} file", if d > 0 { "next" } else { "previous" }));
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
        if self.pano_active() {
            self.disp.pano_last = Some((x, y)); // dragging looks around the 360 picture
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
            // A small picture that "fit" has enlarged is already bigger than 100% or 200%: holding zooms in from there.
            let shown = self.view.scale(img, win);
            let target = if target <= shown * 1.05 { (shown * 2.0).min(64.0) } else { target };
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
        if self.media.vol_drag {
            self.set_volume_at(x);
            return;
        }
        if self.disp.pano_last.is_some() {
            self.pano_drag(x, y);
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
        self.disp.pano_last = None;
        self.seek_drag = false;
        if std::mem::take(&mut self.media.vol_drag) {
            let _ = self.settings.save(); // the volume the user dragged to is kept
        }
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
        let label = nv_text::tr(label);
        Entry::item(ACTION_BASE + idx as u32, if hint.is_empty() { label.into_owned() } else { format!("{label}\t{hint}") })
    }

    /// "Undo delete" / "Undo move" ...: what Ctrl+Z would take back, named.
    pub fn undo_label(&self) -> String {
        use nv_core::UndoOp;
        match self.undo.last() {
            Some(UndoOp::Deleted { .. }) => "Undo delete",
            Some(UndoOp::Moved { .. }) => "Undo move / rename",
            Some(UndoOp::Copied { .. }) => "Undo copy",
            Some(UndoOp::Rewritten { .. }) => "Undo save",
            Some(UndoOp::Group(v)) => match v.first() {
                Some(UndoOp::Deleted { .. }) => "Undo delete (RAW + JPG)",
                Some(UndoOp::Copied { .. }) => "Undo copy (RAW + JPG)",
                _ => "Undo move / rename (RAW + JPG)",
            },
            None => "Undo",
        }
        .to_string()
    }

    /// "Open with": the programs Windows offers for this kind of file (the list Explorer's Open with menu shows), then the picker
    /// for any other program. The ids of the programs are kept for `menu_pick`.
    fn open_with_entry(&self, enabled: bool) -> Entry {
        let file = self.target().and_then(|t| self.folder.files.get(t)).cloned();
        let handlers = match (&file, enabled) {
            (Some(f), true) => nv_win::openwith::handlers(f),
            _ => Vec::new(),
        };
        *self.open_with_ids.borrow_mut() = handlers.iter().map(|h| h.id.clone()).collect();
        let mut items: Vec<Entry> = handlers.iter().enumerate().take(14).map(|(i, h)| Entry::item(OPEN_WITH_BASE + i as u32, h.name.clone())).collect();
        if !items.is_empty() {
            items.push(Entry::Separator);
        }
        items.push(Entry::item(MENU_OPEN_AS, "Choose another app…"));
        Entry::Submenu { text: "Open with".into(), items, enabled: enabled && file.is_some() }
    }

    /// "Edit with": the programs set up in Settings, any program chosen on the spot, and the way to set the list up.
    fn edit_with_entry(&self, enabled: bool) -> Entry {
        let mut items: Vec<Entry> = self.settings.external.iter().enumerate().map(|(i, e)| Entry::item(EXTERNAL_BASE + i as u32, e.name.clone())).collect();
        if !items.is_empty() {
            items.push(Entry::Separator);
        }
        items.push(Entry::item(MENU_CHOOSE_PROGRAM, "Choose a program…"));
        items.push(Entry::item(MENU_SET_UP_PROGRAMS, "Set up the list…"));
        Entry::Submenu { text: "Edit with…".into(), items, enabled }
    }

    /// "Quick edit": turn or flip the file itself in one click (JPEG and PNG; the original stays in the undo folder).
    fn quick_edit_entry(&self) -> Entry {
        let ok = !self.is_model() && self.target().and_then(|t| self.folder.files.get(t)).is_some_and(|p| nv_core::quick_edit::can_rewrite(p));
        let items = vec![
            self.action_entry(Action::QuickRotateLeft, "Rotate left 90°"),
            self.action_entry(Action::QuickRotateRight, "Rotate right 90°"),
            self.action_entry(Action::QuickFlipH, "Flip horizontally"),
            self.action_entry(Action::QuickFlipV, "Flip vertically"),
        ];
        Entry::Submenu { text: "Quick edit".into(), items, enabled: ok }
    }

    /// "Plug-ins": the commands that apply to this picture, then the folder to put plug-ins in and a reload.
    fn plugins_entry(&self, enabled: bool) -> Entry {
        let loaded = nv_core::plugins::current();
        let mut items: Vec<Entry> = match self.target().and_then(|t| self.folder.files.get(t)) {
            Some(f) => loaded.commands_for(f).into_iter().filter(|(pi, ci, _)| *pi < 64 && *ci < 64).map(|(pi, ci, name)| Entry::item(MENU_PLUGIN + (pi * 64 + ci) as u32, name)).collect(),
            None => Vec::new(),
        };
        if items.is_empty() {
            items.push(Entry::item(MENU_PLUGIN_NOTE, if loaded.plugins.is_empty() { "No plug-ins installed" } else { "No plug-in command for this file" }).enabled(false));
        }
        items.push(Entry::Separator);
        items.push(Entry::item(MENU_PLUGIN_FOLDER, "Open the plug-ins folder…"));
        items.push(Entry::item(MENU_PLUGIN_RELOAD, format!("Reload plug-ins ({} loaded{})", loaded.plugins.len(), if loaded.skipped.is_empty() { String::new() } else { format!(", {} skipped", loaded.skipped.len()) })));
        Entry::Submenu { text: "Plug-ins".into(), items, enabled }
    }

    /// The entries under menu bar label `i` (labels: File, Edit, View, Go, Tools, Help).
    pub fn menu_entries(&self, i: usize) -> Vec<Entry> {
        let has = self.target().is_some();
        let viewer = self.mode == Mode::Viewer;
        let many = self.folder.files.len() > 1;
        let checked = |e: Entry, on: bool| match e {
            Entry::Item { id, text, enabled, .. } => Entry::Item { id, text, checked: on, enabled },
            other => other,
        };
        match i {
            0 => vec![
                self.open_with_entry(has),
                self.action_entry(Action::Rename, "Rename…").enabled(has),
                self.action_entry(Action::CopyTo, "Copy to…").enabled(has),
                self.action_entry(Action::MoveTo, "Move to…").enabled(has),
                self.action_entry(Action::Reveal, "Show in Explorer").enabled(has),
                self.action_entry(Action::Share, "Share to\u{2026}").enabled(has),
                self.action_entry(Action::ExtractFrames, "Extract frames\u{2026}").enabled(has),
                Entry::item(MENU_PROPERTIES, "Properties").enabled(has),
                Entry::Separator,
                self.action_entry(Action::ConvertPng, nv_core::i18n::t("menu.convert_png")).enabled(has && viewer),
                self.action_entry(Action::ConvertJpeg, nv_core::i18n::t("menu.convert_jpeg")).enabled(has && viewer),
                self.action_entry(Action::Snapshot, nv_core::i18n::t("menu.snapshot")).enabled(viewer && (self.media_current() || self.controller().is_some())),
                Entry::Separator,
                self.action_entry(Action::Delete, "Delete (Recycle Bin)").enabled(has),
                self.action_entry(Action::DeletePermanently, "Delete permanently…").enabled(has),
                Entry::Separator,
                self.action_entry(Action::Settings, "Settings…"),
                Entry::item(MENU_FILE_TYPES, "File types…"),
                Entry::Separator,
                self.action_entry(Action::Quit, "Close"),
            ],
            1 => vec![
                self.action_entry(Action::Undo, &self.undo_label()).enabled(!self.undo.is_empty()),
                Entry::Separator,
                self.action_entry(Action::CopyImage, "Copy picture").enabled(has),
                self.action_entry(Action::CopyPath, "Copy file path").enabled(has),
                Entry::Separator,
                self.action_entry(Action::Edit, "Edit in NuEditor").enabled(has && viewer),
                self.quick_edit_entry(),
                self.edit_with_entry(has),
                Entry::Separator,
                self.action_entry(Action::RotateMinus45, "Rotate −45°").enabled(viewer),
                self.action_entry(Action::RotatePlus45, "Rotate +45°").enabled(viewer),
                self.action_entry(Action::RotateCcw90, "Rotate −90°").enabled(viewer),
                self.action_entry(Action::RotateCw90, "Rotate +90°").enabled(viewer),
                self.action_entry(Action::SaveRotation, "Save the rotation").enabled(viewer),
            ],
            2 => vec![
                self.action_entry(Action::FitReset, self.fit_toggle().1).enabled(viewer),
                self.action_entry(Action::ZoomIn, "Zoom in").enabled(viewer),
                self.action_entry(Action::ZoomOut, "Zoom out").enabled(viewer),
                Entry::Separator,
                checked(self.action_entry(Action::Gallery, "Gallery").enabled(has), !viewer),
                checked(self.action_entry(Action::Fullscreen, "Fullscreen"), nv_win::is_fullscreen()),
                checked(self.action_entry(Action::Borderless, "Borderless window"), nv_win::is_borderless()),
                checked(self.action_entry(Action::Info, "Info / EXIF").enabled(has), self.hud.show_info),
                checked(self.action_entry(Action::FilmStrip, "Film strip").enabled(viewer && many), self.strip.pinned),
                checked(self.action_entry(Action::ReadMode, "Read mode (two pages)").enabled(viewer && many), self.read.on),
                checked(self.action_entry(Action::ReadDirection, "Japanese reading (right to left)").enabled(viewer), self.settings.read_rtl),
                checked(self.action_entry(Action::Dim, "Dim the screen"), self.disp.dim),
                checked(self.action_entry(Action::Pano, "360 view").enabled(viewer), self.disp.pano.is_some()),
                checked(self.action_entry(Action::RawLook, "RAW: camera look (not sensor data)"), self.settings.raw_mode.is_camera_look()),
                checked(self.action_entry(Action::ToggleSidebar, "Viewer controls").enabled(viewer), self.sidebar.pinned),
                checked(self.action_entry(Action::Layers, "Layers (PSD, XCF, OpenRaster)").enabled(viewer), self.layers.open),
                checked(self.action_entry(Action::ToggleSequences, "Fold image sequences"), self.settings.sequences),
                checked(self.action_entry(Action::TogglePairs, "Group RAW + JPG pairs"), self.settings.pair_group),
                Entry::Separator,
                checked(self.action_entry(Action::Slideshow, "Slideshow").enabled(many), self.slide.on),
                checked(self.action_entry(Action::Shuffle, "Shuffle").enabled(many), self.folder.order.shuffle.is_some()),
            ],
            3 => vec![
                self.action_entry(Action::Prev, "Previous picture").enabled(viewer && many),
                self.action_entry(Action::Next, "Next picture").enabled(viewer && many),
                self.action_entry(Action::First, "First picture").enabled(viewer && many),
                self.action_entry(Action::Last, "Last picture").enabled(viewer && many),
                Entry::Separator,
                self.action_entry(Action::PrevFile, "Previous file").enabled(many),
                self.action_entry(Action::NextFile, "Next file").enabled(many),
            ],
            4 => vec![
                self.action_entry(Action::Edit, "NuEditor").enabled(has && viewer),
                self.edit_with_entry(has),
                checked(self.action_entry(Action::PickColour, "Pick a colour (copy it)").enabled(has && viewer), self.pick.on),
                checked(self.action_entry(Action::Palette, "Palette of this picture").enabled(has && viewer), self.pick.palette_on),
                self.action_entry(Action::Settings, "Settings…"),
                self.action_entry(Action::DiskMonitor, "Disk speed"),
            ],
            _ => vec![self.action_entry(Action::Help, "Keys, mouse and credits")],
        }
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
        let entries = self.context_entries();
        let Some(id) = menu::popup(self.hwnd, x, y, &entries) else {
            if nv_win::is_fullscreen() {
                set_timer(self.hwnd, TIMER_CURSOR, CURSOR_IDLE_MS);
            }
            return;
        };
        self.menu_pick(id);
    }

    /// The right-click menu's entries.
    pub fn context_entries(&self) -> Vec<Entry> {
        let has_file = self.target().is_some();
        let viewer = self.mode == Mode::Viewer;
        let many = self.folder.files.len() > 1;
        let playable = self.controller().is_some();
        let checked = |e: Entry, on: bool| match e {
            Entry::Item { id, text, enabled, .. } => Entry::Item { id, text, checked: on, enabled },
            other => other,
        };
        let edit_with = self.edit_with_entry(has_file);
        let plugins = self.plugins_entry(has_file);
        let open_with = self.open_with_entry(has_file);
        let order = self.folder.order;
        let sort_menu = {
            let mut items: Vec<Entry> = nv_core::SortKey::ALL.iter().enumerate().map(|(i, k)| Entry::checked(MENU_SORT + i as u32, k.label(), order.shuffle.is_none() && order.key == *k)).collect();
            items.push(Entry::Separator);
            items.push(Entry::checked(MENU_SORT_ASC, "Ascending", !order.descending));
            items.push(Entry::checked(MENU_SORT_DESC, "Descending", order.descending));
            Entry::Submenu { text: "Sort by".into(), items, enabled: has_file && many }
        };
        let rating_menu = {
            let now = self.target().map(|i| nv_core::xmp::read(&self.folder.files[i])).unwrap_or_default();
            let mut items: Vec<Entry> = (0..=5u8)
                .map(|n| {
                    let a = [Action::Rate0, Action::Rate1, Action::Rate2, Action::Rate3, Action::Rate4, Action::Rate5][n as usize];
                    let text = if n == 0 { "No stars".to_string() } else { "\u{2605}".repeat(n as usize) };
                    checked(self.action_entry(a, &text), !now.rejected && now.stars == n)
                })
                .collect();
            items.push(checked(self.action_entry(Action::Reject, "Rejected"), now.rejected));
            items.push(Entry::Separator);
            for (a, l) in [(Action::LabelRed, Label::Red), (Action::LabelYellow, Label::Yellow), (Action::LabelGreen, Label::Green), (Action::LabelBlue, Label::Blue), (Action::LabelPurple, Label::Purple)] {
                items.push(checked(self.action_entry(a, l.name()), now.label == Some(l)));
            }
            items.push(self.action_entry(Action::LabelNone, "No label"));
            Entry::Submenu { text: "Rating".into(), items, enabled: has_file }
        };
        let entries = vec![
            self.action_entry(Action::Prev, "Previous picture").enabled(viewer && many),
            self.action_entry(Action::Next, "Next picture").enabled(viewer && many),
            Entry::Separator,
            self.action_entry(Action::FitReset, self.fit_toggle().1).enabled(viewer),
            checked(self.action_entry(Action::Gallery, "Gallery").enabled(has_file), !viewer),
            checked(self.action_entry(Action::Fullscreen, "Fullscreen"), nv_win::is_fullscreen()),
            checked(self.action_entry(Action::Borderless, "Borderless window"), nv_win::is_borderless()),
            checked(self.action_entry(Action::PickColour, "Pick a colour (copy it)").enabled(has_file && viewer), self.pick.on),
            checked(self.action_entry(Action::Palette, "Palette of this picture").enabled(has_file && viewer), self.pick.palette_on),
            checked(self.action_entry(Action::Slideshow, "Slideshow").enabled(many), self.slide.on),
            self.action_entry(Action::PlayPause, "Play / pause").enabled(playable),
            checked(self.action_entry(Action::ToggleSequences, "Fold image sequences"), self.settings.sequences),
            checked(self.action_entry(Action::TogglePairs, "Group RAW + JPG pairs"), self.settings.pair_group),
            rating_menu,
            checked(self.action_entry(Action::FilmStrip, "Film strip").enabled(viewer && many), self.strip.pinned),
            checked(self.action_entry(Action::ReadMode, "Read mode (two pages)").enabled(viewer && many), self.read.on),
            checked(self.action_entry(Action::ReadDirection, "Japanese reading (right to left)").enabled(viewer), self.settings.read_rtl),
            checked(self.action_entry(Action::Dim, "Dim the screen"), self.disp.dim),
            checked(self.action_entry(Action::Pano, "360 view").enabled(viewer), self.disp.pano.is_some()),
            checked(self.action_entry(Action::RawLook, "RAW: camera look (not sensor data)"), self.settings.raw_mode.is_camera_look()),
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
            self.action_entry(Action::Share, "Share to\u{2026}").enabled(has_file),
            self.action_entry(Action::ExtractFrames, "Extract frames\u{2026}").enabled(has_file),
            Entry::item(MENU_PROPERTIES, "Properties").enabled(has_file),
            self.action_entry(Action::ConvertPng, nv_core::i18n::t("menu.convert_png")).enabled(has_file && viewer),
            self.action_entry(Action::ConvertJpeg, nv_core::i18n::t("menu.convert_jpeg")).enabled(has_file && viewer),
            self.action_entry(Action::Snapshot, nv_core::i18n::t("menu.snapshot")).enabled(viewer && (self.media_current() || self.controller().is_some())),
            self.action_entry(Action::Delete, "Delete (Recycle Bin)").enabled(has_file),
            self.action_entry(Action::Undo, &self.undo_label()).enabled(!self.undo.is_empty()),
            Entry::Separator,
            self.action_entry(Action::Edit, "Edit in NuEditor").enabled(has_file && viewer),
            self.quick_edit_entry(),
            open_with,
            edit_with,
            plugins,
            Entry::Separator,
            self.action_entry(Action::Settings, "Settings…"),
            Entry::item(MENU_FILE_TYPES, "File types…"),
            checked(self.action_entry(Action::Help, "Help"), self.hud.show_help),
            self.action_entry(Action::Quit, "Close"),
        ];
        entries
    }

    /// Carry out a menu choice (right-click menu or menu bar).
    pub fn menu_pick(&mut self, id: u32) {
        match id {
            MENU_CHOOSE_PROGRAM => {
                if let (Some(program), Some(t)) = (nv_win::shell::pick_program(self.hwnd), self.target()) {
                    let command = format!("\"{}\"", program.display());
                    if let Err(e) = nv_win::shell::run_external(&command, &self.folder.files[t]) {
                        self.toast(e);
                    }
                }
            }
            MENU_SET_UP_PROGRAMS => self.open_settings(),
            MENU_PLUGIN_FOLDER => {
                let dir = nv_core::plugins::plugins_dir();
                let _ = std::fs::create_dir_all(&dir);
                nv_win::shell::open_url(&dir.display().to_string());
            }
            MENU_PLUGIN_RELOAD => self.reload_plugins(),
            i if (MENU_PLUGIN..MENU_PLUGIN + 4096).contains(&i) => {
                let n = (i - MENU_PLUGIN) as usize;
                let loaded = nv_core::plugins::current();
                if let (Some(p), Some(f)) = (loaded.plugins.get(n / 64), self.target().and_then(|t| self.folder.files.get(t)).cloned()) {
                    if let Some(c) = p.commands.get(n % 64) {
                        match nv_core::plugins::run_command(p, c, &[f]) {
                            Ok(_) => self.toast(format!("{}: {}", p.name, c.name)),
                            Err(e) => self.toast(e),
                        }
                    }
                }
            }
            MENU_OPEN_AS => {
                if let Some(f) = self.target().and_then(|t| self.folder.files.get(t)).cloned() {
                    if let Err(e) = nv_win::openwith::choose(self.hwnd, &f) {
                        self.toast(format!("Can't open it: {e}"));
                    }
                }
            }
            MENU_PROPERTIES => {
                if let Some(f) = self.target().and_then(|t| self.folder.files.get(t)).cloned() {
                    if let Err(e) = nv_win::files::properties(self.hwnd, &f) {
                        self.toast(format!("Can't show the properties: {e}"));
                    }
                }
            }
            i if (OPEN_WITH_BASE..OPEN_WITH_BASE + 64).contains(&i) => {
                let program = self.open_with_ids.borrow().get((i - OPEN_WITH_BASE) as usize).cloned();
                if let (Some(id), Some(f)) = (program, self.target().and_then(|t| self.folder.files.get(t)).cloned()) {
                    if let Err(e) = nv_win::openwith::open(&f, &id) {
                        self.toast(format!("Can't open it: {e}"));
                    }
                }
            }
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
