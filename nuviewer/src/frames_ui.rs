//! Ctrl+Shift+F / "Extract frames...": the file in front (a video or animation, a PDF, a 3D model) becomes a folder of pictures.
//! Format, quality and folder are the video snapshot's settings; for a 3D model the pictures start at the view on screen.

use super::*;
use nv_core::frames;

impl App {
    /// What NuEditor should be given for the file in front when it is not a still picture: the frame being looked at, saved as a picture the
    /// way Settings > Video snapshot say. A video: the frame on screen; an animation (not a GIF: a GIF is edited as a whole): the frame
    /// showing; a PDF: the page; a 3D model: the view on screen. `Ok(None)` = edit the file itself.
    pub fn frame_to_edit(&mut self, src: &Path) -> Result<Option<(PathBuf, String)>, String> {
        let prefs = self.settings.more.clone();
        let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let pick = if self.is_model() {
            let (w, h) = self.renderer.size();
            frames::Pick::Model { orbit: self.cam, style: self.settings.model, size: (w.clamp(320, 4096), h.clamp(240, 4096)) }
        } else if let Some(p) = self.media.player.as_ref().filter(|p| p.info.has_video) {
            let shown = self.media.last.as_ref().map(|f| nv_core::snapshot::Shown { width: f.width, height: f.height, rgba: f.bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect() });
            frames::Pick::Video { ms: p.frame_ms(), first_ms: p.first_frame_ms().unwrap_or(0), shown }
        } else if ext != "gif" && self.controller().map(|c| c.kind == controls::MediaKind::Animation).unwrap_or(false) {
            frames::Pick::Animation(self.renderer.frame_index(self.id_at(self.folder.index)))
        } else if matches!(ext.as_str(), "pdf" | "ai") {
            frames::Pick::Page(self.doc_page.max(1))
        } else {
            return Ok(None);
        };
        frames::frame_for_editing(src, pick, &prefs).map(Some)
    }

    pub fn extract_frames_current(&mut self) {
        if self.anim_active() {
            self.anim_frames();
            return;
        }
        let Some(i) = self.target() else { return };
        let path = self.folder.files[i].clone();
        let (path, _) = nv_codecs::split_page(&path);
        let mut opts = frames::Options::from_prefs(&self.settings.more);
        if self.is_model() {
            // The turntable starts exactly where the user is looking, drawn the way they have it set up, at the size of the window.
            let (w, h) = self.renderer.size();
            opts.orbit = self.cam;
            opts.style = self.settings.model;
            opts.model_size = (w.clamp(320, 4096), h.clamp(240, 4096));
        }
        let results = nv_win::progress::run("Extract frames", &format!("{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), move |cancel, report| {
            frames::extract_many(&[path], &opts, cancel, &mut |_, _, _, done, total| report.set(done, total))
        });
        if let Some(results) = results {
            nv_win::shell::message(&frames::summary_many(&results));
            if let Some(first) = frames::first_output(&results) {
                nv_win::shell::reveal_in_explorer(&first);
            }
        }
    }
}
