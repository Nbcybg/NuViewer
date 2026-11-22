//! One set of controls for the two players (Spine in `spine.rs`, Live2D in `live2d.rs`): the layer panel, the keys and the mouse call these,
//! and each does what the player in front does.

use super::*;

impl App {
    pub fn anim_active(&self) -> bool {
        self.spine_active() || self.l2d_active()
    }

    pub fn anim_path(&self) -> Option<PathBuf> {
        if let Some(p) = self.spine.player.as_ref() {
            return Some(p.path.clone());
        }
        self.l2d.player.as_ref().map(|p| p.path.clone())
    }

    pub fn anim_play(&mut self) {
        if self.spine_active() {
            self.spine_toggle_play();
        } else {
            self.l2d_toggle_play();
        }
    }

    pub fn anim_playing(&self) -> bool {
        self.spine.player.as_ref().map(|p| p.playing).or_else(|| self.l2d.player.as_ref().map(|p| p.playing)).unwrap_or(false)
    }

    /// Animation (Spine) / motion (Live2D): previous or next.
    pub fn anim_step(&mut self, d: i32) {
        if self.spine_active() {
            self.spine_step_anim(d);
        } else {
            self.l2d_step_motion(d);
        }
    }

    /// Skin (Spine) / expression (Live2D): next.
    pub fn anim_skin(&mut self) {
        if self.spine_active() {
            self.spine_next_skin();
        } else {
            self.l2d_next_expression();
        }
    }

    pub fn anim_info(&self) -> String {
        if self.spine.player.is_some() {
            self.spine_info()
        } else {
            self.l2d_info()
        }
    }

    pub fn anim_title(&self) -> String {
        if let Some(p) = &self.spine.player {
            format!("Spine {} - {} of {} slots shown", p.rig.version, self.layers.set.shown_count(), self.layers.set.nodes.len())
        } else {
            format!("Live2D - {} of {} parts shown", self.layers.set.shown_count(), self.layers.set.nodes.len())
        }
    }

    pub fn anim_note(&self) -> String {
        match self.spine.player.as_ref().map(|p| p.rig.unsupported.clone()) {
            Some(u) if !u.is_empty() => format!("Not applied: {}.", u.join(", ")),
            Some(_) => "Animations ease into each other; physics swings while it plays.".to_string(),
            None => "Parts: the opacity is the model's own part opacity (0 to 1). Physics, pose and eye blink are not applied.".to_string(),
        }
    }

    /// The panel's rows: (name, note), top of the list first.
    pub fn anim_nodes(&self) -> Vec<(String, String)> {
        if self.spine.player.is_some() {
            self.spine_nodes()
        } else {
            self.l2d_part_nodes()
        }
    }

    pub fn anim_apply(&mut self, eff: &[nv_codecs::layerdoc::Eff]) {
        if self.spine_active() {
            self.spine_apply_slots(eff);
        } else {
            self.l2d_apply_parts(eff);
        }
    }

    pub fn anim_snapshot(&mut self) -> bool {
        if self.spine_active() {
            self.spine_snapshot()
        } else if self.l2d_active() {
            self.l2d_snapshot()
        } else {
            false
        }
    }

    pub fn anim_frames(&mut self) {
        if self.spine_active() {
            self.spine_export_frames();
        } else {
            self.l2d_export_frames();
        }
    }

    pub fn anim_webm(&mut self) {
        if self.spine_active() {
            self.spine_export_webm();
        } else {
            self.l2d_export_webm();
        }
    }
}
