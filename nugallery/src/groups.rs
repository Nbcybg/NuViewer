//! Group view: the files of a folder cut into labelled groups (by type, application, name, date or size), in ascending or
//! descending order of the group. Inside a group the files keep the ordinary sort. No drawing and no window in here.

use crate::model::{Entry, Kind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GroupBy {
    /// No groups (the default).
    #[default]
    None,
    /// The file type: JPG, PNG, PSD ...
    Type,
    /// The program the file belongs to: Photoshop, Clip Studio Paint, Blender, a camera maker ...
    App,
    /// The first letter of the name.
    Name,
    /// When it was last changed: today, yesterday, last 7 days ... then month by month.
    Date,
    /// How big it is: tiny, small, medium ...
    Size,
}

impl GroupBy {
    pub const ALL: [GroupBy; 6] = [GroupBy::None, GroupBy::Type, GroupBy::App, GroupBy::Name, GroupBy::Date, GroupBy::Size];

    pub fn id(self) -> &'static str {
        match self {
            GroupBy::None => "none",
            GroupBy::Type => "type",
            GroupBy::App => "app",
            GroupBy::Name => "name",
            GroupBy::Date => "date",
            GroupBy::Size => "size",
        }
    }

    pub fn from_id(v: &str) -> Option<GroupBy> {
        let v = v.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|g| g.id() == v)
    }

    pub fn label(self) -> &'static str {
        match self {
            GroupBy::None => "(None)",
            GroupBy::Type => "Type",
            GroupBy::App => "App",
            GroupBy::Name => "Name",
            GroupBy::Date => "Date",
            GroupBy::Size => "Size",
        }
    }

    /// The next choice in the cycle None > Type > App > Name > Date > Size > None.
    pub fn next(self) -> GroupBy {
        let i = Self::ALL.iter().position(|g| *g == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// The group choice: what to group by and in which direction the groups run.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Group {
    pub by: GroupBy,
    pub descending: bool,
}

impl Group {
    pub fn on(self) -> bool {
        self.by != GroupBy::None
    }
}

/// Where a group header starts in the (arranged) list.
#[derive(Clone, PartialEq, Debug)]
pub struct GroupSpan {
    pub start: usize,
    pub label: String,
    pub count: usize,
}

/// The clock the date groups are relative to.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    pub now: i64,
    pub utc_offset_minutes: i32,
}

impl Clock {
    pub fn local() -> Clock {
        Clock { now: nv_core::batch::unix_of(std::time::SystemTime::now()), utc_offset_minutes: nv_win::utc_offset_minutes() }
    }

    fn day(&self, t: i64) -> i64 {
        (t + self.utc_offset_minutes as i64 * 60).div_euclid(86_400)
    }
}

/// Sort position and header text of a group. Groups are ordered by `rank`, then by `label`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Key {
    pub rank: i64,
    pub label: String,
}

fn key(rank: i64, label: impl Into<String>) -> Key {
    Key { rank, label: label.into() }
}

/// The group of an entry. Folders are always the group "Folders" (which the caller keeps in front).
pub fn key_of(e: &Entry, by: GroupBy, clock: Clock) -> Key {
    if e.kind == Kind::Folder {
        return key(i64::MIN, "Folders");
    }
    match by {
        GroupBy::None => key(0, ""),
        GroupBy::Type => key(0, type_name(e)),
        GroupBy::App => {
            let app = app_of_ext(&e.ext);
            key(if app == OTHER_APP { 1 } else { 0 }, app)
        }
        GroupBy::Name => name_key(&e.name),
        GroupBy::Date => date_key(e.modified, clock),
        GroupBy::Size => size_key(e.size),
    }
}

/// "JPG", "PNG", "PSD"; the spellings of one format are one group.
pub fn type_name(e: &Entry) -> String {
    if e.ext.is_empty() {
        return "No extension".into();
    }
    let canon = match e.ext.as_str() {
        "jpeg" | "jpe" | "jfif" | "jif" => "jpg",
        "tif" => "tiff",
        "htm" | "xhtml" => "html",
        "mpeg" => "mpg",
        other => other,
    };
    canon.to_uppercase()
}

// ---- application ---------------------------------------------------------------------------------------------------

pub const OTHER_APP: &str = "Other";

/// Which program a file belongs to (by its extension): the one that made it, or the one it is meant for.
pub fn app_of_ext(ext: &str) -> &'static str {
    match ext {
        "psd" | "psb" | "pat" | "abr" => "Adobe Photoshop",
        "ai" | "eps" => "Adobe Illustrator",
        "indd" | "idml" => "Adobe InDesign",
        "pdf" => "PDF document",
        "fla" | "xfl" | "swf" | "as" => "Adobe Animate (Flash)",
        "aep" | "aepx" | "prproj" => "Adobe After Effects / Premiere",
        "clip" | "csp" | "cmc" | "lip" => "Clip Studio Paint",
        "sai" | "sai2" => "PaintTool SAI",
        "kra" | "krz" => "Krita",
        "xcf" => "GIMP",
        "pdn" => "Paint.NET",
        "procreate" => "Procreate",
        "afphoto" | "afdesign" | "afpub" => "Affinity",
        "sketch" | "fig" | "xd" => "UI design tools",
        "ora" | "mdp" | "medibang" => "Open raster / MediBang",
        "blend" => "Blender",
        "max" | "3ds" | "ma" | "mb" | "fbx" | "dae" => "Autodesk (Maya / 3ds Max)",
        "c4d" => "Cinema 4D",
        "skp" => "SketchUp",
        "vrm" | "vroid" => "VRoid / VRM avatar",
        "3mf" | "gcode" | "stl" => "3D printing",
        "obj" | "ply" | "gltf" | "glb" | "usd" | "usdz" | "x3d" | "wrl" | "vrml" | "off" => "3D models (any viewer)",
        "duf" | "dsf" | "daz" => "DAZ Studio",
        // Animation software.
        "skel" | "spine" | "atlas" | "spinejson" => "Spine",
        "unity3d" | "unityfs" | "bundle" | "ab" | "assetbundle" => "Unity bundles",
        "xstage" | "tpl" | "sbpz" | "sbp" | "sbz" | "xsb" => "Toon Boom",
        "moho" | "anme" | "mohoproj" => "Moho",
        "tvpp" | "tvp" => "TVPaint",
        "rig" | "pegtool" => "Animation rigs",
        // Camera makers (RAW).
        "cr2" | "cr3" | "crw" => "Canon (RAW)",
        "nef" | "nrw" => "Nikon (RAW)",
        "arw" | "srf" | "sr2" => "Sony (RAW)",
        "raf" => "Fujifilm (RAW)",
        "orf" => "Olympus / OM System (RAW)",
        "rw2" | "raw" => "Panasonic / Leica (RAW)",
        "pef" | "ptx" => "Pentax (RAW)",
        "srw" => "Samsung (RAW)",
        "3fr" | "fff" => "Hasselblad (RAW)",
        "iiq" | "cap" => "Phase One (RAW)",
        "x3f" => "Sigma (RAW)",
        "dng" => "Adobe DNG (RAW)",
        "dcr" | "kdc" | "k25" => "Kodak (RAW)",
        "mrw" => "Minolta (RAW)",
        "erf" => "Epson (RAW)",
        "mef" => "Mamiya (RAW)",
        "r3d" | "braw" | "mxf" => "Cinema camera",
        // Office and text.
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "vsd" | "vsdx" => "Microsoft Office",
        "odt" | "ods" | "odp" | "odg" => "LibreOffice",
        "html" | "htm" | "xhtml" | "mhtml" | "mht" | "svg" => "Web browser",
        "txt" | "md" | "markdown" | "json" | "xml" | "csv" | "log" | "ini" | "yaml" | "yml" | "toml" | "js" | "ts" | "css" | "rs" | "py" | "c" | "h" | "cpp" | "cs" | "java" | "bat" | "ps1" | "sh" => "Text editor",
        "ttf" | "otf" | "woff" | "woff2" | "ttc" | "eot" => "Fonts",
        "zip" | "7z" | "rar" | "cbz" | "cbr" | "tar" | "gz" | "tgz" | "iso" => "Archives",
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "m4v" | "wmv" | "flv" | "f4v" | "mpg" | "mpeg" | "3gp" | "ogv" => "Video player",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" | "opus" | "mid" | "midi" => "Audio player",
        "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "apng" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "avif" | "heic" | "heif" | "hif" | "ico" | "icns" | "cur" | "jxl" | "jp2" | "j2k" | "tga" | "dds" | "exr" | "hdr" | "qoi" | "pcx" | "pnm" | "ppm" | "pgm" | "pbm" | "flif" | "dib" | "emf" | "wmf" => "Any image viewer",
        _ => OTHER_APP,
    }
}

// ---- name ----------------------------------------------------------------------------------------------------------

fn is_thai_leading_vowel(c: char) -> bool {
    matches!(c, 'เ' | 'แ' | 'โ' | 'ใ' | 'ไ')
}

fn name_key(name: &str) -> Key {
    // The first character that says something: Thai names are shelved by their first consonant, not the vowel before it.
    let stem = std::path::Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Some(c) = stem.chars().find(|c| c.is_alphanumeric() && !is_thai_leading_vowel(*c)).or_else(|| stem.chars().find(|c| c.is_alphanumeric())) else {
        return key(4, "#");
    };
    if c.is_ascii_digit() {
        key(0, "0-9")
    } else if c.is_ascii_alphabetic() {
        key(1, c.to_ascii_uppercase().to_string())
    } else if ('\u{0E01}'..='\u{0E5B}').contains(&c) {
        key(3, c.to_string())
    } else if c.is_alphabetic() {
        // Accented Latin letters sit with their base letter's neighbours; everything else by its own letter.
        key(2, c.to_uppercase().collect::<String>())
    } else {
        key(4, "#")
    }
}

// ---- date ----------------------------------------------------------------------------------------------------------

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn date_key(modified: i64, clock: Clock) -> Key {
    let today = clock.day(clock.now);
    let day = clock.day(modified);
    let ago = today - day;
    // The rank is the first day of the bucket, so buckets run in time order.
    if ago <= 0 {
        key(today, "Today")
    } else if ago == 1 {
        key(today - 1, "Yesterday")
    } else if ago < 7 {
        key(today - 6, "Last 7 days")
    } else if ago < 30 {
        key(today - 29, "Last 30 days")
    } else {
        let (y, m, _, _, _, _) = nv_core::batch::civil_from_unix(modified, clock.utc_offset_minutes);
        let first = nv_core::batch::days_from_civil(y, m, 1);
        key(first, format!("{} {y}", MONTHS[(m as usize - 1).min(11)]))
    }
}

// ---- size ----------------------------------------------------------------------------------------------------------

fn size_key(size: u64) -> Key {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    match size {
        0 => key(0, "Empty (0 bytes)"),
        s if s < 16 * KB => key(1, "Tiny (under 16 KB)"),
        s if s < MB => key(2, "Small (16 KB – 1 MB)"),
        s if s < 16 * MB => key(3, "Medium (1 – 16 MB)"),
        s if s < 128 * MB => key(4, "Large (16 – 128 MB)"),
        s if s < GB => key(5, "Huge (128 MB – 1 GB)"),
        _ => key(6, "Gigantic (over 1 GB)"),
    }
}

// ---- ordering ------------------------------------------------------------------------------------------------------

/// Compare two group keys the way the view runs: folders first whatever the direction, then ascending or descending.
pub fn compare_keys(a: &Key, b: &Key, descending: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.rank == i64::MIN, b.rank == i64::MIN) {
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        (true, true) => return Ordering::Equal,
        _ => {}
    }
    let o = a.rank.cmp(&b.rank).then_with(|| a.label.cmp(&b.label));
    if descending { o.reverse() } else { o }
}

/// The header strips of an arranged list: one span per run of equal group labels.
pub fn spans(entries: impl Iterator<Item = Key>) -> Vec<GroupSpan> {
    let mut out: Vec<GroupSpan> = Vec::new();
    for (i, k) in entries.enumerate() {
        match out.last_mut() {
            Some(s) if s.label == k.label => s.count += 1,
            _ => out.push(GroupSpan { start: i, label: k.label, count: 1 }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn e(name: &str, size: u64, modified: i64) -> Entry {
        let ext = std::path::Path::new(name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let kind = if name.ends_with('/') { Kind::Folder } else { crate::model::kind_of_ext(&ext) };
        Entry { id: crate::model::path_id(&PathBuf::from(name)), path: PathBuf::from(name), name: name.to_string(), ext, kind, size, modified }
    }

    // 2025-06-15 12:00:00 UTC, no offset.
    const NOW: i64 = 1_749_988_800;
    const CLOCK: Clock = Clock { now: NOW, utc_offset_minutes: 0 };
    const DAY: i64 = 86_400;

    #[test]
    fn types_merge_spellings() {
        assert_eq!(type_name(&e("a.jpeg", 1, 0)), "JPG");
        assert_eq!(type_name(&e("a.JPG", 1, 0)), "JPG");
        assert_eq!(type_name(&e("a.tif", 1, 0)), "TIFF");
        assert_eq!(type_name(&e("noext", 1, 0)), "No extension");
    }

    #[test]
    fn apps_are_named_by_extension() {
        assert_eq!(app_of_ext("psd"), "Adobe Photoshop");
        assert_eq!(app_of_ext("clip"), "Clip Studio Paint");
        assert_eq!(app_of_ext("raf"), "Fujifilm (RAW)");
        assert_eq!(app_of_ext("jpg"), "Any image viewer");
        assert_eq!(app_of_ext("whatever"), OTHER_APP);
        // "Other" runs after the named apps when ascending.
        assert!(key_of(&e("a.zzz", 1, 0), GroupBy::App, CLOCK).rank > key_of(&e("a.psd", 1, 0), GroupBy::App, CLOCK).rank);
    }

    #[test]
    fn names_group_by_first_letter_digits_and_thai() {
        let k = |n: &str| key_of(&e(n, 1, 0), GroupBy::Name, CLOCK);
        assert_eq!(k("apple.png").label, "A");
        assert_eq!(k("Zebra.png").label, "Z");
        assert_eq!(k("007.png").label, "0-9");
        assert_eq!(k("_x.png").label, "X");
        assert_eq!(k("---.png").label, "#");
        // A leading Thai vowel is skipped: เกม is shelved under ก.
        assert_eq!(k("เกม.png").label, "ก");
        assert_eq!(k("ไทย.png").label, "ท");
        // Digits, then Latin, then Thai.
        assert!(compare_keys(&k("1.png"), &k("a.png"), false).is_lt());
        assert!(compare_keys(&k("a.png"), &k("ก.png"), false).is_lt());
    }

    #[test]
    fn dates_fall_into_time_buckets_that_run_in_order() {
        let k = |t: i64| key_of(&e("a.png", 1, t), GroupBy::Date, CLOCK);
        assert_eq!(k(NOW - 3600).label, "Today");
        assert_eq!(k(NOW - DAY).label, "Yesterday");
        assert_eq!(k(NOW - 3 * DAY).label, "Last 7 days");
        assert_eq!(k(NOW - 10 * DAY).label, "Last 30 days");
        assert_eq!(k(NOW - 40 * DAY).label, "May 2025");
        assert_eq!(k(NOW - 400 * DAY).label, "May 2024");
        // Oldest first when ascending.
        let order: Vec<String> = {
            let mut v: Vec<Key> = [NOW, NOW - DAY, NOW - 3 * DAY, NOW - 10 * DAY, NOW - 40 * DAY, NOW - 400 * DAY].map(k).to_vec();
            v.sort_by(|a, b| compare_keys(a, b, false));
            v.into_iter().map(|k| k.label).collect()
        };
        assert_eq!(order, ["May 2024", "May 2025", "Last 30 days", "Last 7 days", "Yesterday", "Today"]);
    }

    #[test]
    fn the_local_day_decides_not_the_utc_day() {
        // 23:30 UTC on June 14 is already June 15 in UTC+7.
        let late = 1_749_940_200; // 2025-06-14 22:30 UTC
        let c = Clock { now: NOW, utc_offset_minutes: 7 * 60 };
        assert_eq!(key_of(&e("a.png", 1, late), GroupBy::Date, c).label, "Today");
        assert_eq!(key_of(&e("a.png", 1, late), GroupBy::Date, CLOCK).label, "Yesterday");
    }

    #[test]
    fn sizes_fall_into_the_stated_buckets() {
        let l = |s: u64| key_of(&e("a.png", s, 0), GroupBy::Size, CLOCK).label;
        assert_eq!(l(0), "Empty (0 bytes)");
        assert_eq!(l(100), "Tiny (under 16 KB)");
        assert_eq!(l(16 * 1024), "Small (16 KB – 1 MB)");
        assert_eq!(l(5 * 1024 * 1024), "Medium (1 – 16 MB)");
        assert_eq!(l(100 * 1024 * 1024), "Large (16 – 128 MB)");
        assert_eq!(l(500 * 1024 * 1024), "Huge (128 MB – 1 GB)");
        assert_eq!(l(3 * 1024 * 1024 * 1024), "Gigantic (over 1 GB)");
    }

    #[test]
    fn folders_stay_in_front_both_ways() {
        let f = key_of(&e("Sub/", 0, 0), GroupBy::Size, CLOCK);
        let s = key_of(&e("a.png", 5, 0), GroupBy::Size, CLOCK);
        assert!(compare_keys(&f, &s, false).is_lt());
        assert!(compare_keys(&f, &s, true).is_lt());
        assert!(compare_keys(&s, &f, true).is_gt());
    }

    #[test]
    fn descending_reverses_the_groups() {
        let a = key_of(&e("a.png", 5, 0), GroupBy::Size, CLOCK);
        let b = key_of(&e("b.png", 5_000_000, 0), GroupBy::Size, CLOCK);
        assert!(compare_keys(&a, &b, false).is_lt());
        assert!(compare_keys(&a, &b, true).is_gt());
    }

    #[test]
    fn spans_cut_runs_of_equal_labels() {
        let ks = ["A", "A", "B", "C", "C", "C"].map(|l| key(0, l));
        let s = spans(ks.into_iter());
        assert_eq!(s.iter().map(|x| (x.start, x.count)).collect::<Vec<_>>(), [(0, 2), (2, 1), (3, 3)]);
        assert!(spans(std::iter::empty()).is_empty());
    }

    #[test]
    fn ids_round_trip() {
        for g in GroupBy::ALL {
            assert_eq!(GroupBy::from_id(g.id()), Some(g));
        }
        assert_eq!(GroupBy::from_id("nonsense"), None);
        assert_eq!(GroupBy::Size.next(), GroupBy::None);
    }
}
