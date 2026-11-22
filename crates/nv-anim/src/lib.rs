//! Spine skeletons (versions 3.8, 4.0, 4.1 and 4.2): read the file, pose it, rasterise it. See `json` and `binary` for the readers,
//! `pose` for the animation and constraints, `raster` for the picture. Nothing here knows about windows or image formats: the caller
//! decodes the atlas pages and puts them in `Rig::atlas.pages[i].image`.

pub mod atlas;
pub mod binary;
pub mod cubism;
pub mod json;
pub mod live2d;
pub mod model;
pub mod pose;
pub mod raster;

pub use atlas::{Atlas, PageImage};
pub use model::*;
pub use pose::{DrawItem, Pose};
pub use raster::{render, render_pages, View};

impl Rig {
    /// Parse a JSON skeleton with its atlas text.
    pub fn from_json(src: &str, atlas_text: &str) -> Result<Rig, String> {
        json::parse(src, Atlas::parse(atlas_text)?)
    }

    /// Draw the skeleton in `pose` into straight RGBA8, `w` x `h`, with `view`. See `raster::render`.
    pub fn draw(&self, pose: &Pose, w: usize, h: usize, view: View, ssaa: usize, background: Option<[f32; 3]>) -> Vec<u8> {
        raster::render(self, &self.geometry(pose), w, h, view, ssaa, background)
    }
}

#[cfg(test)]
mod tests;
