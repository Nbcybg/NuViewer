//! ffmpeg as the helper for what Windows cannot decode (10-bit 4:2:2 camera video, MXF, odd containers): a picture of the
//! clip for thumbnails and the viewer, and a small playable copy ("proxy") made in the background and kept in a cache.
//! Optional: without ffmpeg these files get a clear message instead.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

static CONFIGURED: Mutex<String> = Mutex::new(String::new());

/// The path from Settings (may be empty): set at start and whenever the settings change.
pub fn configure(path: &str) {
    *CONFIGURED.lock().unwrap() = path.trim().trim_matches('"').to_string();
}

/// ffmpeg.exe: the configured path, else PATH, the WinGet links folder and the usual install folders.
pub fn find() -> Option<PathBuf> {
    let c = CONFIGURED.lock().unwrap().clone();
    if !c.is_empty() && Path::new(&c).is_file() {
        return Some(PathBuf::from(c));
    }
    let mut places: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join("ffmpeg.exe")).collect()).unwrap_or_default();
    if let Some(l) = std::env::var_os("LOCALAPPDATA") {
        places.push(PathBuf::from(l).join("Microsoft").join("WinGet").join("Links").join("ffmpeg.exe"));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = std::env::var_os(var) {
            places.push(PathBuf::from(p).join("ffmpeg").join("bin").join("ffmpeg.exe"));
        }
    }
    places.push(PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"));
    places.into_iter().find(|p| p.is_file())
}

fn quiet(c: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    c.stdin(Stdio::null()).stderr(Stdio::piped()).creation_flags(0x0800_0000)
}

/// One picture of the clip (a second in, or the first frame for a short one), as a JPEG-decoded image.
pub fn poster(src: &Path) -> Result<nv_codecs::Image, String> {
    let ff = find().ok_or("this video needs a decoder Windows doesn't have; install ffmpeg (winget install Gyan.FFmpeg) to see it")?;
    for ss in ["1", "0"] {
        let mut c = Command::new(&ff);
        c.args(["-v", "error", "-ss", ss, "-i"]).arg(src).args(["-frames:v", "1", "-vf", "scale='min(1920,iw)':-2", "-f", "image2pipe", "-vcodec", "mjpeg", "-q:v", "3", "-"]).stdout(Stdio::piped());
        let out = quiet(&mut c).output().map_err(|e| e.to_string())?;
        if out.status.success() && out.stdout.len() > 200 {
            return nv_codecs::decode_jpeg(&out.stdout);
        }
    }
    Err("ffmpeg could not read a picture from this clip".into())
}

/// Where the playable copy of `src` is kept.
pub fn proxy_path(src: &Path) -> PathBuf {
    let meta = std::fs::metadata(src).ok();
    let (size, mtime) = meta.map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))).unwrap_or((0, 0));
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in src.to_string_lossy().to_lowercase().bytes().chain(size.to_le_bytes()).chain(mtime.to_le_bytes()) {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("proxy").join(format!("{h:016x}.mp4"))
}

/// Make the playable copy: 720p H.264 + AAC, which every Windows can play. Written to a `.part` file and renamed when whole,
/// so a half-made copy is never taken for a finished one. `keep_going` is polled; false kills ffmpeg.
pub fn make_proxy(src: &Path, dest: &Path, keep_going: &dyn Fn() -> bool) -> Result<(), String> {
    let ff = find().ok_or("ffmpeg is not installed")?;
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part.mp4");
    let mut c = Command::new(&ff);
    c.args(["-y", "-v", "error", "-i"]).arg(src).args(["-vf", "scale=-2:'min(720,ih)'", "-c:v", "libx264", "-preset", "veryfast", "-crf", "24", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]).arg(&part).stdout(Stdio::null());
    let mut child = quiet(&mut c).spawn().map_err(|e| e.to_string())?;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) if s.success() => break,
            Some(_) => {
                let mut err = String::new();
                if let Some(mut s) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut s, &mut err);
                }
                let _ = std::fs::remove_file(&part);
                return Err(err.trim().lines().last().unwrap_or("ffmpeg failed").to_string());
            }
            None => {
                if !keep_going() {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&part);
                    return Err("cancelled".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// Keep the proxy cache under `max_bytes` (oldest first).
pub fn prune_proxies(max_bytes: u64) {
    let Some(dir) = proxy_path(Path::new("x")).parent().map(Path::to_path_buf) else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd.flatten().filter_map(|e| e.metadata().ok().map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len(), e.path()))).collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, size, p) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}
