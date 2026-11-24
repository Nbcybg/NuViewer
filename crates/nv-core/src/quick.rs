//! Super-quick conversion: "to PNG" and "to JPEG" with no questions. The result goes next to the original with the same
//! name (a free " (2)" name if that is taken), keeps the file's date, and loses nothing the format can keep:
//! PNG is lossless; JPEG is written at quality 100 with full-resolution colour (4:4:4) and the camera's EXIF.
//!
//! The same code serves the right-click menus of NuViewer, NuGallery and NuEditor and Explorer's own right-click menu
//! (`nuviewer.exe --to-png file ...`). Explorer starts one process per selected file, so [`run_cli`] lets the first process
//! collect the others' file names for a moment and convert them all, one after the other.

use nv_editor::convert::{self, Out};
use nv_editor::raster::Pixels;
use nv_editor::save;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Png,
    Jpeg,
}

impl Target {
    pub fn ext(self) -> &'static str {
        match self {
            Target::Png => "png",
            Target::Jpeg => "jpg",
        }
    }

    pub fn flag(self) -> &'static str {
        match self {
            Target::Png => "--to-png",
            Target::Jpeg => "--to-jpg",
        }
    }

    pub fn from_flag(s: &str) -> Option<Target> {
        match s {
            "--to-png" => Some(Target::Png),
            "--to-jpg" => Some(Target::Jpeg),
            _ => None,
        }
    }

    /// Is `path` already in this format?
    pub fn is_already(self, path: &Path) -> bool {
        let e = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match self {
            Target::Png => e == "png",
            Target::Jpeg => matches!(e.as_str(), "jpg" | "jpeg" | "jpe" | "jfif"),
        }
    }
}

/// Where the converted copy goes: next to `src`, same name, new extension, never over a file that exists.
pub fn output_for(src: &Path, t: Target) -> PathBuf {
    let dir = src.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    crate::fileops::unique_path(dir, std::ffi::OsStr::new(&format!("{stem}.{}", t.ext())))
}

/// Convert one file. Returns the new file.
pub fn convert_file(src: &Path, t: Target) -> Result<PathBuf, String> {
    if t.is_already(src) {
        return Err(format!("already a {}", t.ext().to_uppercase()));
    }
    let img = nv_codecs::decode_file_first_frame(src)?;
    if img.mesh.is_some() {
        return Err("a 3D model is not a picture".into());
    }
    if img.reduced_from.is_some() {
        return Err("too large to convert without losing pixels".into());
    }
    let img = img.finish_for_cpu(); // embedded profile -> sRGB, upright; the display profile is never baked in
    let px = Pixels::new(img.width, img.height, img.rgba);
    let exif = match t {
        Target::Jpeg if matches!(src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg" | "jpe" | "jfif")) => {
            std::fs::read(src).ok().and_then(|b| save::exif_of_jpeg(&b, (px.w, px.h)))
        }
        _ => None,
    };
    let out = match t {
        Target::Png => Out::Png,
        Target::Jpeg => Out::Jpeg(100),
    };
    let bytes = convert::encode(&px, out, [255, 255, 255], exif.as_deref())?;
    let dst = output_for(src, t);
    save::write_atomic(&dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    if let Some((_, modified)) = crate::batch::read_file_times(src) {
        let _ = crate::batch::set_file_times(&dst, modified, crate::batch::WhichTimes { created: false, modified: true, accessed: false });
    }
    Ok(dst)
}

/// The file formats "Save as" can write, as `(extension, name)`; the extension picks the encoder.
pub const SAVE_AS_FORMATS: &[(&str, &str)] = &[("png", "PNG"), ("jpg", "JPEG"), ("webp", "WebP (lossless)"), ("bmp", "BMP"), ("tif", "TIFF"), ("tga", "TGA")];

fn out_for(ext: &str, quality: u8) -> Option<Out> {
    match ext {
        "png" => Some(Out::Png),
        "jpg" | "jpeg" | "jpe" | "jfif" => Some(Out::Jpeg(quality.clamp(1, 100))),
        "webp" => Some(Out::Webp),
        "bmp" => Some(Out::Bmp),
        "tif" | "tiff" => Some(Out::Tiff),
        "tga" => Some(Out::Tga),
        _ => None,
    }
}

fn same_format(a: &str, b: &str) -> bool {
    let norm = |e: &str| match e {
        "jpeg" | "jpe" | "jfif" => "jpg".to_string(),
        "tiff" => "tif".to_string(),
        o => o.to_string(),
    };
    norm(a) == norm(b)
}

/// "Save as": a copy of `src` written to `dst`, in the format the extension of `dst` names. The same format as the original is a
/// plain copy of the file (nothing is re-encoded, so nothing is lost); another format is decoded (embedded profile -> sRGB, upright)
/// and encoded again, first frame only. Returns the file written.
pub fn save_as(src: &Path, dst: &Path, jpeg_quality: u8) -> Result<PathBuf, String> {
    if src == dst {
        return Err("that is the file itself - pick another name".into());
    }
    let ext = |p: &Path| p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let (se, de) = (ext(src), ext(dst));
    if same_format(&se, &de) {
        std::fs::copy(src, dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        return Ok(dst.to_path_buf());
    }
    let out = out_for(&de, jpeg_quality).ok_or_else(|| format!("can't write .{de} files (use png, jpg, webp, bmp, tif or tga)"))?;
    let img = nv_codecs::decode_file_first_frame(src)?;
    if img.mesh.is_some() {
        return Err("a 3D model is not a picture".into());
    }
    if img.reduced_from.is_some() {
        return Err("too large to save without losing pixels".into());
    }
    let img = img.finish_for_cpu();
    let px = Pixels::new(img.width, img.height, img.rgba);
    let bytes = convert::encode(&px, out, [255, 255, 255], None)?;
    save::write_atomic(dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(dst.to_path_buf())
}

#[derive(Default, Debug)]
pub struct Report {
    pub made: Vec<PathBuf>,
    pub skipped: usize,
    pub failed: Vec<(PathBuf, String)>,
}

impl Report {
    /// One line for a message: what was made, what was not.
    pub fn summary(&self, t: Target) -> String {
        let mut s = format!("{} converted to {}", self.made.len(), t.ext().to_uppercase());
        if self.skipped > 0 {
            s += &format!(", {} already in that format", self.skipped);
        }
        if let Some((p, e)) = self.failed.first() {
            s += &format!(", {} failed (first: {}: {e})", self.failed.len(), p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
        s
    }
}

pub fn convert_many(files: &[PathBuf], t: Target, mut progress: impl FnMut(usize, usize) -> bool) -> Report {
    let mut r = Report::default();
    for (i, f) in files.iter().enumerate() {
        if !progress(i, files.len()) {
            break;
        }
        if t.is_already(f) {
            r.skipped += 1;
            continue;
        }
        match convert_file(f, t) {
            Ok(p) => r.made.push(p),
            Err(e) => r.failed.push((f.clone(), e)),
        }
    }
    r
}

// ---- Explorer: one process per file, one batch ----

fn queue_dir() -> PathBuf {
    std::env::var_os("NUVIEWER_QUEUE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("NuViewer").join("convq"))
}

fn stamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// Take every waiting list (`png-*.lst` / `jpg-*.lst`) out of the queue.
fn take_lists(dir: &Path) -> Vec<(Target, Vec<PathBuf>)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut names: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "lst")).collect();
    names.sort();
    for p in names {
        let t = if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("png-")) { Target::Png } else { Target::Jpeg };
        let work = p.with_extension("work");
        if std::fs::rename(&p, &work).is_err() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&work) {
            out.push((t, text.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()));
        }
        let _ = std::fs::remove_file(&work);
    }
    out
}

/// What Explorer's menu entry runs: add `files` to the queue; if no other process is converting, become the one that does
/// and return its report (the others return `None` at once). Quiet: the caller shows a message only for failures.
pub fn run_cli(t: Target, files: Vec<PathBuf>) -> Option<Report> {
    let dir = queue_dir();
    let _ = std::fs::create_dir_all(&dir);
    let prefix = if t == Target::Png { "png" } else { "jpg" };
    let list = dir.join(format!("{prefix}-{:039}-{}.lst", stamp(), std::process::id()));
    let text: String = files.iter().map(|f| format!("{}\n", f.display())).collect();
    let tmp = list.with_extension("tmp");
    if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &list)).is_err() {
        return None;
    }
    let lock = dir.join("leader.lock");
    let mut total = Report::default();
    let mut led = false;
    loop {
        // A lock older than a minute belongs to a process that died.
        if let Ok(m) = std::fs::metadata(&lock) {
            if m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e > Duration::from_secs(60)) {
                let _ = std::fs::remove_file(&lock);
            }
        }
        if std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).is_err() {
            return if led { Some(total) } else { None };
        }
        led = true;
        let deadline = Instant::now() + Duration::from_millis(350);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50)); // let the rest of the selection arrive
        }
        loop {
            let lists = take_lists(&dir);
            if lists.is_empty() {
                break;
            }
            for (tg, files) in lists {
                let r = convert_many(&files, tg, |_, _| {
                    let _ = std::fs::OpenOptions::new().write(true).open(&lock); // keeps the lock fresh on long jobs
                    true
                });
                total.made.extend(r.made);
                total.skipped += r.skipped;
                total.failed.extend(r.failed);
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        let _ = std::fs::remove_file(&lock);
        // Someone may have queued a list just before the lock went: look once more.
        let more = std::fs::read_dir(&dir).map(|rd| rd.flatten().any(|e| e.path().extension().is_some_and(|x| x == "lst"))).unwrap_or(false);
        if !more {
            return Some(total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_quick_{tag}_{}_{}", std::process::id(), stamp()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_png(path: &Path, w: u32, h: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i * 7 % 253) as u8, (i * 13 % 255) as u8, 255]).collect();
        let bytes = convert::encode(&Pixels::new(w, h, rgba.clone()), Out::Png, [255; 3], None).unwrap();
        std::fs::write(path, bytes).unwrap();
        rgba
    }

    #[test]
    fn png_to_jpeg_and_back_keep_the_picture_and_never_overwrite() {
        let d = temp("rt");
        let png = d.join("a.png");
        let rgba = write_png(&png, 64, 48);
        let jpg = convert_file(&png, Target::Jpeg).unwrap();
        assert_eq!(jpg, d.join("a.jpg"));
        let again = convert_file(&png, Target::Jpeg).unwrap();
        assert_eq!(again, d.join("a (2).jpg"), "a second conversion gets a free name");
        // JPEG -> PNG is lossless with respect to what the JPEG decodes to.
        let back = convert_file(&jpg, Target::Png).unwrap();
        let a = nv_codecs::decode_file_first_frame(&jpg).unwrap().finish_for_cpu();
        let b = nv_codecs::decode_file_first_frame(&back).unwrap().finish_for_cpu();
        assert_eq!(a.rgba, b.rgba);
        // Quality 100 / 4:4:4: the JPEG stays close to the PNG it came from.
        let worst = rgba.iter().zip(&a.rgba).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap();
        assert!(worst <= 24, "worst channel error {worst}");
        assert!(convert_file(&png, Target::Png).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_queue_collects_several_processes_into_one_batch() {
        let d = temp("q");
        std::env::set_var("NUVIEWER_QUEUE_DIR", d.join("q"));
        let files: Vec<PathBuf> = (0..4).map(|i| {
            let p = d.join(format!("f{i}.png"));
            write_png(&p, 16, 16);
            p
        }).collect();
        let handles: Vec<_> = files.iter().cloned().map(|f| std::thread::spawn(move || run_cli(Target::Jpeg, vec![f]))).collect();
        let reports: Vec<Option<Report>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let leaders = reports.iter().filter(|r| r.is_some()).count();
        assert!(leaders >= 1);
        for i in 0..4 {
            assert!(d.join(format!("f{i}.jpg")).is_file(), "f{i}.jpg");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
