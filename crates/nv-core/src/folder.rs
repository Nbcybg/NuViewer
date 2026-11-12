use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Images in one directory, naturally sorted, plus the index of the current one.
pub struct FolderList {
    /// The directory being browsed (empty for "nothing open").
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub index: usize,
}

impl FolderList {
    pub fn empty() -> Self {
        FolderList { dir: PathBuf::new(), files: Vec::new(), index: 0 }
    }

    /// `path` may be an image file (list its folder, select it) or a directory (select first image).
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("not found: {}", path.display()));
        }
        let (dir, target): (PathBuf, Option<std::ffi::OsString>) = if path.is_dir() {
            (path.to_path_buf(), None)
        } else {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            (parent, path.file_name().map(|n| n.to_os_string()))
        };
        let dir = std::path::absolute(&dir).unwrap_or(dir); // other windows/processes can't resolve relative paths
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()).map(nv_codecs::is_supported_ext).unwrap_or(false))
            .collect();
        files.sort_by(|a, b| cmp_paths(a, b));
        let index = target
            .and_then(|t| {
                let t = t.to_string_lossy().to_lowercase();
                files.iter().position(|f| f.file_name().map(|n| n.to_string_lossy().to_lowercase() == t).unwrap_or(false))
            })
            .unwrap_or(0);
        Ok(FolderList { dir, files, index })
    }

    pub fn current(&self) -> Option<&PathBuf> {
        self.files.get(self.index)
    }

    pub fn position(&self, p: &Path) -> Option<usize> {
        self.files.iter().position(|f| f == p)
    }

    /// Like `position`, but ignoring letter case (Windows paths) — for paths that came from elsewhere.
    pub fn position_ignore_case(&self, p: &Path) -> Option<usize> {
        let want = p.to_string_lossy().to_lowercase();
        self.files.iter().position(|f| f.to_string_lossy().to_lowercase() == want)
    }

    /// True if `dir` is the folder this list was made from (case-insensitive).
    pub fn is_dir(&self, dir: &Path) -> bool {
        self.dir.to_string_lossy().to_lowercase() == dir.to_string_lossy().to_lowercase()
    }

    fn follow(&mut self, path: Option<PathBuf>) {
        if let Some(i) = path.and_then(|p| self.position(&p)) {
            self.index = i;
        }
    }

    /// Insert `p` at its sorted place (no-op if present). Keeps `index` on the same file. Returns its index.
    pub fn insert_sorted(&mut self, p: PathBuf) -> usize {
        if let Some(i) = self.position(&p) {
            return i;
        }
        let cur = self.current().cloned();
        let at = self.files.partition_point(|f| cmp_paths(f, &p) == Ordering::Less);
        self.files.insert(at, p);
        self.follow(cur);
        at
    }

    /// Remove entry `i`. If it was before the current one, `index` shifts down; if it *was* the current one,
    /// `index` stays put so the next image slides into place (clamped at the end).
    pub fn remove_at(&mut self, i: usize) {
        if i >= self.files.len() {
            return;
        }
        self.files.remove(i);
        if i < self.index {
            self.index -= 1;
        }
        self.index = self.index.min(self.files.len().saturating_sub(1));
    }

    /// Swap entry `i` for `new` (after a rename), re-sorting. `index` follows the file if it was the current one.
    pub fn replace_at(&mut self, i: usize, new: PathBuf) -> usize {
        if i >= self.files.len() {
            return self.insert_sorted(new);
        }
        let was_current = i == self.index;
        let cur = self.current().cloned();
        self.files.remove(i);
        let at = self.files.partition_point(|f| cmp_paths(f, &new) == Ordering::Less);
        self.files.insert(at, new);
        if was_current {
            self.index = at;
        } else {
            self.follow(cur);
        }
        at
    }
}

fn cmp_paths(a: &Path, b: &Path) -> Ordering {
    let (an, bn) = (a.file_name().unwrap_or_default().to_string_lossy(), b.file_name().unwrap_or_default().to_string_lossy());
    natural_cmp(&an, &bn).then_with(|| an.cmp(&bn))
}

fn take_digits(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(&c) = it.peek() {
        if c.is_ascii_digit() {
            s.push(c);
            it.next();
        } else {
            break;
        }
    }
    s
}

/// Case-insensitive "natural" order: `img2` < `img10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x.is_ascii_digit() && y.is_ascii_digit() {
                    let (da, db) = (take_digits(&mut ai), take_digits(&mut bi));
                    let (ta, tb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
                    let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                    if o != Ordering::Equal {
                        return o;
                    }
                } else {
                    let (xl, yl) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
                    if xl != yl {
                        return xl.cmp(&yl);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_cmp(a, b));
        v
    }

    fn list(names: &[&str], index: usize) -> FolderList {
        FolderList { dir: PathBuf::from("d"), files: names.iter().map(|n| PathBuf::from(format!("d/{n}"))).collect(), index }
    }

    fn names(l: &FolderList) -> Vec<String> {
        l.files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn remove_keeps_selection_sensible() {
        let mut l = list(&["a", "b", "c", "d"], 2);
        l.remove_at(0); // before current: index shifts
        assert_eq!((names(&l), l.index), (vec!["b".into(), "c".into(), "d".into()], 1));
        l.remove_at(1); // current removed: next slides in
        assert_eq!((names(&l), l.index), (vec!["b".into(), "d".into()], 1));
        l.remove_at(1); // last removed: clamp
        assert_eq!((names(&l), l.index), (vec!["b".into()], 0));
        l.remove_at(0);
        assert_eq!((l.files.len(), l.index), (0, 0));
    }

    #[test]
    fn insert_sorted_uses_natural_order_and_keeps_current() {
        let mut l = list(&["img1.jpg", "img2.jpg", "img10.jpg"], 2);
        let at = l.insert_sorted(PathBuf::from("d/img3.jpg"));
        assert_eq!(at, 2);
        assert_eq!(names(&l), ["img1.jpg", "img2.jpg", "img3.jpg", "img10.jpg"]);
        assert_eq!(l.current().unwrap().file_name().unwrap(), "img10.jpg", "still on the same file");
        assert_eq!(l.insert_sorted(PathBuf::from("d/img3.jpg")), 2, "no duplicates");
    }

    #[test]
    fn replace_follows_a_renamed_current_file() {
        let mut l = list(&["a.jpg", "b.jpg", "c.jpg"], 0);
        let at = l.replace_at(0, PathBuf::from("d/z.jpg"));
        assert_eq!(at, 2);
        assert_eq!(l.index, 2, "index follows the renamed file to its new place");
        assert_eq!(names(&l), ["b.jpg", "c.jpg", "z.jpg"]);

        let mut l = list(&["a.jpg", "b.jpg", "c.jpg"], 2);
        l.replace_at(0, PathBuf::from("d/z.jpg")); // renaming a different file
        assert_eq!(l.current().unwrap().file_name().unwrap(), "c.jpg");
    }

    #[test]
    fn missing_path_is_an_error() {
        assert!(FolderList::open(Path::new("Z:/definitely/not/here.jpg")).is_err());
    }

    #[test]
    fn numbers_sort_numerically() {
        assert_eq!(sorted(vec!["img10.jpg", "img2.jpg", "img1.jpg"]), ["img1.jpg", "img2.jpg", "img10.jpg"]);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(sorted(vec!["b.png", "A.png", "c.png"]), ["A.png", "b.png", "c.png"]);
    }

    #[test]
    fn leading_zeros_equal_value() {
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Equal);
        assert_eq!(natural_cmp("a02", "a1"), Ordering::Greater);
    }

    #[test]
    fn prefix_is_smaller() {
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
    }

    #[test]
    fn thai_names_do_not_panic() {
        assert_eq!(sorted(vec!["รูป10.jpg", "รูป2.jpg"]), ["รูป2.jpg", "รูป10.jpg"]);
    }
}
