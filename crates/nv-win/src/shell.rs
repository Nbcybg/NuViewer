//! Windows shell integration: Recycle Bin, folder picker, clipboard, Explorer.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, SHFileOperationW, DROPFILES, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
    SHFILEOPSTRUCTW, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().collect()
}

/// Send `path` to the Recycle Bin. Uses the shell, so if Windows would delete it permanently instead
/// (file too big for the bin, bin disabled) it shows its own warning rather than doing it silently.
pub fn recycle(hwnd: HWND, path: &Path) -> Result<(), String> {
    let mut from = wide(path.as_os_str());
    from.extend([0, 0]); // double-NUL terminated list
    let mut op = SHFILEOPSTRUCTW {
        hwnd,
        wFunc: 3,                       // FO_DELETE
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: 0x0040 | 0x0004 | 0x4000, // FOF_ALLOWUNDO | FOF_SILENT | FOF_WANTNUKEWARNING
        ..Default::default()
    };
    let rc = unsafe { SHFileOperationW(&mut op) };
    if rc != 0 || op.fAnyOperationsAborted.as_bool() {
        return Err(if op.fAnyOperationsAborted.as_bool() { "cancelled".into() } else { format!("shell error 0x{rc:X}") });
    }
    if path.exists() {
        return Err("file is still there".into());
    }
    Ok(())
}

/// OK message box. Owned by the viewer's window when there is one (so it stays on top of it and Alt+Tab finds it),
/// otherwise a plain box for the command-line modes that have no window of their own.
pub fn message(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK};
    let mut t: Vec<u16> = text.encode_utf16().collect();
    t.push(0);
    unsafe {
        MessageBoxW(crate::dialog_parent(), PCWSTR(t.as_ptr()), w!("NuViewer"), MB_OK | MB_ICONINFORMATION);
    }
}

/// Modal Yes/No warning; defaults to No.
pub fn confirm(hwnd: HWND, text: &str) -> bool {
    let mut t: Vec<u16> = text.encode_utf16().collect();
    t.push(0);
    unsafe { MessageBoxW(Some(hwnd), PCWSTR(t.as_ptr()), w!("NuViewer"), MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2) == IDYES }
}

/// The answer to a Yes / No / Cancel question.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Yes,
    No,
    Cancel,
}

/// Modal Yes / No / Cancel question; Cancel is the default and also what Esc and the close box give.
pub fn ask3(hwnd: HWND, text: &str) -> Answer {
    use windows::Win32::UI::WindowsAndMessaging::{IDNO, MB_DEFBUTTON3, MB_ICONQUESTION, MB_YESNOCANCEL};
    let mut t: Vec<u16> = text.encode_utf16().collect();
    t.push(0);
    let r = unsafe { MessageBoxW(Some(hwnd), PCWSTR(t.as_ptr()), w!("NuViewer"), MB_YESNOCANCEL | MB_ICONQUESTION | MB_DEFBUTTON3) };
    if r == IDYES {
        Answer::Yes
    } else if r == IDNO {
        Answer::No
    } else {
        Answer::Cancel
    }
}

/// Native folder picker. None if cancelled.
pub fn pick_folder(hwnd: HWND, title: &str, initial: Option<&Path>) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opts = dlg.GetOptions().ok()?;
        dlg.SetOptions(opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM).ok()?;
        let mut t: Vec<u16> = title.encode_utf16().collect();
        t.push(0);
        let _ = dlg.SetTitle(PCWSTR(t.as_ptr()));
        if let Some(dir) = initial {
            let mut d = wide(dir.as_os_str());
            d.push(0);
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(d.as_ptr()), None) {
                let _ = dlg.SetFolder(&item);
            }
        }
        dlg.Show(Some(hwnd)).ok()?; // Err(ERROR_CANCELLED) when dismissed
        let item = dlg.GetResult().ok()?;
        let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = name.to_string().ok();
        CoTaskMemFree(Some(name.0 as *const c_void));
        s.map(PathBuf::from)
    }
}

/// Open Explorer with `path` selected.
pub fn reveal_in_explorer(path: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
}

/// Another program (clipboard managers, remote-desktop sync) may hold the clipboard for a moment: retry briefly.
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

/// Put the file (paste into Explorer) and its pixels (paste into Paint etc.) on the clipboard.
/// `bgra` is top-down 32-bit; alpha is flattened onto white because CF_DIB consumers disagree about it.
pub fn clipboard_copy_image_and_file(hwnd: HWND, path: &Path, width: u32, height: u32, bgra: &[u8]) -> Result<(), String> {
    unsafe {
        // CF_HDROP payload: DROPFILES header followed by a double-NUL-terminated wide path list.
        let mut files = wide(path.as_os_str());
        files.extend([0, 0]);
        let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
        let mut drop_bytes = std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()).to_vec();
        drop_bytes.extend(files.iter().flat_map(|c| c.to_le_bytes()));

        // CF_DIB payload: BITMAPINFOHEADER + bottom-up rows.
        let (w, h) = (width as usize, height as usize);
        let mut dib = Vec::with_capacity(40 + w * h * 4);
        for v in [40u32, width, height] {
            dib.extend(v.to_le_bytes());
        }
        dib.extend(1u16.to_le_bytes()); // planes
        dib.extend(32u16.to_le_bytes()); // bit count
        for v in [0u32, (w * h * 4) as u32, 0, 0, 0, 0] {
            dib.extend(v.to_le_bytes()); // BI_RGB, image size, ppm x/y, colours used/important
        }
        for y in (0..h).rev() {
            for px in bgra[y * w * 4..(y + 1) * w * 4].chunks_exact(4) {
                let a = px[3] as u32;
                let blend = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                dib.extend([blend(px[0]), blend(px[1]), blend(px[2]), 255]);
            }
        }

        open_clipboard(hwnd)?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            SetClipboardData(CF_HDROP, Some(alloc_global(&drop_bytes)?)).map_err(|e| e.to_string())?;
            SetClipboardData(CF_DIB, Some(alloc_global(&dib)?)).map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

pub fn clipboard_copy_text(hwnd: HWND, text: &str) -> Result<(), String> {
    unsafe {
        let bytes: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect();
        open_clipboard(hwnd)?;
        let result = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| e.to_string())?;
            SetClipboardData(CF_UNICODETEXT, Some(alloc_global(&bytes)?)).map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Parse a Recycle Bin `$I` metadata file: (original path, deletion FILETIME).
fn parse_recycle_info(bytes: &[u8]) -> Option<(String, u64)> {
    if bytes.len() < 28 {
        return None;
    }
    let version = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let when = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let (start, chars) = match version {
        2 => (28usize, u32::from_le_bytes(bytes[24..28].try_into().ok()?) as usize),
        1 => (24usize, (bytes.len().saturating_sub(24)) / 2),
        _ => return None,
    };
    let end = (start + chars * 2).min(bytes.len());
    let units: Vec<u16> = bytes[start..end].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    Some((String::from_utf16(&units).ok()?, when))
}

/// Put the most recently recycled copy of `original` back where it was. Relies on the Recycle Bin's on-disk
/// layout (`<drive>\$Recycle.Bin\<SID>\$I… / $R…`), which has been stable since Windows Vista.
pub fn recycle_restore(original: &Path) -> Result<(), String> {
    let root = original.ancestors().last().ok_or("no drive")?.to_path_buf();
    let bin = root.join("$Recycle.Bin");
    let want = original.to_string_lossy().to_lowercase();
    let mut best: Option<(u64, PathBuf, PathBuf)> = None;
    for sid in std::fs::read_dir(&bin).map_err(|e| format!("can't read the Recycle Bin: {e}"))?.flatten() {
        let Ok(entries) = std::fs::read_dir(sid.path()) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("$I") {
                continue;
            }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            let Some((orig, when)) = parse_recycle_info(&bytes) else { continue };
            if orig.to_lowercase() != want {
                continue;
            }
            let r_path = e.path().with_file_name(format!("$R{}", &name[2..]));
            if r_path.exists() && best.as_ref().map(|b| when > b.0).unwrap_or(true) {
                best = Some((when, e.path(), r_path));
            }
        }
    }
    let (_, i_path, r_path) = best.ok_or("not found in the Recycle Bin")?;
    if original.exists() {
        return Err("a file with that name exists again".into());
    }
    std::fs::rename(&r_path, original).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(i_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info_v2(path: &str, when: u64) -> Vec<u8> {
        let units: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut b = Vec::new();
        b.extend(2u64.to_le_bytes());
        b.extend(1234u64.to_le_bytes());
        b.extend(when.to_le_bytes());
        b.extend((units.len() as u32).to_le_bytes());
        b.extend(units.iter().flat_map(|u| u.to_le_bytes()));
        b
    }

    #[test]
    fn parses_v2_info_files() {
        let (p, t) = parse_recycle_info(&info_v2("C:\\Pics\\รูป 1.jpg", 42)).unwrap();
        assert_eq!((p.as_str(), t), ("C:\\Pics\\รูป 1.jpg", 42));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_recycle_info(&[1, 2, 3]).is_none());
        assert!(parse_recycle_info(&[9u8; 64]).is_none());
    }
}

/// The system colour chooser. `initial` and the result are 0xRRGGBB. None if cancelled.
pub fn pick_color(hwnd: HWND, initial: u32) -> Option<u32> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
    let to_ref = |rgb: u32| COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF));
    let mut custom = [COLORREF(0x00FF_FFFF); 16];
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: to_ref(initial),
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    unsafe {
        if !ChooseColorW(&mut cc).as_bool() {
            return None;
        }
    }
    let c = cc.rgbResult.0;
    Some(((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF))
}

/// The system "Open" dialog for a program (.exe). None if cancelled.
pub fn pick_program(hwnd: HWND) -> Option<PathBuf> {
    pick_file(hwnd, "Programs (*.exe)\0*.exe\0All files\0*.*\0\0", None)
}

/// The system "Open" dialog for an ICC colour profile. None if cancelled.
pub fn pick_profile(hwnd: HWND, current: Option<&Path>) -> Option<PathBuf> {
    pick_file(hwnd, "Colour profiles (*.icc;*.icm)\0*.icc;*.icm\0All files\0*.*\0\0", current)
}

/// The system "Open" dialog. `filter` is the usual NUL-separated description / pattern list ending in a double NUL.
fn pick_file(hwnd: HWND, filter: &str, start_in: Option<&Path>) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let mut file = vec![0u16; 1024];
    let dir: Option<Vec<u16>> = start_in.and_then(|p| p.parent()).filter(|d| d.is_dir()).map(|d| d.as_os_str().encode_wide().chain(std::iter::once(0)).collect());
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: hwnd,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrInitialDir: dir.as_ref().map(|d| PCWSTR(d.as_ptr())).unwrap_or(PCWSTR::null()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    unsafe {
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None;
        }
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(PathBuf::from(String::from_utf16_lossy(&file[..len])))
}

/// Start `command` for the picture at `path`: `{file}` in the command line is replaced by the path (quote it in the
/// command if the program needs quotes); without `{file}` the quoted path is appended.
pub fn run_external(command: &str, path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let cmd = command.trim();
    let line = if cmd.contains("{file}") { cmd.replace("{file}", &path.display().to_string()) } else { format!("{cmd} \"{}\"", path.display()) };
    // Split off the program: a quoted path, or everything up to the first space.
    let (exe, args) = if let Some(rest) = line.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => (rest[..end].to_string(), rest[end + 1..].trim_start().to_string()),
            None => return Err("unbalanced quote in the command".into()),
        }
    } else {
        match line.split_once(' ') {
            Some((e, a)) => (e.to_string(), a.to_string()),
            None => (line.clone(), String::new()),
        }
    };
    if exe.is_empty() {
        return Err("the command is empty".into());
    }
    std::process::Command::new(&exe).raw_arg(args).spawn().map(|_| ()).map_err(|e| format!("couldn't start {exe}: {e}"))
}

#[cfg(test)]
mod external_tests {
    use super::*;

    fn wait_for(path: &Path) -> Option<String> {
        for _ in 0..100 {
            if let Ok(t) = std::fs::read_to_string(path) {
                if !t.is_empty() {
                    return Some(t);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        None
    }

    #[test]
    fn external_program_gets_the_picture_path() {
        let dir = std::env::temp_dir().join(format!("nv_ext_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (pic, out) = (dir.join("my picture.png"), dir.join("out.txt"));
        // {file} replaced; the program is a quoted path with spaces in the command line, as the Settings window writes it.
        let cmd = format!("\"C:\\Windows\\System32\\cmd.exe\" /c echo \"{{file}}\"> \"{}\"", out.display());
        run_external(&cmd, &pic).unwrap();
        let text = wait_for(&out).expect("the program ran");
        assert!(text.contains("my picture.png"), "{text}");
        // Without {file} the quoted path is appended.
        let out2 = dir.join("out2.txt");
        std::fs::write(dir.join("echo.cmd"), format!("@echo %~1> \"{}\"\r\n", out2.display())).unwrap();
        run_external(&format!("\"{}\"", dir.join("echo.cmd").display()), &pic).ok(); // .cmd needs the shell: failing to spawn is acceptable
        assert!(run_external("", &pic).is_err());
        assert!(run_external("\"unbalanced", &pic).is_err());
        assert!(run_external("this-program-does-not-exist-12345.exe", &pic).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
