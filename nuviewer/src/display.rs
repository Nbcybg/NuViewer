//! How the screen around and over the picture looks: the Dim toggle, the canvas (plain colour, the picture itself blurred,
//! or a picture file the user chose) and the 360-degree view. Everything is drawn from `App::draw`; the settings are in
//! `Settings.canvas` and `Settings.more` (`nv_core::prefs`).

use super::*;
use nv_core::prefs::CanvasMode;

/// Texture id of the user's canvas picture (path ids are hashes, this is outside their way).
pub const BG_ID: u64 = 0xB6B6_B6B6_0000_0001;
const FOV_MIN: f32 = 0.35;
const FOV_MAX: f32 = 2.1;

/// The 360 view: where it looks and how wide.
#[derive(Clone, Copy)]
pub struct Pano {
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

pub struct Display {
    pub dim: bool,
    pub pano: Option<Pano>,
    pub pano_last: Option<(i32, i32)>,
    /// A picture whose shape looks like a 360 one was announced already.
    hinted: Option<u64>,
    /// The canvas picture on the GPU (its path) and the one being decoded.
    bg_loaded: String,
    bg_loading: String,
    bg_rx: Option<mpsc::Receiver<Option<nv_codecs::Image>>>,
}

impl Display {
    pub fn new() -> Display {
        Display { dim: false, pano: None, pano_last: None, hinted: None, bg_loaded: String::new(), bg_loading: String::new(), bg_rx: None }
    }
}

impl App {
    // ---- dim ----------------------------------------------------------------------------------

    pub fn toggle_dim(&mut self) {
        self.disp.dim = !self.disp.dim;
        let pct = self.settings.more.dim_pct;
        self.toast(if self.disp.dim { format!("Dimmed by {pct}%") } else { "Dim off".to_string() });
        invalidate(self.hwnd);
    }

    /// A black veil over the picture (not over the controls drawn after it).
    pub fn draw_dim(&mut self) {
        if self.disp.dim {
            let (w, h) = self.renderer.size();
            let _ = self.renderer.fill_rect([0.0, 0.0, w as f32, h as f32], [0.0, 0.0, 0.0, self.settings.more.dim_pct as f32 / 100.0]);
        }
    }

    // ---- canvas -------------------------------------------------------------------------------

    /// Is anything but the plain colour wanted behind the picture?
    fn background_mode(&self) -> CanvasMode {
        self.settings.more.canvas_mode
    }

    /// Start decoding the canvas picture when the settings name one that is not on the GPU yet.
    pub fn poll_background(&mut self) {
        if let Some(rx) = &self.disp.bg_rx {
            if let Ok(res) = rx.try_recv() {
                self.disp.bg_rx = None;
                if let Some(img) = res {
                    if self.renderer.upload(BG_ID, &img).is_ok() {
                        self.disp.bg_loaded = std::mem::take(&mut self.disp.bg_loading);
                        invalidate(self.hwnd);
                    }
                }
            }
        }
        if self.background_mode() != CanvasMode::Image {
            return;
        }
        let want = self.settings.more.canvas_image.clone();
        if want.is_empty() || want == self.disp.bg_loaded || (want == self.disp.bg_loading && self.disp.bg_rx.is_some()) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let wake = self.wake.clone();
        self.disp.bg_loading = want.clone();
        self.disp.bg_rx = Some(rx);
        std::thread::spawn(move || {
            let img = nv_codecs::decode_file(Path::new(&want)).ok().map(|i| nv_codecs::downscale(&i, 2560).oriented());
            let _ = tx.send(img);
            wake();
        });
    }

    /// Behind the picture: the blurred picture or the canvas picture, laid over the window like a cover. Nothing is
    /// drawn when the picture itself fills the window.
    pub fn draw_background(&mut self, picture: Option<[f32; 4]>) {
        let mode = self.background_mode();
        if mode == CanvasMode::Solid {
            return;
        }
        let (w, h) = self.renderer.size();
        let (wf, hf) = (w as f32, h as f32);
        if let Some(d) = picture {
            if d[0] <= 0.0 && d[1] <= 0.0 && d[2] >= wf && d[3] >= hf {
                return;
            }
        }
        let (id, blur) = match mode {
            CanvasMode::Blur => match self.renderer.current() {
                Some(id) => (id, true),
                None => return,
            },
            _ if self.renderer.has(BG_ID) => (BG_ID, false),
            _ => return,
        };
        let Some((iw, ih)) = self.renderer.image_size_of(id) else { return };
        let s = (wf / iw as f32).max(hf / ih as f32);
        let (cw, ch) = (iw as f32 * s, ih as f32 * s);
        let cover = [(wf - cw) / 2.0, (hf - ch) / 2.0, (wf + cw) / 2.0, (hf + ch) / 2.0];
        if blur {
            if let Some(lod) = self.renderer.blur_lod(id, 28.0) {
                let _ = self.renderer.draw_blurred(id, cover, lod);
            }
        } else {
            let _ = self.renderer.draw_image(id, cover, false);
        }
        let shade = self.settings.more.canvas_shade as f32 / 100.0;
        if shade > 0.0 {
            let _ = self.renderer.fill_rect([0.0, 0.0, wf, hf], [0.0, 0.0, 0.0, shade]);
        }
    }

    // ---- 360 view -----------------------------------------------------------------------------

    pub fn pano_active(&self) -> bool {
        self.disp.pano.is_some() && self.mode == Mode::Viewer
    }

    /// `Shift+P`: look into the picture as a 360 one (equirectangular: a full turn across, half a turn down).
    pub fn toggle_pano(&mut self) {
        if self.mode != Mode::Viewer || self.folder.files.is_empty() {
            return;
        }
        if self.disp.pano.take().is_some() {
            self.toast("360 view off");
            invalidate(self.hwnd);
            return;
        }
        if self.is_model() || self.renderer.image_size().is_none() || self.rotation_pending() {
            self.toast("360 view needs a picture that has finished loading");
            return;
        }
        self.disp.pano = Some(Pano { yaw: 0.0, pitch: 0.0, fov: 1.5 });
        self.toast("360 view — drag to look around, wheel to zoom, Shift+P to leave");
        invalidate(self.hwnd);
    }

    /// A picture twice as wide as high is probably a 360 one: say so once.
    pub fn maybe_hint_pano(&mut self) {
        let Some(cur) = self.renderer.current() else { return };
        if self.disp.hinted == Some(cur) || self.disp.pano.is_some() {
            return;
        }
        let Some((w, h)) = self.renderer.image_size() else { return };
        self.disp.hinted = Some(cur);
        if w >= 2000 && (w as f32 / h as f32 - 2.0).abs() < 0.02 {
            self.toast("This looks like a 360° picture — Shift+P to look around");
        }
    }

    pub fn pano_wheel(&mut self, notches: f32) {
        if let Some(p) = &mut self.disp.pano {
            p.fov = (p.fov * 0.9f32.powf(notches)).clamp(FOV_MIN, FOV_MAX);
            invalidate(self.hwnd);
        }
    }

    pub fn pano_drag(&mut self, x: i32, y: i32) {
        let (Some((lx, ly)), Some(p)) = (self.disp.pano_last, self.disp.pano.as_mut()) else { return };
        let (w, _) = self.renderer.size();
        let per_px = p.fov / w.max(1) as f32; // one pixel of drag = one pixel of the view
        p.yaw -= (x - lx) as f32 * per_px;
        p.pitch = (p.pitch + (y - ly) as f32 * per_px).clamp(-1.55, 1.55);
        if p.yaw > std::f32::consts::PI {
            p.yaw -= std::f32::consts::TAU;
        } else if p.yaw < -std::f32::consts::PI {
            p.yaw += std::f32::consts::TAU;
        }
        self.disp.pano_last = Some((x, y));
        invalidate(self.hwnd);
    }

    /// Draw the 360 view of the current picture; false if there is nothing to look into.
    pub fn draw_pano_view(&mut self) -> bool {
        let (Some(p), Some(id)) = (self.disp.pano, self.renderer.current()) else { return false };
        self.renderer.draw_pano(id, p.yaw, p.pitch, p.fov).is_ok()
    }
}
