//! The colours the viewer's own interface is drawn in (bars, panels, buttons, the film strip): the skin in use
//! (`nv_text::skin`), as tints for the renderer. Every function takes the alpha the thing is drawn with.

use super::srgb;
use nv_text::skin::{self, mix};

fn sk() -> &'static skin::Skin {
    skin::current()
}

/// Text on a panel.
pub fn text(a: f32) -> [f32; 4] {
    srgb(sk().text, a)
}

/// Hints and small print.
pub fn dim(a: f32) -> [f32; 4] {
    srgb(sk().dim, a)
}

/// Switched-off labels.
pub fn off(a: f32) -> [f32; 4] {
    srgb(sk().disabled(), a)
}

pub fn accent(a: f32) -> [f32; 4] {
    srgb(sk().accent, a)
}

/// A link or a lighter touch of the accent.
pub fn link(a: f32) -> [f32; 4] {
    srgb(mix(sk().accent, sk().text, 0.35), a)
}

/// Bars and panels laid over the picture.
pub fn panel(a: f32) -> [f32; 4] {
    srgb(sk().panel, a)
}

/// A button or a track at rest.
pub fn tile(a: f32) -> [f32; 4] {
    srgb(sk().tile, a)
}

/// A button under the pointer.
pub fn hover(a: f32) -> [f32; 4] {
    srgb(sk().hover, a)
}

/// The chosen row of a list.
pub fn select(a: f32) -> [f32; 4] {
    srgb(sk().selection(), a)
}

pub fn edge(a: f32) -> [f32; 4] {
    srgb(sk().edge, a)
}

/// The grip of a slider or a scroll bar.
pub fn knob(a: f32) -> [f32; 4] {
    srgb(mix(sk().tile, sk().text, 0.35), a)
}

pub fn error(a: f32) -> [f32; 4] {
    srgb(sk().error, a)
}

/// A picture that would not open.
pub fn failed(a: f32) -> [f32; 4] {
    srgb(mix(sk().tile, sk().error, 0.35), a)
}

/// A highlighted word (the amber of "look here").
pub fn star(a: f32) -> [f32; 4] {
    srgb(sk().star, a)
}

/// Text on top of an accent-coloured button.
pub fn on_accent(a: f32) -> [f32; 4] {
    srgb(sk().on_accent(), a)
}
