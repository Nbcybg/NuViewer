//! User settings, stored as a small `key = value` text file (`%APPDATA%\NuViewer\settings.ini`; the environment
//! variable `NUVIEWER_SETTINGS` points somewhere else, which is what the tests use). Unknown keys are kept out of the
//! way, bad values fall back to the defaults, so an old or hand-edited file never stops the viewer from starting.

use crate::folder::SortKey;
use nv_codecs::model::{LightMode, ModelStyle, Shading, UpAxis};
use std::collections::HashMap;
use std::path::PathBuf;

/// What happens to a rotated picture when the user moves on (next / previous picture, gallery, close).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateSave {
    /// Ask every time: save, discard or stay.
    Ask,
    /// Save without asking.
    Auto,
}

/// Where a colour profile comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileChoice {
    Srgb,
    DisplayP3,
    AdobeRgb,
    /// The profile Windows has for the display (output only).
    System,
    /// An `.icc` / `.icm` file the user picked.
    File,
}

impl ProfileChoice {
    pub fn id(self) -> &'static str {
        match self {
            ProfileChoice::Srgb => "srgb",
            ProfileChoice::DisplayP3 => "p3",
            ProfileChoice::AdobeRgb => "adobe",
            ProfileChoice::System => "system",
            ProfileChoice::File => "file",
        }
    }

    pub fn from_id(v: &str) -> Option<ProfileChoice> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "srgb" => ProfileChoice::Srgb,
            "p3" | "displayp3" => ProfileChoice::DisplayP3,
            "adobe" | "adobergb" => ProfileChoice::AdobeRgb,
            "system" => ProfileChoice::System,
            "file" => ProfileChoice::File,
            _ => return None,
        })
    }
}

/// What the mouse wheel does in the viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelAction {
    /// Previous / next picture (or page, or frame).
    Pages,
    Zoom,
    Off,
}

/// What holding the left button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoldZoom {
    /// 100%, or 200% for a picture smaller than `small_pct` of the screen.
    Auto,
    X100,
    X200,
    X400,
    Off,
}

/// A program the current file can be sent to. `command` is a command line; `{file}` is replaced by the path
/// (if it is missing the quoted path is appended).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct External {
    pub name: String,
    pub command: String,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    // Viewer
    /// Background around the picture, 0xRRGGBB.
    pub canvas: u32,
    pub start_fullscreen: bool,
    /// Colour management on: pictures are converted from their own profile to the output profile (off = show the
    /// numbers as they are).
    pub icc: bool,
    /// The profile of what the pictures are shown on: sRGB, Windows' display profile, or a file (`icc_output_file`).
    pub icc_output: ProfileChoice,
    pub icc_output_file: String,
    /// What a JPEG / PNG / WebP without a profile is taken to be: sRGB, Display P3, Adobe RGB or a file.
    pub icc_untagged: ProfileChoice,
    pub icc_untagged_file: String,
    pub hold_zoom: HoldZoom,
    /// "Small picture" threshold for `HoldZoom::Auto`, percent of the screen in both directions.
    pub small_pct: u32,
    // Mouse
    pub wheel: WheelAction,
    /// The viewer controls (a vertical bar) appear when the pointer touches the left edge.
    pub sidebar: bool,
    pub top_bar: bool,
    pub taskbar_edge: bool,
    // Image sequences and playback
    /// Fold runs of numbered files into one gallery entry.
    pub sequences: bool,
    pub sequence_min: u32,
    pub fps: u32,
    /// Keep the playback controls on screen instead of only near the bottom edge.
    pub controls_always: bool,
    /// Show the disk's read / write speed on screen all the time (it always appears while a picture is loading).
    pub disk_monitor: bool,
    /// Load the textures of 3D models (OBJ + MTL, glTF, FBX, PLY); off shows the plain grey surface.
    pub model_textures: bool,
    /// Rotating a picture: save it when moving on, or ask first.
    pub rotate_save: RotateSave,
    /// How the pictures of a folder are ordered (next / previous, gallery, film strip), and whether that is reversed.
    pub sort: SortKey,
    pub sort_desc: bool,
    /// The film strip appears when the pointer touches the right edge of the window.
    pub film_strip: bool,
    /// Read mode (two pages side by side without a gap): right-to-left books (manga) put the first page on the right.
    pub read_rtl: bool,
    /// Read mode: the page (1-based) the first spread starts at. 2 = the cover stands alone and the pairs are 2+3, 4+5 ...;
    /// 1 = the pairs are 1+2, 3+4 ...
    pub read_start: u32,
    /// How 3D models are shaded, lit and projected.
    pub model: ModelStyle,
    pub external: Vec<External>,
    /// Keyboard overrides: action id -> "Key, Key" (see `keys`).
    pub keys: HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            canvas: 0x1A1A1A,
            start_fullscreen: true,
            icc: true,
            icc_output: ProfileChoice::Srgb,
            icc_output_file: String::new(),
            icc_untagged: ProfileChoice::Srgb,
            icc_untagged_file: String::new(),
            hold_zoom: HoldZoom::Auto,
            small_pct: 50,
            wheel: WheelAction::Pages,
            sidebar: true,
            top_bar: true,
            taskbar_edge: true,
            sequences: false,
            sequence_min: 10,
            fps: 24,
            controls_always: false,
            disk_monitor: false,
            model_textures: true,
            rotate_save: RotateSave::Ask,
            sort: SortKey::Name,
            sort_desc: false,
            film_strip: true,
            read_rtl: false,
            read_start: 2,
            model: ModelStyle::default(),
            external: Vec::new(),
            keys: HashMap::new(),
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Settings {
    /// Where the file lives.
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("NUVIEWER_SETTINGS") {
            return PathBuf::from(p);
        }
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        base.join("NuViewer").join("settings.ini")
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(Self::path()).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        }
        std::fs::write(&path, self.serialize()).map_err(|e| format!("can't write {}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut external: Vec<(usize, Option<String>, Option<String>)> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "canvas" => {
                    if let Ok(c) = u32::from_str_radix(v.trim_start_matches('#'), 16) {
                        s.canvas = c & 0xFF_FFFF;
                    }
                }
                "start_fullscreen" => s.start_fullscreen = parse_bool(v).unwrap_or(s.start_fullscreen),
                "icc" => s.icc = parse_bool(v).unwrap_or(s.icc),
                "icc_output" => s.icc_output = ProfileChoice::from_id(v).unwrap_or(s.icc_output),
                "icc_output_file" => s.icc_output_file = v.to_string(),
                "icc_untagged" => s.icc_untagged = ProfileChoice::from_id(v).filter(|c| *c != ProfileChoice::System).unwrap_or(s.icc_untagged),
                "icc_untagged_file" => s.icc_untagged_file = v.to_string(),
                "hold_zoom" => {
                    s.hold_zoom = match v.to_ascii_lowercase().as_str() {
                        "auto" => HoldZoom::Auto,
                        "100" => HoldZoom::X100,
                        "200" => HoldZoom::X200,
                        "400" => HoldZoom::X400,
                        "off" => HoldZoom::Off,
                        _ => s.hold_zoom,
                    }
                }
                "small_pct" => s.small_pct = v.parse().ok().filter(|p| (5..=100).contains(p)).unwrap_or(s.small_pct),
                "wheel" => {
                    s.wheel = match v.to_ascii_lowercase().as_str() {
                        "pages" => WheelAction::Pages,
                        "zoom" => WheelAction::Zoom,
                        "off" => WheelAction::Off,
                        _ => s.wheel,
                    }
                }
                "sidebar" => s.sidebar = parse_bool(v).unwrap_or(s.sidebar),
                "top_bar" => s.top_bar = parse_bool(v).unwrap_or(s.top_bar),
                "taskbar_edge" => s.taskbar_edge = parse_bool(v).unwrap_or(s.taskbar_edge),
                "sequences" => s.sequences = parse_bool(v).unwrap_or(s.sequences),
                "sequence_min" => s.sequence_min = v.parse().ok().filter(|p| (2..=10_000).contains(p)).unwrap_or(s.sequence_min),
                "fps" => s.fps = v.parse().ok().filter(|p| (1..=240).contains(p)).unwrap_or(s.fps),
                "controls_always" => s.controls_always = parse_bool(v).unwrap_or(s.controls_always),
                "disk_monitor" => s.disk_monitor = parse_bool(v).unwrap_or(s.disk_monitor),
                "model_textures" => s.model_textures = parse_bool(v).unwrap_or(s.model_textures),
                "rotate_save" => {
                    s.rotate_save = match v.to_ascii_lowercase().as_str() {
                        "ask" => RotateSave::Ask,
                        "auto" => RotateSave::Auto,
                        _ => s.rotate_save,
                    }
                }
                "sort" => s.sort = SortKey::from_id(v).unwrap_or(s.sort),
                "sort_desc" => s.sort_desc = parse_bool(v).unwrap_or(s.sort_desc),
                "film_strip" => s.film_strip = parse_bool(v).unwrap_or(s.film_strip),
                "read_rtl" => s.read_rtl = parse_bool(v).unwrap_or(s.read_rtl),
                "read_start" => s.read_start = v.parse().ok().filter(|p| (1..=9999).contains(p)).unwrap_or(s.read_start),
                "model_shading" => s.model.shading = Shading::from_id(v).unwrap_or(s.model.shading),
                "model_light" => {
                    s.model.light = match v.to_ascii_lowercase().as_str() {
                        "viewer" => LightMode::Viewer,
                        "world" => LightMode::World,
                        _ => s.model.light,
                    }
                }
                "model_light_az" => s.model.light_az = v.parse::<f32>().ok().filter(|a| (-180.0..=180.0).contains(a)).unwrap_or(s.model.light_az),
                "model_light_el" => s.model.light_el = v.parse::<f32>().ok().filter(|a| (-90.0..=90.0).contains(a)).unwrap_or(s.model.light_el),
                "model_ortho" => s.model.ortho = parse_bool(v).unwrap_or(s.model.ortho),
                "model_up" => {
                    s.model.up = match v.to_ascii_lowercase().as_str() {
                        "auto" => UpAxis::Auto,
                        "y" => UpAxis::Y,
                        "z" => UpAxis::Z,
                        _ => s.model.up,
                    }
                }
                "model_turn_x" => s.model.turns[0] = v.parse::<u8>().ok().filter(|t| *t < 4).unwrap_or(s.model.turns[0]),
                "model_turn_y" => s.model.turns[1] = v.parse::<u8>().ok().filter(|t| *t < 4).unwrap_or(s.model.turns[1]),
                "model_turn_z" => s.model.turns[2] = v.parse::<u8>().ok().filter(|t| *t < 4).unwrap_or(s.model.turns[2]),
                "model_texture" => s.model.texture = parse_bool(v).unwrap_or(s.model.texture),
                "model_normal_map" => s.model.normal_map = parse_bool(v).unwrap_or(s.model.normal_map),
                "model_wireframe" => s.model.wireframe = parse_bool(v).unwrap_or(s.model.wireframe),
                "model_shadow" => s.model.shadow = parse_bool(v).unwrap_or(s.model.shadow),
                _ => {
                    if let Some(rest) = k.strip_prefix("key.") {
                        s.keys.insert(rest.to_string(), v.to_string());
                    } else if let Some(rest) = k.strip_prefix("external.") {
                        let Some((n, field)) = rest.split_once('.') else { continue };
                        let Ok(n) = n.parse::<usize>() else { continue };
                        if external.iter().all(|e| e.0 != n) {
                            external.push((n, None, None));
                        }
                        let e = external.iter_mut().find(|e| e.0 == n).unwrap();
                        match field {
                            "name" => e.1 = Some(v.to_string()),
                            "command" => e.2 = Some(v.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }
        external.sort_by_key(|e| e.0);
        s.external = external.into_iter().filter_map(|(_, n, c)| Some(External { name: n?, command: c? })).filter(|e| !e.name.is_empty() && !e.command.is_empty()).collect();
        s
    }

    pub fn serialize(&self) -> String {
        let b = |v: bool| if v { "true" } else { "false" };
        let mut out = String::from("# NuViewer settings. Edit here or in the Settings window (Ctrl+,).\n");
        out += &format!("canvas = {:06X}\nstart_fullscreen = {}\nicc = {}\n", self.canvas, b(self.start_fullscreen), b(self.icc));
        out += &format!("icc_output = {}\nicc_output_file = {}\nicc_untagged = {}\nicc_untagged_file = {}\n", self.icc_output.id(), self.icc_output_file, self.icc_untagged.id(), self.icc_untagged_file);
        out += &format!(
            "hold_zoom = {}\nsmall_pct = {}\n",
            match self.hold_zoom {
                HoldZoom::Auto => "auto",
                HoldZoom::X100 => "100",
                HoldZoom::X200 => "200",
                HoldZoom::X400 => "400",
                HoldZoom::Off => "off",
            },
            self.small_pct
        );
        out += &format!(
            "wheel = {}\nsidebar = {}\ntop_bar = {}\ntaskbar_edge = {}\n",
            match self.wheel {
                WheelAction::Pages => "pages",
                WheelAction::Zoom => "zoom",
                WheelAction::Off => "off",
            },
            b(self.sidebar),
            b(self.top_bar),
            b(self.taskbar_edge)
        );
        out += &format!("sequences = {}\nsequence_min = {}\nfps = {}\ncontrols_always = {}\n", b(self.sequences), self.sequence_min, self.fps, b(self.controls_always));
        out += &format!("disk_monitor = {}\nmodel_textures = {}\n", b(self.disk_monitor), b(self.model_textures));
        out += &format!(
            "rotate_save = {}\n",
            match self.rotate_save {
                RotateSave::Ask => "ask",
                RotateSave::Auto => "auto",
            }
        );
        out += &format!("sort = {}\nsort_desc = {}\nfilm_strip = {}\nread_rtl = {}\nread_start = {}\n", self.sort.id(), b(self.sort_desc), b(self.film_strip), b(self.read_rtl), self.read_start);
        let m = &self.model;
        out += &format!(
            "model_shading = {}\nmodel_light = {}\nmodel_light_az = {}\nmodel_light_el = {}\nmodel_ortho = {}\nmodel_up = {}\n",
            m.shading.id(),
            match m.light {
                LightMode::Viewer => "viewer",
                LightMode::World => "world",
            },
            m.light_az,
            m.light_el,
            b(m.ortho),
            match m.up {
                UpAxis::Auto => "auto",
                UpAxis::Y => "y",
                UpAxis::Z => "z",
            }
        );
        out += &format!(
            "model_turn_x = {}\nmodel_turn_y = {}\nmodel_turn_z = {}\nmodel_texture = {}\nmodel_normal_map = {}\nmodel_wireframe = {}\nmodel_shadow = {}\n",
            m.turns[0],
            m.turns[1],
            m.turns[2],
            b(m.texture),
            b(m.normal_map),
            b(m.wireframe),
            b(m.shadow)
        );
        for (i, e) in self.external.iter().enumerate() {
            out += &format!("external.{i}.name = {}\nexternal.{i}.command = {}\n", e.name, e.command);
        }
        let mut keys: Vec<_> = self.keys.iter().collect();
        keys.sort();
        for (k, v) in keys {
            out += &format!("key.{k} = {v}\n");
        }
        out
    }

    /// Zoom factor for the hold-left-button gesture on a picture of `img` pixels shown on a `screen`-pixel monitor
    /// (None = the gesture is switched off).
    pub fn hold_scale(&self, img: (u32, u32), screen: (u32, u32)) -> Option<f32> {
        match self.hold_zoom {
            HoldZoom::Off => None,
            HoldZoom::X100 => Some(1.0),
            HoldZoom::X200 => Some(2.0),
            HoldZoom::X400 => Some(4.0),
            HoldZoom::Auto => {
                let small = (img.0 as u64) * 100 <= screen.0 as u64 * self.small_pct as u64 && (img.1 as u64) * 100 <= screen.1 as u64 * self.small_pct as u64;
                Some(if small { 2.0 } else { 1.0 })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_the_file_format() {
        let d = Settings::default();
        assert_eq!(Settings::parse(&d.serialize()), d);
    }

    #[test]
    fn everything_round_trips() {
        let mut s = Settings::default();
        s.canvas = 0x203040;
        s.start_fullscreen = false;
        s.icc = false;
        s.icc_output = ProfileChoice::File;
        s.icc_output_file = r"D:\Colour\My Monitor = calibrated.icc".into();
        s.icc_untagged = ProfileChoice::DisplayP3;
        s.hold_zoom = HoldZoom::X400;
        s.small_pct = 30;
        s.wheel = WheelAction::Zoom;
        s.sidebar = false;
        s.rotate_save = RotateSave::Auto;
        s.sort = SortKey::Modified;
        s.sort_desc = true;
        s.film_strip = false;
        s.read_rtl = true;
        s.read_start = 5;
        s.model = ModelStyle {
            shading: Shading::Cartoon,
            light: LightMode::World,
            light_az: 40.0,
            light_el: -10.5,
            ortho: true,
            up: UpAxis::Z,
            turns: [1, 2, 3],
            texture: false,
            normal_map: false,
            wireframe: true,
            shadow: true,
        };
        s.top_bar = false;
        s.taskbar_edge = false;
        s.sequences = true;
        s.sequence_min = 4;
        s.fps = 30;
        s.controls_always = true;
        s.disk_monitor = true;
        s.model_textures = false;
        s.external = vec![
            External { name: "Photoshop".into(), command: "\"C:\\Program Files\\Adobe\\ps.exe\" \"{file}\"".into() },
            External { name: "Paint".into(), command: "mspaint.exe".into() },
        ];
        s.keys.insert("next".into(), "N, Ctrl+,".into());
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn bad_values_and_unknown_keys_are_ignored() {
        let t = "canvas = zzz\nhold_zoom = banana\nsmall_pct = 5000\nfps = 0\nwheel = pages\nmystery = 1\nno equals sign\nsequence_min = 7\n# comment\n";
        let s = Settings::parse(t);
        let d = Settings::default();
        assert_eq!((s.canvas, s.hold_zoom, s.small_pct, s.fps, s.wheel), (d.canvas, d.hold_zoom, d.small_pct, d.fps, d.wheel));
        assert_eq!(s.sequence_min, 7, "valid values next to bad ones still apply");
        assert_eq!(Settings::parse(""), d);
    }

    #[test]
    fn external_entries_need_both_fields_and_keep_their_order() {
        let t = "external.1.name = B\nexternal.1.command = b.exe\nexternal.0.name = A\nexternal.0.command = a.exe\nexternal.2.name = NoCommand\n";
        let s = Settings::parse(t);
        assert_eq!(s.external.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }

    #[test]
    fn hold_zoom_levels() {
        let s = Settings::default();
        let screen = (2560, 1440);
        assert_eq!(s.hold_scale((1000, 600), screen), Some(2.0), "small picture doubles");
        assert_eq!(s.hold_scale((1281, 600), screen), Some(1.0), "just over half the width");
        assert_eq!(s.hold_scale((4000, 2000), screen), Some(1.0));
        let mut s2 = s.clone();
        s2.small_pct = 20;
        assert_eq!(s2.hold_scale((1000, 600), screen), Some(1.0));
        s2.hold_zoom = HoldZoom::X400;
        assert_eq!(s2.hold_scale((1000, 600), screen), Some(4.0));
        s2.hold_zoom = HoldZoom::Off;
        assert_eq!(s2.hold_scale((1000, 600), screen), None);
    }

    #[test]
    fn save_and_load_use_the_override_path() {
        let dir = std::env::temp_dir().join(format!("nv_settings_test_{}", std::process::id()));
        let file = dir.join("sub").join("s.ini");
        // This test owns the variable for its duration; nothing else in nv-core reads it.
        std::env::set_var("NUVIEWER_SETTINGS", &file);
        let mut s = Settings::default();
        s.fps = 12;
        s.save().unwrap();
        assert_eq!(Settings::load().fps, 12);
        std::env::remove_var("NUVIEWER_SETTINGS");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
