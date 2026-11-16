//! The gallery program: its state, how it starts, and how it reacts to everything that happens (events, finished
//! background work). Drawing is in `draw.rs`, the menus in `menus.rs`, file operations in `ops.rs`, the batch dialogs in
//! `batch_ui.rs`; they are all `impl App` blocks.

use crate::chrome::{self, Btn, Chrome, Panels};
use crate::gfx::Text;
use crate::grid::Grid;
use crate::info::{self, Histogram, InfoData};
use crate::input::{Clip, Outcome, TextInput};
use crate::model::{self, Entry, Listing, Order, Selection, Show};
use crate::state::{Persist, ViewMode, CELL_DEFAULT, CELL_LEVELS, CELL_MAX, CELL_MIN};
use crate::tree::Tree;
use nv_core::folder::SortKey;
use nv_core::settings::Settings;
use nv_core::Loader;
use nv_render::Renderer;
use nv_win::{invalidate, kill_timer, set_timer, set_title, Event, WindowHandle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub const TIMER_EDGE: u32 = 1;
pub const TIMER_TOAST: u32 = 2;
pub const TIMER_INFO: u32 = 3;
pub const TIMER_TASKBAR: u32 = 4;
pub const TIMER_CURSOR: u32 = 5;

/// Pointer must rest on an edge this long before the panel there slides out, and be away this long before it goes.
const EDGE_DWELL: Duration = Duration::from_millis(0);
const EDGE_LEAVE: Duration = Duration::from_millis(60);
/// The right panel loads its details this long after the focus last moved (so arrowing through a folder stays fast).
const INFO_DEBOUNCE_MS: u32 = 130;
const TOAST_MS: u32 = 3500;
/// Rows of thumbnails requested / kept beyond the visible ones.
const PREFETCH_ROWS: usize = 2;
const KEEP_ROWS: usize = 5;
/// Time budget for uploading thumbnails per wake-up.
const UPLOAD_SLICE: Duration = Duration::from_millis(8);

/// What background threads tell the UI thread.
pub enum Msg {
    Scanned { gen: u64, dir: PathBuf, result: Result<Vec<Entry>, String>, select: Vec<PathBuf>, keep_view: bool },
    Info(InfoData),
    FolderSize { gen: u64, bytes: u64, files: u64, folders: u64, done: bool },
    JobProgress(String),
    JobDone(JobResult),
    /// An archive was unpacked (or the password was wrong): see `tools.rs`.
    Archive(Box<crate::tools::ArchiveDone>),
    /// The volume label of a drive, read on a worker thread (a sleeping disk can take a while to answer).
    DriveLabel { root: PathBuf, label: String },
    /// The folder tree's first rows (Quick access, the drives), built off the UI thread.
    TreeReady(Tree),
    /// A frame of the preview's animation or image sequence (decoded off the UI thread).
    AnimFrame { id: u64, img: nv_codecs::Image, delay: u32, total: usize, label: String },
    /// The geometry of the focused 3D model.
    Mesh { id: u64, mesh: Arc<nv_codecs::model::Mesh> },
    /// The sharper decode of the focused picture, made after the preview was zoomed.
    BigPreview { id: u64, img: nv_codecs::Image },
}

pub struct JobResult {
    pub message: String,
    pub refresh: bool,
    /// Files to select after the refresh (new copies, renamed files).
    pub select: Vec<PathBuf>,
}

pub struct Job {
    pub cancel: Arc<AtomicBool>,
    pub label: String,
}

/// Which text box has the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum Focus {
    Path,
    Search,
    Rename(PathBuf),
    NewFolder(PathBuf),
    Meta(nv_win::props::Field),
}

pub struct Active {
    pub focus: Focus,
    pub ti: TextInput,
    pub error: Option<String>,
}

/// A rubber-band selection in progress.
pub struct Rubber {
    pub start: (f32, f32),
    pub now: (f32, f32),
    /// Scroll offset when it started (the band is anchored to the content, not the window).
    pub scroll0: f32,
    pub additive: bool,
    pub base: HashSet<u64>,
}

#[derive(Default)]
pub struct Timing {
    pub started: Option<Instant>,
    pub window_ms: Option<f64>,
    pub listing_ms: Option<f64>,
    pub gpu_ms: Option<f64>,
    pub first_thumb_ms: Option<f64>,
    /// Every visible thumbnail on screen after the last folder was opened, and how many that was.
    pub visible_ready_ms: Option<f64>,
    pub visible_count: usize,
    pub first_frame_ms: Option<f64>,
    /// The first picture on screen of any kind (painted on the CPU while Direct3D starts).
    pub cpu_frame_ms: Option<f64>,
    /// When the folder being waited for was requested (for navigations after start-up).
    pub nav_start: Option<Instant>,
    pub reported: bool,
}

#[derive(Default, Clone)]
pub struct FolderSize {
    pub gen: u64,
    pub bytes: u64,
    pub files: u64,
    pub folders: u64,
    pub done: bool,
}

pub struct Options {
    pub windowed: bool,
    pub bench_out: Option<PathBuf>,
    pub bench_exit: bool,
}

pub struct App {
    pub hwnd: WindowHandle,
    pub renderer: Option<Renderer>,
    pub gpu_rx: mpsc::Receiver<Result<nv_render::GpuDevice, String>>,
    pub text: Text,
    pub wake: Arc<dyn Fn() + Send + Sync>,
    pub size: (u32, u32),
    pub dpi: f32,
    pub persist: Persist,
    pub settings: Settings,
    pub opts: Options,

    pub listing: Listing,
    pub sel: Selection,
    pub mode: ViewMode,
    pub scroll: f32,
    /// A folder is being read (shown in the title).
    pub loading: Option<PathBuf>,
    pub scan_gen: u64,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,

    pub panels: Panels,
    pub chrome: Chrome,
    pub edge: Option<(u8, Instant)>,
    pub left_since: Option<Instant>,
    pub tree: Tree,
    pub tree_for: PathBuf,
    /// Drive labels that have arrived (kept for a tree that is not there yet).
    pub drive_labels: HashMap<PathBuf, String>,
    pub right_scroll: f32,

    pub tx: mpsc::Sender<Msg>,
    pub rx: mpsc::Receiver<Msg>,
    pub thumbs: Loader,
    /// Thumbnails decoded before the GPU was ready.
    pub staged: Vec<(u64, nv_codecs::Image)>,
    pub thumb_failed: HashSet<u64>,
    pub thumb_keep: HashSet<u64>,
    pub previews: Loader,
    pub preview_id: Option<u64>,
    pub preview_wanted: Option<(u64, PathBuf)>,
    pub hist: HashMap<u64, Histogram>,
    pub info: Option<InfoData>,
    pub info_for: Option<u64>,
    pub folder_size: FolderSize,

    pub active: Option<Active>,
    pub toast: Option<(String, Instant)>,
    pub help: bool,
    /// The links among the credit lines on screen (rectangle, address).
    pub help_links: Vec<([f32; 4], String)>,
    pub rubber: Option<Rubber>,
    pub last_mouse: (i32, i32),
    pub job: Option<Job>,
    /// The archive shown as a folder, if one is (see `tools.rs`).
    pub archive: Option<crate::tools::ArchiveView>,
    /// The result of an unpacking, handled once its job has been cleared (a retry needs a free job slot).
    pub archive_pending: Option<Box<crate::tools::ArchiveDone>>,
    pub job_text: String,
    pub timing: Timing,
    pub shuffle_seed: Option<u64>,
    pub rng: u64,
    pub taskbar_shown: bool,
    pub edge_ticks: u8,
    pub cut: Vec<PathBuf>,
    /// The last folder picked for Copy to / Move to.
    pub last_dest: Option<PathBuf>,
    /// A "refresh" request carries the files to select once it has run.
    pub pending_select: Vec<PathBuf>,
    /// After a delete: where the focus should land in the re-read folder.
    pub cursor_hint: Option<usize>,
    /// The CPU-painted first frames (see `cpu.rs`); gone once the GPU is drawing.
    pub cpu: Option<crate::cpu::Canvas>,
    /// NuViewer was started from here: closing the gallery closes it too.
    pub viewer_started: bool,
    /// NuEditor was started from here: closing the gallery closes it too (it asks about unsaved edits first).
    pub editor_started: bool,
    /// Type-ahead: the letters typed so far and when the last one came.
    pub typeahead: String,
    pub typeahead_at: Option<Instant>,
    /// Dragging the scroll bar: where inside the thumb it was grabbed.
    pub sb_drag: Option<f32>,
    /// The left button went down on a selected item here: moving on from it starts dragging the files out.
    pub drag_from: Option<(i32, i32)>,
    /// Dragging the folder tree's scroll bar: where inside the thumb it was grabbed.
    pub tree_sb_drag: Option<f32>,
    /// File / Edit / View / Tools / Help along the top edge (Alt or the pointer on the edge shows it).
    pub menubar: nv_win::menubar::MenuBar,
    /// What Ctrl+Z can take back, newest last.
    pub undo: Vec<crate::undo::Step>,
    /// The preview's zoom / pan, animation and 3D view (see `preview.rs`).
    pub pv: crate::preview::Preview,
}

pub fn worker_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().saturating_sub(1)).unwrap_or(2).clamp(2, 6)
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hwnd: WindowHandle,
        size: (u32, u32),
        dpi: f32,
        wake: Arc<dyn Fn() + Send + Sync>,
        gpu_rx: mpsc::Receiver<Result<nv_render::GpuDevice, String>>,
        tx: mpsc::Sender<Msg>,
        rx: mpsc::Receiver<Msg>,
        thumbs: Loader,
        persist: Persist,
        settings: Settings,
        opts: Options,
        started: Instant,
    ) -> App {
        let previews = Loader::new(1, false, wake.clone(), Arc::new(|p: &Path| info::preview_decode(p)));
        let panels = Panels { pin_top: persist.pin_top, pin_left: persist.pin_left, pin_right: persist.pin_right, ..Default::default() };
        let order = Order { key: persist.sort, descending: persist.sort_desc, shuffle: None };
        let mut listing = Listing::new(persist.show, order);
        listing.folders = persist.folders;
        listing.fold = settings.sequences.then_some(settings.sequence_min as usize);
        let chrome = chrome::layout(size.0 as f32, size.1 as f32, dpi, &panels);
        let mut app = App {
            hwnd,
            renderer: None,
            gpu_rx,
            text: Text::default(),
            wake,
            size,
            dpi,
            mode: persist.view,
            persist,
            settings,
            opts,
            listing,
            sel: Selection::default(),
            scroll: 0.0,
            loading: None,
            scan_gen: 1,
            back: Vec::new(),
            forward: Vec::new(),
            panels,
            chrome,
            edge: None,
            left_since: None,
            tree: Tree::empty(),
            drive_labels: HashMap::new(),
            tree_for: PathBuf::new(),
            right_scroll: 0.0,
            tx,
            rx,
            thumbs,
            staged: Vec::new(),
            thumb_failed: HashSet::new(),
            thumb_keep: HashSet::new(),
            previews,
            preview_id: None,
            preview_wanted: None,
            hist: HashMap::new(),
            info: None,
            info_for: None,
            folder_size: FolderSize::default(),
            active: None,
            toast: None,
            help: false,
            help_links: Vec::new(),
            rubber: None,
            last_mouse: (i32::MIN, i32::MIN),
            job: None,
            archive: None,
            archive_pending: None,
            job_text: String::new(),
            timing: Timing { started: Some(started), ..Default::default() },
            shuffle_seed: None,
            rng: 0x9E37_79B9_7F4A_7C15 ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1,
            taskbar_shown: false,
            edge_ticks: 0,
            cut: Vec::new(),
            last_dest: None,
            pending_select: Vec::new(),
            cursor_hint: None,
            cpu: None,
            viewer_started: false,
            editor_started: false,
            typeahead: String::new(),
            typeahead_at: None,
            sb_drag: None,
            drag_from: None,
            tree_sb_drag: None,
            menubar: nv_win::menubar::MenuBar::new(&crate::menus::BAR_LABELS),
            undo: Vec::new(),
            pv: crate::preview::Preview::new(None),
        };
        app.start_tree();
        app.start_drive_labels();
        app.timing.window_ms = Some(started.elapsed().as_secs_f64() * 1000.0);
        app.sync_cell();
        if nv_win::is_fullscreen() {
            app.fullscreen_timers(true);
        }
        app
    }

    /// Build the folder tree's first rows on a worker thread: the window and the first thumbnails never wait for it.
    fn start_tree(&self) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::TreeReady(Tree::new()));
            wake();
        });
    }

    /// Ask every drive for its volume label, each on its own thread (a sleeping hard disk or a network drive can take
    /// seconds to answer; the tree shows "Local disk (D:)" until the label arrives, and the window never waits for it).
    fn start_drive_labels(&self) {
        for (root, kind) in nv_win::files::drives() {
            if matches!(kind, nv_win::files::DriveKind::Optical) {
                continue; // an empty drive spins up and clicks for nothing
            }
            let (tx, wake) = (self.tx.clone(), self.wake.clone());
            std::thread::spawn(move || {
                if let Some(label) = nv_win::files::volume_label(&root) {
                    let _ = tx.send(Msg::DriveLabel { root, label });
                    wake();
                }
            });
        }
    }

    pub fn ms_since_start(&self) -> f64 {
        self.timing.started.map(|s| s.elapsed().as_secs_f64() * 1000.0).unwrap_or(0.0)
    }

    // ---- geometry ----------------------------------------------------------------------------------

    /// The thumbnail cell width in physical pixels.
    pub fn cell_px(&self) -> f32 {
        self.persist.cell * self.dpi
    }

    fn sync_cell(&mut self) {
        self.persist.cell = self.persist.cell.clamp(CELL_MIN, CELL_MAX);
    }

    pub fn label_h(&self) -> f32 {
        if self.persist.names && self.persist.cell >= 100.0 { crate::gfx::line_h((13.0 * self.dpi) as i32) + 8.0 * self.dpi } else { 0.0 }
    }

    pub fn header_h(&self) -> f32 {
        if self.mode == ViewMode::Details { 28.0 * self.dpi } else { 0.0 }
    }

    /// The menu bar pushes the toolbar and the side panels down while it shows (it used to cover the buttons).
    pub fn sync_menubar_inset(&mut self) {
        let inset = if self.menubar.visible() { self.menubar.height(self.dpi) } else { 0.0 };
        if (self.panels.top_inset - inset).abs() > 0.5 {
            self.panels.top_inset = inset;
            self.relayout();
        }
    }

    pub fn relayout(&mut self) {
        self.chrome = chrome::layout(self.size.0 as f32, self.size.1 as f32, self.dpi, &self.panels);
    }

    /// The item grid for the current view.
    pub fn grid(&self) -> Grid {
        let d = self.dpi;
        let mut area = self.chrome.area;
        area[1] += self.header_h();
        let n = self.listing.len();
        match self.mode {
            ViewMode::Thumbs => {
                let cell = self.cell_px();
                Grid::new(n, cell, cell + self.label_h(), 10.0 * d, 10.0 * d, 16.0 * d, area, false)
            }
            ViewMode::Details => Grid::new(n, 0.0, 30.0 * d, 0.0, 2.0 * d, 8.0 * d, area, true),
            ViewMode::List => Grid::new(n, 250.0 * d, 28.0 * d, 8.0 * d, 2.0 * d, 12.0 * d, area, false),
        }
    }

    pub fn clamp_scroll(&mut self) {
        let g = self.grid();
        self.scroll = g.clamp(self.scroll);
    }

    // ---- title / toast -----------------------------------------------------------------------------

    pub fn title(&self) -> String {
        let shown = self.display_dir();
        let dir = shown.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| shown.display().to_string());
        let n = self.listing.len();
        let selected = self.sel.count(&self.listing);
        let sel = match selected {
            0 => String::new(),
            1 => format!(" — 1 selected: {}", self.sel.entries(&self.listing).first().map(|e| e.name.clone()).unwrap_or_default()),
            n => format!(" — {n} selected"),
        };
        let ready = match (self.timing.visible_ready_ms, self.loading.is_some()) {
            (Some(ms), false) => format!(" — thumbnails ready in {ms:.0} ms"),
            _ => String::new(),
        };
        let loading = if self.loading.is_some() { " — opening…" } else { "" };
        format!("{dir} — {n} items{sel}{ready}{loading} — NuGallery")
    }

    pub fn update_title(&self) {
        set_title(self.hwnd, &self.title());
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
        set_timer(self.hwnd, TIMER_TOAST, TOAST_MS);
        invalidate(self.hwnd);
    }

    // ---- folders -----------------------------------------------------------------------------------

    /// Open `dir` (selecting `select` in it). `history`: remember where we were so Back works.
    pub fn go(&mut self, dir: PathBuf, select: Vec<PathBuf>, history: bool) {
        if history && !self.listing.dir.as_os_str().is_empty() && !same_dir(&self.listing.dir, &dir) {
            self.back.push(self.listing.dir.clone());
            if self.back.len() > 200 {
                self.back.remove(0);
            }
            self.forward.clear();
        }
        self.active = None;
        self.listing.search.clear();
        self.timing.nav_start = Some(Instant::now());
        self.timing.reported = false;
        self.start_scan(dir, select, false);
    }

    pub fn start_scan(&mut self, dir: PathBuf, select: Vec<PathBuf>, keep_view: bool) {
        self.scan_gen += 1;
        let gen = self.scan_gen;
        self.loading = Some(dir.clone());
        let (tx, wake, hidden) = (self.tx.clone(), self.wake.clone(), self.persist.hidden);
        std::thread::spawn(move || {
            let result = model::scan(&dir, hidden);
            let _ = tx.send(Msg::Scanned { gen, dir, result, select, keep_view });
            wake();
        });
        self.update_title();
    }

    /// Read the folder again, keeping the view and the selection (plus `select`, e.g. a new copy).
    pub fn refresh(&mut self, select: Vec<PathBuf>) {
        let dir = self.listing.dir.clone();
        if dir.as_os_str().is_empty() {
            return;
        }
        self.start_scan(dir, select, true);
    }

    fn on_scanned(&mut self, dir: PathBuf, result: Result<Vec<Entry>, String>, select: Vec<PathBuf>, keep_view: bool) {
        self.loading = None;
        let entries = match result {
            Ok(e) => e,
            Err(e) => {
                self.toast(format!("Can't open the folder: {e}"));
                self.update_title();
                return;
            }
        };
        if self.timing.listing_ms.is_none() {
            self.timing.listing_ms = Some(self.ms_since_start());
        }
        let same = same_dir(&self.listing.dir, &dir);
        let old_scroll = self.scroll;
        let cursor_id = self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| e.id);
        let kept_ids: HashSet<u64> = if keep_view && same { self.sel.ids.clone() } else { HashSet::new() };
        self.listing.set_entries(dir.clone(), entries);
        self.sel = Selection::default();
        self.sel.ids = kept_ids;
        // Drop anything that is no longer in the folder.
        let present: HashSet<u64> = self.listing.all.iter().map(|e| e.id).collect();
        self.sel.ids.retain(|id| present.contains(id));
        for p in &select {
            let id = model::path_id(p);
            if present.contains(&id) {
                self.sel.ids.insert(id);
            }
        }
        // The focused item: the first of `select`, else the one that was focused, else the first item.
        let want = select.first().map(|p| model::path_id(p)).or(if keep_view && same { cursor_id } else { None });
        let hint = self.cursor_hint.take().map(|h| h.min(self.listing.len().saturating_sub(1)));
        self.sel.cursor = want.and_then(|id| self.listing.position_of(id)).or(hint).or(if self.listing.len() > 0 { Some(0) } else { None }).filter(|_| self.listing.len() > 0);
        self.sel.anchor = self.sel.cursor;
        if select.is_empty() && !(keep_view && same) {
            self.sel.ids.clear(); // opening a folder starts with nothing selected, only the focus on the first item
        }
        self.scroll = if keep_view && same { old_scroll } else { 0.0 };
        if !select.is_empty() {
            if let Some(c) = self.sel.cursor {
                let g = self.grid();
                self.scroll = g.scroll_to_center(c);
            }
        }
        self.clamp_scroll();
        self.thumb_failed.clear();
        self.archive_check();
        // An archive's cache folder is not a place to start the next run in.
        if self.archive.is_none() {
            self.persist.dir = Some(dir.clone());
        }
        self.persist.save(); // the next start (also from nuviewer.exe alone) opens this folder, even if we are killed
        if !(keep_view && same) {
            self.folder_size = FolderSize { gen: self.folder_size.gen + 1, ..Default::default() };
            self.start_folder_size(dir.clone());
            self.timing.visible_ready_ms = None;
            self.timing.visible_count = 0;
        }
        self.request_thumbs();
        self.after_focus_change();
        self.reveal_tree();
        self.update_title();
        if self.renderer.is_none() {
            self.cpu_full();
        }
        invalidate(self.hwnd);
    }

    fn start_folder_size(&self, dir: PathBuf) {
        let gen = self.folder_size.gen;
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let (mut bytes, mut files, mut folders) = (0u64, 0u64, 0u64);
            let mut stack = vec![dir];
            let mut last = Instant::now();
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let Ok(m) = e.metadata() else { continue };
                    if m.is_dir() {
                        folders += 1;
                        stack.push(e.path());
                    } else {
                        files += 1;
                        bytes += m.len();
                    }
                    if last.elapsed() > Duration::from_millis(250) {
                        last = Instant::now();
                        if tx.send(Msg::FolderSize { gen, bytes, files, folders, done: false }).is_err() {
                            return;
                        }
                        wake();
                    }
                }
            }
            let _ = tx.send(Msg::FolderSize { gen, bytes, files, folders, done: true });
            wake();
        });
    }

    pub fn go_up(&mut self) {
        if self.archive_up() {
            return;
        }
        let from = self.listing.dir.clone();
        if let Some(parent) = from.parent().map(Path::to_path_buf).filter(|p| !p.as_os_str().is_empty()) {
            self.go(parent, vec![from], true);
        }
    }

    pub fn go_back(&mut self) {
        if let Some(prev) = self.back.pop() {
            let here = self.listing.dir.clone();
            self.forward.push(here.clone());
            self.go(prev, vec![here], false);
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            let here = self.listing.dir.clone();
            self.back.push(here);
            self.go(next, Vec::new(), false);
        }
    }

    /// Show whatever `path` is: a folder, or a file (its folder with the file selected).
    pub fn show_path(&mut self, path: &Path) {
        if path.is_dir() {
            self.go(path.to_path_buf(), Vec::new(), true);
        } else if let Some(parent) = path.parent() {
            if same_dir(&self.listing.dir, parent) && self.listing.position_of(model::path_id(path)).is_some() {
                let i = self.listing.position_of(model::path_id(path)).unwrap_or(0);
                self.select_only(i);
            } else {
                self.go(parent.to_path_buf(), vec![path.to_path_buf()], true);
            }
        }
    }

    fn reveal_tree(&mut self) {
        if !self.panels.left_shown() || self.tree_for == self.listing.dir {
            return;
        }
        self.tree_for = self.listing.dir.clone();
        let dir = self.listing.dir.clone();
        if let Some(i) = self.tree.reveal(&dir) {
            let (row_h, view_h) = (crate::draw::TREE_ROW * self.dpi, self.size.1 as f32 * 0.5);
            self.tree.scroll_to_show(i, row_h, view_h);
        }
    }

    // ---- selection ---------------------------------------------------------------------------------

    pub fn after_focus_change(&mut self) {
        // The right panel describes the focused item, after a short rest.
        self.update_title();
        if self.panels.right_shown() {
            set_timer(self.hwnd, TIMER_INFO, INFO_DEBOUNCE_MS);
        }
        invalidate(self.hwnd);
    }

    pub fn select_only(&mut self, i: usize) {
        self.sel.only(&self.listing, i);
        self.show_cursor();
    }

    /// Scroll so the focused item is visible, fetch its thumbnail, and update the title / panel.
    pub fn show_cursor(&mut self) {
        if let Some(c) = self.sel.cursor {
            let g = self.grid();
            self.scroll = g.scroll_to_show(c, g.clamp(self.scroll));
        }
        self.request_thumbs();
        self.after_focus_change();
    }

    /// Move the focus by `delta` items (Shift extends the selection, Ctrl moves without selecting).
    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.listing.len();
        if n == 0 {
            return;
        }
        let from = self.sel.cursor.unwrap_or(0) as isize;
        let to = (from + delta).clamp(0, n as isize - 1) as usize;
        self.move_cursor_to(to);
    }

    pub fn move_cursor_to(&mut self, to: usize) {
        let n = self.listing.len();
        if n == 0 {
            return;
        }
        let to = to.min(n - 1);
        let m = nv_win::modifiers();
        if m.shift {
            self.sel.range_to(&self.listing, to, false);
        } else if m.ctrl {
            self.sel.cursor = Some(to);
        } else {
            self.sel.only(&self.listing, to);
        }
        self.show_cursor();
    }

    pub fn page_items(&self) -> usize {
        let g = self.grid();
        let rows = ((g.area[3] - g.area[1]) / (g.item_h + g.gap_y)).floor() as usize;
        rows.saturating_sub(1).max(1) * g.cols
    }

    // ---- thumbnails --------------------------------------------------------------------------------

    pub fn has_thumb(&self, id: u64) -> bool {
        self.renderer.as_ref().map(|r| r.has_thumb(id)).unwrap_or(false) || self.staged.iter().any(|(i, _)| *i == id)
    }

    /// Keep thumbnails for the visible rows (plus a margin) and ask for the missing ones, visible first.
    pub fn request_thumbs(&mut self) {
        let g = self.grid();
        let scroll = g.clamp(self.scroll);
        let vis = g.visible(scroll);
        let near = g.visible_with_margin(scroll, PREFETCH_ROWS);
        let keep_range = g.visible_with_margin(scroll, KEEP_ROWS);
        let mut keep: HashSet<u64> = keep_range.filter_map(|i| self.listing.get(i)).map(|e| e.id).collect();
        if let Some(id) = self.preview_id {
            keep.insert(info::preview_key(id));
        }
        if let Some(r) = self.renderer.as_mut() {
            r.retain_thumbs(|id| keep.contains(&id));
        }
        self.staged.retain(|(id, _)| keep.contains(id));
        self.thumb_keep = keep;
        let order = vis.clone().chain(near.filter(|i| !vis.contains(i)));
        let todo: Vec<PathBuf> = order
            .filter_map(|i| self.listing.get(i))
            .filter(|e| e.wants_thumb() && !self.has_thumb(e.id) && !self.thumb_failed.contains(&e.id))
            .map(|e| e.path.clone())
            .collect();
        self.thumbs.request(todo);
        self.check_visible_ready();
    }

    /// Take finished thumbnails and put them on the GPU (within a time budget). True if anything changed.
    pub fn drain_thumbs(&mut self) -> bool {
        let t0 = Instant::now();
        let mut changed = false;
        while t0.elapsed() < UPLOAD_SLICE {
            let Some(l) = self.thumbs.try_recv() else { break };
            if l.cancelled {
                continue;
            }
            let id = model::path_id(&l.path);
            match l.result {
                Ok(img) => {
                    if !self.thumb_keep.contains(&id) {
                        continue; // scrolled far away meanwhile
                    }
                    if self.timing.first_thumb_ms.is_none() {
                        self.timing.first_thumb_ms = Some(self.ms_since_start());
                    }
                    match self.renderer.as_mut() {
                        Some(r) => {
                            let _ = r.upload_thumb(id, &img);
                        }
                        None => {
                            self.staged.push((id, img));
                            self.cpu_thumb(id);
                        }
                    }
                    changed = true;
                }
                Err(_) => {
                    self.thumb_failed.insert(id);
                    changed = true;
                }
            }
        }
        if changed {
            self.check_visible_ready();
            invalidate(self.hwnd);
        }
        changed
    }

    pub fn upload_staged(&mut self) {
        if let Some(r) = self.renderer.as_mut() {
            for (id, img) in self.staged.drain(..) {
                let _ = r.upload_thumb(id, &img);
            }
        }
    }

    /// Record how long it took for every visible thumbnail to be on screen (once per opened folder).
    pub fn check_visible_ready(&mut self) {
        if self.timing.visible_ready_ms.is_some() || self.listing.len() == 0 {
            return;
        }
        let g = self.grid();
        let vis = g.visible(g.clamp(self.scroll));
        // Only count what is really inside the window (the last row may be cut), and only what has a thumbnail to wait for.
        let (area_top, area_bottom) = (g.area[1], g.area[3]);
        let mut total = 0;
        let mut ready = 0;
        for i in vis {
            let r = g.rect(i, g.clamp(self.scroll));
            if r[3] <= area_top || r[1] >= area_bottom {
                continue;
            }
            let Some(e) = self.listing.get(i) else { continue };
            if !e.has_thumb() {
                continue;
            }
            total += 1;
            if self.has_thumb(e.id) || self.thumb_failed.contains(&e.id) {
                ready += 1;
            }
        }
        if ready < total || self.loading.is_some() {
            return;
        }
        // Wait for the GPU too: "on screen" needs a frame.
        if self.renderer.is_none() && self.cpu.is_none() {
            return;
        }
        let ms = match self.timing.nav_start {
            Some(t) => t.elapsed().as_secs_f64() * 1000.0,
            None => self.ms_since_start(),
        };
        self.timing.visible_ready_ms = Some(ms);
        self.timing.visible_count = total;
        self.update_title();
    }

    // ---- right panel: preview and details -------------------------------------------------------------

    /// The timer fired: load the preview / details of the focused item.
    fn load_info(&mut self) {
        kill_timer(self.hwnd, TIMER_INFO);
        if !self.panels.right_shown() {
            return;
        }
        let Some(e) = self.sel.cursor.and_then(|c| self.listing.get(c)) else {
            self.info = None;
            self.info_for = None;
            return;
        };
        let (id, path, is_dir, has_thumb, kind, ext) = (e.id, e.path.clone(), e.is_dir(), e.has_thumb(), e.kind, e.ext.clone());
        if self.info_for == Some(id) {
            return;
        }
        self.info_for = Some(id);
        self.info = None;
        self.right_scroll = 0.0;
        self.pv_begin(id, path.clone(), kind, &ext);
        if has_thumb && !self.hist.contains_key(&id) {
            self.preview_wanted = Some((id, path.clone()));
            self.previews.request(vec![path.clone()]);
        }
        if !is_dir {
            let (tx, wake) = (self.tx.clone(), self.wake.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Info(info::collect(&path, id)));
                wake();
            });
        }
        invalidate(self.hwnd);
    }

    fn drain_previews(&mut self) {
        while let Some(l) = self.previews.try_recv() {
            if l.cancelled {
                continue;
            }
            let id = model::path_id(&l.path);
            if let Ok(img) = l.result {
                let wanted = self.preview_wanted.as_ref().map(|(w, _)| *w == id).unwrap_or(false);
                if !wanted {
                    continue;
                }
                self.hist.insert(id, info::histogram(&img));
                if self.hist.len() > 64 {
                    let keep = self.info_for;
                    self.hist.retain(|k, _| Some(*k) == keep);
                }
                if let Some(r) = self.renderer.as_mut() {
                    let _ = r.upload_thumb(info::preview_key(id), &img);
                }
                self.preview_id = Some(id);
                invalidate(self.hwnd);
            }
        }
    }

    // ---- events ------------------------------------------------------------------------------------

    pub fn handle(&mut self, ev: Event) {
        match ev {
            Event::Resize { width, height } => {
                self.size = (width, height);
                if let Some(r) = self.renderer.as_mut() {
                    let _ = r.resize(width, height);
                }
                self.relayout();
                self.clamp_scroll();
                self.request_thumbs();
                if self.renderer.is_none() {
                    self.cpu_full();
                }
                invalidate(self.hwnd);
            }
            Event::Dpi(scale) => {
                self.dpi = scale;
                self.relayout();
                self.clamp_scroll();
                self.request_thumbs();
                invalidate(self.hwnd);
            }
            Event::Paint => {
                if self.renderer.is_none() {
                    self.cpu_full();
                } else {
                    self.draw();
                }
            }
            Event::User => self.on_user(),
            Event::Timer(id) => self.on_timer(id),
            Event::Key(vk) => {
                if !self.menubar_key(vk) {
                    self.on_key(vk);
                }
            }
            Event::AltTap => {
                if self.active.is_none() {
                    self.menubar.alt_tap();
                    invalidate(self.hwnd);
                }
            }
            Event::Char(c) => self.on_char(c),
            Event::Wheel { delta, x, y, ctrl } => self.on_wheel(delta, x, y, ctrl),
            Event::MouseDown { x, y, ctrl } => {
                if self.help {
                    if let Some((_, url)) = self.help_links.iter().find(|(r, _)| (x as f32) >= r[0] && (x as f32) < r[2] && (y as f32) >= r[1] && (y as f32) < r[3]) {
                        nv_win::shell::open_url(url);
                        return;
                    }
                }
                if !self.menubar_press(x, y) {
                    self.on_mouse_down(x, y, ctrl, false)
                }
            }
            Event::DoubleClick { x, y, ctrl } => {
                if !self.menubar_press(x, y) {
                    self.on_mouse_down(x, y, ctrl, true)
                }
            }
            Event::MouseMove { x, y, left_down } => {
                if !left_down && self.menubar.pointer(x, y, self.dpi) {
                    self.sync_menubar_inset();
                    invalidate(self.hwnd);
                }
                self.on_mouse_move(x, y, left_down)
            }
            Event::MouseUp => self.on_mouse_up(),
            Event::RightClick { x, y } => self.on_right_click(x, y),
            Event::MiddleClick => self.on_middle_click(),
            Event::Open { path, .. } => {
                // The viewer (or a second launch) says where we should be: window or fullscreen as it was.
                if let Some(s) = nv_win::winstate::load() {
                    if s.fullscreen != nv_win::is_fullscreen() {
                        crate::draw::toggle_fullscreen(self);
                    }
                }
                nv_win::instance::bring_to_front(self.hwnd);
                if let Some(p) = path {
                    // A picture may have been rewritten or a copy added meanwhile (NuEditor): read the folder again, on that picture.
                    let p = Path::new(&p);
                    if p.parent().map(|d| same_dir(&self.listing.dir, d)).unwrap_or(false) {
                        // Its thumbnail may be out of date (burned in, or its pair file changed): make it again.
                        let id = model::path_id(p);
                        if let Some(r) = &mut self.renderer {
                            r.retain_thumbs(|t| t != id && t != info::preview_key(id));
                        }
                        self.refresh(vec![p.to_path_buf()]);
                    } else {
                        self.show_path(p);
                    }
                }
            }
            Event::Drop { paths, x, y } => self.on_drop(paths, x, y),
            Event::Close => {
                self.save_state();
                // Closing the gallery closes the program: a viewer it opened goes with it.
                if self.viewer_started {
                    nv_win::instance::close_window(nv_win::instance::Program::Viewer);
                }
                if self.editor_started {
                    nv_win::instance::close_window(nv_win::instance::Program::Editor);
                }
                if self.job.is_some() {
                    if let Some(j) = &self.job {
                        j.cancel.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    pub fn save_state(&mut self) {
        self.persist.view = self.mode;
        self.persist.show = self.listing.show;
        self.persist.folders = self.listing.folders;
        self.persist.sort = self.listing.order.key;
        self.persist.sort_desc = self.listing.order.descending;
        self.persist.pin_top = self.panels.pin_top;
        self.persist.pin_left = self.panels.pin_left;
        self.persist.pin_right = self.panels.pin_right;
        self.persist.save();
        // The viewer walks the folder in the same order as the gallery shows it.
        let mut s = Settings::load();
        if s.sort != self.listing.order.key || s.sort_desc != self.listing.order.descending {
            s.sort = self.listing.order.key;
            s.sort_desc = self.listing.order.descending;
            let _ = s.save();
        }
    }

    fn on_user(&mut self) {
        // The GPU device arrives from its own thread.
        if self.renderer.is_none() {
            if let Ok(r) = self.gpu_rx.try_recv() {
                match r {
                    Ok(gpu) => match Renderer::from_device(gpu, self.hwnd, self.size.0, self.size.1) {
                        Ok(rend) => {
                            self.renderer = Some(rend);
                            self.cpu = None;
                            self.timing.gpu_ms = Some(self.ms_since_start());
                            self.upload_staged();
                            invalidate(self.hwnd);
                        }
                        Err(e) => set_title(self.hwnd, &format!("NuGallery — renderer failed: {e}  [NV-5002]")),
                    },
                    Err(e) => set_title(self.hwnd, &format!("NuGallery — GPU init failed: {e}  [NV-5002]")),
                }
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Scanned { gen, dir, result, select, keep_view } => {
                    if gen == self.scan_gen {
                        let mut select = select;
                        select.extend(std::mem::take(&mut self.pending_select));
                        self.on_scanned(dir, result, select, keep_view);
                    }
                }
                Msg::Info(data) => {
                    if self.info_for == Some(data.id) {
                        self.info = Some(data);
                        invalidate(self.hwnd);
                    }
                }
                Msg::FolderSize { gen, bytes, files, folders, done } => {
                    if gen == self.folder_size.gen {
                        self.folder_size = FolderSize { gen, bytes, files, folders, done };
                        if self.panels.left_shown() {
                            invalidate(self.hwnd);
                        }
                    }
                }
                Msg::AnimFrame { id, img, delay, total, label } => self.pv_frame(id, img, delay, total, label),
                Msg::Mesh { id, mesh } => self.pv_mesh(id, mesh),
                Msg::BigPreview { id, img } => self.pv_big(id, img),
                Msg::DriveLabel { root, label } => {
                    self.tree.set_drive_label(&root, &label);
                    self.drive_labels.insert(root, label);
                    if self.panels.left_shown() {
                        invalidate(self.hwnd);
                    }
                }
                Msg::TreeReady(mut tree) => {
                    for (root, label) in &self.drive_labels {
                        tree.set_drive_label(root, label);
                    }
                    self.tree = tree;
                    self.tree_for = PathBuf::new();
                    self.reveal_tree();
                    if self.panels.left_shown() {
                        invalidate(self.hwnd);
                    }
                }
                Msg::JobProgress(text) => {
                    self.job_text = text;
                    invalidate(self.hwnd);
                }
                Msg::Archive(d) => self.archive_pending = Some(d),
                Msg::JobDone(r) => {
                    self.job = None;
                    self.job_text.clear();
                    if !r.message.is_empty() {
                        self.toast(r.message);
                    }
                    if r.refresh {
                        self.refresh(r.select);
                    }
                    if let Some(d) = self.archive_pending.take() {
                        self.archive_done(*d);
                    }
                }
            }
        }
        self.drain_thumbs();
        self.drain_previews();
        // A frame is waiting for the GPU: draw it as soon as it exists.
        if self.renderer.is_some() && self.timing.first_frame_ms.is_none() {
            invalidate(self.hwnd);
        }
        self.check_visible_ready();
        self.maybe_report();
    }

    /// `--bench-out`: write the start-up timings once everything visible is ready (and optionally exit).
    fn maybe_report(&mut self) {
        let Some(path) = self.opts.bench_out.clone() else { return };
        if self.timing.reported || self.timing.visible_ready_ms.is_none() || self.timing.first_frame_ms.is_none() {
            return;
        }
        self.timing.reported = true;
        let f = |v: Option<f64>| v.map(|x| format!("{x:.1}")).unwrap_or_else(|| "n/a".into());
        let text = format!(
            "window_ms={}\nlisting_ms={}\ngpu_ms={}\nfirst_thumb_ms={}\ncpu_frame_ms={}\nfirst_frame_ms={}\nvisible_ready_ms={}\nvisible_count={}\nitems={}\n",
            f(self.timing.window_ms),
            f(self.timing.listing_ms),
            f(self.timing.gpu_ms),
            f(self.timing.first_thumb_ms),
            f(self.timing.cpu_frame_ms),
            f(self.timing.first_frame_ms),
            f(self.timing.visible_ready_ms),
            self.timing.visible_count,
            self.listing.len()
        );
        let _ = std::fs::write(&path, text);
        if self.opts.bench_exit {
            nv_win::quit();
        }
    }

    fn on_timer(&mut self, id: u32) {
        match id {
            TIMER_TOAST => {
                kill_timer(self.hwnd, TIMER_TOAST);
                self.toast = None;
                invalidate(self.hwnd);
            }
            TIMER_INFO => self.load_info(),
            crate::preview::TIMER_PREVIEW => self.pv_tick(),
            TIMER_EDGE => self.edge_tick(),
            TIMER_TASKBAR => self.taskbar_tick(),
            TIMER_CURSOR => {
                kill_timer(self.hwnd, TIMER_CURSOR);
            }
            _ => {}
        }
    }

    // ---- edges: panels and the Windows taskbar -------------------------------------------------------

    pub fn fullscreen_timers(&mut self, on: bool) {
        if on {
            set_timer(self.hwnd, TIMER_TASKBAR, 80);
        } else {
            kill_timer(self.hwnd, TIMER_TASKBAR);
            self.taskbar_shown = false;
            self.edge_ticks = 0;
        }
    }

    /// Fullscreen: the pointer on the bottom row shows the Windows taskbar (the bottom edge's panel).
    pub fn taskbar_tick(&mut self) {
        if !nv_win::is_fullscreen() {
            kill_timer(self.hwnd, TIMER_TASKBAR);
            return;
        }
        let (cx, cy) = nv_win::cursor_pos();
        let (l, _, r, b) = nv_win::monitor_rect(self.hwnd);
        let in_x = cx >= l && cx < r;
        if !self.taskbar_shown {
            if !nv_win::is_foreground(self.hwnd) {
                return;
            }
            if in_x && cy >= b - 1 {
                self.edge_ticks = self.edge_ticks.saturating_add(1);
                if self.edge_ticks >= 2 {
                    nv_win::set_taskbar_hidden(self.hwnd, false);
                    self.taskbar_shown = true;
                }
            } else {
                self.edge_ticks = 0;
            }
        } else if !in_x || cy < b - nv_win::taskbar_height() - 6 {
            nv_win::set_taskbar_hidden(self.hwnd, true);
            self.taskbar_shown = false;
            self.edge_ticks = 0;
        }
    }

    /// Which edge (1 left, 2 right, 3 top) the pointer is on, if any.
    fn edge_of(&self, x: i32, y: i32) -> Option<u8> {
        let z = (chrome::EDGE_ZONE * self.dpi).max(2.0) as i32;
        let (w, _) = (self.size.0 as i32, self.size.1 as i32);
        if y < z {
            Some(3)
        } else if x < z {
            Some(1)
        } else if x >= w - z {
            Some(2)
        } else {
            None
        }
    }

    fn on_edge_move(&mut self, x: i32, y: i32) {
        match self.edge_of(x, y) {
            Some(e) if self.edge.map(|(k, _)| k) != Some(e) => {
                self.edge = Some((e, Instant::now()));
                set_timer(self.hwnd, TIMER_EDGE, 16);
            }
            None => {
                self.edge = None;
            }
            _ => {}
        }
        // The panel comes out the moment the pointer touches its edge (no waiting for the timer).
        if self.edge.is_some() {
            self.edge_tick();
        }
        if self.panels.top_open || self.panels.left_open || self.panels.right_open {
            set_timer(self.hwnd, TIMER_EDGE, 16);
        }
    }

    /// Slides panels out after the pointer has rested on their edge, and away after it has left them.
    pub fn edge_tick(&mut self) {
        let (x, y) = self.last_mouse;
        let (fx, fy) = (x as f32, y as f32);
        let mut changed = false;
        if let Some((e, since)) = self.edge {
            if since.elapsed() >= EDGE_DWELL {
                match e {
                    1 if !self.panels.left_open => {
                        self.panels.left_open = true;
                        changed = true;
                    }
                    2 if !self.panels.right_open => {
                        self.panels.right_open = true;
                        changed = true;
                    }
                    3 if !self.panels.top_open => {
                        self.panels.top_open = true;
                        changed = true;
                    }
                    _ => {}
                }
                self.edge = None;
            }
        }
        // Leaving: a panel stays while the pointer is on it (or on its edge) or a box inside it has the keyboard.
        let typing = self.active.is_some();
        let on_edge = self.edge_of(x, y);
        let slack = 3.0 * self.dpi;
        let near = |r: Option<[f32; 4]>| r.map(|r| fx >= r[0] - slack && fx < r[2] + slack && fy >= r[1] - slack && fy < r[3] + slack).unwrap_or(false);
        let keep_top = typing || near(self.chrome.top) || on_edge == Some(3);
        let keep_left = near(self.chrome.left) || on_edge == Some(1);
        let keep_right = (typing && matches!(self.active.as_ref().map(|a| &a.focus), Some(Focus::Meta(_)))) || near(self.chrome.right) || on_edge == Some(2);
        let mut any_open = false;
        for (open, keep) in [(&mut self.panels.top_open, keep_top), (&mut self.panels.left_open, keep_left), (&mut self.panels.right_open, keep_right)] {
            if *open {
                if keep {
                    self.left_since = None;
                    any_open = true;
                } else {
                    let since = *self.left_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= EDGE_LEAVE {
                        *open = false;
                        self.left_since = None;
                        changed = true;
                    } else {
                        any_open = true;
                    }
                }
            }
        }
        if !any_open && self.edge.is_none() {
            kill_timer(self.hwnd, TIMER_EDGE);
        }
        if changed {
            self.relayout();
            self.panels_changed();
        }
    }

    /// The set of visible panels changed: re-fit the items and refresh what a newly shown panel needs.
    pub fn panels_changed(&mut self) {
        self.relayout();
        self.clamp_scroll();
        self.reveal_tree();
        if self.panels.right_shown() {
            set_timer(self.hwnd, TIMER_INFO, INFO_DEBOUNCE_MS);
        }
        self.request_thumbs();
        invalidate(self.hwnd);
    }

    // ---- view settings --------------------------------------------------------------------------------

    pub fn set_mode(&mut self, m: ViewMode) {
        if self.mode == m {
            return;
        }
        self.mode = m;
        // Keep the focused item in view in the new layout.
        let g = self.grid();
        self.scroll = self.sel.cursor.map(|c| g.scroll_to_center(c)).unwrap_or(0.0);
        self.request_thumbs();
        self.save_state();
        invalidate(self.hwnd);
    }

    pub fn set_cell(&mut self, logical: f32) {
        let before = self.sel.cursor;
        self.persist.cell = logical.clamp(CELL_MIN, CELL_MAX);
        if self.mode != ViewMode::Thumbs {
            self.mode = ViewMode::Thumbs;
        }
        let g = self.grid();
        self.scroll = before.map(|c| g.scroll_to_show(c, g.clamp(self.scroll))).unwrap_or(0.0);
        self.clamp_scroll();
        self.request_thumbs();
        invalidate(self.hwnd);
    }

    /// Ctrl + wheel: a step bigger or smaller (about one column more or less).
    pub fn zoom_cells(&mut self, notches: f32) {
        self.set_cell(self.persist.cell * 1.1f32.powf(notches));
    }

    pub fn cell_level(&mut self, n: usize) {
        if let Some(&c) = CELL_LEVELS.get(n) {
            self.set_cell(c);
            self.toast(format!("Thumbnail size {}", n + 1));
        }
    }

    pub fn reset_cell(&mut self) {
        self.set_cell(CELL_DEFAULT);
        self.toast("Thumbnail size reset");
    }

    pub fn set_sort(&mut self, key: SortKey) {
        if self.listing.order.key == key && self.listing.order.shuffle.is_none() {
            self.listing.order.descending = !self.listing.order.descending;
        } else {
            self.listing.order.key = key;
            self.listing.order.descending = false;
        }
        self.listing.order.shuffle = None;
        self.shuffle_seed = None;
        self.rearrange_keeping_focus();
        self.save_state();
    }

    pub fn set_show(&mut self, show: Show) {
        self.listing.show = show;
        self.rearrange_keeping_focus();
        self.save_state();
    }

    pub fn toggle_shuffle(&mut self) {
        if self.listing.order.shuffle.is_some() {
            self.listing.order.shuffle = None;
            self.shuffle_seed = None;
            self.toast("Shuffle off");
        } else {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            self.listing.order.shuffle = Some(self.rng);
            self.shuffle_seed = Some(self.rng);
            self.toast("Shuffled — the viewer follows this order");
        }
        self.rearrange_keeping_focus();
    }

    /// Re-sort / re-filter, keeping the same item focused and the selection.
    pub fn rearrange_keeping_focus(&mut self) {
        let focused = self.sel.cursor.and_then(|c| self.listing.get(c)).map(|e| e.id);
        self.listing.rearrange();
        self.sel.cursor = focused.and_then(|id| self.listing.position_of(id)).or(if self.listing.len() > 0 { Some(0) } else { None });
        self.sel.anchor = self.sel.cursor;
        let g = self.grid();
        self.scroll = match self.sel.cursor {
            Some(c) => g.scroll_to_show(c, g.clamp(self.scroll)),
            None => 0.0,
        };
        self.clamp_scroll();
        self.request_thumbs();
        self.update_title();
        invalidate(self.hwnd);
    }

    pub fn set_search(&mut self, text: &str) {
        self.listing.search = text.to_string();
        self.rearrange_keeping_focus();
    }

    // ---- text boxes -----------------------------------------------------------------------------------

    pub fn start_input(&mut self, focus: Focus, ti: TextInput) {
        self.active = Some(Active { focus, ti, error: None });
        invalidate(self.hwnd);
    }

    pub fn focus_path_box(&mut self) {
        self.panels.top_open = true;
        self.relayout();
        let text = self.listing.dir.display().to_string();
        self.start_input(Focus::Path, TextInput::selected(&text));
        set_timer(self.hwnd, TIMER_EDGE, 16);
        self.panels_changed();
    }

    pub fn focus_search(&mut self) {
        self.panels.top_open = true;
        self.relayout();
        let text = self.listing.search.clone();
        self.start_input(Focus::Search, TextInput::selected(&text));
        set_timer(self.hwnd, TIMER_EDGE, 16);
        self.panels_changed();
    }

    /// Typing letters with nothing in a text box jumps to the first item whose name starts with them (like Explorer);
    /// the same letter again goes on to the next one.
    fn type_ahead(&mut self, c: char) {
        let now = Instant::now();
        if self.typeahead_at.map(|t| now.duration_since(t) > Duration::from_millis(1000)).unwrap_or(true) {
            self.typeahead.clear();
        }
        self.typeahead_at = Some(now);
        self.typeahead.extend(c.to_lowercase());
        let n = self.listing.len();
        if n == 0 {
            return;
        }
        let first = self.typeahead.chars().next().unwrap_or(' ');
        let repeated = self.typeahead.chars().all(|x| x == first);
        let key: String = if repeated { first.to_string() } else { self.typeahead.clone() };
        let cur = self.sel.cursor.unwrap_or(0);
        let start = if repeated { (cur + 1) % n } else { cur };
        for k in 0..n {
            let i = (start + k) % n;
            if self.listing.get(i).map(|e| e.name.to_lowercase().starts_with(&key)).unwrap_or(false) {
                self.select_only(i);
                return;
            }
        }
    }

    fn on_char(&mut self, c: char) {
        if self.active.is_none() {
            if c.is_alphanumeric() && !('1'..='5').contains(&c) {
                self.type_ahead(c);
            }
            return;
        }
        let Some(a) = self.active.as_mut() else { return };
        a.ti.insert(c);
        a.error = None;
        self.after_text_change();
    }

    fn after_text_change(&mut self) {
        if let Some(a) = &self.active {
            if a.focus == Focus::Search {
                let t = a.ti.text();
                self.set_search(&t);
            }
        }
        invalidate(self.hwnd);
    }

    /// Keys while a text box has the keyboard. Returns true if the key was used.
    fn input_key(&mut self, vk: u32) -> bool {
        let m = nv_win::modifiers();
        let Some(a) = self.active.as_mut() else { return false };
        let out = a.ti.key(vk, m.ctrl, m.shift);
        match out {
            Outcome::Ignored => {
                // Keys that mean something to the window even while typing: nothing; swallow plain letters (they come as chars).
                !m.ctrl && !m.alt
            }
            Outcome::Handled => {
                a.error = None;
                self.after_text_change();
                true
            }
            Outcome::Tab => true,
            Outcome::Escape => {
                let was = self.active.take();
                if let Some(Active { focus: Focus::Search, .. }) = was {
                    self.set_search("");
                }
                invalidate(self.hwnd);
                true
            }
            Outcome::Enter => {
                self.commit_input();
                true
            }
            Outcome::Clipboard(op) => {
                let hwnd = self.hwnd;
                let Some(a) = self.active.as_mut() else { return true };
                match op {
                    Clip::Copy => {
                        let t = a.ti.selected_text();
                        if !t.is_empty() {
                            let _ = nv_win::shell::clipboard_copy_text(hwnd, &t);
                        }
                    }
                    Clip::Cut => {
                        let t = a.ti.cut();
                        if !t.is_empty() {
                            let _ = nv_win::shell::clipboard_copy_text(hwnd, &t);
                        }
                    }
                    Clip::Paste => {
                        if let Some(t) = nv_win::files::clipboard_text(hwnd) {
                            a.ti.insert_str(&t);
                        }
                    }
                }
                a.error = None;
                self.after_text_change();
                true
            }
        }
    }

    fn commit_input(&mut self) {
        let Some(a) = self.active.take() else { return };
        let text = a.ti.text();
        match a.focus.clone() {
            Focus::Search => {
                self.set_search(&text);
            }
            Focus::Path => {
                let p = PathBuf::from(text.trim().trim_matches('"'));
                if p.exists() {
                    self.show_path(&p);
                } else {
                    self.toast(format!("Not found: {}", p.display()));
                    self.active = Some(Active { focus: Focus::Path, ti: a.ti, error: Some("not found".into()) });
                }
            }
            Focus::Rename(path) => self.finish_rename(path, &text, a),
            Focus::NewFolder(path) => self.finish_new_folder(path, &text, a),
            Focus::Meta(field) => self.finish_meta(field, &text),
        }
        invalidate(self.hwnd);
    }

    // ---- keyboard ---------------------------------------------------------------------------------------

    fn on_key(&mut self, vk: u32) {
        if self.input_key(vk) {
            return;
        }
        let m = nv_win::modifiers();
        let (ctrl, shift, alt) = (m.ctrl, m.shift, m.alt);
        let g = self.grid();
        let cols = g.cols as isize;
        match vk {
            // Alt + arrows: history and "up".
            0x25 if alt => self.go_back(),
            0x27 if alt => self.go_forward(),
            0x26 if alt => self.go_up(),
            0x25 => self.move_cursor(-1),
            0x27 => self.move_cursor(1),
            0x26 => self.move_cursor(-cols),
            0x28 => self.move_cursor(cols),
            0x21 => self.move_cursor(-(self.page_items() as isize)),
            0x22 => self.move_cursor(self.page_items() as isize),
            0x24 => self.move_cursor_to(0),
            0x23 => self.move_cursor_to(usize::MAX),
            0x20 => self.open_selected(false),  // Space: view
            0x73 => self.launch_editor(),       // F4: edit in NuEditor
            0x0D if alt => crate::draw::toggle_fullscreen(self),
            0x0D => self.move_cursor(1),        // Enter: next file
            0x08 => self.go_up(),               // Backspace: up one folder
            0x1B => self.on_escape(),
            0x2E if shift => self.delete_selected(true),
            0x2E => self.delete_selected(false),
            0x71 => self.start_rename(),        // F2
            0x74 => self.refresh(Vec::new()),   // F5
            0x75 => self.transfer_selected(true),  // F6
            0x76 => self.transfer_selected(false), // F7
            0x7A => crate::draw::toggle_fullscreen(self), // F11
            0x70 => {
                self.help = !self.help;
                invalidate(self.hwnd);
            }
            0xC0 => self.reset_cell(), // ` / ~
            0x31..=0x35 if !ctrl && !alt => self.cell_level((vk - 0x31) as usize),
            0x41 if ctrl => {
                self.sel.all(&self.listing);
                self.update_title();
                invalidate(self.hwnd);
            }
            0x43 if ctrl && shift => self.copy_path(),
            0x43 if ctrl => self.copy_files(false),
            0x58 if ctrl => self.copy_files(true),
            0x56 if ctrl => self.paste(),
            0x4E if ctrl => self.new_folder(),
            0x46 if ctrl => self.focus_search(),
            0x4C if ctrl => self.focus_path_box(),
            0x50 if ctrl && shift => self.make_pdf(),       // Ctrl+Shift+P: a PDF of the selected pictures / folder
            0x45 if ctrl && shift => self.extract_selected(), // Ctrl+Shift+E: unpack the selected archive
            0x50 if ctrl => self.print_selected(),
            0x42 if ctrl => {
                self.panels.pin_left = !self.panels.pin_left;
                self.panels_changed();
            }
            0x49 if ctrl => {
                self.panels.pin_right = !self.panels.pin_right;
                self.panels_changed();
            }
            0x54 if ctrl => {
                self.panels.pin_top = !self.panels.pin_top;
                self.panels_changed();
            }
            // Ctrl + / Ctrl - : more / fewer columns (the thumbnails get smaller / bigger); Ctrl+0 is the default size.
            0xBB | 0x6B if ctrl => self.zoom_cells(1.0),
            0xBD | 0x6D if ctrl => self.zoom_cells(-1.0),
            0x30 | 0x60 if ctrl => self.reset_cell(),
            0x5A if ctrl => self.undo_last(),
            0x47 if ctrl => self.toggle_sequences(), // Ctrl+G: group numbered pictures into image sequences
            // Test hooks (nothing on a keyboard sends these): the commands that live in menus and dialogs.
            0xFFF1 => self.batch_rename(),
            0xFFF2 => self.batch_convert(),
            0xFFF3 => self.batch_times(),
            0xFFF4 => self.batch_strip(),
            0xFFF5 => self.compress_selected(),
            0xFFF6 => self.quick_convert(crate::menus::Preset::Png),
            0xFFF7 => self.quick_edit(crate::menus::QuickOp::RotateRight),
            0xFFF8 => self.extract_selected(),
            0xFFF9 => self.make_pdf(),
            0x52 if ctrl => self.toggle_shuffle(),
            0x4A if ctrl => self.set_sort(self.listing.order.key.next()),
            0x31 if ctrl => self.set_mode(ViewMode::Thumbs),
            0x32 if ctrl => self.set_mode(ViewMode::Details),
            0x33 if ctrl => self.set_mode(ViewMode::List),
            0x77 => self.open_selected(true), // F8: slideshow
            _ => {}
        }
    }

    fn on_escape(&mut self) {
        if self.help {
            self.help = false;
        } else if let Some(j) = &self.job {
            j.cancel.store(true, Ordering::Relaxed);
            self.toast("Cancelling…");
        } else if !self.sel.ids.is_empty() {
            self.sel.clear();
            self.update_title();
        } else if !self.listing.search.is_empty() {
            self.set_search("");
        } else if self.panels.left_open || self.panels.right_open || self.panels.top_open {
            self.panels.left_open = false;
            self.panels.right_open = false;
            self.panels.top_open = false;
            self.panels_changed();
        }
        invalidate(self.hwnd);
    }

    pub fn exit(&mut self) {
        self.save_state();
        if self.viewer_started {
            nv_win::instance::close_window(nv_win::instance::Program::Viewer);
        }
        if self.editor_started {
            nv_win::instance::close_window(nv_win::instance::Program::Editor);
        }
        nv_win::quit();
    }

    // ---- mouse ----------------------------------------------------------------------------------------------

    fn on_wheel(&mut self, delta: i32, x: i32, y: i32, ctrl: bool) {
        let (fx, fy) = (x as f32, y as f32);
        // Over a panel the wheel scrolls that panel.
        if let Some(l) = self.chrome.left.filter(|l| crate::gfx::contains(*l, fx, fy)) {
            let _ = l;
            self.tree.scroll -= delta as f32 / 120.0 * 3.0 * crate::draw::TREE_ROW * self.dpi;
            let (row_h, view_h) = (crate::draw::TREE_ROW * self.dpi, self.chrome.left.map(|l| l[3] - l[1]).unwrap_or(400.0) - crate::draw::TREE_FOOT * self.dpi - 36.0 * self.dpi);
            self.tree.clamp(row_h, view_h);
            invalidate(self.hwnd);
            return;
        }
        if self.chrome.right.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
            if self.preview_wheel(delta, x, y) {
                return;
            }
            self.right_scroll = (self.right_scroll - delta as f32 / 120.0 * 60.0 * self.dpi).max(0.0);
            invalidate(self.hwnd);
            return;
        }
        if self.chrome.top.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
            return;
        }
        if ctrl {
            self.zoom_cells(delta as f32 / 120.0);
            return;
        }
        // The wheel moves the scroll bar (three rows a notch); selecting the next file is what the arrow keys are for.
        let g = self.grid();
        let before = self.scroll;
        self.scroll = g.clamp(self.scroll - delta as f32 / 120.0 * 3.0 * (g.item_h + g.gap_y));
        if self.scroll != before {
            self.request_thumbs();
            invalidate(self.hwnd);
        }
    }

    pub fn item_at(&self, x: i32, y: i32) -> Option<usize> {
        if self.chrome.over_panel(x as f32, y as f32) {
            return None;
        }
        let g = self.grid();
        g.hit(x as f32, y as f32, g.clamp(self.scroll))
    }

    fn on_middle_click(&mut self) {
        let (x, y) = self.last_mouse;
        if let Some(i) = self.item_at(x, y) {
            self.select_only(i);
            self.open_selected(false);
        }
    }

    fn on_mouse_move(&mut self, x: i32, y: i32, left_down: bool) {
        let moved = self.last_mouse != (x, y);
        self.last_mouse = (x, y);
        if moved {
            self.on_edge_move(x, y);
        }
        if left_down {
            self.drag_move(x, y);
        }
    }

    /// The scroll bar of the items: (track, thumb), None when everything fits.
    pub fn scrollbar(&self) -> Option<([f32; 4], [f32; 4])> {
        let g = self.grid();
        let max = g.max_scroll();
        if max <= 0.0 {
            return None;
        }
        let d = self.dpi;
        let track = [g.area[2] - 16.0 * d, g.area[1], g.area[2] - 5.0 * d, g.area[3]];
        let th = track[3] - track[1];
        let view = g.area[3] - g.area[1];
        let thumb_h = (th * view / g.content_height()).clamp(36.0 * d, th);
        let y = track[1] + (g.clamp(self.scroll) / max) * (th - thumb_h);
        Some((track, [track[0], y, track[2], y + thumb_h]))
    }

    /// Put the top of the scroll thumb at window y.
    fn scroll_thumb_to(&mut self, thumb_top: f32) {
        let Some((track, thumb)) = self.scrollbar() else { return };
        let g = self.grid();
        let range = (track[3] - track[1]) - (thumb[3] - thumb[1]);
        if range <= 0.0 {
            return;
        }
        let frac = ((thumb_top - track[1]) / range).clamp(0.0, 1.0);
        self.scroll = frac * g.max_scroll();
        self.request_thumbs();
        invalidate(self.hwnd);
    }

    fn on_mouse_up(&mut self) {
        self.sb_drag = None;
        self.tree_sb_drag = None;
        self.preview_release();
        self.drag_from = None;
        if self.rubber.take().is_some() {
            self.update_title();
            invalidate(self.hwnd);
        }
    }

    fn drag_move(&mut self, x: i32, y: i32) {
        if (self.pv.drag.is_some() || self.pv.scrubbing) && self.preview_drag(x, y) {
            return;
        }
        if let Some(grab) = self.tree_sb_drag {
            if let Some(tr) = self.tree_rect() {
                self.tree.scroll_thumb_to(tr, crate::draw::TREE_ROW * self.dpi, self.dpi, y as f32 - grab);
                invalidate(self.hwnd);
            }
            return;
        }
        if let Some(grab) = self.sb_drag {
            self.scroll_thumb_to(y as f32 - grab);
            return;
        }
        if let Some((sx, sy)) = self.drag_from {
            let (dx, dy) = ((x - sx) as f32, (y - sy) as f32);
            if (dx * dx + dy * dy).sqrt() > 8.0 * self.dpi {
                self.drag_from = None;
                self.start_drag();
            }
            return;
        }
        let Some(r) = self.rubber.as_mut() else { return };
        r.now = (x as f32, y as f32);
        let (start_x, start_y, scroll0) = (r.start.0, r.start.1, r.scroll0);
        let (additive, base) = (r.additive, r.base.clone());
        let g = self.grid();
        // Auto-scroll when the band is dragged past the top or bottom of the area.
        let (top, bottom) = (g.area[1], g.area[3]);
        let before = self.scroll;
        if (y as f32) < top + 12.0 {
            self.scroll = g.clamp(self.scroll - 24.0 * self.dpi);
        } else if (y as f32) > bottom - 12.0 {
            self.scroll = g.clamp(self.scroll + 24.0 * self.dpi);
        }
        if self.scroll != before {
            self.request_thumbs();
        }
        // The band's start moves with the content.
        let start_y_now = start_y - (self.scroll - scroll0);
        let hits = g.in_rect([start_x, start_y_now, x as f32, y as f32], self.scroll);
        let mut ids: HashSet<u64> = if additive { base } else { HashSet::new() };
        for i in hits {
            if let Some(e) = self.listing.get(i) {
                ids.insert(e.id);
            }
        }
        self.sel.ids = ids;
        invalidate(self.hwnd);
    }

    fn on_mouse_down(&mut self, x: i32, y: i32, ctrl: bool, double: bool) {
        let (fx, fy) = (x as f32, y as f32);
        self.last_mouse = (x, y);
        // A click anywhere else ends text editing (Enter commits, a click elsewhere does not).
        if let Some(a) = &self.active {
            let inside = match a.focus {
                Focus::Path => crate::gfx::contains(self.chrome.path_box, fx, fy),
                Focus::Search => crate::gfx::contains(self.chrome.search_box, fx, fy),
                _ => false,
            };
            if inside {
                self.click_in_box(x);
                return;
            }
            if !matches!(a.focus, Focus::Meta(_)) || !self.chrome.right.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
                let was = self.active.take();
                if let Some(Active { focus: Focus::Search, .. }) = was {
                    // The search words stay applied.
                }
                invalidate(self.hwnd);
            }
        }
        if let Some(b) = self.chrome.button_at(fx, fy) {
            self.on_button(b);
            return;
        }
        if self.chrome.top.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
            if crate::gfx::contains(self.chrome.path_box, fx, fy) {
                self.focus_path_box();
                self.click_in_box(x);
            } else if crate::gfx::contains(self.chrome.search_box, fx, fy) {
                self.focus_search();
                self.click_in_box(x);
            }
            return;
        }
        if self.chrome.left.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
            self.left_click(fx, fy, double);
            return;
        }
        if self.chrome.right.map(|r| crate::gfx::contains(r, fx, fy)).unwrap_or(false) {
            if !self.preview_press(fx, fy, double) {
                self.right_click_panel(fx, fy);
            }
            return;
        }
        // The scroll bar.
        if let Some((track, thumb)) = self.scrollbar() {
            let d = self.dpi;
            if contains_f([track[0] - 5.0 * d, track[1], track[2] + 3.0 * d, track[3]], fx, fy) {
                if contains_f(thumb, fx, fy) {
                    self.sb_drag = Some(fy - thumb[1]);
                } else {
                    let half = (thumb[3] - thumb[1]) / 2.0;
                    self.sb_drag = Some(half);
                    self.scroll_thumb_to(fy - half);
                }
                return;
            }
        }
        // The details header sorts.
        if self.mode == ViewMode::Details && fy >= self.chrome.area[1] && fy < self.chrome.area[1] + self.header_h() {
            if let Some(k) = crate::draw::header_hit(self, fx) {
                self.set_sort(k);
            }
            return;
        }
        let m = nv_win::modifiers();
        match self.item_at(x, y) {
            Some(i) => {
                if double {
                    let Some(e) = self.listing.get(i) else { return };
                    if e.is_dir() {
                        let p = e.path.clone();
                        self.go(p, Vec::new(), true);
                    } else {
                        self.select_only(i);
                        self.open_selected(false);
                    }
                    return;
                }
                if m.shift {
                    self.sel.range_to(&self.listing, i, ctrl);
                    self.show_cursor();
                } else if ctrl {
                    self.sel.toggle(&self.listing, i);
                    self.show_cursor();
                } else {
                    self.drag_from = Some((x, y));
                    if !self.sel.is_selected(self.listing.get(i).map(|e| e.id).unwrap_or(0)) || self.sel.count(&self.listing) <= 1 {
                        self.sel.only(&self.listing, i);
                    } else {
                        self.sel.cursor = Some(i);
                        self.sel.anchor = Some(i);
                    }
                    self.show_cursor();
                }
            }
            None => {
                // Empty space: start a rubber band (and clear unless Ctrl keeps the selection).
                if !(ctrl || m.shift) {
                    self.sel.clear();
                }
                let g = self.grid();
                if fy >= g.area[1] && fy < g.area[3] && fx >= g.area[0] && fx < g.area[2] {
                    self.rubber = Some(Rubber { start: (fx, fy), now: (fx, fy), scroll0: self.scroll, additive: ctrl || m.shift, base: self.sel.ids.clone() });
                }
                self.update_title();
                invalidate(self.hwnd);
            }
        }
    }

    fn click_in_box(&mut self, x: i32) {
        let px = (15.0 * self.dpi).round() as i32;
        let (rect, pad) = match self.active.as_ref().map(|a| &a.focus) {
            Some(Focus::Path) => (self.chrome.path_box, 8.0 * self.dpi),
            Some(Focus::Search) => (self.chrome.search_box, 8.0 * self.dpi),
            _ => return,
        };
        let rel = x as f32 - rect[0] - pad;
        let Some(a) = self.active.as_mut() else { return };
        // Which character boundary is closest to the click?
        let (mut best, mut best_d) = (0usize, f32::MAX);
        let chars = a.ti.chars.clone();
        for i in 0..=chars.len() {
            let prefix: String = chars[..i].iter().collect();
            let w = self.text.width(&prefix, px) as f32 - self.box_scroll(i == 0);
            let d = (w - rel).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        let extend = nv_win::modifiers().shift;
        if let Some(a) = self.active.as_mut() {
            a.ti.click_at(best, extend);
        }
        invalidate(self.hwnd);
    }

    /// Horizontal scroll of the text in a box (0: the text is shown from its start). Kept simple: the caret end is kept in view when drawing.
    fn box_scroll(&self, _at_start: bool) -> f32 {
        0.0
    }

    pub fn on_button(&mut self, b: Btn) {
        match b {
            Btn::Prev => self.move_cursor(-1),
            Btn::Next => self.move_cursor(1),
            Btn::Back => self.go_back(),
            Btn::Forward => self.go_forward(),
            Btn::Up => self.go_up(),
            Btn::ViewThumbs => self.set_mode(ViewMode::Thumbs),
            Btn::ViewDetails => self.set_mode(ViewMode::Details),
            Btn::ViewList => self.set_mode(ViewMode::List),
            Btn::Exit => self.exit(),
            Btn::PinTop => {
                self.panels.pin_top = !self.panels.pin_top;
                self.panels_changed();
            }
            Btn::PinLeft => {
                self.panels.pin_left = !self.panels.pin_left;
                self.panels_changed();
            }
            Btn::PinRight => {
                self.panels.pin_right = !self.panels.pin_right;
                self.panels_changed();
            }
            Btn::Sort | Btn::Show | Btn::Tools => self.toolbar_menu(b),
        }
    }

    fn on_right_click(&mut self, x: i32, y: i32) {
        self.last_mouse = (x, y);
        if let Some(i) = self.item_at(x, y) {
            let id = self.listing.get(i).map(|e| e.id).unwrap_or(0);
            if !self.sel.is_selected(id) {
                self.sel.only(&self.listing, i);
                self.show_cursor();
            } else {
                self.sel.cursor = Some(i);
            }
        } else if !self.chrome.over_panel(x as f32, y as f32) && !nv_win::modifiers().ctrl {
            // Right click on empty space: the menu is about the folder, not about a selection.
            self.sel.clear();
        }
        self.context_menu(x, y);
    }
}

fn contains_f(r: [f32; 4], x: f32, y: f32) -> bool {
    x >= r[0] && x < r[2] && y >= r[1] && y < r[3]
}

pub fn same_dir(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string();
    n(a) == n(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_compare_like_windows_does() {
        assert!(same_dir(Path::new("C:\\Pics\\"), Path::new("c:\\pics")));
        assert!(!same_dir(Path::new("C:\\Pics"), Path::new("C:\\Pics2")));
    }
}
