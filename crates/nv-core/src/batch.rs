//! Batch operations on many files at once: renaming by a pattern, setting file times. Pure logic (no window), so it is
//! unit-tested here and the gallery only collects the input and shows the plan.

use crate::fileops::valid_file_name;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---- dates -------------------------------------------------------------------------------------------------------

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month, day, hour, minute, second) of a unix time shifted by `utc_offset_minutes`.
pub fn civil_from_unix(secs: i64, utc_offset_minutes: i32) -> (i64, i64, i64, i64, i64, i64) {
    let t = secs + utc_offset_minutes as i64 * 60;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Unix time (UTC) of a local date and time given as "2025-06-01 14:30:05", "2025-06-01 14:30", "2025-06-01",
/// "2025/06/01 14:30" or "20250601T143005". None if it isn't a real date.
pub fn parse_local_datetime(text: &str, utc_offset_minutes: i32) -> Option<i64> {
    let digits: Vec<i64> = {
        let mut out = Vec::new();
        let mut cur: Option<i64> = None;
        for c in text.trim().chars() {
            if let Some(d) = c.to_digit(10) {
                cur = Some(cur.unwrap_or(0) * 10 + d as i64);
            } else if let Some(v) = cur.take() {
                out.push(v);
            }
        }
        out.extend(cur);
        out
    };
    let (y, mo, d, h, mi, s) = match digits.as_slice() {
        [y, mo, d] => (*y, *mo, *d, 0, 0, 0),
        [y, mo, d, h, mi] => (*y, *mo, *d, *h, *mi, 0),
        [y, mo, d, h, mi, s] => (*y, *mo, *d, *h, *mi, *s),
        // "20250601T143005" has no separators inside the date part.
        [packed] if *packed >= 1_000_0101 && *packed <= 9999_1231 => (packed / 10_000, packed / 100 % 100, packed % 100, 0, 0, 0),
        [packed, hms] if *packed >= 1_000_0101 && *packed <= 9999_1231 && *hms < 240_000 => (packed / 10_000, packed / 100 % 100, packed % 100, hms / 10_000, hms / 100 % 100, hms % 100),
        _ => return None,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 59 || !(1601..=9999).contains(&y) {
        return None;
    }
    // Reject 31 February and friends: converting back must give the same day.
    let t = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - utc_offset_minutes as i64 * 60;
    let back = civil_from_unix(t, utc_offset_minutes);
    (back.0 == y && back.1 == mo && back.2 == d).then_some(t)
}

/// "2025-06-01 14:30:05" in local time.
pub fn format_local(secs: i64, utc_offset_minutes: i32) -> String {
    let (y, mo, d, h, mi, s) = civil_from_unix(secs, utc_offset_minutes);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

pub fn unix_of(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

pub fn system_time_of(secs: i64) -> SystemTime {
    if secs >= 0 { UNIX_EPOCH + Duration::from_secs(secs as u64) } else { UNIX_EPOCH - Duration::from_secs((-secs) as u64) }
}

// ---- renaming ----------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CaseMode {
    #[default]
    Keep,
    Lower,
    Upper,
    /// First letter of every word capital.
    Title,
}

impl CaseMode {
    pub fn label(self) -> &'static str {
        match self {
            CaseMode::Keep => "Keep as is",
            CaseMode::Lower => "lower case",
            CaseMode::Upper => "UPPER CASE",
            CaseMode::Title => "Title Case",
        }
    }

    pub const ALL: [CaseMode; 4] = [CaseMode::Keep, CaseMode::Lower, CaseMode::Upper, CaseMode::Title];

    fn apply(self, s: &str) -> String {
        match self {
            CaseMode::Keep => s.to_string(),
            CaseMode::Lower => s.to_lowercase(),
            CaseMode::Upper => s.to_uppercase(),
            CaseMode::Title => {
                let mut out = String::with_capacity(s.len());
                let mut start = true;
                for c in s.chars() {
                    if start && c.is_alphabetic() {
                        out.extend(c.to_uppercase());
                        start = false;
                    } else {
                        out.extend(c.to_lowercase());
                        start = !c.is_alphanumeric() && c != '\'';
                    }
                }
                out
            }
        }
    }
}

/// How the names are made. The extension is kept (lower-cased if `ext_lower`); the template makes the part before it.
///
/// Tokens in `template`: `{name}` the old name, `{n}` / `{n:3}` a counter (padded to 3 digits), `{date}` modified date
/// (2025-06-01), `{time}` modified time (143005), `{taken}` date taken from EXIF (else the modified time) as
/// 2025-06-01_143005, `{folder}` the folder's name.
#[derive(Clone, Debug, PartialEq)]
pub struct RenameSpec {
    pub template: String,
    pub find: String,
    pub replace: String,
    pub case: CaseMode,
    pub start: u32,
    pub step: u32,
    pub ext_lower: bool,
}

impl Default for RenameSpec {
    fn default() -> Self {
        RenameSpec { template: "{name}".into(), find: String::new(), replace: String::new(), case: CaseMode::Keep, start: 1, step: 1, ext_lower: false }
    }
}

/// One file to rename and what is known about it.
#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub modified: i64,
    pub taken: Option<i64>,
}

/// Replace every `find` in `text`, ignoring case (like Explorer's search); an empty `find` changes nothing.
fn replace_ci(text: &str, find: &str, with: &str) -> String {
    if find.is_empty() {
        return text.to_string();
    }
    let (lt, lf): (Vec<char>, Vec<char>) = (text.to_lowercase().chars().collect(), find.to_lowercase().chars().collect());
    let orig: Vec<char> = text.chars().collect();
    // Lower-casing can change the length for a few letters; fall back to a plain replace then.
    if lt.len() != orig.len() {
        return text.replace(find, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while i < orig.len() {
        if i + lf.len() <= lt.len() && lt[i..i + lf.len()] == lf[..] {
            out.push_str(with);
            i += lf.len();
        } else {
            out.push(orig[i]);
            i += 1;
        }
    }
    out
}

/// The new file name for the `index`-th file (0-based) of the batch.
pub fn new_name(spec: &RenameSpec, src: &Source, index: usize, utc_offset_minutes: i32) -> String {
    let stem = src.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = src.path.extension().map(|s| s.to_string_lossy().into_owned());
    let counter = spec.start as i64 + spec.step as i64 * index as i64;
    let folder = src.path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (y, mo, d, h, mi, s) = civil_from_unix(src.modified, utc_offset_minutes);
    let taken = civil_from_unix(src.taken.unwrap_or(src.modified), utc_offset_minutes);

    let mut out = String::new();
    let chars: Vec<char> = spec.template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(close) = chars[i..].iter().position(|&c| c == '}') {
                let token: String = chars[i + 1..i + close].iter().collect();
                let lower = token.to_lowercase();
                let value = match lower.as_str() {
                    "name" => Some(stem.clone()),
                    "n" => Some(counter.to_string()),
                    "date" => Some(format!("{y:04}-{mo:02}-{d:02}")),
                    "time" => Some(format!("{h:02}{mi:02}{s:02}")),
                    "taken" => Some(format!("{:04}-{:02}-{:02}_{:02}{:02}{:02}", taken.0, taken.1, taken.2, taken.3, taken.4, taken.5)),
                    "folder" => Some(folder.clone()),
                    t if t.starts_with("n:") => t[2..].parse::<usize>().ok().filter(|w| *w <= 12).map(|w| format!("{counter:0w$}")),
                    _ => None,
                };
                if let Some(v) = value {
                    out.push_str(&v);
                    i += close + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    let out = replace_ci(&out, &spec.find, &spec.replace);
    let mut out = spec.case.apply(&out).trim().to_string();
    // Windows doesn't allow a name to end with a dot or a space.
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    match ext {
        Some(e) => format!("{out}.{}", if spec.ext_lower { e.to_lowercase() } else { e }),
        None => out,
    }
}

/// What will happen to one file.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub from: PathBuf,
    pub to: PathBuf,
    /// The name stays as it is (nothing to do).
    pub unchanged: bool,
    /// Why this one can't be renamed.
    pub error: Option<String>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// Work out every new name and find the problems (bad characters, two files ending up with one name, a name already
/// taken by a file that isn't part of the batch). Files are numbered in the order given.
pub fn plan_rename(spec: &RenameSpec, sources: &[Source], utc_offset_minutes: i32) -> Vec<PlanItem> {
    let mut items: Vec<PlanItem> = sources
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let name = new_name(spec, s, i, utc_offset_minutes);
            let dir = s.path.parent().unwrap_or(Path::new("."));
            let to = dir.join(&name);
            let unchanged = to == s.path;
            let error = valid_file_name(&name).err();
            PlanItem { from: s.path.clone(), to, unchanged, error }
        })
        .collect();
    for i in 0..items.len() {
        if items[i].error.is_some() || items[i].unchanged {
            continue;
        }
        let to = items[i].to.clone();
        if items.iter().enumerate().any(|(j, o)| j != i && same_path(&o.to, &to)) {
            items[i].error = Some("another file would get the same name".into());
        } else if to.exists() && !same_path(&to, &items[i].from) && !items.iter().any(|o| same_path(&o.from, &to)) {
            items[i].error = Some("a file with that name already exists".into());
        }
    }
    items
}

/// Rename everything in the plan that has no error. Names that are swapped around inside the batch (a -> b while b -> c)
/// go through temporary names first. Returns one result per plan item: the new path, or why it failed.
pub fn apply_rename(plan: &[PlanItem]) -> Vec<Result<PathBuf, String>> {
    let todo: Vec<usize> = (0..plan.len()).filter(|&i| plan[i].error.is_none() && !plan[i].unchanged).collect();
    let chain = todo.iter().any(|&i| todo.iter().any(|&j| j != i && same_path(&plan[j].from, &plan[i].to)) || (plan[i].to.exists() && !same_path(&plan[i].to, &plan[i].from)));
    let mut results: Vec<Result<PathBuf, String>> = plan
        .iter()
        .map(|p| match (&p.error, p.unchanged) {
            (Some(e), _) => Err(e.clone()),
            (None, true) => Ok(p.from.clone()),
            _ => Err("not done".into()),
        })
        .collect();
    if !chain {
        for &i in &todo {
            results[i] = std::fs::rename(&plan[i].from, &plan[i].to).map(|_| plan[i].to.clone()).map_err(|e| e.to_string());
        }
        return results;
    }
    // Phase 1: everything moves out of the way.
    let mut parked: Vec<(usize, PathBuf)> = Vec::new();
    for &i in &todo {
        let dir = plan[i].from.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(format!(".nv-rename-{}-{i}.tmp", std::process::id()));
        match std::fs::rename(&plan[i].from, &tmp) {
            Ok(()) => parked.push((i, tmp)),
            Err(e) => results[i] = Err(e.to_string()),
        }
    }
    // Phase 2: and into its final place. A failure puts the file back where it was.
    for (i, tmp) in parked {
        match std::fs::rename(&tmp, &plan[i].to) {
            Ok(()) => results[i] = Ok(plan[i].to.clone()),
            Err(e) => {
                let _ = std::fs::rename(&tmp, &plan[i].from);
                results[i] = Err(e.to_string());
            }
        }
    }
    results
}

// ---- file times --------------------------------------------------------------------------------------------------

/// Which of the file's times to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhichTimes {
    pub created: bool,
    pub modified: bool,
    pub accessed: bool,
}

/// Set the times of `path` to `unix` (UTC seconds).
pub fn set_file_times(path: &Path, unix: i64, which: WhichTimes) -> Result<(), String> {
    use std::os::windows::fs::FileTimesExt;
    let t = system_time_of(unix);
    let mut times = std::fs::FileTimes::new();
    if which.modified {
        times = times.set_modified(t);
    }
    if which.accessed {
        times = times.set_accessed(t);
    }
    if which.created {
        times = times.set_created(t);
    }
    // Folders need FILE_FLAG_BACKUP_SEMANTICS to be opened at all; files open normally.
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().write(true).custom_flags(0x0200_0000).open(path).map_err(|e| format!("{}: {e}", path.display()))?
    };
    file.set_times(times).map_err(|e| format!("{}: {e}", path.display()))
}

/// (created, modified) of a file as unix seconds.
pub fn read_file_times(path: &Path) -> Option<(i64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    let modified = unix_of(m.modified().ok()?);
    let created = m.created().map(unix_of).unwrap_or(modified);
    Some((created, modified))
}

/// How the new time of each file is worked out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeRule {
    /// Every file gets this time (plus `step` seconds per file, in the order given, to keep a sequence in order).
    Set { unix: i64, step: i64 },
    /// Move every file's time by this many seconds (negative = earlier).
    Shift(i64),
    /// The date taken from the picture's EXIF; files without one are left alone.
    FromTaken,
}

/// The time `path` should get under `rule`, or None to leave it. `current` is its modified time; `taken` its EXIF time.
pub fn target_time(rule: TimeRule, index: usize, current: i64, taken: Option<i64>) -> Option<i64> {
    match rule {
        TimeRule::Set { unix, step } => Some(unix + step * index as i64),
        TimeRule::Shift(secs) => Some(current + secs),
        TimeRule::FromTaken => taken,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_batch_{tag}_{}_{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn src(p: &str) -> Source {
        Source { path: PathBuf::from(p), modified: parse_local_datetime("2025-06-01 14:30:05", 0).unwrap(), taken: None }
    }

    #[test]
    fn civil_dates_round_trip() {
        for t in [0i64, 86_399, 1_748_788_205, 951_782_400 /* 2000-02-29 */, -86_400] {
            let (y, m, d, h, mi, s) = civil_from_unix(t, 0);
            assert_eq!(parse_local_datetime(&format!("{y}-{m}-{d} {h}:{mi}:{s}"), 0), Some(t));
        }
        assert_eq!(format_local(1_748_788_205, 0), "2025-06-01 14:30:05");
        assert_eq!(format_local(1_748_788_205, 7 * 60), "2025-06-01 21:30:05");
    }

    #[test]
    fn dates_are_parsed_in_the_usual_spellings() {
        let t = parse_local_datetime("2025-06-01 14:30:05", 0).unwrap();
        assert_eq!(parse_local_datetime("2025/06/01 14:30:05", 0), Some(t));
        assert_eq!(parse_local_datetime("20250601T143005", 0), Some(t));
        assert_eq!(parse_local_datetime("2025-06-01 14:30", 0), Some(t - 5));
        assert_eq!(parse_local_datetime("2025-06-01", 0), Some(t - 14 * 3600 - 30 * 60 - 5));
        // Local time: Bangkok is UTC+7.
        assert_eq!(parse_local_datetime("2025-06-01 21:30:05", 420), Some(t));
        for bad in ["", "hello", "2025-13-01", "2025-02-30", "2025-06-01 25:00", "2025-06"] {
            assert_eq!(parse_local_datetime(bad, 0), None, "{bad:?}");
        }
    }

    #[test]
    fn templates_expand() {
        let s = src("C:\\Pics\\Trip\\IMG_0042.JPG");
        let mut spec = RenameSpec { template: "{folder}_{n:3}_{name}".into(), ..Default::default() };
        assert_eq!(new_name(&spec, &s, 6, 0), "Trip_007_IMG_0042.JPG");
        spec.template = "{date} {time}".into();
        assert_eq!(new_name(&spec, &s, 0, 0), "2025-06-01 143005.JPG");
        spec.template = "{taken}".into();
        assert_eq!(new_name(&spec, &s, 0, 0), "2025-06-01_143005.JPG", "no EXIF date: the modified time");
        let mut with_taken = s.clone();
        with_taken.taken = parse_local_datetime("2024-12-25 08:00:00", 0);
        assert_eq!(new_name(&spec, &with_taken, 0, 0), "2024-12-25_080000.JPG");
        spec.template = "x{unknown}{n:99}{".into();
        assert_eq!(new_name(&spec, &s, 0, 0), "x{unknown}{n:99}{.JPG", "unknown tokens stay as typed");
    }

    #[test]
    fn find_replace_case_and_counter_options() {
        let s = src("D:\\a\\holiday PHOTO 1.png");
        let spec = RenameSpec { find: "photo".into(), replace: "pic".into(), ..Default::default() };
        assert_eq!(new_name(&spec, &s, 0, 0), "holiday pic 1.png", "find ignores case");
        let spec = RenameSpec { case: CaseMode::Title, ..Default::default() };
        assert_eq!(new_name(&spec, &s, 0, 0), "Holiday Photo 1.png");
        let spec = RenameSpec { case: CaseMode::Upper, ext_lower: true, ..Default::default() };
        assert_eq!(new_name(&spec, &src("D:\\a\\x.JPG"), 0, 0), "X.jpg");
        let spec = RenameSpec { template: "{n:2}".into(), start: 10, step: 5, ..Default::default() };
        assert_eq!(new_name(&spec, &s, 2, 0), "20.png");
    }

    #[test]
    fn plan_flags_problems() {
        let d = temp_dir("plan");
        for n in ["a.jpg", "b.jpg", "c.jpg", "taken.jpg"] {
            fs::write(d.join(n), n).unwrap();
        }
        let srcs: Vec<Source> = ["a.jpg", "b.jpg"].iter().map(|n| Source { path: d.join(n), modified: 0, taken: None }).collect();
        // Both would be called "same.jpg".
        let plan = plan_rename(&RenameSpec { template: "same".into(), ..Default::default() }, &srcs, 0);
        assert!(plan.iter().all(|p| p.error.as_deref() == Some("another file would get the same name")));
        // An existing file that is not in the batch.
        let plan = plan_rename(&RenameSpec { template: "taken".into(), start: 1, ..Default::default() }, &srcs[..1], 0);
        assert_eq!(plan[0].error.as_deref(), Some("a file with that name already exists"));
        // Bad character.
        let plan = plan_rename(&RenameSpec { template: "a?b".into(), ..Default::default() }, &srcs[..1], 0);
        assert!(plan[0].error.is_some());
        // Nothing to do.
        let plan = plan_rename(&RenameSpec::default(), &srcs, 0);
        assert!(plan.iter().all(|p| p.unchanged && p.error.is_none()));
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn renaming_applies_and_handles_swaps() {
        let d = temp_dir("apply");
        fs::write(d.join("1.jpg"), "one").unwrap();
        fs::write(d.join("2.jpg"), "two").unwrap();
        // A plain numbering.
        let srcs: Vec<Source> = ["1.jpg", "2.jpg"].iter().map(|n| Source { path: d.join(n), modified: 0, taken: None }).collect();
        let plan = plan_rename(&RenameSpec { template: "pic_{n:2}".into(), ..Default::default() }, &srcs, 0);
        let r = apply_rename(&plan);
        assert!(r.iter().all(|x| x.is_ok()), "{r:?}");
        assert_eq!(fs::read_to_string(d.join("pic_01.jpg")).unwrap(), "one");
        assert_eq!(fs::read_to_string(d.join("pic_02.jpg")).unwrap(), "two");
        // Now the names are shifted by one inside the batch: 01 -> 02, 02 -> 03 (02 is taken by a batch member).
        let srcs: Vec<Source> = ["pic_01.jpg", "pic_02.jpg"].iter().map(|n| Source { path: d.join(n), modified: 0, taken: None }).collect();
        let plan = plan_rename(&RenameSpec { template: "pic_{n:2}".into(), start: 2, ..Default::default() }, &srcs, 0);
        assert!(plan.iter().all(|p| p.error.is_none()), "{plan:?}");
        let r = apply_rename(&plan);
        assert!(r.iter().all(|x| x.is_ok()), "{r:?}");
        assert_eq!(fs::read_to_string(d.join("pic_02.jpg")).unwrap(), "one");
        assert_eq!(fs::read_to_string(d.join("pic_03.jpg")).unwrap(), "two");
        assert!(!d.join("pic_01.jpg").exists());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn file_times_can_be_set_and_read_back() {
        let d = temp_dir("times");
        let f = d.join("t.txt");
        fs::write(&f, "x").unwrap();
        let t = parse_local_datetime("2020-01-02 03:04:05", 0).unwrap();
        set_file_times(&f, t, WhichTimes { created: true, modified: true, accessed: false }).unwrap();
        assert_eq!(read_file_times(&f), Some((t, t)));
        // A folder works too.
        set_file_times(&d, t, WhichTimes { created: false, modified: true, accessed: false }).unwrap();
        assert_eq!(read_file_times(&d).map(|x| x.1), Some(t));
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn time_rules() {
        assert_eq!(target_time(TimeRule::Set { unix: 1000, step: 2 }, 3, 5, None), Some(1006));
        assert_eq!(target_time(TimeRule::Shift(-60), 0, 500, None), Some(440));
        assert_eq!(target_time(TimeRule::FromTaken, 0, 500, Some(9)), Some(9));
        assert_eq!(target_time(TimeRule::FromTaken, 0, 500, None), None);
    }
}
