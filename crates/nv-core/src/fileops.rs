//! File operations on the current image: pure `std::fs` logic with no UI, so it can be unit-tested.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7",
    "LPT8", "LPT9",
];

/// Windows file-name rules (also fine on other platforms). Err carries a short user-facing reason.
pub fn valid_file_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name is empty".into());
    }
    if name.chars().count() > 255 {
        return Err("name is too long".into());
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err("name can't end with a space or dot".into());
    }
    if name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return Err("name can't contain \\ / : * ? \" < > |".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err("that name is reserved by Windows".into());
    }
    Ok(())
}

/// `dir/file_name`, or `dir/stem (2).ext`, `(3)`, ... if it already exists.
pub fn unique_path(dir: &Path, file_name: &OsStr) -> PathBuf {
    let first = dir.join(file_name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(file_name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !c.exists())
        .expect("unbounded range always yields a free name")
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Move `src` into `dst_dir` (never overwriting: a clash gets a " (2)" suffix). Works across drives.
pub fn move_file(src: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
    let name = src.file_name().ok_or("no file name")?;
    if src.parent().map(|p| same_dir(p, dst_dir)).unwrap_or(false) {
        return Err("already in that folder".into());
    }
    let dst = unique_path(dst_dir, name);
    match std::fs::rename(src, &dst) {
        Ok(()) => Ok(dst),
        // ERROR_NOT_SAME_DEVICE: rename can't cross volumes, so copy then delete.
        Err(e) if e.raw_os_error() == Some(17) => {
            std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
            std::fs::remove_file(src).map_err(|e| e.to_string())?;
            Ok(dst)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Copy `src` into `dst_dir` (never overwriting). Copying into its own folder makes " (2)".
pub fn copy_file(src: &Path, dst_dir: &Path) -> Result<PathBuf, String> {
    let name = src.file_name().ok_or("no file name")?;
    let dst = unique_path(dst_dir, name);
    std::fs::copy(src, &dst).map_err(|e: io::Error| e.to_string())?;
    Ok(dst)
}

/// Rename within the same folder. A case-only change (`a.jpg` -> `A.jpg`) is allowed.
pub fn rename_file(src: &Path, new_name: &str) -> Result<PathBuf, String> {
    valid_file_name(new_name)?;
    let dst = src.with_file_name(new_name);
    if dst == src {
        return Ok(dst);
    }
    let case_only = dst.to_string_lossy().to_lowercase() == src.to_string_lossy().to_lowercase();
    if dst.exists() && !case_only {
        return Err("a file with that name already exists".into());
    }
    std::fs::rename(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

/// Something the user did that Ctrl+Z can reverse.
#[derive(Debug, Clone, PartialEq)]
pub enum UndoOp {
    /// File renamed or moved: put `to` back at `from`.
    Moved { from: PathBuf, to: PathBuf },
    /// File copied: remove the copy at `copy`.
    Copied { copy: PathBuf },
    /// File sent to the Recycle Bin from `original`.
    Deleted { original: PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_fileops_{tag}_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn name_validation() {
        for ok in ["a.jpg", "รูป 1.png", "my photo (2).jpeg", "x"] {
            assert!(valid_file_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "a/b.jpg", "a:b", "what?.png", "trail.", "trail ", "con.txt", "NUL", "a\tb", "a|b"] {
            assert!(valid_file_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn unique_names_count_up() {
        let d = temp_dir("uniq");
        fs::write(d.join("a.jpg"), b"1").unwrap();
        assert_eq!(unique_path(&d, OsStr::new("b.jpg")), d.join("b.jpg"));
        assert_eq!(unique_path(&d, OsStr::new("a.jpg")), d.join("a (2).jpg"));
        fs::write(d.join("a (2).jpg"), b"2").unwrap();
        assert_eq!(unique_path(&d, OsStr::new("a.jpg")), d.join("a (3).jpg"));
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn rename_rules() {
        let d = temp_dir("ren");
        let a = d.join("a.jpg");
        fs::write(&a, b"A").unwrap();
        fs::write(d.join("b.jpg"), b"B").unwrap();
        assert!(rename_file(&a, "b.jpg").is_err(), "must not overwrite");
        assert!(rename_file(&a, "bad/name.jpg").is_err());
        let c = rename_file(&a, "c.jpg").unwrap();
        assert!(c.exists() && !a.exists());
        let up = rename_file(&c, "C.jpg").unwrap(); // case-only change
        assert_eq!(fs::read(&up).unwrap(), b"A");
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn move_and_copy_never_overwrite() {
        let (src_dir, dst_dir) = (temp_dir("src"), temp_dir("dst"));
        let f = src_dir.join("p.png");
        fs::write(&f, b"new").unwrap();
        fs::write(dst_dir.join("p.png"), b"old").unwrap();

        let copied = copy_file(&f, &dst_dir).unwrap();
        assert_eq!(copied, dst_dir.join("p (2).png"));
        assert!(f.exists(), "copy keeps the source");
        assert_eq!(fs::read(dst_dir.join("p.png")).unwrap(), b"old");

        let moved = move_file(&f, &dst_dir).unwrap();
        assert_eq!(moved, dst_dir.join("p (3).png"));
        assert!(!f.exists(), "move removes the source");
        assert_eq!(fs::read(&moved).unwrap(), b"new");

        assert!(move_file(&moved, &dst_dir).is_err(), "same folder is refused");
        fs::remove_dir_all(&src_dir).ok();
        fs::remove_dir_all(&dst_dir).ok();
    }
}
