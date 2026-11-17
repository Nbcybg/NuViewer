//! What makes the editor's features findable: the menu bar (Alt, F10 or the top edge), the right-click menu, the tool strip at
//! the bottom of the picture, the window / fullscreen switch, the navigator that appears when the picture is zoomed (drag it to
//! pan), the extent outline that shows what a turn, Pitch or Yaw did to the picture's edges, and the help page with the credits.
//! Every entry runs the same code as its key (`command`), so a menu item and a shortcut can't drift apart.

use super::panel::{GROUP_COMPOSITION, TOOL_KINDS};
use super::*;
use nv_editor::recipe::Crop;
use nv_win::menu::{self, Entry};
use view::Scope;

pub const MENU_LABELS: [&str; 5] = ["&File", "&Edit", "&View", "&Tools", "&Help"];

// Command ids (never 0: that means "nothing picked").
pub const C_SAVE: u32 = 1;
pub const C_SAVE_COPY: u32 = 2;
pub const C_CLOSE: u32 = 3;
pub const C_UNDO: u32 = 10;
pub const C_REDO: u32 = 11;
pub const C_RESET_ALL: u32 = 12;
pub const C_ORIGINAL: u32 = 13;
pub const C_FLIP_H: u32 = 14;
pub const C_FLIP_V: u32 = 15;
pub const C_TURN_L: u32 = 16;
pub const C_TURN_R: u32 = 17;
pub const C_INVERT: u32 = 18;
pub const C_RESTORE: u32 = 19;
pub const C_TO_PNG: u32 = 20;
pub const C_COPY: u32 = 22;
pub const C_TO_JPEG: u32 = 21;
pub const C_FIT: u32 = 30;
pub const C_Z100: u32 = 31;
pub const C_Z200: u32 = 32;
pub const C_Z400: u32 = 33;
pub const C_ZIN: u32 = 34;
pub const C_ZOUT: u32 = 35;
pub const C_SPLIT: u32 = 36;
pub const C_SCOPE_OFF: u32 = 37;
pub const C_SCOPE_HIST: u32 = 38;
pub const C_SCOPE_PARADE: u32 = 39;
pub const C_SCOPE_VECTOR: u32 = 40;
pub const C_FALSE: u32 = 41;
pub const C_PANEL: u32 = 42;
pub const C_FULLSCREEN: u32 = 43;
pub const C_BORDERLESS: u32 = 120;
pub const C_PICK: u32 = 121;
pub const C_PALETTE: u32 = 122;
pub const C_SCOPE_NEXT: u32 = 44;
pub const C_CROP: u32 = 50;
pub const C_WB: u32 = 51;
pub const C_AUTOCROP: u32 = 57;
pub const C_CLEARCROP: u32 = 58;
pub const C_RETOUCH_MENU: u32 = 59;
pub const C_HEAL_BASE: u32 = 60; // + index into TOOL_KINDS
pub const C_HELP: u32 = 70;
pub const C_GALLERY: u32 = 71;

pub const HELP: &[&str] = &[
    concat!("NuEditor ", env!("CARGO_PKG_VERSION"), " — keys and mouse"),
    "",
    "Alt / F10  menu bar (also: the pointer on the top edge)  ·  right-click the picture: menu  ·  F11 / Alt+Enter  window ⇄ fullscreen",
    "Picture:  wheel or Ctrl + / Ctrl −  zoom  ·  drag a zoomed picture to pan (or drag the navigator, arrow keys too)  ·  double click  fit ⇄ 100%",
    "          F fit  ·  1 2 4  100% 200% 400%  ·  Ctrl+0 fit  ·  S  A/B split  ·  C  show the original  ·  Tab  hide / show the panel",
    "Graphs:   the three scopes sit at the top of the panel (histogram, RGB parade, vector scope with the skin-tone line)  ·  Y  next graph  ·  X  false colour",
    "Crop:     R  or the Crop button  ·  drag on the picture for a new frame, drag the frame or its handles  ·  A  ratio  ·  Ctrl+drag a line to level  ·  Enter applies",
    "          Composition (Rotate / Pitch / Yaw / Zoom) shows the whole picture with its outline and your crop frame  ·  \"Crop to the picture\" removes the empty corners",
    "Tools:    W  white balance picker  ·  K clone  J heal  M remove  P patch  E red-eye  ·  , .  turn 45°  ·  H V  flip  ·  I  invert  ·  0  reset all",
    "Panel:    drag a slider  ·  wheel over it  ·  right-click or double-click a slider  back to its default  ·  right-click a header  resets the group",
    "Keep:     Ctrl+S  save the way Settings says  ·  Ctrl+Shift+S  save a copy  ·  Ctrl+Z / Ctrl+Y  undo / redo  ·  Ctrl+Alt+Z  bring back a burned-in original",
    "Leave:    Esc  asks how to keep your edits (pair file / new file / burn in / discard) and goes back to where you came from",
    "          F1  this page",
];

impl App {
    // ------------------------------------------------------------------------------------------ geometry overlay

    /// While the Composition section is open the whole picture is shown with its outline and the crop frame, so Rotate, Pitch
    /// and Yaw can be judged against the edges and the crop (instead of the already cropped result).
    pub(super) fn geo_overlay(&self) -> bool {
        self.panel && self.open_group == GROUP_COMPOSITION && self.tools.mode == Mode::None && self.view.zoom.is_none() && self.full.is_some()
    }

    /// The picture's four corners on the (uncropped) canvas, if they are all in front of the camera.
    pub(super) fn extent(&self) -> Option<[(f32, f32); 4]> {
        let f = self.full.as_ref()?;
        let q = engine::extent_quad(&self.recipe, f.w, f.h);
        Some([q[0]?, q[1]?, q[2]?, q[3]?])
    }

    /// Size in pixels of the canvas the geometry makes before the crop, and the picture's own width / height.
    fn canvas_and_aspect(&self) -> Option<((u32, u32), f32)> {
        let f = self.full.as_ref()?;
        let angle = self.recipe.get(P::Angle) + 90.0 * (self.recipe.quarter_turns % 4) as f32;
        let canvas = nv_editor::raster::rotated_size(f.w, f.h, angle);
        let aspect = if self.recipe.quarter_turns % 2 == 1 { f.h as f32 / f.w as f32 } else { f.w as f32 / f.h as f32 };
        Some((canvas, aspect))
    }

    /// "Crop to the picture": the biggest frame of the picture's own shape that has no empty corner.
    pub(super) fn auto_crop(&mut self) {
        let (Some(quad), Some((canvas, aspect))) = (self.extent(), self.canvas_and_aspect()) else {
            self.say("The picture's corners are out of view: lower Pitch / Yaw first");
            return;
        };
        match nv_editor::cropmath::inscribed(quad, canvas.0, canvas.1, aspect) {
            Some(c) => {
                let whole = c.x <= 0.002 && c.y <= 0.002 && c.w >= 0.996 && c.h >= 0.996;
                self.snapshot();
                self.recipe.crop = if whole { None } else { Some(c) };
                self.say(if whole { "Nothing to cut away: the picture fills its canvas" } else { "Cropped to the picture: no empty corners" });
                self.changed();
            }
            None => self.say("Couldn't find a frame inside the picture"),
        }
    }

    // ------------------------------------------------------------------------------------------ window

    /// F11 / Alt+Enter / View > Fullscreen: window ⇄ fullscreen (the other programs follow, `winstate`).
    pub(super) fn toggle_window(&mut self) {
        nv_win::toggle_fullscreen(self.hwnd);
        let (w, h) = nv_win::client_size(self.hwnd);
        // The WM_SIZE of the style change arrives while this handler is busy: resize by hand.
        self.handle(Event::Resize { width: w, height: h });
        self.update_title();
    }

    /// Shift+F11 / View > Borderless window.
    pub(super) fn toggle_borderless(&mut self) {
        nv_win::toggle_borderless(self.hwnd);
        let (w, h) = nv_win::client_size(self.hwnd);
        self.handle(Event::Resize { width: w, height: h });
        self.update_title();
    }

    pub(super) fn toggle_panel(&mut self) {
        self.panel = !self.panel;
        if self.ensure_proxy() {
            self.rerender();
        }
        if self.view.zoom.is_some() {
            self.view.detail_dirty = true;
        }
        self.refresh_view();
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ menus

    fn checked(id: u32, text: &str, on: bool) -> Entry {
        Entry::checked(id, text, on)
    }

    fn scope_menu(&self) -> Entry {
        let s = self.view.scope;
        Entry::Submenu {
            text: "Graphs".into(),
            items: vec![
                Self::checked(C_SCOPE_HIST, "Histogram", s == Scope::Histogram),
                Self::checked(C_SCOPE_PARADE, "RGB parade", s == Scope::Parade),
                Self::checked(C_SCOPE_VECTOR, "Vector scope (with the skin line)", s == Scope::Vector),
                Self::checked(C_SCOPE_OFF, "Off", s == Scope::Off),
                Entry::Separator,
                Entry::item(C_SCOPE_NEXT, "Next graph\tY"),
            ],
            enabled: true,
        }
    }

    fn retouch_menu(&self) -> Entry {
        let items = TOOL_KINDS.iter().enumerate().map(|(i, (k, name, key))| Self::checked(C_HEAL_BASE + i as u32, &format!("{name}\t{key}"), self.tools.mode == Mode::Heal(*k))).collect();
        Entry::Submenu { text: "Retouch".into(), items, enabled: self.full.is_some() }
    }

    /// The entries under menu bar label `i`.
    pub(super) fn menu_entries(&self, i: usize) -> Vec<Entry> {
        let has = self.full.is_some();
        let edits = !self.recipe.is_identity();
        let zoomed = self.view.zoom.is_some();
        match i {
            0 => vec![
                Entry::item(C_SAVE, "Save\tCtrl+S").enabled(has && (edits || self.dirty())),
                Entry::item(C_SAVE_COPY, "Save a copy…\tCtrl+Shift+S").enabled(has),
                Entry::item(C_TO_PNG, nv_core::i18n::t("menu.convert_png")).enabled(has),
                Entry::item(C_TO_JPEG, nv_core::i18n::t("menu.convert_jpeg")).enabled(has),
                Entry::item(C_COPY, nv_core::i18n::t("menu.copy_picture")).enabled(has),
                Entry::item(C_RESTORE, "Bring back the burned-in original\tCtrl+Alt+Z"),
                Entry::Separator,
                Entry::item(C_GALLERY, "Close and go back\tEsc"),
            ],
            1 => vec![
                Entry::item(C_UNDO, "Undo\tCtrl+Z").enabled(!self.undo.is_empty()),
                Entry::item(C_REDO, "Redo\tCtrl+Y").enabled(!self.redo.is_empty()),
                Entry::item(C_RESET_ALL, "Reset all\t0").enabled(edits),
                Entry::Separator,
                Self::checked(C_ORIGINAL, "Show the original\tC", self.show_original),
                Entry::Separator,
                Self::checked(C_FLIP_H, "Flip horizontally\tH", self.recipe.flip_h),
                Self::checked(C_FLIP_V, "Flip vertically\tV", self.recipe.flip_v),
                Entry::item(C_TURN_L, "Turn left 45°\t,"),
                Entry::item(C_TURN_R, "Turn right 45°\t."),
                Self::checked(C_INVERT, "Invert\tI", self.recipe.invert),
            ],
            2 => vec![
                Entry::item(C_FIT, "Fit to the window\tF").enabled(zoomed),
                Entry::item(C_Z100, "100%\t1"),
                Entry::item(C_Z200, "200%\t2"),
                Entry::item(C_Z400, "400%\t4"),
                Entry::item(C_ZIN, "Zoom in\tCtrl +"),
                Entry::item(C_ZOUT, "Zoom out\tCtrl −"),
                Entry::Separator,
                Self::checked(C_SPLIT, "Compare with the original (split)\tS", self.view.split),
                self.scope_menu(),
                Self::checked(C_FALSE, "False colour\tX", self.view.false_colour),
                Self::checked(C_PANEL, "Panel\tTab", self.panel),
                Entry::Separator,
                Self::checked(C_FULLSCREEN, "Fullscreen\tF11", nv_win::is_fullscreen()),
                Self::checked(C_BORDERLESS, "Borderless window\tShift+F11", nv_win::is_borderless()),
            ],
            3 => vec![
                Self::checked(C_CROP, "Crop…\tR", self.tools.mode == Mode::Crop),
                Entry::item(C_AUTOCROP, "Crop to the picture (no empty corners)").enabled(has),
                Entry::item(C_CLEARCROP, "Remove the crop").enabled(self.recipe.crop.is_some()),
                Self::checked(C_WB, "White balance picker\tW", self.tools.mode == Mode::Picker),
                Self::checked(C_PICK, "Pick a colour (copy it)\tK", self.colour.on),
                Self::checked(C_PALETTE, "Palette of the picture\tCtrl+K", self.colour.palette_on),
                self.retouch_menu(),
            ],
            _ => vec![Entry::item(C_HELP, "Keys, mouse and credits\tF1")],
        }
    }

    /// The right-click menu on the picture: the things you reach for while looking at it.
    fn context_entries(&self) -> Vec<Entry> {
        let has = self.full.is_some();
        let zoomed = self.view.zoom.is_some();
        vec![
            Entry::item(C_UNDO, "Undo\tCtrl+Z").enabled(!self.undo.is_empty()),
            Entry::item(C_REDO, "Redo\tCtrl+Y").enabled(!self.redo.is_empty()),
            Self::checked(C_ORIGINAL, "Show the original\tC", self.show_original),
            Self::checked(C_SPLIT, "Compare with the original\tS", self.view.split),
            Entry::Separator,
            Entry::item(C_FIT, "Fit to the window\tF").enabled(zoomed),
            Entry::item(C_Z100, "100%\t1"),
            Entry::item(C_ZIN, "Zoom in\tCtrl +"),
            Entry::item(C_ZOUT, "Zoom out\tCtrl −"),
            Entry::Separator,
            Self::checked(C_CROP, "Crop…\tR", self.tools.mode == Mode::Crop),
            Self::checked(C_WB, "White balance picker\tW", self.tools.mode == Mode::Picker),
            Self::checked(C_PICK, "Pick a colour (copy it)\tK", self.colour.on),
            Self::checked(C_PALETTE, "Palette of the picture\tCtrl+K", self.colour.palette_on),
            self.retouch_menu(),
            self.scope_menu(),
            Self::checked(C_PANEL, "Panel\tTab", self.panel),
            Self::checked(C_FULLSCREEN, "Fullscreen\tF11", nv_win::is_fullscreen()),
            Self::checked(C_BORDERLESS, "Borderless window\tShift+F11", nv_win::is_borderless()),
            Entry::Separator,
            Entry::item(C_SAVE, "Save\tCtrl+S").enabled(has && self.dirty()),
            Entry::item(C_SAVE_COPY, "Save a copy…\tCtrl+Shift+S").enabled(has),
            Entry::item(C_TO_PNG, nv_core::i18n::t("menu.convert_png")).enabled(has),
            Entry::item(C_TO_JPEG, nv_core::i18n::t("menu.convert_jpeg")).enabled(has),
            Entry::item(C_HELP, "Keys and credits\tF1"),
            Entry::item(C_GALLERY, "Close\tEsc"),
        ]
    }

    pub(super) fn open_menu(&mut self, i: usize) {
        let entries = self.menu_entries(i);
        invalidate(self.hwnd);
        if let Some(id) = self.menubar.run(self.hwnd, i, &entries) {
            self.command(id);
        }
        invalidate(self.hwnd);
    }

    pub(super) fn context_menu(&mut self, x: i32, y: i32) {
        let entries = self.context_entries();
        if let Some(id) = menu::popup(self.hwnd, x, y, &entries) {
            self.command(id);
        }
    }

    /// Run a menu / tool strip command.
    pub(super) fn command(&mut self, id: u32) {
        let a = self.area();
        let mid = ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0);
        match id {
            C_SAVE => self.save_now(),
            C_SAVE_COPY => self.save_copy_dialog(),
            C_TO_PNG => self.quick_convert(nv_core::quick::Target::Png),
            C_COPY => self.copy_picture(),
            C_TO_JPEG => self.quick_convert(nv_core::quick::Target::Jpeg),
            C_RESTORE => self.restore_original(),
            C_CLOSE | C_GALLERY => {
                if self.request_exit() {
                    nv_win::quit();
                }
            }
            C_UNDO => self.undo(),
            C_REDO => self.redo(),
            C_RESET_ALL => {
                if !self.recipe.is_identity() {
                    self.snapshot();
                    self.recipe = Recipe::default();
                    self.changed();
                }
            }
            C_ORIGINAL => {
                self.show_original = !self.show_original;
                self.refresh_view();
            }
            C_FLIP_H => {
                self.snapshot();
                self.recipe.flip_h = !self.recipe.flip_h;
                self.changed();
            }
            C_FLIP_V => {
                self.snapshot();
                self.recipe.flip_v = !self.recipe.flip_v;
                self.changed();
            }
            C_TURN_L => self.turn(-45.0),
            C_TURN_R => self.turn(45.0),
            C_INVERT => {
                self.snapshot();
                self.recipe.invert = !self.recipe.invert;
                self.changed();
            }
            C_FIT => {
                if self.view.zoom.is_some() {
                    self.leave_detail();
                }
            }
            C_Z100 => self.set_zoom(1.0, mid),
            C_Z200 => self.set_zoom(2.0, mid),
            C_Z400 => self.set_zoom(4.0, mid),
            C_ZIN => self.set_zoom(self.zoom_now() * 1.25, mid),
            C_ZOUT => self.set_zoom(self.zoom_now() / 1.25, mid),
            C_SPLIT => {
                self.view.split = !self.view.split;
                self.view.detail_dirty = self.view.zoom.is_some();
                invalidate(self.hwnd);
            }
            C_SCOPE_OFF => self.set_scope(Scope::Off),
            C_SCOPE_HIST => self.set_scope(Scope::Histogram),
            C_SCOPE_PARADE => self.set_scope(Scope::Parade),
            C_SCOPE_VECTOR => self.set_scope(Scope::Vector),
            C_SCOPE_NEXT => {
                let s = self.view.scope.next();
                self.set_scope(s);
            }
            C_FALSE => {
                self.view_key(0x58);
            }
            C_PANEL => self.toggle_panel(),
            C_FULLSCREEN => self.toggle_window(),
            C_BORDERLESS => self.toggle_borderless(),
            C_PICK => self.toggle_colour(),
            C_PALETTE => self.toggle_palette(),
            C_CROP => {
                if self.tools.mode == Mode::Crop {
                    self.finish_crop(true);
                } else if self.tools.mode == Mode::None {
                    self.tool_key(0x52);
                } else {
                    self.say("Finish the tool that is on first (Esc)");
                }
            }
            C_WB => {
                if self.tools.mode == Mode::None || self.tools.mode == Mode::Picker {
                    self.tool_key(0x57);
                }
            }
            C_AUTOCROP => self.auto_crop(),
            C_CLEARCROP => {
                if self.recipe.crop.is_some() {
                    self.snapshot();
                    self.recipe.crop = None;
                    self.changed();
                }
            }
            C_RETOUCH_MENU => {
                // The tool strip's "Retouch": the five tools under the button.
                let at = self.toolbar_hits.iter().find(|(c, _)| *c == C_RETOUCH_MENU).map(|(_, r)| *r);
                if let Some(r) = at {
                    let items: Vec<Entry> = TOOL_KINDS.iter().enumerate().map(|(i, (k, name, key))| Self::checked(C_HEAL_BASE + i as u32, &format!("{name}\t{key}"), self.tools.mode == Mode::Heal(*k))).collect();
                    if let Some(pick) = menu::popup(self.hwnd, r[0] as i32, (r[1] - 4.0 * self.dpi - items.len() as f32 * 24.0 * self.dpi).max(0.0) as i32, &items) {
                        self.command(pick);
                    }
                }
            }
            C_HELP => {
                self.help = !self.help;
                invalidate(self.hwnd);
            }
            i if (C_HEAL_BASE..C_HEAL_BASE + TOOL_KINDS.len() as u32).contains(&i) => self.set_heal_tool(TOOL_KINDS[(i - C_HEAL_BASE) as usize].0),
            _ => {}
        }
    }

    /// A view of a newly opened picture: fitted, with the graph the user chose last time.
    pub(super) fn fresh_view(&self) -> View {
        View { scope: if self.legacy_layout { Scope::Off } else { Scope::from_setting(self.settings.editor_graph) }, ..View::default() }
    }

    pub(super) fn set_scope(&mut self, s: Scope) {
        self.view.scope = s;
        // Remember it for next time (load first: the gallery or the viewer may have changed other settings meanwhile).
        if self.settings.editor_graph != s.to_setting() {
            self.settings.editor_graph = s.to_setting();
            let mut on_disk = nv_core::settings::Settings::load();
            on_disk.editor_graph = s.to_setting();
            let _ = on_disk.save();
        }
        if s == Scope::Off {
            self.say("Graphs off (Y brings them back)");
        }
        self.refresh_view();
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ the tool strip

    /// The strip's buttons: (command, label, is it on).
    fn toolbar_items(&self) -> Vec<(u32, String, bool)> {
        let m = self.tools.mode;
        vec![
            (C_CROP, "Crop".into(), m == Mode::Crop),
            (C_WB, "White picker".into(), m == Mode::Picker),
            (C_RETOUCH_MENU, "Retouch ▴".into(), matches!(m, Mode::Heal(_))),
            (0, String::new(), false),
            (C_FIT, "Fit".into(), self.view.zoom.is_none()),
            (C_Z100, "100%".into(), self.view.zoom.map(|z| (z - 1.0).abs() < 1e-3).unwrap_or(false)),
            (C_ZOUT, "−".into(), false),
            (C_ZIN, "+".into(), false),
            (0, String::new(), false),
            (C_SPLIT, "Split".into(), self.view.split),
            (C_SCOPE_NEXT, format!("Graph: {}", self.view.scope.short()), self.view.scope != Scope::Off),
            (C_PANEL, "Panel".into(), self.panel),
        ]
    }

    /// Place the strip along the bottom of the picture area; returns what to draw and records where each button is for clicks.
    pub(super) fn toolbar_layout(&self) -> Vec<(u32, [f32; 4], String, bool)> {
        let d = self.dpi;
        let items = self.toolbar_items();
        let widths: Vec<f32> = items.iter().map(|(c, label, _)| if *c == 0 { 10.0 * d } else { (label.chars().count() as f32 * 7.6 + 22.0) * d }).collect();
        let total: f32 = widths.iter().sum::<f32>() + 3.0 * d * items.len() as f32;
        let a = self.area();
        let mut x = ((a[0] + a[2]) / 2.0 - total / 2.0).max(a[0] + 8.0 * d);
        let (y0, y1) = (a[3] - 12.0 * d - 30.0 * d, a[3] - 12.0 * d);
        let mut out = Vec::new();
        for ((c, label, on), w) in items.into_iter().zip(widths) {
            out.push((c, [x, y0, x + w, y1], label, on));
            x += w + 3.0 * d;
        }
        out
    }

    /// A press on the strip or the navigator. True if used.
    pub(super) fn chrome_press(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        if self.help {
            self.help = false;
            invalidate(self.hwnd);
            return true;
        }
        if let Some((c, _)) = self.toolbar_hits.iter().find(|(c, r)| *c != 0 && contains(*r, fx, fy)).copied() {
            self.command(c);
            return true;
        }
        if let Some(nav) = self.nav_rect {
            if contains(nav, fx, fy) {
                self.view.nav_drag = true;
                self.nav_to(fx, fy);
                return true;
            }
        }
        false
    }

    /// Put the view's centre where the pointer is on the navigator.
    pub(super) fn nav_to(&mut self, x: f32, y: f32) {
        let Some(nav) = self.nav_rect else { return };
        let (w, h) = self.shown_dims();
        let (fx, fy) = (((x - nav[0]) / (nav[2] - nav[0]).max(1.0)).clamp(0.0, 1.0), ((y - nav[1]) / (nav[3] - nav[1]).max(1.0)).clamp(0.0, 1.0));
        self.view.center = (fx * w as f32, fy * h as f32);
        self.clamp_center_pub();
        self.view.detail_dirty = true;
        invalidate(self.hwnd);
    }

    /// Arrow keys pan a zoomed picture by a tenth of the window.
    pub(super) fn pan_key(&mut self, vk: u32) -> bool {
        let Some(z) = self.view.zoom else { return false };
        let a = self.area();
        let (sx, sy) = ((a[2] - a[0]) * 0.1 / z, (a[3] - a[1]) * 0.1 / z);
        match vk {
            0x25 => self.view.center.0 -= sx,
            0x27 => self.view.center.0 += sx,
            0x26 => self.view.center.1 -= sy,
            0x28 => self.view.center.1 += sy,
            _ => return false,
        }
        self.clamp_center_pub();
        self.view.detail_dirty = true;
        invalidate(self.hwnd);
        true
    }

    // ------------------------------------------------------------------------------------------ drawing

    /// Where the navigator goes, if it shows: the picture's shape in a box at the bottom right of the picture area.
    pub(super) fn nav_layout(&self) -> Option<[f32; 4]> {
        if self.view.zoom.is_none() || self.view.proxy_stale || self.full.is_none() {
            return None;
        }
        let d = self.dpi;
        let (w, h) = self.shown_dims();
        let s = (170.0 * d / w as f32).min(120.0 * d / h as f32);
        let (nw, nh) = (w as f32 * s, h as f32 * s);
        let a = self.area();
        let (x1, y1) = (a[2] - 12.0 * d, a[3] - 52.0 * d);
        Some([x1 - nw, y1 - nh, x1, y1])
    }
}

/// Draw a straight line as a run of small squares (the renderer has rectangles only).
pub fn draw_line(r: &mut Renderer, a: (f32, f32), b: (f32, f32), t: f32, color: [f32; 4]) {
    let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let steps = (len / (t * 0.7).max(1.0)).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let (x, y) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
        let _ = r.fill_rect([x - t / 2.0, y - t / 2.0, x + t / 2.0, y + t / 2.0], color);
    }
}

/// The picture's outline (yellow) and, when there is one, the crop frame (white, the rest dimmed) on the picture area `p`.
pub fn draw_extent(r: &mut Renderer, p: [f32; 4], quad: Option<[(f32, f32); 4]>, crop: Option<Crop>, d: f32) {
    let (pw, ph) = (p[2] - p[0], p[3] - p[1]);
    if pw <= 0.0 || ph <= 0.0 {
        return;
    }
    if let Some(c) = crop {
        let (l, t, rr, b) = (p[0] + c.x * pw, p[1] + c.y * ph, p[0] + (c.x + c.w) * pw, p[1] + (c.y + c.h) * ph);
        let dim = rgb(0x000000, 0.55);
        for e in [[p[0], p[1], p[2], t], [p[0], b, p[2], p[3]], [p[0], t, l, b], [rr, t, p[2], b]] {
            let _ = r.fill_rect(e, dim);
        }
        let white = rgb(0xFFFFFF, 0.95);
        for e in [[l, t, rr, t + 1.5 * d], [l, b - 1.5 * d, rr, b], [l, t, l + 1.5 * d, b], [rr - 1.5 * d, t, rr, b]] {
            let _ = r.fill_rect(e, white);
        }
    }
    if let Some(q) = quad {
        let pts = q.map(|(x, y)| (p[0] + x * pw, p[1] + y * ph));
        let c = rgb(0xFFD23D, 0.95);
        for i in 0..4 {
            draw_line(r, pts[i], pts[(i + 1) % 4], 1.6 * d, c);
        }
    }
}
