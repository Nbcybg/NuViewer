//! The turntable of the 3D view: the model turns about its up axis by itself. It is driven from the same bar as a video: play / pause
//! (the P key, or Shift+A), the speed button (1x 2x 4x 0.5x of the turntable speed in Settings > 3D), step buttons (15 degrees) and a
//! seek track that is the angle. Also the bone names: the joints of the skeleton, labelled where they are on the screen.

use super::*;

pub const TIMER_TURNTABLE: u32 = 11;
const STEP_DEG: f32 = 15.0;
const TAU: f32 = std::f32::consts::TAU;

impl App {
    /// Start or stop the turntable (a 3D model must be in front).
    pub fn turntable_toggle(&mut self) {
        if !self.is_model() {
            return;
        }
        self.turntable = !self.turntable;
        if self.turntable {
            self.turntable_last = Instant::now();
            set_timer(self.hwnd, TIMER_TURNTABLE, 16);
            self.toast(format!("Turntable on: {:.0}\u{00B0} a second (the speed button on the bar changes it)", self.settings.model.turntable_dps * self.anim_speed));
        } else {
            kill_timer(self.hwnd, TIMER_TURNTABLE);
            self.toast("Turntable off");
        }
        invalidate(self.hwnd);
    }

    /// The timer: turn by the time that passed (so a slow frame doesn't slow the model down).
    pub fn turntable_tick(&mut self) {
        if !self.turntable || !self.is_model() {
            self.turntable = false;
            kill_timer(self.hwnd, TIMER_TURNTABLE);
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.turntable_last).as_secs_f32().min(0.25);
        self.turntable_last = now;
        let dps = self.settings.model.turntable_dps * self.anim_speed;
        self.cam.yaw = turned(self.cam.yaw, dps, dt);
        invalidate(self.hwnd);
    }

    /// 0..1 along the seek track: the angle of the view.
    pub fn turntable_progress(&self) -> f32 {
        self.cam.yaw.rem_euclid(TAU) / TAU
    }

    pub fn turntable_seek(&mut self, frac: f32) {
        self.cam.yaw = frac.clamp(0.0, 1.0) * TAU;
        invalidate(self.hwnd);
    }

    /// The step buttons: 15 degrees either way.
    pub fn turntable_step(&mut self, d: isize) {
        self.cam.yaw = (self.cam.yaw + d as f32 * STEP_DEG.to_radians()).rem_euclid(TAU);
        invalidate(self.hwnd);
    }

    /// "135\u{00B0}" for the time field of the bar.
    pub fn turntable_text(&self) -> String {
        format!("{:.0}\u{00B0}", self.turntable_progress() * 360.0)
    }

    /// Names of the joints beside them, when the skeleton and its names are on. Called while the frame is open (after the model).
    pub fn draw_bone_names(&mut self) {
        let style = self.settings.model;
        if !self.is_model() || !style.bones || !style.bone_names {
            return;
        }
        let id = self.id_at(self.folder.index);
        let points = self.renderer.bone_points(id, &self.cam, &style);
        if points.is_empty() {
            return;
        }
        let d = self.gallery.dpi;
        let (w, h) = self.renderer.size();
        let mut placed: Vec<[f32; 4]> = Vec::new();
        for (name, p, _) in points {
            let Some(p) = p else { continue };
            let label = crate::hud::clip(&name, 28);
            let Some((key, tw, th)) = self.overlay_glyph(&label, 11.0) else { continue };
            // To the right of the joint, nudged down while it would cover another label.
            let (x, mut y) = (p[0] + 7.0 * d, p[1] - th / 2.0);
            let mut guard = 0;
            while placed.iter().any(|r| x < r[2] && x + tw > r[0] && y < r[3] && y + th > r[1]) && guard < 12 {
                y += th * 0.9;
                guard += 1;
            }
            if x > w as f32 || y > h as f32 || x + tw < 0.0 || y + th < 0.0 {
                continue;
            }
            placed.push([x, y, x + tw, y + th]);
            let _ = self.renderer.draw_overlay(key, x, y, [1.0, 0.85, 0.45, 1.0]);
        }
    }
}

/// `yaw` (radians) after turning at `dps` degrees a second for `dt` seconds, kept within one turn.
pub fn turned(yaw: f32, dps: f32, dt: f32) -> f32 {
    (yaw + dps.to_radians() * dt).rem_euclid(TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_is_by_time_and_stays_within_one_turn() {
        assert!((turned(0.0, 90.0, 1.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        let t = turned(6.0, 360.0, 1.0);
        assert!((0.0..TAU).contains(&t), "{t}");
        // Half the speed over twice the time gives the same angle.
        assert!((turned(1.0, 30.0, 2.0) - turned(1.0, 60.0, 1.0)).abs() < 1e-5);
        // Backwards (a negative speed) wraps too.
        assert!((0.0..TAU).contains(&turned(0.1, -90.0, 1.0)));
    }
}
