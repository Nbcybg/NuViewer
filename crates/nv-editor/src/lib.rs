//! NuEditor core. Everything here works on plain RGBA8 pixels, so it needs no window and no GPU and can be tested
//! on its own; the programs (`nueditor`, `nuviewer`, `nugallery`) own the screen and call into it.
//!
//! * [`recipe`]: everything the user did, as data: one table of sliders plus curves, crop, heal strokes, flags.
//! * [`sidecar`]: the recipe as a pair file next to the picture (`IMG.jpg.nuedit`, plain text), the "not burned in" form.
//! * [`engine`]: applies a recipe to pixels (live preview, burning in and batch all use it).
//! * [`raster`]: rotate (any angle, exact for right angles), flip, flatten onto a background.
//! * [`ops`]: the short edit list of the viewer's quick turn, with undo / redo.
//! * [`save`]: encode (JPEG / PNG), carry the EXIF block over, replace the file safely.
//! * [`tools`]: the catalogue of tools the editor is meant to have.

pub mod bgremove;
pub mod blur;
pub mod convert;
pub mod cropmath;
pub mod engine;
pub mod export;
pub mod gifenc;
pub mod huecurves;
pub mod journal;
pub mod lut;
pub mod ops;
pub mod preset;
pub mod quantize;
pub mod rawbase;
pub mod raster;
pub mod resample;
pub mod retouch;
pub mod recipe;
pub mod save;
pub mod scopes;
pub mod sidecar;
pub mod spatial;
pub mod tools;
pub mod watermark;

pub use ops::{EditStack, Op};
pub use raster::Pixels;
pub use recipe::{Recipe, P};
