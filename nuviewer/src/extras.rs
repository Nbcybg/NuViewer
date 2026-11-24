//! Things the viewer does on one key press with no questions: save the frame of a video (snapshot) and convert the picture on
//! screen to PNG / JPEG next to the file. Both run on a worker thread and answer with a toast.

use super::*;
use nv_core::i18n::{t, tf};
use nv_core::quick::{self, Target};
use nv_core::snapshot::{self, Shown};

pub struct Jobs {
    rx: Vec<mpsc::Receiver<String>>,
}

impl Jobs {
    pub fn new() -> Jobs {
        Jobs { rx: Vec::new() }
    }
}

impl App {
    /// `Ctrl+Shift+S` / right-click > Save this frame: the frame on screen as a picture (format, folder, colours: Settings).
    pub fn snapshot(&mut self) {
        // A layered picture with its panel open: the picture as the panel has it.
        if self.anim_active() && self.anim_snapshot() {
            return;
        }
        if self.layers_save() {
            return;
        }
        let prefs = self.settings.more.clone();
        let (tx, rx) = mpsc::channel::<String>();
        let wake = self.wake.clone();
        if let Some(p) = &self.media.player {
            if !p.info.has_video {
                self.toast(t("snapshot.not_video"));
                return;
            }
            let src = self.media.origin.clone();
            let ms = p.frame_ms();
            let first = p.first_frame_ms().unwrap_or(0);
            let shown = self.media.last.as_ref().map(|f| Shown { width: f.width, height: f.height, rgba: f.bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect() });
            self.toast(t("snapshot.working"));
            std::thread::spawn(move || {
                let msg = match snapshot::take(&src, ms, first, &prefs, shown) {
                    Ok(s) => {
                        let name = s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if s.note.is_empty() { tf("snapshot.saved", &[&name]) } else { tf("snapshot.saved_note", &[&name, &s.note]) }
                    }
                    Err(e) => tf("snapshot.failed", &[&e]),
                };
                let _ = tx.send(msg);
                wake();
            });
        } else if let Some(c) = self.controller().filter(|c| c.kind == controls::MediaKind::Animation) {
            // A GIF / APNG / WebP: the frame that is showing, as it is.
            let Some(path) = self.folder.files.get(self.folder.index).cloned() else { return };
            let id = self.id_at(self.folder.index);
            let frame = self.renderer.frame_index(id);
            let _ = c;
            std::thread::spawn(move || {
                let msg = (|| -> Result<String, String> {
                    let img = nv_codecs::decode_file(&path)?;
                    let rgba = if frame == 0 { img.rgba.clone() } else { img.extra_frames.get(frame - 1).map(|f| f.rgba.clone()).unwrap_or_else(|| img.rgba.clone()) };
                    let s = snapshot::take_shown(&path, frame as u64 + 1, &prefs, Shown { width: img.width, height: img.height, rgba })?;
                    Ok(tf("snapshot.saved", &[&s.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]))
                })()
                .unwrap_or_else(|e| tf("snapshot.failed", &[&e]));
                let _ = tx.send(msg);
                wake();
            });
        } else {
            // A still picture has no frame to take: Ctrl+Shift+S is then the usual "Save as".
            if self.target().is_some() {
                self.save_as();
            } else {
                self.toast(t("snapshot.not_video"));
            }
            return;
        }
        self.jobs.rx.push(rx);
    }

    /// `Ctrl+Alt+S` / File > Save as: a copy of the picture under another name, or in another format (PNG, JPEG, WebP, BMP, TIFF, TGA;
    /// the same format is a plain copy). Written on a worker thread, answered with a toast.
    pub fn save_as(&mut self) {
        let Some(i) = self.target() else { return };
        let (path, _) = nv_codecs::split_page(&self.folder.files[i]);
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
        let src_ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let first = if src_ext == "png" { "jpg" } else { "png" };
        let mut filter = String::new();
        // The format the file is not in comes first, so the suggested name is a different format.
        let mut formats: Vec<_> = quick::SAVE_AS_FORMATS.to_vec();
        formats.sort_by_key(|(e, _)| *e != first);
        for (ext, name) in &formats {
            filter += &format!("{name} (*.{ext})\0*.{ext}\0");
        }
        filter.push('\0');
        // Tests: the dialog is skipped when this variable names the file to write.
        let test_target = std::env::var_os("NUVIEWER_SAVE_AS").map(PathBuf::from);
        let Some(dst) = test_target.or_else(|| nv_win::shell::save_dialog(self.hwnd, "Save a copy as", &filter, &format!("{stem}.{first}"), path.parent(), first)) else { return };
        let quality = self.settings.editor_quality.clamp(1, 100) as u8;
        let (tx, rx) = mpsc::channel::<String>();
        let wake = self.wake.clone();
        self.toast("Saving\u{2026}");
        std::thread::spawn(move || {
            let msg = match quick::save_as(&path, &dst, quality) {
                Ok(p) => format!("Saved {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                Err(e) => format!("Couldn't save: {}", nv_codecs::errors::plain(&e)),
            };
            let _ = tx.send(msg);
            wake();
        });
        self.jobs.rx.push(rx);
    }

    /// `Ctrl+Alt+P` / `Ctrl+Alt+J` / right-click > Convert to PNG / JPEG: a copy next to the file, same name, no questions.
    pub fn quick_convert(&mut self, target: Target) {
        let Some(path) = self.folder.files.get(self.folder.index).cloned() else { return };
        // A page of a document is not a file of its own.
        let (path, _) = nv_codecs::split_page(&path);
        let (tx, rx) = mpsc::channel::<String>();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let msg = match quick::convert_file(&path, target) {
                Ok(p) => tf("convert.done", &[&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]),
                Err(e) => tf("convert.failed", &[&nv_codecs::errors::plain(&e)]),
            };
            let _ = tx.send(msg);
            wake();
        });
        self.jobs.rx.push(rx);
    }

    /// Show what finished jobs have to say (called when the window is woken).
    pub fn jobs_pump(&mut self) {
        let mut done = Vec::new();
        self.jobs.rx.retain(|rx| match rx.try_recv() {
            Ok(m) => {
                done.push(m);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for m in done {
            self.toast(m);
            invalidate(self.hwnd);
        }
    }
}
