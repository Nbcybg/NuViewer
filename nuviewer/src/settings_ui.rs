//! The Settings window (Ctrl+, / F9 / right click > Settings / `nuviewer.exe --settings`) with four pages:
//! Viewer, File types, External programs, Mouse & keyboard. OK saves to the settings file and applies at once.

use super::*;
use nv_core::keys::{self, ACTIONS};
use nv_core::settings::{External, HoldZoom, Settings, WheelAction};
use nv_win::ui::{Form, ID_CANCEL, ID_OK, SELECTION_CHANGED};

// Page tabs.
const TAB_VIEWER: u16 = 10;
const TAB_TYPES: u16 = 11;
const TAB_EXTERNAL: u16 = 12;
const TAB_INPUT: u16 = 13;
// Viewer page.
const V_CANVAS: u16 = 101;
const V_PICK: u16 = 102;
const V_FULLSCREEN: u16 = 103;
const V_ICC: u16 = 104;
const V_HOLD: u16 = 105;
const V_SMALL: u16 = 106;
const V_SEQ: u16 = 107;
const V_SEQ_MIN: u16 = 108;
const V_FPS: u16 = 109;
const V_CONTROLS: u16 = 110;
const V_DISK: u16 = 111;
const V_MODEL_TEX: u16 = 112;
// File types page.
const T_APPLY: u16 = 210;
// External programs page.
const E_LIST: u16 = 300;
const E_NAME: u16 = 301;
const E_COMMAND: u16 = 302;
const E_BROWSE: u16 = 303;
const E_ADD: u16 = 304;
const E_UPDATE: u16 = 305;
const E_REMOVE: u16 = 306;
// Mouse & keyboard page.
const M_WHEEL: u16 = 401;
const M_EDGE: u16 = 402;
const M_EDGE_PCT: u16 = 403;
const M_TOPBAR: u16 = 404;
const M_TASKBAR: u16 = 405;
const K_LIST: u16 = 410;
const K_EDIT: u16 = 411;
const K_SET: u16 = 412;
const K_RESET: u16 = 413;
const K_RESET_ALL: u16 = 414;

const HOLD_ITEMS: [&str; 5] = ["Automatic: 100%, 200% for small pictures", "Always 100%", "Always 200%", "Always 400%", "Off"];
const HOLD_VALUES: [HoldZoom; 5] = [HoldZoom::Auto, HoldZoom::X100, HoldZoom::X200, HoldZoom::X400, HoldZoom::Off];
const WHEEL_ITEMS: [&str; 3] = ["Previous / next picture (Ctrl+wheel zooms)", "Zoom (Ctrl+wheel: previous / next)", "Nothing"];
const WHEEL_VALUES: [WheelAction; 3] = [WheelAction::Pages, WheelAction::Zoom, WheelAction::Off];

fn key_rows(s: &Settings) -> Vec<String> {
    let km = keys::Keymap::new(&s.keys);
    ACTIONS
        .iter()
        .map(|a| {
            let chords = km.chords_of(a.action);
            let keys = if chords.is_empty() { "(none)".to_string() } else { keys::format_chords(&chords) };
            format!("{}    —    {}", a.label, keys)
        })
        .collect()
}

fn external_rows(s: &Settings) -> Vec<String> {
    s.external.iter().map(|e| format!("{}    —    {}", e.name, e.command)).collect()
}

fn parse_percent(text: &str, lo: u32, hi: u32, fallback: u32) -> u32 {
    text.trim().trim_end_matches('%').trim().parse::<u32>().map(|v| v.clamp(lo, hi)).unwrap_or(fallback)
}

/// Show the window. Returns the new settings if the user pressed OK.
pub fn edit(current: &Settings) -> Option<Settings> {
    let form = Form::new(&format!("NuViewer {} — Settings", env!("CARGO_PKG_VERSION")), 720, 704).ok()?;
    let c = current;

    // Tabs (always visible).
    form.begin_page(0);
    for (i, (id, name)) in [(TAB_VIEWER, "Viewer"), (TAB_TYPES, "File types"), (TAB_EXTERNAL, "External programs"), (TAB_INPUT, "Mouse && keyboard")].iter().enumerate() {
        form.tab(*id, 12 + i as i32 * 172, 10, 168, 30, name, i == 0, i == 0);
    }

    // ---- page 1: Viewer ----
    form.begin_page(1);
    form.group(12, 52, 696, 150, "Picture");
    form.label(28, 78, 230, 20, "Background colour (hex RRGGBB):");
    form.edit(V_CANVAS, 262, 74, 100, 24, &format!("{:06X}", c.canvas));
    form.button(V_PICK, 372, 73, 90, 26, "Choose…", false);
    form.check(V_FULLSCREEN, 28, 106, 660, 22, "Open pictures fullscreen (Alt+Enter or F11 switches)", c.start_fullscreen);
    form.check(V_ICC, 28, 132, 660, 22, "Convert pictures with an embedded colour profile (ICC) to sRGB", c.icc);
    form.label(28, 164, 150, 20, "Hold the left button:");
    form.combo(V_HOLD, 182, 160, 270, 160, &HOLD_ITEMS, HOLD_VALUES.iter().position(|h| *h == c.hold_zoom).unwrap_or(0));
    form.label(464, 164, 170, 20, "\"Small\" = up to (% of screen):");
    form.edit(V_SMALL, 640, 160, 56, 24, &c.small_pct.to_string());
    form.group(12, 212, 696, 130, "Image sequences and playback");
    form.check(V_SEQ, 28, 238, 660, 22, "Fold numbered pictures (shot_0001.png, shot_0002.png …) into one gallery entry  (Q toggles)", c.sequences);
    form.label(28, 270, 200, 20, "A run needs at least (frames):");
    form.edit(V_SEQ_MIN, 232, 266, 56, 24, &c.sequence_min.to_string());
    form.label(320, 270, 270, 20, "Sequence playback (frames per second):");
    form.edit(V_FPS, 600, 266, 56, 24, &c.fps.to_string());
    form.check(V_CONTROLS, 28, 302, 660, 22, "Show the playback controls all the time (otherwise only near the bottom edge; C pins them)", c.controls_always);
    form.group(12, 352, 696, 58, "Loading");
    form.check(V_DISK, 28, 378, 660, 22, "Show the disk's read / write speed on screen all the time  (D; it always appears while a picture loads)", c.disk_monitor);
    form.group(12, 420, 696, 58, "3D models");
    form.check(V_MODEL_TEX, 28, 446, 660, 22, "Load the textures of 3D models (OBJ + MTL, glTF / GLB, FBX, PLY); off = plain grey surface, faster", c.model_textures);
    form.label(28, 494, 660, 60, "RAW and JPG of the same shot are never folded together: a sequence only ever contains files with the same extension.");

    // ---- page 2: file types ----
    form.begin_page(2);
    form.label(14, 56, 680, 34, "Tick the formats Windows should offer to open with NuViewer. Windows only lets you confirm the default yourself: \"Apply\" registers the ticked formats and opens Settings > Default apps.");
    let registered = nv_win::registry::registered_exts(&nv_win::registry::Layout::real());
    let picked = if registered.is_empty() { nv_win::assoc::default_picks() } else { nv_win::assoc::picked_for(&registered) };
    let bottom = nv_win::assoc::build_checklist(&form, 12, 96, 696, &picked);
    form.button(nv_win::assoc::ID_SELECT_ALL, 12, bottom + 4, 96, 26, "Select all", false);
    form.button(nv_win::assoc::ID_SELECT_NONE, 116, bottom + 4, 96, 26, "Select none", false);
    form.button(T_APPLY, 552, bottom + 4, 156, 26, "Apply file types", false);

    // ---- page 3: external programs ----
    form.begin_page(3);
    form.label(14, 56, 680, 34, "Programs the current picture can be sent to. They appear in the right-click menu under \"Open with\". In the command, {file} stands for the picture's path.");
    form.list(E_LIST, 12, 96, 696, 170);
    form.label(14, 280, 80, 20, "Name:");
    form.edit(E_NAME, 100, 276, 240, 24, "");
    form.label(14, 312, 80, 20, "Command:");
    form.edit(E_COMMAND, 100, 308, 520, 24, "");
    form.button(E_BROWSE, 628, 307, 80, 26, "Browse…", false);
    form.button(E_ADD, 100, 344, 90, 28, "Add", false);
    form.button(E_UPDATE, 198, 344, 90, 28, "Update", false);
    form.button(E_REMOVE, 296, 344, 90, 28, "Remove", false);
    form.label(14, 384, 690, 60, "Example:  \"C:\\Program Files\\Adobe\\Adobe Photoshop\\Photoshop.exe\" \"{file}\"");

    // ---- page 4: mouse and keyboard ----
    form.begin_page(4);
    form.group(12, 52, 696, 150, "Mouse");
    form.label(28, 78, 120, 20, "Mouse wheel:");
    form.combo(M_WHEEL, 132, 74, 330, 120, &WHEEL_ITEMS, WHEEL_VALUES.iter().position(|w| *w == c.wheel).unwrap_or(0));
    form.check(M_EDGE, 28, 106, 520, 22, "Click the left / right edge of the picture for previous / next", c.edge_click);
    form.label(556, 108, 80, 20, "Zone width %:");
    form.edit(M_EDGE_PCT, 640, 104, 56, 24, &c.edge_pct.to_string());
    form.check(M_TOPBAR, 28, 134, 660, 22, "Show the info bar when the pointer touches the top edge", c.top_bar);
    form.check(M_TASKBAR, 28, 160, 660, 22, "Show the Windows taskbar when the pointer touches the bottom edge (fullscreen)", c.taskbar_edge);
    form.group(12, 212, 696, 330, "Keyboard shortcuts");
    form.list(K_LIST, 24, 236, 672, 220);
    form.label(24, 466, 150, 20, "Keys for the selected action:");
    form.edit(K_EDIT, 200, 462, 300, 24, "");
    form.button(K_SET, 508, 461, 60, 26, "Set", false);
    form.button(K_RESET, 574, 461, 60, 26, "Reset", false);
    form.button(K_RESET_ALL, 640, 461, 56, 26, "All", false);
    form.label(24, 494, 672, 40, "Separate several keys with commas (Right, Space). Modifiers: Ctrl+  Shift+  Alt+ (Ctrl+Shift+C). Leave it empty to unbind.");

    // Buttons (always visible).
    form.begin_page(0);
    form.button(ID_OK, 536, 666, 84, 28, "OK", true);
    form.button(ID_CANCEL, 628, 666, 80, 28, "Cancel", false);
    form.show_page(1);

    let mut cur = c.clone();
    form.list_set(E_LIST, &external_rows(&cur), None);
    form.list_set(K_LIST, &key_rows(&cur), None);
    let mut result: Option<Settings> = None;

    form.run(&mut |form, id| {
        if nv_win::assoc::handle_select(form, id) {
            return false;
        }
        match id {
            TAB_VIEWER => form.show_page(1),
            TAB_TYPES => form.show_page(2),
            TAB_EXTERNAL => form.show_page(3),
            TAB_INPUT => form.show_page(4),
            V_PICK => {
                let now = u32::from_str_radix(form.text(V_CANVAS).trim().trim_start_matches('#'), 16).unwrap_or(cur.canvas);
                if let Some(c) = nv_win::shell::pick_color(form.hwnd, now) {
                    form.set_text(V_CANVAS, &format!("{c:06X}"));
                }
            }
            T_APPLY => {
                let exe = std::env::current_exe().unwrap_or_default();
                let (text, open) = nv_win::assoc::apply_and_report(&exe, &nv_win::assoc::exts_of(&nv_win::assoc::read_picks(form)));
                nv_win::shell::message(&text);
                if open {
                    nv_win::assoc::open_default_apps_settings();
                }
            }
            x if x == E_LIST | SELECTION_CHANGED => {
                if let Some(i) = form.list_index(E_LIST).and_then(|i| cur.external.get(i)) {
                    form.set_text(E_NAME, &i.name);
                    form.set_text(E_COMMAND, &i.command);
                }
            }
            E_BROWSE => {
                if let Some(p) = nv_win::shell::pick_program(form.hwnd) {
                    form.set_text(E_COMMAND, &format!("\"{}\" \"{{file}}\"", p.display()));
                    if form.text(E_NAME).trim().is_empty() {
                        form.set_text(E_NAME, &p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                    }
                }
            }
            E_ADD | E_UPDATE => {
                let (name, command) = (form.text(E_NAME).trim().to_string(), form.text(E_COMMAND).trim().to_string());
                if name.is_empty() || command.is_empty() {
                    nv_win::shell::message("Give the program a name and a command.");
                } else if id == E_UPDATE {
                    match form.list_index(E_LIST) {
                        Some(i) if i < cur.external.len() => {
                            cur.external[i] = External { name, command };
                            form.list_set(E_LIST, &external_rows(&cur), Some(i));
                        }
                        _ => nv_win::shell::message("Select the entry to update in the list first."),
                    }
                } else {
                    cur.external.push(External { name, command });
                    form.list_set(E_LIST, &external_rows(&cur), Some(cur.external.len() - 1));
                }
            }
            E_REMOVE => {
                if let Some(i) = form.list_index(E_LIST).filter(|&i| i < cur.external.len()) {
                    cur.external.remove(i);
                    form.list_set(E_LIST, &external_rows(&cur), None);
                    form.set_text(E_NAME, "");
                    form.set_text(E_COMMAND, "");
                }
            }
            x if x == K_LIST | SELECTION_CHANGED => {
                if let Some(a) = form.list_index(K_LIST).and_then(|i| ACTIONS.get(i)) {
                    let km = keys::Keymap::new(&cur.keys);
                    form.set_text(K_EDIT, &keys::format_chords(&km.chords_of(a.action)));
                }
            }
            K_SET => {
                let Some(sel) = form.list_index(K_LIST).filter(|&i| i < ACTIONS.len()) else {
                    nv_win::shell::message("Select an action in the list first.");
                    return false;
                };
                let info = &ACTIONS[sel];
                match keys::parse_chords(&form.text(K_EDIT)) {
                    Err(e) => nv_win::shell::message(&format!("Can't use that: {e}.")),
                    Ok(chords) => {
                        // A key can only do one thing: refuse if another action already owns it.
                        let mut without = cur.keys.clone();
                        without.insert(info.id.to_string(), String::new());
                        let km = keys::Keymap::new(&without);
                        let clash = chords.iter().find_map(|ch| km.lookup(ch.vk, ch.ctrl, ch.shift, ch.alt).map(|other| (ch.format(), keys::info(other).label)));
                        if let Some((k, other)) = clash {
                            nv_win::shell::message(&format!("{k} is already used for \"{other}\". Change that one first."));
                        } else {
                            let text = keys::format_chords(&chords);
                            if chords == keys::default_chords(info) {
                                cur.keys.remove(info.id);
                            } else {
                                cur.keys.insert(info.id.to_string(), text);
                            }
                            form.list_set(K_LIST, &key_rows(&cur), Some(sel));
                        }
                    }
                }
            }
            K_RESET | K_RESET_ALL => {
                let sel = form.list_index(K_LIST);
                if id == K_RESET_ALL {
                    cur.keys.clear();
                } else if let Some(i) = sel.filter(|&i| i < ACTIONS.len()) {
                    cur.keys.remove(ACTIONS[i].id);
                }
                form.list_set(K_LIST, &key_rows(&cur), sel);
                if let Some(a) = sel.and_then(|i| ACTIONS.get(i)) {
                    form.set_text(K_EDIT, &keys::format_chords(&keys::Keymap::new(&cur.keys).chords_of(a.action)));
                }
            }
            ID_OK => {
                let mut s = cur.clone();
                s.canvas = u32::from_str_radix(form.text(V_CANVAS).trim().trim_start_matches('#'), 16).map(|v| v & 0xFF_FFFF).unwrap_or(cur.canvas);
                s.start_fullscreen = form.is_checked(V_FULLSCREEN);
                s.icc = form.is_checked(V_ICC);
                s.hold_zoom = HOLD_VALUES[form.combo_index(V_HOLD).min(HOLD_VALUES.len() - 1)];
                s.small_pct = parse_percent(&form.text(V_SMALL), 5, 100, cur.small_pct);
                s.sequences = form.is_checked(V_SEQ);
                s.sequence_min = parse_percent(&form.text(V_SEQ_MIN), 2, 10_000, cur.sequence_min);
                s.fps = parse_percent(&form.text(V_FPS), 1, 240, cur.fps);
                s.controls_always = form.is_checked(V_CONTROLS);
                s.disk_monitor = form.is_checked(V_DISK);
                s.model_textures = form.is_checked(V_MODEL_TEX);
                s.wheel = WHEEL_VALUES[form.combo_index(M_WHEEL).min(WHEEL_VALUES.len() - 1)];
                s.edge_click = form.is_checked(M_EDGE);
                s.edge_pct = parse_percent(&form.text(M_EDGE_PCT), 3, 40, cur.edge_pct);
                s.top_bar = form.is_checked(M_TOPBAR);
                s.taskbar_edge = form.is_checked(M_TASKBAR);
                result = Some(s);
                return true;
            }
            ID_CANCEL => return true,
            _ => {}
        }
        false
    });
    result
}

impl App {
    /// Open the Settings window and apply what the user chose.
    pub fn open_settings(&mut self) {
        nv_win::set_cursor_hidden(false);
        if let Some(s) = edit(&self.settings) {
            self.apply_settings(s);
        }
    }

    /// Make `s` the active settings: save them and update everything that depends on them.
    pub fn apply_settings(&mut self, s: Settings) {
        let sequences_changed = s.sequences != self.settings.sequences || s.sequence_min != self.settings.sequence_min;
        self.keymap = Keymap::new(&s.keys);
        CANVAS.store(s.canvas, Ordering::Relaxed);
        nv_codecs::set_icc_enabled(s.icc);
        let textures_changed = s.model_textures != self.settings.model_textures;
        nv_codecs::set_model_textures(s.model_textures);
        self.fps = s.fps;
        self.settings = s;
        self.sync_loading();
        if textures_changed {
            self.reload_models();
        }
        if let Err(e) = self.settings.save() {
            self.toast(format!("Settings not saved: {e}"));
        }
        if !self.settings.top_bar {
            self.top_bar = false;
        }
        if sequences_changed {
            self.rebuild_items();
            if self.mode == Mode::Gallery {
                self.request_thumbs();
            }
        }
        invalidate(self.hwnd);
    }
}
