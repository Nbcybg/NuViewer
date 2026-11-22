//! A modal "working..." window for a job that takes a while (extracting the frames of a video, the pages of a PDF): a bar, a line of
//! text and a Cancel button. The job runs on its own thread; the window ticks every 100 ms and closes itself when the job ends.
//! The window of the program stays where it is (the form is owned by it, like every other dialog) and cannot be clicked meanwhile.

use crate::ui::{Form, ID_CANCEL};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const ID_TICK: u16 = 61_001;
const ID_FINISHED: u16 = 61_002;
const ID_BAR: u16 = 61_003;
const ID_LINE: u16 = 61_004;
const ID_COUNT: u16 = 61_005;

/// What the job tells the window: how far it is, and a line of text (the file it is on).
pub struct Report {
    done: AtomicUsize,
    total: AtomicUsize,
    line: Mutex<String>,
}

impl Report {
    /// `done` of `total` (0 = not known).
    pub fn set(&self, done: usize, total: usize) {
        self.done.store(done, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn line(&self, text: &str) {
        *self.line.lock().unwrap() = text.to_string();
    }
}

/// "12 of 340" / "12" for the count under the bar.
pub fn count_text(done: usize, total: usize) -> String {
    if total > 0 { format!("{done} of {total}") } else { format!("{done}") }
}

/// Run `job` on a thread with a progress window over the program's window. The job gets a cancel flag (true once the user pressed
/// Cancel or closed the window: it should stop soon and return what it has) and a `Report`. Returns the job's result.
pub fn run<R: Send + 'static>(title: &str, first_line: &str, job: impl FnOnce(&AtomicBool, &Report) -> R + Send + 'static) -> Option<R> {
    let form = Form::new(title, 460, 150).ok()?;
    form.label_id(ID_LINE, 16, 14, 428, 40, first_line);
    form.progress(ID_BAR, 16, 60, 428, 18);
    form.label_id(ID_COUNT, 16, 84, 300, 20, "");
    form.button(ID_CANCEL, 354, 106, 90, 28, "Cancel", false);
    let hwnd_value = form.hwnd.0 as isize;
    let cancel = Arc::new(AtomicBool::new(false));
    let report = Arc::new(Report { done: AtomicUsize::new(0), total: AtomicUsize::new(0), line: Mutex::new(String::new()) });
    let (c2, r2) = (cancel.clone(), report.clone());
    let worker = std::thread::spawn(move || {
        let out = job(&c2, &r2);
        let _ = crate::post_command(hwnd_value, ID_FINISHED);
        out
    });
    // The ticker: the window asks for the numbers 10 times a second.
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let ticker = std::thread::spawn(move || {
        while !s2.load(Ordering::Relaxed) {
            if !crate::post_command(hwnd_value, ID_TICK) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    let (c3, r3) = (cancel.clone(), report.clone());
    let mut last_line = String::new();
    form.run(&mut |f, id| match id {
        ID_TICK => {
            let (done, total) = (r3.done.load(Ordering::Relaxed), r3.total.load(Ordering::Relaxed));
            f.progress_set(ID_BAR, done, total);
            f.set_text(ID_COUNT, &if c3.load(Ordering::Relaxed) { "Stopping...".to_string() } else { count_text(done, total) });
            let line = r3.line.lock().unwrap().clone();
            if !line.is_empty() && line != last_line {
                f.set_text(ID_LINE, &line);
                last_line = line;
            }
            false
        }
        ID_FINISHED => true,
        ID_CANCEL => {
            c3.store(true, Ordering::Relaxed); // the job stops, then it posts ID_FINISHED
            false
        }
        _ => false,
    });
    stop.store(true, Ordering::Relaxed);
    // The window was closed by the X while the job still runs: ask it to stop, and wait for it.
    cancel.store(true, Ordering::Relaxed);
    let out = worker.join().ok();
    let _ = ticker.join();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_reads_naturally() {
        assert_eq!(count_text(12, 340), "12 of 340");
        assert_eq!(count_text(7, 0), "7");
    }

    #[test]
    fn a_report_keeps_the_last_numbers_and_line() {
        let r = Report { done: AtomicUsize::new(0), total: AtomicUsize::new(0), line: Mutex::new(String::new()) };
        r.set(3, 10);
        r.line("a.mp4");
        assert_eq!((r.done.load(Ordering::Relaxed), r.total.load(Ordering::Relaxed)), (3, 10));
        assert_eq!(&*r.line.lock().unwrap(), "a.mp4");
    }
}
