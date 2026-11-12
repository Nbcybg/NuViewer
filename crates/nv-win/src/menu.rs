//! Native popup (context) menu.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

pub enum Entry {
    /// `id` must be non-zero. `text` may contain a tab: what follows is drawn right-aligned (the shortcut hint).
    Item { id: u32, text: String, checked: bool, enabled: bool },
    /// A nested menu (its items' ids are returned like any other).
    Submenu { text: String, items: Vec<Entry>, enabled: bool },
    Separator,
}

impl Entry {
    pub fn item(id: u32, text: impl Into<String>) -> Entry {
        Entry::Item { id, text: text.into(), checked: false, enabled: true }
    }

    pub fn checked(id: u32, text: impl Into<String>, checked: bool) -> Entry {
        Entry::Item { id, text: text.into(), checked, enabled: true }
    }

    pub fn enabled(self, on: bool) -> Entry {
        match self {
            Entry::Item { id, text, checked, .. } => Entry::Item { id, text, checked, enabled: on },
            Entry::Submenu { text, items, .. } => Entry::Submenu { text, items, enabled: on },
            sep => sep,
        }
    }
}

/// Show the menu at a point given in client pixels of `hwnd` and wait. Returns the chosen item's id, or None if dismissed.
pub fn popup(hwnd: HWND, x: i32, y: i32, entries: &[Entry]) -> Option<u32> {
    unsafe {
        let menu = build(entries)?;
        let mut pt = POINT { x, y };
        let _ = ClientToScreen(hwnd, &mut pt);
        // The window must be in front or the menu won't close when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_TOPALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)); // documented workaround so the menu dismisses cleanly
        let _ = DestroyMenu(menu); // also destroys the submenus
        let id = picked.0 as u32;
        (id != 0).then_some(id)
    }
}

unsafe fn build(entries: &[Entry]) -> Option<HMENU> {
    let menu = CreatePopupMenu().ok()?;
    for e in entries {
        match e {
            Entry::Separator => {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            Entry::Item { id, text, checked, enabled } => {
                let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let mut flags = MF_STRING;
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                let _ = AppendMenuW(menu, flags, *id as usize, PCWSTR(t.as_ptr()));
            }
            Entry::Submenu { text, items, enabled } => {
                if let Some(sub) = build(items) {
                    let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                    let mut flags = MF_POPUP | MF_STRING;
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    let _ = AppendMenuW(menu, flags, sub.0 as usize, PCWSTR(t.as_ptr()));
                }
            }
        }
    }
    Some(menu)
}
