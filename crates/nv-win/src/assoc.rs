//! File-type chooser: which image formats should Windows offer to open with NuViewer?
//! Shared by `nuviewer.exe --associate` and the installer. The check boxes only decide what NuViewer *registers*
//! (see `registry`): Windows 10/11 won't let a program make itself the default, so after applying we report which types
//! already open with NuViewer and send the user to Settings > Default apps for the rest.

use crate::registry::{self, Layout, PROG_ID};
use crate::ui::{Form, ID_CANCEL, ID_OK};

/// One line in the chooser: a format (or family of formats) and the extensions it covers (lowercase, no dot).
pub struct Format {
    pub name: &'static str,
    pub exts: &'static [&'static str],
    /// Ticked by default in a first-time install (formats other programs usually own are left unticked).
    pub default_on: bool,
}

const fn f(name: &'static str, exts: &'static [&'static str], default_on: bool) -> Format {
    Format { name, exts, default_on }
}

/// Everything in `PICTURES` first, then the camera RAW families (`FORMATS[PICTURES..]`).
pub const FORMATS: &[Format] = &[
    f("JPEG", &["jpg", "jpeg", "jpe", "jfif"], true),
    f("PNG", &["png", "apng"], true),
    f("GIF", &["gif"], true),
    f("WebP", &["webp"], true),
    f("HEIC / HEIF", &["heic", "heif"], true),
    f("AVIF", &["avif"], true),
    f("BMP", &["bmp"], true),
    f("TIFF", &["tif", "tiff"], false),
    f("Icon", &["ico"], false),
    f("JPEG XR", &["jxr", "wdp", "hdp"], false),
    f("Photoshop", &["psd"], false),
    f("Targa (TGA)", &["tga", "icb", "vda", "vst"], false),
    f("DDS texture", &["dds"], false),
    f("PNM (PPM, PGM, PBM, PAM)", &["ppm", "pgm", "pbm", "pam", "pnm"], false),
    f("HDR / EXR / QOI", &["hdr", "exr", "qoi"], false),
    f("PDF / Illustrator", &["pdf", "ai"], false),
    f("SVG", &["svg", "svgz"], false),
    f("EPS", &["eps"], false),
    f("Windows metafile", &["emf", "wmf"], false),
    f("STL", &["stl"], false),
    f("OBJ", &["obj"], false),
    f("PLY", &["ply"], false),
    f("glTF", &["glb", "gltf"], false),
    f("FBX", &["fbx"], false),
    f("Canon RAW", &["cr2", "cr3", "crw"], true),
    f("Nikon RAW", &["nef", "nrw"], true),
    f("Sony RAW", &["arw", "sr2", "srf"], true),
    f("Fujifilm RAW", &["raf"], true),
    f("Olympus RAW", &["orf"], true),
    f("Panasonic RAW", &["rw2"], true),
    f("Pentax RAW", &["pef"], true),
    f("Samsung RAW", &["srw"], true),
    f("Adobe DNG", &["dng"], true),
    f("Other RAW", &["3fr", "erf", "kdc", "mrw", "raw", "rwl", "iiq"], true),
];

/// The groups of the chooser: title and the range of `FORMATS` each one shows.
pub const GROUPS: &[(&str, std::ops::Range<usize>)] = &[
    ("Pictures", 0..15),
    ("Documents and vector art", 15..19),
    ("3D models", 19..24),
    ("Camera RAW", 24..34),
];

const ROW_HEIGHT: i32 = 20;

/// Height in dips of the check-box list built by `build_checklist`.
pub fn checklist_height() -> i32 {
    GROUPS.iter().map(|(_, r)| (r.len() as i32 + 1) / 2 * ROW_HEIGHT + 30 + 8).sum()
}

const ID_FORMAT_BASE: u16 = 1000;
pub const ID_SELECT_ALL: u16 = 900;
pub const ID_SELECT_NONE: u16 = 901;

/// Every extension NuViewer can register, with its leading dot.
pub fn all_exts() -> Vec<String> {
    FORMATS.iter().flat_map(|f| f.exts.iter().map(|e| format!(".{e}"))).collect()
}

/// The extensions (with dots) of the formats whose box is ticked in `picked` (one flag per `FORMATS` entry).
pub fn exts_of(picked: &[bool]) -> Vec<String> {
    FORMATS.iter().zip(picked).filter(|(_, on)| **on).flat_map(|(f, _)| f.exts.iter().map(|e| format!(".{e}"))).collect()
}

/// Which formats count as ticked for a set of registered extensions (a format is ticked if any of its extensions is).
pub fn picked_for(registered: &[String]) -> Vec<bool> {
    FORMATS.iter().map(|f| f.exts.iter().any(|e| registered.iter().any(|r| r.trim_start_matches('.') == *e))).collect()
}

pub fn default_picks() -> Vec<bool> {
    FORMATS.iter().map(|f| f.default_on).collect()
}

/// Lay the format check boxes out in two groups ("Pictures", "Camera RAW") starting at (`x`,`y`), `width` dips wide.
/// Returns the y just below the last group. Check box ids are private; use `read_picks` / `handle_select`.
pub fn build_checklist(form: &Form, x: i32, y: i32, width: i32, picked: &[bool]) -> i32 {
    const ROW: i32 = ROW_HEIGHT;
    let col_w = (width - 36) / 2;
    let mut top = y;
    for (title, range) in GROUPS.iter().cloned() {
        let rows = (range.len() as i32 + 1) / 2;
        let height = rows * ROW + 30;
        form.group(x, top, width, height, title);
        for (n, i) in range.enumerate() {
            let (col, row) = ((n as i32) % 2, (n as i32) / 2);
            let label = format!("{}   .{}", FORMATS[i].name, FORMATS[i].exts.join(" ."));
            form.check(ID_FORMAT_BASE + i as u16, x + 14 + col * (col_w + 8), top + 22 + row * ROW, col_w, ROW - 2, &label, picked.get(i).copied().unwrap_or(false));
        }
        top += height + 8;
    }
    top
}

pub fn read_picks(form: &Form) -> Vec<bool> {
    (0..FORMATS.len()).map(|i| form.is_checked(ID_FORMAT_BASE + i as u16)).collect()
}

/// Handles the "Select all" / "Select none" buttons; returns true if `id` was one of them.
pub fn handle_select(form: &Form, id: u16) -> bool {
    let on = match id {
        ID_SELECT_ALL => true,
        ID_SELECT_NONE => false,
        _ => return false,
    };
    for i in 0..FORMATS.len() {
        form.set_checked(ID_FORMAT_BASE + i as u16, on);
    }
    true
}

/// The chooser window. `current` = extensions registered now (empty on first use: the defaults are pre-ticked).
/// Returns the extensions (with dots) to register, or None if cancelled.
pub fn choose(current: &[String]) -> Option<Vec<String>> {
    let picked = if current.is_empty() { default_picks() } else { picked_for(current) };
    let form = Form::new("NuViewer — file types", 560, 52 + checklist_height() + 124).ok()?;
    form.label(14, 12, 532, 34, "Choose the image formats NuViewer should open. Unticked formats are removed from NuViewer's \"Open with\" list.");
    let bottom = build_checklist(&form, 12, 52, 536, &picked);
    form.button(ID_SELECT_ALL, 12, bottom + 4, 96, 26, "Select all", false);
    form.button(ID_SELECT_NONE, 116, bottom + 4, 96, 26, "Select none", false);
    form.button(ID_OK, 372, bottom + 4, 84, 26, "Apply", true);
    form.button(ID_CANCEL, 464, bottom + 4, 84, 26, "Cancel", false);
    form.label(14, bottom + 38, 532, 34, "Windows only lets you confirm the default yourself: after Apply, NuViewer opens Settings > Default apps for you.");

    let mut result = None;
    form.run(&mut |form, id| {
        if handle_select(form, id) {
            return false;
        }
        match id {
            ID_OK => {
                result = Some(exts_of(&read_picks(form)));
                true
            }
            ID_CANCEL => true,
            _ => false,
        }
    });
    result
}

/// Split `enabled` into (types that already open with NuViewer, types still waiting for the user in Settings).
/// Reads Windows' own record of the user's choice; works only for the real registry (HKCU\...\FileExts).
pub fn default_status(enabled: &[String]) -> (Vec<String>, Vec<String>) {
    enabled.iter().cloned().partition(|e| registry::user_choice(e).as_deref() == Some(PROG_ID))
}

/// Open Settings > Apps > Default apps on NuViewer's own page (where each registered type has a "set default" button).
pub fn open_default_apps_settings() {
    use windows::core::w;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        ShellExecuteW(None, w!("open"), w!("ms-settings:defaultapps?registeredAppUser=NuViewer"), None, None, SW_SHOWNORMAL);
    }
}

/// Describe where the registered types stand, for a message box: how many already open with NuViewer and how many
/// still wait for the user in Settings. The bool says whether the caller should open that Settings page.
pub fn describe(enabled: &[String]) -> (String, bool) {
    if enabled.is_empty() {
        return ("NuViewer is not offered for any image type.".into(), false);
    }
    let (now, pending) = default_status(enabled);
    let mut text = format!("NuViewer is registered for {} file types.", enabled.len());
    if !now.is_empty() {
        text += &format!("\n{} of them already open with NuViewer.", now.len());
    }
    if !pending.is_empty() {
        text += &format!(
            "\n\n{} still open with another program. Windows only lets you change that yourself, so Settings > Default apps > NuViewer opens next: use \"Set default\" there.",
            pending.len()
        );
    }
    (text, !pending.is_empty())
}

/// Apply a choice to the real registry (HKCU), then `describe` it. `exe` is the installed nuviewer.exe.
pub fn apply_and_report(exe: &std::path::Path, enabled: &[String]) -> (String, bool) {
    if let Err(e) = registry::apply(&Layout::real(), exe, &all_exts(), enabled) {
        return (format!("Couldn't update the file types: {e}"), false);
    }
    describe(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_are_unique_lowercase_and_split_correctly() {
        let mut seen = std::collections::HashSet::new();
        for f in FORMATS {
            assert!(!f.exts.is_empty(), "{} has no extensions", f.name);
            for e in f.exts {
                assert_eq!(*e, e.to_ascii_lowercase());
                assert!(!e.starts_with('.'));
                assert!(seen.insert(*e), "duplicate extension {e}");
            }
        }
        assert_eq!(GROUPS.last().map(|g| g.1.end), Some(FORMATS.len()), "the groups cover every format");
        assert!(GROUPS.windows(2).all(|w| w[0].1.end == w[1].1.start));
        assert_eq!(FORMATS[GROUPS[1].1.start].name, "PDF / Illustrator");
        assert_eq!(FORMATS[GROUPS[3].1.start].name, "Canon RAW");
    }

    #[test]
    fn picks_round_trip() {
        let all = vec![true; FORMATS.len()];
        assert_eq!(exts_of(&all).len(), all_exts().len());
        let only_png = exts_of(&FORMATS.iter().map(|f| f.name == "PNG").collect::<Vec<_>>());
        assert_eq!(only_png, vec![".png", ".apng"]);
        let back = picked_for(&only_png);
        assert_eq!(back.iter().filter(|b| **b).count(), 1);
        assert!(picked_for(&[]).iter().all(|b| !b));
        // Off by default: TIFF, Icon, JPEG XR, Photoshop, the four game / render formats (TGA, DDS, PNM, HDR/EXR/QOI), and every document / vector / 3D format (other programs own those).
        assert_eq!(default_picks().iter().filter(|b| !**b).count(), 4 + 4 + 9);
    }
}
