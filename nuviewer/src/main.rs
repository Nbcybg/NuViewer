#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod controls;
mod hud;
mod loading;
mod mouse;
mod ops;
mod settings_ui;

use hud::Hud;
use nv_core::keys::Keymap;
use nv_core::sequence::{self, Run};
use nv_core::settings::{Settings, WheelAction};
use nv_core::{FolderList, Layout, Loaded, Loader, UndoOp};
use ops::{Rename, Slideshow};
use nv_render::{GpuDevice, Renderer};
use nv_win::{invalidate, kill_timer, quit, set_timer, set_title, Event, WindowHandle};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const VK_DELETE: u32 = 0x2E;

/// `SetTimer` ids.
const TIMER_ANIM: u32 = 1;
const TIMER_TOAST: u32 = 2;
const TIMER_SLIDE: u32 = 3;
const TIMER_CURSOR: u32 = 4;
const TIMER_EDGE: u32 = 5;
const TIMER_SEQ: u32 = 6;
const TIMER_UI: u32 = 7;
/// Next band of a big picture going to the GPU.
const TIMER_UPLOAD: u32 = 8;
/// Redraws the loading panel / disk readout.
const TIMER_LOAD: u32 = 9;
/// Fullscreen: how often the pointer is checked against the bottom screen edge (taskbar reveal).
const EDGE_POLL_MS: u32 = 80;
/// In fullscreen the pointer disappears after this long without moving.
const CURSOR_IDLE_MS: u32 = 2000;

const VK_BACK: u32 = 0x08;
const VK_RETURN: u32 = 0x0D;
const VK_UP: u32 = 0x26;
const VK_DOWN: u32 = 0x28;
const VK_END: u32 = 0x23;
const VK_HOME: u32 = 0x24;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;
const VK_ESCAPE: u32 = 0x1B;

/// Colour around the picture (0xRRGGBB), set from the settings; the CPU first frame and the GPU frames both use it.
static CANVAS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x1A1A1A);

fn canvas_rgb() -> u32 {
    CANVAS.load(Ordering::Relaxed)
}

/// The same colour as linear RGBA for the sRGB render target.
fn clear_color() -> [f32; 4] {
    srgb(canvas_rgb(), 1.0)
}
/// Cap on GPU memory spent on cached textures (incl. mips): a share of the card's memory, within these bounds.
const GPU_BUDGET_MIN: usize = 768 * 1024 * 1024;
const GPU_BUDGET_MAX: usize = 2560 * 1024 * 1024;
/// Max time spent uploading textures per wake-up, so key presses stay responsive.
const UPLOAD_SLICE: Duration = Duration::from_millis(8);

/// Gallery metrics in logical pixels (multiplied by the monitor's DPI scale).
const GALLERY_CELL: f32 = 176.0;
const GALLERY_CELL_MIN: f32 = 96.0;
const GALLERY_CELL_MAX: f32 = 264.0;
const GALLERY_GAP: f32 = 10.0;
const GALLERY_MARGIN: f32 = 16.0;
/// Rows of thumbnails requested beyond the visible ones (and kept on the GPU, plus a bit more).
const PREFETCH_ROWS: usize = 2;
const KEEP_ROWS: usize = 4;

/// sRGB hex (0xRRGGBB) to linear RGBA, for use as a tint with the sRGB render target.
fn srgb(hex: u32, a: f32) -> [f32; 4] {
    let ch = |shift: u32| {
        let c = ((hex >> shift) & 0xFF) as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    [ch(16), ch(8), ch(0), a]
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Viewer,
    Gallery,
}

struct Gallery {
    cell: f32,
    selected: usize,
    scroll: f32,
    dpi: f32,
    /// Thumbnails that failed to generate (shown as a dark-red tile).
    failed: HashSet<u64>,
    /// Ids of thumbnails currently worth keeping on the GPU.
    keep: HashSet<u64>,
    req_start: Option<Instant>,
    ready_ms: Option<f64>,
}

fn file_name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn path_id(p: &Path) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
}

/// View state: either "fit" (scale to window, never upscale) or an explicit zoom with pan.
struct View {
    fit: bool,
    zoom: f32,
    pan: (f32, f32), // offset of image centre from window centre, in pixels
    /// Last pointer position while the left button is held (for panning).
    drag_last: Option<(i32, i32)>,
    /// Left button held: the view to go back to on release (fit, zoom, pan). While set, the picture is zoomed in.
    held: Option<(bool, f32, (f32, f32))>,
    /// Ctrl + left drag in progress: the rubber-band corners (start, current) in client pixels.
    area: Option<((i32, i32), (i32, i32))>,
    /// A press on an already zoomed picture: where it started. Released without moving it cancels the zoom (back to fit);
    /// moving it pans instead.
    click_cancel: Option<(i32, i32)>,
}

impl View {
    fn new() -> Self {
        View { fit: true, zoom: 1.0, pan: (0.0, 0.0), drag_last: None, held: None, area: None, click_cancel: None }
    }

    /// Keep the picture covering the window: when it is larger than the window an edge can never move inside it
    /// (no canvas shows), when it is smaller it stays centred. Only matters for an explicit zoom, not for "fit".
    fn clamp_pan(&mut self, img: (u32, u32), win: (u32, u32)) {
        if self.fit {
            return;
        }
        let limit = |size: f32, win: u32| ((size - win as f32) / 2.0).max(0.0);
        let (lx, ly) = (limit(img.0 as f32 * self.zoom, win.0), limit(img.1 as f32 * self.zoom, win.1));
        self.pan = (self.pan.0.clamp(-lx, lx), self.pan.1.clamp(-ly, ly));
    }

    /// The point a zoom is centred on: the pointer, but never outside the picture. Over the canvas it becomes the
    /// nearest picture edge, so zooming there goes to the border of the picture and not into empty space.
    fn anchor(&self, img: (u32, u32), win: (u32, u32), cursor: (i32, i32)) -> (f32, f32) {
        let r = self.dst_rect(img, win);
        ((cursor.0 as f32).clamp(r[0], r[2]), (cursor.1 as f32).clamp(r[1], r[3]))
    }

    /// Set the scale to `new` around the pointer (see `anchor`); the result is clamped so no canvas shows.
    fn zoom_to(&mut self, img: (u32, u32), win: (u32, u32), cursor: (i32, i32), new: f32) {
        let (ax, ay) = self.anchor(img, win, cursor);
        let old = self.scale(img, win);
        let (cx, cy) = self.centre(win);
        let k = new / old;
        let (nx, ny) = (ax - (ax - cx) * k, ay - (ay - cy) * k);
        self.fit = false;
        self.zoom = new;
        self.pan = (nx - win.0 as f32 / 2.0, ny - win.1 as f32 / 2.0);
        self.clamp_pan(img, win);
    }

    /// Zoom so the client-pixel rectangle `r` (l, t, r, b) fills the window, centred. Returns false if it is too small to mean it.
    fn zoom_to_rect(&mut self, img: (u32, u32), win: (u32, u32), r: [i32; 4]) -> bool {
        let (rw, rh) = ((r[2] - r[0]) as f32, (r[3] - r[1]) as f32);
        if rw < 8.0 || rh < 8.0 {
            return false;
        }
        let old = self.scale(img, win);
        let new = (old * (win.0 as f32 / rw).min(win.1 as f32 / rh)).clamp(0.02, 64.0);
        let k = new / old;
        let (cx, cy) = self.centre(win);
        let (rcx, rcy) = ((r[0] + r[2]) as f32 / 2.0, (r[1] + r[3]) as f32 / 2.0);
        self.fit = false;
        self.zoom = new;
        self.pan = (-(rcx - cx) * k, -(rcy - cy) * k);
        self.clamp_pan(img, win);
        true
    }

    fn fit_scale(img: (u32, u32), win: (u32, u32)) -> f32 {
        (win.0 as f32 / img.0 as f32).min(win.1 as f32 / img.1 as f32).min(1.0)
    }

    fn scale(&self, img: (u32, u32), win: (u32, u32)) -> f32 {
        if self.fit { Self::fit_scale(img, win) } else { self.zoom }
    }

    fn centre(&self, win: (u32, u32)) -> (f32, f32) {
        if self.fit { (win.0 as f32 / 2.0, win.1 as f32 / 2.0) } else { (win.0 as f32 / 2.0 + self.pan.0, win.1 as f32 / 2.0 + self.pan.1) }
    }

    /// Destination rect in client pixels. Origin is snapped to whole pixels so 100% is texel-exact.
    fn dst_rect(&self, img: (u32, u32), win: (u32, u32)) -> [f32; 4] {
        let s = self.scale(img, win);
        let (w, h) = (img.0 as f32 * s, img.1 as f32 * s);
        let (cx, cy) = self.centre(win);
        let (l, t) = ((cx - w / 2.0).round(), (cy - h / 2.0).round());
        [l, t, l + w, t + h]
    }

    /// Ctrl + wheel: zoom around the pointer. Zooming out stops at the fitted view (and returns to "fit").
    fn zoom_at(&mut self, img: (u32, u32), win: (u32, u32), cursor: (i32, i32), notches: f32) {
        let old = self.scale(img, win);
        let floor = Self::fit_scale(img, win);
        let new = (old * 1.15f32.powf(notches)).clamp(floor, 64.0);
        if new <= floor * 1.001 {
            self.fit = true;
            self.pan = (0.0, 0.0);
            return;
        }
        self.zoom_to(img, win, cursor, new);
    }
}

struct Meta {
    width: u32,
    height: u32,
    decode_ms: f64,
    /// Original size if the image was scaled down to fit the GPU.
    reduced_from: Option<(u32, u32)>,
    /// (page, page count) for multi-page documents such as PDF.
    page: Option<(u32, u32)>,
    /// Triangle count for 3D models.
    tris: Option<usize>,
    /// Pictures a 3D model brings with it (0 = plain surface).
    textures: usize,
    /// Bytes read from disk and the seconds that took (whole-file reads only).
    read: Option<(u64, f64)>,
}

/// A decoded picture waiting for its turn to be copied to the GPU.
struct Staged {
    id: u64,
    img: nv_codecs::Image,
    decode_ms: f64,
    read: Option<(u64, f64)>,
}

/// What the picture being copied to the GPU came with: (id, decode ms, read).
type Uploading = (u64, f64, Option<(u64, f64)>);

/// Bytes a picture takes on the GPU (RGBA + mip chain), frames included.
fn gpu_bytes(img: &nv_codecs::Image) -> usize {
    img.width as usize * img.height as usize * 4 * 4 / 3 * img.frame_count()
}

struct App {
    hwnd: WindowHandle,
    renderer: Renderer,
    loader: Loader,
    thumb_loader: Loader,
    mode: Mode,
    gallery: Gallery,
    wake: Arc<dyn Fn() + Send + Sync>,
    folder: FolderList,
    view: View,
    /// Direction of the last navigation (+1/-1): prefetch leans this way.
    dir: isize,
    meta: HashMap<u64, Meta>,
    failed: HashMap<u64, String>,
    started: Instant,
    first_image_ms: Option<f64>,
    /// Set when a navigation begins; cleared once the new image has been presented.
    nav_start: Option<Instant>,
    last_nav_ms: Option<f64>,
    hud: Hud,
    rename: Option<Rename>,
    undo: Vec<UndoOp>,
    /// Last folder picked for move/copy; the next picker opens there.
    last_dest: Option<PathBuf>,
    slide: Slideshow,
    rng: u64,
    last_mouse: (i32, i32),
    /// Wheel movement not yet worth a whole step (high-resolution wheels send fractions of a notch).
    wheel_acc: i32,
    /// The pointer is on the top edge: the info bar is shown.
    top_bar: bool,
    top_bar_h: f32,
    /// Page (1-based) of the current multi-page document (PDF) being shown; 1 for everything else.
    doc_page: u32,
    /// Camera for 3D models, and where a left-button drag that turns the model last was.
    cam: nv_render::Orbit,
    orbit_last: Option<(i32, i32)>,
    settings: Settings,
    keymap: Keymap,
    /// Gallery tiles as file indices: every file, or (sequences folded) the first frame of each run.
    items: Vec<usize>,
    runs: Vec<Run>,
    /// Playback of a numbered image sequence / an animation (see `controls.rs`).
    seq_playing: bool,
    anim_paused: bool,
    anim_speed: f32,
    fps: u32,
    controls_pinned: bool,
    /// The left button went down in an edge zone (-1 left, +1 right) at this point: a click there turns the page.
    edge_press: Option<(i32, (i32, i32))>,
    /// Dragging the playback seek bar.
    seek_drag: bool,
    /// The next navigation keeps the zoom (stepping through a sequence).
    keep_view_once: bool,
    /// Fullscreen: the pointer touched the bottom edge, so the Windows taskbar is being shown.
    taskbar_shown: bool,
    edge_ticks: u8,
    /// Decoded pictures waiting to be copied to the GPU in bands (see `pump_uploads`) and the one being copied.
    staged: VecDeque<Staged>,
    uploading: Option<Uploading>,
    /// GPU bytes of every picture decoded so far, so a neighbour that wouldn't fit isn't decoded again and again.
    sizes: HashMap<u64, usize>,
    gpu_budget: usize,
    /// A decode was cancelled while it was still wanted: ask for it again.
    retry: bool,
    /// The picture the user is waiting for, and since when (drives the loading panel).
    loading_since: Option<(u64, Instant)>,
}

impl App {
    fn id_at(&self, i: usize) -> u64 {
        if i == self.folder.index && self.doc_page > 1 {
            path_id(&self.load_path(i))
        } else {
            path_id(&self.folder.files[i])
        }
    }

    /// What to ask the decoder for: the file itself, or "file.pdf?page=N" for a later page of the current document.
    /// True when the current picture is a 3D model whose geometry is on the GPU (drawn live, turned with the mouse).
    fn is_model(&self) -> bool {
        !self.folder.files.is_empty() && self.mode == Mode::Viewer && self.renderer.has_mesh(self.id_at(self.folder.index))
    }

    fn load_path(&self, i: usize) -> PathBuf {
        if i == self.folder.index && self.doc_page > 1 {
            nv_codecs::page_path(&self.folder.files[i], self.doc_page)
        } else {
            self.folder.files[i].clone()
        }
    }

    /// (page, page count) of the current picture if it is a multi-page document.
    fn doc_pages(&self) -> Option<(u32, u32)> {
        if self.folder.files.is_empty() {
            return None;
        }
        self.meta.get(&self.id_at(self.folder.index)).and_then(|m| m.page).filter(|&(_, n)| n > 1)
    }

    /// The pages right before and after the current one: kept decoded so turning a page is instant.
    fn wanted_pages(&self) -> Vec<PathBuf> {
        let Some((p, n)) = self.doc_pages() else { return Vec::new() };
        let file = &self.folder.files[self.folder.index];
        let order: [i64; 2] = if self.dir >= 0 { [1, -1] } else { [-1, 1] };
        order.iter().map(|d| p as i64 + d).filter(|&q| q >= 1 && q <= n as i64).map(|q| nv_codecs::page_path(file, q as u32)).collect()
    }

    /// Ids of everything worth keeping decoded: neighbouring files and neighbouring pages.
    fn wanted_ids(&self) -> HashSet<u64> {
        let mut ids: HashSet<u64> = self.wanted().iter().map(|&i| self.id_at(i)).collect();
        ids.extend(self.wanted_pages().iter().map(|p| path_id(p)));
        ids
    }

    /// Indices to keep decoded, most important first: current, next in travel direction, previous, then further ahead.
    fn wanted(&self) -> Vec<usize> {
        let n = self.folder.files.len() as isize;
        let cur = self.folder.index as isize;
        let d = self.dir;
        let mut out = Vec::new();
        let push = |i: isize, out: &mut Vec<usize>| {
            if (0..n).contains(&i) && !out.contains(&(i as usize)) {
                out.push(i as usize);
            }
        };
        push(cur, &mut out);
        if let (true, Some(next)) = (self.slide.on, self.slide.next) {
            push(next as isize, &mut out); // the slideshow knows exactly where it goes next
        }
        for off in [d, -d, 2 * d, 3 * d, -2 * d] {
            push(cur + off, &mut out);
        }
        out
    }

    fn title(&self) -> String {
        if self.mode == Mode::Gallery {
            return self.gallery_title();
        }
        let Some(path) = self.folder.current() else { return "NuViewer — no images".into() };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let pos = format!("{}/{}", self.folder.index + 1, self.folder.files.len());
        let id = self.id_at(self.folder.index); // the page being shown, not just the file
        let frames = match self.renderer.frame_count(id) {
            n if n > 1 => format!(" — frame {}/{n}", self.renderer.frame_index(id) + 1),
            _ => String::new(),
        };
        let slide = if self.slide.on { format!(" — slideshow {:.0}s{}", self.slide.secs, if self.slide.shuffle { " shuffle" } else { "" }) } else { String::new() };
        let first = format!(
            "{frames}{slide}{}{}",
            self.first_image_ms.map(|m| format!(" — first image {m:.0} ms")).unwrap_or_default(),
            self.last_nav_ms.map(|m| format!(" — nav {m:.1} ms")).unwrap_or_default()
        );
        if let Some(e) = self.failed.get(&id) {
            return format!("{name} — {pos} — cannot open: {e} — NuViewer");
        }
        match self.meta.get(&id) {
            Some(m) if self.renderer.has(id) => {
                let dims = match m.reduced_from {
                    Some((ow, oh)) => format!("{}x{} (reduced from {ow}x{oh})", m.width, m.height),
                    None => format!("{}x{}", m.width, m.height),
                };
                let page = m.page.filter(|&(_, n)| n > 1).map(|(p, n)| format!(" — page {p}/{n}")).unwrap_or_default();
                let dims = match m.tris {
                    Some(t) if m.textures > 0 => format!("3D, {t} triangles, {} texture{}", m.textures, if m.textures == 1 { "" } else { "s" }),
                    Some(t) => format!("3D, {t} triangles"),
                    None => dims,
                };
                format!("{name} — {dims}{page} — {pos} — decode {:.0} ms{first} — NuViewer", m.decode_ms)
            }
            _ => format!("{name} — {pos} — loading… {} — NuViewer", self.loading_summary(id)),
        }
    }

    /// Drop GPU textures (and their metadata) that are no longer wanted. The displayed texture is
    /// kept by the renderer even if unwanted, so call this again once a new image has been shown.
    fn evict(&mut self) {
        let want_ids = self.wanted_ids();
        self.renderer.retain(|id| want_ids.contains(&id));
        let r = &self.renderer;
        self.meta.retain(|id, _| r.has(*id));
    }

    /// Run the animation timer iff the displayed image is animated and we're in the viewer.
    fn sync_anim(&mut self) {
        if self.mode == Mode::Viewer && !self.folder.files.is_empty() {
            let id = self.id_at(self.folder.index);
            if self.renderer.current() == Some(id) {
                if let Some(ms) = self.renderer.frame_delay(id) {
                    if !self.anim_paused {
                        set_timer(self.hwnd, TIMER_ANIM, self.scaled_delay(ms));
                        return;
                    }
                }
            }
        }
        kill_timer(self.hwnd, TIMER_ANIM);
    }

    /// A frame delay stretched or squeezed by the animation speed setting.
    fn scaled_delay(&self, ms: u32) -> u32 {
        ((ms.max(10) as f32 / self.anim_speed).round() as u32).max(10)
    }

    fn on_timer(&mut self) {
        if self.mode == Mode::Viewer {
            if let Some(id) = self.renderer.current() {
                if let (false, Some(ms)) = (self.anim_paused, self.renderer.advance_frame(id)) {
                    set_timer(self.hwnd, TIMER_ANIM, self.scaled_delay(ms));
                    set_title(self.hwnd, &self.title());
                    invalidate(self.hwnd);
                    return;
                }
            }
        }
        kill_timer(self.hwnd, TIMER_ANIM);
    }

    /// Show the current image if it's ready, evict textures nobody needs, and queue whatever is missing.
    fn refresh(&mut self) {
        if self.folder.files.is_empty() {
            set_title(self.hwnd, "NuViewer — no images");
            return;
        }
        let want = self.wanted();
        let cur_id = self.id_at(self.folder.index);
        self.renderer.show(cur_id);
        self.evict();

        // Neighbours are only worth loading while they fit in the GPU budget next to the picture on screen: a
        // 100-megapixel neighbour takes half a gigabyte, so a second one further out is left alone.
        let mut left = self.gpu_budget as isize;
        let mut todo: Vec<PathBuf> = Vec::new();
        for &i in &want {
            let id = self.id_at(i);
            let est = self.renderer.texture_size(id).or_else(|| self.sizes.get(&id).copied()).unwrap_or(0) as isize;
            if i != self.folder.index && est > left {
                continue;
            }
            left -= est;
            if !self.renderer.has(id) && !self.failed.contains_key(&id) && !self.upload_busy_with(id) {
                todo.push(self.load_path(i));
            }
        }
        // Next to a really big picture only one neighbour is prepared at a time: each is hundreds of megabytes of
        // pixels in memory while it decodes.
        let current_bytes = self.renderer.texture_size(self.id_at(self.folder.index)).or_else(|| self.sizes.get(&self.id_at(self.folder.index)).copied()).unwrap_or(0);
        if current_bytes > 256 * 1024 * 1024 {
            todo.truncate(2);
        }
        // Neighbouring pages of a document go right after the current picture (before further-away files).
        let extra: Vec<PathBuf> = self.wanted_pages().into_iter().filter(|p| !self.renderer.has(path_id(p)) && !self.failed.contains_key(&path_id(p)) && !self.upload_busy_with(path_id(p))).collect();
        let at = todo.len().min(1);
        todo.splice(at..at, extra);
        self.loader.request(todo);
        self.sync_anim();
        self.sync_loading();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    fn navigate(&mut self, to: isize) {
        let n = self.folder.files.len() as isize;
        if n == 0 {
            return;
        }
        let to = to.clamp(0, n - 1) as usize;
        if to == self.folder.index {
            return;
        }
        let dir = if to > self.folder.index { 1 } else { -1 };
        self.navigate_with_dir(to, dir);
    }

    /// Go to image `to`, prefetching in direction `dir` (slideshow wraps from last to first, which is still "forward").
    fn navigate_with_dir(&mut self, to: usize, dir: isize) {
        if to >= self.folder.files.len() || to == self.folder.index {
            return;
        }
        self.dir = dir;
        self.folder.index = to;
        self.doc_page = 1;
        self.cam = nv_render::Orbit::default();
        self.nav_start = Some(Instant::now());
        if !std::mem::take(&mut self.keep_view_once) {
            self.view = View::new();
            self.anim_paused = false;
        }
        if self.slide.on {
            set_timer(self.hwnd, TIMER_SLIDE, (self.slide.secs * 1000.0) as u32); // manual navigation restarts the interval
        }
        self.refresh();
    }

    // ---- gallery ---------------------------------------------------------------------------------

    fn layout(&self) -> Layout {
        let (w, h) = self.renderer.size();
        let d = self.gallery.dpi;
        Layout::new(self.items.len(), self.gallery.cell, GALLERY_GAP * d, GALLERY_MARGIN * d, w as f32, h as f32)
    }

    /// File index shown by gallery tile `tile`.
    fn file_of(&self, tile: usize) -> usize {
        self.items.get(tile).copied().unwrap_or(0)
    }

    /// Gallery tile that contains file `file` (a frame in the middle of a folded sequence belongs to its first frame's tile).
    fn tile_of(&self, file: usize) -> usize {
        match self.items.binary_search(&file) {
            Ok(t) => t,
            Err(t) => t.saturating_sub(1),
        }
    }

    /// The folded sequence containing file `file`, if sequences are folded.
    fn run_of(&self, file: usize) -> Option<Run> {
        self.runs.iter().find(|r| (r.start..=r.end).contains(&file)).cloned()
    }

    /// Recompute which gallery tiles exist (after the folder or the sequence setting changed); keeps the selection on a tile.
    fn rebuild_items(&mut self) {
        self.runs = if self.settings.sequences { sequence::find_runs(&self.folder.files, self.settings.sequence_min as usize) } else { Vec::new() };
        self.items = sequence::gallery_items(self.folder.files.len(), &self.runs);
        if !self.items.is_empty() {
            let sel = self.gallery.selected.min(self.folder.files.len() - 1);
            self.gallery.selected = self.items[self.tile_of(sel)];
        }
    }

    fn gallery_title(&self) -> String {
        let n = self.items.len();
        if n == 0 {
            return "NuViewer — no images".into();
        }
        let sel = self.gallery.selected.min(self.folder.files.len() - 1);
        let name = self.folder.files[sel].file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let seq = self.run_of(sel).map(|r| format!(" (sequence, {} frames)", r.len())).unwrap_or_default();
        let vis = self.layout().visible(self.gallery.scroll);
        let loaded = vis.clone().filter(|&i| self.renderer.has_thumb(self.id_at(self.file_of(i)))).count();
        let ready = self.gallery.ready_ms.map(|m| format!(" in {m:.0} ms")).unwrap_or_default();
        format!("{name}{seq} — {}/{n} — gallery — thumbs {loaded}/{}{ready} — NuViewer", self.tile_of(sel) + 1, vis.len())
    }

    /// Keep thumbnails for the visible rows (plus margin) on the GPU, and queue the missing ones, visible first.
    fn request_thumbs(&mut self) {
        if self.folder.files.is_empty() {
            return;
        }
        let lay = self.layout();
        let scroll = self.gallery.scroll;
        let vis = lay.visible(scroll);
        let near = lay.visible_with_margin(scroll, PREFETCH_ROWS);
        let (items, files) = (&self.items, &self.folder.files);
        self.gallery.keep = lay.visible_with_margin(scroll, KEEP_ROWS).map(|i| path_id(&files[items[i]])).collect();
        let keep = &self.gallery.keep;
        self.renderer.retain_thumbs(|id| keep.contains(&id));

        let order = vis.clone().chain(near.filter(|i| !vis.contains(i)));
        let todo: Vec<PathBuf> = order
            .filter(|&i| {
                let id = path_id(&self.folder.files[self.items[i]]);
                !self.renderer.has_thumb(id) && !self.gallery.failed.contains(&id)
            })
            .map(|i| self.folder.files[self.items[i]].clone())
            .collect();
        self.thumb_loader.request(todo);
    }

    fn enter_gallery(&mut self) {
        if self.folder.files.is_empty() {
            return;
        }
        self.stop_slideshow(false);
        self.mode = Mode::Gallery;
        self.sync_anim(); // stops the timer
        self.doc_page = 1; // the gallery shows first pages
        self.rebuild_items();
        self.seq_playing = false;
        kill_timer(self.hwnd, TIMER_SEQ);
        self.gallery.selected = self.items.get(self.tile_of(self.folder.index)).copied().unwrap_or(0);
        self.gallery.scroll = self.layout().scroll_to_center(self.tile_of(self.gallery.selected));
        self.gallery.req_start = Some(Instant::now());
        self.gallery.ready_ms = None;
        self.loader.request(Vec::new()); // stop prefetching full images while browsing
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    /// Back to the viewer; `open` shows the gallery's selection, otherwise the image stays as it was.
    fn leave_gallery(&mut self, open: bool) {
        self.mode = Mode::Viewer;
        self.thumb_loader.request(Vec::new());
        if open && self.gallery.selected != self.folder.index {
            // Don't leave the previously viewed picture on screen while the chosen one decodes.
            if !self.renderer.has(self.id_at(self.gallery.selected)) {
                self.renderer.hide_current();
            }
            self.navigate(self.gallery.selected as isize);
        } else {
            self.refresh();
        }
    }

    /// Select gallery tile `tile` (clamped) and scroll it into view.
    fn gallery_select(&mut self, tile: usize) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let tile = tile.min(n - 1);
        self.gallery.selected = self.items[tile];
        let lay = self.layout();
        self.gallery.scroll = lay.scroll_to_show(tile, lay.clamp_scroll(self.gallery.scroll));
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    fn gallery_scroll_by(&mut self, pixels: f32) {
        let lay = self.layout();
        self.gallery.scroll = lay.clamp_scroll(self.gallery.scroll + pixels);
        self.request_thumbs();
        set_title(self.hwnd, &self.title());
        invalidate(self.hwnd);
    }

    fn gallery_zoom(&mut self, notches: f32) {
        let d = self.gallery.dpi;
        self.gallery.cell = (self.gallery.cell * 1.1f32.powf(notches)).clamp(GALLERY_CELL_MIN * d, GALLERY_CELL_MAX * d);
        let lay = self.layout();
        self.gallery.scroll = lay.scroll_to_show(self.tile_of(self.gallery.selected), lay.clamp_scroll(self.gallery.scroll));
        self.request_thumbs();
        invalidate(self.hwnd);
    }

    /// Keys that move by whole rows in the gallery (not remappable: they only make sense in a grid).
    fn gallery_key(&mut self, k: u32) {
        let lay = self.layout();
        let (sel, cols) = (self.tile_of(self.gallery.selected), lay.cols);
        match k {
            VK_DOWN => self.gallery_select(sel + cols),
            VK_UP => self.gallery_select(sel.saturating_sub(cols)),
            _ => {}
        }
    }

    /// One screen of tiles (for the Page keys in the gallery).
    fn gallery_page(&self) -> usize {
        let lay = self.layout();
        ((lay.view_h / (lay.cell + lay.gap)) as usize).saturating_sub(1).max(1) * lay.cols
    }

    fn draw_gallery(&mut self) {
        if !self.renderer.begin(clear_color()) {
            return;
        }
        let lay = self.layout();
        let scroll = lay.clamp_scroll(self.gallery.scroll);
        let d = self.gallery.dpi;
        let tile = srgb(0x2A2A2A, 1.0);
        let tile_failed = srgb(0x4A2222, 1.0);
        let accent = srgb(0x3D8BFF, 1.0);
        for i in lay.visible(scroll) {
            let r = lay.rect(i, scroll);
            let fi = self.items[i];
            let id = self.id_at(fi);
            if fi == self.gallery.selected {
                let b = 4.0 * d;
                let _ = self.renderer.fill_rect([r[0] - b, r[1] - b, r[2] + b, r[3] + b], accent);
            }
            let _ = self.renderer.fill_rect(r, if self.gallery.failed.contains(&id) { tile_failed } else { tile });
            if let Some((tw, th)) = self.renderer.thumb_size(id) {
                // Fit inside the cell, never enlarging beyond the thumbnail's own pixels; snap to whole pixels.
                let (cw, ch) = (r[2] - r[0], r[3] - r[1]);
                let s = (cw / tw as f32).min(ch / th as f32).min(1.0);
                let (w, h) = ((tw as f32 * s).round(), (th as f32 * s).round());
                let (l, t) = ((r[0] + (cw - w) / 2.0).round(), (r[1] + (ch - h) / 2.0).round());
                let _ = self.renderer.draw_thumb(id, [l, t, l + w, t + h]);
            }
            // A folded image sequence: show how many frames it holds.
            if let Some(run) = self.runs.iter().find(|r| r.start == fi).cloned() {
                if let Some((key, tw, th)) = self.overlay_small(&[format!("\u{25B6} {}", run.len())]) {
                    let (x, y) = (r[2] - tw - 4.0 * d, r[3] - th - 4.0 * d);
                    let _ = self.renderer.fill_rect([x, y, x + tw, y + th], srgb(0x0E0E0E, 0.82));
                    let _ = self.renderer.draw_overlay(key, x, y, srgb(0xF2F2F2, 1.0));
                }
            }
        }
        self.draw_hud();
        let _ = self.renderer.end();
    }

    // ---- full-image + thumbnail completion -------------------------------------------------------

    /// Take one finished full-image decode: upload it if still wanted, and show it if it's the current image.
    /// Small pictures go to the GPU at once; big ones are queued and copied in bands (`pump_uploads`).
    /// Returns true when the current image was (re)shown.
    fn ingest(&mut self, l: Loaded) -> bool {
        let id = path_id(&l.path);
        if l.cancelled {
            self.retry = true; // it was told to stop and is wanted again by now (or not: refresh sorts that out)
            return false;
        }
        let wanted = self.folder.files.is_empty() || self.wanted_ids().contains(&id);
        if !wanted || self.renderer.has(id) || self.upload_busy_with(id) {
            return false;
        }
        let read = (l.snap.total > 0 && l.snap.read >= l.snap.total).then_some((l.snap.total, l.snap.read_secs));
        match l.result {
            Ok(img) => {
                let est = gpu_bytes(&img);
                self.sizes.insert(id, est);
                let is_current = !self.folder.files.is_empty() && id == self.id_at(self.folder.index);
                let queued: usize = self.staged.iter().map(|s| gpu_bytes(&s.img)).sum();
                // A neighbour that doesn't fit next to what is already on the GPU isn't worth keeping.
                if !is_current && self.renderer.texture_bytes() + queued + est > self.gpu_budget {
                    return false;
                }
                if nv_render::Renderer::wants_staged_upload(&img) {
                    self.staged.push_back(Staged { id, img, decode_ms: l.decode_ms, read });
                    return false; // pump_uploads copies it
                }
                match self.renderer.upload(id, &img) {
                    Ok(()) => return self.finish_ingest(id, img, l.decode_ms, read),
                    Err(e) => {
                        self.failed.insert(id, e);
                    }
                }
            }
            Err(e) => {
                self.failed.insert(id, e);
            }
        }
        false
    }

    /// The picture is on the GPU: remember its facts, and show it if it is the current one.
    fn finish_ingest(&mut self, id: u64, img: nv_codecs::Image, decode_ms: f64, read: Option<(u64, f64)>) -> bool {
        let (dw, dh) = img.display_size();
        let swap = |(w, h): (u32, u32)| if img.orientation >= 5 { (h, w) } else { (w, h) };
        self.meta.insert(id, Meta { width: dw, height: dh, decode_ms, reduced_from: img.reduced_from.map(swap), page: img.page, tris: img.mesh.as_ref().map(|m| m.triangles()), textures: img.mesh.as_ref().map(|m| m.texture_count()).unwrap_or(0), read });
        if let Some(mesh) = &img.mesh {
            if let Err(e) = self.renderer.upload_mesh(id, mesh) {
                self.toast(format!("3D view unavailable: {e}"));
            }
        }
        let shown = !self.folder.files.is_empty() && id == self.id_at(self.folder.index);
        if shown {
            self.renderer.show(id);
            if let Some((ow, oh)) = img.reduced_from {
                let (ow, oh) = swap((ow, oh));
                self.toast(format!("Shown at {dw}x{dh} — the original {ow}x{oh} is more than the GPU can display"));
            }
            if self.first_image_ms.is_none() {
                self.first_image_ms = Some(self.started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        // Freeing hundreds of megabytes of pixels takes a moment: not on the UI thread.
        if img.rgba.len() >= 32 * 1024 * 1024 {
            std::thread::spawn(move || drop(img));
        }
        shown
    }

    /// Copy queued big pictures to the GPU in bands, a few milliseconds at a time, so the window keeps answering.
    /// The picture on screen goes first. Returns true when the current image was shown.
    fn pump_uploads(&mut self) -> bool {
        let t0 = Instant::now();
        let mut shown = false;
        loop {
            let cur_id = if self.folder.files.is_empty() { None } else { Some(self.id_at(self.folder.index)) };
            // The user is waiting for another picture than the one being copied: set that one aside.
            if let (Some(c), Some((pid, _))) = (cur_id, self.renderer.upload_progress()) {
                if pid != c && self.staged.iter().any(|s| s.id == c) {
                    if let (Some((id, img, _)), Some((_, decode_ms, read))) = (self.renderer.cancel_upload(), self.uploading.take()) {
                        self.staged.push_back(Staged { id, img, decode_ms, read });
                    }
                }
            }
            if self.renderer.upload_progress().is_none() {
                let pos = cur_id.and_then(|c| self.staged.iter().position(|s| s.id == c)).unwrap_or(0);
                let Some(s) = self.staged.remove(pos) else { break };
                if self.renderer.has(s.id) || (Some(s.id) != cur_id && !self.wanted_ids().contains(&s.id)) {
                    continue; // not needed any more
                }
                match self.renderer.begin_upload(s.id, s.img, 0.0) {
                    Ok(()) => self.uploading = Some((s.id, s.decode_ms, s.read)),
                    Err((_, e)) => {
                        self.failed.insert(s.id, e);
                        continue;
                    }
                }
            }
            match self.renderer.step_upload(UPLOAD_SLICE.saturating_sub(t0.elapsed())) {
                Ok(Some(done)) => {
                    let (decode_ms, read) = self.uploading.take().map(|u| (u.1, u.2)).unwrap_or((0.0, None));
                    shown |= self.finish_ingest(done.id, done.img, decode_ms, read);
                }
                Ok(None) => break,
                Err(e) => {
                    if let Some(u) = self.uploading.take() {
                        self.failed.insert(u.0, e);
                    }
                }
            }
            if t0.elapsed() >= UPLOAD_SLICE {
                break;
            }
        }
        if self.renderer.upload_progress().is_some() || !self.staged.is_empty() {
            // A timer, not a posted message: Windows paints and handles input before it delivers timers, so the
            // window stays alive between bands.
            set_timer(self.hwnd, TIMER_UPLOAD, 1);
            invalidate(self.hwnd);
        } else {
            kill_timer(self.hwnd, TIMER_UPLOAD);
        }
        shown
    }

    /// Drain finished decodes, uploading to the GPU within a small time slice.
    fn on_user(&mut self) {
        let t0 = Instant::now();
        let mut got_thumb = false;
        let mut slice_expired = false;
        while let Some(t) = self.thumb_loader.try_recv() {
            let id = path_id(&t.path);
            if self.gallery.keep.contains(&id) && !self.renderer.has_thumb(id) {
                match t.result {
                    Ok(img) => match self.renderer.upload_thumb(id, &img) {
                        Ok(()) => got_thumb = true,
                        Err(_) => {
                            self.gallery.failed.insert(id);
                        }
                    },
                    Err(_) => {
                        self.gallery.failed.insert(id);
                        got_thumb = true; // redraw to show the failed tile
                    }
                }
            }
            if t0.elapsed() > UPLOAD_SLICE {
                slice_expired = true;
                break;
            }
        }
        if got_thumb && self.mode == Mode::Gallery {
            if self.gallery.ready_ms.is_none() {
                let vis = self.layout().visible(self.gallery.scroll);
                let done = vis.clone().all(|i| {
                    let id = self.id_at(i);
                    self.renderer.has_thumb(id) || self.gallery.failed.contains(&id)
                });
                if done {
                    self.gallery.ready_ms = self.gallery.req_start.map(|s| s.elapsed().as_secs_f64() * 1000.0);
                }
            }
            set_title(self.hwnd, &self.title());
            invalidate(self.hwnd);
        }

        let mut shown_new = false;
        while let Some(l) = self.loader.try_recv() {
            shown_new |= self.ingest(l);
            if t0.elapsed() > UPLOAD_SLICE {
                slice_expired = true;
                break;
            }
        }
        if slice_expired {
            (self.wake)(); // more may be waiting: come back after pending input is handled
        }
        if !self.staged.is_empty() || self.renderer.upload_progress().is_some() {
            shown_new |= self.pump_uploads();
        }
        self.after_ingest(shown_new);
        if std::mem::take(&mut self.retry) && self.mode == Mode::Viewer {
            self.refresh(); // a stopped decode is wanted again
        }
    }

    /// Bookkeeping after pictures arrived: evict what nobody needs, restart animation timers, keep to the GPU budget.
    fn after_ingest(&mut self, shown_new: bool) {
        if shown_new {
            self.evict(); // the previously displayed texture may now be unwanted
            self.sync_anim();
            // A document page just appeared: now that its page count is known, start on the neighbouring pages.
            if self.doc_pages().is_some() && !self.wanted_pages().is_empty() {
                self.refresh();
            }
        }
        // Stay within the GPU budget by dropping the lowest-priority cached neighbours.
        if self.renderer.texture_bytes() > self.gpu_budget {
            let want = self.wanted();
            for &i in want.iter().skip(1).rev() {
                if self.renderer.texture_bytes() <= self.gpu_budget {
                    break;
                }
                self.renderer.remove(self.id_at(i));
            }
        }
        self.sync_loading();
        set_title(self.hwnd, &self.title());
        let current_failed = !self.folder.files.is_empty() && self.failed.contains_key(&self.id_at(self.folder.index));
        if shown_new || current_failed {
            invalidate(self.hwnd);
        }
    }

    /// The current picture's gallery thumbnail, scaled to fit the window: (texture id, destination rect).
    fn placeholder(&self, win: (u32, u32)) -> Option<(u64, [f32; 4])> {
        let id = self.id_at(self.folder.index.min(self.folder.files.len().checked_sub(1)?));
        let (tw, th) = self.renderer.thumb_size(id)?;
        let s = (win.0 as f32 / tw as f32).min(win.1 as f32 / th as f32);
        let (w, h) = ((tw as f32 * s).round(), (th as f32 * s).round());
        let (l, t) = (((win.0 as f32 - w) / 2.0).round(), ((win.1 as f32 - h) / 2.0).round());
        Some((id, [l, t, l + w, t + h]))
    }

    fn draw(&mut self) {
        if self.mode == Mode::Gallery {
            return self.draw_gallery();
        }
        let win = self.renderer.size();
        if let Some(i) = self.renderer.image_size() {
            self.view.clamp_pan(i, win); // the window may have been resized since the pan was set
        }
        let (dst, nearest) = match self.renderer.image_size() {
            Some(i) => (Some(self.view.dst_rect(i, win)), self.view.scale(i, win) >= 4.0),
            None => (None, false),
        };
        // Nothing decoded for the current picture yet (opened from the gallery): show its thumbnail, enlarged, instead
        // of the previous picture or an empty window.
        let placeholder = if dst.is_none() { self.placeholder(win) } else { None };
        let model_id = if self.is_model() { Some(self.id_at(self.folder.index)) } else { None };
        if self.renderer.begin(clear_color()) {
            if let Some(mid) = model_id {
                let _ = self.renderer.draw_mesh(mid, &self.cam);
            } else if let Some(d) = dst {
                let _ = self.renderer.draw_current(d, nearest);
            } else if let Some((id, rect)) = placeholder {
                let _ = self.renderer.draw_thumb(id, rect);
            }
            self.draw_loading();
            if let Some((a, b)) = self.view.area {
                self.draw_area_box(a, b);
            }
            self.draw_hud();
            let _ = self.renderer.end();
        }
        if let (Some(t), Some(cur)) = (self.nav_start, self.renderer.current()) {
            if !self.folder.files.is_empty() && cur == self.id_at(self.folder.index) {
                self.last_nav_ms = Some(t.elapsed().as_secs_f64() * 1000.0);
                self.nav_start = None;
                set_title(self.hwnd, &self.title());
            }
        }
    }

    fn handle(&mut self, ev: Event) {
        // Any mouse activity brings the pointer back in fullscreen and restarts the idle countdown.
        // Windows also sends synthetic WM_MOUSEMOVEs at an unchanged position (e.g. after a repaint): those don't count.
        let activity = match ev {
            Event::MouseMove { x, y, .. } => std::mem::replace(&mut self.last_mouse, (x, y)) != (x, y),
            Event::MouseDown { .. } | Event::Wheel { .. } | Event::RightClick { .. } => true,
            _ => false,
        };
        if activity && nv_win::is_fullscreen() {
            nv_win::set_cursor_hidden(false);
            set_timer(self.hwnd, TIMER_CURSOR, CURSOR_IDLE_MS);
        }
        if let Event::MouseMove { y, left_down: false, .. } = &ev {
            // Info bar: appears when the pointer touches the top edge, stays while it is over the bar.
            let y = *y as f32;
            let want = self.settings.top_bar && (y <= 4.0 * self.gallery.dpi.max(1.0) || (self.top_bar && y <= self.top_bar_h + 8.0));
            if want != self.top_bar && self.mode == Mode::Viewer {
                self.top_bar = want;
                invalidate(self.hwnd);
            }
        }
        match ev {
            // Test hook (only reachable through WM_NV_INJECT_KEY; no real key has this code): proves panics are survivable.
            Event::Key(0xFFFE) => panic!("injected test panic"),
            Event::Resize { width, height } => {
                let _ = self.renderer.resize(width, height);
                if self.mode == Mode::Gallery {
                    let lay = self.layout();
                    self.gallery.scroll = lay.clamp_scroll(self.gallery.scroll);
                    self.request_thumbs();
                }
                self.draw();
            }
            Event::Paint => self.draw(),
            Event::User => self.on_user(),
            Event::Open { path, gallery } => self.open_request(path, gallery),
            Event::Timer(TIMER_ANIM) => self.on_timer(),
            Event::Timer(TIMER_TOAST) => self.on_toast_timer(),
            Event::Timer(TIMER_SLIDE) => self.slideshow_tick(),
            Event::Timer(TIMER_CURSOR) => {
                if nv_win::is_fullscreen() {
                    nv_win::set_cursor_hidden(true);
                }
                kill_timer(self.hwnd, TIMER_CURSOR);
            }
            Event::Dpi(scale) => {
                let ratio = scale / self.gallery.dpi.max(0.01);
                self.gallery.dpi = scale;
                self.gallery.cell = (self.gallery.cell * ratio).clamp(GALLERY_CELL_MIN * scale, GALLERY_CELL_MAX * scale);
                if self.mode == Mode::Gallery {
                    self.request_thumbs();
                }
                invalidate(self.hwnd);
            }
            Event::Timer(TIMER_UPLOAD) => {
                let shown = self.pump_uploads();
                self.after_ingest(shown);
            }
            Event::Timer(TIMER_LOAD) => self.load_tick(),
            Event::Timer(TIMER_EDGE) => self.edge_tick(),
            Event::Timer(TIMER_SEQ) => self.seq_tick(),
            Event::Timer(TIMER_UI) => {
                if self.controls_visible() {
                    invalidate(self.hwnd); // slideshow progress
                }
            }
            Event::Timer(_) => {}
            // Middle click closes the viewer (not the gallery, and not while the rename box is open).
            Event::MiddleClick if self.mode == Mode::Viewer && self.rename.is_none() => quit(),
            Event::MiddleClick => {}
            Event::Char(c) => self.rename_char(c),
            Event::Key(k) if self.rename.is_some() => self.rename_key(k),
            Event::Key(k) => self.on_key(k),
            Event::Wheel { delta, ctrl, .. } if self.mode == Mode::Gallery => {
                let notches = delta as f32 / 120.0;
                if ctrl {
                    self.gallery_zoom(notches);
                } else {
                    let step = (self.gallery.cell + GALLERY_GAP * self.gallery.dpi) * 0.8;
                    self.gallery_scroll_by(-notches * step);
                }
            }
            Event::MouseDown { x, y, .. } if self.mode == Mode::Gallery => {
                let lay = self.layout();
                if let Some(i) = lay.hit(x as f32, y as f32, lay.clamp_scroll(self.gallery.scroll)) {
                    self.gallery_select(i);
                }
            }
            Event::DoubleClick { x, y, .. } if self.mode == Mode::Gallery => {
                let lay = self.layout();
                if let Some(i) = lay.hit(x as f32, y as f32, lay.clamp_scroll(self.gallery.scroll)) {
                    self.gallery.selected = self.file_of(i);
                    self.leave_gallery(true);
                }
            }
            Event::MouseDown { .. } | Event::MouseUp | Event::MouseMove { .. } | Event::DoubleClick { .. } | Event::Wheel { .. }
                if self.mode == Mode::Gallery => {}
            // Viewer mouse: the wheel flips pictures (Ctrl+wheel zooms); holding the left button zooms to 100%
            // (200% for a small picture) and dragging pans while it is held; Ctrl + left drag zooms into an area.
            Event::Wheel { delta, x, y, ctrl } => {
                if self.view.held.is_some() || self.view.area.is_some() {
                    return;
                }
                if self.is_model() {
                    self.cam.zoom_by(delta as f32 / 120.0); // the wheel zooms a 3D model instead of turning pages
                    invalidate(self.hwnd);
                    return;
                }
                // Ctrl swaps what the wheel does (zoom <-> pages), whichever one the settings made the default.
                let zoom = match (self.settings.wheel, ctrl) {
                    (WheelAction::Pages, true) | (WheelAction::Zoom, false) => true,
                    (WheelAction::Off, _) => return,
                    _ => false,
                };
                if zoom {
                    if let Some(i) = self.renderer.image_size() {
                        self.view.zoom_at(i, self.renderer.size(), (x, y), delta as f32 / 120.0);
                        invalidate(self.hwnd);
                    }
                } else {
                    self.wheel_step(delta);
                }
            }
            // A quick second click arrives as DoubleClick: for these gestures it is just another press.
            Event::MouseDown { x, y, ctrl } | Event::DoubleClick { x, y, ctrl } => {
                if !self.controls_press(x, y) {
                    self.viewer_press(x, y, ctrl);
                }
            }
            Event::MouseUp => self.viewer_release(),
            Event::MouseMove { x, y, left_down } => self.viewer_drag(x, y, left_down),
            Event::RightClick { x, y } => self.context_menu(x, y),
            Event::Close => {}
        }
    }
}

/// Background colour used outside the image, as 0xRRGGBB (matches the renderer's clear colour).

impl App {
    #[allow(clippy::too_many_arguments)]
    fn new(
        hwnd: WindowHandle,
        renderer: Renderer,
        loader: Loader,
        wake: Arc<dyn Fn() + Send + Sync>,
        folder: FolderList,
        started: Instant,
        dpi: f32,
        first: Option<Loaded>,
        first_image_ms: Option<f64>,
        start_gallery: bool,
        actual_size: bool,
    ) -> Self {
        let threads = worker_threads();
        let mut app = App {
            hwnd,
            renderer,
            loader,
            thumb_loader: Loader::thumbs(threads, wake.clone()),
            mode: Mode::Viewer,
            gallery: Gallery {
                cell: GALLERY_CELL * dpi,
                selected: 0,
                scroll: 0.0,
                dpi,
                failed: HashSet::new(),
                keep: HashSet::new(),
                req_start: None,
                ready_ms: None,
            },
            wake,
            folder,
            view: View::new(),
            dir: 1,
            meta: HashMap::new(),
            failed: HashMap::new(),
            started,
            first_image_ms,
            nav_start: None,
            last_nav_ms: None,
            hud: Hud::new(),
            rename: None,
            undo: Vec::new(),
            last_dest: None,
            slide: Slideshow::new(),
            last_mouse: (i32::MIN, i32::MIN),
            wheel_acc: 0,
            top_bar: false,
            top_bar_h: 40.0,
            doc_page: 1,
            cam: nv_render::Orbit::default(),
            orbit_last: None,
            keymap: Keymap::new(&Settings::load().keys),
            fps: Settings::load().fps,
            settings: Settings::load(),
            items: Vec::new(),
            runs: Vec::new(),
            seq_playing: false,
            anim_paused: false,
            anim_speed: 1.0,
            controls_pinned: false,
            edge_press: None,
            seek_drag: false,
            keep_view_once: false,
            taskbar_shown: false,
            edge_ticks: 0,
            staged: VecDeque::new(),
            uploading: None,
            sizes: HashMap::new(),
            gpu_budget: GPU_BUDGET_MIN,
            retry: false,
            loading_since: None,
            rng: 0x9E37_79B9_7F4A_7C15 ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1,
        };
        app.gpu_budget = (app.renderer.video_memory() as usize / 10 * 4).clamp(GPU_BUDGET_MIN, GPU_BUDGET_MAX);
        app.rebuild_items();
        // The first image was decoded while the GPU was starting: upload it before refresh() so it isn't re-requested.
        if let Some(l) = first {
            app.ingest(l);
            app.pump_uploads();
        }
        if actual_size {
            app.view = View { fit: false, ..View::new() };
        }
        if nv_win::is_fullscreen() {
            app.fullscreen_timers(true);
        }
        app.refresh();
        if start_gallery {
            app.enter_gallery();
        }
        app
    }
}

fn worker_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(1)).unwrap_or(1).clamp(2, 4)
}

/// CPU-rendered first frame: image already scaled to fit the window, as top-down BGRA (alpha flattened).
struct Preview {
    /// Top-left in client pixels, rounded exactly like `View::dst_rect` so the GPU frame that replaces
    /// this one lands on the same pixels (no 1 px jump when the window has an odd margin).
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    bgra: Vec<u8>,
}

fn make_preview(img: &nv_codecs::Image, win: (u32, u32)) -> Preview {
    // Same fit rule as the GPU path: shrink to fit (using the displayed, i.e. rotated, size), never enlarge.
    let (dw, dh) = img.display_size();
    let s = (win.0 as f32 / dw as f32).min(win.1 as f32 / dh as f32).min(1.0);
    let edge = ((img.width.max(img.height) as f32) * s).round().max(1.0) as u32;
    // Rotate after shrinking: the CPU only ever turns the small preview, the GPU turns the real thing.
    let scaled;
    let src = if s < 1.0 {
        scaled = nv_codecs::downscale(img, edge).oriented();
        &scaled
    } else {
        scaled = img.to_oriented();
        &scaled
    };
    let bg_rgb = canvas_rgb();
    let bg = [(bg_rgb >> 16) as u32, ((bg_rgb >> 8) & 0xFF) as u32, (bg_rgb & 0xFF) as u32];
    let mut bgra = Vec::with_capacity(src.rgba.len());
    for p in src.rgba.chunks_exact(4) {
        let a = p[3] as u32;
        let mix = |c: u8, b: u32| ((c as u32 * a + b * (255 - a) + 127) / 255) as u8;
        bgra.extend_from_slice(&[mix(p[2], bg[2]), mix(p[1], bg[1]), mix(p[0], bg[0]), 255]);
    }
    let x = (win.0 as f32 / 2.0 - dw as f32 * s / 2.0).round() as i32;
    let y = (win.1 as f32 / 2.0 - dh as f32 * s / 2.0).round() as i32;
    Preview { x, y, w: src.width as i32, h: src.height as i32, bgra }
}

/// Start-up phase: the window is up and the first image is decoding, but the GPU device is still being
/// created on a background thread. We paint the first image on the CPU (GDI) the moment it's decoded,
/// then turn into a full `App` as soon as the GPU (and the folder listing) are ready.
struct Boot {
    hwnd: WindowHandle,
    size: (u32, u32),
    loader: Loader,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Folder listing; arrives from a background thread (big folders take a while to list and sort).
    folder: Option<FolderList>,
    folder_rx: mpsc::Receiver<Result<FolderList, String>>,
    /// True if the image to open was requested straight from the command line (before the folder was listed).
    early_request: bool,
    /// File name shown in the title until the folder listing is known.
    arg_name: String,
    gpu_rx: mpsc::Receiver<Result<GpuDevice, String>>,
    gpu: Option<GpuDevice>,
    first: Option<Loaded>,
    preview: Option<Preview>,
    first_paint_ms: Option<f64>,
    started: Instant,
    start_gallery: bool,
    /// `--actual-size`: the first image opens at 100%. The CPU preview is a fitted picture, so it is skipped.
    actual_size: bool,
    dpi: f32,
    /// A second launch asked us to open something while we were still starting; applied once the App exists.
    pending_open: Option<(Option<String>, bool)>,
}

enum Step {
    Stay(Boot),
    Go(Box<App>),
}

impl Boot {
    fn rebuild_preview(&mut self) {
        self.preview = if self.actual_size {
            None
        } else {
            self.first.as_ref().and_then(|l| l.result.as_ref().ok()).map(|img| make_preview(img, self.size))
        };
    }

    fn paint(&mut self) {
        let Some(p) = &self.preview else { return };
        nv_win::paint_bgra(self.hwnd, p.x, p.y, p.w, p.h, &p.bgra, canvas_rgb());
        if self.first_paint_ms.is_none() {
            let ms = self.started.elapsed().as_secs_f64() * 1000.0;
            self.first_paint_ms = Some(ms);
            let (name, pos) = match &self.folder {
                Some(f) => (
                    f.current().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                    format!("{}/{}", f.index + 1, f.files.len()),
                ),
                None => (self.arg_name.clone(), "1/?".to_string()),
            };
            let decode = self.first.as_ref().map(|l| l.decode_ms).unwrap_or(0.0);
            set_title(self.hwnd, &format!("{name} — {}x{} — {pos} — decode {decode:.0} ms — first image {ms:.0} ms — NuViewer", p.w, p.h));
        }
    }

    fn handle(mut self, ev: Event) -> Step {
        match ev {
            Event::Resize { width, height } => {
                self.size = (width, height);
                self.rebuild_preview();
                self.paint();
            }
            Event::Paint => self.paint(),
            Event::Key(VK_ESCAPE) | Event::MiddleClick => quit(),
            Event::Open { path, gallery } => self.pending_open = Some((path, gallery)),
            Event::User => {
                // Folder listing (may arrive after the image; an unreadable folder just means "no images").
                if self.folder.is_none() {
                    if let Ok(r) = self.folder_rx.try_recv() {
                        if let Err(e) = &r {
                            set_title(self.hwnd, &format!("NuViewer — {e}"));
                        }
                        let f = r.unwrap_or(FolderList::empty());
                        if !self.early_request {
                            // Directory argument: now we know which image is first.
                            if let Some(p) = f.current() {
                                self.loader.request(vec![p.clone()]);
                            }
                        }
                        self.folder = Some(f);
                    }
                }
                while let Some(l) = self.loader.try_recv() {
                    self.first = Some(l);
                }
                if self.preview.is_none() && self.first.is_some() {
                    self.rebuild_preview();
                    self.paint();
                }
                if self.gpu.is_none() {
                    match self.gpu_rx.try_recv() {
                        Ok(Ok(gpu)) => self.gpu = Some(gpu),
                        Ok(Err(e)) => set_title(self.hwnd, &format!("NuViewer — GPU init failed: {e}")),
                        Err(_) => {}
                    }
                }
                if self.gpu.is_some() && self.folder.is_some() {
                    let (gpu, folder) = (self.gpu.take().unwrap(), self.folder.take().unwrap());
                    // The early request used the path as typed; the App identifies images by the listed path.
                    if let (Some(l), Some(cur)) = (self.first.as_mut(), folder.current()) {
                        l.path = cur.clone();
                    }
                    match Renderer::from_device(gpu, self.hwnd, self.size.0, self.size.1) {
                        Ok(renderer) => {
                            let mut app = App::new(
                                self.hwnd,
                                renderer,
                                self.loader,
                                self.wake,
                                folder,
                                self.started,
                                self.dpi,
                                self.first,
                                self.first_paint_ms,
                                self.start_gallery,
                                self.actual_size,
                            );
                            if let Some((path, gallery)) = self.pending_open.take() {
                                app.open_request(path, gallery);
                            }
                            return Step::Go(Box::new(app));
                        }
                        Err(e) => set_title(self.hwnd, &format!("NuViewer — renderer failed: {e}")),
                    }
                }
            }
            _ => {}
        }
        Step::Stay(self)
    }
}

/// `--settings`: just the Settings window (no viewer).
fn settings_only() {
    unsafe {
        let _ = windows_com_init();
    }
    if let Some(s) = settings_ui::edit(&Settings::load()) {
        if let Err(e) = s.save() {
            nv_win::shell::message(&format!("Settings not saved: {e}"));
        }
    }
}

/// The file dialogs need COM on this thread.
unsafe fn windows_com_init() -> bool {
    nv_win::com_init()
}

/// `--associate`: tick which formats Windows should offer to open with NuViewer, apply, then open Settings > Default apps.
fn associate_dialog() {
    use nv_win::registry::{self, Layout};
    let exe = std::env::current_exe().unwrap_or_default();
    let Some(chosen) = nv_win::assoc::choose(&registry::registered_exts(&Layout::real())) else { return };
    let (text, open_settings) = nv_win::assoc::apply_and_report(&exe, &chosen);
    nv_win::shell::message(&text);
    if open_settings {
        nv_win::assoc::open_default_apps_settings();
    }
}

/// `--register` / `--unregister`: add or remove NuViewer from "Open with" and Default apps (current user only).
fn file_associations(register: bool) {
    use nv_win::registry::{self, Layout};
    let exts: Vec<String> = nv_codecs::supported_extensions().iter().map(|e| format!(".{e}")).collect();
    let layout = Layout::real();
    let exe = std::env::current_exe().unwrap_or_default();
    if register {
        match registry::register(&layout, &exe, &exts) {
            Ok(()) => nv_win::shell::message(&format!(
                "NuViewer is now listed for {} image types.\n\nTo make it the default: Settings > Apps > Default apps > NuViewer, or right-click an image > Open with > Choose another app > NuViewer > Always.\n\nPick which formats with: nuviewer.exe --associate\n\nRemove it again with: nuviewer.exe --unregister",
                exts.len()
            )),
            Err(e) => nv_win::shell::message(&format!("Couldn't register file types: {e}")),
        }
    } else {
        let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "nuviewer.exe".into());
        registry::unregister(&layout, &name, &exts);
        nv_win::shell::message("NuViewer's file type registration was removed.");
    }
}

enum Shell {
    Boot(Boot),
    Run(Box<App>),
    Done,
}

fn main() {
    nv_win::crash::install();
    // Flags may appear anywhere; the first argument that isn't a flag is the file or folder to open.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    // Command-line maintenance modes: no window, just a message.
    if flag("--settings") {
        return settings_only();
    }
    if flag("--associate") {
        return associate_dialog();
    }
    if flag("--register") {
        return file_associations(true);
    }
    if flag("--unregister") {
        return file_associations(false);
    }
    let arg: Option<PathBuf> = args.iter().find(|a| !a.to_string_lossy().starts_with("--")).map(|a| nv_win::instance::absolute(Path::new(a)));
    let start_gallery = flag("--gallery");
    // Pictures open borderless-fullscreen from the first frame (--windowed opts out; --fullscreen is accepted and is the default). --actual-size starts at 100%.
    let startup_settings = Settings::load();
    CANVAS.store(startup_settings.canvas, Ordering::Relaxed);
    nv_codecs::set_icc_enabled(startup_settings.icc);
    nv_codecs::set_model_textures(startup_settings.model_textures);
    let start_fullscreen = !flag("--windowed") && startup_settings.start_fullscreen;
    let actual_size = flag("--actual-size");

    // One window: if NuViewer is already running, hand it this request and exit (--new-window opts out).
    if !flag("--new-window") {
        let request = nv_win::instance::OpenRequest { path: arg.as_ref().map(|p| p.display().to_string()), gallery: start_gallery };
        if let nv_win::instance::Instance::Forwarded = nv_win::instance::claim_or_forward(&request) {
            return;
        }
    }
    let started = Instant::now();

    // Everything slow starts *now*, before the window even exists. Worker threads can't post wake-ups until
    // the window handle is known, so they go through this slot; `Boot` also polls once the window is ready,
    // so a result that finished early is never missed.
    let hwnd_slot = Arc::new(AtomicIsize::new(0));
    let wake: Arc<dyn Fn() + Send + Sync> = {
        let slot = hwnd_slot.clone();
        Arc::new(move || {
            let h = slot.load(Ordering::Acquire);
            if h != 0 {
                nv_win::post_user_raw(h);
            }
        })
    };

    // Housekeeping well after startup so it never competes with showing the first image.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        nv_core::thumbs::prune_cache();
    });

    // 1. GPU driver load (~200+ ms) on its own thread.
    let (gpu_tx, gpu_rx) = mpsc::channel();
    {
        let wake = wake.clone();
        std::thread::spawn(move || {
            let _ = gpu_tx.send(nv_render::create_device());
            wake();
        });
    }
    // 2. Decode the requested image immediately, without waiting for the folder listing.
    let loader = Loader::full(worker_threads(), wake.clone());
    let is_file = arg.as_ref().map(|p| p.is_file()).unwrap_or(false);
    if let (true, Some(p)) = (is_file, arg.as_ref()) {
        loader.request(vec![p.clone()]);
    }
    // 3. List and sort the folder on a thread too (thousands of files take a noticeable while).
    let (folder_tx, folder_rx) = mpsc::channel();
    {
        let (wake, arg) = (wake.clone(), arg.clone());
        std::thread::spawn(move || {
            let r = match &arg {
                Some(p) => FolderList::open(p),
                None => Ok(FolderList::empty()),
            };
            let _ = folder_tx.send(r);
            wake();
        });
    }
    let arg_name = arg.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    let result = nv_win::run("NuViewer", 1280, 800, start_fullscreen, move |hwnd, w, h| {
        hwnd_slot.store(hwnd.0 as isize, Ordering::Release);
        let mut shell = Shell::Boot(Boot {
            hwnd,
            size: (w, h),
            loader,
            wake,
            folder: None,
            folder_rx,
            early_request: is_file,
            arg_name,
            gpu_rx,
            gpu: None,
            first: None,
            preview: None,
            first_paint_ms: None,
            started,
            start_gallery,
            actual_size,
            dpi: nv_win::dpi_scale(hwnd),
            pending_open: None,
        });
        Ok(Box::new(move |ev: Event| {
            shell = match std::mem::replace(&mut shell, Shell::Done) {
                Shell::Boot(b) => match b.handle(ev) {
                    Step::Stay(b) => Shell::Boot(b),
                    Step::Go(app) => Shell::Run(app),
                },
                Shell::Run(mut app) => {
                    app.handle(ev);
                    Shell::Run(app)
                }
                Shell::Done => Shell::Done,
            };
        }) as Box<dyn FnMut(Event)>)
    });

    if let Err(e) = result {
        eprintln!("NuViewer error: {e}");
        std::process::exit(1);
    }
}
