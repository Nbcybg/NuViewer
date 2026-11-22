//! "Quick edit": turn or flip a JPEG / PNG in place. One implementation for the gallery's right-click menu and the viewer's.
//!
//! The original goes to the backup folder first (so Ctrl+Z can bring it back), the file's date is kept, and a JPEG keeps its EXIF
//! (with the orientation tag set to upright). Anything that can't be written back safely is refused with a plain sentence.

use nv_editor::{raster, save, Pixels};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuickOp {
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
}

impl QuickOp {
    pub const ALL: [QuickOp; 4] = [QuickOp::RotateLeft, QuickOp::RotateRight, QuickOp::FlipH, QuickOp::FlipV];

    pub fn label(self) -> &'static str {
        match self {
            QuickOp::RotateLeft => "Rotate left 90°",
            QuickOp::RotateRight => "Rotate right 90°",
            QuickOp::FlipH => "Flip horizontally",
            QuickOp::FlipV => "Flip vertically",
        }
    }

    /// Past tense, for "Flipped horizontally — Ctrl+Z restores the original".
    pub fn done(self) -> &'static str {
        match self {
            QuickOp::RotateLeft => "Rotated left 90°",
            QuickOp::RotateRight => "Rotated right 90°",
            QuickOp::FlipH => "Flipped horizontally",
            QuickOp::FlipV => "Flipped vertically",
        }
    }
}

/// Where originals are kept while their rewritten version exists (pruned after a week). The same folder for every program.
pub fn backup_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NUEDITOR_BACKUP_DIR") {
        return PathBuf::from(d); // the tests (and NuEditor, which keeps the same folder)
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("undo")
}

/// Can this file be rewritten by a quick edit (by its name alone: JPEG or PNG)?
pub fn can_rewrite(path: &Path) -> bool {
    save::Format::of_path(path).is_some()
}

/// Apply `op` to the file. On success the copy of the original is returned (what Ctrl+Z puts back).
pub fn rewrite(path: &Path, op: QuickOp) -> Result<PathBuf, String> {
    let fmt = save::Format::of_path(path).ok_or("only JPEG and PNG pictures can be rewritten")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = nv_codecs::decode_file(path)?;
    if img.frame_count() > 1 {
        return Err("animated pictures can't be edited here".into());
    }
    if img.reduced_from.is_some() {
        return Err("too large to rewrite safely".into());
    }
    let img = img.finish_for_cpu();
    let src = Pixels::new(img.width, img.height, img.rgba);
    let out = match op {
        QuickOp::RotateLeft => raster::rotate(&src, -90.0),
        QuickOp::RotateRight => raster::rotate(&src, 90.0),
        QuickOp::FlipH => raster::flip_horizontal(&src),
        QuickOp::FlipV => raster::flip_vertical(&src),
    };
    let exif = if fmt == save::Format::Jpeg { save::exif_of_jpeg(&bytes, (out.w, out.h)) } else { None };
    let encoded = save::encode(&out, fmt, exif.as_deref())?;
    let times = crate::batch::read_file_times(path);
    let backup = save::backup_copy(path, &backup_dir()).map_err(|e| format!("no backup possible ({e})"))?;
    if let Err(e) = save::write_atomic(path, &encoded) {
        let _ = std::fs::remove_file(&backup);
        return Err(e.to_string());
    }
    if let Some((_, modified)) = times {
        let _ = crate::batch::set_file_times(path, modified, crate::batch::WhichTimes { created: false, modified: true, accessed: false });
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_rewrites_the_file_keeps_its_date_and_returns_the_original() {
        let dir = std::env::temp_dir().join(format!("nv_quick_edit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("NUEDITOR_BACKUP_DIR", dir.join("undo"));
        // Left half red, right half blue.
        let mut rgba = Vec::new();
        for _ in 0..4 {
            for x in 0..8 {
                rgba.extend(if x < 4 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
            }
        }
        let file = dir.join("a.png");
        std::fs::write(&file, save::encode(&Pixels::new(8, 4, rgba), save::Format::Png, None).unwrap()).unwrap();
        let before = std::fs::read(&file).unwrap();
        let date = crate::batch::read_file_times(&file).unwrap().1;

        let backup = rewrite(&file, QuickOp::FlipH).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), before, "the backup is the original, byte for byte");
        let now = nv_codecs::decode_file(&file).unwrap().finish_for_cpu();
        assert_eq!((now.width, now.height), (8, 4));
        assert!(now.rgba[0] < 60 && now.rgba[2] > 200, "left is blue now");
        assert_eq!(crate::batch::read_file_times(&file).unwrap().1, date, "the file's date is kept");

        assert!(rewrite(&dir.join("x.tga"), QuickOp::FlipH).is_err(), "only JPEG and PNG");
        assert!(can_rewrite(&file) && !can_rewrite(&dir.join("x.webp")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
