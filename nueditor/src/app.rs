//! The editor window: the picture on the left, the sliders of the recipe on the right, everything driven from the tables in
//! `nv_editor::recipe` (so a new slider needs no UI code).
//!
//! Work flow of one slider move: set the value in the recipe -> run the engine on the *proxy* (the picture shrunk to the size
//! it is shown at) -> upload that to the GPU -> draw. Nothing here ever touches the full-size picture except saving.

use crate::flow::{self, Choice, Full, Origin};
use crate::gfx::{self, pal, rgb, Text};
use nv_codecs::Image;
use nv_core::settings::Settings;
use nv_editor::engine::{self, Frame};
use nv_editor::recipe::{self, Group, Recipe, PARAMS, PARAM_COUNT, P};
use nv_editor::{sidecar, Pixels};
use nv_render::{GpuDevice, Renderer};
use nv_win::{invalidate, set_title, Event, WindowHandle};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

mod heal;
mod panel;
mod tools;
mod view;
use panel::{Item, SECTION_NAMES, SECTION_PRESETS};
use tools::{Mode, Tools, ID_WHEEL, SECTION_CURVES, SECTION_WHEEL};
use view::{View, ID_DETAIL, ID_DETAIL_ORIG, ID_SCOPE};

const ID_EDIT: u64 = 1;
const ID_ORIG: u64 = 2;
const TIMER_TOAST: u32 = 1;
const VK_ESCAPE: u32 = 0x1B;
const VK_TAB: u32 = 0x09;
const VK_COMMA: u32 = 0xBC;
const VK_PERIOD: u32 = 0xBE;

pub struct Options {
    pub windowed: bool,
    pub origin: Origin,
    /// A turn made in the viewer, to continue here.
    pub start_angle: Option<f32>,
}

pub enum Msg {
    Loaded { path: PathBuf, result: Result<flow::Decoded, String> },
    /// The render worker finished a picture (`gen` says which request it answers).
    Rendered { gen: u64, out: Pixels, ms: f32 },
}

/// One request to the render worker.
struct RenderJob {
    gen: u64,
    src: Arc<Pixels>,
    /// Names the picture `src` is a proxy of; changes whenever `src` is rebuilt.
    src_gen: u64,
    rec: Recipe,
    /// `src` already has its heal strokes, turn and crop (the cropped, screen-sized picture).
    geo_done: bool,
}

/// The thread that runs the engine for the preview. A job that is not the newest when the thread gets to it is dropped, so
/// dragging a slider never builds up a queue: what is shown is always the latest value, as fast as the engine can make it.
fn spawn_renderer(tx: mpsc::Sender<Msg>, wake: Arc<dyn Fn() + Send + Sync>) -> mpsc::Sender<RenderJob> {
    let (jtx, jrx) = mpsc::channel::<RenderJob>();
    std::thread::spawn(move || {
        let mut cache = engine::RetouchCache::default();
        while let Ok(mut job) = jrx.recv() {
            while let Ok(newer) = jrx.try_recv() {
                job = newer;
            }
            let t = Instant::now();
            let out = engine::render_proxy(&job.src, job.src_gen, &job.rec, job.geo_done, &mut cache);
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            if tx.send(Msg::Rendered { gen: job.gen, out, ms }).is_err() {
                break;
            }
            wake();
        }
    });
    jtx
}

pub fn spawn_decode(tx: &mpsc::Sender<Msg>, wake: &Arc<dyn Fn() + Send + Sync>, path: PathBuf) {
    let (tx, wake) = (tx.clone(), wake.clone());
    std::thread::spawn(move || {
        let result = flow::decode(&path);
        let _ = tx.send(Msg::Loaded { path, result });
        wake();
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Header(usize),
    Row(usize),
    /// An item of the open section (see `panel`).
    Item(usize),
    ResetAll,
    Save,
}

struct Layout {
    area: [f32; 4],
    panel: [f32; 4],
    headers: Vec<(usize, [f32; 4])>,
    rows: Vec<(usize, [f32; 4], [f32; 4])>,
    reset_all: [f32; 4],
    save: [f32; 4],
    special: Option<tools::Special>,
    items: Vec<Item>,
    /// Where the scrolling content of the panel shows: between the file name and the buttons.
    view: [f32; 4],
    /// Bottom of the content (window pixels, with the scroll applied).
    content_end: f32,
}

pub struct App {
    hwnd: WindowHandle,
    size: (u32, u32),
    dpi: f32,
    started: Instant,
    settings: Settings,
    opts: Options,
    wake: Arc<dyn Fn() + Send + Sync>,
    gpu_rx: mpsc::Receiver<Result<GpuDevice, String>>,
    renderer: Option<Renderer>,
    rx: mpsc::Receiver<Msg>,
    tx: mpsc::Sender<Msg>,
    text: Text,

    path: PathBuf,
    loading: bool,
    error: Option<String>,
    full: Option<Pixels>,
    reduced: bool,
    decode_ms: f64,
    /// The unedited picture at the size it is shown at.
    proxy: Option<Arc<Pixels>>,
    /// Counts the proxies built, so the worker can tell one from another.
    proxy_gen: u64,
    jobs: mpsc::Sender<RenderJob>,
    /// The newest request sent to the worker; an answer to an older one is dropped.
    render_gen: u64,
    /// The edited proxy, kept only until the GPU can take it (the CPU first frame).
    shown: Option<Pixels>,

    recipe: Recipe,
    /// What is already kept (the pair file, or nothing): a difference means unsaved edits.
    kept: Recipe,
    undo: Vec<Recipe>,
    redo: Vec<Recipe>,
    edit_ms: f32,
    show_original: bool,
    view: View,
    tools: Tools,
    /// The picture with turn, flip and crop applied at the size it is shown at (only kept while there is a crop, so a cropped
    /// picture is not a blown-up piece of the fitted proxy). Key: what produced it.
    geo: Option<(String, Arc<Pixels>)>,

    panel: bool,
    open_group: usize,
    /// How far the panel's content is scrolled up (pixels).
    panel_scroll: f32,
    /// `NUEDITOR_TEST` is set: the title also carries a line of internal state, so the e2e scripts can read it.
    test_hooks: bool,
    export_ui: panel::ExportUi,
    presets: Vec<nv_editor::preset::Preset>,
    /// The `Num` item being dragged (index into the layout's items).
    item_drag: Option<usize>,
    drag: Option<usize>,
    hot: Option<Hit>,
    last_wheel: Option<(usize, Instant)>,
    toast: Option<String>,
    first_frame_ms: Option<f64>,
}

fn contains(r: [f32; 4], x: f32, y: f32) -> bool {
    gfx::contains(r, x, y)
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hwnd: WindowHandle,
        size: (u32, u32),
        dpi: f32,
        wake: Arc<dyn Fn() + Send + Sync>,
        gpu_rx: mpsc::Receiver<Result<GpuDevice, String>>,
        tx: mpsc::Sender<Msg>,
        rx: mpsc::Receiver<Msg>,
        settings: Settings,
        opts: Options,
        started: Instant,
        path: PathBuf,
    ) -> App {
        set_title(hwnd, &format!("{} — NuEditor", flow::file_name(&path)));
        let jobs = spawn_renderer(tx.clone(), wake.clone());
        let export_ui = panel::ExportUi::from_settings(&settings);
        App {
            hwnd,
            size,
            dpi,
            started,
            settings,
            opts,
            wake,
            gpu_rx,
            renderer: None,
            rx,
            tx,
            text: Text::default(),
            path,
            loading: true,
            error: None,
            full: None,
            reduced: false,
            decode_ms: 0.0,
            proxy: None,
            proxy_gen: 0,
            jobs,
            render_gen: 0,
            shown: None,
            recipe: Recipe::default(),
            kept: Recipe::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            edit_ms: 0.0,
            show_original: false,
            view: View::default(),
            tools: Tools::default(),
            geo: None,
            panel: true,
            open_group: 1,
            panel_scroll: 0.0,
            test_hooks: std::env::var_os("NUEDITOR_TEST").is_some(),
            export_ui,
            presets: Vec::new(),
            item_drag: None,
            drag: None,
            hot: None,
            last_wheel: None,
            toast: None,
            first_frame_ms: None,
        }
    }

    // ------------------------------------------------------------------------------------------ events

    pub fn handle(&mut self, ev: Event) {
        match ev {
            Event::Resize { width, height } => {
                self.size = (width, height);
                if let Some(r) = &mut self.renderer {
                    let _ = r.resize(width, height);
                }
                if self.ensure_proxy() {
                    self.rerender();
                }
                if self.view.zoom.is_some() {
                    self.view.detail_dirty = true;
                }
                self.draw();
            }
            Event::Paint => self.draw(),
            Event::User => self.poll(),
            Event::Dpi(d) => {
                self.dpi = d;
                invalidate(self.hwnd);
            }
            Event::Timer(TIMER_TOAST) => {
                nv_win::kill_timer(self.hwnd, TIMER_TOAST);
                self.toast = None;
                invalidate(self.hwnd);
            }
            Event::Key(vk) => self.key(vk),
            Event::MouseDown { x, y, ctrl } => {
                if !self.tool_press(x, y, ctrl) && !self.press_image(x, y) {
                    self.press(x, y);
                }
            }
            Event::MouseMove { x, y, left_down } => {
                if let (Some(i), true) = (self.drag, left_down) {
                    self.set_from_x(i, x as f32);
                } else if let (Some(i), true) = (self.item_drag, left_down) {
                    self.item_value_from_x(i, x as f32);
                } else if left_down && (self.tool_drag(x, y) || self.drag_image(x, y)) {
                } else {
                    let hot = self.hit(x, y);
                    if hot != self.hot {
                        self.hot = hot;
                        invalidate(self.hwnd);
                    }
                    if let Mode::Heal(_) = self.tools.mode {
                        self.tools.cursor = contains(self.area(), x as f32, y as f32).then_some((x, y));
                        invalidate(self.hwnd);
                    }
                }
            }
            Event::MouseUp => {
                self.drag = None;
                self.item_drag = None;
                self.view.pan = None;
                self.tool_release();
            }
            Event::DoubleClick { x, y, .. } => {
                if self.double_click_image(x, y) {
                } else if let Some(Hit::Row(i)) = self.hit(x, y) {
                    self.reset_param(i);
                } else if let Some(Hit::Item(i)) = self.hit(x, y) {
                    self.item_right(i);
                }
            }
            Event::RightClick { x, y } => {
                if self.tool_right(x, y) {
                } else {
                    match self.hit(x, y) {
                        Some(Hit::Row(i)) => self.reset_param(i),
                        Some(Hit::Item(i)) => self.item_right(i),
                        Some(Hit::Header(g)) if g < Group::ALL.len() => self.reset_group(Group::ALL[g]),
                        Some(Hit::Header(g)) => self.reset_section(g),
                        _ => {}
                    }
                }
            }
            Event::Wheel { delta, x, y, .. } => {
                if !self.wheel_image(delta, x, y) {
                    self.wheel(delta, x, y);
                }
            }
            Event::Close => {
                if !self.request_exit() {
                    nv_win::veto_close();
                }
            }
            Event::Open { path: Some(p), .. } => self.open_path(PathBuf::from(p)),
            Event::Drop { paths, .. } => {
                if let Some(p) = paths.first() {
                    self.open_path(PathBuf::from(p));
                }
            }
            _ => {}
        }
    }

    /// Worker threads finished something: the GPU device, or the picture.
    fn poll(&mut self) {
        if self.renderer.is_none() {
            match self.gpu_rx.try_recv() {
                Ok(Ok(gpu)) => match Renderer::from_device(gpu, self.hwnd, self.size.0, self.size.1) {
                    Ok(r) => {
                        self.renderer = Some(r);
                        self.upload_all();
                    }
                    Err(e) => self.error = Some(format!("renderer failed: {e}")),
                },
                Ok(Err(e)) => self.error = Some(format!("GPU init failed: {e}")),
                Err(_) => {}
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Loaded { path, result } if path == self.path => self.on_loaded(result),
                Msg::Loaded { .. } => {} // a picture we have already moved on from
                Msg::Rendered { gen, out, ms } => {
                    if gen == self.render_gen {
                        self.finish_render(out, ms);
                    }
                }
            }
        }
        invalidate(self.hwnd);
    }

    fn on_loaded(&mut self, result: Result<flow::Decoded, String>) {
        self.loading = false;
        match result {
            Err(e) => {
                self.error = Some(e);
            }
            Ok(d) => {
                self.decode_ms = d.ms;
                self.reduced = d.reduced_from.is_some();
                let img = d.image;
                self.full = Some(Pixels::new(img.width, img.height, img.rgba));
                self.proxy = None;
                self.view = View::default();
                self.tools = Tools::default();
                self.geo = None;
                self.recipe = Recipe::default();
                self.kept = Recipe::default();
                match sidecar::load(&self.path) {
                    Ok(Some((r, src))) => {
                        let (w, h) = self.full.as_ref().map(|f| (f.w, f.h)).unwrap_or((0, 0));
                        if src.matches(&sidecar::Source { width: w, height: h, ..Default::default() }) {
                            self.recipe = r.clone();
                            self.kept = r;
                            self.say("Your saved edits (pair file) are applied");
                        } else {
                            self.say("The pair file is for a picture of another size - ignored");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.say(&format!("Can't read the pair file: {e}")),
                }
                if self.recipe == Recipe::default() && nv_editor::journal::last_for(&flow::backup_dir(), &self.path).is_some() {
                    self.say("This picture was burned in earlier: Ctrl+Alt+Z brings the original back");
                }
                if let Some(a) = self.opts.start_angle.take() {
                    self.recipe.set(P::Angle, a);
                }
                self.ensure_proxy();
                self.rerender();
                if self.renderer.is_none() {
                    self.paint_cpu();
                }
            }
        }
        self.update_title();
    }

    // ------------------------------------------------------------------------------------------ the picture

    fn area(&self) -> [f32; 4] {
        let w = self.size.0 as f32;
        let panel = if self.panel { 300.0 * self.dpi } else { 0.0 };
        [0.0, 0.0, (w - panel).max(1.0), self.size.1 as f32]
    }

    /// Make sure the proxy is about the size the picture is shown at. True if it was rebuilt.
    fn ensure_proxy(&mut self) -> bool {
        let Some(full) = &mut self.full else { return false };
        let a = {
            let w = self.size.0 as f32;
            let panel = if self.panel { 300.0 * self.dpi } else { 0.0 };
            ((w - panel).max(1.0), self.size.1 as f32)
        };
        // Long edge the proxy needs so the fitted picture is shown 1:1 (never more than the picture has).
        let (fw, fh) = (full.w as f32, full.h as f32);
        let s = (a.0 / fw).min(a.1 / fh).min(1.0);
        let need = ((fw.max(fh) * s).round() as u32).max(16);
        if let Some(p) = &self.proxy {
            let have = p.w.max(p.h);
            if have >= need && (have as f32) < need as f32 * 1.4 {
                return false;
            }
        }
        let img = Image::new(full.w, full.h, std::mem::take(&mut full.rgba));
        let small = nv_codecs::downscale(&img, need);
        full.rgba = img.rgba;
        self.proxy = Some(Arc::new(Pixels::new(small.width, small.height, small.rgba)));
        self.proxy_gen += 1;
        if let (Some(r), Some(p)) = (&mut self.renderer, &self.proxy) {
            let mut img = Image::new(p.w, p.h, p.rgba.clone());
            img.srgb_to_output();
            let _ = r.upload(ID_ORIG, &img);
        }
        true
    }

    /// The recipe on the proxy: this is what runs on every slider move.
    fn rerender(&mut self) {
        if self.view.zoom.is_some() {
            // Zoomed in: the detail cut-out is what is shown; the fitted proxy is brought up to date when we come back.
            self.view.detail_dirty = true;
            self.view.proxy_stale = true;
            invalidate(self.hwnd);
            return;
        }
        if self.proxy.is_none() {
            return;
        }
        let rec = self.display_recipe();
        if rec.crop.is_some() {
            self.ensure_geo(&rec);
        }
        let (src, geo_done) = match (&self.geo, &self.proxy) {
            (Some((_, g)), _) if rec.crop.is_some() => (g.clone(), true),
            (_, Some(p)) => (p.clone(), false),
            _ => return,
        };
        self.render_gen += 1;
        if self.renderer.is_none() {
            // The first frame, before the GPU exists: no hand-over to the worker, the picture is painted from here.
            let t = Instant::now();
            let mut cache = engine::RetouchCache::default();
            let out = engine::render_proxy(&src, self.proxy_gen, &rec, geo_done, &mut cache);
            self.finish_render(out, t.elapsed().as_secs_f32() * 1000.0);
        } else {
            let _ = self.jobs.send(RenderJob { gen: self.render_gen, src, src_gen: self.proxy_gen, rec, geo_done });
        }
    }

    /// A rendered picture is back (from the worker, or made here for the first frame): scopes, GPU, title.
    fn finish_render(&mut self, out: Pixels, ms: f32) {
        self.edit_ms = ms;
        self.update_scope(&out);
        let out = self.display_copy(out);
        if let Some(r) = &mut self.renderer {
            let mut img = Image::new(out.w, out.h, out.rgba);
            img.srgb_to_output();
            if let Err(e) = r.upload(ID_EDIT, &img) {
                self.error = Some(format!("upload: {e}"));
            }
        } else {
            self.shown = Some(out);
        }
        self.update_title();
    }

    /// Build the cropped / turned picture at screen size from the full picture when the crop, angle or flips changed.
    fn ensure_geo(&mut self, rec: &Recipe) {
        let key = format!("{:?}", (rec.get(P::Angle).to_bits(), rec.quarter_turns, rec.flip_h, rec.flip_v, rec.crop.map(|c| (c.x.to_bits(), c.y.to_bits(), c.w.to_bits(), c.h.to_bits())), &rec.retouch, self.size, self.panel));
        if matches!(&self.geo, Some((k, _)) if *k == key) {
            return;
        }
        let Some(full) = &self.full else { return };
        let retouched;
        let base = if rec.retouch.is_empty() {
            full
        } else {
            retouched = nv_editor::retouch::apply(full, &rec.retouch);
            &retouched
        };
        let g = engine::apply_geometry(base, rec);
        let a = self.area();
        let s = ((a[2] - a[0]) / g.w as f32).min((a[3] - a[1]) / g.h as f32).min(1.0);
        let edge = ((g.w.max(g.h) as f32 * s).round() as u32).max(16);
        let img = Image::new(g.w, g.h, g.rgba);
        let small = nv_codecs::downscale(&img, edge);
        self.geo = Some((key, Arc::new(Pixels::new(small.width, small.height, small.rgba))));
    }

    /// The GPU just came up: put what exists on it.
    fn upload_all(&mut self) {
        let Some(r) = &mut self.renderer else { return };
        if let Some(p) = &self.proxy {
            let mut img = Image::new(p.w, p.h, p.rgba.clone());
            img.srgb_to_output();
            let _ = r.upload(ID_ORIG, &img);
        }
        if let Some(out) = self.shown.take() {
            let mut img = Image::new(out.w, out.h, out.rgba);
            img.srgb_to_output();
            let _ = r.upload(ID_EDIT, &img);
        } else if self.proxy.is_some() {
            self.rerender();
        }
    }

    /// The first frame, before the GPU exists: the edited proxy through plain GDI.
    fn paint_cpu(&mut self) {
        let Some(p) = &self.shown else { return };
        let a = self.area();
        let (aw, ah) = (a[2] - a[0], a[3] - a[1]);
        let s = (aw / p.w as f32).min(ah / p.h as f32).min(1.0);
        if s < 1.0 {
            return; // only a picture that needs no scaling is painted this way; the GPU scales the rest
        }
        let x = (aw / 2.0 - p.w as f32 / 2.0).round() as i32;
        let y = (ah / 2.0 - p.h as f32 / 2.0).round() as i32;
        let mut bgra = Vec::with_capacity(p.rgba.len());
        for px in p.rgba.chunks_exact(4) {
            let al = px[3] as u32;
            let mix = |c: u8| ((c as u32 * al + 0x1A * (255 - al) + 127) / 255) as u8;
            bgra.extend_from_slice(&[mix(px[2]), mix(px[1]), mix(px[0]), 255]);
        }
        nv_win::paint_bgra(self.hwnd, x, y, p.w as i32, p.h as i32, &bgra, 0x1A1A1A);
        if self.first_frame_ms.is_none() {
            self.first_frame_ms = Some(self.started.elapsed().as_secs_f64() * 1000.0);
            self.update_title();
        }
    }

    fn update_title(&mut self) {
        let name = flow::file_name(&self.path);
        let mut t = format!("{}{name}", if self.dirty() { "● " } else { "" });
        if let Some(f) = &self.full {
            t.push_str(&format!(" — {}x{} — {}", f.w, f.h, self.zoom_label()));
        }
        if self.proxy.is_some() {
            t.push_str(&format!(" — edit {:.1} ms", self.edit_ms));
        }
        if let Some(ms) = self.first_frame_ms {
            t.push_str(&format!(" — first image {ms:.0} ms (decode {:.0})", self.decode_ms));
        }
        if self.test_hooks {
            t.push_str(&format!(
                " — [T strokes={} mode={:?} brush={:.1} presets={} lut={} export={:?}/{:.0}/{:?} scroll={:.0}]",
                self.recipe.retouch.len(),
                self.tools.mode,
                self.tools.brush,
                self.presets.len(),
                self.recipe.lut.is_some(),
                self.export_ui.format,
                self.export_ui.quality,
                self.export_ui.size_kind,
                self.panel_scroll
            ));
        }
        t.push_str(" — NuEditor");
        set_title(self.hwnd, &t);
    }

    fn dirty(&self) -> bool {
        self.recipe != self.kept
    }

    fn say(&mut self, s: &str) {
        self.toast = Some(s.to_string());
        nv_win::set_timer(self.hwnd, TIMER_TOAST, 3000);
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ editing

    fn snapshot(&mut self) {
        self.undo.push(self.recipe.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn changed(&mut self) {
        self.rerender();
        invalidate(self.hwnd);
    }

    fn set_param(&mut self, i: usize, v: f32) {
        let info = &PARAMS[i];
        let range = info.max - info.min;
        // Round to what the number shows, and let the default pull the knob in (so "back to zero" is easy to hit).
        let step = if range <= 10.0 { 0.01 } else { 1.0 };
        let mut v = (v / step).round() * step;
        if (v - info.default).abs() < range * 0.012 {
            v = info.default;
        }
        if (self.recipe.v[i] - v).abs() < 1e-6 {
            return;
        }
        self.recipe.set_index(i, v);
        self.changed();
    }

    fn set_from_x(&mut self, i: usize, x: f32) {
        let l = self.layout();
        let Some((_, _, track)) = l.rows.iter().find(|r| r.0 == i) else { return };
        let t = ((x - track[0]) / (track[2] - track[0]).max(1.0)).clamp(0.0, 1.0);
        let info = &PARAMS[i];
        self.set_param(i, info.min + (info.max - info.min) * t);
    }

    fn reset_param(&mut self, i: usize) {
        if !self.recipe.is_default(i) {
            self.snapshot();
            self.recipe.reset_param(i);
            self.changed();
        }
    }

    fn reset_group(&mut self, g: Group) {
        if self.recipe.group_changed(g) {
            self.snapshot();
            self.recipe.reset_group(g);
            self.changed();
        }
    }

    /// Right click on the Curves / Hue wheel header: put that section back.
    fn reset_section(&mut self, section: usize) {
        if section == panel::SECTION_EXPORT {
            self.export_ui = panel::ExportUi::from_settings(&self.settings);
            invalidate(self.hwnd);
            return;
        }
        if !self.section_changed(section) {
            return;
        }
        self.snapshot();
        if section == SECTION_CURVES {
            self.recipe.curves = Default::default();
        } else if section == SECTION_WHEEL {
            self.recipe.hue_wheel = Recipe::default().hue_wheel;
        }
        self.changed();
    }

    fn section_changed(&self, section: usize) -> bool {
        match section {
            SECTION_CURVES => !self.recipe.curves.is_identity(),
            SECTION_WHEEL => self.recipe.hue_wheel != Recipe::default().hue_wheel,
            panel::SECTION_EXPORT => self.export_ui != panel::ExportUi::from_settings(&self.settings),
            _ => false,
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.recipe, prev));
            self.changed();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.recipe, next));
            self.changed();
        }
    }

    /// Ctrl+Alt+Z: put back the original of a picture that was burned in (the journal knows where it was kept).
    fn restore_original(&mut self) {
        let dir = flow::backup_dir();
        let Some(e) = nv_editor::journal::last_for(&dir, &self.path) else {
            self.say("There is no burned-in original to bring back for this picture");
            return;
        };
        if !nv_win::shell::confirm(self.hwnd, &format!("Replace {} with the original from before it was burned in?\n\nYour current edits in the editor are dropped.", flow::file_name(&self.path))) {
            return;
        }
        match nv_editor::journal::restore(&dir, &e) {
            Ok(()) => {
                let path = self.path.clone();
                self.begin_load(path);
                self.say("The original is back");
            }
            Err(msg) => flow::tell(self.hwnd, &format!("Couldn't restore the original: {msg}")),
        }
    }

    fn turn(&mut self, degrees: f32) {
        self.snapshot();
        let a = nv_editor::raster::normalize_degrees(self.recipe.get(P::Angle) + degrees);
        self.recipe.set(P::Angle, if a > 180.0 { a - 360.0 } else { a });
        self.changed();
    }

    fn key(&mut self, vk: u32) {
        let m = nv_win::modifiers();
        if !m.ctrl && (self.tool_key(vk) || self.view_key(vk)) {
            return;
        }
        match vk {
            VK_ESCAPE => {
                if self.request_exit() {
                    nv_win::quit();
                }
            }
            VK_TAB => {
                self.panel = !self.panel;
                if self.ensure_proxy() {
                    self.rerender();
                }
                if self.view.zoom.is_some() {
                    self.view.detail_dirty = true;
                }
                invalidate(self.hwnd);
            }
            0x5A if m.ctrl && m.alt => self.restore_original(),
            0x5A if m.ctrl && m.shift => self.redo(),
            0x5A if m.ctrl => self.undo(),
            0x59 if m.ctrl => self.redo(),
            0x53 if m.ctrl && m.shift => self.save_copy_dialog(),
            0x53 if m.ctrl => self.save_now(),
            0x43 if !m.ctrl => {
                self.show_original = !self.show_original;
                invalidate(self.hwnd);
            }
            0x30 if !m.ctrl => {
                if !self.recipe.is_identity() {
                    self.snapshot();
                    self.recipe = Recipe::default();
                    self.changed();
                }
            }
            VK_COMMA => self.turn(-45.0),
            VK_PERIOD => self.turn(45.0),
            0x49 if !m.ctrl => {
                self.snapshot();
                self.recipe.invert = !self.recipe.invert;
                self.changed();
            }
            0x48 => {
                self.snapshot();
                self.recipe.flip_h = !self.recipe.flip_h;
                self.changed();
            }
            0x56 => {
                self.snapshot();
                self.recipe.flip_v = !self.recipe.flip_v;
                self.changed();
            }
            _ => {}
        }
    }

    fn wheel(&mut self, delta: i32, x: i32, y: i32) {
        let i = match self.hit(x, y) {
            Some(Hit::Row(i)) => i,
            Some(Hit::Item(i)) => {
                if !self.item_wheel(i, delta) {
                    self.scroll_panel(-(delta as f32) / 120.0 * 40.0 * self.dpi);
                }
                return;
            }
            _ => {
                // Anywhere else on the panel: scroll it.
                if self.panel && x as f32 >= self.layout().panel[0] {
                    self.scroll_panel(-(delta as f32) / 120.0 * 40.0 * self.dpi);
                }
                return;
            }
        };
        if !engine::is_implemented(i) {
            return;
        }
        let now = Instant::now();
        let continuing = matches!(self.last_wheel, Some((j, t)) if j == i && now.duration_since(t) < Duration::from_millis(700));
        if !continuing {
            self.snapshot();
        }
        self.last_wheel = Some((i, now));
        let info = &PARAMS[i];
        let step = (info.max - info.min) / 100.0 * if delta > 0 { 1.0 } else { -1.0 };
        self.set_param(i, self.recipe.v[i] + step);
    }

    // ------------------------------------------------------------------------------------------ leaving and saving

    /// How to keep the edits: ask (the setting's way is the default button), or just use the setting's way.
    fn choose_save(&self) -> Choice {
        if self.settings.editor_ask {
            flow::ask(&flow::file_name(&self.path), self.can_burn(), self.settings.editor_save)
        } else {
            flow::default_choice(self.settings.editor_save, self.can_burn())
        }
    }

    fn can_burn(&self) -> bool {
        !self.reduced
    }

    /// Carry out a way of saving. Ok(path to show) or an error already reported to the user.
    fn save_as(&mut self, choice: Choice) -> Result<Option<PathBuf>, ()> {
        let Some(full) = &self.full else { return Ok(None) };
        let f = Full { path: &self.path, pixels: full };
        let saved = match choice {
            Choice::Burn => flow::burn_in(&f, &self.recipe, &flow::CopyOptions::from_settings(&self.settings)),
            Choice::NewFile => flow::new_file(&f, &self.recipe, &flow::CopyOptions::from_settings(&self.settings)),
            Choice::Pair => flow::pair_file(&f, &self.recipe),
            Choice::Discard | Choice::Cancel => return Ok(None),
        };
        match saved {
            Ok(s) => {
                self.say(&s.message);
                Ok(Some(s.show))
            }
            Err(e) => {
                flow::tell(self.hwnd, &format!("Couldn't save: {e}"));
                Err(())
            }
        }
    }

    /// Ctrl+S: choose how, save, keep editing.
    fn save_now(&mut self) {
        if self.full.is_none() || self.recipe.is_identity() && !self.dirty() {
            self.say("Nothing to save");
            return;
        }
        let choice = self.choose_save();
        match self.save_as(choice) {
            Ok(Some(shown)) => match choice {
                Choice::Burn => {
                    // The picture now holds the edit: start from it again, with nothing left to apply.
                    self.begin_load(shown);
                }
                Choice::Pair => self.kept = self.recipe.clone(),
                Choice::NewFile => self.kept = self.recipe.clone(),
                _ => {}
            },
            _ => {}
        }
        self.update_title();
    }

    /// Esc / close box. True if the window may close now (edits were kept, discarded or there were none).
    fn request_exit(&mut self) -> bool {
        let mut show = self.path.clone();
        if self.dirty() && self.full.is_some() {
            let choice = self.choose_save();
            match choice {
                Choice::Cancel => return false,
                Choice::Discard => {}
                c => match self.save_as(c) {
                    Ok(Some(s)) => {
                        if c == Choice::Burn || c == Choice::NewFile {
                            show = s;
                        }
                    }
                    Ok(None) => {}
                    Err(()) => return false,
                },
            }
        }
        flow::hand_back(self.opts.origin, &show);
        true
    }

    /// Another picture was asked for (a second launch, a drop). Edits to the current one are settled first.
    fn open_path(&mut self, path: PathBuf) {
        if path == self.path {
            nv_win::instance::bring_to_front(self.hwnd);
            return;
        }
        if self.dirty() && self.full.is_some() {
            let choice = self.choose_save();
            match choice {
                Choice::Cancel => return,
                Choice::Discard => {}
                c => {
                    if self.save_as(c).is_err() {
                        return;
                    }
                }
            }
        }
        self.begin_load(path);
    }

    fn begin_load(&mut self, path: PathBuf) {
        self.path = path.clone();
        self.loading = true;
        self.error = None;
        self.full = None;
        self.proxy = None;
        self.shown = None;
        self.view = View::default();
        self.tools = Tools::default();
        self.geo = None;
        self.undo.clear();
        self.redo.clear();
        self.recipe = Recipe::default();
        self.kept = Recipe::default();
        spawn_decode(&self.tx, &self.wake, path);
        self.update_title();
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ the panel

    fn layout(&self) -> Layout {
        let d = self.dpi;
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let area = self.area();
        let panel = if self.panel { [w - 300.0 * d, 0.0, w, h] } else { [w, 0.0, w, h] };
        let (pad, head_h, row_h) = (14.0 * d, 30.0 * d, 44.0 * d);
        let by = h - 48.0 * d;
        let mut y = 56.0 * d - self.panel_scroll;
        let mut headers = Vec::new();
        let mut rows = Vec::new();
        let mut items = Vec::new();
        let mut special = None;
        if self.panel {
            for gi in 0..Group::ALL.len() + SECTION_NAMES.len() {
                headers.push((gi, [panel[0], y, panel[2], y + head_h]));
                y += head_h;
                if gi == self.open_group && (gi == SECTION_CURVES || gi == SECTION_WHEEL) {
                    let (sp, ny) = self.special_layout(panel, y, gi);
                    special = Some(sp);
                    y = ny;
                } else if gi == self.open_group && gi >= Group::ALL.len() {
                    self.place_items(&self.section_whats(gi), panel, &mut y, &mut items);
                    y += 6.0 * d;
                } else if gi == self.open_group {
                    self.place_items(&self.lead_whats(gi), panel, &mut y, &mut items);
                    for i in recipe::params_of(Group::ALL[gi]) {
                        let row = [panel[0], y, panel[2], y + row_h];
                        let track = [panel[0] + pad, y + 28.0 * d, panel[2] - pad, y + 32.0 * d];
                        rows.push((i, row, track));
                        y += row_h;
                    }
                    self.place_items(&self.tail_whats(gi), panel, &mut y, &mut items);
                    y += 6.0 * d;
                }
            }
        }
        let content_end = y;
        let view = [panel[0], 52.0 * d, panel[2], by - 4.0 * d];
        let half = (panel[2] - panel[0] - 3.0 * pad) / 2.0;
        let reset_all = [panel[0] + pad, by, panel[0] + pad + half, by + 34.0 * d];
        let save = [reset_all[2] + pad, by, reset_all[2] + pad + half, by + 34.0 * d];
        Layout { area, panel, headers, rows, reset_all, save, special, items, view, content_end }
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        if !self.panel {
            return None;
        }
        let l = self.layout();
        let (fx, fy) = (x as f32, y as f32);
        if contains(l.reset_all, fx, fy) {
            return Some(Hit::ResetAll);
        }
        if contains(l.save, fx, fy) {
            return Some(Hit::Save);
        }
        if !contains(l.view, fx, fy) {
            return None;
        }
        if let Some((g, _)) = l.headers.iter().find(|(_, r)| contains(*r, fx, fy)) {
            return Some(Hit::Header(*g));
        }
        if let Some(i) = l.items.iter().position(|it| contains(it.rect, fx, fy)) {
            return Some(Hit::Item(i));
        }
        l.rows.iter().find(|(_, r, _)| contains(*r, fx, fy)).map(|(i, _, _)| Hit::Row(*i))
    }

    /// Scroll the panel so a long section can be reached; keeps the content inside its limits.
    fn scroll_panel(&mut self, by: f32) {
        let l = self.layout();
        let total = l.content_end + self.panel_scroll - (56.0 * self.dpi);
        let room = (l.view[3] - (56.0 * self.dpi)).max(1.0);
        let max = (total - room).max(0.0);
        let s = (self.panel_scroll + by).clamp(0.0, max);
        if s != self.panel_scroll {
            self.panel_scroll = s;
            self.update_title();
            invalidate(self.hwnd);
        }
    }

    /// After a section opened: bring it into view (its header stays visible).
    fn reveal_section(&mut self) {
        self.panel_scroll = 0.0;
        let l = self.layout();
        let Some((_, head)) = l.headers.iter().find(|(g, _)| *g == self.open_group) else { return };
        let overflow = l.content_end - l.view[3];
        if overflow > 0.0 {
            let max_by_header = (head[1] - l.view[1]).max(0.0);
            self.scroll_panel(overflow.min(max_by_header));
        }
    }

    fn press(&mut self, x: i32, y: i32) {
        match self.hit(x, y) {
            Some(Hit::Header(g)) => {
                self.open_group = if self.open_group == g { usize::MAX } else { g };
                if self.open_group == SECTION_PRESETS {
                    self.reload_presets();
                }
                self.reveal_section();
                invalidate(self.hwnd);
            }
            Some(Hit::Item(i)) => {
                self.item_drag = self.item_press(i, x as f32, y as f32);
            }
            Some(Hit::Row(i)) if engine::is_implemented(i) => {
                self.snapshot();
                self.drag = Some(i);
                self.set_from_x(i, x as f32);
            }
            Some(Hit::ResetAll) => {
                if !self.recipe.is_identity() {
                    self.snapshot();
                    self.recipe = Recipe::default();
                    self.changed();
                }
            }
            Some(Hit::Save) => self.save_now(),
            _ => {}
        }
    }

    fn value_text(&self, i: usize) -> String {
        let v = self.recipe.v[i];
        let range = PARAMS[i].max - PARAMS[i].min;
        let s = if range <= 10.0 { format!("{v:.2}") } else { format!("{v:.0}") };
        if v > 0.0 && PARAMS[i].min < 0.0 { format!("+{s}") } else { s }
    }

    // ------------------------------------------------------------------------------------------ drawing

    fn draw(&mut self) {
        if self.renderer.is_none() {
            if self.shown.is_some() {
                self.paint_cpu();
            }
            return;
        }
        if let Some(z) = self.view.zoom {
            if z <= self.fit_zoom() * 1.001 {
                self.leave_detail(); // the window grew until the picture fits
            }
        }
        if self.view.zoom.is_some() && self.view.detail_dirty {
            self.render_detail();
        }
        let l = self.layout();
        let d = self.dpi;
        let id = if self.show_original { ID_ORIG } else { ID_EDIT };
        let (full_dims, angle, mode) = (self.full.as_ref().map(|f| (f.w, f.h)).unwrap_or((1, 1)), self.recipe.get(P::Angle), self.tools.mode);
        let (zoom, region, centre, split, split_at, scope, clipped) = (self.view.zoom, self.view.region, self.view.center, self.view.split, self.view.split_at, self.view.scope, self.view.clipped);
        let px = |n: f32| (n * d).round() as i32;
        let (text_c, dim_c, accent) = (rgb(pal::TEXT, 1.0), rgb(pal::DIM, 1.0), rgb(pal::ACCENT, 1.0));
        // Everything that needs `&self` is worked out before the renderer is borrowed.
        let status = if self.dirty() { format!("edited · {:.1} ms per change", self.edit_ms) } else { format!("unchanged · {:.1} ms per change", self.edit_ms) };
        let values: Vec<String> = l.rows.iter().map(|(i, _, _)| self.value_text(*i)).collect();
        let sections_changed = [self.section_changed(SECTION_CURVES), self.section_changed(SECTION_WHEEL), self.section_changed(SECTION_PRESETS), self.section_changed(panel::SECTION_EXPORT)];
        let item_list = self.item_draws(&l);
        let hot_item = match self.hot {
            Some(Hit::Item(i)) => Some(i),
            _ => None,
        };
        let overlay = self.heal_overlay();
        let Some(r) = self.renderer.as_mut() else { return };
        if !r.begin(rgb(pal::BG, 1.0)) {
            return;
        }
        self.text.begin_frame();

        // The picture: fitted (the proxy, centred, never enlarged) or zoomed in (the detail cut-out placed by the view), with
        // the original on the left of the divider when the A / B split is on.
        let (aw, ah) = (l.area[2] - l.area[0], l.area[3] - l.area[1]);
        let drew = if let (Some(z), Some((reg, s))) = (zoom, region) {
            let t = z / s;
            let (vx0, vy0) = (centre.0 * s - aw / (2.0 * t), centre.1 * s - ah / (2.0 * t));
            let dst = [(reg[0] - vx0) * t, (reg[1] - vy0) * t, (reg[2] - vx0) * t, (reg[3] - vy0) * t];
            r.set_clip(Some(l.area));
            let _ = r.draw_image(ID_DETAIL, dst, z >= 2.0);
            if split {
                r.set_clip(Some([l.area[0], l.area[1], l.area[0] + aw * split_at, l.area[3]]));
                let _ = r.draw_image(ID_DETAIL_ORIG, dst, z >= 2.0);
            }
            r.set_clip(None);
            true
        } else if let Some((iw, ih)) = r.image_size_of(if split { ID_EDIT } else { id }) {
            let s = (aw / iw as f32).min(ah / ih as f32).min(1.0);
            let (w, h) = (iw as f32 * s, ih as f32 * s);
            let (x, y) = ((aw / 2.0 - w / 2.0).round(), (ah / 2.0 - h / 2.0).round());
            let _ = r.draw_image(if split { ID_EDIT } else { id }, [x, y, x + w, y + h], false);
            self.tools.pic_rect = [x, y, x + w, y + h];
            if split {
                if let Some((ow, oh)) = r.image_size_of(ID_ORIG) {
                    let s = (aw / ow as f32).min(ah / oh as f32).min(1.0);
                    let (w, h) = (ow as f32 * s, oh as f32 * s);
                    let (x, y) = ((aw / 2.0 - w / 2.0).round(), (ah / 2.0 - h / 2.0).round());
                    r.set_clip(Some([l.area[0], l.area[1], l.area[0] + aw * split_at, l.area[3]]));
                    let _ = r.draw_image(ID_ORIG, [x, y, x + w, y + h], false);
                    r.set_clip(None);
                }
            }
            true
        } else {
            false
        };
        if drew && mode == Mode::Crop {
            tools::draw_crop(r, &mut self.text, &self.tools, full_dims, angle, d);
        }
        if let (true, Some(o)) = (drew, &overlay) {
            r.set_clip(Some(l.area));
            heal::draw_heal(r, o, d);
            r.set_clip(None);
        }
        if drew && split {
            let x = (l.area[0] + aw * split_at).round();
            let _ = r.fill_rect([x - 1.0, l.area[1], x + 1.0, l.area[3]], rgb(0xFFFFFF, 0.9));
            for (label, lx) in [("A  original", x - 100.0 * d), ("B  edited", x + 8.0 * d)] {
                let _ = r.fill_rect([lx, 12.0 * d, lx + 92.0 * d, 34.0 * d], rgb(0x000000, 0.6));
                self.text.draw(r, label, px(12.0), lx + 8.0 * d, 17.0 * d, text_c);
            }
        }
        if drew && scope != view::Scope::Off {
            if let Some((sw, sh)) = r.image_size_of(ID_SCOPE) {
                let (x, y) = (12.0 * d, l.area[3] - sh as f32 - 12.0 * d);
                let _ = r.draw_image(ID_SCOPE, [x, y, x + sw as f32, y + sh as f32], true);
                if scope == view::Scope::Histogram {
                    let msg = format!("clipped: shadows {:.1}%  highlights {:.1}%", clipped.0 * 100.0, clipped.1 * 100.0);
                    self.text.draw(r, &msg, px(11.0), x, y - 16.0 * d, dim_c);
                }
            }
        }
        if !drew && self.loading {
            self.text.draw_centered(r, "Loading…", px(15.0), l.area[2] / 2.0, l.area[3] / 2.0, dim_c);
        }
        if let Some(e) = &self.error {
            let msg = self.text.fit(e, px(13.0), (l.area[2] - 40.0 * d) as i32);
            self.text.draw_centered(r, &msg, px(13.0), l.area[2] / 2.0, l.area[3] / 2.0, rgb(pal::ERROR, 1.0));
        }
        if self.show_original {
            let _ = r.fill_rect([12.0 * d, 12.0 * d, 112.0 * d, 36.0 * d], rgb(0x000000, 0.6));
            self.text.draw(r, "ORIGINAL", px(13.0), 22.0 * d, 17.0 * d, text_c);
        }
        if let Some(t) = &self.toast {
            let w = self.text.width(t, px(13.0)) as f32 + 28.0 * d;
            let x = l.area[2] / 2.0 - w / 2.0;
            let y = l.area[3] - 64.0 * d;
            let _ = r.fill_rect([x, y, x + w, y + 32.0 * d], rgb(0x000000, 0.75));
            self.text.draw(r, t, px(13.0), x + 14.0 * d, y + 7.0 * d, text_c);
        }

        if self.panel {
            let _ = r.fill_rect(l.panel, rgb(pal::PANEL, 1.0));
            let _ = r.fill_rect([l.panel[0], 0.0, l.panel[0] + 1.0, l.panel[3]], rgb(pal::PANEL_EDGE, 1.0));
            let name = self.text.fit(&flow::file_name(&self.path), px(14.0), (270.0 * d) as i32);
            self.text.draw(r, &name, px(14.0), l.panel[0] + 14.0 * d, 10.0 * d, text_c);
            self.text.draw(r, &status, px(11.0), l.panel[0] + 14.0 * d, 32.0 * d, dim_c);

            r.set_clip(Some(l.view));
            for (gi, rect) in &l.headers {
                if self.hot == Some(Hit::Header(*gi)) {
                    let _ = r.fill_rect(*rect, rgb(pal::TILE_HOVER, 1.0));
                }
                let open = *gi == self.open_group;
                let arrow = if open { "▾" } else { "▸" };
                self.text.draw(r, arrow, px(12.0), rect[0] + 14.0 * d, rect[1] + 7.0 * d, dim_c);
                let (label, changed) = if *gi < Group::ALL.len() { (Group::ALL[*gi].label(), self.recipe.group_changed(Group::ALL[*gi])) } else { (SECTION_NAMES[*gi - Group::ALL.len()], sections_changed[*gi - Group::ALL.len()]) };
                self.text.draw(r, label, px(13.0), rect[0] + 34.0 * d, rect[1] + 6.0 * d, text_c);
                if changed {
                    let _ = r.fill_rect([rect[2] - 26.0 * d, rect[1] + 11.0 * d, rect[2] - 18.0 * d, rect[1] + 19.0 * d], accent);
                }
            }
            if let Some(sp) = &l.special {
                if self.open_group == SECTION_CURVES {
                    tools::draw_curves(r, &mut self.text, &self.tools, &self.recipe, sp, None, d);
                } else if self.open_group == SECTION_WHEEL {
                    let side = (sp.area[2] - sp.area[0]) as u32;
                    if r.image_size_of(ID_WHEEL) != Some((side, side)) {
                        let _ = r.upload(ID_WHEEL, &tools::wheel_image(side));
                    }
                    tools::draw_wheel(r, &mut self.text, &self.tools, &self.recipe, sp, d);
                }
            }
            for (n, (i, row, track)) in l.rows.iter().enumerate() {
                let info = &PARAMS[*i];
                let live = engine::is_implemented(*i);
                let (label_c, value_c) = if live { (text_c, if self.recipe.is_default(*i) { dim_c } else { accent }) } else { (rgb(0x6B7078, 1.0), rgb(0x6B7078, 1.0)) };
                let label = if live { info.label.to_string() } else { format!("{}  (soon)", info.label) };
                self.text.draw(r, &label, px(12.0), row[0] + 14.0 * d, row[1] + 5.0 * d, label_c);
                self.text.draw_right(r, &values[n], px(12.0), row[2] - 14.0 * d, row[1] + 5.0 * d, value_c);
                let _ = r.fill_rect(*track, rgb(0x2E3238, 1.0));
                let t = (self.recipe.v[*i] - info.min) / (info.max - info.min);
                let dt = (info.default - info.min) / (info.max - info.min);
                let (a, b) = (track[0] + (track[2] - track[0]) * dt.min(t), track[0] + (track[2] - track[0]) * dt.max(t));
                if live {
                    let _ = r.fill_rect([a, track[1], b, track[3]], accent);
                }
                let kx = track[0] + (track[2] - track[0]) * t;
                let knob = if live { rgb(0xE8E8E8, 1.0) } else { rgb(0x5A6068, 1.0) };
                let _ = r.fill_rect([kx - 5.0 * d, track[1] - 6.0 * d, kx + 5.0 * d, track[3] + 6.0 * d], knob);
            }
            panel::draw_item_list(r, &mut self.text, &item_list, hot_item, d);
            r.set_clip(None);
            for (rect, label, hit) in [(l.reset_all, "Reset all", Hit::ResetAll), (l.save, "Save…", Hit::Save)] {
                let fill = if self.hot == Some(hit) { rgb(pal::TILE_HOVER, 1.0) } else { rgb(pal::TILE, 1.0) };
                let _ = r.fill_rect(rect, fill);
                self.text.draw_centered(r, label, px(13.0), (rect[0] + rect[2]) / 2.0, rect[1] + 8.0 * d, text_c);
            }
            self.text.draw(r, "Tab panel · C original · S split · Y scope · X false colour", px(10.0), l.panel[0] + 14.0 * d, l.save[3] + 6.0 * d, dim_c);
            self.text.draw(r, "R crop · W white picker · wheel / 1 2 4 / F zoom · , . turn · H V flip · I invert", px(10.0), l.panel[0] + 14.0 * d, l.save[3] + 22.0 * d, dim_c);
        }
        self.text.end_frame(r);
        let _ = r.end();
        if self.first_frame_ms.is_none() && self.full.is_some() {
            self.first_frame_ms = Some(self.started.elapsed().as_secs_f64() * 1000.0);
            self.update_title();
        }
    }
}

#[allow(dead_code)]
fn _unused(_: &Path) {
    let _ = PARAM_COUNT;
}
