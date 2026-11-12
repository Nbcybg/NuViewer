//! User actions on files and on the window: rename, delete, move/copy, undo, clipboard, slideshow, fullscreen.

use super::*;
use nv_core::fileops;
use nv_win::shell;

const SLIDE_STEPS_S: [f32; 8] = [1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 15.0, 30.0];
const UNDO_LIMIT: usize = 50;

/// In-place file-name editor (drawn by `hud`).
pub struct Rename {
    pub path: PathBuf,
    pub chars: Vec<char>,
    pub cursor: usize,
    /// Selected range in chars (initially the file-name stem, like Explorer).
    pub sel: Option<(usize, usize)>,
    pub error: Option<String>,
}

pub struct Slideshow {
    pub on: bool,
    pub secs: f32,
    pub shuffle: bool,
    /// Image to show next when shuffling (known early so it can be prefetched).
    pub next: Option<usize>,
    /// When the current picture appeared (the playback bar shows how far through the interval we are).
    pub since: Instant,
}

impl Slideshow {
    pub fn new() -> Self {
        Slideshow { on: false, secs: 3.0, shuffle: false, next: None, since: Instant::now() }
    }
}


impl App {
    /// Index of the file actions apply to: the open image, or the selected tile in the gallery.
    pub fn target(&self) -> Option<usize> {
        let i = if self.mode == Mode::Gallery { self.gallery.selected } else { self.folder.index };
        (i < self.folder.files.len()).then_some(i)
    }

    // ---- opening something from another launch ----------------------------------------------------

    /// A second launch of NuViewer asked this window to open `path` (or just to come forward).
    pub fn open_request(&mut self, path: Option<String>, gallery: bool) {
        nv_win::instance::bring_to_front(self.hwnd);
        let Some(path) = path.map(PathBuf::from) else { return };
        if !path.exists() {
            self.toast(format!("Not found: {}", path.display()));
            return;
        }
        self.rename = None;

        // Same folder and the file is already listed: just go to it, keeping caches and view state.
        let dir = if path.is_dir() { None } else { path.parent() };
        if let (Some(dir), Some(i)) = (dir, self.folder.position_ignore_case(&path)) {
            if self.folder.is_dir(dir) {
                self.stop_slideshow(false);
                match (self.mode, gallery) {
                    (Mode::Viewer, false) => self.navigate(i as isize),
                    (Mode::Viewer, true) => {
                        self.navigate(i as isize);
                        self.enter_gallery();
                    }
                    (Mode::Gallery, false) => {
                        self.gallery.selected = i;
                        self.leave_gallery(true);
                    }
                    (Mode::Gallery, true) => self.gallery_select(self.tile_of(i)),
                }
                return;
            }
        }

        match FolderList::open(&path) {
            Err(e) => self.toast(format!("Couldn't open: {e}")),
            Ok(list) => {
                self.replace_folder(list);
                if gallery {
                    self.enter_gallery();
                }
            }
        }
    }

    /// Switch to a different folder: everything tied to the old list is dropped.
    fn replace_folder(&mut self, list: FolderList) {
        self.stop_slideshow(false);
        self.undo.clear();
        self.rename = None;
        self.hud.info_for = None;
        self.folder = list;
        self.dir = 1;
        self.meta.clear();
        self.failed.clear();
        self.staged.clear();
        self.uploading = None;
        let _ = self.renderer.cancel_upload();
        self.sizes.clear();
        self.gallery.failed.clear();
        self.gallery.keep.clear();
        self.gallery.selected = self.folder.index;
        self.gallery.scroll = 0.0;
        self.renderer.clear_cache();
        self.loader.request(Vec::new());
        self.thumb_loader.request(Vec::new());
        self.mode = Mode::Viewer;
        self.nav_start = None;
        self.after_list_change(); // shows the new current image (or "no images")
    }

    // ---- fullscreen ------------------------------------------------------------------------------

    pub fn toggle_fullscreen_app(&mut self) {
        let fullscreen = nv_win::toggle_fullscreen(self.hwnd);
        self.fullscreen_timers(fullscreen);
        // The WM_SIZE caused by the style change arrives while we're busy handling this key, so resize by hand.
        let (w, h) = nv_win::client_size(self.hwnd);
        let _ = self.renderer.resize(w, h);
        if self.mode == Mode::Gallery {
            let lay = self.layout();
            self.gallery.scroll = lay.clamp_scroll(self.gallery.scroll);
            self.request_thumbs();
        }
        invalidate(self.hwnd);
    }

    // ---- removing / re-adding files from the list ------------------------------------------------

    /// Forget file `i` everywhere (list, GPU caches, metadata) after it was deleted or moved away.
    pub fn drop_file_at(&mut self, i: usize) {
        let id = self.id_at(i);
        self.folder.remove_at(i);
        let n = self.folder.files.len();
        if i < self.gallery.selected {
            self.gallery.selected -= 1;
        }
        self.gallery.selected = self.gallery.selected.min(n.saturating_sub(1));
        self.renderer.forget(id);
        self.staged.retain(|s| s.id != id);
        if self.renderer.upload_progress().map(|(p, _)| p) == Some(id) {
            let _ = self.renderer.cancel_upload();
            self.uploading = None;
        }
        self.meta.remove(&id);
        self.failed.remove(&id);
        self.gallery.failed.remove(&id);
        self.after_list_change();
    }

    /// Re-sync the view after files were added to / removed from the list.
    pub fn after_list_change(&mut self) {
        self.hud.info_for = None;
        self.rebuild_items();
        if self.folder.files.is_empty() {
            self.loader.request(Vec::new());
            set_title(self.hwnd, "NuViewer — no images");
            invalidate(self.hwnd);
        } else if self.mode == Mode::Viewer {
            self.view = View::new();
            self.refresh();
        } else {
            self.request_thumbs();
            set_title(self.hwnd, &self.title());
            invalidate(self.hwnd);
        }
    }

    /// Put `path` (back) into the list and select it.
    fn reveal_in_list(&mut self, path: PathBuf) {
        if path.parent() != Some(self.folder.dir.as_path()) || !nv_codecs::is_supported_ext(path.extension().and_then(|e| e.to_str()).unwrap_or("")) {
            return;
        }
        let idx = self.folder.insert_sorted(path);
        self.gallery.selected = idx;
        self.folder.index = idx;
        self.after_list_change();
    }

    // ---- rename ----------------------------------------------------------------------------------

    pub fn start_rename(&mut self) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        let chars: Vec<char> = file_name_of(&path).chars().collect();
        let stem_len = chars.iter().rposition(|&c| c == '.').filter(|&p| p > 0).unwrap_or(chars.len());
        self.rename = Some(Rename { path, cursor: stem_len, sel: Some((0, stem_len)), chars, error: None });
        invalidate(self.hwnd);
    }

    pub fn rename_key(&mut self, k: u32) {
        let Some(r) = self.rename.as_mut() else { return };
        r.error = None;
        let len = r.chars.len();
        // Deleting with a selection removes the selection first.
        let delete_sel = |r: &mut Rename| -> bool {
            if let Some((a, b)) = r.sel.take() {
                r.chars.drain(a..b);
                r.cursor = a;
                true
            } else {
                false
            }
        };
        match k {
            VK_ESCAPE => self.rename = None,
            VK_RETURN => return self.commit_rename(),
            VK_BACK => {
                if !delete_sel(r) && r.cursor > 0 {
                    r.cursor -= 1;
                    r.chars.remove(r.cursor);
                }
            }
            VK_DELETE => {
                if !delete_sel(r) && r.cursor < len {
                    r.chars.remove(r.cursor);
                }
            }
            VK_LEFT => {
                r.cursor = r.sel.take().map(|(a, _)| a).unwrap_or(r.cursor.saturating_sub(1));
            }
            VK_RIGHT => {
                r.cursor = r.sel.take().map(|(_, b)| b).unwrap_or((r.cursor + 1).min(len));
            }
            VK_HOME => {
                r.sel = None;
                r.cursor = 0;
            }
            VK_END => {
                r.sel = None;
                r.cursor = len;
            }
            _ => {}
        }
        invalidate(self.hwnd);
    }

    pub fn rename_char(&mut self, c: char) {
        let Some(r) = self.rename.as_mut() else { return };
        if "<>:\"/\\|?*".contains(c) {
            r.error = Some("A file name can't contain \\ / : * ? \" < > |".into());
        } else {
            r.error = None;
            if let Some((a, b)) = r.sel.take() {
                r.chars.drain(a..b);
                r.cursor = a;
            }
            r.chars.insert(r.cursor, c);
            r.cursor += 1;
        }
        invalidate(self.hwnd);
    }

    fn commit_rename(&mut self) {
        let Some(r) = self.rename.as_mut() else { return };
        let new_name: String = r.chars.iter().collect();
        let old = r.path.clone();
        match fileops::rename_file(&old, &new_name) {
            Err(e) => {
                r.error = Some(e);
                invalidate(self.hwnd);
            }
            Ok(new) => {
                self.rename = None;
                if new == old {
                    invalidate(self.hwnd);
                    return;
                }
                self.push_undo(UndoOp::Moved { from: old.clone(), to: new.clone() });
                self.rekey_file(&old, &new);
                self.toast(format!("Renamed to {}", file_name_of(&new)));
            }
        }
    }

    /// `old` was renamed to `new` on disk: update list, caches and selection to follow it.
    fn rekey_file(&mut self, old: &Path, new: &Path) {
        let (old_id, new_id) = (path_id(old), path_id(new));
        if let Some(i) = self.folder.position(old) {
            let at = self.folder.replace_at(i, new.to_path_buf());
            if self.mode == Mode::Gallery || self.gallery.selected == i {
                self.gallery.selected = at;
            }
        }
        self.renderer.rekey(old_id, new_id);
        if let Some(m) = self.meta.remove(&old_id) {
            self.meta.insert(new_id, m);
        }
        if let Some(e) = self.failed.remove(&old_id) {
            self.failed.insert(new_id, e);
        }
        self.gallery.failed.remove(&old_id);
        self.after_list_change();
    }

    // ---- delete ----------------------------------------------------------------------------------

    pub fn delete_target(&mut self, permanent: bool) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        let name = file_name_of(&path);
        if permanent {
            if !shell::confirm(self.hwnd, &format!("Permanently delete \"{name}\"?\n\nThis can't be undone.")) {
                invalidate(self.hwnd);
                return;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    self.drop_file_at(i);
                    self.toast(format!("Deleted {name} permanently"));
                }
                Err(e) => self.toast(format!("Couldn't delete: {e}")),
            }
        } else {
            match shell::recycle(self.hwnd, &path) {
                Ok(()) => {
                    self.push_undo(UndoOp::Deleted { original: path });
                    self.drop_file_at(i);
                    self.toast(format!("Moved {name} to the Recycle Bin — Ctrl+Z to undo"));
                }
                Err(e) => self.toast(format!("Couldn't delete: {e}")),
            }
        }
        invalidate(self.hwnd);
    }

    // ---- move / copy -----------------------------------------------------------------------------

    fn pick_destination(&self, title: &str) -> Option<PathBuf> {
        // Automation hook: lets tests choose a folder without driving the native dialog.
        if let Some(p) = std::env::var_os("NUVIEWER_PICK_FOLDER") {
            return Some(PathBuf::from(p));
        }
        shell::pick_folder(self.hwnd, title, self.last_dest.as_deref())
    }

    pub fn transfer_target(&mut self, mv: bool) {
        let Some(i) = self.target() else { return };
        let src = self.folder.files[i].clone();
        let Some(dest) = self.pick_destination(if mv { "Move to folder" } else { "Copy to folder" }) else {
            invalidate(self.hwnd);
            return;
        };
        let result = if mv { fileops::move_file(&src, &dest) } else { fileops::copy_file(&src, &dest) };
        match result {
            Err(e) => self.toast(format!("Couldn't {}: {e}", if mv { "move" } else { "copy" })),
            Ok(new) => {
                self.last_dest = Some(dest.clone());
                let dest_name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dest.display().to_string());
                if mv {
                    self.push_undo(UndoOp::Moved { from: src, to: new });
                    self.drop_file_at(i);
                    self.toast(format!("Moved to {dest_name} — Ctrl+Z to undo"));
                } else {
                    self.push_undo(UndoOp::Copied { copy: new.clone() });
                    if new.parent() == Some(self.folder.dir.as_path()) {
                        self.folder.insert_sorted(new);
                        self.after_list_change();
                    }
                    self.toast(format!("Copied to {dest_name} — Ctrl+Z to undo"));
                }
            }
        }
        invalidate(self.hwnd);
    }

    // ---- undo ------------------------------------------------------------------------------------

    fn push_undo(&mut self, op: UndoOp) {
        self.undo.push(op);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self) {
        let Some(op) = self.undo.pop() else {
            self.toast("Nothing to undo");
            return;
        };
        match op {
            UndoOp::Moved { from, to } => {
                let back = if from.parent() == to.parent() {
                    std::fs::rename(&to, &from).map(|_| from.clone()).map_err(|e| e.to_string())
                } else {
                    // Put it back by its original name if free, else with a " (2)" suffix.
                    fileops::move_file(&to, from.parent().unwrap_or(Path::new("."))).map(|p| p)
                };
                match back {
                    Err(e) => self.toast(format!("Couldn't undo: {e}")),
                    Ok(restored) => {
                        if self.folder.position(&to).is_some() {
                            if to.parent() == restored.parent() {
                                self.rekey_file(&to, &restored);
                            } else if let Some(i) = self.folder.position(&to) {
                                self.drop_file_at(i);
                            }
                        } else {
                            self.reveal_in_list(restored.clone());
                        }
                        self.toast(format!("Undone — {}", file_name_of(&restored)));
                    }
                }
            }
            UndoOp::Copied { copy } => match shell::recycle(self.hwnd, &copy) {
                Err(e) => self.toast(format!("Couldn't undo: {e}")),
                Ok(()) => {
                    if let Some(i) = self.folder.position(&copy) {
                        self.drop_file_at(i);
                    }
                    self.toast(format!("Removed the copy {}", file_name_of(&copy)));
                }
            },
            UndoOp::Deleted { original } => match shell::recycle_restore(&original) {
                Err(e) => {
                    // Keep it on the stack: the user can still restore it from the Recycle Bin by hand.
                    self.toast(format!("Couldn't restore automatically ({e}) — open the Recycle Bin"));
                }
                Ok(()) => {
                    self.reveal_in_list(original.clone());
                    self.toast(format!("Restored {}", file_name_of(&original)));
                }
            },
        }
        invalidate(self.hwnd);
    }

    // ---- clipboard -------------------------------------------------------------------------------

    pub fn copy_to_clipboard(&mut self, path_only: bool) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        if path_only {
            match shell::clipboard_copy_text(self.hwnd, &path.display().to_string()) {
                Ok(()) => self.toast("Copied the file path"),
                Err(e) => self.toast(format!("Couldn't copy: {e}")),
            }
            return;
        }
        match nv_codecs::decode_file_first_frame(&path).map(nv_codecs::Image::finish_for_cpu) {
            Err(e) => self.toast(format!("Couldn't copy: {e}")),
            Ok(img) => {
                let mut bgra = img.rgba;
                bgra.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
                match shell::clipboard_copy_image_and_file(self.hwnd, &path, img.width, img.height, &bgra) {
                    Ok(()) => self.toast("Copied image and file"),
                    Err(e) => self.toast(format!("Couldn't copy: {e}")),
                }
            }
        }
    }

    // ---- slideshow -------------------------------------------------------------------------------

    pub fn toggle_slideshow(&mut self) {
        if self.slide.on {
            self.stop_slideshow(true);
        } else {
            self.start_slideshow();
        }
    }

    fn start_slideshow(&mut self) {
        if self.folder.files.is_empty() {
            return;
        }
        if self.mode == Mode::Gallery {
            self.leave_gallery(true);
        }
        self.slide.on = true;
        self.slide.since = Instant::now();
        set_timer(self.hwnd, TIMER_UI, 250); // keeps the playback bar's progress moving
        self.pick_next();
        set_timer(self.hwnd, TIMER_SLIDE, (self.slide.secs * 1000.0) as u32);
        self.refresh(); // include the upcoming image in the prefetch set
        self.toast(format!("Slideshow {:.0} s — S/Esc stop · +/- speed · R shuffle", self.slide.secs));
    }

    pub fn stop_slideshow(&mut self, announce: bool) {
        if self.slide.on {
            self.slide.on = false;
            self.slide.next = None;
            kill_timer(self.hwnd, TIMER_SLIDE);
            kill_timer(self.hwnd, TIMER_UI);
            if announce {
                self.toast("Slideshow stopped");
            }
            set_title(self.hwnd, &self.title());
        }
    }

    pub fn adjust_speed(&mut self, step: i32) {
        let cur = SLIDE_STEPS_S.iter().position(|&s| (s - self.slide.secs).abs() < 0.01).unwrap_or(2) as i32;
        let next = (cur + step).clamp(0, SLIDE_STEPS_S.len() as i32 - 1) as usize;
        self.slide.secs = SLIDE_STEPS_S[next];
        if self.slide.on {
            set_timer(self.hwnd, TIMER_SLIDE, (self.slide.secs * 1000.0) as u32);
        }
        self.toast(format!("Slideshow interval {:.0} s", self.slide.secs));
    }

    fn next_random(&mut self) -> usize {
        // xorshift64*; quality is irrelevant here, we only need "not the same image twice".
        let n = self.folder.files.len();
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let r = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize;
        if n <= 1 { 0 } else { (self.folder.index + 1 + r % (n - 1)) % n }
    }

    /// Decide the next image now (so it can be prefetched).
    pub fn pick_next(&mut self) {
        let n = self.folder.files.len();
        self.slide.next = (n > 1).then(|| if self.slide.shuffle { self.next_random() } else { (self.folder.index + 1) % n });
    }

    pub fn slideshow_tick(&mut self) {
        if !self.slide.on || self.mode != Mode::Viewer || self.folder.files.len() < 2 {
            return;
        }
        let n = self.folder.files.len();
        let to = self.slide.next.filter(|&i| i < n).unwrap_or((self.folder.index + 1) % n);
        self.slide.since = Instant::now();
        self.navigate_with_dir(to, 1);
        self.pick_next();
        self.refresh();
    }
}
