//! Single instance: when NuViewer is already running, a second launch hands its request (a path, maybe "open in
//! gallery") to the existing window via WM_COPYDATA and exits, so double-clicking images reuses one window.

use std::path::Path;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, SendMessageTimeoutW, SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_COPYDATA,
};

/// High half tags our messages so a stray WM_COPYDATA from another program is ignored.
const TAG: usize = 0x4E56_0000;
const FLAG_GALLERY: usize = 1;
const FLAG_HAS_PATH: usize = 2;

/// How long a second launch keeps trying to reach a first instance that is still starting up or briefly busy.
const REACH_TIMEOUT_MS: u64 = 3000;

pub enum Instance {
    /// This process is the (only) viewer: carry on starting up.
    Primary,
    /// The request was handed to a running viewer: just exit.
    Forwarded,
}

/// What a forwarded launch asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRequest {
    pub path: Option<String>,
    pub gallery: bool,
}

pub fn encode(req: &OpenRequest) -> (usize, Vec<u16>) {
    let mut flags = TAG;
    if req.gallery {
        flags |= FLAG_GALLERY;
    }
    match &req.path {
        Some(p) => {
            flags |= FLAG_HAS_PATH;
            (flags, p.encode_utf16().chain(std::iter::once(0)).collect())
        }
        None => (flags, Vec::new()),
    }
}

/// None if `tag` isn't one of ours.
pub fn decode(tag: usize, payload: &[u16]) -> Option<OpenRequest> {
    if tag & 0xFFFF_0000 != TAG {
        return None;
    }
    let path = (tag & FLAG_HAS_PATH != 0).then(|| {
        let end = payload.iter().position(|&c| c == 0).unwrap_or(payload.len());
        String::from_utf16_lossy(&payload[..end])
    });
    Some(OpenRequest { path, gallery: tag & FLAG_GALLERY != 0 })
}

/// Try to hand `req` to the window `hwnd`. True only if that window actually processed it.
fn deliver(hwnd: HWND, req: &OpenRequest) -> bool {
    let (tag, payload) = encode(req);
    let cds = COPYDATASTRUCT { dwData: tag, cbData: (payload.len() * 2) as u32, lpData: payload.as_ptr() as *mut _ };
    unsafe {
        // Let the existing window take the foreground (we hold the right to: the user just launched us).
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let _ = AllowSetForegroundWindow(pid);
        let mut result = 0usize;
        let sent = SendMessageTimeoutW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG | SMTO_BLOCK, 3000, Some(&mut result));
        sent.0 != 0 && result == 1
    }
}

/// Become the primary instance, or forward `req` to the running one.
/// If a running instance can't be reached (hung, or it opened without a window), we just carry on as a new window.
pub fn claim_or_forward(req: &OpenRequest) -> Instance {
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\NuViewer.SingleInstance.v1"));
        let already_running = GetLastError() == ERROR_ALREADY_EXISTS;
        match (mutex, already_running) {
            (Ok(handle), false) => {
                let _ = handle; // HANDLE has no Drop: the mutex stays held until this process exits, which is what releases it
                Instance::Primary
            }
            (Err(_), _) => Instance::Primary, // couldn't even create the mutex: don't get in the way
            (Ok(_), true) => {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(REACH_TIMEOUT_MS);
                while std::time::Instant::now() < deadline {
                    if let Ok(hwnd) = FindWindowW(w!("NuViewerWindow"), PCWSTR::null()) {
                        if deliver(hwnd, req) {
                            return Instance::Forwarded;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Instance::Primary
            }
        }
    }
}

/// Make `hwnd` visible and focused (restoring it if minimised).
pub fn bring_to_front(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Decode a WM_COPYDATA lParam into a request (None if it isn't ours or is malformed).
///
/// # Safety
/// `lparam` must be the lParam of a WM_COPYDATA message.
pub(crate) unsafe fn request_from_message(lparam: LPARAM) -> Option<OpenRequest> {
    let cds = (lparam.0 as *const COPYDATASTRUCT).as_ref()?;
    let units = if cds.lpData.is_null() { &[][..] } else { std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2) };
    decode(cds.dwData, units)
}

/// Absolute form of a path given on the command line (relative paths mean nothing to another process).
pub fn absolute(p: &Path) -> std::path::PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        for req in [
            OpenRequest { path: Some("C:\\Pics\\รูป 1.jpg".into()), gallery: false },
            OpenRequest { path: Some("D:\\x".into()), gallery: true },
            OpenRequest { path: None, gallery: false },
        ] {
            let (tag, payload) = encode(&req);
            assert_eq!(decode(tag, &payload), Some(req));
        }
    }

    #[test]
    fn foreign_messages_are_ignored() {
        assert_eq!(decode(0x1234_0003, &[65, 0]), None);
        assert_eq!(decode(0, &[]), None);
    }

    #[test]
    fn missing_terminator_is_tolerated() {
        let (tag, _) = encode(&OpenRequest { path: Some("ab".into()), gallery: false });
        assert_eq!(decode(tag, &[97, 98]).unwrap().path.as_deref(), Some("ab"));
    }
}
