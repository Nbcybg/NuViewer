//! Crash safety: panics are written to a log (with a backtrace) instead of vanishing, and a panic in the UI
//! handler shows a message and closes the window cleanly instead of unwinding through the Win32 callback.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Where reports go: `%LOCALAPPDATA%\NuViewer\crash.log` (override with `NUVIEWER_CRASH_LOG`, used by tests).
pub fn log_path() -> PathBuf {
    if let Some(p) = std::env::var_os("NUVIEWER_CRASH_LOG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("crash.log")
}

/// Append a timestamped report; the file is restarted once it passes 1 MB so it can't grow without bound.
pub fn write_report(path: &Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(path).map(|m| m.len() > 1 << 20).unwrap_or(false) {
        let _ = std::fs::remove_file(path);
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "=== unix time {secs} ===\n{text}\n");
    }
}

/// The UI handler panicked (already logged by the panic hook): tell the user, then close the window.
pub(crate) fn fatal(hwnd: windows::Win32::Foundation::HWND) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, PostQuitMessage, MB_ICONERROR, MB_OK};
    let text = format!("NuViewer hit an internal error and has to close.\n\nDetails were saved to:\n{}", log_path().display());
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(Some(hwnd), PCWSTR(wide.as_ptr()), w!("NuViewer"), MB_OK | MB_ICONERROR);
        PostQuitMessage(1);
    }
}

/// Log every panic in the process (including ones that are caught later, e.g. a decoder choking on a bad file).
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let msg = payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "(non-string payload)".into());
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current();
        let report = format!(
            "panic on thread '{}': {msg}\n  at {loc}\nversion {}\n{}",
            thread.name().unwrap_or("?"),
            env!("CARGO_PKG_VERSION"),
            std::backtrace::Backtrace::force_capture()
        );
        write_report(&log_path(), &report);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nv_crash_{tag}_{}_{}.log", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()))
    }

    #[test]
    fn reports_are_appended() {
        let p = temp_file("append");
        write_report(&p, "first problem");
        write_report(&p, "second problem");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("first problem") && text.contains("second problem"));
        assert_eq!(text.matches("=== unix time").count(), 2);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn oversized_log_is_restarted() {
        let p = temp_file("big");
        std::fs::write(&p, vec![b'x'; (1 << 20) + 10]).unwrap();
        write_report(&p, "fresh start");
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("fresh start") && text.len() < 1000, "len {}", text.len());
        let _ = std::fs::remove_file(&p);
    }
}
