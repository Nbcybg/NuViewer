//! User actions on files and on the window: rename, delete, move/copy, undo, clipboard, slideshow, fullscreen.

use super::*;
use nv_core::fileops;
use nv_win::shell;

const SLIDE_STEPS_S: [f32; 8] = [1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 15.0, 30.0];
const UNDO_LIMIT: usize = 50;

/// In-place file-name editor (drawn by `hud`).
pub struct Rename {
    pub path: PathBuf,
    /// The text, caret and selection (initially the file-name stem is selected, like Explorer).
    pub ti: nv_core::textinput::TextInput,
    /// The left button is held in the box (dragging selects).
    pub dragging: bool,
    pub error: Option<String>,
}

pub struct Slideshow {
    pub on: bool,
    pub secs: f32,
    /// Image to show next (known early so it can be prefetched).
    pub next: Option<usize>,
    /// When the current picture appeared (the playback bar shows how far through the interval we are).
    pub since: Instant,
}

impl Slideshow {
    pub fn new() -> Self {
        Slideshow { on: false, secs: 3.0, next: None, since: Instant::now() }
    }
}


impl App {
    /// Ctrl+Shift+H / "Share to...": the file in front goes to the Windows share sheet.
    pub fn share_current(&mut self) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        if let Err(e) = nv_win::share::share_files(self.hwnd, &[path]) {
            self.toast(e);
        }
    }

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
            self.toast(crate::hud::failure(1001, format!("Not found: {}", path.display())));
            return;
        }
        if !self.settle_rotation() {
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

        let order = nv_core::Order { key: self.settings.sort, descending: self.settings.sort_desc, shuffle: self.folder.order.shuffle };
        match FolderList::open_with(&path, order) {
            Err(e) => self.toast(crate::hud::failure(nv_codecs::errors::classify(&e, ""), format!("Couldn't open: {e}"))),
            Ok(list) => {
                self.replace_folder(list);
                if gallery {
                    self.enter_gallery();
                }
            }
        }
    }

    /// Switch to a different folder: everything tied to the old list is dropped.
    pub(crate) fn replace_folder(&mut self, list: FolderList) {
        self.stop_slideshow(false);
        self.undo.clear();
        self.rename = None;
        self.hud.info_for = None;
        self.folder = list;
        self.strip.requested = None;
        self.strip.scroll = 0.0;
        self.read_folder_changed();
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
        self.thumb_request(Vec::new());
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
        // Draw the new size at once: until a frame of the new size is presented Windows stretches the old one over the
        // changed window, which looks like the picture jumping to another ratio.
        self.draw();
        invalidate(self.hwnd);
    }

    /// Shift+F11 / View > Borderless window: the window without its title bar and frame (not fullscreen).
    pub fn toggle_borderless_app(&mut self) {
        let on = nv_win::toggle_borderless(self.hwnd);
        let (w, h) = nv_win::client_size(self.hwnd);
        let _ = self.renderer.resize(w, h);
        self.draw();
        invalidate(self.hwnd);
        self.toast(if on { "Borderless window: Alt + drag moves it, the edges resize it, Shift+F11 brings the frame back" } else { "Window with its frame" });
    }

    // ---- removing / re-adding files from the list ------------------------------------------------

    /// Forget file `i` everywhere (list, GPU caches, metadata) after it was deleted or moved away.
    pub fn drop_file_at(&mut self, i: usize) {
        let id = self.id_at(i);
        if self.edits_for == Some(id) {
            self.edits.clear(); // a turn on a picture that is gone has nothing to save
            self.edits_for = None;
        }
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
        self.strip.requested = None;
        self.rebuild_items();
        self.read_settle(); // read mode: the current page must start its spread
        if self.folder.files.is_empty() {
            self.loader.request(Vec::new());
            set_title(self.hwnd, "NuViewer — no images");
            invalidate(self.hwnd);
        } else if self.mode == Mode::Viewer {
            self.view = self.view.fresh();
            self.refresh();
        } else {
            self.request_thumbs();
            set_title(self.hwnd, &self.title());
            invalidate(self.hwnd);
        }
    }

    /// Put `path` (back) into the list and select it.
    fn reveal_in_list(&mut self, path: PathBuf) {
        if path.parent() != Some(self.folder.dir.as_path()) || !nv_core::folder::is_listed_ext(path.extension().and_then(|e| e.to_str()).unwrap_or("")) {
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
        let ti = nv_core::textinput::TextInput::name_selected(&file_name_of(&path));
        self.rename = Some(Rename { path, ti, dragging: false, error: None });
        invalidate(self.hwnd);
    }

    pub fn rename_key(&mut self, k: u32) {
        use nv_core::textinput::Outcome;
        let m = nv_win::modifiers();
        let Some(r) = self.rename.as_mut() else { return };
        r.error = None;
        match r.ti.key(k, m.ctrl, m.shift) {
            Outcome::Escape => self.rename = None,
            Outcome::Enter => return self.commit_rename(),
            Outcome::Clipboard(op) => self.rename_clip(op),
            _ => {}
        }
        invalidate(self.hwnd);
    }

    pub fn rename_char(&mut self, c: char) {
        let Some(r) = self.rename.as_mut() else { return };
        if c.is_control() {
            return;
        }
        if "<>:\"/\\|?*".contains(c) {
            r.error = Some("A file name can't contain \\ / : * ? \" < > |".into());
        } else {
            r.error = None;
            r.ti.insert(c);
        }
        invalidate(self.hwnd);
    }

    /// Cut / copy / paste in the rename box.
    pub fn rename_clip(&mut self, op: nv_core::textinput::Clip) {
        use nv_core::textinput::Clip;
        let hwnd = self.hwnd;
        let Some(r) = self.rename.as_mut() else { return };
        match op {
            Clip::Copy => {
                let t = r.ti.selected_text();
                if !t.is_empty() {
                    let _ = shell::clipboard_copy_text(hwnd, &t);
                }
            }
            Clip::Cut => {
                let t = r.ti.cut();
                if !t.is_empty() {
                    let _ = shell::clipboard_copy_text(hwnd, &t);
                }
            }
            Clip::Paste => {
                if let Some(t) = nv_win::files::clipboard_text(hwnd) {
                    // A file name can't hold the characters Windows refuses.
                    let clean: String = t.chars().filter(|c| !"<>:\"/\\|?*".contains(*c)).collect();
                    r.ti.insert_str(&clean);
                }
            }
        }
        invalidate(hwnd);
    }

    /// The character boundary of the rename box closest to window x.
    fn rename_index_at(&self, x: f32) -> usize {
        let Some(r) = &self.rename else { return 0 };
        let Some(g) = self.hud.rename_geom else { return r.ti.cursor };
        let px = self.text_px();
        let rel = x - g.0;
        let (mut best, mut best_d) = (0usize, f32::MAX);
        for i in 0..=r.ti.chars.len() {
            let prefix: String = r.ti.chars[..i].iter().collect();
            let d = (nv_win::text::measure(&prefix, px) as f32 - rel).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// Is the point on the rename box?
    pub fn rename_box_hit(&self, x: i32, y: i32) -> bool {
        self.hud.rename_geom.is_some_and(|g| (x as f32) >= g.3 && (x as f32) < g.4 && (y as f32) >= g.1 && (y as f32) < g.2)
    }

    /// Left press in the rename box: put the caret there (Shift extends the selection); outside the box nothing happens.
    pub fn rename_press(&mut self, x: i32, y: i32) {
        if !self.rename_box_hit(x, y) {
            return;
        }
        let i = self.rename_index_at(x as f32);
        let shift = nv_win::modifiers().shift;
        if let Some(r) = self.rename.as_mut() {
            r.ti.click_at(i, shift);
            r.dragging = true;
        }
        invalidate(self.hwnd);
    }

    /// Dragging in the rename box selects.
    pub fn rename_drag(&mut self, x: i32) {
        let i = self.rename_index_at(x as f32);
        if let Some(r) = self.rename.as_mut().filter(|r| r.dragging) {
            r.ti.click_at(i, true);
            invalidate(self.hwnd);
        }
    }

    pub fn rename_double(&mut self, x: i32, y: i32) {
        if !self.rename_box_hit(x, y) {
            return;
        }
        let i = self.rename_index_at(x as f32);
        if let Some(r) = self.rename.as_mut() {
            r.ti.select_word_at(i);
        }
        invalidate(self.hwnd);
    }

    /// Right click while renaming: Cut, Copy, Paste, Delete, Select all.
    pub fn rename_menu(&mut self, x: i32, y: i32) {
        use nv_core::textinput::Clip;
        use nv_win::menu::Entry as Item;
        let Some(r) = self.rename.as_ref() else { return };
        let (has_sel, has_text) = (r.ti.selection().is_some(), !r.ti.chars.is_empty());
        let paste_ok = nv_win::files::clipboard_text(self.hwnd).is_some();
        let menu = vec![
            Item::item(1, "Cut\tCtrl+X").enabled(has_sel),
            Item::item(2, "Copy\tCtrl+C").enabled(has_sel),
            Item::item(3, "Paste\tCtrl+V").enabled(paste_ok),
            Item::item(4, "Delete").enabled(has_sel),
            Item::Separator,
            Item::item(5, "Select all\tCtrl+A").enabled(has_text),
        ];
        match nv_win::menu::popup(self.hwnd, x, y, &menu) {
            Some(1) => self.rename_clip(Clip::Cut),
            Some(2) => self.rename_clip(Clip::Copy),
            Some(3) => self.rename_clip(Clip::Paste),
            Some(4) => {
                if let Some(r) = self.rename.as_mut() {
                    let _ = r.ti.cut();
                }
            }
            Some(5) => {
                if let Some(r) = self.rename.as_mut() {
                    r.ti.select_all();
                }
            }
            _ => {}
        }
        invalidate(self.hwnd);
    }

    fn commit_rename(&mut self) {
        let Some(r) = self.rename.as_mut() else { return };
        let new_name: String = r.ti.text();
        let old = r.path.clone();
        match fileops::rename_file(&old, &new_name) {
            Err(e) => {
                r.error = Some(crate::hud::failure(7001, e));
                invalidate(self.hwnd);
            }
            Ok(new) => {
                self.rename = None;
                if new == old {
                    invalidate(self.hwnd);
                    return;
                }
                // The RAW of a grouped pair takes the new name too, so the pair stays a pair.
                let mut ops = vec![UndoOp::Moved { from: old.clone(), to: new.clone() }];
                if nv_core::rawpair::group() {
                    for c in nv_core::rawpair::companions_of(&old) {
                        let to = nv_core::rawpair::renamed_companion(&c, &new);
                        // never over a file that is already there
                        if !to.exists() && std::fs::rename(&c, &to).is_ok() {
                            ops.push(UndoOp::Moved { from: c, to });
                        }
                    }
                }
                self.push_undo(if ops.len() == 1 { ops.remove(0) } else { UndoOp::Group(ops) });
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
        if self.edits_for == Some(old_id) {
            self.edits_for = Some(new_id); // a pending turn follows its picture to the new name
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

    // ---- star ratings and colour labels (XMP sidecars) --------------------------------------------

    /// Rate or label the picture on screen (the selected one in the gallery). Saved at once in the picture's `.xmp` sidecar, in
    /// the same fields Bridge, Lightroom, darktable and XnView use; the picture is never touched. A grouped RAW + JPG pair gets
    /// the same rating on both files.
    pub fn rate(&mut self, change: nv_core::xmp::RateChange) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        let after = nv_core::xmp::read(&path).changed(change);
        let mut failed = None;
        for p in nv_core::rawpair::with_companions(&path) {
            if let Err(e) = nv_core::xmp::write(&p, &after) {
                failed = Some(e);
            }
        }
        self.hud.rating = Some((self.id_at(i), after));
        match failed {
            None => self.toast(format!("{}  —  {}", file_name_of(&path), after.describe())),
            Some(e) => self.toast(crate::hud::failure(7008, format!("Rating not saved: {e}"))),
        }
        invalidate(self.hwnd);
    }

    // ---- plug-ins ---------------------------------------------------------------------------------

    /// Plug-ins menu > Reload: read the plug-ins folder again (new formats appear in the list at once).
    pub fn reload_plugins(&mut self) {
        let l = nv_core::plugins::load(self.settings.plugins);
        self.relist_folder();
        let skipped = if l.skipped.is_empty() { String::new() } else { format!(" — skipped: {}", l.skipped.iter().map(|(id, why)| format!("{id} ({why})")).collect::<Vec<_>>().join("; ")) };
        self.toast(format!("{} plug-in{} loaded{skipped}", l.plugins.len(), if l.plugins.len() == 1 { "" } else { "s" }));
    }

    // ---- RAW + JPG pairs --------------------------------------------------------------------------

    /// List the folder again (what is listed changed: documents, RAW + JPG grouping), staying on the same picture.
    pub fn relist_folder(&mut self) {
        let Some(cur) = self.folder.current().cloned() else { return };
        let order = self.folder.order;
        if let Ok(list) = FolderList::open_with(&cur, order) {
            let gallery = self.mode == Mode::Gallery;
            self.replace_folder(list);
            if gallery {
                self.enter_gallery();
            }
        }
    }

    /// RAW + JPG of one shot: listed as two pictures (factory) or as one (the JPG stands for both). Q+Shift, the menus, Settings.
    pub fn toggle_pairs(&mut self) {
        self.settings.pair_group = !self.settings.pair_group;
        nv_core::rawpair::set_group(self.settings.pair_group);
        let _ = self.settings.save();
        self.relist_folder();
        self.toast(if self.settings.pair_group { "RAW + JPG grouped: one picture for each pair (delete, move, copy and rename act on both)" } else { "RAW and JPG listed separately" });
        invalidate(self.hwnd);
    }

    // ---- delete ----------------------------------------------------------------------------------

    pub fn delete_target(&mut self, permanent: bool) {
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        // With RAW + JPG pairs grouped the picture stands for both files: both go.
        let paths = nv_core::rawpair::with_companions(&path);
        let name = paths.iter().map(|p| file_name_of(p)).collect::<Vec<_>>().join(" + ");
        if permanent {
            if !shell::confirm(self.hwnd, &format!("Permanently delete \"{name}\"?\n\nThis can't be undone.")) {
                invalidate(self.hwnd);
                return;
            }
            let mut failed = None;
            for p in &paths {
                if let Err(e) = std::fs::remove_file(p) {
                    failed = Some(e);
                    break;
                }
            }
            if !paths[0].exists() {
                self.drop_file_at(i);
            }
            match failed {
                None => self.toast(format!("Deleted {name} permanently")),
                Some(e) => self.toast(crate::hud::failure(7002, format!("Couldn't delete: {e}"))),
            }
        } else {
            // Ask first (Settings > Viewer turns this off): Enter = yes, Esc = no. Ctrl+Z brings the file back either way.
            if self.settings.confirm_delete && !shell::confirm_yes(self.hwnd, &format!("Move \"{name}\" to the Recycle Bin?\n\nCtrl+Z brings it back.")) {
                invalidate(self.hwnd);
                return;
            }
            let mut done = Vec::new();
            let mut failed = None;
            for p in &paths {
                match shell::recycle(self.hwnd, p) {
                    Ok(()) => done.push(UndoOp::Deleted { original: p.clone() }),
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            let front_gone = !done.is_empty() && !paths[0].exists();
            match done.len() {
                0 => {}
                1 => self.push_undo(done.remove(0)),
                _ => self.push_undo(UndoOp::Group(done)),
            }
            if front_gone {
                self.drop_file_at(i);
            }
            match failed {
                None => self.toast(format!("Moved {name} to the Recycle Bin — Ctrl+Z to undo")),
                Some(e) => self.toast(crate::hud::failure(7002, format!("Couldn't delete: {e}"))),
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
            Err(e) => self.toast(crate::hud::failure(7003, format!("Couldn't {}: {e}", if mv { "move" } else { "copy" }))),
            Ok(new) => {
                self.last_dest = Some(dest.clone());
                let dest_name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dest.display().to_string());
                // The RAW of a grouped pair travels with its JPG.
                let mut ops = vec![if mv { UndoOp::Moved { from: src.clone(), to: new.clone() } } else { UndoOp::Copied { copy: new.clone() } }];
                let mut extra_fail = None;
                let companions = if nv_core::rawpair::group() { nv_core::rawpair::companions_of(&src) } else { Vec::new() };
                for c in companions {
                    match if mv { fileops::move_file(&c, &dest) } else { fileops::copy_file(&c, &dest) } {
                        Ok(n) => ops.push(if mv { UndoOp::Moved { from: c, to: n } } else { UndoOp::Copied { copy: n } }),
                        Err(e) => extra_fail = Some(e),
                    }
                }
                self.push_undo(if ops.len() == 1 { ops.remove(0) } else { UndoOp::Group(ops) });
                if mv {
                    self.drop_file_at(i);
                } else if new.parent() == Some(self.folder.dir.as_path()) {
                    self.folder.insert_sorted(new);
                    self.after_list_change();
                }
                match extra_fail {
                    Some(e) => self.toast(crate::hud::failure(7003, format!("The RAW file did not go with it: {e}"))),
                    None => self.toast(format!("{} to {dest_name} — Ctrl+Z to undo", if mv { "Moved" } else { "Copied" })),
                }
            }
        }
        invalidate(self.hwnd);
    }

    // ---- undo ------------------------------------------------------------------------------------

    pub fn push_undo(&mut self, op: UndoOp) {
        self.undo.push(op);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self) {
        if self.undo_rotation() {
            return; // a turn that isn't saved yet comes first
        }
        let Some(popped) = self.undo.pop() else {
            // Nothing of this session's own: a burn-in made in NuEditor (the journal is shared) can still be taken back.
            if let Some(path) = self.folder.current().cloned() {
                let dir = crate::rotate::backup_dir();
                if let Some(e) = nv_editor::journal::last_for(&dir, &path) {
                    self.restore_backup(&path, &e.backup);
                    nv_editor::journal::remove(&dir, &e);
                    return;
                }
            }
            self.toast("Nothing to undo");
            return;
        };
        match popped {
            // A pair that was one step comes back as one step (the front file first: the RAW that follows then finds its JPG listed
            // and stays part of that pair).
            UndoOp::Group(ops) => ops.into_iter().for_each(|o| self.undo_one(o)),
            op => self.undo_one(op),
        }
        invalidate(self.hwnd);
    }

    fn undo_one(&mut self, op: UndoOp) {
        match op {
            UndoOp::Group(ops) => ops.into_iter().for_each(|o| self.undo_one(o)),
            UndoOp::Moved { from, to } => {
                let back = if from.parent() == to.parent() {
                    std::fs::rename(&to, &from).map(|_| from.clone()).map_err(|e| e.to_string())
                } else {
                    // Put it back by its original name if free, else with a " (2)" suffix.
                    fileops::move_file(&to, from.parent().unwrap_or(Path::new("."))).map(|p| p)
                };
                match back {
                    Err(e) => self.toast(crate::hud::failure(7005, format!("Couldn't undo: {e}"))),
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
                Err(e) => self.toast(crate::hud::failure(7005, format!("Couldn't undo: {e}"))),
                Ok(()) => {
                    if let Some(i) = self.folder.position(&copy) {
                        self.drop_file_at(i);
                    }
                    self.toast(format!("Removed the copy {}", file_name_of(&copy)));
                }
            },
            UndoOp::Rewritten { path, backup } => self.restore_backup(&path, &backup),
            UndoOp::Deleted { original } => match shell::recycle_restore(&original) {
                Err(e) => {
                    // Keep it on the stack: the user can still restore it from the Recycle Bin by hand.
                    self.toast(crate::hud::failure(7005, format!("Couldn't restore automatically ({e}) — open the Recycle Bin")));
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
                Err(e) => self.toast(crate::hud::failure(7006, format!("Couldn't copy: {e}"))),
            }
            return;
        }
        match nv_codecs::decode_file_first_frame(&path).map(nv_codecs::Image::finish_for_cpu) {
            Err(e) => self.toast(crate::hud::failure(7006, format!("Couldn't copy: {e}"))),
            Ok(img) => {
                let mut bgra = img.rgba;
                bgra.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
                match shell::clipboard_copy_image_and_file(self.hwnd, &path, img.width, img.height, &bgra) {
                    Ok(()) => self.toast("Copied image and file"),
                    Err(e) => self.toast(crate::hud::failure(7006, format!("Couldn't copy: {e}"))),
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
        set_timer(self.hwnd, TIMER_SLIDE, self.slide_ms());
        self.refresh(); // include the upcoming image in the prefetch set
        self.toast(format!("Slideshow {:.0} s — S/Esc stop · +/- speed · R shuffle the order", self.slide.secs));
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
            set_timer(self.hwnd, TIMER_SLIDE, self.slide_ms());
        }
        self.toast(format!("Slideshow interval {:.0} s", self.slide.secs));
    }

    /// Decide the next image now (so it can be prefetched). It is the next one in the list, which is in random order
    /// while shuffle is on, and the next spread in read mode.
    pub fn pick_next(&mut self) {
        let n = self.folder.files.len();
        self.slide.next = (n > 1).then(|| {
            let (start, len) = self.current_spread();
            (start + len) % n
        });
    }

    pub fn slideshow_tick(&mut self) {
        if !self.slide.on || self.mode != Mode::Viewer || self.folder.files.len() < 2 {
            return;
        }
        let n = self.folder.files.len();
        let to = self.slide.next.filter(|&i| i < n).unwrap_or_else(|| {
            let (start, len) = self.current_spread();
            (start + len) % n
        });
        if to <= self.folder.index && !self.settings.more.play_loop {
            self.stop_slideshow(true); // loop off: the slideshow ends after the last picture
            return;
        }
        self.slide.since = Instant::now();
        self.navigate_with_dir(to, 1);
        self.pick_next();
        self.refresh();
    }
}
