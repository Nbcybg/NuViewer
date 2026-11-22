//! RAW + JPG pairs: one shot saved twice by the camera (`DSCF8252.RAF` + `DSCF8252.JPG`).
//!
//! The factory rule is that both are listed as separate pictures (a file that is hidden is a file you cannot cull, and editing
//! programs need both). **Grouping is an opt-in view**: with it on, the RAW of a pair is not listed, the JPG stands for the pair, and
//! delete / move / copy / rename of that picture act on both files, so a pair never falls apart. Nothing is ever merged on disk, and
//! a RAW without a JPG (or the other way round) is always listed on its own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static GROUP: AtomicBool = AtomicBool::new(false);

/// Group RAW + JPG pairs in the lists of folders opened from now on (the viewer's list and the gallery's).
pub fn set_group(on: bool) {
    GROUP.store(on, Ordering::Relaxed);
}

pub fn group() -> bool {
    GROUP.load(Ordering::Relaxed)
}

/// The in-camera companions of a RAW: what stands for the pair when they are grouped.
pub fn is_front_ext(ext: &str) -> bool {
    matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "jpe" | "jfif" | "heic" | "heif" | "hif")
}

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// For a list of files of one folder: which entries are a RAW that has a JPG / HEIF of the same name (so they are left out when
/// pairs are grouped).
pub fn hidden_raws(files: &[PathBuf]) -> Vec<bool> {
    let mut fronts: std::collections::HashSet<String> = std::collections::HashSet::new();
    for f in files {
        if is_front_ext(&ext_of(f)) {
            fronts.insert(stem_of(f));
        }
    }
    files.iter().map(|f| !fronts.is_empty() && nv_codecs::is_raw_ext(&ext_of(f)) && fronts.contains(&stem_of(f))).collect()
}

/// `files` without the RAWs that are part of a pair.
pub fn without_hidden(files: Vec<PathBuf>) -> Vec<PathBuf> {
    let hide = hidden_raws(&files);
    if !hide.iter().any(|h| *h) {
        return files;
    }
    files.into_iter().zip(hide).filter(|(_, h)| !*h).map(|(f, _)| f).collect()
}

/// The RAW files that go with `front` (a JPG / HEIF), found in its folder: exact names as they are on disk. Empty for anything
/// else. Works whether or not grouping is on, so file operations can ask it only when it is.
pub fn companions_of(front: &Path) -> Vec<PathBuf> {
    if !is_front_ext(&ext_of(front)) {
        return Vec::new();
    }
    let (Some(dir), stem) = (front.parent(), stem_of(front)) else { return Vec::new() };
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.path())
        .filter(|p| nv_codecs::is_raw_ext(&ext_of(p)) && stem_of(p) == stem)
        .collect();
    out.sort();
    out
}

/// `front` and its RAW companions (the files an operation on the pair touches); just `front` when pairs are not grouped.
pub fn with_companions(front: &Path) -> Vec<PathBuf> {
    let mut v = vec![front.to_path_buf()];
    if group() {
        v.extend(companions_of(front));
    }
    v
}

/// Where `companion` goes when its pair's front file is renamed to `new_front`: the same name with the new stem.
pub fn renamed_companion(companion: &Path, new_front: &Path) -> PathBuf {
    let stem = new_front.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
    let mut name = stem;
    if let Some(e) = companion.extension() {
        name.push(".");
        name.push(e);
    }
    companion.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(format!("d/{s}"))
    }

    #[test]
    fn a_raw_with_a_jpg_is_hidden_and_everything_else_stays() {
        let files = vec![p("A.JPG"), p("a.raf"), p("b.RAF"), p("c.jpg"), p("d.hif"), p("d.cr3"), p("e.png"), p("e.nef")];
        let hide = hidden_raws(&files);
        // a.raf (A.JPG), d.cr3 (d.hif) go; b.RAF has no JPG; e.nef has only a PNG (not an in-camera companion).
        assert_eq!(hide, [false, true, false, false, false, true, false, false]);
        let kept: Vec<String> = without_hidden(files).iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(kept, ["A.JPG", "b.RAF", "c.jpg", "d.hif", "e.png", "e.nef"]);
    }

    #[test]
    fn nothing_changes_without_pairs() {
        let files = vec![p("a.jpg"), p("b.raf")];
        assert_eq!(without_hidden(files.clone()), files);
    }

    #[test]
    fn companions_are_found_on_disk_by_exact_name() {
        let d = std::env::temp_dir().join(format!("nv_rawpair_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["IMG_1.JPG", "img_1.CR2", "IMG_2.JPG", "IMG_3.CR2"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        assert_eq!(companions_of(&d.join("IMG_1.JPG")), vec![d.join("img_1.CR2")]);
        assert!(companions_of(&d.join("IMG_2.JPG")).is_empty());
        assert!(companions_of(&d.join("IMG_3.CR2")).is_empty(), "a RAW has no companion of its own");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_renamed_pair_keeps_each_extension() {
        assert_eq!(renamed_companion(Path::new("d/IMG_1.CR2"), Path::new("d/Kyoto 1.jpg")), PathBuf::from("d/Kyoto 1.CR2"));
    }
}
