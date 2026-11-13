//! Platform-independent core: folder listing, background decoding/prefetch, thumbnails, gallery layout.

pub mod fileops;
pub mod folder;
pub mod gallery;
pub mod keys;
pub mod loader;
pub mod sequence;
pub mod settings;
pub mod spread;
pub mod thumbs;

pub use fileops::UndoOp;
pub use folder::{FolderList, Order, SortKey};
pub use gallery::Layout;
pub use loader::{LoadState, Loaded, Loader};
