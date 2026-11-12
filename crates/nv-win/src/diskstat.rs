//! How busy the disk a picture lives on is, for the "loading" panel: read and write speed of the whole drive (not
//! just NuViewer) and its busy percentage, from the Windows performance counters (`\LogicalDisk(F:)\...`).
//!
//! A slow picture is usually the disk, and the disk is often slow because something else is using it; the drive-wide
//! numbers let the user see which. Sampling runs on its own thread, only while somebody has asked in the last few
//! seconds (`watch`), so an idle viewer costs nothing.

use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::System::Performance::*;

/// Drive letter being watched (upper case ASCII), 0 = none.
static DRIVE: AtomicU32 = AtomicU32::new(0);
/// Milliseconds since `start()` at the last `watch` call.
static WANTED_AT_MS: AtomicU64 = AtomicU64::new(0);
/// Latest sample (f64 bits) and when it was taken (ms since start, 0 = none yet) and for which drive.
static READ_BPS: AtomicU64 = AtomicU64::new(0);
static WRITE_BPS: AtomicU64 = AtomicU64::new(0);
static BUSY_PCT: AtomicU64 = AtomicU64::new(0);
static SAMPLED_AT_MS: AtomicU64 = AtomicU64::new(0);
static SAMPLED_DRIVE: AtomicU32 = AtomicU32::new(0);
static STARTER: Once = Once::new();

/// Sample every this often while watched.
const INTERVAL: Duration = Duration::from_millis(400);
/// Stop sampling this long after the last `watch`.
const KEEP_ALIVE_MS: u64 = 5_000;
/// A sample older than this is not shown.
const FRESH_MS: u64 = 1_500;

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    start().elapsed().as_millis() as u64 + 1
}

/// Drive-wide disk activity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiskStats {
    pub drive: char,
    pub read_bps: f64,
    pub write_bps: f64,
    /// 0..=100: the share of the last interval the disk spent working.
    pub busy_pct: f64,
}

/// The drive letter a path is on ("F" for `F:\Photos\a.jpg`, also for `\\?\F:\...`); None for network paths.
pub fn drive_of(path: &Path) -> Option<char> {
    use std::path::{Component, Prefix};
    match path.components().next()? {
        Component::Prefix(p) => match p.kind() {
            Prefix::Disk(d) | Prefix::VerbatimDisk(d) => Some((d as char).to_ascii_uppercase()),
            _ => None,
        },
        _ => None,
    }
}

/// Say that the disk holding `path` matters right now: starts (or keeps alive) the sampler for that drive.
pub fn watch(path: &Path) {
    let Some(d) = drive_of(path) else { return };
    DRIVE.store(d as u32, Ordering::Relaxed);
    WANTED_AT_MS.store(now_ms(), Ordering::Relaxed);
    STARTER.call_once(|| {
        let _ = std::thread::Builder::new().name("disk-stats".into()).spawn(sampler);
    });
}

/// The latest numbers for the watched drive, if they are fresh.
pub fn stats() -> Option<DiskStats> {
    let drive = DRIVE.load(Ordering::Relaxed);
    let at = SAMPLED_AT_MS.load(Ordering::Relaxed);
    if drive == 0 || at == 0 || SAMPLED_DRIVE.load(Ordering::Relaxed) != drive || now_ms().saturating_sub(at) > FRESH_MS {
        return None;
    }
    let f = |a: &AtomicU64| f64::from_bits(a.load(Ordering::Relaxed));
    Some(DiskStats { drive: char::from_u32(drive)?, read_bps: f(&READ_BPS), write_bps: f(&WRITE_BPS), busy_pct: f(&BUSY_PCT).clamp(0.0, 100.0) })
}

struct Query {
    handle: PDH_HQUERY,
    read: PDH_HCOUNTER,
    write: PDH_HCOUNTER,
    idle: PDH_HCOUNTER,
    drive: u32,
}

impl Query {
    fn open(drive: u32) -> Option<Query> {
        let letter = char::from_u32(drive)?;
        unsafe {
            let mut handle = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut handle) != 0 {
                return None;
            }
            let add = |name: &str| -> Option<PDH_HCOUNTER> {
                let path: Vec<u16> = format!("\\LogicalDisk({letter}:)\\{name}").encode_utf16().chain(std::iter::once(0)).collect();
                let mut c = PDH_HCOUNTER::default();
                (PdhAddEnglishCounterW(handle, PCWSTR(path.as_ptr()), 0, &mut c) == 0).then_some(c)
            };
            let counters = (|| Some((add("Disk Read Bytes/sec")?, add("Disk Write Bytes/sec")?, add("% Idle Time")?)))();
            match counters {
                Some((read, write, idle)) => {
                    // The first collection only sets the baseline.
                    PdhCollectQueryData(handle);
                    Some(Query { handle, read, write, idle, drive })
                }
                None => {
                    PdhCloseQuery(handle);
                    None
                }
            }
        }
    }

    fn sample(&self) -> Option<(f64, f64, f64)> {
        unsafe {
            if PdhCollectQueryData(self.handle) != 0 {
                return None;
            }
            let get = |c: PDH_HCOUNTER| -> Option<f64> {
                let mut v = PDH_FMT_COUNTERVALUE::default();
                if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) != 0 || v.CStatus != PDH_CSTATUS_VALID_DATA {
                    return None;
                }
                Some(v.Anonymous.doubleValue)
            };
            Some((get(self.read)?.max(0.0), get(self.write)?.max(0.0), 100.0 - get(self.idle)?.clamp(0.0, 100.0)))
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.handle);
        }
    }
}

fn sampler() {
    let mut query: Option<Query> = None;
    // A drive whose counters couldn't be created is not retried until another drive is watched.
    let mut failed_drive = 0u32;
    loop {
        std::thread::sleep(INTERVAL);
        let wanted = now_ms().saturating_sub(WANTED_AT_MS.load(Ordering::Relaxed)) < KEEP_ALIVE_MS;
        let drive = DRIVE.load(Ordering::Relaxed);
        if !wanted || drive == 0 {
            query = None; // nobody is looking: stop collecting
            continue;
        }
        if query.as_ref().map(|q| q.drive) != Some(drive) {
            query = None;
            if failed_drive == drive {
                continue;
            }
            query = Query::open(drive);
            if query.is_none() {
                failed_drive = drive;
                continue;
            }
            failed_drive = 0;
            continue; // the next tick has a full interval to measure
        }
        if let Some((r, w, b)) = query.as_ref().and_then(|q| q.sample()) {
            READ_BPS.store(r.to_bits(), Ordering::Relaxed);
            WRITE_BPS.store(w.to_bits(), Ordering::Relaxed);
            BUSY_PCT.store(b.to_bits(), Ordering::Relaxed);
            SAMPLED_DRIVE.store(drive, Ordering::Relaxed);
            SAMPLED_AT_MS.store(now_ms(), Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_come_from_the_path() {
        assert_eq!(drive_of(Path::new(r"F:\Photos\a.jpg")), Some('F'));
        assert_eq!(drive_of(Path::new(r"c:\x")), Some('C'));
        assert_eq!(drive_of(Path::new(r"\\?\D:\deep\x.png")), Some('D'));
        assert_eq!(drive_of(Path::new(r"\\server\share\x.png")), None);
        assert_eq!(drive_of(Path::new("relative.png")), None);
    }

    #[test]
    fn sampling_the_system_drive_gives_sane_numbers_or_nothing() {
        // The counters may be missing on a stripped-down machine; when they exist the numbers must make sense.
        watch(Path::new(r"C:\Windows"));
        std::thread::sleep(Duration::from_millis(1500));
        if let Some(s) = stats() {
            assert_eq!(s.drive, 'C');
            assert!(s.read_bps >= 0.0 && s.write_bps >= 0.0 && (0.0..=100.0).contains(&s.busy_pct), "{s:?}");
        }
    }
}
