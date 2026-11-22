//! What Ctrl+Z can take back in the gallery: deletes (the files are still in the Recycle Bin), moves and renames (put them
//! back), copies (remove the copies). The shell does the moving with its own dialogs, so after the fact we look at what is
//! on disk to learn what really happened; these helpers do that looking and have no window in them.

use std::path::{Path, PathBuf};

/// How many steps are kept.
pub const LIMIT: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// These went to the Recycle Bin from these paths.
    Deleted(Vec<PathBuf>),
    /// `(from, to)`: moved or renamed.
    Moved(Vec<(PathBuf, PathBuf)>),
    /// These are copies that did not exist before.
    Copied(Vec<PathBuf>),
}

impl Step {
    /// "Undo delete (3 items)" and so on, for the menu.
    pub fn label(&self) -> String {
        let (what, n) = match self {
            Step::Deleted(v) => ("delete", v.len()),
            Step::Moved(v) if !v.is_empty() && v.iter().all(|(a, b)| a.parent() == b.parent()) => ("rename", v.len()),
            Step::Moved(v) => ("move", v.len()),
            Step::Copied(v) => ("copy", v.len()),
        };
        if n > 1 { format!("Undo {what} ({n} items)") } else { format!("Undo {what}") }
    }

    pub fn count(&self) -> usize {
        match self {
            Step::Deleted(v) => v.len(),
            Step::Moved(v) => v.len(),
            Step::Copied(v) => v.len(),
        }
    }
}

/// After moving `sources` into `dest`: the pairs that really moved (the source is gone and the file is there).
pub fn moved_pairs(sources: &[PathBuf], dest: &Path) -> Vec<(PathBuf, PathBuf)> {
    sources
        .iter()
        .filter_map(|s| {
            let to = dest.join(s.file_name()?);
            (!s.exists() && to.exists()).then(|| (s.clone(), to))
        })
        .collect()
}

/// After copying `sources` into `dest`: the copies that exist now and did not before. `existed` lists the names that were
/// already in `dest` (lower case) before the copy, so a file that was replaced is not mistaken for a new copy.
pub fn copied_files(sources: &[PathBuf], dest: &Path, existed: &[String]) -> Vec<PathBuf> {
    sources
        .iter()
        .filter_map(|s| {
            let name = s.file_name()?;
            let to = dest.join(name);
            (to.exists() && !existed.contains(&name.to_string_lossy().to_lowercase())).then_some(to)
        })
        .collect()
}

/// The names (lower case) of what is in `dir` right now.
pub fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default()
}

/// Of the files that were sent to the Recycle Bin, the ones that are really gone from where they were.
pub fn gone(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().filter(|p| !p.exists()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_gal_undo_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn labels_say_what_would_be_undone() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(Step::Deleted(vec![p("C:\\a")]).label(), "Undo delete");
        assert_eq!(Step::Deleted(vec![p("C:\\a"), p("C:\\b")]).label(), "Undo delete (2 items)");
        assert_eq!(Step::Moved(vec![(p("C:\\x\\a.jpg"), p("C:\\x\\b.jpg"))]).label(), "Undo rename");
        assert_eq!(Step::Moved(vec![(p("C:\\x\\a.jpg"), p("C:\\y\\a.jpg"))]).label(), "Undo move");
        assert_eq!(Step::Copied(vec![p("C:\\y\\a.jpg")]).label(), "Undo copy");
    }

    #[test]
    fn a_move_is_recognised_by_the_source_being_gone_and_the_target_being_there() {
        let (src, dst) = (temp("ms"), temp("md"));
        fs::write(src.join("a.jpg"), b"a").unwrap();
        fs::write(src.join("b.jpg"), b"b").unwrap();
        let sources = vec![src.join("a.jpg"), src.join("b.jpg")];
        fs::rename(src.join("a.jpg"), dst.join("a.jpg")).unwrap(); // only a was moved (b's move was cancelled)
        assert_eq!(moved_pairs(&sources, &dst), vec![(src.join("a.jpg"), dst.join("a.jpg"))]);
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn a_copy_is_new_only_if_the_name_was_not_there_before() {
        let (src, dst) = (temp("cs"), temp("cd"));
        fs::write(src.join("a.jpg"), b"a").unwrap();
        fs::write(src.join("b.jpg"), b"b").unwrap();
        fs::write(dst.join("b.jpg"), b"old b").unwrap();
        let before = names_in(&dst);
        let sources = vec![src.join("a.jpg"), src.join("b.jpg")];
        fs::copy(src.join("a.jpg"), dst.join("a.jpg")).unwrap();
        fs::copy(src.join("b.jpg"), dst.join("b.jpg")).unwrap(); // replaced the old b
        assert_eq!(copied_files(&sources, &dst, &before), vec![dst.join("a.jpg")]);
        let _ = fs::remove_dir_all(&src);
        let _ = fs::remove_dir_all(&dst);
    }
}
