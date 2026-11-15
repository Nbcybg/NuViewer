//! What the importer would do, as plain data and functions (no window): the files found on a source, the sub-folder and the
//! name each one would get, and a summary line. The mock-up shows these; the real copy will carry them out.

use nv_core::batch::civil_from_unix;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    /// When it was taken (EXIF is read by the real importer; the mock-up uses the file's own time), seconds since 1970 UTC.
    pub taken: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Flat,
    Day,
    YearMonth,
    YearMonthDay,
}

impl Layout {
    pub const ALL: [Layout; 4] = [Layout::Flat, Layout::Day, Layout::YearMonth, Layout::YearMonthDay];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Flat => "Straight into the folder",
            Layout::Day => "One folder per day",
            Layout::YearMonth => "Year \\ month",
            Layout::YearMonthDay => "Year \\ month \\ day",
        }
    }

    /// The sub-folder (relative, with `\`) a picture taken at local date y-m-d goes to. `event` is appended to the day's folder.
    pub fn folder(self, (y, m, d): (i64, i64, i64), event: &str) -> String {
        let tag = if event.is_empty() { String::new() } else { format!(" {}", clean(event)) };
        match self {
            Layout::Flat => if event.is_empty() { String::new() } else { clean(event) },
            Layout::Day => format!("{y:04}-{m:02}-{d:02}{tag}"),
            Layout::YearMonth => format!("{y:04}\\{m:02}{tag}"),
            Layout::YearMonthDay => format!("{y:04}\\{m:02}\\{d:02}{tag}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Naming {
    Keep,
    DateTimeName,
    DateNumber,
}

impl Naming {
    pub const ALL: [Naming; 3] = [Naming::Keep, Naming::DateTimeName, Naming::DateNumber];

    pub fn label(self) -> &'static str {
        match self {
            Naming::Keep => "Keep the camera's names",
            Naming::DateTimeName => "20250612_140233_name",
            Naming::DateNumber => "20250612_0001 (per shot)",
        }
    }
}

pub struct Settings {
    pub dest: PathBuf,
    pub layout: Layout,
    pub naming: Naming,
    pub event: String,
}

/// A folder name without what Windows doesn't allow in one.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect::<String>().trim().to_string()
}

fn local_date(secs: i64) -> (i64, i64, i64) {
    let (y, m, d, ..) = civil_from_unix(secs, utc_offset());
    (y, m, d)
}

fn utc_offset() -> i32 {
    #[cfg(windows)]
    {
        nv_win::utc_offset_minutes()
    }
    #[cfg(not(windows))]
    {
        0
    }
}

pub fn format_time(secs: i64) -> String {
    nv_core::batch::format_local(secs, utc_offset())
}

pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// The name a picture would get. `shot` is the number of its shot (RAW and JPG of one shot share it), 1-based.
pub fn new_name(f: &Found, naming: Naming, shot: usize) -> String {
    let (y, m, d, hh, mm, ss) = civil_from_unix(f.taken, utc_offset());
    let ext = Path::new(&f.name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    match naming {
        Naming::Keep => f.name.clone(),
        Naming::DateTimeName => format!("{y:04}{m:02}{d:02}_{hh:02}{mm:02}{ss:02}_{}", f.name),
        Naming::DateNumber => format!("{y:04}{m:02}{d:02}_{shot:04}{ext}"),
    }
}

/// Shot numbers: files with the same name stem (a RAW and its JPG) share one, counted in the order the shots were taken.
pub fn shot_numbers(found: &[Found]) -> Vec<usize> {
    let stem = |f: &Found| Path::new(&f.name).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut order: Vec<usize> = (0..found.len()).collect();
    order.sort_by(|&a, &b| found[a].taken.cmp(&found[b].taken).then_with(|| found[a].name.cmp(&found[b].name)));
    let mut numbers = vec![0; found.len()];
    let mut by_stem: BTreeMap<String, usize> = BTreeMap::new();
    for i in order {
        let next = by_stem.len() + 1;
        numbers[i] = *by_stem.entry(stem(&found[i])).or_insert(next);
    }
    numbers
}

/// Where each file would go, relative to the destination folder: `folder\new name`.
pub fn targets(found: &[Found], s: &Settings) -> Vec<String> {
    let shots = shot_numbers(found);
    found
        .iter()
        .zip(shots)
        .map(|(f, n)| {
            let folder = s.layout.folder(local_date(f.taken), &s.event);
            let name = new_name(f, s.naming, n);
            if folder.is_empty() { name } else { format!("{folder}\\{name}") }
        })
        .collect()
}

/// (sub-folder, files, bytes) for each folder the files would go to, in name order.
pub fn folders(found: &[Found], s: &Settings) -> Vec<(String, usize, u64)> {
    let mut map: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for (f, t) in found.iter().zip(targets(found, s)) {
        let folder = t.rsplit_once('\\').map(|(a, _)| a.to_string()).unwrap_or_else(|| ".".to_string());
        let e = map.entry(folder).or_default();
        e.0 += 1;
        e.1 += f.size;
    }
    map.into_iter().map(|(k, (n, b))| (k, n, b)).collect()
}

/// The line under the destination.
pub fn summary(found: &[Found], s: &Settings) -> String {
    if found.is_empty() {
        return String::new();
    }
    let f = folders(found, s);
    let example = targets(found, s).into_iter().next().unwrap_or_default();
    format!("{} file{} would go into {} folder{} — for example {}\\{}", found.len(), if found.len() == 1 { "" } else { "s" }, f.len(), if f.len() == 1 { "" } else { "s" }, s.dest.display(), example)
}

fn wanted(ext: &str) -> bool {
    nv_codecs::is_raw_ext(ext) || nv_codecs::is_supported_ext(ext) && !matches!(ext, "pdf" | "ai" | "eps" | "svg" | "ico" | "cur")
}

/// The pictures on a card or in a folder: DCIM when there is one, else the folder itself, up to four levels down.
pub fn scan(root: &Path) -> Vec<Found> {
    let start = if root.join("DCIM").is_dir() { root.join("DCIM") } else { root.to_path_buf() };
    let mut out = Vec::new();
    walk(&start, 0, &mut out);
    out.sort_by(|a, b| a.taken.cmp(&b.taken).then_with(|| a.name.cmp(&b.name)));
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<Found>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() >= 20_000 {
            return;
        }
        let Ok(meta) = e.metadata() else { continue };
        let path = e.path();
        if meta.is_dir() {
            if depth < 4 {
                walk(&path, depth + 1, out);
            }
        } else {
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if wanted(&ext) {
                out.push(Found { name: e.file_name().to_string_lossy().into_owned(), path, size: meta.len(), taken: meta.modified().map(nv_core::batch::unix_of).unwrap_or(0) });
            }
        }
    }
}

/// Made-up files so the window can be tried with no card (a Fujifilm-style card: RAF + JPG pairs over two days).
pub fn sample() -> Vec<Found> {
    let day1 = 1_749_736_800; // 2025-06-12 14:00 UTC
    let day2 = day1 + 86_400;
    let mut v = Vec::new();
    for (i, (base, t)) in [(8252, day1), (8253, day1 + 70), (8254, day1 + 190), (8260, day2), (8261, day2 + 45), (8262, day2 + 300)].into_iter().enumerate() {
        v.push(Found { name: format!("DSCF{base}.RAF"), path: PathBuf::from(format!("sample\\DSCF{base}.RAF")), size: 56_000_000 + i as u64 * 1_300_000, taken: t });
        v.push(Found { name: format!("DSCF{base}.JPG"), path: PathBuf::from(format!("sample\\DSCF{base}.JPG")), size: 16_000_000 + i as u64 * 400_000, taken: t });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(layout: Layout, naming: Naming, event: &str) -> Settings {
        Settings { dest: PathBuf::from("D:\\Photos"), layout, naming, event: event.to_string() }
    }

    #[test]
    fn folders_follow_the_chosen_layout_and_the_event_name() {
        assert_eq!(Layout::Day.folder((2025, 6, 2), ""), "2025-06-02");
        assert_eq!(Layout::Day.folder((2025, 6, 2), "Kyoto"), "2025-06-02 Kyoto");
        assert_eq!(Layout::YearMonthDay.folder((2025, 6, 2), ""), "2025\\06\\02");
        assert_eq!(Layout::YearMonth.folder((2025, 6, 2), "a/b:c"), "2025\\06 abc", "characters Windows refuses are dropped");
        assert_eq!(Layout::Flat.folder((2025, 6, 2), ""), "");
    }

    #[test]
    fn a_raw_and_its_jpg_are_two_files_with_one_shot_number() {
        let found = sample();
        let shots = shot_numbers(&found);
        // DSCF8252.RAF and .JPG are the first two entries and one shot.
        assert_eq!(shots[0], shots[1]);
        assert_ne!(shots[0], shots[2]);
        let names: Vec<String> = targets(&found, &s(Layout::Flat, Naming::DateNumber, "")).iter().take(2).cloned().collect();
        assert!(names[0].ends_with("_0001.RAF") && names[1].ends_with("_0001.JPG"), "{names:?}");
        // Nothing is merged: as many targets as files, and no two the same.
        let t = targets(&found, &s(Layout::Day, Naming::DateNumber, ""));
        assert_eq!(t.len(), found.len());
        let mut u = t.clone();
        u.sort();
        u.dedup();
        assert_eq!(u.len(), t.len());
    }

    #[test]
    fn the_summary_counts_files_and_folders() {
        let found = sample();
        let by_day = folders(&found, &s(Layout::Day, Naming::Keep, ""));
        assert_eq!(by_day.iter().map(|f| f.1).sum::<usize>(), found.len());
        assert!(by_day.len() == 2 || by_day.len() == 3, "two shooting days (a time zone can split the first): {by_day:?}");
        assert!(summary(&found, &s(Layout::Flat, Naming::Keep, "")).starts_with("12 files would go into 1 folder"));
        assert_eq!(summary(&[], &s(Layout::Flat, Naming::Keep, "")), "");
    }

    #[test]
    fn scanning_finds_pictures_below_dcim_and_ignores_other_files() {
        let d = std::env::temp_dir().join(format!("nv_importer_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("DCIM").join("100CANON")).unwrap();
        std::fs::create_dir_all(d.join("Other")).unwrap();
        std::fs::write(d.join("DCIM").join("100CANON").join("IMG_0001.JPG"), b"x").unwrap();
        std::fs::write(d.join("DCIM").join("100CANON").join("IMG_0001.CR2"), b"x").unwrap();
        std::fs::write(d.join("DCIM").join("100CANON").join("notes.txt"), b"x").unwrap();
        std::fs::write(d.join("Other").join("not_from_dcim.jpg"), b"x").unwrap();
        let found = scan(&d);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"IMG_0001.JPG") && names.contains(&"IMG_0001.CR2"), "{names:?}");
        assert!(!names.contains(&"notes.txt") && !names.contains(&"not_from_dcim.jpg"), "{names:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
