//! The Settings window's "Space preview" page: switch the NuQuick agent (`nuquick.exe`) on and off, and its "start with Windows" entry.
//! The state shown is the real one (is its window there, is the Run entry there), not a copy kept in settings.ini.

use nv_win::quick;

pub fn running() -> bool {
    quick::is_running()
}

pub fn startup() -> bool {
    quick::startup_enabled()
}

fn found() -> bool {
    quick::exe_next_to_us().is_some()
}

/// One line about now.
pub fn status() -> String {
    if !found() && !running() {
        return "nuquick.exe was not found next to nuviewer.exe, so it can't be started from here.".to_string();
    }
    format!("Now: NuQuick is {}; it {} start with Windows.", if running() { "running" } else { "not running" }, if startup() { "will" } else { "will not" })
}

/// What OK will do with these two boxes.
pub fn status_after(run: bool, start: bool) -> String {
    let mut todo = Vec::new();
    if run && !running() {
        todo.push("start NuQuick");
    }
    if !run && running() {
        todo.push("stop NuQuick");
    }
    if start != startup() {
        todo.push(if start { "start it with Windows from now on" } else { "stop starting it with Windows" });
    }
    if todo.is_empty() { status() } else { format!("OK will: {}.", todo.join(", ")) }
}

/// Make the real state match the boxes. An error is shown to the user as it is.
pub fn apply(run: bool, start: bool) -> Result<(), String> {
    if start != startup() {
        match quick::exe_next_to_us() {
            Some(exe) => quick::set_startup(start, &exe)?,
            None if !start => quick::set_startup(false, std::path::Path::new(""))?,
            None => return Err("nuquick.exe was not found next to nuviewer.exe, so it can't be set to start with Windows.".into()),
        }
    }
    if run && !running() {
        quick::start()?;
    } else if !run && running() && !quick::stop() {
        return Err("NuQuick did not stop. Use Quit from its icon near the clock.".into());
    }
    Ok(())
}
