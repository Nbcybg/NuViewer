//! The folder tree as a file manager: click and keyboard, the right-click menu, cut / copy / paste / delete / rename / new folder
//! on a folder of the tree, dropping files onto it (from here or from Explorer), dragging files from the items onto a folder
//! with feedback, and the horizontal scroll bar for deep trees. The tree's rows and drawing are in `tree.rs` and `panels.rs`.

use crate::app::{same_dir, Active, App, Focus};
use crate::draw::TREE_ROW;
use crate::gfx::contains;
use crate::input::TextInput;
use crate::tree::is_within;
use nv_win::files::{self, Op};
use nv_win::menu::{popup, Entry as Item};
use nv_win::shell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const TIMER_DRAG: u32 = 8;

/// A drag of files from the items, inside the window, on its way to a folder.
pub struct DragState {
    pub paths: Vec<PathBuf>,
    /// The folder under the pointer that would take the files.
    pub target: Option<PathBuf>,
    /// When the pointer came to rest on a collapsed tree row (it opens after a moment, like Explorer's tree).
    pub hover: Option<(PathBuf, Instant)>,
    /// Ctrl / Shift decide copy or move; None = Explorer's rule (move on the same drive, copy to another).
    pub op: Option<Op>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TreeCmd {
    Open,
    Toggle,
    Reveal,
    Refresh,
    Cut,
    Copy,
    CopyPath,
    Paste,
    NewFolder,
    Rename,
    Delete,
    Properties,
}

fn drive_of(p: &Path) -> Option<char> {
    p.to_string_lossy().chars().next().map(|c| c.to_ascii_lowercase())
}

impl App {
    // ---- geometry ----------------------------------------------------------------------------------------

    /// The row of the tree under a window point, if it is on a folder row (not a section title).
    pub fn tree_row_at(&self, x: f32, y: f32) -> Option<usize> {
        let tr = self.tree_rect()?;
        if !contains(tr, x, y) {
            return None;
        }
        let i = ((y - tr[1] + self.tree.scroll) / (TREE_ROW * self.dpi)).floor();
        if i < 0.0 {
            return None;
        }
        let i = i as usize;
        self.tree.nodes.get(i).filter(|n| !n.header).map(|_| i)
    }

    fn tree_view_h(&self) -> f32 {
        self.tree_rect().map(|r| r[3] - r[1]).unwrap_or(400.0)
    }

    /// Scroll the tree so that row `i` is in view (vertically; and sideways when it is out of sight there).
    pub fn tree_show_row(&mut self, i: usize) {
        let (row_h, view_h) = (TREE_ROW * self.dpi, self.tree_view_h());
        self.tree.scroll_to_show(i, row_h, view_h);
        if let (Some(tr), Some(n)) = (self.tree_rect(), self.tree.nodes.get(i)) {
            let d = self.dpi;
            let x0 = 10.0 * d + n.depth as f32 * 16.0 * d;
            let w = tr[2] - tr[0];
            if x0 - self.tree.hscroll < 0.0 || x0 + 60.0 * d - self.tree.hscroll > w {
                self.tree.hscroll = (x0 - 40.0 * d).max(0.0);
            }
            self.tree.clamp_h(w);
        }
    }

    /// The folder (a tree row or a folder tile) that a drop at this window point would go into.
    pub fn drop_target_at(&self, x: i32, y: i32) -> Option<PathBuf> {
        let (fx, fy) = (x as f32, y as f32);
        if let Some(i) = self.tree_row_at(fx, fy) {
            return self.tree.nodes.get(i).map(|n| n.path.clone());
        }
        if self.chrome.over_panel(fx, fy) {
            return None;
        }
        self.item_at(x, y).and_then(|i| self.listing.get(i)).filter(|e| e.is_dir()).map(|e| e.path.clone())
    }

    // ---- mouse -------------------------------------------------------------------------------------------

    /// A left press in the tree.
    pub fn tree_press(&mut self, x: f32, y: f32, double: bool) {
        self.tree_focus = true;
        let Some(tr) = self.tree_rect() else { return };
        let d = self.dpi;
        let row_h = TREE_ROW * d;
        // The scroll bars: grab a thumb (or jump there by clicking the track).
        if let Some((track, thumb)) = self.tree.scrollbar(tr, row_h, d) {
            if contains([track[0] - 4.0 * d, track[1], track[2] + 3.0 * d, track[3]], x, y) {
                let grab = if contains(thumb, x, y) { y - thumb[1] } else { (thumb[3] - thumb[1]) / 2.0 };
                self.tree_sb_drag = Some(grab);
                self.tree.scroll_thumb_to(tr, row_h, d, y - grab);
                nv_win::invalidate(self.hwnd);
                return;
            }
        }
        if let Some((track, thumb)) = self.tree.hscrollbar(tr, d) {
            if contains([track[0], track[1] - 4.0 * d, track[2], track[3] + 3.0 * d], x, y) {
                let grab = if contains(thumb, x, y) { x - thumb[0] } else { (thumb[2] - thumb[0]) / 2.0 };
                self.tree_hsb_drag = Some(grab);
                self.tree.hscroll_thumb_to(tr, d, x - grab);
                nv_win::invalidate(self.hwnd);
                return;
            }
        }
        let Some(i) = self.tree_row_at(x, y) else { return };
        let n = &self.tree.nodes[i];
        let (path, depth, kids) = (n.path.clone(), n.depth, n.kids);
        self.tree_sel = Some(path.clone());
        let ix = tr[0] + 10.0 * d + depth as f32 * 16.0 * d - self.tree.hscroll;
        if kids && x < ix + 22.0 * d {
            self.tree.toggle(i);
        } else {
            if double && kids {
                self.tree.toggle(i);
            }
            // Clicking a row never moves the tree under the pointer: the row is where it was clicked.
            self.tree_for = path.clone();
            self.go(path, Vec::new(), true);
        }
        nv_win::invalidate(self.hwnd);
    }

    /// Right click in the tree: the menu of the folder under the pointer.
    pub fn tree_right_click(&mut self, x: i32, y: i32) {
        let Some(i) = self.tree_row_at(x as f32, y as f32) else { return };
        self.tree_focus = true;
        let path = self.tree.nodes[i].path.clone();
        self.tree_sel = Some(path.clone());
        nv_win::invalidate(self.hwnd);
        let (expanded, kids) = (self.tree.nodes[i].expanded, self.tree.nodes[i].kids);
        let paste_ok = files::clipboard_files(self.hwnd).is_some();
        let is_root = is_drive_root(&path);
        let mut cmds: Vec<TreeCmd> = Vec::new();
        let add = |cmds: &mut Vec<TreeCmd>, c: TreeCmd, text: &str| -> Item {
            cmds.push(c);
            Item::item(cmds.len() as u32, text)
        };
        let mut menu: Vec<Item> = vec![add(&mut cmds, TreeCmd::Open, "Open")];
        menu.push(add(&mut cmds, TreeCmd::Toggle, if expanded { "Collapse" } else { "Expand" }).enabled(kids || expanded));
        menu.push(add(&mut cmds, TreeCmd::Reveal, "Open in Explorer\tCtrl+E"));
        menu.push(add(&mut cmds, TreeCmd::Refresh, "Refresh\tF5"));
        menu.push(Item::Separator);
        menu.push(add(&mut cmds, TreeCmd::Cut, "Cut\tCtrl+X").enabled(!is_root));
        menu.push(add(&mut cmds, TreeCmd::Copy, "Copy\tCtrl+C").enabled(!is_root));
        menu.push(add(&mut cmds, TreeCmd::CopyPath, "Copy path\tCtrl+Shift+C"));
        menu.push(add(&mut cmds, TreeCmd::Paste, "Paste into this folder\tCtrl+V").enabled(paste_ok));
        menu.push(Item::Separator);
        menu.push(add(&mut cmds, TreeCmd::NewFolder, "New folder\tCtrl+N"));
        menu.push(add(&mut cmds, TreeCmd::Rename, "Rename\tF2").enabled(!is_root));
        menu.push(add(&mut cmds, TreeCmd::Delete, "Delete\tDel").enabled(!is_root));
        menu.push(Item::Separator);
        menu.push(add(&mut cmds, TreeCmd::Properties, "Properties"));
        if let Some(id) = popup(self.hwnd, x, y, &menu) {
            if let Some(&cmd) = cmds.get(id as usize - 1) {
                self.run_tree_cmd(cmd, &path);
            }
        }
    }

    fn run_tree_cmd(&mut self, cmd: TreeCmd, path: &Path) {
        match cmd {
            TreeCmd::Open => self.go(path.to_path_buf(), Vec::new(), true),
            TreeCmd::Toggle => {
                if let Some(i) = self.tree.nodes.iter().position(|n| !n.header && same_dir(&n.path, path)) {
                    self.tree.toggle(i);
                }
            }
            TreeCmd::Reveal => shell::reveal_in_explorer(path),
            TreeCmd::Refresh => {
                self.tree_reload(path);
                if is_within(&self.listing.dir, path) {
                    self.refresh(Vec::new());
                }
            }
            TreeCmd::Cut => self.copy_paths(&[path.to_path_buf()], true),
            TreeCmd::Copy => self.copy_paths(&[path.to_path_buf()], false),
            TreeCmd::CopyPath => {
                let _ = shell::clipboard_copy_text(self.hwnd, &path.display().to_string());
                self.toast("Path copied");
            }
            TreeCmd::Paste => self.paste_into(path.to_path_buf()),
            TreeCmd::NewFolder => {
                self.active = Some(Active { focus: Focus::NewFolder(path.to_path_buf()), ti: TextInput::selected("New folder"), error: None });
            }
            TreeCmd::Rename => self.tree_rename(path),
            TreeCmd::Delete => self.delete_paths(vec![path.to_path_buf()], false),
            TreeCmd::Properties => {
                if let Err(e) = files::properties(self.hwnd, path) {
                    self.toast(format!("Can't show the properties: {e}"));
                }
            }
        }
        nv_win::invalidate(self.hwnd);
    }

    fn tree_rename(&mut self, path: &Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.is_empty() {
            return;
        }
        self.active = Some(Active { focus: Focus::Rename(path.to_path_buf()), ti: TextInput::selected(&name), error: None });
    }

    // ---- keyboard ----------------------------------------------------------------------------------------

    /// The row the keyboard works on: the one clicked last, else the folder on screen.
    fn tree_current_row(&self) -> Option<usize> {
        let by = |p: &Path| self.tree.nodes.iter().position(|n| !n.header && same_dir(&n.path, p));
        self.tree_sel.as_deref().and_then(by).or_else(|| by(&self.listing.dir))
    }

    /// A key while the tree has the keyboard. True if it was used.
    pub fn tree_key(&mut self, vk: u32) -> bool {
        let m = nv_win::modifiers();
        let Some(i) = self.tree_current_row() else { return false };
        let path = self.tree.nodes[i].path.clone();
        let step = |from: usize, up: bool, nodes: &[crate::tree::Node]| -> Option<usize> {
            let mut k = from as isize + if up { -1 } else { 1 };
            while k >= 0 && (k as usize) < nodes.len() {
                if !nodes[k as usize].header {
                    return Some(k as usize);
                }
                k += if up { -1 } else { 1 };
            }
            None
        };
        let select_row = |app: &mut App, row: usize| {
            let p = app.tree.nodes[row].path.clone();
            app.tree_sel = Some(p.clone());
            app.tree_for = p.clone();
            app.tree_show_row(row);
            app.go(p, Vec::new(), true);
        };
        match vk {
            // Esc gives the keyboard back to the items.
            0x1B => {
                self.tree_focus = false;
            }
            0x26 if !m.alt => {
                if let Some(k) = step(i, true, &self.tree.nodes) {
                    select_row(self, k);
                }
            }
            0x28 if !m.alt => {
                if let Some(k) = step(i, false, &self.tree.nodes) {
                    select_row(self, k);
                }
            }
            0x25 if !m.alt => {
                // Left: close an open folder, else go to the folder it is in.
                if self.tree.nodes[i].expanded {
                    self.tree.collapse(i);
                } else if let Some(p) = (0..i).rev().find(|&k| !self.tree.nodes[k].header && self.tree.nodes[k].depth + 1 == self.tree.nodes[i].depth) {
                    select_row(self, p);
                }
            }
            0x27 if !m.alt => {
                if !self.tree.nodes[i].expanded && self.tree.nodes[i].kids {
                    self.tree.expand(i);
                } else if self.tree.nodes[i].expanded && i + 1 < self.tree.nodes.len() && self.tree.nodes[i + 1].depth > self.tree.nodes[i].depth {
                    select_row(self, i + 1);
                }
            }
            0x24 => {
                if let Some(k) = step(0, false, &self.tree.nodes).or(Some(0)).filter(|&k| !self.tree.nodes[k].header) {
                    select_row(self, k);
                }
            }
            0x23 => {
                let last = self.tree.nodes.len().saturating_sub(1);
                if !self.tree.nodes.is_empty() {
                    select_row(self, last);
                }
            }
            0x0D => {
                self.tree.toggle(i);
                self.go(path, Vec::new(), true);
            }
            0x71 => self.tree_rename(&path),
            0x2E if is_drive_root(&path) => self.toast("A drive can't be deleted here"),
            0x2E => self.delete_paths(vec![path], m.shift),
            0x74 => self.run_tree_cmd(TreeCmd::Refresh, &path),
            0x43 if m.ctrl && m.shift => self.run_tree_cmd(TreeCmd::CopyPath, &path),
            0x43 | 0x58 if m.ctrl && is_drive_root(&path) => self.toast("A whole drive can't be copied or cut here"),
            0x43 if m.ctrl => self.copy_paths(&[path], false),
            0x58 if m.ctrl => self.copy_paths(&[path], true),
            0x56 if m.ctrl => self.paste_into(path),
            0x4E if m.ctrl => self.run_tree_cmd(TreeCmd::NewFolder, &path),
            0x45 if m.ctrl => shell::reveal_in_explorer(&path),
            _ => return false,
        }
        nv_win::invalidate(self.hwnd);
        true
    }

    // ---- changes to the tree after file operations -------------------------------------------------------

    /// Read the sub-folders of `path` again (a folder was made, renamed, moved or deleted there), keeping the folders that
    /// were open below it open.
    pub fn tree_reload(&mut self, path: &Path) {
        let Some(i) = self.tree.nodes.iter().position(|n| !n.header && same_dir(&n.path, path)) else { return };
        if !self.tree.nodes[i].expanded {
            // Not open: it only needs to forget what it knew about having sub-folders.
            self.tree.nodes[i].loaded = false;
            self.tree.nodes[i].kids = true;
            return;
        }
        let depth = self.tree.nodes[i].depth;
        let open: Vec<PathBuf> = self.tree.nodes[i + 1..].iter().take_while(|n| n.header || n.depth > depth).filter(|n| n.expanded).map(|n| n.path.clone()).collect();
        self.tree.collapse(i);
        self.tree.expand(i);
        for p in open {
            if let Some(k) = self.tree.nodes.iter().position(|n| !n.header && same_dir(&n.path, &p)) {
                self.tree.expand(k);
            }
        }
        nv_win::invalidate(self.hwnd);
    }

    // ---- clipboard and delete on any paths ---------------------------------------------------------------

    pub fn copy_paths(&mut self, paths: &[PathBuf], cut: bool) {
        match files::clipboard_set_files(self.hwnd, paths, cut) {
            Ok(()) => {
                self.cut = if cut { paths.to_vec() } else { Vec::new() };
                self.toast(format!("{} {} item{} — paste with Ctrl+V here or in Explorer", if cut { "Cut" } else { "Copied" }, paths.len(), if paths.len() == 1 { "" } else { "s" }));
            }
            Err(e) => self.toast(format!("Couldn't use the clipboard: {e}")),
        }
    }

    /// Paste what is on the clipboard into `dir`.
    pub fn paste_into(&mut self, dir: PathBuf) {
        let Some((paths, cut)) = files::clipboard_files(self.hwnd) else {
            self.toast("There are no files on the clipboard");
            return;
        };
        if paths.iter().any(|p| is_within(&dir, p) && p.is_dir()) {
            self.toast("A folder can't be pasted into itself");
            return;
        }
        let same_folder = paths.iter().all(|p| p.parent().map(|d| same_dir(d, &dir)).unwrap_or(false));
        if cut && same_folder {
            return;
        }
        let existed = crate::undo::names_in(&dir);
        match files::run(self.hwnd, if cut { Op::Move } else { Op::Copy }, &paths, Some(&dir), same_folder) {
            Ok(()) => {
                if !same_folder {
                    self.record_transfer(&paths, &dir, cut, &existed);
                }
                let here = same_dir(&dir, &self.listing.dir);
                let select: Vec<PathBuf> = if same_folder || !here { Vec::new() } else { paths.iter().filter_map(|p| p.file_name().map(|n| dir.join(n))).collect() };
                self.toast(format!("Pasted {} item{} into {}", paths.len(), if paths.len() == 1 { "" } else { "s" }, dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string())));
                let mut touched: Vec<PathBuf> = vec![dir.clone()];
                if cut {
                    touched.extend(paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)));
                }
                for t in &touched {
                    self.tree_reload(t);
                }
                self.refresh(select);
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't paste: {e}")),
        }
    }

    /// Delete `paths` (to the Recycle Bin, or for good). When the folder on screen is one of them or inside one, the view
    /// first goes up to the folder they are in.
    pub fn delete_paths(&mut self, paths: Vec<PathBuf>, permanent: bool) {
        if paths.is_empty() {
            return;
        }
        if paths.iter().any(|p| is_drive_root(p)) {
            self.toast("A drive can't be deleted here");
            return;
        }
        let n = paths.len();
        let name = if n == 1 { paths[0].file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default() } else { format!("{n} items") };
        if permanent && !shell::confirm(self.hwnd, &format!("Delete {name} permanently?\n\nThis can't be undone.")) {
            return;
        }
        if !permanent && self.settings.confirm_delete {
            let what = if n == 1 { format!("\"{name}\"") } else { format!("these {n} items") };
            if !shell::confirm_yes(self.hwnd, &format!("Move {what} to the Recycle Bin?\n\nCtrl+Z brings {} back.", if n == 1 { "it" } else { "them" })) {
                return;
            }
        }
        // Leave a folder before it is deleted (Windows will not remove the folder a program is showing).
        let parents: Vec<PathBuf> = paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
        let inside = paths.iter().find(|p| is_within(&self.listing.dir, p)).cloned();
        if let Some(gone) = &inside {
            if let Some(up) = gone.parent() {
                self.go(up.to_path_buf(), Vec::new(), true);
            }
        }
        match files::run(self.hwnd, if permanent { Op::Delete } else { Op::Recycle }, &paths, None, false) {
            Ok(()) => {
                if !permanent {
                    let gone = crate::undo::gone(&paths);
                    self.push_undo(crate::undo::Step::Deleted(gone));
                }
                self.toast(if permanent { format!("Deleted {n} item{}", if n == 1 { "" } else { "s" }) } else { format!("Moved {n} item{} to the Recycle Bin — Ctrl+Z to undo", if n == 1 { "" } else { "s" }) });
                for p in &parents {
                    self.tree_reload(p);
                }
                if inside.is_none() {
                    self.refresh(Vec::new());
                }
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't delete: {e}")),
        }
    }

    // ---- dragging files onto a folder ---------------------------------------------------------------------

    /// The pointer moved far enough with the button down on a selected item: files are being dragged. Inside the window the
    /// drag is ours (so the folder under the pointer can be lit up); once the pointer leaves, Windows' own drag takes over.
    pub fn begin_drag(&mut self) {
        let paths = self.picked_paths();
        if paths.is_empty() || paths.iter().any(|p| p.parent().map(|d| !same_dir(d, &self.listing.dir)).unwrap_or(true)) {
            return;
        }
        self.drag = Some(DragState { paths, target: None, hover: None, op: None });
        nv_win::set_timer(self.hwnd, TIMER_DRAG, 120);
        let (x, y) = self.last_mouse;
        self.drag_update(x, y);
    }

    /// Where the dragged files would go if they were dropped now.
    pub fn drag_update(&mut self, x: i32, y: i32) {
        let Some(paths) = self.drag.as_ref().map(|d| d.paths.clone()) else { return };
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            // Out of the window: hand over to Windows' drag and drop (Explorer, mail, chat ...).
            self.drag = None;
            nv_win::kill_timer(self.hwnd, TIMER_DRAG);
            self.start_drag();
            return;
        }
        let m = nv_win::modifiers();
        let op = if m.ctrl { Some(Op::Copy) } else if m.shift { Some(Op::Move) } else { None };
        let target = self.drop_target_at(x, y).filter(|t| {
            let already_there = paths.iter().all(|p| p.parent().map(|d| same_dir(d, t)).unwrap_or(false));
            let into_itself = paths.iter().any(|p| is_within(t, p));
            !(already_there && op != Some(Op::Copy)) && !into_itself
        });
        let Some(d) = self.drag.as_mut() else { return };
        let changed = d.target != target;
        if changed {
            d.hover = target.clone().map(|t| (t, Instant::now()));
        }
        d.target = target;
        d.op = op;
        if changed && self.test_mode {
            self.update_title();
        }
        // Near the top or bottom of the tree it scrolls.
        if let Some(tr) = self.tree_rect() {
            let (fx, fy, dd) = (x as f32, y as f32, self.dpi);
            if contains(tr, fx, fy) {
                let row_h = TREE_ROW * dd;
                if fy < tr[1] + row_h * 0.8 {
                    self.tree.scroll -= row_h * 0.5;
                } else if fy > tr[3] - row_h * 0.8 {
                    self.tree.scroll += row_h * 0.5;
                }
                self.tree.clamp(row_h, tr[3] - tr[1]);
            }
        }
        nv_win::invalidate(self.hwnd);
    }

    /// The timer of a drag: a collapsed tree row opens when the pointer rests on it.
    pub fn drag_tick(&mut self) {
        let Some(d) = self.drag.as_ref() else {
            nv_win::kill_timer(self.hwnd, TIMER_DRAG);
            return;
        };
        if let Some((p, since)) = d.hover.clone() {
            if since.elapsed() >= Duration::from_millis(700) {
                if let Some(i) = self.tree.nodes.iter().position(|n| !n.header && same_dir(&n.path, &p)) {
                    if !self.tree.nodes[i].expanded && self.tree.nodes[i].kids {
                        self.tree.expand(i);
                    }
                }
                if let Some(d) = self.drag.as_mut() {
                    d.hover = None;
                }
                nv_win::invalidate(self.hwnd);
            }
        }
        let (x, y) = self.last_mouse;
        self.drag_update(x, y);
    }

    /// The button went up: drop the files on the folder that was lit up.
    pub fn finish_drag(&mut self) {
        nv_win::kill_timer(self.hwnd, TIMER_DRAG);
        let Some(d) = self.drag.take() else { return };
        if let Some(t) = d.target {
            self.drop_paths(d.paths, t, d.op);
        }
        nv_win::invalidate(self.hwnd);
    }

    /// Move or copy `paths` into `target`. Like Explorer: moved within a drive, copied between drives; Ctrl forces a copy,
    /// Shift a move.
    pub fn drop_paths(&mut self, paths: Vec<PathBuf>, target: PathBuf, forced: Option<Op>) {
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| !is_within(&target, p) && (p.parent().map(|d| !same_dir(d, &target)).unwrap_or(true) || forced == Some(Op::Copy))).collect();
        if paths.is_empty() {
            self.toast("Those files are already in that folder");
            return;
        }
        let op = forced.unwrap_or(if drive_of(&paths[0]) == drive_of(&target) { Op::Move } else { Op::Copy });
        let same_folder = paths.iter().all(|p| p.parent().map(|d| same_dir(d, &target)).unwrap_or(false));
        let n = paths.len();
        let existed = crate::undo::names_in(&target);
        match files::run(self.hwnd, op, &paths, Some(&target), same_folder) {
            Ok(()) => {
                if !same_folder {
                    self.record_transfer(&paths, &target, op == Op::Move, &existed);
                }
                let to = target.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| target.display().to_string());
                self.toast(format!("{} {n} item{} to {to} — Ctrl+Z to undo", if op == Op::Move { "Moved" } else { "Copied" }, if n == 1 { "" } else { "s" }));
                let mut touched = vec![target.clone()];
                if op == Op::Move {
                    touched.extend(paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)));
                }
                for t in &touched {
                    self.tree_reload(t);
                }
                self.refresh(Vec::new());
            }
            Err(e) if e == "cancelled" => self.refresh(Vec::new()),
            Err(e) => self.toast(format!("Couldn't {}: {e}", if op == Op::Move { "move" } else { "copy" })),
        }
    }

    /// Files dropped on the window from Explorer or another program: into the folder under the pointer (a tree row or a
    /// folder tile), else into the folder on screen.
    pub fn on_drop(&mut self, dropped: Vec<String>, x: i32, y: i32) {
        let target = self.drop_target_at(x, y).unwrap_or_else(|| self.listing.dir.clone());
        let m = nv_win::modifiers();
        let forced = if m.ctrl { Some(Op::Copy) } else if m.shift { Some(Op::Move) } else { None };
        let paths: Vec<PathBuf> = dropped.into_iter().map(PathBuf::from).collect();
        self.drop_paths(paths, target, forced);
    }
}

/// "C:\", "D:" ...: a drive, not a folder that can be cut, deleted or renamed here.
pub fn is_drive_root(p: &Path) -> bool {
    p.parent().is_none() || p.to_string_lossy().trim_end_matches('\\').ends_with(':')
}

/// Convenience for the rename of a folder that the view is in or below: the path after the rename.
pub fn renamed_view(current: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    if !is_within(current, old) {
        return None;
    }
    let rest = current.strip_prefix(old).ok()?;
    Some(if rest.as_os_str().is_empty() { new.to_path_buf() } else { new.join(rest) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_folder_takes_the_view_with_it() {
        let (old, new) = (Path::new("C:\\a\\x"), Path::new("C:\\a\\y"));
        assert_eq!(renamed_view(Path::new("C:\\a\\x"), old, new), Some(PathBuf::from("C:\\a\\y")));
        assert_eq!(renamed_view(Path::new("C:\\a\\x\\deep\\er"), old, new), Some(PathBuf::from("C:\\a\\y\\deep\\er")));
        assert_eq!(renamed_view(Path::new("C:\\a\\xx"), old, new), None);
    }
}
