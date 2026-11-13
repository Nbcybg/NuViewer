use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// What the pictures of a folder are ordered by. One order serves everything that walks the list: next / previous, the
/// gallery, the film strip, the slideshow.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SortKey {
    /// File name, "natural" (img2 before img10), ignoring case. The default.
    #[default]
    Name,
    /// Date modified, oldest first.
    Modified,
    /// File size, smallest first.
    Size,
    /// File type (extension), then name.
    Type,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [SortKey::Name, SortKey::Modified, SortKey::Size, SortKey::Type];

    pub fn id(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Modified => "date",
            SortKey::Size => "size",
            SortKey::Type => "type",
        }
    }

    pub fn from_id(v: &str) -> Option<SortKey> {
        Some(match v.trim().to_ascii_lowercase().as_str() {
            "name" => SortKey::Name,
            "date" | "modified" => SortKey::Modified,
            "size" => SortKey::Size,
            "type" | "ext" => SortKey::Type,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Modified => "Date",
            SortKey::Size => "Size",
            SortKey::Type => "Type",
        }
    }

    /// The next key in the cycle Name > Date > Size > Type > Name.
    pub fn next(self) -> SortKey {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// How a folder list is ordered: by a key (ascending or descending), or shuffled.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Order {
    pub key: SortKey,
    pub descending: bool,
    /// `Some(seed)`: a random order that stays the same while the seed does (and for files that are added later).
    /// `key` is kept underneath, so switching the shuffle off returns to it.
    pub shuffle: Option<u64>,
}

/// Images in one directory in the chosen order (name by default), plus the index of the current one.
pub struct FolderList {
    /// The directory being browsed (empty for "nothing open").
    pub dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub index: usize,
    pub order: Order,
}

impl FolderList {
    pub fn empty() -> Self {
        FolderList { dir: PathBuf::new(), files: Vec::new(), index: 0, order: Order::default() }
    }

    /// `path` may be an image file (list its folder, select it) or a directory (select first image).
    pub fn open(path: &Path) -> Result<Self, String> {
        Self::open_with(path, Order::default())
    }

    /// Like `open`, in the given order.
    pub fn open_with(path: &Path, order: Order) -> Result<Self, String> {
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
        sort_files(&mut files, order);
        let index = target
            .and_then(|t| {
                let t = t.to_string_lossy().to_lowercase();
                files.iter().position(|f| f.file_name().map(|n| n.to_string_lossy().to_lowercase() == t).unwrap_or(false))
            })
            .unwrap_or(0);
        Ok(FolderList { dir, files, index, order })
    }

    /// Put the files in another order. The current picture stays current.
    pub fn set_order(&mut self, order: Order) {
        let cur = self.current().cloned();
        self.order = order;
        sort_files(&mut self.files, order);
        self.follow(cur);
    }

    /// Where `p` belongs in the current order, among the files already listed.
    fn place_of(&self, p: &Path) -> usize {
        let order = self.order;
        self.files.partition_point(|f| compare(order, f, p) == Ordering::Less)
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
        let at = self.place_of(&p);
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
        let at = self.place_of(&new);
        self.files.insert(at, new);
        if was_current {
            self.index = at;
        } else {
            self.follow(cur);
        }
        at
    }
}

/// A well mixed number for `path` under `seed`: sorting by it gives a random but repeatable order.
fn shuffle_key(path: &Path, seed: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ seed;
    for b in path.file_name().unwrap_or_default().to_string_lossy().to_lowercase().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    // splitmix64 finaliser
    h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

fn extension_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The number a file is ordered by for the date and size keys (0 when the file can't be read).
fn stat_key(p: &Path, key: SortKey) -> u64 {
    let Ok(m) = std::fs::metadata(p) else { return 0 };
    match key {
        SortKey::Size => m.len(),
        SortKey::Modified => m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0),
        _ => 0,
    }
}

/// Order two paths the way `sort_files` would (reads the files' dates / sizes when that is the key).
fn compare(order: Order, a: &Path, b: &Path) -> Ordering {
    if let Some(seed) = order.shuffle {
        return shuffle_key(a, seed).cmp(&shuffle_key(b, seed)).then_with(|| cmp_paths(a, b));
    }
    let o = match order.key {
        SortKey::Name => cmp_paths(a, b),
        SortKey::Type => extension_of(a).cmp(&extension_of(b)).then_with(|| cmp_paths(a, b)),
        k => stat_key(a, k).cmp(&stat_key(b, k)).then_with(|| cmp_paths(a, b)),
    };
    if order.descending { o.reverse() } else { o }
}

/// Sort `files` in place. Dates and sizes are read once per file, not once per comparison.
pub fn sort_files(files: &mut Vec<PathBuf>, order: Order) {
    if let Some(seed) = order.shuffle {
        files.sort_by(|a, b| shuffle_key(a, seed).cmp(&shuffle_key(b, seed)).then_with(|| cmp_paths(a, b)));
        return;
    }
    match order.key {
        SortKey::Name => files.sort_by(|a, b| cmp_paths(a, b)),
        SortKey::Type => files.sort_by(|a, b| extension_of(a).cmp(&extension_of(b)).then_with(|| cmp_paths(a, b))),
        k => {
            let mut keyed: Vec<(u64, PathBuf)> = files.drain(..).map(|p| (stat_key(&p, k), p)).collect();
            keyed.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| cmp_paths(&a.1, &b.1)));
            files.extend(keyed.into_iter().map(|(_, p)| p));
        }
    }
    if order.descending {
        files.reverse();
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
        FolderList { dir: PathBuf::from("d"), files: names.iter().map(|n| PathBuf::from(format!("d/{n}"))).collect(), index, order: Order::default() }
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

    /// A scratch folder with files of these names and sizes; modified times are 1 s apart in the given order of `age_rank`.
    fn scratch(tag: &str, files: &[(&str, usize, u64)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nv_folder_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, size, secs_after_epoch) in files {
            let p = dir.join(name);
            std::fs::write(&p, vec![0u8; *size]).unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
            f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + secs_after_epoch)).unwrap();
        }
        dir
    }

    fn listed(l: &FolderList) -> Vec<String> {
        names(l)
    }

    #[test]
    fn size_date_and_type_orders_read_the_files() {
        let dir = scratch("orders", &[("a.png", 30, 5), ("b.jpg", 10, 9), ("c.gif", 20, 1)]);
        let open = |key, descending| FolderList::open_with(&dir, Order { key, descending, shuffle: None }).unwrap();
        assert_eq!(listed(&open(SortKey::Name, false)), ["a.png", "b.jpg", "c.gif"]);
        assert_eq!(listed(&open(SortKey::Name, true)), ["c.gif", "b.jpg", "a.png"]);
        assert_eq!(listed(&open(SortKey::Size, false)), ["b.jpg", "c.gif", "a.png"]);
        assert_eq!(listed(&open(SortKey::Size, true)), ["a.png", "c.gif", "b.jpg"]);
        assert_eq!(listed(&open(SortKey::Modified, false)), ["c.gif", "a.png", "b.jpg"]);
        assert_eq!(listed(&open(SortKey::Modified, true)), ["b.jpg", "a.png", "c.gif"]);
        assert_eq!(listed(&open(SortKey::Type, false)), ["c.gif", "b.jpg", "a.png"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_the_order_keeps_the_current_picture() {
        let dir = scratch("keep", &[("a.png", 30, 5), ("b.jpg", 10, 9), ("c.gif", 20, 1)]);
        let mut l = FolderList::open(&dir.join("b.jpg")).unwrap();
        assert_eq!(l.current().unwrap().file_name().unwrap(), "b.jpg");
        l.set_order(Order { key: SortKey::Modified, descending: false, shuffle: None });
        assert_eq!(listed(&l), ["c.gif", "a.png", "b.jpg"]);
        assert_eq!(l.current().unwrap().file_name().unwrap(), "b.jpg", "still on b");
        assert_eq!(l.index, 2);
        l.set_order(Order::default());
        assert_eq!(l.current().unwrap().file_name().unwrap(), "b.jpg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shuffle_is_repeatable_per_seed_and_reversible() {
        let many: Vec<String> = (0..40).map(|i| format!("img{i:02}.jpg")).collect();
        let refs: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
        let mut l = list(&refs, 5);
        let by_name = names(&l);
        let cur = l.current().cloned().unwrap();
        l.set_order(Order { shuffle: Some(1), ..Order::default() });
        let first = names(&l);
        assert_ne!(first, by_name, "40 files are not left in name order");
        assert_eq!(l.current().unwrap(), &cur, "the picture on screen does not change");
        let mut sorted_back = first.clone();
        sorted_back.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(sorted_back, by_name, "same files, only the order differs");
        l.set_order(Order { shuffle: Some(2), ..Order::default() });
        assert_ne!(names(&l), first, "another seed, another order");
        l.set_order(Order { shuffle: Some(1), ..Order::default() });
        assert_eq!(names(&l), first, "the same seed gives the same order again");
        l.set_order(Order::default());
        assert_eq!(names(&l), by_name, "switching it off returns to the name order");
    }

    #[test]
    fn files_added_while_shuffled_land_where_the_shuffle_puts_them() {
        let mut l = list(&["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg", "f.jpg"], 0);
        l.set_order(Order { shuffle: Some(7), ..Order::default() });
        let at = l.insert_sorted(PathBuf::from("d/zz.jpg"));
        let mut fresh = list(&["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg", "f.jpg", "zz.jpg"], 0);
        fresh.set_order(Order { shuffle: Some(7), ..Order::default() });
        assert_eq!(names(&l), names(&fresh), "same result as shuffling the whole list at once");
        assert_eq!(l.files[at], PathBuf::from("d/zz.jpg"));
    }

    #[test]
    fn sort_key_ids_round_trip_and_cycle() {
        for k in SortKey::ALL {
            assert_eq!(SortKey::from_id(k.id()), Some(k));
        }
        assert_eq!(SortKey::from_id("nonsense"), None);
        let mut k = SortKey::Name;
        for _ in 0..SortKey::ALL.len() {
            k = k.next();
        }
        assert_eq!(k, SortKey::Name);
    }

    #[test]
    fn thai_names_do_not_panic() {
        assert_eq!(sorted(vec!["รูป10.jpg", "รูป2.jpg"]), ["รูป2.jpg", "รูป10.jpg"]);
    }
}
