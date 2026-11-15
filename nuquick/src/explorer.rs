//! Which file does Explorer (or the desktop) have selected right now? Plain shell COM, no UI Automation:
//! `IShellWindows` -> the window's `IShellBrowser` -> its `IFolderView2`. Measured ~1.5 ms warm.

use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Lookup {
    sw: Option<IShellWindows>,
}

impl Lookup {
    pub fn new() -> Self {
        Lookup { sw: None }
    }

    /// The selected item (the marked one, else the focused one) of the Explorer window whose top-level window is `fg`
    /// (or of the desktop when `desktop`). `any` accepts the first Explorer window when none matches (test runs only).
    pub fn selected(&mut self, fg: HWND, desktop: bool, any: bool, only_under: Option<&Path>) -> Option<PathBuf> {
        for attempt in 0..2 {
            if self.sw.is_none() {
                self.sw = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok() };
            }
            let sw = self.sw.clone()?;
            match unsafe { self.find(&sw, fg, desktop, any, only_under) } {
                Found::Item(p) => return Some(p),
                Found::Nothing => return None,
                // The shell restarted or a call failed: ask for a fresh object once.
                Found::Broken if attempt == 0 => self.sw = None,
                Found::Broken => return None,
            }
        }
        None
    }

    unsafe fn find(&self, sw: &IShellWindows, fg: HWND, desktop: bool, any: bool, only_under: Option<&Path>) -> Found {
        // Tests look only inside their own temp folder, so a picture selected in the user's own windows is never picked up.
        let item = |b: &IShellBrowser| item_of(b).filter(|p| only_under.map(|d| p.starts_with(d)).unwrap_or(true));
        if desktop {
            let mut hwnd = 0i32;
            let r = sw.FindWindowSW(&VARIANT::from(CSIDL_DESKTOP as i32), &VARIANT::default(), SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH);
            let Ok(disp) = r else { return Found::Broken };
            return match browser_of(&disp).and_then(|b| item(&b)) {
                Some(p) => Found::Item(p),
                None => Found::Nothing,
            };
        }
        let Ok(n) = sw.Count() else { return Found::Broken };
        let mut visible: Option<IShellBrowser> = None;
        let mut first: Option<IShellBrowser> = None;
        let mut fallback: Vec<IShellBrowser> = Vec::new();
        for i in 0..n {
            let Ok(disp) = sw.Item(&VARIANT::from(i)) else { continue };
            let Some(sb) = browser_of(&disp) else { continue };
            let Ok(win) = sb.GetWindow() else { continue };
            if GetAncestor(win, GA_ROOT) == fg {
                // With tabs, several browsers share one top-level window; the shown tab's view is the visible one.
                if visible.is_none() && IsWindowVisible(win).as_bool() {
                    visible = Some(sb.clone());
                }
                if first.is_none() {
                    first = Some(sb);
                }
            } else if any {
                fallback.push(sb);
            }
        }
        let found = visible.into_iter().chain(first).chain(fallback).find_map(|b| item(&b));
        match found {
            Some(p) => Found::Item(p),
            None => Found::Nothing,
        }
    }
}

enum Found {
    Item(PathBuf),
    Nothing,
    Broken,
}

unsafe fn browser_of(disp: &windows::Win32::System::Com::IDispatch) -> Option<IShellBrowser> {
    let sp = disp.cast::<IServiceProvider>().ok()?;
    sp.QueryService::<IShellBrowser>(&SID_STopLevelBrowser).ok()
}

unsafe fn item_of(browser: &IShellBrowser) -> Option<PathBuf> {
    let view = browser.QueryActiveShellView().ok()?;
    let fv: IFolderView2 = view.cast().ok()?;
    // The marked item exists only while something is selected (-1 = nothing): a bare focus rectangle is not a selection.
    let idx = fv.GetSelectionMarkedItem().ok().filter(|i| *i >= 0).or_else(|| fv.GetFocusedItem().ok().filter(|i| *i >= 0 && fv.ItemCount(SVGIO_SELECTION).unwrap_or(0) > 0))?;
    let item: IShellItem = fv.GetItem(idx).ok()?;
    let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let path = name.to_string().ok();
    CoTaskMemFree(Some(name.0 as *const _));
    path.map(PathBuf::from)
}
