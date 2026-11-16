//! Typing a number into a slider: click the value on the right of a row and type. Enter applies, Esc cancels, a click elsewhere
//! applies. The slider's own range is only where the knob can go; a typed number may be beyond it (any finite number up to
//! `HARD` is taken: "unlimited" for every practical purpose; the pictures stay valid because every step of the engine clamps
//! what it computes).

use super::panel::Num;
use super::*;

/// What a typed value may not exceed in size (so a slip like 1e30 can't turn the picture into NaNs).
pub const HARD: f32 = 1.0e6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Param(usize),
    Num(Num),
}

pub struct NumEntry {
    pub target: Target,
    pub text: String,
    /// Nothing has been typed yet: the first character replaces the whole text.
    pub fresh: bool,
}

/// The part of a row where the value is shown (and where a click starts typing).
fn value_zone(row: [f32; 4], d: f32) -> [f32; 4] {
    [row[2] - 120.0 * d, row[1], row[2], row[1] + 24.0 * d]
}

/// Parse what was typed: "12", "-3.5", "+4", "1e3", "7,5" (a comma is a decimal point), trailing "%" ignored.
pub fn parse(text: &str) -> Option<f32> {
    let t = text.trim().trim_end_matches('%').trim().replace(',', ".");
    let v: f32 = t.parse().ok()?;
    v.is_finite().then_some(v.clamp(-HARD, HARD))
}

impl App {
    fn entry_zones(&self) -> Vec<(Target, [f32; 4])> {
        let l = self.layout();
        let d = self.dpi;
        let mut v: Vec<(Target, [f32; 4])> = l.rows.iter().filter(|(i, _, _)| engine::is_implemented(*i)).map(|(i, row, _)| (Target::Param(*i), value_zone(*row, d))).collect();
        for it in &l.items {
            if let panel::What::Num(n) = it.what {
                v.push((Target::Num(n), value_zone(it.rect, d)));
            }
        }
        v
    }

    fn entry_current_text(&self, t: Target) -> String {
        match t {
            Target::Param(i) => {
                let v = self.recipe.v[i];
                if v.fract().abs() < 1e-4 { format!("{v:.0}") } else { format!("{v:.3}").trim_end_matches('0').trim_end_matches('.').to_string() }
            }
            Target::Num(n) => {
                let v = self.num_get(n);
                if v.fract().abs() < 1e-4 { format!("{v:.0}") } else { format!("{v:.2}").trim_end_matches('0').trim_end_matches('.').to_string() }
            }
        }
    }

    /// A left press at (x, y): starts typing if it is on a value; applies a running entry first. True if the press was used.
    pub(super) fn entry_press(&mut self, x: i32, y: i32) -> bool {
        let (fx, fy) = (x as f32, y as f32);
        let hit = self.entry_zones().into_iter().find(|(_, z)| contains(*z, fx, fy));
        if let Some(e) = &self.entry {
            if hit.map(|(t, _)| t) == Some(e.target) {
                return true; // a click inside the box
            }
            self.entry_commit();
        }
        if let Some((t, _)) = hit {
            self.entry = Some(NumEntry { target: t, text: self.entry_current_text(t), fresh: true });
            invalidate(self.hwnd);
            return true;
        }
        false
    }

    pub(super) fn entry_char(&mut self, c: char) {
        let Some(e) = &mut self.entry else { return };
        if c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | 'e' | 'E') {
            if e.fresh {
                e.text.clear();
                e.fresh = false;
            }
            if e.text.len() < 14 {
                e.text.push(if c == ',' { '.' } else { c });
            }
            invalidate(self.hwnd);
        }
    }

    /// A key while typing. True if it was used (everything is, so no shortcut fires in the middle of a number).
    pub(super) fn entry_key(&mut self, vk: u32) -> bool {
        if self.entry.is_none() {
            return false;
        }
        match vk {
            0x0D => self.entry_commit(),
            0x1B => {
                self.entry = None;
                invalidate(self.hwnd);
            }
            0x08 => {
                if let Some(e) = &mut self.entry {
                    if e.fresh {
                        e.text.clear();
                        e.fresh = false;
                    } else {
                        e.text.pop();
                    }
                    invalidate(self.hwnd);
                }
            }
            _ => {}
        }
        true
    }

    fn entry_commit(&mut self) {
        let Some(e) = self.entry.take() else { return };
        invalidate(self.hwnd);
        let Some(v) = parse(&e.text) else {
            self.say("That is not a number");
            return;
        };
        match e.target {
            Target::Param(i) => {
                if (self.recipe.v[i] - v).abs() > 1e-6 {
                    self.snapshot();
                    self.recipe.set_index(i, v);
                    self.changed();
                }
            }
            Target::Num(n) => self.num_set_free(n, v),
        }
    }

    /// Where the box over the value being typed goes, and what is in it (worked out before the renderer is borrowed).
    pub(super) fn entry_box(&self) -> Option<EntryBox> {
        let e = self.entry.as_ref()?;
        let (_, z) = self.entry_zones().into_iter().find(|(t, _)| *t == e.target)?;
        Some(EntryBox { zone: z, text: e.text.clone(), fresh: e.fresh })
    }
}

pub struct EntryBox {
    zone: [f32; 4],
    text: String,
    fresh: bool,
}

/// Draw the box over the value being typed.
pub fn draw_entry_box(r: &mut Renderer, text: &mut Text, e: &EntryBox, d: f32) {
    let z = e.zone;
    let b = [z[0] + 24.0 * d, z[1] + 2.0 * d, z[2] - 8.0 * d, z[3] - 1.0 * d];
    let _ = r.fill_rect(b, rgb(0x0E0E10, 1.0));
    gfx::outline(r, b, 1.5 * d, rgb(pal::ACCENT, 1.0));
    let px = (12.0 * d).round() as i32;
    let w = text.width(&e.text, px) as f32;
    if e.fresh {
        let _ = r.fill_rect([b[2] - 6.0 * d - w, b[1] + 3.0 * d, b[2] - 6.0 * d, b[3] - 3.0 * d], rgb(pal::ACCENT, 0.45));
    }
    text.draw_right(r, &e.text, px, b[2] - 6.0 * d, b[1] + 3.0 * d, rgb(pal::TEXT, 1.0));
    if !e.fresh {
        let _ = r.fill_rect([b[2] - 5.0 * d, b[1] + 4.0 * d, b[2] - 4.0 * d, b[3] - 4.0 * d], rgb(pal::TEXT, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_numbers_parse_with_commas_signs_and_limits() {
        assert_eq!(parse("12"), Some(12.0));
        assert_eq!(parse(" -3.5 "), Some(-3.5));
        assert_eq!(parse("+4"), Some(4.0));
        assert_eq!(parse("7,5"), Some(7.5));
        assert_eq!(parse("50%"), Some(50.0));
        assert_eq!(parse("1e3"), Some(1000.0));
        assert_eq!(parse("1e30"), Some(HARD));
        assert_eq!(parse("-1e30"), Some(-HARD));
        assert_eq!(parse(""), None);
        assert_eq!(parse("abc"), None);
        assert_eq!(parse("inf"), None);
        assert_eq!(parse("NaN"), None);
    }
}
