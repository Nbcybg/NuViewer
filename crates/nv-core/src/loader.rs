use nv_codecs::progress::{self, JobProgress, Snapshot};
use nv_codecs::Image;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::Instant;

type WorkFn = dyn Fn(&Path) -> Result<Image, String> + Send + Sync;

pub struct Loaded {
    pub path: PathBuf,
    pub result: Result<Image, String>,
    pub decode_ms: f64,
    /// How the job went: bytes read from disk and for how long (for "read 120 MB in 3.2 s").
    pub snap: Snapshot,
    /// The job was told to stop (the picture was no longer wanted) and gave up while reading: not an error.
    pub cancelled: bool,
}

struct Queue {
    jobs: VecDeque<(PathBuf, Arc<JobProgress>)>,
    in_flight: HashMap<PathBuf, Arc<JobProgress>>,
    shutdown: bool,
}

struct Shared {
    q: Mutex<Queue>,
    cv: Condvar,
}

/// What the viewer can tell the user about a picture that is loading.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadState {
    /// Waiting for a free worker (nothing has happened yet).
    pub queued: bool,
    pub progress: Snapshot,
}

/// Background decode pool. `request` replaces the pending queue (stale prefetches are dropped); results come back
/// via `try_recv`, and `notify` is called from the worker after each result.
///
/// The first path of a request is the *urgent* one (the picture the user is waiting for): it has a worker of its own
/// (`reserve_urgent`) so a long prefetch can't delay it, other jobs that are still reading when it is no longer
/// wanted are told to stop, and prefetches pause their disk reads while the urgent one is still being read.
pub struct Loader {
    shared: Arc<Shared>,
    rx: mpsc::Receiver<Loaded>,
}

impl Loader {
    /// Pool that fully decodes images.
    pub fn full(threads: usize, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::new(threads, true, notify, Arc::new(|p: &Path| nv_codecs::decode_file(p)))
    }

    /// Pool that produces small thumbnails (served from the disk cache when possible).
    pub fn thumbs(threads: usize, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self::new(threads, false, notify, Arc::new(|p: &Path| crate::thumbs::make_thumb(p)))
    }

    pub fn new(threads: usize, reserve_urgent: bool, notify: Arc<dyn Fn() + Send + Sync>, work: Arc<WorkFn>) -> Self {
        let shared = Arc::new(Shared {
            q: Mutex::new(Queue { jobs: VecDeque::new(), in_flight: HashMap::new(), shutdown: false }),
            cv: Condvar::new(),
        });
        let (tx, rx) = mpsc::channel();
        let workers = threads.max(1) + usize::from(reserve_urgent);
        for n in 0..workers {
            let urgent_only = reserve_urgent && n == 0;
            let (shared, tx, notify, work) = (shared.clone(), tx.clone(), notify.clone(), work.clone());
            std::thread::spawn(move || loop {
                let (path, job) = {
                    let mut q = shared.q.lock().unwrap();
                    loop {
                        if q.shutdown {
                            return;
                        }
                        let take = match q.jobs.front() {
                            Some((_, j)) => !urgent_only || j.urgent.load(Ordering::Relaxed),
                            None => false,
                        };
                        if take {
                            let (p, j) = q.jobs.pop_front().unwrap();
                            q.in_flight.insert(p.clone(), j.clone());
                            break (p, j);
                        }
                        q = shared.cv.wait(q).unwrap();
                    }
                };
                // Prefetches stay off the disk while the urgent picture is still being read.
                let hold: Option<Arc<dyn Fn() -> bool + Send + Sync>> = {
                    let (shared, me) = (shared.clone(), job.clone());
                    Some(Arc::new(move || {
                        if me.urgent.load(Ordering::Relaxed) {
                            return false;
                        }
                        let q = shared.q.lock().unwrap();
                        q.in_flight.values().any(|j| j.urgent.load(Ordering::Relaxed) && j.is_reading())
                    }))
                };
                let t0 = Instant::now();
                // A malformed file must never take the viewer down.
                let result = progress::with_job(&job, hold, || {
                    if job.cancelled.load(Ordering::Relaxed) {
                        return Err(progress::CANCELLED.to_string());
                    }
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&path))).unwrap_or_else(|_| Err("decoder panicked".into()))
                });
                let decode_ms = t0.elapsed().as_secs_f64() * 1000.0;
                shared.q.lock().unwrap().in_flight.remove(&path);
                let cancelled = matches!(&result, Err(e) if e == progress::CANCELLED);
                let snap = job.snapshot();
                if tx.send(Loaded { path, result, decode_ms, snap, cancelled }).is_err() {
                    return;
                }
                notify();
            });
        }
        Loader { shared, rx }
    }

    /// Highest priority first (the first path is the urgent one). Replaces whatever was queued; paths already being
    /// decoded are not started twice, and jobs that are running for pictures nobody wants any more are told to stop.
    pub fn request(&self, wanted: Vec<PathBuf>) {
        let mut q = self.shared.q.lock().unwrap();
        for (i, p) in wanted.iter().enumerate() {
            if let Some(j) = q.in_flight.get(p) {
                j.urgent.store(i == 0, Ordering::Relaxed);
                j.cancelled.store(false, Ordering::Relaxed);
            }
        }
        for (p, j) in q.in_flight.iter() {
            if !wanted.contains(p) {
                j.cancelled.store(true, Ordering::Relaxed);
                j.urgent.store(false, Ordering::Relaxed);
            }
        }
        let mut old: HashMap<PathBuf, Arc<JobProgress>> = q.jobs.drain(..).collect();
        let mut jobs = VecDeque::new();
        for (i, p) in wanted.into_iter().enumerate() {
            if q.in_flight.contains_key(&p) {
                continue;
            }
            let j = old.remove(&p).unwrap_or_else(|| JobProgress::new(false));
            j.urgent.store(i == 0, Ordering::Relaxed);
            jobs.push_back((p, j));
        }
        q.jobs = jobs;
        drop(q);
        self.shared.cv.notify_all();
    }

    /// Where `path` stands: `None` when it is neither queued nor being worked on.
    pub fn state_of(&self, path: &Path) -> Option<LoadState> {
        let q = self.shared.q.lock().unwrap();
        if let Some(j) = q.in_flight.get(path) {
            return Some(LoadState { queued: false, progress: j.snapshot() });
        }
        q.jobs.iter().find(|(p, _)| p == path).map(|(_, j)| LoadState { queued: true, progress: j.snapshot() })
    }

    pub fn try_recv(&self) -> Option<Loaded> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut q = self.shared.q.lock().unwrap();
        q.shutdown = true;
        for j in q.in_flight.values() {
            j.cancelled.store(true, Ordering::Relaxed);
        }
        drop(q);
        self.shared.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn wait_for(loader: &Loader, n: usize) -> Vec<Loaded> {
        let mut out = Vec::new();
        let t0 = Instant::now();
        while out.len() < n && t0.elapsed() < Duration::from_secs(10) {
            match loader.try_recv() {
                Some(l) => out.push(l),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        out
    }

    #[test]
    fn the_urgent_job_gets_a_worker_while_prefetches_are_busy() {
        // Two normal workers are tied up for a long time; the urgent job must still run at once on the reserved one.
        let busy = Arc::new(AtomicUsize::new(0));
        let b = busy.clone();
        let work: Arc<WorkFn> = Arc::new(move |p: &Path| {
            if p.to_string_lossy().contains("slow") {
                b.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(700));
            }
            Ok(Image::new(1, 1, vec![0; 4]))
        });
        let loader = Loader::new(2, true, Arc::new(|| {}), work);
        loader.request(vec![PathBuf::from("quick0"), PathBuf::from("slow1"), PathBuf::from("slow2")]);
        assert_eq!(wait_for(&loader, 1).len(), 1);
        let t0 = Instant::now();
        while busy.load(Ordering::SeqCst) < 2 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        loader.request(vec![PathBuf::from("now")]);
        let done = wait_for(&loader, 1);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].path, PathBuf::from("now"));
        assert!(t0.elapsed() < Duration::from_millis(600), "the urgent job waited for a prefetch");
    }

    #[test]
    fn state_of_reports_queued_and_running_jobs() {
        let gate = Arc::new(AtomicUsize::new(0));
        let g = gate.clone();
        let work: Arc<WorkFn> = Arc::new(move |_p: &Path| {
            while g.load(Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(Image::new(1, 1, vec![0; 4]))
        });
        let loader = Loader::new(1, false, Arc::new(|| {}), work);
        loader.request(vec![PathBuf::from("a"), PathBuf::from("b")]);
        std::thread::sleep(Duration::from_millis(60));
        assert!(!loader.state_of(Path::new("a")).unwrap().queued, "a is running");
        assert!(loader.state_of(Path::new("b")).unwrap().queued, "b waits for the only worker");
        assert!(loader.state_of(Path::new("zzz")).is_none());
        gate.store(1, Ordering::SeqCst);
        assert_eq!(wait_for(&loader, 2).len(), 2);
        assert!(loader.state_of(Path::new("a")).is_none());
    }

    #[test]
    fn unwanted_running_jobs_are_cancelled_and_reported_quietly() {
        // A job that reads a real file notices the cancel at the next chunk.
        let dir = std::env::temp_dir().join(format!("nv_loader_cancel_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.bin");
        std::fs::write(&big, vec![1u8; 3_000_000]).unwrap();
        // A hold that never lets go keeps the job parked in its first chunk wait.
        let work: Arc<WorkFn> = Arc::new(|p: &Path| {
            let b = progress::read_file(p)?;
            Ok(Image::new(1, 1, vec![b[0]; 4]))
        });
        let loader = Loader::new(1, false, Arc::new(|| {}), work);
        let urgent_other = JobProgress::new(true);
        urgent_other.set_stage(progress::STAGE_READING);
        loader.shared.q.lock().unwrap().in_flight.insert(PathBuf::from("pretend-urgent"), urgent_other.clone());
        loader.request(vec![big.clone()]); // it is the urgent one itself, so it is not held back
        let ok = wait_for(&loader, 1);
        assert!(ok[0].result.is_ok() && !ok[0].cancelled);
        // Now a non-urgent job is held while the pretend-urgent one "reads", and gets cancelled by the next request.
        loader.request(vec![PathBuf::from("pretend-urgent"), big.clone()]);
        std::thread::sleep(Duration::from_millis(150));
        loader.request(vec![PathBuf::from("pretend-urgent")]); // big is no longer wanted
        let got = wait_for(&loader, 1);
        assert_eq!(got.len(), 1);
        assert!(got[0].cancelled, "{:?}", got[0].result.as_ref().err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
