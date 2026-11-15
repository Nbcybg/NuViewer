//! The folder tree on the left edge: quick-access folders and the drives, opened level by level as the user expands
//! them. Kept as a flat list of the rows that are visible (expanding inserts the children after the row, collapsing removes
//! them), which is also exactly what gets drawn.

use nv_core::folder::natural_cmp;
use nv_win::files::DriveKind;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Folder,
    Drive,
    Removable,
    Network,
    Pictures,
    Desktop,
    Downloads,
    Documents,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub depth: u8,
    pub expanded: bool,
    pub loaded: bool,
    /// May have sub-folders (unknown until it is opened once).
    pub kids: bool,
    /// A section title ("Quick access", "This PC"), not a folder.
    pub header: bool,
    pub icon: Icon,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    /// Pixels scrolled.
    pub scroll: f32,
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string()
}

/// Is `inner` the same as, or below, `outer`?
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let (i, o) = (norm(inner), norm(outer));
    i == o || (i.starts_with(&o) && i[o.len()..].starts_with('\\')) || (o.ends_with(':') && i.starts_with(&o))
}

/// Sub-folders of `dir` (hidden and system ones left out), in natural name order.
pub fn subfolders(dir: &Path) -> Vec<(PathBuf, String)> {
    use std::os::windows::fs::MetadataExt;
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(PathBuf, String)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let dir = m.is_dir() || (m.file_type().is_symlink() && std::fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false));
            (dir && m.file_attributes() & 0x6 == 0).then(|| (e.path(), e.file_name().to_string_lossy().into_owned()))
        })
        .collect();
    out.sort_by(|a, b| natural_cmp(&a.1, &b.1).then_with(|| a.1.cmp(&b.1)));
    out
}

impl Tree {
    /// No rows yet: the real tree is built on a worker thread (`Tree::new` asks Windows for the known folders and checks that they
    /// exist, which can take a while on a cold start with a OneDrive or network folder) and handed over when it is ready.
    pub fn empty() -> Tree {
        Tree { nodes: Vec::new(), scroll: 0.0 }
    }

    /// The usual roots: Quick access and the drives.
    pub fn new() -> Tree {
        let mut nodes = vec![header("Quick access")];
        for (key, name, icon) in [("pictures", "Pictures", Icon::Pictures), ("desktop", "Desktop", Icon::Desktop), ("downloads", "Downloads", Icon::Downloads), ("documents", "Documents", Icon::Documents)] {
            if let Some(p) = nv_win::files::known_folder(key).filter(|p| p.is_dir()) {
                nodes.push(Node { path: p, name: name.into(), depth: 0, expanded: false, loaded: false, kids: true, header: false, icon });
            }
        }
        nodes.push(header("This PC"));
        for (root, kind) in nv_win::files::drives() {
            let letter = root.to_string_lossy().trim_end_matches('\\').to_string();
            let (label, icon) = match kind {
                DriveKind::Fixed => (format!("Local disk ({letter})"), Icon::Drive),
                DriveKind::Removable => (format!("Removable ({letter})"), Icon::Removable),
                DriveKind::Network => (format!("Network ({letter})"), Icon::Network),
                DriveKind::Optical => (format!("Disc ({letter})"), Icon::Removable),
                DriveKind::Other => (format!("Drive ({letter})"), Icon::Drive),
            };
            nodes.push(Node { path: root, name: label, depth: 0, expanded: false, loaded: false, kids: true, header: false, icon });
        }
        Tree { nodes, scroll: 0.0 }
    }

    /// The drive's volume label arrived (read on a worker thread): show it the way Explorer does, "Games (D:)".
    pub fn set_drive_label(&mut self, root: &Path, label: &str) {
        let letter = root.to_string_lossy().trim_end_matches('\\').to_string();
        if let Some(n) = self.nodes.iter_mut().find(|n| !n.header && n.depth == 0 && matches!(n.icon, Icon::Drive | Icon::Removable | Icon::Network) && norm(&n.path) == norm(root)) {
            n.name = format!("{label} ({letter})");
        }
    }

    /// Where the tree's scroll bar is for a view `view` (window pixels: left, top, right, bottom) with rows `row_h` high:
    /// (track, thumb), or None when every row fits.
    pub fn scrollbar(&self, view: [f32; 4], row_h: f32, d: f32) -> Option<([f32; 4], [f32; 4])> {
        let content = self.nodes.len() as f32 * row_h;
        let h = view[3] - view[1];
        if content <= h + 0.5 || h <= 0.0 {
            return None;
        }
        let track = [view[2] - 11.0 * d, view[1], view[2] - 3.0 * d, view[3]];
        let thumb_h = (h * h / content).clamp(30.0 * d, h);
        let max = content - h;
        let y = track[1] + (self.scroll / max).clamp(0.0, 1.0) * (h - thumb_h);
        Some((track, [track[0], y, track[2], y + thumb_h]))
    }

    /// Put the top of the scroll thumb at window y `thumb_top` (dragging it).
    pub fn scroll_thumb_to(&mut self, view: [f32; 4], row_h: f32, d: f32, thumb_top: f32) {
        let Some((track, thumb)) = self.scrollbar(view, row_h, d) else { return };
        let range = (track[3] - track[1]) - (thumb[3] - thumb[1]);
        if range <= 0.0 {
            return;
        }
        let max = self.nodes.len() as f32 * row_h - (view[3] - view[1]);
        self.scroll = ((thumb_top - track[1]) / range).clamp(0.0, 1.0) * max;
    }

    /// A tree with the given roots (tests).
    #[cfg(test)]
    pub fn with_roots(roots: &[PathBuf]) -> Tree {
        Tree { nodes: roots.iter().map(|p| Node { path: p.clone(), name: p.display().to_string(), depth: 0, expanded: false, loaded: false, kids: true, header: false, icon: Icon::Folder }).collect(), scroll: 0.0 }
    }

    /// Open row `i`: read its sub-folders and show them below it.
    pub fn expand(&mut self, i: usize) {
        let Some(n) = self.nodes.get(i) else { return };
        if n.header || n.expanded {
            return;
        }
        let (path, depth) = (n.path.clone(), n.depth);
        let kids = subfolders(&path);
        let new: Vec<Node> = kids
            .into_iter()
            .map(|(p, name)| Node { path: p, name, depth: depth + 1, expanded: false, loaded: false, kids: true, header: false, icon: Icon::Folder })
            .collect();
        let n = &mut self.nodes[i];
        n.expanded = true;
        n.loaded = true;
        n.kids = !new.is_empty();
        let at = i + 1;
        self.nodes.splice(at..at, new);
    }

    pub fn collapse(&mut self, i: usize) {
        let Some(n) = self.nodes.get(i) else { return };
        if !n.expanded {
            return;
        }
        let depth = n.depth;
        let end = self.nodes[i + 1..].iter().position(|m| m.header || m.depth <= depth).map(|k| i + 1 + k).unwrap_or(self.nodes.len());
        self.nodes.drain(i + 1..end);
        self.nodes[i].expanded = false;
    }

    pub fn toggle(&mut self, i: usize) {
        match self.nodes.get(i) {
            Some(n) if n.expanded => self.collapse(i),
            Some(_) => self.expand(i),
            None => {}
        }
    }

    /// Open the tree down to `target` and return its row. None if it can't be found (a hidden folder, another root).
    pub fn reveal(&mut self, target: &Path) -> Option<usize> {
        for _ in 0..128 {
            let best = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| !n.header && is_within(target, &n.path))
                .max_by_key(|(_, n)| norm(&n.path).len())
                .map(|(i, n)| (i, norm(&n.path) == norm(target), n.expanded));
            match best {
                None => return None,
                Some((i, true, _)) => return Some(i),
                Some((_, false, true)) => return None, // open, and the next level isn't in it
                Some((i, false, false)) => self.expand(i),
            }
        }
        None
    }

    /// Scroll so row `i` (of height `row_h`) is inside a view `view_h` tall.
    pub fn scroll_to_show(&mut self, i: usize, row_h: f32, view_h: f32) {
        let (top, bottom) = (i as f32 * row_h, (i + 1) as f32 * row_h);
        if top < self.scroll {
            self.scroll = top;
        } else if bottom > self.scroll + view_h {
            self.scroll = bottom - view_h;
        }
        self.clamp(row_h, view_h);
    }

    pub fn clamp(&mut self, row_h: f32, view_h: f32) {
        let max = (self.nodes.len() as f32 * row_h - view_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }
}

fn header(name: &str) -> Node {
    Node { path: PathBuf::new(), name: name.into(), depth: 0, expanded: true, loaded: true, kids: false, header: true, icon: Icon::Folder }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_tree() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_tree_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for sub in ["a\\x", "a\\y", "b", "Zed\\deep\\deeper"] {
            fs::create_dir_all(d.join(sub)).unwrap();
        }
        fs::write(d.join("file.txt"), "x").unwrap();
        d
    }

    #[test]
    fn expanding_and_collapsing_keeps_the_rows_in_order() {
        let d = temp_tree();
        let mut t = Tree::with_roots(&[d.clone()]);
        t.expand(0);
        let names: Vec<&str> = t.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(&names[1..], ["a", "b", "Zed"], "folders only, natural order ignoring case");
        t.expand(1);
        assert_eq!(t.nodes.len(), 6);
        assert_eq!(t.nodes[2].depth, 2);
        t.collapse(0);
        assert_eq!(t.nodes.len(), 1);
        t.toggle(0);
        assert_eq!(t.nodes.len(), 4, "children are read again when it is reopened");
        t.expand(2); // "b" has no sub-folders
        assert!(!t.nodes[2].kids);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn reveal_opens_the_path_down_to_the_folder() {
        let d = temp_tree();
        let mut t = Tree::with_roots(&[d.clone()]);
        let i = t.reveal(&d.join("zed").join("DEEP").join("deeper")).expect("case differences don't matter");
        assert_eq!(t.nodes[i].name, "deeper");
        assert_eq!(t.nodes[i].depth, 3);
        assert!(t.reveal(&d.join("nope")).is_none());
        assert!(t.reveal(Path::new("Q:\\elsewhere")).is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn within_checks_whole_components() {
        assert!(is_within(Path::new("C:\\a\\b"), Path::new("C:\\a")));
        assert!(is_within(Path::new("c:\\A"), Path::new("C:\\a\\")));
        assert!(is_within(Path::new("C:\\a"), Path::new("C:\\")));
        assert!(!is_within(Path::new("C:\\ab"), Path::new("C:\\a")));
        assert!(!is_within(Path::new("D:\\a"), Path::new("C:\\")));
    }

    #[test]
    fn the_real_tree_has_the_drives() {
        let t = Tree::new();
        assert!(t.nodes.iter().any(|n| n.header && n.name == "This PC"));
        assert!(t.nodes.iter().any(|n| matches!(n.icon, Icon::Drive)), "at least the system drive");
    }

    #[test]
    fn a_drive_shows_its_volume_label_before_the_letter() {
        let mut t = Tree::new();
        let root = t.nodes.iter().find(|n| matches!(n.icon, Icon::Drive)).map(|n| n.path.clone()).expect("a fixed drive");
        t.set_drive_label(&root, "Photos");
        let letter = root.to_string_lossy().trim_end_matches('\\').to_string();
        assert!(t.nodes.iter().any(|n| n.name == format!("Photos ({letter})")));
    }

    #[test]
    fn the_scroll_bar_exists_only_when_rows_overflow_and_follows_the_scroll() {
        let mut t = Tree::with_roots(&(0..50).map(|i| PathBuf::from(format!("C:\\{i}"))).collect::<Vec<_>>());
        let view = [0.0, 100.0, 300.0, 300.0];
        let (track, thumb) = t.scrollbar(view, 20.0, 1.0).expect("50 rows of 20 do not fit in 200");
        assert!(thumb[1] == track[1] && thumb[3] < track[3]);
        t.scroll = 1e9;
        t.clamp(20.0, 200.0);
        let (track, thumb) = t.scrollbar(view, 20.0, 1.0).unwrap();
        assert!((thumb[3] - track[3]).abs() < 0.5, "scrolled to the end: the thumb is at the bottom");
        // Dragging the thumb to the middle of the track scrolls about half way.
        t.scroll_thumb_to(view, 20.0, 1.0, track[1] + ((track[3] - track[1]) - (thumb[3] - thumb[1])) / 2.0);
        assert!((t.scroll - (1000.0 - 200.0) / 2.0).abs() < 1.0, "{}", t.scroll);
        let few = Tree::with_roots(&[PathBuf::from("C:\\a")]);
        assert!(few.scrollbar(view, 20.0, 1.0).is_none());
    }

    #[test]
    fn scrolling_follows_a_row() {
        let mut t = Tree::with_roots(&(0..50).map(|i| PathBuf::from(format!("C:\\{i}"))).collect::<Vec<_>>());
        t.scroll_to_show(40, 20.0, 200.0);
        assert!(t.scroll > 0.0 && 40.0 * 20.0 >= t.scroll && 41.0 * 20.0 <= t.scroll + 200.0);
        t.scroll_to_show(0, 20.0, 200.0);
        assert_eq!(t.scroll, 0.0);
    }
}
