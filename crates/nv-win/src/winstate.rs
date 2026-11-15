//! Whether the program is in a window or fullscreen, and where the window is, shared by NuViewer and NuGallery
//! (`window.ini` next to `settings.ini`): switch to a window in one and the other opens as a window too, in the same place.

use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinState {
    pub fullscreen: bool,
    /// The window's rectangle when it is a window: x, y, width, height (outer, screen pixels).
    pub rect: Option<(i32, i32, i32, i32)>,
}

pub fn path() -> PathBuf {
    if let Some(s) = std::env::var_os("NUVIEWER_SETTINGS") {
        // Tests and portable setups point settings somewhere else; the window state goes with them.
        return PathBuf::from(s).with_file_name("window.ini");
    }
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("NuViewer").join("window.ini")
}

pub fn parse(text: &str) -> Option<WinState> {
    let mut fullscreen = None;
    let mut rect = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "fullscreen" => fullscreen = match v.trim() {
                "1" | "true" => Some(true),
                "0" | "false" => Some(false),
                _ => None,
            },
            "rect" => {
                let n: Vec<i32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                if let [x, y, w, h] = n[..] {
                    if w >= 200 && h >= 150 && w <= 20_000 && h <= 20_000 {
                        rect = Some((x, y, w, h));
                    }
                }
            }
            _ => {}
        }
    }
    fullscreen.map(|fullscreen| WinState { fullscreen, rect })
}

pub fn serialize(s: &WinState) -> String {
    let mut out = format!("fullscreen = {}\n", if s.fullscreen { 1 } else { 0 });
    if let Some((x, y, w, h)) = s.rect {
        out += &format!("rect = {x},{y},{w},{h}\n");
    }
    out
}

pub fn load() -> Option<WinState> {
    std::fs::read_to_string(path()).ok().and_then(|t| parse(&t))
}

pub fn save(s: &WinState) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Only when it changed: both programs write this at start-up and on every toggle.
    let text = serialize(s);
    if std::fs::read_to_string(&p).map(|old| old == text).unwrap_or(false) {
        return;
    }
    let _ = std::fs::write(p, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for s in [WinState { fullscreen: true, rect: Some((-1920, 40, 1280, 800)) }, WinState { fullscreen: false, rect: None }] {
            assert_eq!(parse(&serialize(&s)), Some(s));
        }
    }

    #[test]
    fn nonsense_is_ignored() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("fullscreen = maybe"), None);
        let s = parse("fullscreen = 0\nrect = 1,2,3\n").unwrap();
        assert_eq!(s.rect, None, "a rectangle needs four numbers");
        assert_eq!(parse("fullscreen = 0\nrect = 10,10,5,5").unwrap().rect, None, "too small to be a window");
    }
}
