//! File-manager services from the Windows shell: copy / move / delete with Explorer's own progress and conflict
//! dialogs (folders included), the file clipboard (copy, cut, paste), printing and "open".

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::UI::Shell::{DragQueryFileW, ShellExecuteW, DROPFILES, HDROP, SHFileOperationW, SHFILEOPSTRUCTW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;

const FO_MOVE: u32 = 1;
const FO_COPY: u32 = 2;
const FO_DELETE: u32 = 3;
const FOF_RENAMEONCOLLISION: u16 = 0x0008;
const FOF_NOCONFIRMATION: u16 = 0x0010;
const FOF_ALLOWUNDO: u16 = 0x0040;
const FOF_NOCONFIRMMKDIR: u16 = 0x0200;
const FOF_WANTNUKEWARNING: u16 = 0x4000;

/// What to do with a list of files.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Copy,
    Move,
    /// To the Recycle Bin.
    Recycle,
    /// Gone for good (the caller has asked the user).
    Delete,
}

fn double_nul_list(paths: &[PathBuf]) -> Vec<u16> {
    let mut v: Vec<u16> = Vec::new();
    for p in paths {
        v.extend(p.as_os_str().encode_wide());
        v.push(0);
    }
    v.push(0);
    v
}

/// Run a copy / move / delete with the shell's dialogs (progress, "replace or skip", the Recycle Bin warning). Folders are
/// handled whole. `to` is the destination folder for copy / move. With `keep_both`, a name that already exists in the
/// destination is not asked about: the new file gets "name - Copy" (what pasting into the same folder should do).
pub fn run(hwnd: HWND, op: Op, from: &[PathBuf], to: Option<&Path>, keep_both: bool) -> Result<(), String> {
    if from.is_empty() {
        return Ok(());
    }
    let src = double_nul_list(from);
    let dest: Option<Vec<u16>> = to.map(|d| {
        let mut v: Vec<u16> = d.as_os_str().encode_wide().collect();
        v.extend([0, 0]);
        v
    });
    let (func, mut flags) = match op {
        Op::Copy => (FO_COPY, FOF_NOCONFIRMMKDIR | FOF_ALLOWUNDO),
        Op::Move => (FO_MOVE, FOF_NOCONFIRMMKDIR | FOF_ALLOWUNDO),
        Op::Recycle => (FO_DELETE, FOF_ALLOWUNDO | FOF_WANTNUKEWARNING),
        Op::Delete => (FO_DELETE, FOF_NOCONFIRMATION),
    };
    if keep_both {
        flags |= FOF_RENAMEONCOLLISION;
    }
    let mut sh = SHFILEOPSTRUCTW {
        hwnd,
        wFunc: func,
        pFrom: PCWSTR(src.as_ptr()),
        pTo: dest.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null()),
        fFlags: flags,
        ..Default::default()
    };
    let rc = unsafe { SHFileOperationW(&mut sh) };
    if sh.fAnyOperationsAborted.as_bool() {
        return Err("cancelled".into());
    }
    if rc != 0 {
        return Err(format!("the shell reported error 0x{rc:X}"));
    }
    Ok(())
}

// ---- clipboard ---------------------------------------------------------------------------------------------------

unsafe fn open_clipboard(hwnd: HWND) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..10 {
        match OpenClipboard(Some(hwnd)) {
            Ok(()) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Err(format!("the clipboard is busy ({last})"))
}

unsafe fn alloc_global(bytes: &[u8]) -> Result<HANDLE, String> {
    let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()).map_err(|e| e.to_string())?;
    let p = GlobalLock(h) as *mut u8;
    if p.is_null() {
        return Err("GlobalLock failed".into());
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    let _ = GlobalUnlock(h);
    Ok(HANDLE(h.0))
}

/// Put the files on the clipboard the way Explorer does, so they paste into Explorer, other file managers and back
/// into the gallery. `cut` marks them as "to be moved" (the source is removed when they are pasted).
pub fn clipboard_set_files(hwnd: HWND, paths: &[PathBuf], cut: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    unsafe {
        let files = double_nul_list(paths);
        let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
        let mut bytes = std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()).to_vec();
        bytes.extend(files.iter().flat_map(|c| c.to_le_bytes()));
        let effect: u32 = if cut { 2 } else { 1 }; // DROPEFFECT_MOVE / DROPEFFECT_COPY
        let effect_format = RegisterClipboardFormatW(w!("Preferred DropEffect"));
        open_clipboard(hwnd)?;
        let r = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            SetClipboardData(CF_HDROP, Some(alloc_global(&bytes)?)).map_err(|e| e.to_string())?;
            if effect_format != 0 {
                SetClipboardData(effect_format, Some(alloc_global(&effect.to_le_bytes())?)).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        let _ = CloseClipboard();
        r
    }
}

/// The files on the clipboard (from Explorer or from `clipboard_set_files`) and whether they were cut. None if there are none.
pub fn clipboard_files(hwnd: HWND) -> Option<(Vec<PathBuf>, bool)> {
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP).is_err() {
            return None;
        }
        open_clipboard(hwnd).ok()?;
        let result = (|| {
            let h = GetClipboardData(CF_HDROP).ok()?;
            let drop = HDROP(h.0);
            let n = DragQueryFileW(drop, u32::MAX, None);
            let mut out = Vec::new();
            for i in 0..n {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                out.push(PathBuf::from(String::from_utf16_lossy(&buf[..got.min(len)])));
            }
            let cut = {
                let fmt = RegisterClipboardFormatW(w!("Preferred DropEffect"));
                match (fmt != 0).then(|| GetClipboardData(fmt).ok()).flatten() {
                    Some(h) => {
                        let hg = windows::Win32::Foundation::HGLOBAL(h.0);
                        let p = GlobalLock(hg) as *const u8;
                        let v = if !p.is_null() && GlobalSize(hg) >= 4 { u32::from_le_bytes([*p, *p.add(1), *p.add(2), *p.add(3)]) } else { 1 };
                        let _ = GlobalUnlock(hg);
                        v & 2 != 0
                    }
                    None => false,
                }
            };
            Some((out, cut))
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Put only the pixels (top-down 32-bit BGRA, alpha flattened onto white) on the clipboard: paste into Paint, a document,
/// a chat. (`shell::clipboard_copy_image_and_file` adds the file as well.)
pub fn clipboard_set_image(hwnd: HWND, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    unsafe {
        let (w_, h_) = (width as usize, height as usize);
        if bgra.len() < w_ * h_ * 4 {
            return Err("image buffer too small".into());
        }
        let mut dib = Vec::with_capacity(40 + w_ * h_ * 4);
        for v in [40u32, width, height] {
            dib.extend(v.to_le_bytes());
        }
        dib.extend(1u16.to_le_bytes());
        dib.extend(32u16.to_le_bytes());
        for v in [0u32, (w_ * h_ * 4) as u32, 0, 0, 0, 0] {
            dib.extend(v.to_le_bytes());
        }
        for y in (0..h_).rev() {
            for px in bgra[y * w_ * 4..(y + 1) * w_ * 4].chunks_exact(4) {
                let a = px[3] as u32;
                let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                dib.extend([blend(px[0]), blend(px[1]), blend(px[2]), 255]);
            }
        }
        open_clipboard(hwnd)?;
        let r = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            SetClipboardData(CF_DIB, Some(alloc_global(&dib)?)).map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        r
    }
}

// ---- shell verbs -------------------------------------------------------------------------------------------------

fn verb(hwnd: HWND, verb: PCWSTR, path: &Path) -> Result<(), String> {
    let mut f: Vec<u16> = path.as_os_str().encode_wide().collect();
    f.push(0);
    let r = unsafe { ShellExecuteW(Some(hwnd), verb, PCWSTR(f.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    // Values above 32 mean success; below are error codes.
    let code = r.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(match code {
            2 | 3 => "file not found".into(),
            5 => "access denied".into(),
            31 => "no program is set up for this (Windows has no \"print\" action for this file type)".into(),
            c => format!("Windows error {c}"),
        })
    }
}

/// Print with whatever Windows has for the file type (for pictures: the Print Pictures dialog).
pub fn print(hwnd: HWND, path: &Path) -> Result<(), String> {
    verb(hwnd, w!("print"), path)
}

/// Open with the default program.
pub fn open_default(hwnd: HWND, path: &Path) -> Result<(), String> {
    verb(hwnd, w!("open"), path)
}

/// "Open with > Choose another app": Windows' own program picker for the file.
pub fn open_as(hwnd: HWND, path: &Path) -> Result<(), String> {
    verb(hwnd, w!("openas"), path)
}

/// Explorer's Properties window for the file.
pub fn properties(hwnd: HWND, path: &Path) -> Result<(), String> {
    verb(hwnd, w!("properties"), path)
}

#[allow(dead_code)]
fn _unused(_: *const c_void) {}

// ---- text on the clipboard, drives, well-known folders -----------------------------------------------------------

/// Unicode text on the clipboard, if there is any.
pub fn clipboard_text(hwnd: HWND) -> Option<String> {
    unsafe {
        const CF_UNICODETEXT: u32 = 13;
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        open_clipboard(hwnd).ok()?;
        let r = (|| {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let hg = windows::Win32::Foundation::HGLOBAL(h.0);
            let p = GlobalLock(hg) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(hg) / 2;
            let mut len = 0;
            while len < max && *p.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(hg);
            Some(s)
        })();
        let _ = CloseClipboard();
        r
    }
}

/// What kind of drive a letter is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Optical,
    Other,
}

/// The drives that exist right now (no disk access: a card reader without a card is listed, opening it fails).
pub fn drives() -> Vec<(PathBuf, DriveKind)> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = format!("{letter}:\\");
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = match unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) } {
            3 => DriveKind::Fixed,
            2 => DriveKind::Removable,
            4 => DriveKind::Network,
            5 => DriveKind::Optical,
            _ => DriveKind::Other,
        };
        out.push((PathBuf::from(root), kind));
    }
    out
}

/// The volume label of a drive ("Games", "Photos 2024"): what Explorer writes in front of the letter. None for a drive without
/// a label, without a disk in it, or one that does not answer. This **touches the disk** (a sleeping hard disk spins up, a
/// network drive may take seconds), so ask from a worker thread, never from the UI thread.
pub fn volume_label(root: &Path) -> Option<String> {
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut name = [0u16; 261];
    unsafe { GetVolumeInformationW(PCWSTR(wide.as_ptr()), Some(&mut name), None, None, None, None).ok()? };
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    let label = String::from_utf16_lossy(&name[..len]);
    (!label.trim().is_empty()).then_some(label)
}

/// The user's Pictures / Desktop / Downloads / Documents folders (None for one that Windows doesn't name).
pub fn known_folder(which: &str) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Pictures, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let id = match which {
        "pictures" => &FOLDERID_Pictures,
        "desktop" => &FOLDERID_Desktop,
        "downloads" => &FOLDERID_Downloads,
        "documents" => &FOLDERID_Documents,
        _ => return None,
    };
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s.map(PathBuf::from)
    }
}

// ---- dragging files out to Explorer and other programs ------------------------------------------------------------

mod dnd {
    use windows::core::{implement, HRESULT, BOOL};
    use windows::Win32::Foundation::{DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK};
    use windows::Win32::System::Ole::{IDropSource, IDropSource_Impl, DROPEFFECT};
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;

    /// The drag source: the drag ends with a drop when the left button is let go, and with a cancel on Esc.
    #[implement(IDropSource)]
    pub struct Source;

    impl IDropSource_Impl for Source_Impl {
        fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape.as_bool() {
                DRAGDROP_S_CANCEL
            } else if keys.0 & 0x0001 == 0 {
                DRAGDROP_S_DROP
            } else {
                S_OK
            }
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }
}

/// What a drag ended as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragEnd {
    /// Dropped and copied.
    Copied,
    /// Dropped and moved (the files are gone from here).
    Moved,
    /// Dropped as a shortcut or something else.
    Other,
    /// Cancelled, or dropped where nothing takes files.
    None,
}

/// Start dragging `paths` (all in one folder) out of the window: Explorer, other file managers, chat and mail programs
/// accept them like files dragged from Explorer. Blocks until the button is released; call it from a mouse-move handler
/// while the left button is down.
pub fn drag_files(paths: &[PathBuf]) -> Result<DragEnd, String> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
    use windows::Win32::System::Ole::{DoDragDrop, OleInitialize, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{ILFindLastID, SHCreateDataObject, SHParseDisplayName};
    let first = paths.first().ok_or("nothing to drag")?;
    let parent = first.parent().ok_or("no folder")?;
    unsafe {
        let _ = OleInitialize(None);
        let parse = |p: &Path| -> Result<*mut ITEMIDLIST, String> {
            let mut w: Vec<u16> = p.as_os_str().encode_wide().collect();
            w.push(0);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok(pidl)
        };
        let parent_pidl = parse(parent)?;
        let mut abs: Vec<*mut ITEMIDLIST> = Vec::new();
        for p in paths {
            match parse(p) {
                Ok(x) => abs.push(x),
                Err(e) => {
                    for a in abs {
                        CoTaskMemFree(Some(a as *const c_void));
                    }
                    CoTaskMemFree(Some(parent_pidl as *const c_void));
                    return Err(e);
                }
            }
        }
        let children: Vec<*const ITEMIDLIST> = abs.iter().map(|&a| ILFindLastID(a) as *const ITEMIDLIST).collect();
        let data: Result<IDataObject, _> = SHCreateDataObject(Some(parent_pidl as *const ITEMIDLIST), Some(&children), None::<&IDataObject>);
        let result = match data {
            Ok(data) => {
                let src: windows::Win32::System::Ole::IDropSource = dnd::Source.into();
                let mut effect = DROPEFFECT(0);
                let hr = DoDragDrop(&data, &src, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect);
                let _ = data.cast::<windows::core::IUnknown>();
                if hr.0 == 0x0004_0100 {
                    Ok(match effect {
                        e if e == DROPEFFECT_MOVE => DragEnd::Moved,
                        e if e == DROPEFFECT_COPY => DragEnd::Copied,
                        e if e.0 == 0 => DragEnd::None,
                        _ => DragEnd::Other,
                    })
                } else {
                    Ok(DragEnd::None)
                }
            }
            Err(e) => Err(format!("drag: {e}")),
        };
        for a in abs {
            CoTaskMemFree(Some(a as *const c_void));
        }
        CoTaskMemFree(Some(parent_pidl as *const c_void));
        result
    }
}
