//! The edits applied to a picture, as a list with undo / redo. Nothing is baked in until saving: the viewer shows the
//! result live (a rotation is a transform on the GPU quad) and calls [`EditStack::render`] only when a file is written.

use crate::raster::{self, Pixels};

/// One edit. More will join these as the tools in [`crate::tools`] get built.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op {
    /// Clockwise degrees.
    Rotate(f32),
    FlipH,
    FlipV,
}

#[derive(Clone, Default, Debug)]
pub struct EditStack {
    ops: Vec<Op>,
    /// Number of ops in effect (the rest are redo-able).
    cursor: usize,
}

impl EditStack {
    pub fn new() -> EditStack {
        EditStack::default()
    }

    /// Add an edit; anything that was undone is forgotten. Consecutive rotations are merged into one.
    pub fn push(&mut self, op: Op) {
        self.ops.truncate(self.cursor);
        if let (Op::Rotate(b), Some(Op::Rotate(a))) = (op, self.ops.last().copied()) {
            self.ops.pop();
            let sum = raster::normalize_degrees(a + b);
            if sum != 0.0 {
                self.ops.push(Op::Rotate(sum));
            }
        } else if let Op::Rotate(d) = op {
            let d = raster::normalize_degrees(d);
            if d != 0.0 {
                self.ops.push(Op::Rotate(d));
            }
        } else {
            self.ops.push(op);
        }
        self.cursor = self.ops.len();
    }

    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.cursor >= self.ops.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.ops.len()
    }

    /// The edits in effect, oldest first.
    pub fn active(&self) -> &[Op] {
        &self.ops[..self.cursor]
    }

    /// Nothing to save.
    pub fn is_empty(&self) -> bool {
        self.active().is_empty()
    }

    pub fn clear(&mut self) {
        self.ops.clear();
        self.cursor = 0;
    }

    /// The total clockwise turn when rotation is all there is (what the viewer can show with a transform); None if there
    /// are other edits as well.
    pub fn only_rotation(&self) -> Option<f32> {
        match self.active() {
            [] => Some(0.0),
            [Op::Rotate(d)] => Some(*d),
            _ => None,
        }
    }

    /// Apply every active edit to `src`.
    pub fn render(&self, src: &Pixels) -> Pixels {
        let mut cur = src.clone();
        for op in self.active() {
            cur = match *op {
                Op::Rotate(d) => raster::rotate(&cur, d),
                Op::FlipH => raster::flip_horizontal(&cur),
                Op::FlipV => raster::flip_vertical(&cur),
            };
        }
        cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_merge_and_cancel() {
        let mut s = EditStack::new();
        s.push(Op::Rotate(45.0));
        s.push(Op::Rotate(45.0));
        assert_eq!(s.active(), &[Op::Rotate(90.0)]);
        s.push(Op::Rotate(-90.0));
        assert!(s.is_empty(), "turning back to where it began leaves nothing to save");
        s.push(Op::Rotate(-45.0));
        assert_eq!(s.only_rotation(), Some(315.0));
    }

    #[test]
    fn undo_redo_and_new_edits_forget_the_redo_list() {
        let mut s = EditStack::new();
        s.push(Op::FlipH);
        s.push(Op::Rotate(90.0));
        assert!(s.undo());
        assert_eq!(s.active(), &[Op::FlipH]);
        assert!(s.can_redo());
        assert!(s.redo());
        assert_eq!(s.active().len(), 2);
        assert!(s.undo() && s.undo());
        assert!(!s.undo());
        s.push(Op::FlipV);
        assert!(!s.can_redo());
        assert_eq!(s.active(), &[Op::FlipV]);
        assert_eq!(s.only_rotation(), None);
    }

    #[test]
    fn render_applies_in_order() {
        let p = Pixels::new(2, 1, vec![1, 0, 0, 255, 2, 0, 0, 255]);
        let mut s = EditStack::new();
        s.push(Op::FlipH);
        s.push(Op::Rotate(90.0));
        let out = s.render(&p);
        assert_eq!((out.w, out.h), (1, 2));
        assert_eq!(out.rgba, vec![2, 0, 0, 255, 1, 0, 0, 255]);
    }
}
