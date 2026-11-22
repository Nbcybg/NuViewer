//! Picture formats added by plug-ins (see `nv_core::plugins` and `docs/plugins.md`).
//!
//! A decoder plug-in is a program that turns one file into a PNG: NuViewer runs it with the input and a scratch output path, waits
//! (with a time limit), reads the PNG and deletes it. Nothing is loaded into NuViewer's own process, so a plug-in that crashes or
//! hangs cannot take the viewer down, and a plug-in can only *add* formats: an extension NuViewer reads itself is never handed over.

use crate::Image;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// One extension handler.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoder {
    /// Which plug-in it belongs to (shown in error messages).
    pub plugin: String,
    /// Lower case, no dot.
    pub exts: Vec<String>,
    pub exe: PathBuf,
    /// Arguments with `{input}` and `{output}` still in them; each element is one argument (no shell is involved).
    pub args: Vec<String>,
    pub timeout: Duration,
}

static DECODERS: RwLock<Vec<Decoder>> = RwLock::new(Vec::new());
static SCRATCH: AtomicU32 = AtomicU32::new(0);

/// Replace the registered decoders. Extensions that NuViewer reads by itself are dropped (checked here, once).
pub fn set_decoders(list: Vec<Decoder>) {
    DECODERS.write().unwrap().clear(); // what an earlier load added must not count as "read by NuViewer itself"
    let mut kept = Vec::new();
    for mut d in list {
        d.exts.retain(|e| !crate::is_supported_ext(e));
        if !d.exts.is_empty() {
            kept.push(d);
        }
    }
    *DECODERS.write().unwrap() = kept;
}

/// Is `ext` (lower case) one a plug-in reads?
pub fn handles(ext: &str) -> bool {
    DECODERS.read().map(|d| d.iter().any(|d| d.exts.iter().any(|e| e == ext))).unwrap_or(false)
}

/// All extensions plug-ins read, for the lists that show what is supported.
pub fn extensions() -> Vec<String> {
    DECODERS.read().map(|d| d.iter().flat_map(|d| d.exts.iter().cloned()).collect()).unwrap_or_default()
}

/// Decode `path` with the plug-in that reads `ext`; `None` if no plug-in does.
pub fn decode(path: &Path, ext: &str) -> Option<Result<Image, String>> {
    let d = DECODERS.read().ok()?.iter().find(|d| d.exts.iter().any(|e| e == ext))?.clone();
    Some(run(&d, path))
}

fn run(d: &Decoder, input: &Path) -> Result<Image, String> {
    let out = std::env::temp_dir().join(format!("nuviewer-plugin-{}-{}.png", std::process::id(), SCRATCH.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_file(&out);
    let result = run_to(d, input, &out);
    let bytes = result.and_then(|_| std::fs::read(&out).map_err(|_| format!("the plug-in \"{}\" did not make a picture", d.plugin)));
    let _ = std::fs::remove_file(&out);
    crate::pngdec::decode(&bytes?, true).map_err(|e| format!("the plug-in \"{}\" made a file that is not a PNG ({e})", d.plugin))
}

fn run_to(d: &Decoder, input: &Path, out: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(&d.exe);
    for a in &d.args {
        cmd.arg(a.replace("{input}", &input.to_string_lossy()).replace("{output}", &out.to_string_lossy()));
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Some(dir) = d.exe.parent() {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("the plug-in \"{}\" could not start: {e}", d.plugin))?;
    let deadline = Instant::now() + d.timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("the plug-in \"{}\" could not read this file (exit code {})", d.plugin, status.code().unwrap_or(-1))),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("the plug-in \"{}\" took longer than {} s and was stopped", d.plugin, d.timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(8)),
            Err(e) => return Err(format!("the plug-in \"{}\": {e}", d.plugin)),
        }
    }
}
