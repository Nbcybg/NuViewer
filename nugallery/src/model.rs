//! The folder as the gallery sees it: entries (folders and files), how they are filtered, searched and ordered, and
//! which of them are selected. No drawing and no window in here.

use nv_core::folder::{natural_cmp, shuffle_key, SortKey};
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Folder,
    /// Camera RAW.
    Raw,
    Picture,
    /// PDF, Office, archives with a preview, text: shown by a preview or a card.
    Document,
    Model,
    /// Something we can't show (no thumbnail): listed when "all files" is on.
    Other,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    /// Lower case, no dot.
    pub ext: String,
    pub kind: Kind,
    pub size: u64,
    /// Seconds since 1970 (UTC).
    pub modified: i64,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Folder
    }

    /// Does this entry get a thumbnail made from its contents?
    pub fn has_thumb(&self) -> bool {
        matches!(self.kind, Kind::Raw | Kind::Picture | Kind::Document | Kind::Model)
    }

    /// Gets a thumbnail request: pictures and documents their own, a folder the first picture in it (its "cover").
    pub fn wants_thumb(&self) -> bool {
        self.has_thumb() || self.is_dir()
    }

    /// Can the viewer show it?
    pub fn viewable(&self) -> bool {
        self.has_thumb() && !(self.kind == Kind::Document && !nv_codecs::is_supported_ext(&self.ext))
    }
}

/// A stable number for a path (case-insensitive, like the file system): the key of its thumbnail and its selection.
pub fn path_id(p: &Path) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.to_string_lossy().to_lowercase().hash(&mut h);
    h.finish()
}

pub fn kind_of_ext(ext: &str) -> Kind {
    let e = ext.to_ascii_lowercase();
    if nv_codecs::is_raw_ext(&e) {
        Kind::Raw
    } else if matches!(e.as_str(), "pdf" | "ai" | "eps" | "swf" | "fla" | "html" | "htm" | "xhtml" | "mhtml" | "mht") || (!nv_codecs::is_supported_ext(&e) && nv_codecs::is_document_ext(&e)) {
        Kind::Document
    } else if nv_codecs::model::EXTS.contains(&e.as_str()) {
        Kind::Model
    } else if nv_codecs::is_supported_ext(&e) {
        Kind::Picture
    } else {
        Kind::Other
    }
}

/// The thumbnail work: a picture's own thumbnail, or for a folder the thumbnail of the first picture in it (by name,
/// ordinary pictures before RAW files). `Err` for a folder without pictures.
pub fn thumb_work(path: &Path) -> Result<nv_codecs::Image, String> {
    if !path.is_dir() {
        return nv_core::thumbs::make_thumb(path);
    }
    let rd = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    let mut best: Option<(bool, String, PathBuf)> = None;
    for e in rd.flatten().take(4000) {
        let Ok(t) = e.file_type() else { continue };
        if !t.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let ext = Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !nv_codecs::is_supported_ext(&ext) || matches!(kind_of_ext(&ext), Kind::Model | Kind::Document) {
            continue;
        }
        let raw = nv_codecs::is_raw_ext(&ext);
        let better = match &best {
            None => true,
            Some((braw, bname, _)) => (raw, nv_core::folder::natural_cmp(&name, bname)) < (*braw, std::cmp::Ordering::Equal),
        };
        if better {
            best = Some((raw, name, e.path()));
        }
    }
    match best {
        Some((_, _, p)) => nv_core::thumbs::make_thumb(&p),
        None => Err("no picture in this folder".into()),
    }
}

/// Read a folder. `Err` if it can't be opened. Hidden and system files are left out unless `show_hidden`.
pub fn scan(dir: &Path, show_hidden: bool) -> Result<Vec<Entry>, String> {
    use std::os::windows::fs::MetadataExt;
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !show_hidden && meta.file_attributes() & 0x6 != 0 {
            continue; // hidden or system
        }
        let path = e.path();
        let is_dir = meta.is_dir() || (meta.file_type().is_symlink() && std::fs::metadata(&path).map(|m| m.is_dir()).unwrap_or(false));
        let name = e.file_name().to_string_lossy().into_owned();
        let ext = if is_dir { String::new() } else { path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default() };
        let kind = if is_dir { Kind::Folder } else { kind_of_ext(&ext) };
        let modified = meta.modified().map(nv_core::batch::unix_of).unwrap_or(0);
        out.push(Entry { id: path_id(&path), path, name, ext, kind, size: if is_dir { 0 } else { meta.len() }, modified });
    }
    Ok(out)
}

/// The "Show only…" choices.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Show {
    /// Pictures, RAW, documents and 3D models (what the viewer can open). The default.
    #[default]
    Media,
    /// Every file.
    All,
    Pictures,
    Raw,
    Jpeg,
    Png,
    Gif,
    Webp,
    Tiff,
    Documents,
    Models,
}

impl Show {
    pub const ALL_CHOICES: [Show; 11] = [Show::Media, Show::All, Show::Pictures, Show::Raw, Show::Jpeg, Show::Png, Show::Gif, Show::Webp, Show::Tiff, Show::Documents, Show::Models];

    pub fn id(self) -> &'static str {
        match self {
            Show::Media => "media",
            Show::All => "all",
            Show::Pictures => "pictures",
            Show::Raw => "raw",
            Show::Jpeg => "jpeg",
            Show::Png => "png",
            Show::Gif => "gif",
            Show::Webp => "webp",
            Show::Tiff => "tiff",
            Show::Documents => "documents",
            Show::Models => "models",
        }
    }

    pub fn from_id(v: &str) -> Option<Show> {
        Self::ALL_CHOICES.into_iter().find(|s| s.id() == v.trim().to_ascii_lowercase())
    }

    pub fn label(self) -> &'static str {
        match self {
            Show::Media => "Pictures and documents",
            Show::All => "All files",
            Show::Pictures => "Pictures only",
            Show::Raw => "RAW only",
            Show::Jpeg => "JPEG only",
            Show::Png => "PNG only",
            Show::Gif => "GIF only",
            Show::Webp => "WebP only",
            Show::Tiff => "TIFF only",
            Show::Documents => "Documents only",
            Show::Models => "3D models only",
        }
    }

    /// Does a file pass? (Folders are handled by the caller.)
    pub fn accepts(self, e: &Entry) -> bool {
        match self {
            Show::Media => e.kind != Kind::Other,
            Show::All => true,
            Show::Pictures => e.kind == Kind::Picture,
            Show::Raw => e.kind == Kind::Raw,
            Show::Jpeg => matches!(e.ext.as_str(), "jpg" | "jpeg" | "jpe" | "jfif"),
            Show::Png => matches!(e.ext.as_str(), "png" | "apng"),
            Show::Gif => e.ext == "gif",
            Show::Webp => e.ext == "webp",
            Show::Tiff => matches!(e.ext.as_str(), "tif" | "tiff"),
            Show::Documents => e.kind == Kind::Document,
            Show::Models => e.kind == Kind::Model,
        }
    }
}

/// How the list is ordered. Folders always come first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Order {
    pub key: SortKey,
    pub descending: bool,
    /// A seed: random order that stays the same for the same seed (shared with the viewer through `--shuffle`).
    pub shuffle: Option<u64>,
}

fn compare(a: &Entry, b: &Entry, o: Order) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.is_dir(), b.is_dir()) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    let by_name = || natural_cmp(&a.name, &b.name).then_with(|| a.name.cmp(&b.name));
    if let (Some(seed), false) = (o.shuffle, a.is_dir()) {
        return shuffle_key(&a.path, seed).cmp(&shuffle_key(&b.path, seed)).then_with(by_name);
    }
    let key = match o.key {
        SortKey::Name => by_name(),
        SortKey::Modified => a.modified.cmp(&b.modified).then_with(by_name),
        SortKey::Size => a.size.cmp(&b.size).then_with(by_name),
        SortKey::Type => a.ext.cmp(&b.ext).then_with(by_name),
    };
    if o.descending { key.reverse() } else { key }
}

/// Indices into `all` of what is shown, in order: filtered by `show` (folders stay unless `folders` is off), by the
/// search words (every word must be in the name), then sorted.
pub fn arrange(all: &[Entry], show: Show, folders: bool, search: &str, order: Order) -> Vec<usize> {
    let words: Vec<String> = search.split_whitespace().map(|w| w.to_lowercase()).collect();
    let mut idx: Vec<usize> = (0..all.len())
        .filter(|&i| {
            let e = &all[i];
            let pass = if e.is_dir() { folders } else { show.accepts(e) };
            pass && (words.is_empty() || {
                let lower = e.name.to_lowercase();
                words.iter().all(|w| lower.contains(w.as_str()))
            })
        })
        .collect();
    idx.sort_by(|&a, &b| compare(&all[a], &all[b], order));
    idx
}

/// Fold runs of numbered pictures (`shot_0001.png`, `shot_0002.png` ...) into one tile each: `view` is the list in order (indices
/// into `all`); returns the list with every run reduced to its first frame, and for each run (keyed by the first frame's entry
/// index) all its frames' entry indices. Only pictures of the same kind and extension are ever folded together, so a RAW and
/// its JPG are never merged.
pub fn fold_sequences(all: &[Entry], view: &[usize], min: usize) -> (Vec<usize>, std::collections::HashMap<usize, Vec<usize>>) {
    let paths: Vec<PathBuf> = view.iter().map(|&i| if all[i].kind == Kind::Picture { all[i].path.clone() } else { PathBuf::new() }).collect();
    let runs = nv_core::sequence::find_runs(&paths, min);
    let mut kept = Vec::with_capacity(view.len());
    let mut members = std::collections::HashMap::new();
    let (mut i, mut r) = (0, 0);
    while i < view.len() {
        if r < runs.len() && runs[r].start == i {
            kept.push(view[i]);
            members.insert(view[i], view[runs[r].start..=runs[r].end].to_vec());
            i = runs[r].end + 1;
            r += 1;
        } else {
            kept.push(view[i]);
            i += 1;
        }
    }
    (kept, members)
}

/// The folder being shown.
pub struct Listing {
    pub dir: PathBuf,
    pub all: Vec<Entry>,
    /// Indices into `all`, filtered and sorted.
    pub view: Vec<usize>,
    pub show: Show,
    pub folders: bool,
    pub search: String,
    pub order: Order,
    /// Fold image sequences of at least this many frames into one tile (only while sorted by name, ascending).
    pub fold: Option<usize>,
    /// The folded runs: entry index of the first frame -> entry indices of all its frames.
    pub runs: std::collections::HashMap<usize, Vec<usize>>,
}

impl Listing {
    pub fn new(show: Show, order: Order) -> Self {
        Listing { dir: PathBuf::new(), all: Vec::new(), view: Vec::new(), show, folders: true, search: String::new(), order, fold: None, runs: Default::default() }
    }

    pub fn set_entries(&mut self, dir: PathBuf, all: Vec<Entry>) {
        self.dir = dir;
        self.all = all;
        self.rearrange();
    }

    pub fn rearrange(&mut self) {
        self.view = arrange(&self.all, self.show, self.folders, &self.search, self.order);
        self.runs.clear();
        if let Some(min) = self.fold.filter(|_| self.order.key == SortKey::Name && !self.order.descending && self.order.shuffle.is_none()) {
            let (kept, runs) = fold_sequences(&self.all, &self.view, min);
            self.view = kept;
            self.runs = runs;
        }
    }

    /// How many frames the tile of entry `id` stands for, if it is a folded sequence.
    pub fn sequence_len(&self, id: u64) -> Option<usize> {
        self.runs.iter().find(|(first, _)| self.all[**first].id == id).map(|(_, v)| v.len())
    }

    /// The paths of every frame of the sequence that entry `id` stands for (None for an ordinary entry).
    pub fn sequence_files(&self, id: u64) -> Option<Vec<PathBuf>> {
        self.runs.iter().find(|(first, _)| self.all[**first].id == id).map(|(_, v)| v.iter().map(|&k| self.all[k].path.clone()).collect())
    }

    pub fn len(&self) -> usize {
        self.view.len()
    }

    pub fn get(&self, i: usize) -> Option<&Entry> {
        self.view.get(i).map(|&k| &self.all[k])
    }

    pub fn position_of(&self, id: u64) -> Option<usize> {
        self.view.iter().position(|&k| self.all[k].id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.view.iter().map(|&k| &self.all[k])
    }

    /// (folders, pictures + RAW, documents, other files, total bytes of the files) of everything in the folder, shown or not.
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for e in &self.all {
            match e.kind {
                Kind::Folder => s.folders += 1,
                Kind::Picture | Kind::Raw | Kind::Model => s.pictures += 1,
                Kind::Document => s.documents += 1,
                Kind::Other => s.other += 1,
            }
            s.bytes += e.size;
        }
        s
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub folders: usize,
    pub pictures: usize,
    pub documents: usize,
    pub other: usize,
    pub bytes: u64,
}

/// Which entries are selected, plus the focused one.
#[derive(Default)]
pub struct Selection {
    pub ids: HashSet<u64>,
    /// The focused item (view index): keyboard moves start here, and it is what the right panel describes.
    pub cursor: Option<usize>,
    /// Where a Shift range starts.
    pub anchor: Option<usize>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.ids.clear();
    }

    pub fn is_selected(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    pub fn only(&mut self, l: &Listing, i: usize) {
        self.ids.clear();
        if let Some(e) = l.get(i) {
            self.ids.insert(e.id);
            self.cursor = Some(i);
            self.anchor = Some(i);
        }
    }

    pub fn toggle(&mut self, l: &Listing, i: usize) {
        if let Some(e) = l.get(i) {
            if !self.ids.remove(&e.id) {
                self.ids.insert(e.id);
            }
            self.cursor = Some(i);
            self.anchor = Some(i);
        }
    }

    /// Select the range from the anchor to `i` (replacing the selection; `add` keeps what was selected).
    pub fn range_to(&mut self, l: &Listing, i: usize, add: bool) {
        let a = self.anchor.unwrap_or(i).min(l.len().saturating_sub(1));
        if !add {
            self.ids.clear();
        }
        let (lo, hi) = (a.min(i), a.max(i));
        for k in lo..=hi.min(l.len().saturating_sub(1)) {
            if let Some(e) = l.get(k) {
                self.ids.insert(e.id);
            }
        }
        self.cursor = Some(i);
    }

    pub fn all(&mut self, l: &Listing) {
        self.ids = l.iter().map(|e| e.id).collect();
    }

    pub fn invert(&mut self, l: &Listing) {
        self.ids = l.iter().map(|e| e.id).filter(|id| !self.ids.contains(id)).collect();
    }

    /// The selected entries in list order. With nothing selected, the focused one (so a menu command on a single
    /// item works without clicking it first).
    pub fn entries<'a>(&self, l: &'a Listing) -> Vec<&'a Entry> {
        let picked: Vec<&Entry> = l.iter().filter(|e| self.ids.contains(&e.id)).collect();
        if picked.is_empty() {
            self.cursor.and_then(|c| l.get(c)).into_iter().collect()
        } else {
            picked
        }
    }

    pub fn count(&self, l: &Listing) -> usize {
        l.iter().filter(|e| self.ids.contains(&e.id)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn entry(name: &str, kind: Kind, size: u64, modified: i64) -> Entry {
        let path = PathBuf::from(format!("C:\\t\\{name}"));
        let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        Entry { id: path_id(&path), path, name: name.to_string(), ext, kind, size, modified }
    }

    fn sample() -> Vec<Entry> {
        vec![
            entry("img10.jpg", Kind::Picture, 300, 30),
            entry("img2.jpg", Kind::Picture, 100, 10),
            entry("Sub", Kind::Folder, 0, 99),
            entry("raw1.raf", Kind::Raw, 900, 20),
            entry("notes.txt", Kind::Other, 5, 50),
            entry("a.png", Kind::Picture, 200, 40),
        ]
    }

    fn names(all: &[Entry], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| all[i].name.clone()).collect()
    }

    #[test]
    fn numbered_pictures_fold_into_one_tile_and_raw_pairs_never_do() {
        let mut all = Vec::new();
        for n in 1..=12 {
            all.push(entry(&format!("shot_{n:04}.png"), Kind::Picture, 10, n));
        }
        all.push(entry("DSCF0001.RAF", Kind::Raw, 10, 1));
        all.push(entry("DSCF0001.JPG", Kind::Picture, 10, 1));
        all.push(entry("DSCF0002.RAF", Kind::Raw, 10, 1));
        all.push(entry("DSCF0002.JPG", Kind::Picture, 10, 1));
        all.push(entry("cover.png", Kind::Picture, 10, 1));
        let order = Order::default();
        let view = arrange(&all, Show::Media, true, "", order);
        let (kept, runs) = fold_sequences(&all, &view, 10);
        assert_eq!(kept.len(), view.len() - 11, "12 frames become one tile");
        assert_eq!(runs.len(), 1);
        let first = *runs.keys().next().unwrap();
        assert_eq!(all[first].name, "shot_0001.png");
        assert_eq!(runs[&first].len(), 12);
        // The RAW / JPG pairs are all still there, separately.
        for name in ["DSCF0001.RAF", "DSCF0001.JPG", "DSCF0002.RAF", "DSCF0002.JPG"] {
            assert!(kept.iter().any(|&i| all[i].name == name), "{name} must stay listed");
        }
        // Fewer frames than the minimum: nothing folds.
        assert_eq!(fold_sequences(&all, &view, 13).0.len(), view.len());
    }

    #[test]
    fn a_listing_folds_only_while_sorted_by_name_upwards() {
        let mut l = Listing::new(Show::Media, Order::default());
        let all: Vec<Entry> = (1..=10).map(|n| entry(&format!("f_{n:03}.png"), Kind::Picture, 1, n)).collect();
        l.fold = Some(5);
        l.set_entries(PathBuf::from("C:	"), all);
        assert_eq!(l.len(), 1);
        let id = l.get(0).unwrap().id;
        assert_eq!(l.sequence_len(id), Some(10));
        assert_eq!(l.sequence_files(id).map(|v| v.len()), Some(10));
        l.order.key = SortKey::Size;
        l.rearrange();
        assert_eq!(l.len(), 10, "another order shows every picture");
        assert!(l.runs.is_empty());
    }

    #[test]
    fn kinds_follow_the_extension() {
        assert_eq!(kind_of_ext("JPG"), Kind::Picture);
        assert_eq!(kind_of_ext("raf"), Kind::Raw);
        assert_eq!(kind_of_ext("pdf"), Kind::Document);
        assert_eq!(kind_of_ext("stl"), Kind::Model);
        assert!(matches!(kind_of_ext("txt"), Kind::Document | Kind::Other), "plain text gets a card when documents are listed");
        assert_eq!(kind_of_ext("xyz"), Kind::Other);
    }

    #[test]
    fn folders_first_then_natural_name_order() {
        let all = sample();
        let v = arrange(&all, Show::Media, true, "", Order::default());
        assert_eq!(names(&all, &v), ["Sub", "a.png", "img2.jpg", "img10.jpg", "raw1.raf"]);
        let v = arrange(&all, Show::Media, true, "", Order { descending: true, ..Order::default() });
        assert_eq!(names(&all, &v), ["Sub", "raw1.raf", "img10.jpg", "img2.jpg", "a.png"], "descending keeps folders on top");
    }

    #[test]
    fn other_sort_keys() {
        let all = sample();
        let by = |key| names(&all, &arrange(&all, Show::All, true, "", Order { key, ..Order::default() }));
        assert_eq!(by(SortKey::Size), ["Sub", "notes.txt", "img2.jpg", "a.png", "img10.jpg", "raw1.raf"]);
        assert_eq!(by(SortKey::Modified), ["Sub", "img2.jpg", "raw1.raf", "img10.jpg", "a.png", "notes.txt"]);
        assert_eq!(by(SortKey::Type), ["Sub", "img2.jpg", "img10.jpg", "a.png", "raw1.raf", "notes.txt"]);
    }

    #[test]
    fn show_filters_and_search() {
        let all = sample();
        let shown = |show, folders, q: &str| names(&all, &arrange(&all, show, folders, q, Order::default()));
        assert_eq!(shown(Show::Media, true, ""), ["Sub", "a.png", "img2.jpg", "img10.jpg", "raw1.raf"]);
        assert_eq!(shown(Show::All, false, ""), ["a.png", "img2.jpg", "img10.jpg", "notes.txt", "raw1.raf"]);
        assert_eq!(shown(Show::Raw, true, ""), ["Sub", "raw1.raf"]);
        assert_eq!(shown(Show::Jpeg, false, ""), ["img2.jpg", "img10.jpg"]);
        assert_eq!(shown(Show::Media, false, "IMG 10"), ["img10.jpg"], "all words, any case");
        assert_eq!(shown(Show::Media, true, "zzz"), Vec::<String>::new());
    }

    #[test]
    fn shuffle_matches_the_viewers_order_and_keeps_folders_first() {
        let all = sample();
        let o = Order { shuffle: Some(5), ..Order::default() };
        let a = arrange(&all, Show::All, true, "", o);
        let b = arrange(&all, Show::All, true, "", o);
        assert_eq!(a, b);
        assert_eq!(all[a[0]].name, "Sub");
        let mut files: Vec<PathBuf> = all.iter().filter(|e| !e.is_dir()).map(|e| e.path.clone()).collect();
        nv_core::folder::sort_files(&mut files, nv_core::Order { shuffle: Some(5), ..Default::default() });
        let ours: Vec<PathBuf> = a.iter().filter(|&&i| !all[i].is_dir()).map(|&i| all[i].path.clone()).collect();
        assert_eq!(ours, files, "the viewer, given the same seed, walks the files in the order the gallery shows");
    }

    #[test]
    fn selection_modes() {
        let all = sample();
        let mut l = Listing::new(Show::Media, Order::default());
        l.set_entries(PathBuf::from("C:\\t"), all);
        let mut s = Selection::default();
        s.only(&l, 1);
        assert_eq!(s.count(&l), 1);
        s.toggle(&l, 3);
        assert_eq!(s.count(&l), 2);
        s.toggle(&l, 1);
        assert_eq!(s.count(&l), 1);
        s.only(&l, 1);
        s.range_to(&l, 3, false);
        assert_eq!(s.count(&l), 3);
        s.invert(&l);
        assert_eq!(s.count(&l), 2);
        s.all(&l);
        assert_eq!(s.count(&l), 5);
        s.clear();
        s.cursor = Some(2);
        assert_eq!(s.entries(&l).len(), 1, "no selection: the focused item");
        assert_eq!(s.entries(&l)[0].name, "img2.jpg");
    }

    #[test]
    fn scanning_a_real_folder() {
        let d = std::env::temp_dir().join(format!("nv_gal_scan_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.jpg"), b"12345").unwrap();
        fs::write(d.join("b.raf"), b"1").unwrap();
        fs::write(d.join("c.xyz"), b"1").unwrap();
        let mut es = scan(&d, false).unwrap();
        es.sort_by(|a, b| a.name.cmp(&b.name));
        let got: Vec<(String, Kind, u64)> = es.iter().map(|e| (e.name.clone(), e.kind, e.size)).collect();
        assert_eq!(got, [("a.jpg".into(), Kind::Picture, 5), ("b.raf".into(), Kind::Raw, 1), ("c.xyz".into(), Kind::Other, 1), ("sub".into(), Kind::Folder, 0)]);
        assert!(es.iter().all(|e| e.modified > 1_600_000_000));
        assert!(scan(&d.join("nope"), false).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_gets_the_first_picture_in_it_as_its_cover() {
        let d = std::env::temp_dir().join(format!("nv_gal_cover_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("empty")).unwrap();
        fs::create_dir_all(d.join("pics")).unwrap();
        fs::write(d.join("pics").join("notes.txt"), "x").unwrap();
        assert!(thumb_work(&d.join("empty")).is_err());
        assert!(thumb_work(&d.join("pics")).is_err(), "text is not a cover");
        // Two PNGs: b is red, a is blue; the cover is the first by name (a).
        let make = |name: &str, c: [u8; 3]| {
            let mut rgba = Vec::new();
            for _ in 0..64 {
                rgba.extend([c[0], c[1], c[2], 255]);
            }
            let px = nv_editor::Pixels::new(8, 8, rgba);
            fs::write(d.join("pics").join(name), nv_editor::save::encode(&px, nv_editor::save::Format::Png, None).unwrap()).unwrap();
        };
        make("b.png", [255, 0, 0]);
        make("a.png", [0, 0, 255]);
        let t = thumb_work(&d.join("pics")).unwrap();
        assert!(t.rgba[2] > 200 && t.rgba[0] < 60, "the cover is a.png (blue): {:?}", &t.rgba[..4]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn ids_ignore_case() {
        assert_eq!(path_id(Path::new("C:\\A\\b.JPG")), path_id(Path::new("c:\\a\\B.jpg")));
    }
}
