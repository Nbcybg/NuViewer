//! The Settings window (Ctrl+, / F9 / right click > Settings / `nuviewer.exe --settings`) with five pages:
//! Viewer, 3D, File types, External programs, Mouse & keyboard. OK saves to the settings file and applies at once.

use super::*;
use nv_core::keys::{self, ACTIONS};
use nv_codecs::model::{LightMode, Shading, UpAxis};
use nv_core::settings::{External, HoldZoom, ProfileChoice, RotateSave, Settings, WheelAction};
use nv_win::ui::{Form, ID_CANCEL, ID_OK, SELECTION_CHANGED};

// Page tabs.
const TAB_VIEWER: u16 = 10;
const TAB_TYPES: u16 = 11;
const TAB_EXTERNAL: u16 = 12;
const TAB_INPUT: u16 = 13;
const TAB_3D: u16 = 14;
const TAB_COLOUR: u16 = 15;
// Viewer page.
const V_CANVAS: u16 = 101;
const V_PICK: u16 = 102;
const V_FULLSCREEN: u16 = 103;
const V_HOLD: u16 = 105;
const V_SMALL: u16 = 106;
const V_SEQ: u16 = 107;
const V_SEQ_MIN: u16 = 108;
const V_FPS: u16 = 109;
const V_CONTROLS: u16 = 110;
const V_DISK: u16 = 111;
const V_MODEL_TEX: u16 = 112;
const V_SIDEBAR: u16 = 113;
const V_ROTATE_SAVE: u16 = 114;
// Colour page.
const C_ENABLE: u16 = 701;
const C_OUT: u16 = 702;
const C_OUT_FILE: u16 = 703;
const C_OUT_BROWSE: u16 = 704;
const C_OUT_STATUS: u16 = 705;
const C_UNT: u16 = 706;
const C_UNT_FILE: u16 = 707;
const C_UNT_BROWSE: u16 = 708;
const C_UNT_STATUS: u16 = 709;
// 3D page.
const D_SHADING: u16 = 601;
const D_TEXTURE: u16 = 602;
const D_NORMAL: u16 = 603;
const D_WIRE: u16 = 604;
const D_SHADOW: u16 = 605;
const D_LIGHT: u16 = 606;
const D_AZ: u16 = 607;
const D_EL: u16 = 608;
const D_PROJECTION: u16 = 609;
const D_UP: u16 = 610;
const D_TURN_X: u16 = 611;
const D_TURN_Y: u16 = 612;
const D_TURN_Z: u16 = 613;
const D_RESET: u16 = 614;
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
const M_TOPBAR: u16 = 404;
const M_TASKBAR: u16 = 405;
const K_LIST: u16 = 410;
const K_EDIT: u16 = 411;
const K_SET: u16 = 412;
const K_RESET: u16 = 413;
const K_RESET_ALL: u16 = 414;

const HOLD_ITEMS: [&str; 5] = ["Automatic: 100%, 200% for small pictures", "Always 100%", "Always 200%", "Always 400%", "Off"];
const HOLD_VALUES: [HoldZoom; 5] = [HoldZoom::Auto, HoldZoom::X100, HoldZoom::X200, HoldZoom::X400, HoldZoom::Off];
const OUT_ITEMS: [&str; 3] = ["sRGB (the default: pictures stay as they are)", "This PC's display profile (the one Windows has for the screen)", "A profile file (.icc / .icm)"];
const OUT_VALUES: [ProfileChoice; 3] = [ProfileChoice::Srgb, ProfileChoice::System, ProfileChoice::File];
const UNT_ITEMS: [&str; 4] = ["sRGB (the default)", "Display P3", "Adobe RGB (1998)", "A profile file (.icc / .icm)"];
const UNT_VALUES: [ProfileChoice; 4] = [ProfileChoice::Srgb, ProfileChoice::DisplayP3, ProfileChoice::AdobeRgb, ProfileChoice::File];
const WHEEL_ITEMS: [&str; 3] = ["Previous / next picture (Ctrl+wheel zooms)", "Zoom (Ctrl+wheel: previous / next)", "Nothing"];
const WHEEL_VALUES: [WheelAction; 3] = [WheelAction::Pages, WheelAction::Zoom, WheelAction::Off];
const ROTATE_ITEMS: [&str; 2] = ["Ask me every time (Save / Discard / Stay)", "Save automatically"];
const ROTATE_VALUES: [RotateSave; 2] = [RotateSave::Ask, RotateSave::Auto];
const LIGHT_ITEMS: [&str; 2] = ["From the viewer (a headlight that follows the camera)", "Fixed in the scene (the model turns under it)"];
const LIGHT_VALUES: [LightMode; 2] = [LightMode::Viewer, LightMode::World];
const PROJECTION_ITEMS: [&str; 2] = ["Perspective", "Orthographic"];
const UP_ITEMS: [&str; 3] = ["Automatic (what the file format says)", "Y is up", "Z is up"];
const UP_VALUES: [UpAxis; 3] = [UpAxis::Auto, UpAxis::Y, UpAxis::Z];
const TURN_ITEMS: [&str; 4] = ["0\u{00B0}", "90\u{00B0}", "180\u{00B0}", "270\u{00B0}"];

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

/// Read the colour page's choices into `s`.
fn colour_fields(form: &Form, s: &mut Settings) {
    s.icc = form.is_checked(C_ENABLE);
    s.icc_output = OUT_VALUES[form.combo_index(C_OUT).min(OUT_VALUES.len() - 1)];
    s.icc_output_file = form.text(C_OUT_FILE).trim().trim_matches('"').to_string();
    s.icc_untagged = UNT_VALUES[form.combo_index(C_UNT).min(UNT_VALUES.len() - 1)];
    s.icc_untagged_file = form.text(C_UNT_FILE).trim().trim_matches('"').to_string();
}

/// Refresh the "In use" lines of the colour page and grey out the file boxes that don't apply.
fn colour_status(form: &Form, base: &Settings) {
    let mut s = base.clone();
    colour_fields(form, &mut s);
    for (status, file_box, browse, choice, file) in [(C_OUT_STATUS, C_OUT_FILE, C_OUT_BROWSE, s.icc_output, &s.icc_output_file), (C_UNT_STATUS, C_UNT_FILE, C_UNT_BROWSE, s.icc_untagged, &s.icc_untagged_file)] {
        let text = match crate::colour::resolve(choice, file, Some(form.hwnd)) {
            Ok(found) => format!("In use: {}", found.text),
            Err(e) => format!("Can't use this yet: {e}"),
        };
        form.set_text(status, &text);
        form.enable(file_box, choice == ProfileChoice::File);
        form.enable(browse, choice == ProfileChoice::File);
    }
}

fn parse_degrees(text: &str, lo: f32, hi: f32, fallback: f32) -> f32 {
    text.trim().trim_end_matches('\u{00B0}').trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(lo, hi)).unwrap_or(fallback)
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
    let mut tab_x = 12;
    for (i, (id, name, width)) in [(TAB_VIEWER, "Viewer", 84), (TAB_COLOUR, "Colour", 84), (TAB_3D, "3D", 56), (TAB_TYPES, "File types", 100), (TAB_EXTERNAL, "External programs", 152), (TAB_INPUT, "Mouse && keyboard", 160)].iter().enumerate() {
        form.tab(*id, tab_x, 10, *width, 30, name, i == 0, i == 0);
        tab_x += *width + 4;
    }

    // ---- page 1: Viewer ----
    form.begin_page(1);
    form.group(12, 52, 696, 150, "Picture");
    form.label(28, 78, 230, 20, "Background colour (hex RRGGBB):");
    form.edit(V_CANVAS, 262, 74, 100, 24, &format!("{:06X}", c.canvas));
    form.button(V_PICK, 372, 73, 90, 26, "Choose…", false);
    form.check(V_FULLSCREEN, 28, 106, 660, 22, "Open pictures fullscreen (Alt+Enter or F11 switches)", c.start_fullscreen);
    form.label(28, 134, 660, 20, "Colour profiles (ICC): output and untagged-picture profiles are on the Colour tab.");
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
    form.group(12, 488, 696, 100, "Controls and rotation");
    form.check(V_SIDEBAR, 28, 514, 660, 22, "Show the viewer controls when the pointer touches the left edge  (V keeps them open)", c.sidebar);
    form.label(28, 550, 250, 20, "A rotated picture, when I move on:");
    form.combo(V_ROTATE_SAVE, 280, 546, 300, 90, &ROTATE_ITEMS, ROTATE_VALUES.iter().position(|r| *r == c.rotate_save).unwrap_or(0));
    form.label(28, 596, 660, 40, "RAW and JPG of the same shot are never folded together: a sequence only ever contains files with the same extension.");

    // ---- page 6: Colour ----
    form.begin_page(6);
    form.group(12, 52, 696, 62, "Colour management");
    form.check(C_ENABLE, 28, 78, 660, 22, "Convert pictures from their own colour profile to the output profile below (off = show the numbers as they are)", c.icc);
    form.group(12, 124, 696, 186, "Output: the profile of what the pictures are shown on");
    form.label(28, 152, 110, 20, "Output profile:");
    form.combo(C_OUT, 140, 148, 420, 110, &OUT_ITEMS, OUT_VALUES.iter().position(|v| *v == c.icc_output).unwrap_or(0));
    form.edit(C_OUT_FILE, 28, 182, 566, 24, &c.icc_output_file);
    form.button(C_OUT_BROWSE, 602, 181, 94, 26, "Browse…", false);
    form.label_id(C_OUT_STATUS, 28, 214, 660, 36, "");
    form.label(28, 252, 660, 52, "For a calibrated or wide-gamut screen, or to work in a profile of your own (grading, graphics) instead of whatever Windows has. Every picture is converted from its own profile to this one, sRGB pictures included.");
    form.group(12, 320, 696, 186, "Pictures that carry no profile");
    form.label(28, 348, 380, 20, "A JPEG, PNG or WebP without a profile is:");
    form.combo(C_UNT, 28, 372, 420, 110, &UNT_ITEMS, UNT_VALUES.iter().position(|v| *v == c.icc_untagged).unwrap_or(0));
    form.edit(C_UNT_FILE, 28, 404, 566, 24, &c.icc_untagged_file);
    form.button(C_UNT_BROWSE, 602, 403, 94, 26, "Browse…", false);
    form.label_id(C_UNT_STATUS, 28, 436, 660, 36, "");
    form.label(28, 474, 660, 28, "A picture with its own profile always uses it. SVG, PDF and other renders are always sRGB.");
    form.label(14, 516, 690, 70, "Rotated pictures that are saved, and the clipboard, stay sRGB. Thumbnails and the picture on screen follow the output profile. A changed profile reloads the open pictures when you press OK.");

    // ---- page 5: 3D ----
    form.begin_page(5);
    form.group(12, 52, 696, 150, "Shading and effects");
    form.label(28, 80, 80, 20, "Shading:");
    form.combo(D_SHADING, 110, 76, 300, 160, &Shading::ALL.map(|m| m.label()), Shading::ALL.iter().position(|m| *m == c.model.shading).unwrap_or(0));
    form.check(D_TEXTURE, 28, 110, 320, 22, "Texture  (T)", c.model.texture);
    form.check(D_NORMAL, 360, 110, 320, 22, "Normal map  (N)", c.model.normal_map);
    form.check(D_WIRE, 28, 136, 320, 22, "Wireframe  (W)", c.model.wireframe);
    form.check(D_SHADOW, 360, 136, 320, 22, "Shadow on the floor  (Shift+S)", c.model.shadow);
    form.label(28, 164, 660, 34, "Shift+T steps through the shadings. A texture or normal map only shows if the model brings one (OBJ + MTL, glTF / GLB, PLY).");
    form.group(12, 212, 696, 160, "Light");
    form.label(28, 240, 60, 20, "Light:");
    form.combo(D_LIGHT, 90, 236, 400, 90, &LIGHT_ITEMS, LIGHT_VALUES.iter().position(|l| *l == c.model.light).unwrap_or(0));
    form.label(28, 272, 250, 20, "Angle to the right (-180 to 180):");
    form.edit(D_AZ, 280, 268, 70, 24, &format!("{}", c.model.light_az));
    form.label(380, 272, 220, 20, "Height above the horizon (-90 to 90):");
    form.edit(D_EL, 610, 268, 70, 24, &format!("{}", c.model.light_el));
    form.label(28, 304, 660, 60, "0 and 0 from the viewer shines straight at the model. A shadow needs the light above the horizon (height over about 15).");
    form.group(12, 382, 696, 200, "Camera and axes");
    form.label(28, 410, 80, 20, "Projection:");
    form.combo(D_PROJECTION, 110, 406, 200, 90, &PROJECTION_ITEMS, if c.model.ortho { 1 } else { 0 });
    form.label(340, 410, 70, 20, "Up axis:");
    form.combo(D_UP, 414, 406, 270, 90, &UP_ITEMS, UP_VALUES.iter().position(|u| *u == c.model.up).unwrap_or(0));
    form.label(28, 446, 660, 20, "Turn the model about its axes (for models that arrive face-down or lying on a side):");
    form.label(28, 480, 30, 20, "X:");
    form.combo(D_TURN_X, 56, 476, 90, 120, &TURN_ITEMS, c.model.turns[0] as usize % 4);
    form.label(180, 480, 30, 20, "Y:");
    form.combo(D_TURN_Y, 208, 476, 90, 120, &TURN_ITEMS, c.model.turns[1] as usize % 4);
    form.label(332, 480, 30, 20, "Z:");
    form.combo(D_TURN_Z, 360, 476, 90, 120, &TURN_ITEMS, c.model.turns[2] as usize % 4);
    form.button(D_RESET, 548, 475, 140, 26, "Reset 3D settings", false);
    form.label(28, 516, 660, 56, "While looking at a model, X / Y / Z turn it 90\u{00B0} about that axis (Shift = the other way) and U swaps the up axis. The choice is remembered for all models; the gallery thumbnails keep the file's own orientation.");

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
    form.group(12, 52, 696, 120, "Mouse");
    form.label(28, 78, 120, 20, "Mouse wheel:");
    form.combo(M_WHEEL, 132, 74, 330, 120, &WHEEL_ITEMS, WHEEL_VALUES.iter().position(|w| *w == c.wheel).unwrap_or(0));
    form.check(M_TOPBAR, 28, 106, 660, 22, "Show the info bar when the pointer touches the top edge", c.top_bar);
    form.check(M_TASKBAR, 28, 132, 660, 22, "Show the Windows taskbar when the pointer touches the bottom edge (fullscreen)", c.taskbar_edge);
    form.group(12, 182, 696, 360, "Keyboard shortcuts");
    form.list(K_LIST, 24, 206, 672, 250);
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
            TAB_COLOUR => {
                colour_status(form, &cur);
                form.show_page(6);
            }
            x if x == C_OUT | SELECTION_CHANGED || x == C_UNT | SELECTION_CHANGED => colour_status(form, &cur),
            C_OUT_BROWSE | C_UNT_BROWSE => {
                let file_box = if id == C_OUT_BROWSE { C_OUT_FILE } else { C_UNT_FILE };
                let current = form.text(file_box);
                if let Some(p) = nv_win::shell::pick_profile(form.hwnd, Some(std::path::Path::new(current.trim()))) {
                    form.set_text(file_box, &p.display().to_string());
                    colour_status(form, &cur);
                }
            }
            TAB_3D => form.show_page(5),
            D_RESET => {
                let m = nv_codecs::model::ModelStyle::default();
                form.combo_set(D_SHADING, Shading::ALL.iter().position(|x| *x == m.shading).unwrap_or(0));
                form.set_checked(D_TEXTURE, m.texture);
                form.set_checked(D_NORMAL, m.normal_map);
                form.set_checked(D_WIRE, m.wireframe);
                form.set_checked(D_SHADOW, m.shadow);
                form.combo_set(D_LIGHT, 0);
                form.set_text(D_AZ, &format!("{}", m.light_az));
                form.set_text(D_EL, &format!("{}", m.light_el));
                form.combo_set(D_PROJECTION, 0);
                form.combo_set(D_UP, 0);
                form.combo_set(D_TURN_X, 0);
                form.combo_set(D_TURN_Y, 0);
                form.combo_set(D_TURN_Z, 0);
            }
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
                colour_fields(form, &mut s);
                if s.icc {
                    if let Err(e) = crate::colour::check(&s, Some(form.hwnd)) {
                        nv_win::shell::message(&format!("{e}\n\nChoose another profile on the Colour tab."));
                        form.show_page(6);
                        colour_status(form, &cur);
                        return false;
                    }
                }
                s.hold_zoom = HOLD_VALUES[form.combo_index(V_HOLD).min(HOLD_VALUES.len() - 1)];
                s.small_pct = parse_percent(&form.text(V_SMALL), 5, 100, cur.small_pct);
                s.sequences = form.is_checked(V_SEQ);
                s.sequence_min = parse_percent(&form.text(V_SEQ_MIN), 2, 10_000, cur.sequence_min);
                s.fps = parse_percent(&form.text(V_FPS), 1, 240, cur.fps);
                s.controls_always = form.is_checked(V_CONTROLS);
                s.disk_monitor = form.is_checked(V_DISK);
                s.model_textures = form.is_checked(V_MODEL_TEX);
                s.wheel = WHEEL_VALUES[form.combo_index(M_WHEEL).min(WHEEL_VALUES.len() - 1)];
                s.sidebar = form.is_checked(V_SIDEBAR);
                s.rotate_save = ROTATE_VALUES[form.combo_index(V_ROTATE_SAVE).min(ROTATE_VALUES.len() - 1)];
                s.model.shading = Shading::ALL[form.combo_index(D_SHADING).min(Shading::ALL.len() - 1)];
                s.model.texture = form.is_checked(D_TEXTURE);
                s.model.normal_map = form.is_checked(D_NORMAL);
                s.model.wireframe = form.is_checked(D_WIRE);
                s.model.shadow = form.is_checked(D_SHADOW);
                s.model.light = LIGHT_VALUES[form.combo_index(D_LIGHT).min(LIGHT_VALUES.len() - 1)];
                s.model.light_az = parse_degrees(&form.text(D_AZ), -180.0, 180.0, cur.model.light_az);
                s.model.light_el = parse_degrees(&form.text(D_EL), -90.0, 90.0, cur.model.light_el);
                s.model.ortho = form.combo_index(D_PROJECTION) == 1;
                s.model.up = UP_VALUES[form.combo_index(D_UP).min(UP_VALUES.len() - 1)];
                s.model.turns = [form.combo_index(D_TURN_X).min(3) as u8, form.combo_index(D_TURN_Y).min(3) as u8, form.combo_index(D_TURN_Z).min(3) as u8];
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
        let colour_changed = (s.icc, s.icc_output, &s.icc_output_file, s.icc_untagged, &s.icc_untagged_file) != (self.settings.icc, self.settings.icc_output, &self.settings.icc_output_file, self.settings.icc_untagged, &self.settings.icc_untagged_file);
        let colour_error = crate::colour::apply(&s, Some(self.hwnd)).err();
        let textures_changed = s.model_textures != self.settings.model_textures;
        nv_codecs::set_model_textures(s.model_textures);
        self.fps = s.fps;
        self.settings = s;
        self.sync_loading();
        if textures_changed || colour_changed {
            self.reload_models();
            if colour_changed && self.mode == Mode::Gallery {
                self.request_thumbs();
            }
        }
        if let Some(e) = colour_error {
            self.toast(format!("Colour settings not applied: {e}"));
        }
        if let Err(e) = self.settings.save() {
            self.toast(format!("Settings not saved: {e}"));
        }
        if !self.settings.top_bar {
            self.top_bar = false;
        }
        if !self.settings.sidebar {
            self.sidebar.hover = false;
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
