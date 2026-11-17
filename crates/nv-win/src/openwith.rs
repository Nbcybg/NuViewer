//! "Open with": the programs Windows offers for a kind of file (the list Explorer's Open with menu shows), a way to start one
//! of them on a file, and the "Choose another app" dialog.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::UI::Shell::{IAssocHandler, IShellItem, SHAssocEnumHandlers, SHCreateItemFromParsingName, ASSOC_FILTER, ASSOC_FILTER_NONE, ASSOC_FILTER_RECOMMENDED, BHID_DataObject};

/// A program that can open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    /// Windows' name for it (its program path or ProgID): what `open` is given.
    pub id: String,
    /// What the menu shows ("Paint", "Photos").
    pub name: String,
}

fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

unsafe fn take_string(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
    s
}

fn each_handler(path: &Path, filter: ASSOC_FILTER, mut f: impl FnMut(IAssocHandler) -> bool) {
    let Some(ext) = path.extension() else { return };
    let dotted = wide(std::ffi::OsStr::new(&format!(".{}", ext.to_string_lossy())));
    unsafe {
        let _ = crate::com_init();
        let Ok(en) = SHAssocEnumHandlers(PCWSTR(dotted.as_ptr()), filter) else { return };
        loop {
            let mut one: [Option<IAssocHandler>; 1] = [None];
            let mut got = 0u32;
            let hr = en.Next(&mut one, Some(&mut got));
            if hr.is_err() || got == 0 {
                break;
            }
            let Some(h) = one[0].take() else { break };
            if !f(h) {
                break;
            }
        }
    }
}

/// The programs Windows recommends for `path`'s kind of file, in the order Explorer shows them.
pub fn handlers(path: &Path) -> Vec<Handler> {
    let mut out: Vec<Handler> = Vec::new();
    each_handler(path, ASSOC_FILTER_RECOMMENDED, |h| {
        let (id, name) = unsafe { (h.GetName().map(|p| take_string(p)).unwrap_or_default(), h.GetUIName().map(|p| take_string(p)).unwrap_or_default()) };
        if !id.is_empty() && !name.is_empty() && !out.iter().any(|o| o.id == id) {
            out.push(Handler { id, name });
        }
        true
    });
    out
}

/// Open `path` with the program `id` (from `handlers`).
pub fn open(path: &Path, id: &str) -> Result<(), String> {
    let mut found: Option<IAssocHandler> = None;
    for filter in [ASSOC_FILTER_RECOMMENDED, ASSOC_FILTER_NONE] {
        each_handler(path, filter, |h| {
            let name = unsafe { h.GetName().map(|p| take_string(p)).unwrap_or_default() };
            if name == id {
                found = Some(h);
                false
            } else {
                true
            }
        });
        if found.is_some() {
            break;
        }
    }
    let handler = found.ok_or("that program is not available any more")?;
    unsafe {
        let w = wide(path.as_os_str());
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).map_err(|e| e.to_string())?;
        let data: IDataObject = item.BindToHandler(None, &BHID_DataObject).map_err(|e| e.to_string())?;
        let invoker = handler.CreateInvoker(&data).map_err(|e| e.to_string())?;
        invoker.Invoke().map_err(|e| e.to_string())
    }
}

/// Windows' own "How do you want to open this file?" window.
pub fn choose(hwnd: HWND, path: &Path) -> Result<(), String> {
    crate::files::open_as(hwnd, path)
}
