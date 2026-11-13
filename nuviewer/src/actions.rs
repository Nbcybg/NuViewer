//! Everything a key (or a menu item) can do, as `Action`s from `nv_core::keys`. `on_key` looks the pressed key up in the
//! user's `Keymap`; `perform` runs the action for the current mode (viewer / gallery).

use super::*;
use nv_core::keys::Action;
use nv_win::shell;

impl App {
    /// A key was pressed (and no text box is open): run whatever it is bound to.
    pub fn on_key(&mut self, k: u32) {
        let m = nv_win::modifiers();
        match self.keymap.lookup(k, m.ctrl, m.shift, m.alt) {
            Some(a) => self.perform(a),
            None if self.mode == Mode::Gallery => self.gallery_key(k),
            None => {}
        }
    }

    pub fn perform(&mut self, a: Action) {
        let gallery = self.mode == Mode::Gallery;
        let files = self.folder.files.len();
        let cur = self.folder.index as isize;
        match a {
            Action::Next | Action::PageNext if !gallery => self.step(1),
            Action::Prev | Action::PagePrev if !gallery => self.step(-1),
            Action::Next | Action::NextFile if gallery => self.gallery_select(self.tile_of(self.gallery.selected) + 1),
            Action::Prev | Action::PrevFile if gallery => self.gallery_select(self.tile_of(self.gallery.selected).saturating_sub(1)),
            Action::PageNext if gallery => self.gallery_select(self.tile_of(self.gallery.selected) + self.gallery_page()),
            Action::PagePrev if gallery => self.gallery_select(self.tile_of(self.gallery.selected).saturating_sub(self.gallery_page())),
            Action::First | Action::FirstFile if gallery => self.gallery_select(0),
            Action::Last | Action::LastFile if gallery => self.gallery_select(usize::MAX),
            Action::NextFile => self.navigate(cur + 1),
            Action::PrevFile => self.navigate(cur - 1),
            Action::First | Action::Last if self.doc_pages().is_some() => {
                let (_, n) = self.doc_pages().unwrap_or((1, 1));
                self.set_page(if a == Action::First { 1 } else { n });
            }
            Action::First | Action::FirstFile => self.navigate(0),
            Action::Last | Action::LastFile => self.navigate(files as isize - 1),
            Action::Gallery => {
                if gallery {
                    self.leave_gallery(true);
                } else {
                    self.enter_gallery();
                }
            }
            Action::FitReset | Action::ActualSize | Action::ZoomReset if self.is_model() => {
                self.cam = nv_render::Orbit::default();
                invalidate(self.hwnd);
            }
            // Fit and 100% are the two base views the zoom keys multiply; pressing the one that is already on switches to the other.
            Action::FitReset if !gallery => {
                let already = !self.view.base_actual && self.view.fit;
                self.view.base_actual = already;
                self.view.reset_to_base();
                self.toast(if already { "100%" } else { "Fit to screen" });
                invalidate(self.hwnd);
            }
            Action::ActualSize if !gallery => {
                let already = self.view.base_actual && !self.view.fit && (self.view.zoom - 1.0).abs() < 1e-3;
                self.view.base_actual = !already;
                self.view.reset_to_base();
                self.toast(if already { "Fit to screen" } else { "100%" });
                invalidate(self.hwnd);
            }
            Action::Zoom1 | Action::Zoom2 | Action::Zoom3 | Action::Zoom4 | Action::Zoom5 if !gallery => {
                let n = match a {
                    Action::Zoom1 => 1.0,
                    Action::Zoom2 => 2.0,
                    Action::Zoom3 => 3.0,
                    Action::Zoom4 => 4.0,
                    _ => 5.0,
                };
                if self.is_model() {
                    self.cam.zoom = n;
                } else if let Some(img) = self.view_size() {
                    let win = self.renderer.size();
                    self.view.set_multiple(img, win, n);
                    self.toast(format!("{n:.0}\u{00D7} {}", if self.view.base_actual { "of 100%" } else { "of fit" }));
                }
                invalidate(self.hwnd);
            }
            Action::ZoomReset if !gallery => {
                self.view.reset_to_base();
                invalidate(self.hwnd);
            }
            Action::RotatePlus45 => self.rotate_by(45.0),
            Action::RotateMinus45 => self.rotate_by(-45.0),
            Action::RotateCw90 => self.rotate_by(90.0),
            Action::RotateCcw90 => self.rotate_by(-90.0),
            Action::SaveRotation if !gallery => {
                self.save_rotation(true);
            }
            Action::ToggleSidebar if !gallery => {
                self.sidebar.pinned = !self.sidebar.pinned;
                invalidate(self.hwnd);
            }
            Action::Edit if !gallery => self.toggle_editor(),
            Action::ToggleTexture | Action::ToggleNormalMap | Action::ToggleWireframe | Action::ToggleShadow | Action::CycleShading | Action::ToggleProjection | Action::ModelRotX | Action::ModelRotY | Action::ModelRotZ
                if self.is_model() =>
            {
                self.model_key(a);
            }
            Action::FlipUp if self.is_model() => {
                self.cam.flip_up = !self.cam.flip_up;
                invalidate(self.hwnd);
            }
            Action::Fullscreen => self.toggle_fullscreen_app(),
            Action::Slideshow => self.toggle_slideshow(),
            Action::Shuffle => {
                self.slide.shuffle = !self.slide.shuffle;
                self.slide.next = None;
                if self.slide.on {
                    self.pick_next();
                    self.refresh();
                }
                self.toast(if self.slide.shuffle { "Shuffle on" } else { "Shuffle off" });
            }
            Action::SpeedUp => self.adjust_playback(1),
            Action::SpeedDown => self.adjust_playback(-1),
            Action::PlayPause => self.toggle_play(),
            Action::ToggleSequences => {
                self.settings.sequences = !self.settings.sequences;
                self.rebuild_items();
                self.toast(if self.settings.sequences { "Image sequences folded in the gallery" } else { "Every picture shown separately" });
                if self.mode == Mode::Gallery {
                    self.gallery.selected = self.items.get(self.tile_of(self.gallery.selected)).copied().unwrap_or(0);
                    self.request_thumbs();
                }
                let _ = self.settings.save();
                invalidate(self.hwnd);
            }
            Action::ToggleControls => {
                self.controls_pinned = !self.controls_pinned;
                invalidate(self.hwnd);
            }
            Action::DiskMonitor => {
                self.settings.disk_monitor = !self.settings.disk_monitor;
                let _ = self.settings.save();
                self.toast(if self.settings.disk_monitor { "Disk speed shown" } else { "Disk speed hidden (still shown while a picture loads)" });
                self.sync_loading();
                invalidate(self.hwnd);
            }
            Action::Info => {
                self.hud.show_info = !self.hud.show_info;
                self.hud.info_for = None;
                invalidate(self.hwnd);
            }
            Action::Help => {
                self.hud.show_help = !self.hud.show_help;
                invalidate(self.hwnd);
            }
            Action::Rename => self.start_rename(),
            Action::Delete => self.delete_target(false),
            Action::DeletePermanently => self.delete_target(true),
            Action::CopyTo => self.transfer_target(false),
            Action::MoveTo => self.transfer_target(true),
            Action::CopyImage => self.copy_to_clipboard(false),
            Action::CopyPath => self.copy_to_clipboard(true),
            Action::Reveal => {
                if let Some(i) = self.target() {
                    shell::reveal_in_explorer(&self.folder.files[i]);
                }
            }
            Action::Undo => self.undo(),
            Action::Settings => self.open_settings(),
            Action::Back => {
                if self.view.area.take().is_some() {
                    invalidate(self.hwnd); // cancel a Ctrl+drag selection in progress
                } else if self.editor.open {
                    self.toggle_editor();
                } else if self.hud.show_help || self.hud.show_info {
                    self.hud.show_help = false;
                    self.hud.show_info = false;
                    invalidate(self.hwnd);
                } else if self.slide.on {
                    self.stop_slideshow(true);
                } else if gallery {
                    self.leave_gallery(false);
                } else {
                    self.request_quit();
                }
            }
            Action::Quit => self.request_quit(),
            // An action that doesn't apply in this mode (e.g. "fit" in the gallery) does nothing.
            _ => {}
        }
    }
}
