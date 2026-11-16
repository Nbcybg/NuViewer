//! Explorer's own right-click menu: "Convert to PNG" and "Convert to JPG" on every picture, per user (HKCU, no admin).
//! Classic verbs under `SystemFileAssociations\<.ext>\shell`; on Windows 11 they appear under "Show more options".
//! The command is `nuviewer.exe --to-png "%1"`: one process per selected file, which `nv_core::quick::run_cli` folds into one batch.

use crate::registry::{delete_tree, read_string, set_string, Layout};
use std::path::Path;

pub const VERB_PNG: &str = "NuViewer.ToPng";
pub const VERB_JPG: &str = "NuViewer.ToJpg";

fn is_jpeg(ext: &str) -> bool {
    matches!(ext.trim_start_matches('.').to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "jpe" | "jfif")
}

fn is_png(ext: &str) -> bool {
    ext.trim_start_matches('.').eq_ignore_ascii_case("png")
}

fn key(layout: &Layout, ext: &str, verb: &str) -> String {
    let dotted = if ext.starts_with('.') { ext.to_string() } else { format!(".{ext}") };
    format!("{}\\SystemFileAssociations\\{dotted}\\shell\\{verb}", layout.classes)
}

fn command_key(layout: &Layout, ext: &str, verb: &str) -> String {
    format!("{}\\command", key(layout, ext, verb))
}

/// Add both entries to every extension in `exts` (a JPEG gets only "to PNG", a PNG only "to JPG").
pub fn install(layout: &Layout, exe: &Path, exts: &[String], png_label: &str, jpg_label: &str) -> Result<(), String> {
    let exe_s = exe.display().to_string();
    for ext in exts {
        for (verb, label, flag, skip) in [(VERB_PNG, png_label, "--to-png", is_png(ext)), (VERB_JPG, jpg_label, "--to-jpg", is_jpeg(ext))] {
            if skip {
                continue;
            }
            let k = key(layout, ext, verb);
            set_string(&k, None, label)?;
            set_string(&k, Some("Icon"), &format!("\"{exe_s}\",0"))?;
            set_string(&command_key(layout, ext, verb), None, &format!("\"{exe_s}\" {flag} \"%1\""))?;
        }
    }
    Ok(())
}

pub fn remove(layout: &Layout, exts: &[String]) {
    for ext in exts {
        for verb in [VERB_PNG, VERB_JPG] {
            delete_tree(&key(layout, ext, verb));
        }
    }
}

/// Is the menu in place (looked at on .webp)?
pub fn installed(layout: &Layout) -> bool {
    read_string(&command_key(layout, ".webp", VERB_JPG), None).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_remove_in_a_scratch_key() {
        let prefix = format!("Software\\NuViewerTest\\explorer{}", std::process::id());
        let layout = Layout::sandbox(&prefix);
        let exts = vec![".jpg".to_string(), ".png".to_string(), ".webp".to_string()];
        install(&layout, Path::new("C:\\x\\nuviewer.exe"), &exts, "Convert to PNG", "Convert to JPG").unwrap();
        assert!(read_string(&command_key(&layout, ".jpg", VERB_PNG), None).unwrap().contains("--to-png \"%1\""));
        assert!(read_string(&command_key(&layout, ".jpg", VERB_JPG), None).is_none(), "a JPEG is not offered JPG");
        assert!(read_string(&command_key(&layout, ".png", VERB_PNG), None).is_none(), "a PNG is not offered PNG");
        assert!(read_string(&command_key(&layout, ".webp", VERB_JPG), None).is_some());
        assert!(installed(&layout));
        remove(&layout, &exts);
        assert!(!installed(&layout));
        delete_tree(&prefix);
    }
}
