//! Skins: the colours of the program's own interface (menus, bars, panels, the gallery). Eleven are built in; the one in use
//! is a name in `settings.ini` (`skin = nu-dark`) and every program that loads the settings starts with it.
//!
//! The names are our own words for a mood (a warm yellow-red-white one is "Marigold"), never the name of a company. A skin
//! is plain data: twelve sRGB colours. Nothing else knows what a skin looks like, so a new one is one more line in `SKINS`
//! (and the test below tells you if its text would be hard to read).

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct Skin {
    /// The word in the settings file.
    pub id: &'static str,
    /// What the menu and Settings call it.
    pub name: &'static str,
    /// A light skin (dark text on pale panels).
    pub dark: bool,
    /// Behind everything (gallery background, bars).
    pub bg: u32,
    /// Panels, menus, the strip.
    pub panel: u32,
    /// Thin lines between things.
    pub edge: u32,
    /// A tile or a button at rest.
    pub tile: u32,
    /// A tile, a button or a menu line under the pointer.
    pub hover: u32,
    pub text: u32,
    /// Hints, shortcuts, disabled lines.
    pub dim: u32,
    /// The one colour that says "this one": selection, the keyboard highlight, a switch that is on.
    pub accent: u32,
    /// A text box.
    pub field: u32,
    pub error: u32,
    pub folder: u32,
    pub star: u32,
}

macro_rules! skin {
    ($id:expr, $name:expr, $dark:expr, [$bg:expr, $panel:expr, $edge:expr, $tile:expr, $hover:expr, $text:expr, $dim:expr, $accent:expr, $field:expr, $error:expr, $folder:expr, $star:expr]) => {
        Skin { id: $id, name: $name, dark: $dark, bg: $bg, panel: $panel, edge: $edge, tile: $tile, hover: $hover, text: $text, dim: $dim, accent: $accent, field: $field, error: $error, folder: $folder, star: $star }
    };
}

pub const SKINS: &[Skin] = &[
    //                                  bg        panel     edge      tile      hover     text      dim       accent    field     error     folder    star
    skin!("nu-dark", "Nu Dark", true, [0x1A1A1A, 0x202225, 0x34373C, 0x2A2C30, 0x34373C, 0xF0F0F0, 0x9AA0A6, 0x3D8BFF, 0x15171A, 0xFF7B7B, 0xE8B84A, 0xFFC83D]),
    skin!("nu-light", "Nu Light", false, [0xE9EBEE, 0xF7F8FA, 0xCDD1D8, 0xFFFFFF, 0xE1E6EE, 0x1B1D21, 0x5F6672, 0x1F6FEB, 0xFFFFFF, 0xC62828, 0xB7791F, 0xE0A100]),
    // yellow, white, red
    skin!("marigold", "Marigold", false, [0xFFF1B8, 0xFFFBE6, 0xE8C84A, 0xFFFFFF, 0xFFE680, 0x231A0A, 0x6E5F2E, 0xD4101C, 0xFFFFFF, 0xB00020, 0xC99400, 0xCC8400]),
    // red, white, black
    skin!("rouge", "Rouge", true, [0x0A0A0A, 0x141414, 0x2E2E2E, 0x1E1E1E, 0x2B2B2B, 0xFFFFFF, 0xA8A8A8, 0xE30613, 0x0E0E0E, 0xFF6B6B, 0xE4E4E4, 0xFF4B55]),
    // green, white, red
    skin!("verdant", "Verdant", false, [0xE4F0E7, 0xF7FBF7, 0xB5D3BC, 0xFFFFFF, 0xD2EAD8, 0x10261A, 0x4A6653, 0x0A7A36, 0xFFFFFF, 0xD6182B, 0x2E9E5B, 0xE6A700]),
    // blue, grey, white
    skin!("azure", "Azure", false, [0xE3E3E3, 0xF3F3F3, 0xCFCFCF, 0xFFFFFF, 0xD3E6F6, 0x1A1A1A, 0x5C5C5C, 0x0067C0, 0xFFFFFF, 0xC42B1C, 0xD69300, 0xD69300]),
    // yellow, white, black
    skin!("bumblebee", "Bumblebee", true, [0x0D0D0D, 0x181818, 0x3A382A, 0x222222, 0x2E2B14, 0xF7F7F7, 0xB4B09A, 0xFFD400, 0x101010, 0xFF6E6E, 0xFFD400, 0xFFD400]),
    // a bag of sugar-shelled sweets on chocolate: red, yellow, blue, green, orange
    skin!("candy", "Candy Shell", true, [0x24150F, 0x2F1C14, 0x5A3A2A, 0x3A241A, 0x4A2F22, 0xFFF4E6, 0xC9A98F, 0xFFB000, 0x1D100B, 0xFF5252, 0x4FC3F7, 0xFF8A1F]),
    // a deep plum with four bright pens
    skin!("aubergine", "Aubergine", true, [0x2C0B2E, 0x3F0E40, 0x5C2A5E, 0x4A154B, 0x611F69, 0xFFFFFF, 0xCDB0D0, 0x2EB67D, 0x2A092B, 0xFF5C8A, 0xECB22E, 0xECB22E]),
    // no colour at all
    skin!("greyscale", "Greyscale", true, [0x262626, 0x303030, 0x484848, 0x3A3A3A, 0x4A4A4A, 0xF2F2F2, 0xA6A6A6, 0xD6D6D6, 0x202020, 0xEDEDED, 0xB8B8B8, 0xFFFFFF]),
    // OLED black
    skin!("jet-black", "Jet Black", true, [0x000000, 0x0A0A0A, 0x1F1F1F, 0x111111, 0x1C1C1C, 0xE8E8E8, 0x8A8A8A, 0x4DA3FF, 0x050505, 0xFF6B6B, 0xE8B84A, 0xFFC83D]),
];

pub const DEFAULT: &str = "nu-dark";

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn by_id(id: &str) -> Option<usize> {
    let id = id.trim();
    SKINS.iter().position(|s| s.id.eq_ignore_ascii_case(id) || s.name.eq_ignore_ascii_case(id))
}

/// The skin in use.
pub fn current() -> &'static Skin {
    &SKINS[CURRENT.load(Ordering::Relaxed).min(SKINS.len() - 1)]
}

pub fn current_index() -> usize {
    CURRENT.load(Ordering::Relaxed).min(SKINS.len() - 1)
}

/// Make skin number `i` the one in use.
pub fn set_index(i: usize) {
    CURRENT.store(i.min(SKINS.len() - 1), Ordering::Relaxed);
}

/// Make the skin called `id` the one in use. False (and nothing changes) if there is none.
pub fn set(id: &str) -> bool {
    match by_id(id) {
        Some(i) => {
            set_index(i);
            true
        }
        None => false,
    }
}

/// Linear blend of two sRGB colours, `t` = 0 gives `a`, 1 gives `b` (done on the encoded values, which is enough for UI greys).
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * t).round().clamp(0.0, 255.0) as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// WCAG relative luminance of an sRGB colour.
pub fn luminance(c: u32) -> f32 {
    let f = |s: u32| {
        let v = ((c >> s) & 0xFF) as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(16) + 0.7152 * f(8) + 0.0722 * f(0)
}

/// WCAG contrast ratio, 1 (none) to 21.
pub fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

impl Skin {
    /// Text that reads on the accent colour: black or white, whichever has more contrast.
    pub fn on_accent(&self) -> u32 {
        if contrast(self.accent, 0x000000) >= contrast(self.accent, 0xFFFFFF) { 0x0B0B0B } else { 0xFFFFFF }
    }

    /// A line of a menu that is switched off.
    pub fn disabled(&self) -> u32 {
        mix(self.panel, self.dim, 0.62)
    }

    /// The keyboard / pointer highlight of a menu line: the hover colour with a little accent in it.
    pub fn selection(&self) -> u32 {
        mix(self.hover, self.accent, if self.dark { 0.22 } else { 0.16 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_found() {
        for (i, s) in SKINS.iter().enumerate() {
            assert_eq!(by_id(s.id), Some(i));
            assert_eq!(by_id(s.name), Some(i));
        }
        assert_eq!(by_id("no such skin"), None);
        assert_eq!(SKINS[0].id, DEFAULT);
        assert!(SKINS.len() >= 11);
    }

    #[test]
    fn every_skin_is_readable() {
        for s in SKINS {
            let need = |what: &str, a: u32, b: u32, min: f32| {
                let c = contrast(a, b);
                assert!(c >= min, "{}: {what} has contrast {c:.2} (needs {min})", s.id);
            };
            need("text on panel", s.text, s.panel, 7.0);
            need("text on bg", s.text, s.bg, 7.0);
            need("text on tile", s.text, s.tile, 7.0);
            need("text on hover", s.text, s.hover, 4.5);
            need("text on selection", s.text, s.selection(), 4.5);
            need("text on field", s.text, s.field, 7.0);
            need("hint on panel", s.dim, s.panel, 3.5);
            need("hint on bg", s.dim, s.bg, 3.0);
            need("disabled on panel", s.disabled(), s.panel, 1.8);
            need("accent on panel", s.accent, s.panel, 3.0);
            need("error on panel", s.error, s.panel, 3.0);
            need("star on bg", s.star, s.bg, 1.8);
            need("on-accent text", s.on_accent(), s.accent, 4.5);
            // A light skin has light panels and the other way round.
            assert_eq!(luminance(s.panel) < 0.4, s.dark, "{}: dark flag", s.id);
        }
    }

    #[test]
    fn mix_ends() {
        assert_eq!(mix(0x102030, 0xF0E0D0, 0.0), 0x102030);
        assert_eq!(mix(0x102030, 0xF0E0D0, 1.0), 0xF0E0D0);
    }
}
