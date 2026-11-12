//! Image sequences: runs of files that differ only in a counter (`shot_0001.png`, `shot_0002.png`, ...).
//! Used by the gallery to fold such a run into one entry. RAW and JPG of the same shot are never merged: a run
//! only ever contains files with the same extension.

use std::path::{Path, PathBuf};

/// A run of consecutive files `files[start..=end]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub start: usize,
    pub end: usize,
}

impl Run {
    pub fn len(&self) -> usize {
        self.end - self.start + 1
    }
}

/// (prefix, number, digit count, lower-case extension) of a file name like `shot_0012.png`.
fn parts(p: &Path) -> Option<(String, u64, usize, String)> {
    let stem = p.file_stem()?.to_string_lossy();
    let ext = p.extension()?.to_string_lossy().to_ascii_lowercase();
    let digits = stem.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 18 {
        return None;
    }
    let split = stem.len() - digits; // digits are ASCII, so this is a char boundary
    Some((stem[..split].to_lowercase(), stem[split..].parse().ok()?, digits, ext))
}

/// All runs of at least `min` frames in `files` (which must be in folder order). Frames must be numbered 1 apart,
/// have the same prefix, the same number of digits and the same extension.
pub fn find_runs(files: &[PathBuf], min: usize) -> Vec<Run> {
    let min = min.max(2);
    let mut runs = Vec::new();
    let mut start = 0;
    while start < files.len() {
        let mut end = start;
        if let Some(first) = parts(&files[start]) {
            let mut last = first.clone();
            while end + 1 < files.len() {
                match parts(&files[end + 1]) {
                    Some(next) if next.0 == last.0 && next.2 == last.2 && next.3 == last.3 && next.1 == last.1 + 1 => {
                        last = next;
                        end += 1;
                    }
                    _ => break,
                }
            }
        }
        if end - start + 1 >= min {
            runs.push(Run { start, end });
        }
        start = end + 1;
    }
    runs
}

/// What the gallery shows when sequences are folded: the file index of each tile (a run is represented by its first
/// frame). With `runs` empty this is every file.
pub fn gallery_items(count: usize, runs: &[Run]) -> Vec<usize> {
    let mut items = Vec::with_capacity(count);
    let mut i = 0;
    let mut r = 0;
    while i < count {
        if r < runs.len() && runs[r].start == i {
            items.push(i);
            i = runs[r].end + 1;
            r += 1;
        } else {
            items.push(i);
            i += 1;
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<PathBuf> {
        v.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn finds_a_numbered_run_and_leaves_the_rest() {
        let f = names(&["a.jpg", "shot_0001.png", "shot_0002.png", "shot_0003.png", "shot_0004.png", "z.png"]);
        let runs = find_runs(&f, 3);
        assert_eq!(runs, vec![Run { start: 1, end: 4 }]);
        assert_eq!(runs[0].len(), 4);
        assert_eq!(gallery_items(f.len(), &runs), vec![0, 1, 5]);
    }

    #[test]
    fn gaps_prefix_digits_and_extension_break_a_run() {
        // 1,2,4: the gap splits it, and each half is shorter than 3.
        assert!(find_runs(&names(&["s1.png", "s2.png", "s4.png", "s5.png"]), 3).is_empty());
        assert_eq!(find_runs(&names(&["s1.png", "s2.png", "s3.png", "s5.png", "s6.png"]), 3), vec![Run { start: 0, end: 2 }]);
        assert!(find_runs(&names(&["a1.png", "b2.png", "b3.png"]), 3).is_empty(), "different prefix");
    }

    #[test]
    fn nine_to_ten_with_fixed_width_is_one_run_but_changing_width_is_not() {
        assert_eq!(find_runs(&names(&["s09.png", "s10.png", "s11.png"]), 3).len(), 1);
        assert!(find_runs(&names(&["s8.png", "s9.png", "s10.png"]), 3).is_empty(), "width 1 -> 2");
    }

    #[test]
    fn raw_and_jpg_of_the_same_shot_are_never_merged() {
        // DSCF0001.RAF DSCF0001.JPG DSCF0002.RAF DSCF0002.JPG ... sorted the way the folder lists them
        let f = names(&["DSCF0001.JPG", "DSCF0001.RAF", "DSCF0002.JPG", "DSCF0002.RAF", "DSCF0003.JPG", "DSCF0003.RAF"]);
        assert!(find_runs(&f, 2).is_empty(), "alternating extensions never form a run");
        let only_raf = names(&["DSCF0001.RAF", "DSCF0002.RAF", "DSCF0003.RAF"]);
        assert_eq!(find_runs(&only_raf, 3).len(), 1);
        assert_eq!(gallery_items(f.len(), &[]), vec![0, 1, 2, 3, 4, 5], "no runs: every file is a tile");
    }

    #[test]
    fn extension_case_does_not_matter_and_names_without_digits_are_ignored() {
        assert_eq!(find_runs(&names(&["f1.PNG", "f2.png", "f3.Png"]), 3).len(), 1);
        assert!(find_runs(&names(&["alpha.png", "beta.png", "gamma.png"]), 2).is_empty());
        assert!(find_runs(&[], 2).is_empty());
    }
}
