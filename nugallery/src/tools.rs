//! Archives and PDF in NuGallery: make a PDF from pictures, compress, unpack, and open an archive as if it were a folder
//! (its pictures are unpacked into a cache folder, `nv_core::archive_tools::cache_dir`, and that folder is shown; the way
//! back leads to the archive's own folder). Passwords are asked for in a small dialog.

use crate::app::{App, JobResult, Msg};
use nv_core::archive_tools::{self as at, ArcError};
use nv_core::fileops;
use nv_win::ui::{Form, ID_CANCEL, ID_OK};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An archive being shown as a folder: where it is and where its pictures were unpacked.
#[derive(Clone)]
pub struct ArchiveView {
    pub archive: PathBuf,
    pub dir: PathBuf,
}

/// The result of unpacking, handed from the worker thread to the UI thread.
pub struct ArchiveDone {
    pub archive: PathBuf,
    pub dest: PathBuf,
    pub password: Option<String>,
    pub result: Result<(), ArcError>,
    /// Show the result as a folder (opening) rather than only report it (unpacking next to the archive).
    pub open: bool,
}

const PASSWORD_EDIT: u16 = 10;

/// Ask for a password. `again`: the last one was wrong. None = cancelled.
pub fn ask_password(archive: &Path, again: bool) -> Option<String> {
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let form = Form::new("Password", 440, 150).ok()?;
    form.begin_page(0);
    form.label(16, 14, 408, 20, &format!("{name} is protected."));
    form.label(16, 38, 408, 20, if again { "That password did not work. Try again:" } else { "Password:" });
    form.password(PASSWORD_EDIT, 16, 62, 408, 24);
    form.button(ID_OK, 244, 104, 86, 28, "OK", true);
    form.button(ID_CANCEL, 338, 104, 86, 28, "Cancel", false);
    let mut out = None;
    form.run(&mut |f, id| match id {
        ID_OK => {
            out = Some(f.text(PASSWORD_EDIT));
            true
        }
        ID_CANCEL => true,
        _ => false,
    });
    out
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Files the viewer can show (pictures, documents, models, sounds, videos ...) as opposed to anything else in an archive.
fn showable(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("");
    nv_codecs::is_supported_ext(ext) && !at::is_archive(Path::new(name))
}

impl App {
    // ---- archives as folders -------------------------------------------------------------------------

    /// The path to show for the folder on screen: inside an archive it is "archive.zip\sub\folder".
    pub fn display_dir(&self) -> PathBuf {
        match &self.archive {
            Some(a) => match self.listing.dir.strip_prefix(&a.dir) {
                Ok(rel) => a.archive.join(rel),
                Err(_) => self.listing.dir.clone(),
            },
            None => self.listing.dir.clone(),
        }
    }

    /// Going up from the top of an archive leads to the folder the archive is in (the archive itself selected).
    pub fn archive_up(&mut self) -> bool {
        let Some(a) = self.archive.clone() else { return false };
        if crate::app::same_dir(&self.listing.dir, &a.dir) {
            if let Some(parent) = a.archive.parent() {
                self.archive = None;
                self.go(parent.to_path_buf(), vec![a.archive], true);
                return true;
            }
        }
        false
    }

    /// The folder on screen changed: still inside the archive's cache, or out of it.
    pub fn archive_check(&mut self) {
        if let Some(a) = &self.archive {
            if !self.listing.dir.starts_with(&a.dir) {
                self.archive = None;
                if let Some(show) = self.archive_show.take() {
                    self.listing.show = show;
                    self.listing.rearrange();
                }
            }
        }
    }

    /// Open `path` as a folder. The listing is read first (fast), the folder is shown at once, and the files are unpacked into
    /// it while it is on screen: they appear as they are written, pictures first being whatever comes first in the archive.
    /// Every kind of file is unpacked, not only pictures, so an archive of documents, sounds or videos opens too. A damaged
    /// archive (cut off, or without its table of contents) gives what could be read, like 7-Zip does.
    pub fn open_archive(&mut self, path: PathBuf, password: Option<String>) {
        let cache = at::cache_dir(&path);
        if cache.join(".done").exists() {
            self.enter_archive(path, cache, false, None);
            return;
        }
        let tool = self.settings.more.seven_zip.clone();
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        self.spawn_job("Opening archive", move |progress| {
            let nothing = |message: String| JobResult { message, refresh: false, select: Vec::new() };
            let ask = |result: Result<(), ArcError>| {
                let _ = tx.send(Msg::Archive(Box::new(ArchiveDone { archive: path.clone(), dest: cache.clone(), password: password.clone(), result, open: true })));
            };
            let (entries, damaged) = match at::list_ex(&path, password.as_deref(), &tool) {
                Ok(v) => v,
                Err(ArcError::Password) => {
                    ask(Err(ArcError::Password));
                    return nothing(String::new());
                }
                Err(e) => return nothing(format!("Can't open {}: {e}", name_of(&path))),
            };
            if password.is_none() && entries.iter().any(|e| e.encrypted && !e.is_dir) {
                ask(Err(ArcError::Password));
                return nothing(String::new());
            }
            // Every file, within sensible limits (one file over 1 GB, or more than 4 GB in all, is left for "Unpack here").
            let (mut bytes, mut wanted, mut skipped) = (0u64, Vec::new(), 0usize);
            for e in entries.iter().filter(|e| !e.is_dir) {
                if e.size > 1 << 30 || bytes + e.size > 4 << 30 {
                    skipped += 1;
                    continue;
                }
                bytes += e.size;
                wanted.push(e.name.clone());
            }
            if wanted.is_empty() {
                return nothing(if entries.is_empty() { format!("{} is empty", name_of(&path)) } else { format!("{} has only files that are too big to open here — use Unpack here", name_of(&path)) });
            }
            let viewable = entries.iter().filter(|e| !e.is_dir && showable(&e.name)).count();
            let _ = std::fs::remove_dir_all(&cache); // a half-finished earlier try
            if std::fs::create_dir_all(&cache).is_err() {
                return nothing("Can't make room to unpack in the cache folder".into());
            }
            // From here the folder is on screen and fills up.
            let _ = tx.send(Msg::ArchiveShow { archive: path.clone(), dir: cache.clone(), damaged, files: wanted.len(), viewable });
            wake();
            let mut last = std::time::Instant::now();
            let r = at::extract(&path, &cache, &wanted, password.as_deref(), &tool, &mut |done, total| {
                if last.elapsed() > std::time::Duration::from_millis(350) {
                    last = std::time::Instant::now();
                    let _ = tx.send(Msg::ArchiveTick);
                    wake();
                }
                progress(format!("{done} / {total} files"))
            });
            if r.is_ok() && !damaged && skipped == 0 {
                let _ = std::fs::write(cache.join(".done"), b"");
            }
            let message = match &r {
                Ok(()) if damaged => format!("{} is damaged: {} file{} could be read", name_of(&path), wanted.len(), if wanted.len() == 1 { "" } else { "s" }),
                Ok(()) if skipped > 0 => format!("{skipped} file{} too big to open here — use Unpack here", if skipped == 1 { " is" } else { "s are" }),
                Ok(()) => String::new(),
                Err(ArcError::Password) => String::new(),
                Err(ArcError::Other(e)) if e == "stopped" => "Unpacking stopped — what was unpacked so far is shown".into(),
                Err(e) => format!("Couldn't unpack all of {}: {e}", name_of(&path)),
            };
            let _ = tx.send(Msg::Archive(Box::new(ArchiveDone { archive: path.clone(), dest: cache.clone(), password: password.clone(), result: r, open: true })));
            JobResult { message, refresh: false, select: Vec::new() }
        });
    }

    /// Show the unpacked archive (or what has been unpacked so far) as the current folder.
    pub fn enter_archive(&mut self, archive: PathBuf, dir: PathBuf, damaged: bool, counts: Option<(usize, usize)>) {
        let _ = std::fs::write(dir.join(".touch"), b""); // keeps the cache folder "recently used"
        let _ = std::fs::remove_file(dir.join(".touch"));
        self.archive = Some(ArchiveView { archive: archive.clone(), dir: dir.clone() });
        // An archive of things the current filter hides (documents, sounds ... under "pictures only") would look empty.
        if let Some((_, viewable)) = counts {
            if viewable == 0 && self.listing.show != crate::model::Show::All {
                self.archive_show = Some(self.listing.show);
                self.listing.show = crate::model::Show::All;
            }
        }
        self.go(dir, Vec::new(), true);
        let what = if damaged { " (damaged: showing what could be read)" } else { "" };
        self.toast(format!("{} opened as a folder{what}", name_of(&archive)));
    }

    /// Unpack `wanted` (everything when empty) into `dest` on a worker thread (Unpack here).
    fn unpack(&mut self, archive: PathBuf, dest: PathBuf, wanted: Vec<String>, password: Option<String>) {
        let tool = self.settings.more.seven_zip.clone();
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        self.spawn_job("Unpacking", move |progress| {
            let r = at::extract(&archive, &dest, &wanted, password.as_deref(), &tool, &mut |done, total| progress(format!("{done} / {total} files")));
            let message = match &r {
                Ok(()) => format!("Unpacked to {}", name_of(&dest)),
                Err(ArcError::Password) => String::new(),
                Err(ArcError::Other(e)) if e == "stopped" => "Unpacking cancelled".into(),
                Err(e) => format!("Couldn't unpack {}: {e}", name_of(&archive)),
            };
            let _ = tx.send(Msg::Archive(Box::new(ArchiveDone { archive, dest: dest.clone(), password, result: r, open: false })));
            wake();
            JobResult { message, refresh: true, select: vec![dest] }
        });
    }

    /// The worker finished: read the folder once more, ask again for a wrong password.
    pub fn archive_done(&mut self, d: ArchiveDone) {
        match d.result {
            Ok(()) if d.open => {
                if self.archive.as_ref().is_some_and(|a| a.archive == d.archive) {
                    self.refresh(Vec::new());
                }
            }
            Err(ArcError::Password) => {
                if let Some(pw) = ask_password(&d.archive, d.password.is_some()) {
                    if d.open {
                        self.open_archive(d.archive, Some(pw));
                    } else {
                        self.extract_archive(d.archive, Some(pw), Some(d.dest));
                    }
                }
            }
            Err(_) if d.open => {
                if self.archive.as_ref().is_some_and(|a| a.archive == d.archive) {
                    self.refresh(Vec::new());
                }
            }
            _ => {}
        }
    }

    /// Unpack every selected archive next to itself, each into a folder named after it (`dest` overrides, for a retry).
    pub fn extract_selected(&mut self) {
        let archives: Vec<PathBuf> = self.picked_paths().into_iter().filter(|p| at::is_archive(p)).collect();
        if archives.is_empty() {
            return self.toast("Select an archive first");
        }
        if let Some(a) = archives.first() {
            self.extract_archive(a.clone(), None, None);
        }
    }

    pub fn extract_archive(&mut self, archive: PathBuf, password: Option<String>, dest: Option<PathBuf>) {
        let tool = self.settings.more.seven_zip.clone();
        match at::list(&archive, password.as_deref(), &tool) {
            Err(ArcError::Password) => {
                if let Some(pw) = ask_password(&archive, password.is_some()) {
                    self.extract_archive(archive, Some(pw), dest);
                }
                return;
            }
            Err(e) => return self.toast(format!("Can't read {}: {e}", name_of(&archive))),
            Ok(entries) => {
                if password.is_none() && entries.iter().any(|e| e.encrypted && !e.is_dir) {
                    if let Some(pw) = ask_password(&archive, false) {
                        self.extract_archive(archive, Some(pw), dest);
                    }
                    return;
                }
            }
        }
        let dir = archive.parent().map(Path::to_path_buf).unwrap_or_default();
        let stem = archive.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Unpacked".into());
        let dest = dest.unwrap_or_else(|| fileops::unique_path(&dir, std::ffi::OsStr::new(&stem)));
        self.unpack(archive, dest, Vec::new(), password);
    }

    // ---- compress ------------------------------------------------------------------------------------

    /// Compress the selection into one archive next to it: the format from Settings, made by 7-Zip or Windows' tar when the
    /// format needs one (a zip is made by NuGallery itself, so it works everywhere).
    pub fn compress_selected(&mut self) {
        let paths = self.picked_paths();
        if paths.is_empty() {
            return;
        }
        let dir = self.listing.dir.clone();
        let stem = if paths.len() == 1 { paths[0].file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into()) } else { dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Archive".into()) };
        let m = self.settings.more.clone();
        let seven = at::seven_zip(&m.seven_zip);
        let tar = at::tar_exe();
        let mut format = m.archive_format.clone();
        let mut note = String::new();
        if (format == "7z" && seven.is_none()) || (format == "tar.gz" && seven.is_none() && tar.is_none()) {
            note = format!(" ({format} needs 7-Zip: made a zip instead)");
            format = "zip".into();
        }
        let ext = format.clone();
        let dest = fileops::unique_path(&dir, std::ffi::OsStr::new(&format!("{stem}.{ext}")));
        let offset = nv_win::utc_offset_minutes();
        let level = m.archive_level;
        self.spawn_job("Compressing", move |progress| {
            let done_msg = |dest: &Path| {
                let size = std::fs::metadata(dest).map(|m| crate::draw::fmt_bytes(m.len())).unwrap_or_default();
                format!("Created {} ({size}){note}", name_of(dest))
            };
            let result = if format == "zip" {
                let entries = nv_core::archive::collect(&paths);
                if entries.is_empty() {
                    return JobResult { message: "Nothing to compress".into(), refresh: false, select: Vec::new() };
                }
                let total = entries.len();
                nv_core::archive::write_zip(&dest, &entries, offset, &mut |done, _| progress(format!("{done} / {total} files")))
            } else {
                progress("compressing…".into());
                external_compress(&format, seven.as_deref(), tar.as_deref(), &dir, &paths, &dest, level, &progress)
            };
            match result {
                Ok(()) => JobResult { message: done_msg(&dest), refresh: true, select: vec![dest.clone()] },
                Err(e) if e == "cancelled" => {
                    let _ = std::fs::remove_file(&dest);
                    JobResult { message: "Compress cancelled".into(), refresh: true, select: Vec::new() }
                }
                Err(e) => {
                    let _ = std::fs::remove_file(&dest);
                    JobResult { message: format!("Couldn't compress: {e}"), refresh: true, select: Vec::new() }
                }
            }
        });
    }

    // ---- PDF -----------------------------------------------------------------------------------------

    /// A PDF of the selected pictures, or of the pictures in the selected folders; with nothing selected, of this folder.
    pub fn make_pdf(&mut self) {
        // Only what is selected on purpose counts: with nothing selected (just a focused tile) it is the whole folder.
        let picked = if self.sel.count(&self.listing) > 0 { self.picked_paths() } else { Vec::new() };
        let mut sources: Vec<PathBuf> = Vec::new();
        let mut label = self.listing.dir.clone();
        if picked.is_empty() {
            sources.extend(pictures_in(&self.listing.dir));
        } else {
            label = picked[0].clone();
            for p in &picked {
                if p.is_dir() {
                    sources.extend(pictures_in(p));
                } else if is_picture(p) {
                    sources.push(p.clone());
                }
            }
        }
        if sources.is_empty() {
            return self.toast("No pictures to put in a PDF here");
        }
        let dir = self.listing.dir.clone();
        let stem = label.file_stem().or_else(|| label.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Pictures".into());
        let dest = fileops::unique_path(&dir, std::ffi::OsStr::new(&format!("{stem}.pdf")));
        let (page, quality) = (self.settings.more.pdf_page, self.settings.more.pdf_quality);
        self.spawn_job("Making PDF", move |progress| {
            let total = sources.len();
            match nv_core::pdf_make::make(&dest, &sources, page, quality, &mut |done, _| progress(format!("{done} / {total} pictures"))) {
                Ok(pages) => {
                    let size = std::fs::metadata(&dest).map(|m| crate::draw::fmt_bytes(m.len())).unwrap_or_default();
                    JobResult { message: format!("Created {} — {pages} pages, {size}", name_of(&dest)), refresh: true, select: vec![dest.clone()] }
                }
                Err(e) if e == "cancelled" => JobResult { message: "PDF cancelled".into(), refresh: true, select: Vec::new() },
                Err(e) => JobResult { message: format!("Couldn't make the PDF: {e}"), refresh: true, select: Vec::new() },
            }
        });
    }
}

/// A file that goes into a PDF as a picture (not a document, a sound, a model or an archive).
fn is_picture(p: &Path) -> bool {
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    nv_codecs::is_supported_ext(&ext) && nv_codecs::media::media_kind(&ext).is_none() && !matches!(ext.as_str(), "pdf" | "ai" | "eps" | "svg" | "svgz" | "swf" | "fla" | "html" | "htm" | "xhtml" | "mhtml" | "mht" | "blend" | "blend1" | "stl" | "obj" | "ply" | "glb" | "gltf" | "fbx" | "vrm")
}

/// The pictures of a folder in natural name order.
fn pictures_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && is_picture(p)).collect()).unwrap_or_default();
    v.sort_by(|a, b| nv_core::folder::natural_cmp(&name_of(a), &name_of(b)));
    v
}

/// 7-Zip (7z, tar.gz) or Windows' tar (tar.gz) in the folder `dir`, with the selected names relative to it. Waits for the
/// tool, killing it when the job is cancelled.
#[allow(clippy::too_many_arguments)]
fn external_compress(format: &str, seven: Option<&Path>, tar: Option<&Path>, dir: &Path, paths: &[PathBuf], dest: &Path, level: u8, progress: &dyn Fn(String) -> bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let names: Vec<String> = paths.iter().filter_map(|p| p.strip_prefix(dir).ok().map(|r| r.to_string_lossy().into_owned()).or_else(|| p.file_name().map(|n| n.to_string_lossy().into_owned()))).collect();
    let mut cmd = match (format, seven, tar) {
        ("7z", Some(z), _) => {
            let mut c = Command::new(z);
            c.args(["a", "-t7z", &format!("-mx={}", level.clamp(1, 9)), "-y", "-bd"]).arg(dest).args(&names);
            c
        }
        ("tar.gz", _, Some(t)) => {
            let mut c = Command::new(t);
            c.arg("-czf").arg(dest).args(&names);
            c
        }
        ("tar.gz", Some(z), None) => {
            let mut c = Command::new(z);
            c.args(["a", "-tzip", &format!("-mx={}", level.clamp(1, 9)), "-y", "-bd"]).arg(dest.with_extension("zip")).args(&names);
            c
        }
        _ => return Err("no tool for this format".into()),
    };
    cmd.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => {
                let mut err = String::new();
                if let Some(mut s) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut s, &mut err);
                }
                return Err(err.trim().lines().next().unwrap_or("the tool reported an error").to_string());
            }
            None => {
                if !progress("compressing…".into()) {
                    let _ = child.kill();
                    return Err("cancelled".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pictures_go_in_a_pdf() {
        assert!(is_picture(Path::new("a.JPG")));
        assert!(is_picture(Path::new("a.webp")));
        assert!(!is_picture(Path::new("a.pdf")));
        assert!(!is_picture(Path::new("a.mp4")));
        assert!(!is_picture(Path::new("a.glb")));
        assert!(!is_picture(Path::new("a.zip")));
    }

    #[test]
    fn archive_entries_worth_showing() {
        assert!(showable("pages/001.jpg"));
        assert!(!showable("readme.txt"));
        assert!(!showable("inner.zip"));
    }
}
