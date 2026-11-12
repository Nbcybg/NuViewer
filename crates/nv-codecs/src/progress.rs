//! Progress of the file that a decode thread is working on, so the viewer can say what is going on while a big
//! picture loads ("reading 45 / 120 MB at 37 MB/s", "decoding…") instead of looking frozen.
//!
//! A pool worker installs a [`JobProgress`] for the duration of one job (`with_job`); every decoder reads its file
//! through [`read_file`], which reads in chunks and keeps the counters up to date. The same hook lets a stale
//! prefetch stop reading early (`cancel`) and lets prefetches wait while the picture the user is waiting for is
//! still coming off the disk (`hold`), which matters on a spinning disk where two readers fight for the head.

use std::cell::RefCell;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Job stages (`JobProgress::stage`).
pub const STAGE_QUEUED: u8 = 0;
/// Started, but we don't know yet whether it is reading or decoding (formats whose reader is a system component).
pub const STAGE_WORKING: u8 = 1;
pub const STAGE_READING: u8 = 2;
pub const STAGE_DECODING: u8 = 3;

/// Bytes per read call: big enough to keep a hard disk streaming, small enough for fine-grained progress.
const CHUNK: usize = 2 * 1024 * 1024;

pub struct JobProgress {
    pub total: AtomicU64,
    pub read: AtomicU64,
    pub stage: AtomicU8,
    /// The picture on screen (or the one the user is waiting for) rather than a prefetch.
    pub urgent: AtomicBool,
    /// Set when the job is no longer wanted: reading stops at the next chunk.
    pub cancelled: AtomicBool,
    pub created: Instant,
    /// Nanoseconds after `created` when reading began / ended (0 = not yet).
    read_start_ns: AtomicU64,
    read_end_ns: AtomicU64,
    /// When decoding began (ns after `created`, 0 = not yet).
    decode_start_ns: AtomicU64,
}

impl JobProgress {
    pub fn new(urgent: bool) -> Arc<JobProgress> {
        Arc::new(JobProgress {
            total: AtomicU64::new(0),
            read: AtomicU64::new(0),
            stage: AtomicU8::new(STAGE_QUEUED),
            urgent: AtomicBool::new(urgent),
            cancelled: AtomicBool::new(false),
            created: Instant::now(),
            read_start_ns: AtomicU64::new(0),
            read_end_ns: AtomicU64::new(0),
            decode_start_ns: AtomicU64::new(0),
        })
    }

    fn ns(&self) -> u64 {
        self.created.elapsed().as_nanos().max(1) as u64
    }

    pub fn is_reading(&self) -> bool {
        self.stage.load(Ordering::Relaxed) == STAGE_READING
    }

    pub fn snapshot(&self) -> Snapshot {
        let now = self.ns();
        let (start, end) = (self.read_start_ns.load(Ordering::Relaxed), self.read_end_ns.load(Ordering::Relaxed));
        let read = self.read.load(Ordering::Relaxed);
        let reading_ns = if start == 0 { 0 } else { (if end == 0 { now } else { end }).saturating_sub(start) };
        let decode_start = self.decode_start_ns.load(Ordering::Relaxed);
        Snapshot {
            stage: self.stage.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            read,
            read_secs: reading_ns as f64 / 1e9,
            decode_secs: if decode_start == 0 { 0.0 } else { now.saturating_sub(decode_start) as f64 / 1e9 },
            elapsed_secs: now as f64 / 1e9,
            bytes_per_sec: if reading_ns > 50_000_000 { read as f64 / (reading_ns as f64 / 1e9) } else { 0.0 },
        }
    }

    pub fn set_stage(&self, stage: u8) {
        self.stage.store(stage, Ordering::Relaxed);
        if stage == STAGE_DECODING && self.decode_start_ns.load(Ordering::Relaxed) == 0 {
            self.decode_start_ns.store(self.ns(), Ordering::Relaxed);
        }
    }
}

/// A moment-in-time copy of a job's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub stage: u8,
    pub total: u64,
    pub read: u64,
    /// Seconds spent reading so far (stops counting when reading ends).
    pub read_secs: f64,
    /// Seconds spent decoding so far (0 before decoding starts).
    pub decode_secs: f64,
    /// Seconds since the job was queued.
    pub elapsed_secs: f64,
    /// Average read speed over the reading phase (0 while there is too little to tell).
    pub bytes_per_sec: f64,
}

type Hold = Arc<dyn Fn() -> bool + Send + Sync>;

struct Ctx {
    job: Arc<JobProgress>,
    /// True while this (non-urgent) job should pause reading.
    hold: Option<Hold>,
}

thread_local! {
    static CURRENT: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

/// Run `f` with `job` installed as this thread's current job.
pub fn with_job<R>(job: &Arc<JobProgress>, hold: Option<Hold>, f: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT.with(|c| *c.borrow_mut() = None);
        }
    }
    CURRENT.with(|c| *c.borrow_mut() = Some(Ctx { job: job.clone(), hold }));
    job.set_stage(STAGE_WORKING);
    let background = !job.urgent.load(Ordering::Relaxed);
    set_background(background);
    let _reset = Reset;
    let r = f();
    if background {
        set_background(false); // the pool thread is reused for the next job
    }
    r
}

/// True when the current thread is working on a prefetch (not the picture the user is waiting for).
pub fn is_background() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().map(|ctx| !ctx.job.urgent.load(Ordering::Relaxed)).unwrap_or(false))
}

/// Run this thread below normal priority (`true`) or at normal priority (`false`). Prefetch decoding does this so a
/// busy CPU never makes the window sluggish; helper threads that a decoder spawns call it with `is_background()`.
pub fn set_background(on: bool) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL};
        let _ = SetThreadPriority(GetCurrentThread(), if on { THREAD_PRIORITY_BELOW_NORMAL } else { THREAD_PRIORITY_NORMAL });
    }
    #[cfg(not(windows))]
    let _ = on;
}

/// Tell the current job (if any) that decoding has begun.
pub fn decoding() {
    CURRENT.with(|c| {
        if let Some(ctx) = c.borrow().as_ref() {
            ctx.job.set_stage(STAGE_DECODING);
        }
    });
}

/// True when the current job was cancelled (decoders may poll this between big steps).
pub fn is_cancelled() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().map(|ctx| ctx.job.cancelled.load(Ordering::Relaxed)).unwrap_or(false))
}

/// The error text of a job that was cancelled while reading (the pool recognises it and drops the result quietly).
pub const CANCELLED: &str = "cancelled";

/// Read a whole file in chunks, updating the current job's counters. Without a job installed it is a plain read.
pub fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("read: {e}"))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let (job, hold) = CURRENT.with(|c| c.borrow().as_ref().map(|ctx| (Some(ctx.job.clone()), ctx.hold.clone())).unwrap_or((None, None)));
    if let Some(j) = &job {
        j.total.store(len, Ordering::Relaxed);
        j.read.store(0, Ordering::Relaxed);
        j.read_start_ns.store(j.ns(), Ordering::Relaxed);
        j.set_stage(STAGE_READING);
    }
    let mut buf: Vec<u8> = Vec::with_capacity((len as usize).saturating_add(1));
    let result = (|| -> Result<(), String> {
        loop {
            if let Some(j) = &job {
                if j.cancelled.load(Ordering::Relaxed) {
                    return Err(CANCELLED.into());
                }
                // A prefetch lets the picture the user is waiting for have the disk to itself.
                if let Some(h) = &hold {
                    let mut waited = 0;
                    while h() && waited < 20_000 && !j.cancelled.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(5));
                        waited += 5;
                    }
                }
            }
            let before = buf.len();
            (&mut f).take(CHUNK as u64).read_to_end(&mut buf).map_err(|e| format!("read: {e}"))?;
            let got = buf.len() - before;
            if let Some(j) = &job {
                j.read.fetch_add(got as u64, Ordering::Relaxed);
            }
            if got < CHUNK {
                return Ok(());
            }
        }
    })();
    if let Some(j) = &job {
        j.read_end_ns.store(j.ns(), Ordering::Relaxed);
        j.set_stage(STAGE_DECODING);
    }
    result.map(|()| buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_read_without_a_job() {
        let p = std::env::temp_dir().join(format!("nv_progress_plain_{}.bin", std::process::id()));
        std::fs::write(&p, vec![7u8; 5_000_000]).unwrap();
        let b = read_file(&p).unwrap();
        assert_eq!(b.len(), 5_000_000);
        assert!(b.iter().all(|&x| x == 7));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn job_counters_follow_the_read() {
        let p = std::env::temp_dir().join(format!("nv_progress_job_{}.bin", std::process::id()));
        std::fs::write(&p, vec![1u8; 4_500_000]).unwrap();
        let job = JobProgress::new(true);
        let b = with_job(&job, None, || read_file(&p)).unwrap();
        assert_eq!(b.len(), 4_500_000);
        let s = job.snapshot();
        assert_eq!((s.total, s.read, s.stage), (4_500_000, 4_500_000, STAGE_DECODING));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn exact_multiple_of_the_chunk_size_reads_everything() {
        let p = std::env::temp_dir().join(format!("nv_progress_exact_{}.bin", std::process::id()));
        std::fs::write(&p, vec![3u8; CHUNK * 2]).unwrap();
        assert_eq!(read_file(&p).unwrap().len(), CHUNK * 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_cancelled_job_stops_reading() {
        let p = std::env::temp_dir().join(format!("nv_progress_cancel_{}.bin", std::process::id()));
        std::fs::write(&p, vec![1u8; 6_000_000]).unwrap();
        let job = JobProgress::new(false);
        job.cancelled.store(true, Ordering::Relaxed);
        let r = with_job(&job, None, || read_file(&p));
        assert_eq!(r.unwrap_err(), CANCELLED);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_held_job_waits_for_the_gate() {
        let p = std::env::temp_dir().join(format!("nv_progress_hold_{}.bin", std::process::id()));
        std::fs::write(&p, vec![1u8; 1000]).unwrap();
        let job = JobProgress::new(false);
        let gate = Arc::new(AtomicBool::new(true));
        let g = gate.clone();
        let hold: Hold = Arc::new(move || g.load(Ordering::Relaxed));
        let opener = std::thread::spawn({
            let gate = gate.clone();
            move || {
                std::thread::sleep(Duration::from_millis(120));
                gate.store(false, Ordering::Relaxed);
            }
        });
        let t0 = Instant::now();
        with_job(&job, Some(hold), || read_file(&p)).unwrap();
        assert!(t0.elapsed() >= Duration::from_millis(100), "should have waited for the gate");
        opener.join().unwrap();
        let _ = std::fs::remove_file(&p);
    }
}
