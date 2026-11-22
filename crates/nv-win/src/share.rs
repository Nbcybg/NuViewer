//! "Share to...": the Windows share sheet (Mail, Nearby sharing, Teams, whatever the user has installed that takes files) for one or
//! more files. Every program has the command: the viewer and the gallery share the selected files, the editor shares the picture as
//! it looks now (a rendered copy when there are unsaved edits), the importer the files it brought in.
//!
//! The sheet is Windows' own (`DataTransferManager` shown for our window); we only hand it the files. Files have to exist when it
//! opens; they are opened here, before the sheet, so nothing slow happens inside Windows' callback.

use std::path::{Path, PathBuf};
use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
use windows_collections::IIterable;
use windows::Foundation::TypedEventHandler;
use windows::Storage::{IStorageItem, StorageFile};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::IDataTransferManagerInterop;
use windows::core::{factory, Interface, HSTRING};

/// The storage items handed to the sheet's callback. The callback runs on the window's own thread (the sheet is raised from it), so
/// the items are never touched from another one; the handler type just insists on Send + Sync.
struct Items(Vec<Option<IStorageItem>>);
unsafe impl Send for Items {}
unsafe impl Sync for Items {}

/// What the sheet shows as the title of the thing being shared.
fn title_for(files: &[PathBuf]) -> String {
    match files {
        [one] => one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        many => format!("{} files", many.len()),
    }
}

/// The files that can really be shared: they exist and are files (a folder cannot go through the sheet).
pub fn shareable(files: &[PathBuf]) -> Vec<PathBuf> {
    files.iter().filter(|p| p.is_file()).cloned().collect()
}

/// A path as Windows' storage API wants it: absolute, with backslashes and no `\\?\` prefix.
fn storage_path(p: &Path) -> HSTRING {
    let s = p.to_string_lossy();
    HSTRING::from(s.strip_prefix(r"\\?\").unwrap_or(&s).replace('/', "\\"))
}

/// Open the Windows share sheet for `files` over window `hwnd`. `Err` says why not (no file to share, or the sheet is not available);
/// the caller shows it.
pub fn share_files(hwnd: HWND, files: &[PathBuf]) -> Result<(), String> {
    let files = shareable(files);
    if files.is_empty() {
        return Err("Nothing to share: pick a file first.".into());
    }
    // The files as storage items, opened now.
    let mut items: Vec<Option<IStorageItem>> = Vec::new();
    for p in &files {
        let file = StorageFile::GetFileFromPathAsync(&storage_path(p)).and_then(|op| op.join()).map_err(|e| format!("Can't open {} for sharing: {e}", p.display()))?;
        items.push(Some(file.cast::<IStorageItem>().map_err(|e| e.to_string())?));
    }
    let title = HSTRING::from(title_for(&files));
    let items = Items(items);
    let interop = factory::<DataTransferManager, IDataTransferManagerInterop>().map_err(|e| format!("The share sheet is not available on this PC: {e}"))?;
    let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd) }.map_err(|e| format!("The share sheet is not available for this window: {e}"))?;
    let token = manager
        .DataRequested(&TypedEventHandler::new(move |_: windows::core::Ref<DataTransferManager>, args: windows::core::Ref<DataRequestedEventArgs>| {
            let items = &items; // the whole wrapper (not just its field) is what the closure holds
            if let Some(args) = args.as_ref() {
                let data = args.Request()?.Data()?;
                data.Properties()?.SetTitle(&title)?;
                data.SetStorageItems(&IIterable::<IStorageItem>::from(items.0.clone()), true)?;
            }
            Ok(())
        }))
        .map_err(|e| format!("The share sheet could not be prepared: {e}"))?;
    let shown = unsafe { interop.ShowShareUIForWindow(hwnd) }.map_err(|e| format!("The share sheet could not be shown: {e}"));
    // The handler stays registered until the sheet has asked for the data; it is dropped with the manager when the process ends or the
    // next share replaces it. Removing it right away would leave the sheet without data.
    let _ = token;
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_existing_files_are_shared() {
        let dir = std::env::temp_dir().join(format!("nv_share_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, "x").unwrap();
        let got = shareable(&[f.clone(), dir.join("missing.txt"), dir.clone()]);
        assert_eq!(got, vec![f.clone()]);
        assert!(share_files(HWND::default(), &[dir.join("missing.txt")]).is_err());
        assert_eq!(title_for(&[f.clone()]), "a.txt");
        assert_eq!(title_for(&[f.clone(), f]), "2 files");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_paths_are_plain_backslash_paths() {
        assert_eq!(storage_path(Path::new(r"\\?\C:\a\b.png")).to_string(), r"C:\a\b.png");
        assert_eq!(storage_path(Path::new("C:/a/b.png")).to_string(), r"C:\a\b.png");
    }
}
