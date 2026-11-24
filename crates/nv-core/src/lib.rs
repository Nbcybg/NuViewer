//! Platform-independent core: folder listing, background decoding/prefetch, thumbnails, gallery layout.

pub mod about;
pub mod colours;
pub mod textinput;
pub mod xmp;
pub mod archive;
pub mod archive_tools;
pub mod batch;
pub mod ffmpeg;
pub mod fileops;
pub mod folder;
pub mod frames;
pub mod gallery;
pub mod importer;
pub mod keys;
pub mod layers;
pub mod loader;
pub mod pair;
pub mod pdf_make;
pub mod plugins;
pub mod posters;
pub mod prefs;
pub mod rawpair;
pub mod quick;
pub mod quick_edit;
pub mod sequence;
pub use nv_text::skin;
pub mod settings;
pub mod snapshot;
pub mod spread;
pub mod textfiles;
pub mod thumbs;
pub mod videocolour;

/// The text layer (see the `nv-text` crate): `nv_core::i18n::t("key")`.
pub use nv_text as i18n;

pub use fileops::UndoOp;
pub use folder::{FolderList, Order, SortKey};
pub use gallery::Layout;
pub use loader::{LoadState, Loaded, Loader};
