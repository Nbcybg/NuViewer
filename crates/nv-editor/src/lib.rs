//! NuEditor core. Everything here works on plain RGBA8 pixels, so it needs no window and no GPU and can be tested
//! on its own; the viewer (`nuviewer`) owns the screen and calls into it.
//!
//! * [`raster`]: rotate (any angle, exact for right angles), flip, flatten onto a background.
//! * [`ops`]: the list of edits applied to a picture, with undo / redo. Edits are *recorded*, not baked in: the viewer
//!   shows them live on the GPU and the pixels are only rewritten when saving.
//! * [`save`]: encode (JPEG / PNG), carry the EXIF block over, replace the file safely.
//! * [`tools`]: the catalogue of tools the editor is meant to have, with their state (the mock-up draws it).

pub mod ops;
pub mod raster;
pub mod save;
pub mod tools;

pub use ops::{EditStack, Op};
pub use raster::Pixels;
