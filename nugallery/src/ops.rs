//! What the commands do: open in the viewer, rename, delete, copy / cut / paste, move / copy to, print, compress,
//! convert, quick edit. Short things run on the UI thread (the shell shows its own progress for copies); anything that
//! reads or writes many files runs as a *job* on a worker thread with a progress line and Esc to cancel.

use crate::app::{same_dir, Active, App, Focus, Job, JobResult, Msg};
use crate::input::TextInput;
use crate::menus::{Preset, QuickOp};
use nv_core::fileops;
use nv_editor::export::{self, Export, Mark, SizeMode};
use nv_editor::preset;
use nv_editor::recipe::Recipe;
use nv_editor::resample::Algo;
use nv_editor::sidecar::{self, Source};
use nv_editor::watermark::Place;
use nv_editor::{convert::Out, engine, raster, save, Pixels};
use nv_win::files::{self, Op};
use nv_win::shell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The viewer's executable: next to this one.
pub fn viewer_exe() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NUVIEWER_EXE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let sibling = exe.with_file_name("nuviewer.exe");
    sibling.is_file().then_some(sibling)
}

/// Where originals are kept before a picture is rewritten (the same place the viewer uses, so its clean-up covers it).
pub fn backup_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("undo")
}

/// How a conversion is done.
#[derive(Clone)]
pub struct ConvertSpec {
    pub out: Out,
    pub max_edge: Option<u32>,
    /// Folder for the results (None: next to each original).
    pub dest: Option<PathBuf>,
    /// Added to the name before the extension ("_small").
    pub suffix: String,
    /// Send the originals to the Recycle Bin once their copy exists.
    pub delete_originals: bool,
    /// Apply each picture's pair file (the edits made in NuEditor) to its copy.
    pub apply_edits: bool,
    /// A NuEditor preset (.nupreset) whose look every copy gets.
    pub preset: Option<PathBuf>,
    /// How the picture is resampled when it changes size.
    pub algo: Algo,
    pub colours: Option<u32>,
    pub target_kb: Option<u32>,
    pub remove_bg: Option<f32>,
    /// Text stamped in the bottom right corner.
    pub watermark: Option<String>,
}

impl ConvertSpec {
    /// A plain conversion: format, longest side, folder, name suffix; nothing else.
    pub fn simple(out: Out, max_edge: Option<u32>, dest: Option<PathBuf>, suffix: String, delete_originals: bool) -> ConvertSpec {
        ConvertSpec { out, max_edge, dest, suffix, delete_originals, apply_edits: false, preset: None, algo: Algo::Lanczos, colours: None, target_kb: None, remove_bg: None, watermark: None }
    }
}

/// Convert one picture; returns the new file.
pub fn convert_one(src: &Path, spec: &ConvertSpec) -> Result<PathBuf, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let img = nv_codecs::decode_file_first_frame(src)?;
    if img.mesh.is_some() {
        return Err("3D models can't be converted to a picture".into());
    }
    let img = img.finish_for_cpu();
    let mut pixels = Pixels::new(img.width, img.height, img.rgba);
    // The edits made in NuEditor: the picture's pair file, then the chosen preset's look over it.
    if spec.apply_edits || spec.preset.is_some() {
        let mut recipe = Recipe::default();
        if spec.apply_edits {
            if let Ok(Some((r, info))) = sidecar::load(src) {
                if info.matches(&Source { width: pixels.w, height: pixels.h, ..Default::default() }) {
                    recipe = r;
                }
            }
        }
        if let Some(look) = spec.preset.as_ref().and_then(|p| preset::load(p).ok()) {
            recipe = preset::apply(&recipe, &look);
        }
        if !recipe.is_identity() {
            pixels = engine::render(&pixels, &recipe);
        }
    }
    // A JPEG made from a JPEG keeps its EXIF (camera, date, ...), with the orientation set to upright.
    let exif_source = if matches!(spec.out, Out::Jpeg(_)) && matches!(ext.as_str(), "jpg" | "jpeg" | "jpe" | "jfif") { std::fs::read(src).ok() } else { None };
    let mut e = Export::new(spec.out);
    e.background = [255, 255, 255];
    e.size = match spec.max_edge {
        Some(m) if pixels.w.max(pixels.h) > m => SizeMode::LongEdge(m),
        _ => SizeMode::Original,
    };
    e.algo = spec.algo;
    e.colours = spec.colours;
    e.target_kb = spec.target_kb;
    e.remove_bg = spec.remove_bg;
    if let Some(text) = &spec.watermark {
        if let Some((w, h, rgba)) = nv_win::text::watermark_bitmap(text) {
            e.mark = Some(Mark { image: Pixels::new(w, h, rgba), place: Place::BottomRight, width_frac: 0.25, opacity: 0.7 });
        }
    }
    let bytes = export::run(pixels, &e, exif_source.as_deref())?.bytes;
    let dir = spec.dest.clone().or_else(|| src.parent().map(Path::to_path_buf)).ok_or("no folder")?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let name = format!("{stem}{}.{}", spec.suffix, spec.out.ext());
    let dst = fileops::unique_path(&dir, std::ffi::OsStr::new(&name));
    save::write_atomic(&dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    // The copy keeps the original's date.
    if let Some((_, modified)) = nv_core::batch::read_file_times(src) {
        let _ = nv_core::batch::set_file_times(&dst, modified, nv_core::batch::WhichTimes { created: false, modified: true, accessed: false });
    }
    Ok(dst)
}

/// Turn / flip a JPEG or PNG in place (the original goes to the backup folder first). The file's date is kept.
pub fn quick_edit_one(path: &Path, op: QuickOp) -> Result<(), String> {
    let fmt = save::Format::of_path(path).ok_or("only JPEG and PNG pictures can be rewritten")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = nv_codecs::decode_file(path)?;
    if img.frame_count() > 1 {
        return Err("animated pictures can't be edited here".into());
    }
    if img.reduced_from.is_some() {
        return Err("too large to rewrite safely".into());
    }
    let img = img.finish_for_cpu();
    let src = Pixels::new(img.width, img.height, img.rgba);
    let out = match op {
        QuickOp::RotateLeft => raster::rotate(&src, -90.0),
        QuickOp::RotateRight => raster::rotate(&src, 90.0),
        QuickOp::FlipH => raster::flip_horizontal(&src),
        QuickOp::FlipV => raster::flip_vertical(&src),
    };
    let exif = if fmt == save::Format::Jpeg { save::exif_of_jpeg(&bytes, (out.w, out.h)) } else { None };
    let encoded = save::encode(&out, fmt, exif.as_deref())?;
    let times = nv_core::batch::read_file_times(path);
    let _ = save::backup_copy(path, &backup_dir());
    save::write_atomic(path, &encoded).map_err(|e| e.to_string())?;
    if let Some((_, modified)) = times {
        let _ = nv_core::batch::set_file_times(path, modified, nv_core::batch::WhichTimes { created: false, modified: true, accessed: false });
    }
    Ok(())
}

/// Remove metadata from one file (JPEG, PNG, WebP), keeping its date. Ok(false): there was nothing to remove.
pub fn strip_one(path: &Path, keep_orientation: bool) -> Result<bool, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    match nv_codecs::metastrip::strip(&data, &ext, keep_orientation)? {
        None => Ok(false),
        Some(s) => {
            let times = nv_core::batch::read_file_times(path);
            let _ = save::backup_copy(path, &backup_dir());
            save::write_atomic(path, &s.bytes).map_err(|e| e.to_string())?;
            if let Some((_, modified)) = times {
                let _ = nv_core::batch::set_file_times(path, modified, nv_core::batch::WhichTimes { created: false, modified: true, accessed: false });
            }
            Ok(true)
        }
    }
}

impl App {
    /// The selected entries' paths (or the focused item's).
    /// A folded image sequence stands for all its frames: deleting, copying or moving its tile does that to every frame.
    pub fn picked_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for e in self.sel.entries(&self.listing) {
            match self.listing.sequence_files(e.id) {
                Some(frames) => out.extend(frames),
                None => out.push(e.path.clone()),
            }
        }
        out
    }

    fn picked_files(&self) -> Vec<PathBuf> {
        self.sel.entries(&self.listing).into_iter().filter(|e| !e.is_dir()).map(|e| e.path.clone()).collect()
    }

    // ---- the viewer ------------------------------------------------------------------------------------

    /// Show `path` in NuViewer (it comes up in front; Esc there returns here with that picture selected).
    pub fn launch_viewer(&mut self, path: &Path, slideshow: bool) {
        let Some(exe) = viewer_exe() else {
            // No viewer next to us: let Windows decide.
            if let Err(e) = files::open_default(self.hwnd, path) {
                self.toast(format!("Can't open it: {e}"));
            }
            return;
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.arg(path).arg("--from-gallery");
        if slideshow {
            cmd.arg("--slideshow");
        }
        if let Some(seed) = self.shuffle_seed {
            cmd.arg("--shuffle").arg(seed.to_string());
        }
        match cmd.spawn() {
            Ok(child) => {
                self.viewer_started = true;
                nv_win::instance::allow_foreground(child.id());
            }
            Err(e) => self.toast(format!("Couldn't start NuViewer: {e}")),
        }
    }

    /// F4 / "Edit in NuEditor": open the selected picture in the editor. The gallery stays behind; when the editor closes it
    /// comes back to this window on that picture (`--from gallery`).
    pub fn launch_editor(&mut self) {
        let Some(path) = self.sel.entries(&self.listing).first().filter(|e| !e.is_dir() && e.viewable()).map(|e| e.path.clone()) else { return };
        let Some(exe) = std::env::current_exe().ok().map(|e| e.with_file_name("nueditor.exe")).filter(|p| p.is_file()) else {
            self.toast("NuEditor isn't installed next to NuGallery");
            return;
        };
        nv_win::remember_window(self.hwnd); // the editor opens as a window / fullscreen like this one, in the same place
        match std::process::Command::new(exe).arg(&path).arg("--from").arg("gallery").spawn() {
            Ok(child) => {
                self.editor_started = true;
                nv_win::instance::allow_foreground(child.id());
            }
            Err(e) => self.toast(format!("Couldn't start NuEditor: {e}")),
        }
    }

    /// Group numbered pictures into image sequences (one tile each) or show every picture (Ctrl+G, View menu). The viewer
    /// uses the same setting, so a sequence opened from here plays there.
    pub fn toggle_sequences(&mut self) {
        self.settings.sequences = !self.settings.sequences;
        let _ = self.settings.save();
        self.listing.fold = self.settings.sequences.then_some(self.settings.sequence_min as usize);
        self.rearrange_keeping_focus();
        self.toast(if self.settings.sequences { format!("Image sequences grouped (runs of {} or more numbered pictures)", self.settings.sequence_min) } else { "Every picture shown separately".to_string() });
    }

    /// Tools > Import photos…: NuImporter (a mock-up for now) next to this program.
    pub fn launch_importer(&mut self) {
        let Some(exe) = std::env::current_exe().ok().map(|e| e.with_file_name("nuimporter.exe")).filter(|p| p.is_file()) else {
            self.toast("NuImporter isn't installed next to NuGallery");
            return;
        };
        nv_win::remember_window(self.hwnd);
        match std::process::Command::new(exe).arg("--from").arg("gallery").arg("--into").arg(&self.listing.dir).spawn() {
            Ok(child) => nv_win::instance::allow_foreground(child.id()),
            Err(e) => self.toast(format!("Couldn't start NuImporter: {e}")),
        }
    }

    pub fn launch_viewer_settings(&mut self) {
        match viewer_exe() {
            Some(exe) => {
                let _ = std::process::Command::new(exe).arg("--settings").spawn();
            }
            None => self.toast("NuViewer isn't next to NuGallery, so its settings can't be opened"),
        }
    }

    /// Space / double click / middle click / Open: a folder is entered, a picture is shown in the viewer, anything else
    /// opens with its own program.
    pub fn open_selected(&mut self, slideshow: bool) {
        let Some(e) = self.sel.entries(&self.listing).first().map(|e| (e.path.clone(), e.is_dir(), e.viewable())) else { return };
        let (path, is_dir, viewable) = e;
        if is_dir {
            if !slideshow {
                self.go(path, Vec::new(), true);
            }
        } else if nv_core::archive_tools::is_archive(&path) && !slideshow {
            self.open_archive(path, None);
        } else if viewable {
            self.launch_viewer(&path, slideshow);
        } else if let Err(err) = files::open_default(self.hwnd, &path) {
            self.toast(format!("Can't open it: {err}"));
        }
    }

    pub fn show_location(&mut self) {
        if let Some(p) = self.picked_paths().first() {
            shell::reveal_in_explorer(p);
        }
    }

    pub fn edit_with(&mut self, index: Option<usize>) {
        let command = match index {
            Some(i) => match self.settings.external.get(i) {
                Some(e) => e.command.clone(),
                None => return,
            },
            None => match shell::pick_program(self.hwnd) {
                Some(p) => format!("\"{}\"", p.display()),
                None => return,
            },
        };
        let paths = self.picked_files();
        for p in paths.iter().take(10) {
            if let Err(e) = shell::run_external(&command, p) {
                self.toast(e);
                break;
            }
        }
    }

    // ---- rename / new folder -------------------------------------------------------------------------------

    pub fn start_rename(&mut self) {
        let Some(e) = self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| (e.path.clone(), e.name.clone(), e.is_dir())) else { return };
        let ti = if e.2 { TextInput::selected(&e.1) } else { TextInput::name_selected(&e.1) };
        self.active = Some(Active { focus: Focus::Rename(e.0), ti, error: None });
        nv_win::invalidate(self.hwnd);
    }

    pub fn finish_rename(&mut self, path: PathBuf, text: &str, mut active: Active) {
        let name = text.trim();
        if name == path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() {
            return;
        }
        match fileops::rename_file(&path, name) {
            Ok(new) => {
                if new != path {
                    self.push_undo(crate::undo::Step::Moved(vec![(path.clone(), new.clone())]));
                }
                self.toast(format!("Renamed to {name} — Ctrl+Z to undo"));
                self.refresh(vec![new]);
            }
            Err(e) => {
                active.error = Some(e);
                self.active = Some(active);
            }
        }
    }

    pub fn new_folder(&mut self) {
        let dir = self.listing.dir.clone();
        if dir.as_os_str().is_empty() {
            return;
        }
        self.active = Some(Active { focus: Focus::NewFolder(dir), ti: TextInput::selected("New folder"), error: None });
        nv_win::invalidate(self.hwnd);
    }

    pub fn finish_new_folder(&mut self, dir: PathBuf, text: &str, mut active: Active) {
        let name = text.trim();
        if let Err(e) = fileops::valid_file_name(name) {
            active.error = Some(e);
            self.active = Some(active);
            return;
        }
        let path = dir.join(name);
        if path.exists() {
            active.error = Some("a file or folder with that name already exists".into());
            self.active = Some(active);
            return;
        }
        match std::fs::create_dir(&path) {
            Ok(()) => {
                self.toast(format!("Folder {name} created"));
                self.refresh(vec![path]);
            }
            Err(e) => {
                active.error = Some(e.to_string());
                self.active = Some(active);
            }
        }
    }

    // ---- delete / clipboard / copy to / move to -----------------------------------------------------------

    pub fn delete_selected(&mut self, permanent: bool) {
        let paths = self.picked_paths();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        if permanent && !shell::confirm(self.hwnd, &format!("Delete {} permanently?\n\nThis can't be undone.", if n == 1 { paths[0].file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default() } else { format!("{n} items") })) {
            return;
        }
        // Ask first (Settings > Viewer > "Ask before a file goes to the Recycle Bin"): Enter = yes, Esc = no.
        if !permanent && self.settings.confirm_delete {
            let what = if n == 1 { format!("\"{}\"", paths[0].file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()) } else { format!("these {n} items") };
            if !shell::confirm_yes(self.hwnd, &format!("Move {what} to the Recycle Bin?\n\nCtrl+Z brings {} back.", if n == 1 { "it" } else { "them" })) {
                return;
            }
        }
        let hint = self.sel.cursor;
        match files::run(self.hwnd, if permanent { Op::Delete } else { Op::Recycle }, &paths, None, false) {
            Ok(()) => {
                if !permanent {
                    let gone = crate::undo::gone(&paths);
                    self.push_undo(crate::undo::Step::Deleted(gone));
                }
                self.toast(if permanent { format!("Deleted {n} item{}", if n == 1 { "" } else { "s" }) } else { format!("Moved {n} item{} to the Recycle Bin — Ctrl+Z to undo", if n == 1 { "" } else { "s" }) });
                self.cursor_hint = hint;
                self.refresh(Vec::new());
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't delete: {e}")),
        }
    }

    // ---- undo ------------------------------------------------------------------------------------------

    pub fn push_undo(&mut self, step: crate::undo::Step) {
        if step.count() == 0 {
            return;
        }
        self.undo.push(step);
        if self.undo.len() > crate::undo::LIMIT {
            self.undo.remove(0);
        }
    }

    /// Remember what a move / copy of `paths` into `dest` did, so Ctrl+Z can reverse it.
    pub fn record_transfer(&mut self, paths: &[PathBuf], dest: &Path, moved: bool, existed: &[String]) {
        let step = if moved { crate::undo::Step::Moved(crate::undo::moved_pairs(paths, dest)) } else { crate::undo::Step::Copied(crate::undo::copied_files(paths, dest, existed)) };
        self.push_undo(step);
    }

    /// The menu text for Edit > Undo.
    pub fn undo_label(&self) -> String {
        self.undo.last().map(|s| s.label()).unwrap_or_else(|| "Undo".to_string())
    }

    /// Ctrl+Z: take back the last delete, move, rename or copy.
    pub fn undo_last(&mut self) {
        use crate::undo::Step;
        let Some(step) = self.undo.pop() else {
            self.toast("Nothing to undo");
            return;
        };
        let (mut done, mut failed) = (0usize, Vec::new());
        let mut select = Vec::new();
        match &step {
            Step::Deleted(paths) => {
                for p in paths {
                    match shell::recycle_restore(p) {
                        Ok(()) => {
                            done += 1;
                            select.push(p.clone());
                        }
                        Err(e) => failed.push(e),
                    }
                }
            }
            Step::Moved(pairs) => {
                for (from, to) in pairs {
                    let back = if from.exists() {
                        // Something is at the old place again: put it back beside it under a free name.
                        fileops::move_file(to, from.parent().unwrap_or(Path::new(".")))
                    } else {
                        std::fs::rename(to, from).map(|_| from.clone()).or_else(|_| fileops::move_file(to, from.parent().unwrap_or(Path::new("."))))
                    };
                    match back {
                        Ok(p) => {
                            done += 1;
                            select.push(p);
                        }
                        Err(e) => failed.push(e.to_string()),
                    }
                }
            }
            Step::Copied(copies) => match files::run(self.hwnd, Op::Recycle, copies, None, false) {
                Ok(()) => done = copies.len(),
                Err(e) => failed.push(e),
            },
        }
        if failed.is_empty() {
            self.toast(format!("{} — done", step.label()));
        } else if done == 0 {
            // Keep the step so the user can try again (or restore from the Recycle Bin by hand).
            self.toast(format!("Couldn't undo: {}", failed[0]));
            self.undo.push(step);
        } else {
            self.toast(format!("Undid {done} of {}: {}", step.count(), failed[0]));
        }
        self.refresh(select);
    }

    pub fn copy_files(&mut self, cut: bool) {
        let paths = self.picked_paths();
        if paths.is_empty() {
            return;
        }
        match files::clipboard_set_files(self.hwnd, &paths, cut) {
            Ok(()) => {
                self.cut = if cut { paths.clone() } else { Vec::new() };
                self.toast(format!("{} {} item{} — paste with Ctrl+V here or in Explorer", if cut { "Cut" } else { "Copied" }, paths.len(), if paths.len() == 1 { "" } else { "s" }));
            }
            Err(e) => self.toast(format!("Couldn't use the clipboard: {e}")),
        }
    }

    pub fn copy_path(&mut self) {
        let paths = self.picked_paths();
        if paths.is_empty() {
            return;
        }
        let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\r\n");
        match shell::clipboard_copy_text(self.hwnd, &text) {
            Ok(()) => self.toast(if paths.len() == 1 { "Path copied".to_string() } else { format!("{} paths copied", paths.len()) }),
            Err(e) => self.toast(format!("Couldn't use the clipboard: {e}")),
        }
    }

    pub fn copy_image(&mut self) {
        let Some(path) = self.sel.entries(&self.listing).into_iter().find(|e| e.viewable()).map(|e| e.path.clone()) else { return };
        match nv_codecs::decode_file_first_frame(&path) {
            Ok(img) => {
                let img = img.finish_for_cpu();
                let mut bgra = img.rgba.clone();
                for px in bgra.chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
                match files::clipboard_set_image(self.hwnd, img.width, img.height, &bgra) {
                    Ok(()) => self.toast("Picture copied"),
                    Err(e) => self.toast(format!("Couldn't use the clipboard: {e}")),
                }
            }
            Err(e) => self.toast(format!("Can't read the picture: {}", nv_codecs::errors::plain(&e))),
        }
    }

    pub fn paste(&mut self) {
        let Some((paths, cut)) = files::clipboard_files(self.hwnd) else {
            self.toast("There are no files on the clipboard");
            return;
        };
        let dir = self.listing.dir.clone();
        let same_folder = paths.iter().all(|p| p.parent().map(|d| same_dir(d, &dir)).unwrap_or(false));
        if cut && same_folder {
            return;
        }
        let existed = crate::undo::names_in(&dir);
        match files::run(self.hwnd, if cut { Op::Move } else { Op::Copy }, &paths, Some(&dir), same_folder) {
            Ok(()) => {
                if !same_folder {
                    self.record_transfer(&paths, &dir, cut, &existed);
                }
                let select: Vec<PathBuf> = if same_folder { Vec::new() } else { paths.iter().filter_map(|p| p.file_name().map(|n| dir.join(n))).collect() };
                self.toast(format!("Pasted {} item{}", paths.len(), if paths.len() == 1 { "" } else { "s" }));
                self.refresh(select);
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't paste: {e}")),
        }
    }

    pub fn transfer_selected(&mut self, mv: bool) {
        let paths = self.picked_paths();
        if paths.is_empty() {
            return;
        }
        let title = if mv { "Move to" } else { "Copy to" };
        let Some(dest) = shell::pick_folder(self.hwnd, title, self.last_dest.as_deref().or(Some(&self.listing.dir))) else { return };
        self.last_dest = Some(dest.clone());
        if same_dir(&dest, &self.listing.dir) {
            self.toast("That is the folder you are in");
            return;
        }
        let existed = crate::undo::names_in(&dest);
        match files::run(self.hwnd, if mv { Op::Move } else { Op::Copy }, &paths, Some(&dest), false) {
            Ok(()) => {
                self.record_transfer(&paths, &dest, mv, &existed);
                self.toast(format!("{} {} item{} to {} — Ctrl+Z to undo", if mv { "Moved" } else { "Copied" }, paths.len(), if paths.len() == 1 { "" } else { "s" }, dest.display()));
                self.refresh(Vec::new());
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't {}: {e}", if mv { "move" } else { "copy" })),
        }
    }

    pub fn print_selected(&mut self) {
        let paths: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| e.viewable()).map(|e| e.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        if paths.len() > 3 && !shell::confirm(self.hwnd, &format!("Open the print window for each of the {} files?", paths.len())) {
            return;
        }
        for p in paths.iter().take(20) {
            if let Err(e) = files::print(self.hwnd, p) {
                self.toast(format!("Can't print {}: {e}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                break;
            }
        }
    }

    /// The pointer left an item with the button down: drag the selected files out to Explorer or another program.
    pub fn start_drag(&mut self) {
        let paths = self.picked_paths();
        if paths.is_empty() || paths.iter().any(|p| p.parent().map(|d| !same_dir(d, &self.listing.dir)).unwrap_or(true)) {
            return;
        }
        nv_win::release_capture();
        let n = paths.len();
        match files::drag_files(&paths) {
            Ok(files::DragEnd::Moved) => {
                self.toast(format!("Moved {n} item{}", if n == 1 { "" } else { "s" }));
                self.refresh(Vec::new());
            }
            Ok(files::DragEnd::Copied) => self.toast(format!("Copied {n} item{}", if n == 1 { "" } else { "s" })),
            Ok(_) => {}
            Err(e) => self.toast(format!("Couldn't start the drag: {e}")),
        }
        self.rubber = None;
        self.drag_from = None;
        nv_win::invalidate(self.hwnd);
    }

    /// Files dropped on the window from Explorer or another program: into the folder under the pointer, or into this
    /// folder. Like Explorer: moved within a drive, copied between drives; Ctrl forces a copy, Shift a move.
    pub fn on_drop(&mut self, dropped: Vec<String>, x: i32, y: i32) {
        let target = match self.item_at(x, y).and_then(|i| self.listing.get(i)) {
            Some(e) if e.is_dir() => e.path.clone(),
            _ => self.listing.dir.clone(),
        };
        let paths: Vec<PathBuf> = dropped
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| !p.parent().map(|d| same_dir(d, &target)).unwrap_or(false) && !crate::tree::is_within(&target, p))
            .collect();
        if paths.is_empty() {
            self.toast("Those files are already in that folder");
            return;
        }
        let drive = |p: &Path| p.to_string_lossy().chars().next().map(|c| c.to_ascii_lowercase());
        let m = nv_win::modifiers();
        let op = if m.ctrl {
            Op::Copy
        } else if m.shift || drive(&paths[0]) == drive(&target) {
            Op::Move
        } else {
            Op::Copy
        };
        let n = paths.len();
        let existed = crate::undo::names_in(&target);
        match files::run(self.hwnd, op, &paths, Some(&target), false) {
            Ok(()) => {
                self.record_transfer(&paths, &target, op == Op::Move, &existed);
                self.toast(format!("{} {n} item{} to {}", if op == Op::Move { "Moved" } else { "Copied" }, if n == 1 { "" } else { "s" }, target.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| target.display().to_string())));
                self.refresh(Vec::new());
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't drop the files: {e}")),
        }
    }

    /// RAW files: the camera's own look (its embedded JPEG) <-> the sensor data. The picture in the details panel is made
    /// again; thumbnails only when they are developed from the sensor data too (Settings > Camera RAW).
    pub fn toggle_raw_look(&mut self) {
        self.settings.raw_mode = self.settings.raw_mode.flipped();
        let _ = self.settings.save();
        nv_codecs::set_raw_mode(self.settings.raw_mode);
        self.hist.clear();
        self.pv_drop_textures();
        self.preview_id = None;
        self.info_for = None;
        if self.settings.raw_thumbs {
            self.rebuild_thumbs();
        }
        self.load_info();
        self.toast(self.settings.raw_mode.look_name());
        nv_win::invalidate(self.hwnd);
    }

    /// Make the thumbnails of the files in this folder again.
    pub fn rebuild_thumbs(&mut self) {
        for e in self.listing.all.iter().filter(|e| e.has_thumb()) {
            nv_core::thumbs::forget(&e.path);
        }
        if let Some(r) = self.renderer.as_mut() {
            r.retain_thumbs(|_| false);
        }
        self.staged.clear();
        self.thumb_failed.clear();
        self.preview_id = None;
        self.timing.visible_ready_ms = None;
        self.timing.nav_start = Some(std::time::Instant::now());
        self.request_thumbs();
        self.toast("Thumbnails are being made again");
    }

    // ---- jobs ------------------------------------------------------------------------------------------------

    /// Run `work` on a thread. It gets a `progress` function (text for the progress line; returns false once the user
    /// pressed Esc) and returns what to tell the user and what to select afterwards.
    pub fn spawn_job<F>(&mut self, label: &str, work: F)
    where
        F: FnOnce(&dyn Fn(String) -> bool) -> JobResult + Send + 'static,
    {
        if self.job.is_some() {
            self.toast("Another operation is still running");
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.job = Some(Job { cancel: cancel.clone(), label: label.to_string() });
        self.job_text = "starting…".into();
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let progress = {
                let (tx, wake, cancel) = (tx.clone(), wake.clone(), cancel.clone());
                move |text: String| {
                    let _ = tx.send(Msg::JobProgress(text));
                    wake();
                    !cancel.load(Ordering::Relaxed)
                }
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&progress))).unwrap_or(JobResult { message: "The operation stopped unexpectedly".into(), refresh: true, select: Vec::new() });
            let _ = tx.send(Msg::JobDone(result));
            wake();
        });
        nv_win::invalidate(self.hwnd);
    }

    pub fn quick_convert(&mut self, p: Preset) {
        let suffix = match p {
            Preset::Resize(n) => format!("_{n}px"),
            _ => String::new(),
        };
        let spec = ConvertSpec::simple(p.target(), p.max_edge(), None, suffix, false);
        self.convert_selected(spec);
    }

    pub fn convert_selected(&mut self, spec: ConvertSpec) {
        let paths: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| e.viewable()).map(|e| e.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        let hwnd_val = self.hwnd.0 as isize;
        self.spawn_job("Converting", move |progress| {
            let total = paths.len();
            let (mut ok, mut failed) = (Vec::new(), Vec::new());
            let mut originals = Vec::new();
            for (i, p) in paths.iter().enumerate() {
                if !progress(format!("{} / {total}  {}", i + 1, p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())) {
                    break;
                }
                match convert_one(p, &spec) {
                    Ok(new) => {
                        ok.push(new);
                        originals.push(p.clone());
                    }
                    Err(e) => failed.push(format!("{}: {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), nv_codecs::errors::plain(&e))),
                }
            }
            if spec.delete_originals && !originals.is_empty() {
                let hwnd = windows_hwnd(hwnd_val);
                let _ = files::run(hwnd, Op::Recycle, &originals, None, false);
            }
            let mut message = format!("Converted {} of {total} to {}", ok.len(), spec.out.label());
            if let Some(f) = failed.first() {
                message += &format!(" — {} failed (first: {f})", failed.len());
            }
            JobResult { message, refresh: true, select: ok }
        });
    }

    /// Right-click > Convert to PNG / JPEG: every picked picture, next to itself, no questions.
    pub fn quick_to(&mut self, target: nv_core::quick::Target) {
        let paths: Vec<PathBuf> = self.sel.entries(&self.listing).into_iter().filter(|e| e.viewable() && !e.is_dir()).map(|e| e.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        self.spawn_job(nv_core::i18n::t("menu.converting"), move |progress| {
            let report = nv_core::quick::convert_many(&paths, target, |i, n| progress(format!("{} / {n}", i + 1)));
            JobResult { message: report.summary(target), refresh: true, select: report.made.clone() }
        });
    }

    pub fn quick_edit(&mut self, op: QuickOp) {
        let paths: Vec<PathBuf> = self.picked_files();
        if paths.is_empty() {
            return;
        }
        self.spawn_job(op.label(), move |progress| {
            let total = paths.len();
            let (mut done, mut first_error) = (0, None);
            for (i, p) in paths.iter().enumerate() {
                if !progress(format!("{} / {total}", i + 1)) {
                    break;
                }
                match quick_edit_one(p, op) {
                    Ok(()) => done += 1,
                    Err(e) => {
                        first_error.get_or_insert(format!("{}: {e}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    }
                }
            }
            let mut message = format!("{}: {done} of {total} done (originals are kept in the undo folder)", op.label());
            if let Some(e) = first_error {
                message += &format!(" — {e}");
            }
            JobResult { message, refresh: true, select: paths.clone() }
        });
        // Thumbnails of rewritten files change: they are made again when the folder is re-read.
    }
}

fn windows_hwnd(v: isize) -> nv_win::WindowHandle {
    nv_win::WindowHandle(v as *mut std::ffi::c_void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_gal_ops_{tag}_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A small 3-colour PNG: the left half red, the right half blue.
    fn write_png(path: &Path, w: u32, h: u32) {
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                rgba.extend(if x < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
            }
        }
        let bytes = save::encode(&Pixels::new(w, h, rgba), save::Format::Png, None).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn converting_makes_a_new_file_and_keeps_the_original() {
        let d = temp_dir("conv");
        let src = d.join("pic.png");
        write_png(&src, 40, 20);
        let spec = ConvertSpec::simple(Out::Jpeg(90), Some(20), None, "_small".into(), false);
        let dst = convert_one(&src, &spec).unwrap();
        assert_eq!(dst.file_name().unwrap().to_string_lossy(), "pic_small.jpg");
        assert!(src.exists());
        let img = nv_codecs::decode_file(&dst).unwrap().finish_for_cpu();
        assert_eq!((img.width, img.height), (20, 10), "longest side 20");
        assert!(img.rgba[0] > 200 && img.rgba[2] < 60, "left half still red");
        // Converting again never overwrites.
        let again = convert_one(&src, &spec).unwrap();
        assert_ne!(again, dst);
        // Into another folder, as PNG -> WebP.
        let other = d.join("out");
        fs::create_dir_all(&other).unwrap();
        let w = convert_one(&src, &ConvertSpec::simple(Out::Webp, None, Some(other.clone()), String::new(), false)).unwrap();
        assert_eq!(w, other.join("pic.webp"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn quick_edit_turns_the_picture_and_backs_up_the_original() {
        let d = temp_dir("edit");
        let f = d.join("a.png");
        write_png(&f, 40, 20);
        let before = fs::read(&f).unwrap();
        quick_edit_one(&f, QuickOp::RotateRight).unwrap();
        let img = nv_codecs::decode_file(&f).unwrap().finish_for_cpu();
        assert_eq!((img.width, img.height), (20, 40), "turned a quarter");
        // Rotated clockwise: the left (red) half is now on top.
        assert!(img.rgba[0] > 200 && img.rgba[2] < 60, "top is red");
        let last = (img.width * img.height * 4) as usize - 4;
        assert!(img.rgba[last] < 60 && img.rgba[last + 2] > 200, "bottom is blue");
        quick_edit_one(&f, QuickOp::RotateLeft).unwrap();
        let back = nv_codecs::decode_file(&f).unwrap().finish_for_cpu();
        assert_eq!((back.width, back.height), (40, 20));
        assert_ne!(before.len(), 0);
        assert!(quick_edit_one(&d.join("x.tga"), QuickOp::FlipH).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn metadata_stripping_rewrites_only_when_there_is_something_to_remove() {
        let d = temp_dir("strip");
        let f = d.join("t.png");
        write_png(&f, 8, 8);
        assert_eq!(strip_one(&f, true), Ok(false), "nothing to remove");
        // Add a text chunk after IHDR by hand.
        let mut bytes = fs::read(&f).unwrap();
        let mut chunk = Vec::new();
        let data = b"Comment\0secret";
        chunk.extend((data.len() as u32).to_be_bytes());
        chunk.extend(b"tEXt");
        chunk.extend(data);
        let mut crc = flate2_crc(b"tEXt", data);
        chunk.extend(crc.to_be_bytes());
        crc = 0;
        let _ = crc;
        bytes.splice(33..33, chunk);
        fs::write(&f, &bytes).unwrap();
        let before = nv_core::batch::read_file_times(&f).unwrap().1;
        assert_eq!(strip_one(&f, true), Ok(true));
        assert!(!fs::read(&f).unwrap().windows(4).any(|w| w == b"tEXt"));
        assert_eq!(nv_core::batch::read_file_times(&f).unwrap().1, before, "the date is kept");
        assert!(strip_one(&d.join("n.bmp"), true).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    fn flate2_crc(kind: &[u8], data: &[u8]) -> u32 {
        // CRC-32 of kind + data, bitwise (the tests of nv-codecs use flate2; this crate doesn't depend on it).
        let mut crc = 0xFFFF_FFFFu32;
        for &b in kind.iter().chain(data) {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
}
