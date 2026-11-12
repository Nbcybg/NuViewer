//! Keyboard shortcuts: the list of things the viewer can do (`Action`), key chords ("Ctrl+Shift+C"), and the
//! `Keymap` that turns a pressed key into an action. Defaults reproduce the built-in keys; the settings file
//! (see `settings`) only stores what the user changed. Platform independent: keys are Windows virtual-key codes.

use std::collections::HashMap;

/// A key plus the modifiers that must be held (exactly these, no more).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Chord {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Key names accepted and produced (the first name of a code is the canonical one).
const NAMES: &[(&str, u32)] = &[
    ("Backspace", 0x08),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Esc", 0x1B),
    ("Space", 0x20),
    ("PgUp", 0x21),
    ("PgDn", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Num+", 0x6B),
    ("Num-", 0x6D),
    ("Num*", 0x6A),
    ("Num/", 0x6F),
    ("=", 0xBB),
    ("+", 0xBB), // the key that types "=" and "+"
    (",", 0xBC),
    ("-", 0xBD),
    (".", 0xBE),
    ("/", 0xBF),
    ("[", 0xDB),
    ("]", 0xDD),
    // aliases
    ("Escape", 0x1B),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Del", 0x2E),
    ("Plus", 0xBB),
    ("Minus", 0xBD),
    ("Comma", 0xBC),
    ("Period", 0xBE),
    ("Return", 0x0D),
];

/// Names for the first entries above that must not come out as aliases when formatting.
const CANONICAL_END: usize = 29;

fn key_name(vk: u32) -> String {
    if (0x30..=0x39).contains(&vk) || (0x41..=0x5A).contains(&vk) {
        return (vk as u8 as char).to_string();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x6F);
    }
    // "+" shares its code with "=": show the unshifted symbol.
    if vk == 0xBB {
        return "+".into();
    }
    NAMES[..CANONICAL_END].iter().find(|(_, c)| *c == vk).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("0x{vk:02X}"))
}

fn key_code(name: &str) -> Option<u32> {
    let n = name.trim();
    if n.chars().count() == 1 {
        let c = n.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(num) = n.strip_prefix(['F', 'f']).and_then(|d| d.parse::<u32>().ok()).filter(|d| (1..=24).contains(d)) {
        return Some(0x6F + num);
    }
    NAMES.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, c)| *c)
}

impl Chord {
    pub const fn new(vk: u32) -> Chord {
        Chord { vk, ctrl: false, shift: false, alt: false }
    }

    /// "Ctrl+Shift+C", "Right", "F11", "Alt+Enter", "Num+", "Ctrl++". Case-insensitive, modifiers in any order.
    /// Modifier prefixes are peeled off one at a time; whatever is left is the key name (which may itself be "+").
    pub fn parse(text: &str) -> Option<Chord> {
        let mut c = Chord::new(0);
        let mut rest = text.trim();
        loop {
            let lower = rest.to_ascii_lowercase();
            let (flag, len) = if lower.starts_with("ctrl+") {
                (&mut c.ctrl, 5)
            } else if lower.starts_with("control+") {
                (&mut c.ctrl, 8)
            } else if lower.starts_with("shift+") {
                (&mut c.shift, 6)
            } else if lower.starts_with("alt+") {
                (&mut c.alt, 4)
            } else {
                break;
            };
            *flag = true;
            rest = &rest[len..];
        }
        c.vk = key_code(rest)?;
        Some(c)
    }

    pub fn format(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.shift {
            s += "Shift+";
        }
        if self.alt {
            s += "Alt+";
        }
        s + &key_name(self.vk)
    }
}

/// Parse "Right, Space, PgDn" into chords. Returns an error naming the first word that isn't a key.
pub fn parse_chords(text: &str) -> Result<Vec<Chord>, String> {
    let mut out = Vec::new();
    // Split on commas, but a lone "," (the comma key) written as "Ctrl+," or "Comma" survives: split on ", " / "," followed by space or end.
    for piece in text.split(';').flat_map(|p| split_list(p)) {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        out.push(Chord::parse(piece).ok_or_else(|| format!("\"{piece}\" is not a key"))?);
    }
    Ok(out)
}

/// Split "A, B, Ctrl+," into ["A", "B", "Ctrl+,"]: a comma that is the key itself is not a separator.
fn split_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        let key_comma = ch == ',' && (cur.is_empty() || cur.ends_with('+')) && (i + 1 == chars.len() || chars[i + 1] == ',' || chars[i + 1] == ' ' || cur.ends_with('+'));
        if ch == ',' && !key_comma {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

pub fn format_chords(chords: &[Chord]) -> String {
    chords.iter().map(|c| c.format()).collect::<Vec<_>>().join(", ")
}

/// Everything a key can be bound to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Next,
    Prev,
    PageNext,
    PagePrev,
    First,
    Last,
    NextFile,
    PrevFile,
    FirstFile,
    LastFile,
    Gallery,
    FitReset,
    ActualSize,
    Fullscreen,
    Slideshow,
    Shuffle,
    SpeedUp,
    SpeedDown,
    PlayPause,
    ToggleSequences,
    ToggleControls,
    DiskMonitor,
    FlipUp,
    Info,
    Help,
    Rename,
    CopyTo,
    MoveTo,
    CopyImage,
    CopyPath,
    Reveal,
    Delete,
    DeletePermanently,
    Undo,
    Settings,
    Back,
    Quit,
}

pub struct ActionInfo {
    pub action: Action,
    /// Stable name used in the settings file.
    pub id: &'static str,
    pub label: &'static str,
    /// Built-in keys, in `Chord::parse` syntax.
    pub defaults: &'static [&'static str],
}

macro_rules! a {
    ($action:ident, $id:expr, $label:expr, [$($k:expr),*]) => {
        ActionInfo { action: Action::$action, id: $id, label: $label, defaults: &[$($k),*] }
    };
}

pub const ACTIONS: &[ActionInfo] = &[
    a!(Next, "next", "Next picture / frame / page", ["Right", "Space"]),
    a!(Prev, "prev", "Previous picture / frame / page", ["Left", "Backspace"]),
    a!(PageNext, "page_next", "Page down (next; gallery: one screen)", ["PgDn"]),
    a!(PagePrev, "page_prev", "Page up (previous; gallery: one screen)", ["PgUp"]),
    a!(First, "first", "First page (documents) or first picture", ["Home"]),
    a!(Last, "last", "Last page (documents) or last picture", ["End"]),
    a!(NextFile, "next_file", "Next file (skips document pages)", ["Ctrl+Right"]),
    a!(PrevFile, "prev_file", "Previous file (skips document pages)", ["Ctrl+Left"]),
    a!(FirstFile, "first_file", "First file", ["Ctrl+Home"]),
    a!(LastFile, "last_file", "Last file", ["Ctrl+End"]),
    a!(Gallery, "gallery", "Gallery on / off", ["G", "Tab", "Enter"]),
    a!(FitReset, "fit", "Fit to screen (3D: reset camera)", ["F"]),
    a!(ActualSize, "actual_size", "Actual size 100% (3D: reset camera)", ["1"]),
    a!(Fullscreen, "fullscreen", "Fullscreen / window", ["F11", "Alt+Enter"]),
    a!(Slideshow, "slideshow", "Slideshow on / off", ["S"]),
    a!(Shuffle, "shuffle", "Slideshow shuffle", ["R"]),
    a!(SpeedUp, "speed_up", "Slideshow / playback faster", ["+", "Num+"]),
    a!(SpeedDown, "speed_down", "Slideshow / playback slower", ["-", "Num-"]),
    a!(PlayPause, "play_pause", "Play / pause animation or sequence", ["P"]),
    a!(ToggleSequences, "sequences", "Fold image sequences / show every picture", ["Q"]),
    a!(ToggleControls, "controls", "Keep the playback controls visible", ["C"]),
    a!(DiskMonitor, "disk_monitor", "Disk read / write speed on screen", ["D"]),
    a!(FlipUp, "flip_up", "3D: other axis is up", ["U"]),
    a!(Info, "info", "Info / EXIF panel", ["I"]),
    a!(Help, "help", "Help", ["F1", "H"]),
    a!(Rename, "rename", "Rename", ["F2"]),
    a!(CopyTo, "copy_to", "Copy to folder", ["F5"]),
    a!(MoveTo, "move_to", "Move to folder", ["F6", "M"]),
    a!(CopyImage, "copy_image", "Copy picture + file to the clipboard", ["Ctrl+C"]),
    a!(CopyPath, "copy_path", "Copy file path", ["Ctrl+Shift+C"]),
    a!(Reveal, "reveal", "Show in Explorer", ["Ctrl+E"]),
    a!(Delete, "delete", "Delete (Recycle Bin)", ["Delete"]),
    a!(DeletePermanently, "delete_permanently", "Delete permanently", ["Shift+Delete"]),
    a!(Undo, "undo", "Undo", ["Ctrl+Z"]),
    a!(Settings, "settings", "Settings", ["Ctrl+,", "F9"]),
    a!(Back, "back", "Back / close panel / stop (Esc)", ["Esc"]),
    a!(Quit, "quit", "Close the viewer", ["Ctrl+Q"]),
];

pub fn info(action: Action) -> &'static ActionInfo {
    ACTIONS.iter().find(|i| i.action == action).expect("every action is listed")
}

/// Active bindings: defaults, with the user's overrides (action id -> chord list) replacing them per action.
pub struct Keymap {
    map: HashMap<Chord, Action>,
    /// Bindings in definition order (the first key of an action is the one shown in menus).
    ordered: Vec<(Chord, Action)>,
}

impl Keymap {
    pub fn new(overrides: &HashMap<String, String>) -> Keymap {
        let mut map = HashMap::new();
        let mut ordered = Vec::new();
        for info in ACTIONS {
            let chords: Vec<Chord> = match overrides.get(info.id) {
                Some(text) => parse_chords(text).unwrap_or_else(|_| default_chords(info)),
                None => default_chords(info),
            };
            for c in chords {
                if !map.contains_key(&c) {
                    map.insert(c, info.action);
                    ordered.push((c, info.action));
                }
            }
        }
        Keymap { map, ordered }
    }

    pub fn lookup(&self, vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Action> {
        self.map.get(&Chord { vk, ctrl, shift, alt }).copied()
    }

    /// The chords bound to `action` right now, for display.
    pub fn chords_of(&self, action: Action) -> Vec<Chord> {
        self.ordered.iter().filter(|(_, a)| *a == action).map(|(c, _)| *c).collect()
    }
}

pub fn default_chords(info: &ActionInfo) -> Vec<Chord> {
    info.defaults.iter().filter_map(|d| Chord::parse(d)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_and_format() {
        let c = Chord::parse("ctrl+shift+c").unwrap();
        assert_eq!((c.vk, c.ctrl, c.shift, c.alt), (0x43, true, true, false));
        assert_eq!(c.format(), "Ctrl+Shift+C");
        assert_eq!(Chord::parse("Alt+Enter").unwrap().format(), "Alt+Enter");
        assert_eq!(Chord::parse("f11").unwrap().vk, 0x7A);
        assert_eq!(Chord::parse("Ctrl+,").unwrap().vk, 0xBC);
        assert_eq!(Chord::parse("+").unwrap().format(), "+");
        assert_eq!(Chord::parse("Ctrl++").unwrap().format(), "Ctrl++");
        assert_eq!(Chord::parse("PageDown").unwrap().format(), "PgDn");
        assert!(Chord::parse("Hyper+X").is_none());
        assert!(Chord::parse("Ctrl+Banana").is_none());
        assert!(Chord::parse("").is_none());
    }

    #[test]
    fn every_default_parses_and_round_trips() {
        for info in ACTIONS {
            assert!(!info.defaults.is_empty(), "{} has no default", info.id);
            for d in info.defaults {
                let c = Chord::parse(d).unwrap_or_else(|| panic!("default {d:?} of {} doesn't parse", info.id));
                assert_eq!(Chord::parse(&c.format()), Some(c), "{d}");
            }
        }
    }

    #[test]
    fn defaults_do_not_clash() {
        let mut seen: HashMap<Chord, &str> = HashMap::new();
        for info in ACTIONS {
            for c in default_chords(info) {
                if let Some(other) = seen.insert(c, info.id) {
                    panic!("{} is bound to both {other} and {}", c.format(), info.id);
                }
            }
        }
    }

    #[test]
    fn lists_with_a_comma_key() {
        let v = parse_chords("Right, Space, Ctrl+,").unwrap();
        assert_eq!(format_chords(&v), "Right, Space, Ctrl+,");
        assert_eq!(parse_chords("").unwrap().len(), 0);
        assert!(parse_chords("Right, Nope").unwrap_err().contains("Nope"));
    }

    #[test]
    fn overrides_replace_the_defaults_of_that_action_only() {
        let km = Keymap::new(&HashMap::new());
        assert_eq!(km.lookup(0x27, false, false, false), Some(Action::Next));
        let mut o = HashMap::new();
        o.insert("next".to_string(), "N".to_string());
        let km = Keymap::new(&o);
        assert_eq!(km.lookup(0x4E, false, false, false), Some(Action::Next));
        assert_eq!(km.lookup(0x27, false, false, false), None, "Right no longer does anything");
        assert_eq!(km.lookup(0x25, false, false, false), Some(Action::Prev), "other actions keep their keys");
        assert_eq!(km.lookup(0x27, true, false, false), Some(Action::NextFile));
        // A broken override falls back to the defaults instead of leaving the action unreachable.
        o.insert("next".to_string(), "Nonsense".to_string());
        assert_eq!(Keymap::new(&o).lookup(0x27, false, false, false), Some(Action::Next));
    }
}
