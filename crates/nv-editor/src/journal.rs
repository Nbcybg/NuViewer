//! The journal of burn-ins: one line per picture that NuEditor rewrote, with where its original was put. NuEditor, the viewer
//! (Ctrl+Z after coming back from the editor) and the editor itself (Ctrl+Alt+Z) read it, so a burn-in can be taken back from any
//! of them, not only from the program that made it. Plain text, tab separated: seconds since 1970, picture, backup.

use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub when: u64,
    pub original: PathBuf,
    pub backup: PathBuf,
}

fn file(dir: &Path) -> PathBuf {
    dir.join("burn-in-journal.tsv")
}

fn clean(p: &Path) -> String {
    p.display().to_string().replace(['\t', '\r', '\n'], " ")
}

/// Note that `original` was rewritten and its old self is at `backup`.
pub fn add(dir: &Path, original: &Path, backup: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file(dir))?;
    writeln!(f, "{when}\t{}\t{}", clean(original), clean(backup))
}

/// Every entry whose backup is still there, oldest first.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(file(dir)) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let when = it.next()?.parse().ok()?;
            let (original, backup) = (PathBuf::from(it.next()?), PathBuf::from(it.next()?));
            backup.is_file().then_some(Entry { when, original, backup })
        })
        .collect()
}

fn same(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// The newest entry for `picture`.
pub fn last_for(dir: &Path, picture: &Path) -> Option<Entry> {
    list(dir).into_iter().rev().find(|e| same(&e.original, picture))
}

/// Forget an entry (after it was restored, or its backup was used).
pub fn remove(dir: &Path, entry: &Entry) {
    let Ok(text) = std::fs::read_to_string(file(dir)) else { return };
    let line = format!("{}\t{}\t{}", entry.when, clean(&entry.original), clean(&entry.backup));
    let kept: Vec<&str> = text.lines().filter(|l| *l != line).collect();
    let _ = std::fs::write(file(dir), kept.join("\n") + if kept.is_empty() { "" } else { "\n" });
}

/// Put the backup back over the picture. The backup file is removed and the entry forgotten.
pub fn restore(dir: &Path, entry: &Entry) -> Result<(), String> {
    let bytes = std::fs::read(&entry.backup).map_err(|e| format!("the backup can't be read: {e}"))?;
    crate::save::write_atomic(&entry.original, &bytes).map_err(|e| format!("{}: {e}", entry.original.display()))?;
    let _ = std::fs::remove_file(&entry.backup);
    remove(dir, entry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burn_in_is_noted_found_and_taken_back() {
        let d = std::env::temp_dir().join(format!("nv-journal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let pic = d.join("A.jpg");
        let backup = d.join("1_A.jpg");
        std::fs::write(&pic, b"edited").unwrap();
        std::fs::write(&backup, b"original").unwrap();
        assert!(list(&d).is_empty());
        add(&d, &pic, &backup).unwrap();
        // A second one whose backup is gone is not listed.
        add(&d, &d.join("B.jpg"), &d.join("missing")).unwrap();
        assert_eq!(list(&d).len(), 1);
        let e = last_for(&d, &d.join("a.JPG")).unwrap();
        assert_eq!(e.backup, backup);
        restore(&d, &e).unwrap();
        assert_eq!(std::fs::read(&pic).unwrap(), b"original");
        assert!(!backup.exists());
        assert!(last_for(&d, &pic).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
