//! The state of a layer panel, for every kind of layered file (PSD, XCF, OpenRaster now; Spine slots and Live2D parts later).
//!
//! Pure code: nothing here knows about pixels or windows. A source fills `nodes` (name, parent, group, what the file says); the
//! panel edits `on` / `opacity` and the viewer asks for the result as one `Eff` per layer (`nv_codecs::layerdoc::compose`).
//!
//!  * **filter**: case-insensitive substring of the name; a match keeps its parents, and a matching group keeps everything inside it;
//!  * **hover**: the row under the pointer is shown at full strength, everything else is dimmed (`DIM`), even if that layer is off;
//!  * **solo**: only one layer (or group) is drawn;
//!  * opacity of a group multiplies the opacity of what is inside.

use nv_codecs::layerdoc::{Eff, LayerDoc};

/// How much of every other layer stays visible while one is highlighted.
pub const DIM: f32 = 0.18;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub group: bool,
    /// What the file says (the "as opened" state).
    pub on0: bool,
    pub opacity0: f32,
    /// What the user chose.
    pub on: bool,
    pub opacity: f32,
    /// Where the layer is on the canvas (x0, y0, x1, y1); None for groups and layers without pixels.
    pub bounds: Option<[i32; 4]>,
    /// "Multiply" ... shown as a hint; empty for normal.
    pub mode: String,
}

#[derive(Clone, Debug, Default)]
pub struct LayerSet {
    pub nodes: Vec<Node>,
    /// Row under the pointer.
    pub hover: Option<usize>,
    pub solo: Option<usize>,
    pub filter: String,
    /// Groups folded shut in the panel.
    pub collapsed: Vec<bool>,
}

/// One line of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub index: usize,
    pub depth: u32,
}

impl LayerSet {
    pub fn from_doc(doc: &LayerDoc) -> LayerSet {
        let nodes: Vec<Node> = doc
            .layers
            .iter()
            .map(|l| Node {
                name: l.name.clone(),
                parent: l.parent,
                group: l.group,
                on0: l.visible,
                opacity0: l.opacity,
                on: l.visible,
                opacity: l.opacity,
                bounds: (!l.group && l.w > 0 && l.h > 0).then(|| l.bounds()),
                mode: if l.mode == nv_codecs::layerdoc::Blend::Normal { String::new() } else { l.mode.name().to_string() },
            })
            .collect();
        let n = nodes.len();
        LayerSet { nodes, hover: None, solo: None, filter: String::new(), collapsed: vec![false; n] }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Indices `i` and everything inside it (children come after their group, so one pass is enough).
    pub fn subtree(&self, i: usize) -> Vec<usize> {
        let mut inside = vec![false; self.nodes.len()];
        if i >= inside.len() {
            return Vec::new();
        }
        inside[i] = true;
        for k in i + 1..self.nodes.len() {
            if let Some(p) = self.nodes[k].parent {
                if inside[p] {
                    inside[k] = true;
                }
            }
        }
        (0..inside.len()).filter(|&k| inside[k]).collect()
    }

    pub fn depth(&self, mut i: usize) -> u32 {
        let mut d = 0;
        while let Some(p) = self.nodes.get(i).and_then(|n| n.parent) {
            d += 1;
            i = p;
            if d > 64 {
                break;
            }
        }
        d
    }

    /// Is `a` inside `b` (or `b` itself)?
    fn within(&self, mut a: usize, b: usize) -> bool {
        for _ in 0..=64 {
            if a == b {
                return true;
            }
            match self.nodes.get(a).and_then(|n| n.parent) {
                Some(p) => a = p,
                None => return false,
            }
        }
        false
    }

    /// What every layer contributes now. `highlight` applies the hover (the preview on screen); export passes false.
    pub fn eff(&self, highlight: bool) -> Vec<Eff> {
        let n = self.nodes.len();
        let mut out: Vec<Eff> = Vec::with_capacity(n);
        let hover = if highlight { self.hover.filter(|&h| h < n) } else { None };
        for i in 0..n {
            let node = &self.nodes[i];
            let up = node.parent.filter(|&p| p < i).map(|p| out[p]).unwrap_or(Eff { on: true, opacity: 1.0 });
            let mut e = Eff { on: up.on && node.on, opacity: up.opacity * node.opacity };
            if let Some(s) = self.solo.filter(|&s| s < n) {
                // The soloed layer (and what is inside it) is drawn whatever its switch says; its parents only lend their opacity.
                e.on = self.within(i, s);
                if e.on {
                    e.opacity = up.opacity * node.opacity;
                }
            }
            if let Some(h) = hover {
                if !self.within(i, h) {
                    e.opacity *= DIM;
                }
            }
            out.push(e);
        }
        // Hovering a layer that is switched off shows it at its own opacity (a preview), not at nothing.
        if let Some(h) = hover {
            for i in 0..n {
                if self.within(i, h) {
                    let mut op = 1.0;
                    let mut k = Some(i);
                    let mut guard = 0;
                    while let Some(x) = k {
                        op *= self.nodes[x].opacity;
                        k = self.nodes[x].parent;
                        guard += 1;
                        if guard > 64 {
                            break;
                        }
                    }
                    out[i].on = true;
                    out[i].opacity = op.max(0.0);
                }
            }
        }
        out
    }

    /// The lines the panel shows, top to bottom: filtered, and without the insides of folded groups.
    pub fn rows(&self) -> Vec<Row> {
        let n = self.nodes.len();
        let f = self.filter.trim().to_lowercase();
        let mut show = vec![true; n];
        if !f.is_empty() {
            let hit: Vec<bool> = self.nodes.iter().map(|x| x.name.to_lowercase().contains(&f)).collect();
            show = vec![false; n];
            for i in 0..n {
                if hit[i] {
                    // the match, its parents, and (for a group) everything inside
                    show[i] = true;
                    let mut k = self.nodes[i].parent;
                    let mut guard = 0;
                    while let Some(p) = k {
                        show[p] = true;
                        k = self.nodes[p].parent;
                        guard += 1;
                        if guard > 64 {
                            break;
                        }
                    }
                    if self.nodes[i].group {
                        for j in self.subtree(i) {
                            show[j] = true;
                        }
                    }
                }
            }
        }
        let folded = |mut i: usize| -> bool {
            // inside a folded group?
            let mut guard = 0;
            while let Some(p) = self.nodes[i].parent {
                if self.collapsed.get(p).copied().unwrap_or(false) {
                    return true;
                }
                i = p;
                guard += 1;
                if guard > 64 {
                    break;
                }
            }
            false
        };
        (0..n).filter(|&i| show[i] && (!f.is_empty() || !folded(i))).map(|i| Row { index: i, depth: self.depth(i) }).collect()
    }

    /// Switch a layer on / off. A group switches as one.
    pub fn toggle(&mut self, i: usize) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.on = !n.on;
        }
    }

    pub fn set_opacity(&mut self, i: usize, v: f32) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.opacity = v.clamp(0.0, 1.0);
        }
    }

    /// Double click on a slider: back to what the file says. Triple click: nothing (opacity 0).
    pub fn reset_one(&mut self, i: usize) {
        if let Some(n) = self.nodes.get_mut(i) {
            n.on = n.on0;
            n.opacity = n.opacity0;
        }
    }

    pub fn clear_one(&mut self, i: usize) {
        self.set_opacity(i, 0.0);
    }

    pub fn toggle_fold(&mut self, i: usize) {
        if let Some(c) = self.collapsed.get_mut(i) {
            *c = !*c;
        }
    }

    pub fn toggle_solo(&mut self, i: usize) {
        self.solo = if self.solo == Some(i) { None } else { Some(i) };
    }

    /// Every layer on (`true`) or off (`false`).
    pub fn set_all(&mut self, on: bool) {
        for n in &mut self.nodes {
            n.on = on;
        }
    }

    /// Back to what the file says, solo and filter cleared.
    pub fn reset_all(&mut self) {
        for n in &mut self.nodes {
            n.on = n.on0;
            n.opacity = n.opacity0;
        }
        self.solo = None;
        self.hover = None;
        self.filter.clear();
    }

    /// Has the user changed anything from the file?
    pub fn modified(&self) -> bool {
        self.solo.is_some() || self.nodes.iter().any(|n| n.on != n.on0 || (n.opacity - n.opacity0).abs() > 1e-4)
    }

    /// How many layers (not groups) are drawn right now.
    pub fn shown_count(&self) -> usize {
        let e = self.eff(false);
        self.nodes.iter().zip(&e).filter(|(n, e)| !n.group && e.on && e.opacity > 0.0).count()
    }

    /// The choice as text (for a pair file): one line per layer, `index <TAB> on <TAB> opacity <TAB> name`.
    pub fn to_text(&self) -> String {
        let mut s = String::from("nulayers 1\n");
        for (i, n) in self.nodes.iter().enumerate() {
            s.push_str(&format!("{i}\t{}\t{:.3}\t{}\n", n.on as u8, n.opacity, n.name.replace(['\t', '\n', '\r'], " ")));
        }
        s
    }

    /// Apply a saved choice. A line is used only when the layer at that index still has that name.
    pub fn apply_text(&mut self, text: &str) -> usize {
        let mut used = 0;
        for line in text.lines().skip(1) {
            let mut it = line.splitn(4, '\t');
            let (Some(i), Some(on), Some(op), Some(name)) = (it.next(), it.next(), it.next(), it.next()) else { continue };
            let (Ok(i), Ok(op)) = (i.parse::<usize>(), op.parse::<f32>()) else { continue };
            if let Some(n) = self.nodes.get_mut(i) {
                if n.name.replace(['\t', '\n', '\r'], " ") == name {
                    n.on = on == "1";
                    n.opacity = op.clamp(0.0, 1.0);
                    used += 1;
                }
            }
        }
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, parent: Option<usize>, group: bool) -> Node {
        Node { name: name.into(), parent, group, on0: true, opacity0: 1.0, on: true, opacity: 1.0, bounds: (!group).then_some([0, 0, 4, 4]), mode: String::new() }
    }

    /// 0 Eyes (group) { 1 Iris, 2 Highlight }, 3 Face, 4 Hair (group) { 5 Fringe { as a layer } }, 6 Background
    fn set() -> LayerSet {
        let nodes = vec![node("Eyes", None, true), node("Iris", Some(0), false), node("Highlight", Some(0), false), node("Face", None, false), node("Hair", None, true), node("Fringe", Some(4), false), node("Background", None, false)];
        let n = nodes.len();
        LayerSet { nodes, hover: None, solo: None, filter: String::new(), collapsed: vec![false; n] }
    }

    #[test]
    fn a_group_switch_and_its_opacity_reach_the_children() {
        let mut s = set();
        s.nodes[0].opacity = 0.5;
        let e = s.eff(false);
        assert_eq!(e[1], Eff { on: true, opacity: 0.5 });
        s.toggle(0);
        let e = s.eff(false);
        assert!(!e[1].on && !e[2].on && e[3].on);
        assert_eq!(s.shown_count(), 3); // Face, Fringe, Background
    }

    #[test]
    fn filter_keeps_parents_and_the_whole_of_a_matching_group() {
        let mut s = set();
        s.filter = "iri".into();
        assert_eq!(s.rows().iter().map(|r| r.index).collect::<Vec<_>>(), vec![0, 1]);
        s.filter = "HAIR".into();
        assert_eq!(s.rows().iter().map(|r| r.index).collect::<Vec<_>>(), vec![4, 5]);
        s.filter = "zzz".into();
        assert!(s.rows().is_empty());
        s.filter = String::new();
        s.collapsed[0] = true;
        assert_eq!(s.rows().iter().map(|r| r.index).collect::<Vec<_>>(), vec![0, 3, 4, 5, 6]);
        assert_eq!(s.rows()[0].depth, 0);
        s.collapsed[0] = false;
        assert_eq!(s.rows()[1], Row { index: 1, depth: 1 });
    }

    #[test]
    fn hover_dims_the_rest_and_shows_a_switched_off_layer_for_the_preview() {
        let mut s = set();
        s.toggle(3); // Face off
        s.hover = Some(3);
        let e = s.eff(true);
        assert_eq!(e[3], Eff { on: true, opacity: 1.0 });
        assert!((e[6].opacity - DIM).abs() < 1e-6 && e[6].on);
        // export ignores the hover
        let x = s.eff(false);
        assert!(!x[3].on && x[6].opacity == 1.0);
        // hovering a group lights everything inside it
        s.hover = Some(0);
        let e = s.eff(true);
        assert!(e[1].opacity == 1.0 && e[2].opacity == 1.0 && e[6].opacity < 0.5);
    }

    #[test]
    fn solo_draws_one_layer_whatever_its_switch_says() {
        let mut s = set();
        s.toggle(1); // Iris off
        s.toggle_solo(1);
        let e = s.eff(false);
        assert!(e[1].on);
        assert!(!e[2].on && !e[3].on && !e[6].on);
        s.toggle_solo(1);
        assert_eq!(s.solo, None);
    }

    #[test]
    fn double_and_triple_click_semantics_and_modified_flag() {
        let mut s = set();
        assert!(!s.modified());
        s.set_opacity(3, 0.4);
        assert!(s.modified());
        s.reset_one(3);
        assert!(!s.modified());
        s.clear_one(3);
        assert_eq!(s.nodes[3].opacity, 0.0);
        s.set_all(false);
        assert_eq!(s.shown_count(), 0);
        s.reset_all();
        assert!(!s.modified() && s.shown_count() == 5);
    }

    #[test]
    fn a_saved_choice_comes_back_only_for_the_same_layers() {
        let mut s = set();
        s.toggle(3);
        s.set_opacity(6, 0.25);
        let text = s.to_text();
        let mut t = set();
        assert_eq!(t.apply_text(&text), 7);
        assert!(!t.nodes[3].on && (t.nodes[6].opacity - 0.25).abs() < 1e-3);
        let mut other = set();
        other.nodes[3].name = "Renamed".into();
        assert_eq!(other.apply_text(&text), 6);
        assert!(other.nodes[3].on);
    }

    #[test]
    fn subtree_and_depth() {
        let s = set();
        assert_eq!(s.subtree(0), vec![0, 1, 2]);
        assert_eq!(s.subtree(3), vec![3]);
        assert_eq!(s.depth(5), 1);
    }
}
