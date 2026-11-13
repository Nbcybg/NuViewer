//! Turning the picture on screen by 45 or 90 degrees, and saving the turn.
//!
//! The turn is a transform on the GPU quad (instant, no pixels touched). It is *pending* until it is saved: Ctrl+S, or when
//! the user moves on (another picture, the gallery, closing). The setting "when I move on" decides whether that saves
//! without asking or asks first. Saving decodes the file again, turns the pixels (`nv_editor`), writes the file safely and
//! keeps the original in a backup folder so Ctrl+Z can bring it back.

use super::*;
use nv_core::settings::RotateSave;
use nv_editor::{raster, save, Op, Pixels};
use nv_win::shell::{self, Answer};

/// Where originals are kept while their rewritten version exists (pruned after a week).
pub fn backup_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("undo")
}

/// "45°", "-90°": the turn the way people say it (negative = anticlockwise).
fn say(deg: f32) -> String {
    let d = raster::normalize_degrees(deg);
    let d = if d > 180.0 { d - 360.0 } else { d };
    format!("{}°", if d.fract() == 0.0 { format!("{d:.0}") } else { format!("{d:.1}") })
}

impl App {
    fn current_picture_id(&self) -> Option<u64> {
        (!self.folder.files.is_empty()).then(|| self.id_at(self.folder.index))
    }

    /// Clockwise degrees the picture on screen is turned by and that are not saved yet (0 = upright).
    pub fn rotation(&self) -> f32 {
        if self.edits_for.is_some() && self.edits_for == self.current_picture_id() { self.edits.only_rotation().unwrap_or(0.0) } else { 0.0 }
    }

    pub fn rotation_pending(&self) -> bool {
        self.rotation() != 0.0
    }

    /// The size the view logic works with: the picture, or the box around it when it is turned.
    pub fn view_size(&self) -> Option<(u32, u32)> {
        let (w, h) = self.renderer.image_size()?;
        let turn = self.rotation();
        Some(if turn == 0.0 { (w, h) } else { raster::rotated_size(w, h, turn) })
    }

    /// Turn the picture by `delta` degrees clockwise (negative = anticlockwise).
    pub fn rotate_by(&mut self, delta: f32) {
        if self.mode != Mode::Viewer {
            return;
        }
        let Some(id) = self.current_picture_id() else { return };
        if self.is_model() {
            self.toast("3D models are turned by dragging with the mouse");
            return;
        }
        if self.renderer.current() != Some(id) || !self.renderer.has(id) {
            self.toast("Wait until the picture has loaded");
            return;
        }
        if self.renderer.frame_count(id) > 1 {
            self.toast("Animations can't be rotated");
            return;
        }
        if self.doc_pages().is_some() {
            self.toast("Pages of a document can't be rotated");
            return;
        }
        if self.edits_for != Some(id) {
            self.edits.clear();
            self.edits_for = Some(id);
        }
        self.edits.push(Op::Rotate(delta));
        let turn = self.rotation();
        if turn == 0.0 {
            self.edits_for = None;
            self.toast("Back upright — nothing to save");
        } else {
            let how = match self.settings.rotate_save {
                RotateSave::Auto => "saved when you move on",
                RotateSave::Ask => "Ctrl+S saves, or you are asked when you move on",
            };
            self.toast(format!("Rotated {} — {how}", say(turn)));
        }
        // The window shows a different box now: keep a zoomed picture covering it.
        if let Some(img) = self.view_size() {
            let win = self.renderer.size();
            self.view.clamp_pan(img, win);
        }
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    /// Ctrl+Z while a turn is pending takes the turn back (before it touches the undo list for files).
    pub fn undo_rotation(&mut self) -> bool {
        if !self.rotation_pending() {
            return false;
        }
        self.edits.clear();
        self.edits_for = None;
        self.toast("Rotation undone");
        invalidate(self.hwnd);
        true
    }

    /// The picture is about to go away (next picture, gallery, close ...). Saves or drops a pending turn according to
    /// the settings. Returns false when the user chose to stay: the caller must then not leave.
    pub fn settle_rotation(&mut self) -> bool {
        if !self.rotation_pending() {
            self.edits.clear();
            self.edits_for = None;
            return true;
        }
        let path = self.folder.files[self.folder.index].clone();
        match self.settings.rotate_save {
            RotateSave::Auto => self.save_rotation(false),
            RotateSave::Ask => {
                let target = save::target_for(&path, |p| p.exists());
                let how = match &target {
                    save::Target::Overwrite { format: save::Format::Jpeg, .. } => "The JPEG is written again at quality 95; the original is kept, Ctrl+Z restores it.".to_string(),
                    save::Target::Overwrite { .. } => "The file is replaced; the original is kept, Ctrl+Z restores it.".to_string(),
                    save::Target::Copy { path } => format!("This format can't be written back, so a PNG copy named {} is saved next to it.", file_name_of(path)),
                };
                let text = format!(
                    "Save the rotated picture?\n\n{} — turned {}\n\n{how}\n\nYes: save    No: throw the rotation away    Cancel: stay on this picture",
                    file_name_of(&path),
                    say(self.rotation())
                );
                nv_win::set_cursor_hidden(false);
                match shell::ask3(self.hwnd, &text) {
                    Answer::Yes => self.save_rotation(false),
                    Answer::No => {
                        self.edits.clear();
                        self.edits_for = None;
                        invalidate(self.hwnd);
                        true
                    }
                    Answer::Cancel => {
                        invalidate(self.hwnd);
                        false
                    }
                }
            }
        }
    }

    /// Write the pending turn to disk. `reload`: show the saved file afterwards (a plain Ctrl+S); when the caller is about
    /// to move on anyway it passes false. Returns false if nothing could be written (the turn stays pending).
    pub fn save_rotation(&mut self, reload: bool) -> bool {
        let deg = self.rotation();
        if deg == 0.0 {
            if reload {
                self.toast("Nothing to save");
            }
            return true;
        }
        let i = self.folder.index;
        let path = self.folder.files[i].clone();
        let id = self.id_at(i);
        let target = save::target_for(&path, |p| p.exists());
        let backup = match &target {
            save::Target::Overwrite { path, .. } => match save::backup_copy(path, &backup_dir()) {
                Ok(b) => Some(b),
                Err(e) => {
                    self.toast(format!("Couldn't save: no backup possible ({e})"));
                    return false;
                }
            },
            save::Target::Copy { .. } => None,
        };
        if let Err(e) = write_rotated(&path, &target, deg) {
            if let Some(b) = &backup {
                let _ = std::fs::remove_file(b);
            }
            self.toast(format!("Couldn't save: {e}"));
            invalidate(self.hwnd);
            return false;
        }
        self.edits.clear();
        self.edits_for = None;
        match (&target, backup) {
            (save::Target::Overwrite { path, .. }, Some(backup)) => {
                self.push_undo(UndoOp::Rewritten { path: path.clone(), backup });
                self.forget_picture(id);
                self.toast("Saved — Ctrl+Z restores the original");
            }
            (save::Target::Copy { path: copy }, _) => {
                self.push_undo(UndoOp::Copied { copy: copy.clone() });
                let at = self.folder.insert_sorted(copy.clone());
                if at <= self.gallery.selected && self.gallery.selected + 1 < self.folder.files.len() {
                    self.gallery.selected += 1;
                }
                self.rebuild_items();
                self.toast(format!("Saved a copy: {}", file_name_of(copy)));
            }
            _ => {}
        }
        if reload {
            self.refresh();
        }
        invalidate(self.hwnd);
        true
    }

    /// The file behind `id` changed on disk: drop everything cached about it so it is read again.
    pub fn forget_picture(&mut self, id: u64) {
        self.renderer.forget(id);
        self.meta.remove(&id);
        self.failed.remove(&id);
        self.sizes.remove(&id);
        self.gallery.failed.remove(&id);
        self.hud.info_for = None;
        self.hud.size_cache = None;
    }

    /// Ctrl+Z on a saved turn: put the backed-up original back.
    pub fn restore_backup(&mut self, path: &Path, backup: &Path) {
        match std::fs::read(backup).and_then(|bytes| save::write_atomic(path, &bytes)) {
            Err(e) => self.toast(format!("Couldn't restore the original: {e}")),
            Ok(()) => {
                let _ = std::fs::remove_file(backup);
                self.forget_picture(path_id(path));
                self.refresh();
                self.toast(format!("Restored the original {}", file_name_of(path)));
            }
        }
    }

    /// Close the viewer, saving or dropping a pending turn first (the user can cancel).
    pub fn request_quit(&mut self) {
        if self.settle_rotation() {
            quit();
        }
    }
}

/// Decode `path`, turn it by `deg`, encode and write it to `target`. No UI.
fn write_rotated(path: &Path, target: &save::Target, deg: f32) -> Result<(), String> {
    let img = nv_codecs::decode_file_first_frame(path)?;
    if let Some((w, h)) = img.reduced_from {
        return Err(format!("{w}x{h} is too large to rewrite"));
    }
    let img = img.finish_for_cpu();
    let (w, h) = raster::rotated_size(img.width, img.height, deg);
    if w as u64 * h as u64 > 250_000_000 {
        return Err("the turned picture would be too large".into());
    }
    let mut pixels = raster::rotate(&Pixels::new(img.width, img.height, img.rgba), deg);
    let (format, dest) = match target {
        save::Target::Overwrite { path, format } => (*format, path.as_path()),
        save::Target::Copy { path } => (save::Format::Png, path.as_path()),
    };
    let mut exif = None;
    if format == save::Format::Jpeg {
        // JPEG has no transparency: the corners of a 45 degree turn get a black background.
        raster::flatten(&mut pixels, [0, 0, 0]);
        exif = std::fs::read(path).ok().and_then(|bytes| save::exif_of_jpeg(&bytes, (pixels.w, pixels.h)));
    }
    let bytes = save::encode(&pixels, format, exif.as_deref())?;
    save::write_atomic(dest, &bytes).map_err(|e| format!("{}: {e}", dest.display()))
}
