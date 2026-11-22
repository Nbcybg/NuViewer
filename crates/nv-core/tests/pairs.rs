//! RAW + JPG pairs in a real folder list. This file is its own test program, so switching the global "group pairs" setting here
//! cannot disturb the other tests (which expect the factory rule: every file is its own picture).

use nv_core::{rawpair, FolderList};
use std::path::{Path, PathBuf};

fn scratch(tag: &str, names: &[&str]) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nv_pairs_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for n in names {
        std::fs::write(d.join(n), b"x").unwrap();
    }
    d
}

fn names(l: &FolderList) -> Vec<String> {
    l.files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn separate_by_factory_grouped_on_request_and_the_current_picture_follows() {
    let d = scratch("list", &["DSCF0001.RAF", "DSCF0001.JPG", "DSCF0002.RAF", "DSCF0003.JPG", "notes.txt"]);

    rawpair::set_group(false);
    let l = FolderList::open(&d).unwrap();
    assert_eq!(names(&l), ["DSCF0001.JPG", "DSCF0001.RAF", "DSCF0002.RAF", "DSCF0003.JPG"], "factory: all four are pictures");

    rawpair::set_group(true);
    let l = FolderList::open(&d).unwrap();
    assert_eq!(names(&l), ["DSCF0001.JPG", "DSCF0002.RAF", "DSCF0003.JPG"], "the RAW with a JPG is part of that pair");

    // Asking for the hidden RAW opens the JPG that stands for it.
    let l = FolderList::open(&d.join("DSCF0001.RAF")).unwrap();
    assert_eq!(l.current().unwrap().file_name().unwrap(), "DSCF0001.JPG");
    // A RAW that has no JPG is opened as itself.
    let l = FolderList::open(&d.join("DSCF0002.RAF")).unwrap();
    assert_eq!(l.current().unwrap().file_name().unwrap(), "DSCF0002.RAF");

    // Operations on the pair reach both files; separate mode touches one.
    assert_eq!(rawpair::with_companions(&d.join("DSCF0001.JPG")), vec![d.join("DSCF0001.JPG"), d.join("DSCF0001.RAF")]);
    rawpair::set_group(false);
    assert_eq!(rawpair::with_companions(&d.join("DSCF0001.JPG")), vec![d.join("DSCF0001.JPG")]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn files_that_arrive_later_join_or_leave_the_pair() {
    let d = scratch("late", &["a.JPG", "b.RAF"]);
    rawpair::set_group(true);
    let mut l = FolderList::open(&d).unwrap();
    assert_eq!(names(&l), ["a.JPG", "b.RAF"]);

    // a.RAF turns up (copied in): it is the pair of a.JPG and stays out of the list.
    let a_raf: &Path = &d.join("a.RAF");
    l.insert_sorted(a_raf.to_path_buf());
    assert_eq!(names(&l), ["a.JPG", "b.RAF"]);

    // b.JPG turns up: b.RAF was listed alone and now joins it.
    l.insert_sorted(d.join("b.JPG"));
    assert_eq!(names(&l), ["a.JPG", "b.JPG"]);

    rawpair::set_group(false);
    let _ = std::fs::remove_dir_all(&d);
}
