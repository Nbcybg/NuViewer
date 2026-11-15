//! What the gallery remembers between runs (`gallery.ini` next to the viewer's `settings.ini`): the last folder, the
//! view, the sizes, which panels are pinned. A plain `key = value` file like the viewer's; unknown keys are kept out of the way.

use crate::model::Show;
use nv_core::folder::SortKey;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViewMode {
    #[default]
    Thumbs,
    Details,
    List,
}

impl ViewMode {
    pub const ALL: [ViewMode; 3] = [ViewMode::Thumbs, ViewMode::Details, ViewMode::List];

    pub fn id(self) -> &'static str {
        match self {
            ViewMode::Thumbs => "thumbs",
            ViewMode::Details => "details",
            ViewMode::List => "list",
        }
    }

    pub fn from_id(v: &str) -> Option<ViewMode> {
        Self::ALL.into_iter().find(|m| m.id() == v.trim().to_ascii_lowercase())
    }

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Thumbs => "Thumbnails",
            ViewMode::Details => "Details",
            ViewMode::List => "List",
        }
    }
}

/// Thumbnail cell widths for the keys 1..5, in logical pixels; the middle one is the default (`~` returns to it).
pub const CELL_LEVELS: [f32; 5] = [88.0, 128.0, 176.0, 224.0, 264.0];
pub const CELL_DEFAULT: f32 = 176.0;
pub const CELL_MIN: f32 = 72.0;
pub const CELL_MAX: f32 = 264.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Persist {
    pub dir: Option<PathBuf>,
    pub view: ViewMode,
    /// Thumbnail cell width in logical pixels.
    pub cell: f32,
    pub show: Show,
    pub folders: bool,
    pub hidden: bool,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub pin_top: bool,
    pub pin_left: bool,
    pub pin_right: bool,
    pub names: bool,
}

impl Default for Persist {
    fn default() -> Self {
        Persist { dir: None, view: ViewMode::Thumbs, cell: CELL_DEFAULT, show: Show::Media, folders: true, hidden: false, sort: SortKey::Name, sort_desc: false, pin_top: false, pin_left: false, pin_right: false, names: true }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

impl Persist {
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("NUGALLERY_STATE") {
            return PathBuf::from(p);
        }
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        base.join("NuViewer").join("gallery.ini")
    }

    pub fn load() -> Persist {
        std::fs::read_to_string(Self::path()).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.serialize());
    }

    pub fn parse(text: &str) -> Persist {
        let mut p = Persist::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "dir" => p.dir = (!v.is_empty()).then(|| PathBuf::from(v)),
                "view" => p.view = ViewMode::from_id(v).unwrap_or(p.view),
                "cell" => p.cell = v.parse::<f32>().ok().map(|c| c.clamp(CELL_MIN, CELL_MAX)).unwrap_or(p.cell),
                "show" => p.show = Show::from_id(v).unwrap_or(p.show),
                "folders" => p.folders = parse_bool(v).unwrap_or(p.folders),
                "hidden" => p.hidden = parse_bool(v).unwrap_or(p.hidden),
                "sort" => p.sort = SortKey::from_id(v).unwrap_or(p.sort),
                "sort_desc" => p.sort_desc = parse_bool(v).unwrap_or(p.sort_desc),
                "pin_top" => p.pin_top = parse_bool(v).unwrap_or(p.pin_top),
                "pin_left" => p.pin_left = parse_bool(v).unwrap_or(p.pin_left),
                "pin_right" => p.pin_right = parse_bool(v).unwrap_or(p.pin_right),
                "names" => p.names = parse_bool(v).unwrap_or(p.names),
                _ => {}
            }
        }
        p
    }

    pub fn serialize(&self) -> String {
        let b = |v: bool| if v { "1" } else { "0" };
        format!(
            "# NuGallery state (written by the program)\ndir = {}\nview = {}\ncell = {}\nshow = {}\nfolders = {}\nhidden = {}\nsort = {}\nsort_desc = {}\npin_top = {}\npin_left = {}\npin_right = {}\nnames = {}\n",
            self.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default(),
            self.view.id(),
            self.cell,
            self.show.id(),
            b(self.folders),
            b(self.hidden),
            self.sort.id(),
            b(self.sort_desc),
            b(self.pin_top),
            b(self.pin_left),
            b(self.pin_right),
            b(self.names),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let p = Persist { dir: Some(PathBuf::from("D:\\รูป\\2025")), view: ViewMode::Details, cell: 128.0, show: Show::Raw, folders: false, hidden: true, sort: SortKey::Size, sort_desc: true, pin_top: true, pin_left: true, pin_right: false, names: false };
        assert_eq!(Persist::parse(&p.serialize()), p);
    }

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let p = Persist::parse("view = banana\ncell = 5000\nshow = ?\nsort = zzz\nfolders = maybe\nnoise\n= x\n");
        let d = Persist::default();
        assert_eq!((p.view, p.show, p.sort, p.folders), (d.view, d.show, d.sort, d.folders));
        assert_eq!(p.cell, CELL_MAX, "sizes are clamped");
        assert_eq!(Persist::parse(""), d);
    }

    #[test]
    fn levels_are_sensible() {
        assert!(CELL_LEVELS.windows(2).all(|w| w[0] < w[1]));
        assert!(CELL_LEVELS[0] >= CELL_MIN && CELL_LEVELS[4] <= CELL_MAX);
        assert_eq!(CELL_LEVELS[2], CELL_DEFAULT);
    }
}
