//! Talking to the NuQuick agent (`nuquick.exe`, the Space-bar preview) from the other programs: is it running, start it,
//! stop it, and the "start with Windows" entry. Used by NuViewer's Settings window and by NuQuick itself.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_APP};

/// Window class of the agent's hidden window (it is also how a second start finds the first).
pub const AGENT_CLASS: PCWSTR = w!("NuQuickAgent");
/// Posted to the agent window: exit.
pub const MSG_QUIT: u32 = WM_APP + 11;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "NuQuick";
pub const EXE_NAME: &str = "nuquick.exe";

pub fn agent_window() -> Option<HWND> {
    unsafe { FindWindowW(AGENT_CLASS, PCWSTR::null()).ok() }
}

pub fn is_running() -> bool {
    agent_window().is_some()
}

/// `nuquick.exe` in the folder of the running program, if it is there.
pub fn exe_next_to_us() -> Option<PathBuf> {
    let p = std::env::current_exe().ok()?.parent()?.join(EXE_NAME);
    p.exists().then_some(p)
}

/// Start the agent (nothing happens if it already runs). Waits up to 3 s for its window so the caller can show the truth.
pub fn start() -> Result<(), String> {
    if is_running() {
        return Ok(());
    }
    let exe = exe_next_to_us().ok_or_else(|| format!("{EXE_NAME} is not in the same folder as this program"))?;
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        std::process::Command::new(&exe).arg("--silent").creation_flags(DETACHED_PROCESS).spawn().map_err(|e| format!("couldn't start {}: {e}", exe.display()))?;
    }
    wait_for(true, Duration::from_secs(3)).then_some(()).ok_or_else(|| "NuQuick did not start (it shows up as an icon near the clock)".to_string())
}

/// Ask the agent to exit and wait (up to 2 s) until it has.
pub fn stop() -> bool {
    let Some(h) = agent_window() else { return true };
    unsafe {
        let _ = PostMessageW(Some(h), MSG_QUIT, WPARAM(0), LPARAM(0));
    }
    wait_for(false, Duration::from_secs(2))
}

fn wait_for(running: bool, limit: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < limit {
        if is_running() == running {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    is_running() == running
}

pub fn startup_enabled() -> bool {
    crate::registry::read_string(RUN_KEY, Some(RUN_NAME)).is_some()
}

/// Add / remove the Run entry (per user) that starts `exe` when the user signs in.
pub fn set_startup(on: bool, exe: &Path) -> Result<(), String> {
    if on {
        crate::registry::set_string(RUN_KEY, Some(RUN_NAME), &format!("\"{}\" --silent", exe.display()))
    } else {
        crate::registry::delete_value(RUN_KEY, RUN_NAME);
        Ok(())
    }
}
