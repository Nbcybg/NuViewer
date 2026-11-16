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

/// Raw `.pcm` has no header: say what it is (16-bit little-endian stereo, CD rate is the usual guess).
fn pre_input(src: &Path) -> Vec<&'static str> {
    if src.extension().is_some_and(|e| e.eq_ignore_ascii_case("pcm")) {
        vec!["-f", "s16le", "-ar", "44100", "-ac", "2"]
    } else {
        Vec::new()
    }
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

/// Make the playable copy: 720p H.264 + AAC (a sound: AAC only), which every Windows can play. HDR (HDR10 / HLG) is
/// tone-mapped to SDR here, other video is converted to BT.709 limited range and tagged, so colours are right in the player.
/// Written to a `.part` file and renamed when whole, so a half-made copy is never taken for a finished one. `keep_going`
/// is polled; false kills ffmpeg.
pub fn make_proxy(src: &Path, dest: &Path, keep_going: &dyn Fn() -> bool) -> Result<(), String> {
    let ff = find().ok_or("ffmpeg is not installed")?;
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part.mp4");
    let info = probe(src).ok();
    let mut c = Command::new(&ff);
    c.args(["-y", "-v", "error"]).args(pre_input(src)).arg("-i").arg(src);
    if info.as_ref().is_some_and(|i| !i.has_video) || (info.is_none() && nv_codecs::media::media_kind_of(src) == Some(nv_codecs::media::MediaKind::Audio)) {
        c.args(["-vn", "-c:a", "aac", "-b:a", "256k", "-ar", "48000", "-ac", "2", "-movflags", "+faststart"]);
    } else {
        c.args(["-vf", &proxy_filter(info.as_ref())]).args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "22", "-pix_fmt", "yuv420p", "-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
        c.args(["-c:a", "aac", "-b:a", "160k", "-ac", "2", "-movflags", "+faststart"]);
    }
    c.arg(&part).stdout(Stdio::null());
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

// ---- what a clip is: ffprobe ----

/// What ffprobe says about a clip.
#[derive(Clone, Debug)]
pub struct Probe {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration_s: f64,
    /// Where the first video frame is on ffmpeg's timeline (ms): 0 for nearly everything.
    pub start_ms: u64,
    pub has_video: bool,
    pub has_audio: bool,
    pub pix_fmt: String,
    pub colour: crate::videocolour::VideoColour,
}

/// ffprobe.exe beside ffmpeg.exe (or on the PATH).
pub fn ffprobe() -> Option<PathBuf> {
    let ff = find()?;
    let side = ff.with_file_name("ffprobe.exe");
    if side.is_file() {
        return Some(side);
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("ffprobe.exe")).find(|p| p.is_file()))
}

fn parse_fraction(s: &str) -> f32 {
    match s.split_once('/') {
        Some((a, b)) => {
            let (a, b) = (a.parse::<f32>().unwrap_or(0.0), b.parse::<f32>().unwrap_or(1.0));
            if b > 0.0 { a / b } else { 0.0 }
        }
        None => s.parse().unwrap_or(0.0),
    }
}

/// Parse `ffprobe -of default=nw=1` output (`key=value` lines; stream keys for the first video stream, `a_` is added by the caller).
pub fn parse_probe(video: &str, audio_count: usize, duration: &str) -> Probe {
    let get = |k: &str| video.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").trim().to_string();
    let (w, h) = (get("width").parse().unwrap_or(0), get("height").parse().unwrap_or(0));
    let pix_fmt = get("pix_fmt");
    let bits = get("bits_per_raw_sample").parse::<u8>().ok().filter(|b| *b > 0).unwrap_or_else(|| {
        if pix_fmt.contains("12") {
            12
        } else if pix_fmt.contains("10") {
            10
        } else {
            8
        }
    });
    let fps = {
        let f = parse_fraction(&get("avg_frame_rate"));
        if f > 0.0 { f } else { parse_fraction(&get("r_frame_rate")) }
    };
    Probe {
        width: w,
        height: h,
        fps,
        duration_s: duration.trim().parse().unwrap_or(0.0),
        start_ms: (get("start_time").parse::<f64>().unwrap_or(0.0).max(0.0) * 1000.0).round() as u64,
        has_video: w > 0 && h > 0,
        has_audio: audio_count > 0,
        colour: crate::videocolour::VideoColour::from_probe(&get("color_space"), &get("color_primaries"), &get("color_transfer"), &get("color_range"), w, h, bits),
        pix_fmt,
    }
}

pub fn probe(src: &Path) -> Result<Probe, String> {
    let pr = ffprobe().ok_or("ffprobe not found")?;
    let run = |args: &[&str]| -> Result<String, String> {
        let mut c = Command::new(&pr);
        c.args(["-v", "error"]).args(args).arg(src).stdout(Stdio::piped());
        let out = quiet(&mut c).output().map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let video = run(&["-select_streams", "v:0", "-show_entries", "stream=width,height,start_time,pix_fmt,bits_per_raw_sample,color_range,color_space,color_transfer,color_primaries,avg_frame_rate,r_frame_rate", "-of", "default=nw=1"])?;
    let audio = run(&["-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"])?;
    let dur = run(&["-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])?;
    Ok(parse_probe(&video, audio.lines().filter(|l| !l.trim().is_empty()).count(), &dur))
}

static FILTERS: Mutex<Option<String>> = Mutex::new(None);

/// Does this ffmpeg build have the filter (zscale needs libzimg, which the "full" builds have)?
pub fn has_filter(name: &str) -> bool {
    let mut g = FILTERS.lock().unwrap();
    if g.is_none() {
        let text = find()
            .and_then(|ff| {
                let mut c = Command::new(ff);
                c.args(["-hide_banner", "-filters"]).stdout(Stdio::piped());
                quiet(&mut c).output().ok()
            })
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        *g = Some(text);
    }
    g.as_ref().unwrap().lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// The video filter chain of a playable copy: 720p, colours converted to BT.709 (HDR tone-mapped when ffmpeg can).
pub fn proxy_filter(info: Option<&Probe>) -> String {
    let size = "scale=-2:'min(720,ih)'";
    let Some(i) = info else { return format!("{size}:out_color_matrix=bt709:out_range=tv") };
    if i.colour.is_hdr() && has_filter("zscale") && has_filter("tonemap") {
        return format!("zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv,{size}");
    }
    format!("{size}:{}:out_color_matrix=bt709:out_range=tv", i.colour.scale_args().replace(":out_range=full", "").replace("in_color_matrix", "in_color_matrix"))
}

/// One frame of a clip at `ms` as 16-bit RGB numbers (the YUV to RGB step done with the clip's own matrix and range; the
/// transfer curve and primaries are still the clip's). Full size, upright.
pub fn grab_rgb16(src: &Path, ms: u64, colour: &crate::videocolour::VideoColour) -> Result<(u32, u32, Vec<u16>), String> {
    let ff = find().ok_or("ffmpeg is not installed")?;
    let t = format!("{}.{:03}", ms / 1000, ms % 1000);
    let vf = format!("scale={}:flags=accurate_rnd+full_chroma_int+bicubic,format=rgb48be", colour.scale_args());
    let mut c = Command::new(&ff);
    c.args(["-v", "error", "-ss", &t, "-i"]).arg(src).args(["-frames:v", "1", "-vf", &vf, "-c:v", "png", "-compression_level", "1", "-f", "image2pipe", "-"]).stdout(Stdio::piped());
    let out = quiet(&mut c).output().map_err(|e| e.to_string())?;
    if !out.status.success() || out.stdout.len() < 64 {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg returned no picture").to_string());
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(&out.stdout));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; r.output_buffer_size().ok_or("picture too large")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.bit_depth != png::BitDepth::Sixteen || info.color_type != png::ColorType::Rgb {
        return Err("ffmpeg did not return 16-bit RGB".into());
    }
    let px: Vec<u16> = buf[..info.buffer_size()].chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
    Ok((info.width, info.height, px))
}

/// The loudness outline of a sound ffmpeg can decode: `cols` (min, max) pairs, one channel.
pub fn audio_peaks(src: &Path, cols: usize) -> Result<Vec<Vec<(f32, f32)>>, String> {
    let ff = find().ok_or("ffmpeg is not installed")?;
    let mut c = Command::new(&ff);
    c.args(["-v", "error"]).args(pre_input(src)).arg("-i").arg(src).args(["-vn", "-ac", "1", "-ar", "8000", "-t", "7200", "-f", "s16le", "-"]).stdout(Stdio::piped());
    let out = quiet(&mut c).output().map_err(|e| e.to_string())?;
    let n = out.stdout.len() / 2;
    if n == 0 || cols == 0 {
        return Err("ffmpeg found no sound in this file".into());
    }
    let sample = |i: usize| i16::from_le_bytes([out.stdout[i * 2], out.stdout[i * 2 + 1]]) as f32 / 32768.0;
    let mut v = Vec::with_capacity(cols);
    for col in 0..cols {
        let (a, b) = (col * n / cols, ((col + 1) * n / cols).max(col * n / cols + 1).min(n));
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for i in a..b {
            let s = sample(i);
            lo = lo.min(s);
            hi = hi.max(s);
        }
        v.push((lo, hi));
    }
    Ok(vec![v])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_parses() {
        let v = "width=3840\nheight=2160\nstart_time=0.040000\npix_fmt=yuv420p10le\nbits_per_raw_sample=10\ncolor_range=tv\ncolor_space=bt2020nc\ncolor_transfer=smpte2084\ncolor_primaries=bt2020\navg_frame_rate=30000/1001\nr_frame_rate=30000/1001\n";
        let p = parse_probe(v, 1, "12.5\n");
        assert_eq!((p.width, p.height, p.has_audio), (3840, 2160, true));
        assert!(p.colour.is_hdr());
        assert_eq!(p.colour.bits, 10);
        assert!((p.fps - 29.97).abs() < 0.01);
        assert!((p.duration_s - 12.5).abs() < 1e-6);
        assert_eq!(p.start_ms, 40);
    }
}
