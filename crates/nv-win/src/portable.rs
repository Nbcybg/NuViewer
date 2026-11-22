//! Portable mode. A folder that has a file `portable.txt` (or `portable.ini`) next to the programs is a portable copy: it keeps its
//! settings, state, thumbnail cache, presets, language files and backups of edited pictures in `Data\` beside the programs, and
//! leaves `%APPDATA%` / `%LOCALAPPDATA%` and the registry alone, so it can live on a USB stick or be deleted by deleting the folder.
//!
//! How: before anything else runs, `init()` points the `APPDATA` and `LOCALAPPDATA` environment variables of the process (and so
//! of the programs it starts: the viewer, the gallery and the editor start each other) into `Data\Roaming` and `Data\Local`. Every
//! place that asks for those folders then finds the portable ones; the real local folder stays readable as
//! `NUVIEWER_REAL_LOCALAPPDATA` for the few lookups that hunt for other programs (ffmpeg, 7-Zip) in it.

use std::path::{Path, PathBuf};

/// The files whose presence next to the programs turns portable mode on.
pub const MARKERS: [&str; 2] = ["portable.txt", "portable.ini"];

/// Is `dir` a portable folder?
pub fn is_portable_dir(dir: &Path) -> bool {
    MARKERS.iter().any(|m| dir.join(m).is_file())
}

/// Where the portable data of a folder lives: (roaming, local).
pub fn data_dirs(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join("Data").join("Roaming"), dir.join("Data").join("Local"))
}

/// The text of `portable.txt` the portable package carries.
pub const MARKER_TEXT: &str = "This folder is a portable copy of NuViewer.\r\n\
\r\n\
While this file is here, the programs keep their settings, thumbnails, presets and the backups of edited pictures in the \"Data\" folder\r\n\
next to them and change nothing else on the PC (no registry entries, nothing in %APPDATA%). Copy or move the whole folder anywhere;\r\n\
delete it to remove everything. Delete this file to make the programs use the ordinary per-user folders again.\r\n";

/// Switch this process to portable mode if the folder it runs from is a portable one. Safe to call more than once; does nothing
/// for an installed copy (and for a test run that points `NUVIEWER_SETTINGS` somewhere else).
pub fn init() {
    if std::env::var_os("NUVIEWER_PORTABLE").is_some() || std::env::var_os("NUVIEWER_SETTINGS").is_some() {
        return;
    }
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    if !is_portable_dir(&dir) {
        return;
    }
    let (roaming, local) = data_dirs(&dir);
    if std::fs::create_dir_all(&roaming).is_err() || std::fs::create_dir_all(&local).is_err() {
        return; // a read-only place (a locked stick): stay with the ordinary folders rather than fail to start
    }
    if let Some(real) = std::env::var_os("LOCALAPPDATA") {
        std::env::set_var("NUVIEWER_REAL_LOCALAPPDATA", real);
    }
    std::env::set_var("APPDATA", &roaming);
    std::env::set_var("LOCALAPPDATA", &local);
    std::env::set_var("NUVIEWER_PORTABLE", "1");
}

/// Is this process running in portable mode?
pub fn is_portable() -> bool {
    std::env::var_os("NUVIEWER_PORTABLE").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_file_makes_a_folder_portable() {
        let d = std::env::temp_dir().join(format!("nv_portable_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(!is_portable_dir(&d));
        std::fs::write(d.join("portable.ini"), "").unwrap();
        assert!(is_portable_dir(&d));
        std::fs::remove_file(d.join("portable.ini")).unwrap();
        std::fs::write(d.join("portable.txt"), MARKER_TEXT).unwrap();
        assert!(is_portable_dir(&d));
        let (r, l) = data_dirs(&d);
        assert!(r.ends_with("Data\\Roaming") && l.ends_with("Data\\Local"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_marker_text_explains_itself() {
        assert!(MARKER_TEXT.contains("Data") && MARKER_TEXT.contains("portable"));
    }
}
