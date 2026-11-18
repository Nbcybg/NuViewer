//! Video frames from ffmpeg, for what Media Foundation cannot give: clips with an alpha channel (ProRes 4444, PNG or
//! Animation in a MOV, VP9 / HEVC with alpha) and codecs Windows has no decoder for. ffmpeg runs as a child process that
//! writes raw BGRA frames to a pipe at the size wanted; the pipe's back-pressure paces it, a seek starts it again at the
//! new spot. Optional: it needs ffmpeg (the program registers where to find it with `set_ffmpeg`).

use crate::readers::{Frame, Read};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Mutex, OnceLock};

type Finder = fn() -> Option<PathBuf>;
static FINDER: OnceLock<Finder> = OnceLock::new();

/// The program says where ffmpeg.exe is (called each time one is needed: the setting may change).
pub fn set_ffmpeg(f: Finder) {
    let _ = FINDER.set(f);
}

pub fn ffmpeg() -> Option<PathBuf> {
    FINDER.get().and_then(|f| f())
}

fn quiet(c: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    c.stdin(Stdio::null()).stderr(Stdio::null()).creation_flags(0x0800_0000)
}

/// Does the first video stream have an alpha channel? (ffprobe; None when it cannot be asked.)
pub fn has_alpha(path: &Path) -> Option<bool> {
    let ff = ffmpeg()?;
    let probe = ff.with_file_name("ffprobe.exe");
    let probe = if probe.is_file() { probe } else { std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("ffprobe.exe")).find(|p| p.is_file()))? };
    let mut c = Command::new(probe);
    c.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=pix_fmt", "-of", "csv=p=0"]).arg(path).stdout(Stdio::piped());
    let out = quiet(&mut c).output().ok()?;
    let fmt = String::from_utf8_lossy(&out.stdout).trim().to_ascii_lowercase();
    if fmt.is_empty() {
        return None;
    }
    Some(pix_fmt_has_alpha(&fmt))
}

pub fn pix_fmt_has_alpha(fmt: &str) -> bool {
    ["yuva", "rgba", "bgra", "argb", "abgr", "gbrap", "ya8", "ya16", "pal8"].iter().any(|k| fmt.contains(k)) && !fmt.starts_with("yuvj")
}

struct State {
    child: Option<Child>,
    out: Option<ChildStdout>,
    /// Where the running ffmpeg started (ms) and how many frames it has given since.
    start_ms: f64,
    n: u64,
    /// Where the next start should be (set by `seek`).
    want_ms: u64,
    ended: bool,
}

pub struct FfReader {
    path: PathBuf,
    ff: PathBuf,
    pub width: u32,
    pub height: u32,
    fps: f64,
    st: Mutex<State>,
}

impl FfReader {
    /// `src_w` x `src_h` is the clip's size (from the probe); the frames come at most `max_edge` on the long side.
    pub fn open(path: &Path, src_w: u32, src_h: u32, fps: f32, max_edge: u32) -> Result<FfReader, String> {
        let ff = ffmpeg().ok_or("this video needs ffmpeg, which is not installed (winget install Gyan.FFmpeg)")?;
        let (mut w, mut h) = (src_w.max(2), src_h.max(2));
        if max_edge > 0 && w.max(h) > max_edge {
            let k = max_edge as f64 / w.max(h) as f64;
            w = (w as f64 * k) as u32;
            h = (h as f64 * k) as u32;
        }
        let (width, height) = ((w & !1).max(2), (h & !1).max(2));
        let r = FfReader { path: path.to_path_buf(), ff, width, height, fps: if fps > 1.0 { fps as f64 } else { 25.0 }, st: Mutex::new(State { child: None, out: None, start_ms: 0.0, n: 0, want_ms: 0, ended: false }) };
        // Start it now so a clip ffmpeg cannot read fails here, not in the middle of the playing.
        r.start(&mut r.st.lock().unwrap())?;
        Ok(r)
    }

    fn start(&self, st: &mut State) -> Result<(), String> {
        Self::stop(st);
        let mut c = Command::new(&self.ff);
        c.args(["-v", "error", "-nostdin"]);
        if st.want_ms > 0 {
            c.args(["-ss", &format!("{}.{:03}", st.want_ms / 1000, st.want_ms % 1000)]);
        }
        c.arg("-i").arg(&self.path);
        c.args(["-an", "-sn", "-vf", &format!("scale={}:{}:flags=fast_bilinear,format=bgra", self.width, self.height), "-f", "rawvideo", "-pix_fmt", "bgra", "-"]);
        c.stdout(Stdio::piped());
        let mut child = quiet(&mut c).spawn().map_err(|e| format!("could not start ffmpeg ({e})"))?;
        st.out = child.stdout.take();
        st.child = Some(child);
        st.start_ms = st.want_ms as f64;
        st.n = 0;
        st.ended = false;
        Ok(())
    }

    fn stop(st: &mut State) {
        st.out = None;
        if let Some(mut c) = st.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn seek(&self, ms: u64) {
        let mut st = self.st.lock().unwrap();
        Self::stop(&mut st);
        st.want_ms = ms;
        st.ended = false;
    }

    pub fn read(&self) -> Read<Frame> {
        let mut st = self.st.lock().unwrap();
        if st.ended {
            return Read::End;
        }
        if st.child.is_none() {
            if let Err(e) = self.start(&mut st) {
                return Read::Error(e);
            }
        }
        let mut buf = vec![0u8; self.width as usize * self.height as usize * 4];
        let got = {
            let Some(out) = st.out.as_mut() else { return Read::End };
            read_full(out, &mut buf)
        };
        if got < buf.len() {
            st.ended = true;
            // Nothing at all from a clip ffmpeg could not decode is an error, a short end is the end of the clip.
            if got == 0 && st.n == 0 && st.want_ms == 0 {
                Self::stop(&mut st);
                return Read::Error("ffmpeg could not decode this video".into());
            }
            return Read::End;
        }
        let ms = (st.start_ms + st.n as f64 * 1000.0 / self.fps).round() as u64;
        st.n += 1;
        Read::Item(Frame { width: self.width, height: self.height, bgra: buf, ms })
    }
}

fn read_full(r: &mut ChildStdout, buf: &mut [u8]) -> usize {
    let mut at = 0;
    while at < buf.len() {
        match r.read(&mut buf[at..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => at += n,
        }
    }
    at
}

impl Drop for FfReader {
    fn drop(&mut self) {
        if let Ok(mut st) = self.st.lock() {
            Self::stop(&mut st);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_pixel_formats() {
        for f in ["yuva444p12le", "rgba", "bgra", "argb", "gbrap", "ya8", "yuva420p", "pal8"] {
            assert!(pix_fmt_has_alpha(f), "{f}");
        }
        for f in ["yuv420p", "yuvj420p", "yuv422p10le", "rgb24", "gray", "nv12"] {
            assert!(!pix_fmt_has_alpha(f), "{f}");
        }
    }
}
