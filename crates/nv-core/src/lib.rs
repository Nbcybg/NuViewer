//! Platform-independent core: folder listing, background decoding/prefetch, thumbnails, gallery layout.

pub mod about;
pub mod archive;
pub mod archive_tools;
pub mod batch;
pub mod ffmpeg;
pub mod fileops;
pub mod folder;
pub mod gallery;
pub mod importer;
pub mod keys;
pub mod loader;
pub mod pair;
pub mod pdf_make;
pub mod posters;
pub mod prefs;
pub mod quick;
pub mod sequence;
pub mod settings;
pub mod snapshot;
pub mod spread;
pub mod thumbs;
pub mod videocolour;

/// The text layer (see the `nv-text` crate): `nv_core::i18n::t("key")`.
pub use nv_text as i18n;

pub use fileops::UndoOp;
pub use folder::{FolderList, Order, SortKey};
pub use gallery::Layout;
pub use loader::{LoadState, Loaded, Loader};
