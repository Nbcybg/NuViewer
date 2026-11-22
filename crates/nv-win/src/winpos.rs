//! Where a window opens. One rule for every program: a window comes back where it was (and maximised if it was), but never half off a
//! screen, never taller than the work area, never on a screen that is gone; with nothing remembered it opens in the middle of the main
//! screen's work area (the part the taskbar leaves). `fit` is that rule as plain arithmetic; the rest reads and writes the file and asks
//! Windows for the screens. `NuNote` uses `place` / `show` / `save`; the viewer, the gallery and the editor share one remembered
//! rectangle (`winstate`) and use `fit` for it.

use std::path::PathBuf;

/// (left, top, right, bottom) of a screen's work area, in screen pixels.
pub type Area = (i32, i32, i32, i32);

/// The result of `fit`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// x, y, width, height of the window (outer).
    pub rect: (i32, i32, i32, i32),
    /// Opens maximised (the rectangle is then the one it returns to).
    pub maximized: bool,
    /// Nothing usable was remembered: the window was centred.
    pub centred: bool,
}

fn centre_in(area: Area, w: i32, h: i32) -> (i32, i32, i32, i32) {
    let (aw, ah) = ((area.2 - area.0).max(1), (area.3 - area.1).max(1));
    let (w, h) = (w.clamp(1, aw), h.clamp(1, ah));
    (area.0 + (aw - w) / 2, area.1 + (ah - h) / 2, w, h)
}

fn overlap(a: (i32, i32, i32, i32), area: Area) -> (i32, i32) {
    let (x0, y0, x1, y1) = (a.0.max(area.0), a.1.max(area.1), (a.0 + a.2).min(area.2), (a.1 + a.3).min(area.3));
    ((x1 - x0).max(0), (y1 - y0).max(0))
}

/// The placement for a window of `default` size when `saved` is what was remembered, on screens with these `works` areas (`primary` is
/// the main screen's index).
pub fn fit(saved: Option<(i32, i32, i32, i32)>, maximized: bool, works: &[Area], primary: usize, default: (i32, i32)) -> Placement {
    let Some(&main) = works.get(primary).or_else(|| works.first()) else {
        // No screen information at all (cannot happen on a running Windows): trust the numbers.
        return Placement { rect: saved.unwrap_or((100, 100, default.0, default.1)), maximized, centred: saved.is_none() };
    };
    let Some(s) = saved.filter(|r| r.2 >= 200 && r.3 >= 150) else {
        return Placement { rect: centre_in(main, default.0, default.1), maximized: false, centred: true };
    };
    // The screen the window is on: the one that shows most of it. It has to show enough to grab the title bar and see something.
    let best = works.iter().copied().map(|a| (overlap(s, a), a)).filter(|((ow, oh), _)| *ow >= 120 && *oh >= 50).max_by_key(|((ow, oh), _)| *ow as i64 * *oh as i64).map(|(_, a)| a);
    match best {
        None => {
            // Its screen is gone (a disconnected monitor, a changed layout): the middle of the main one, at the size it had.
            Placement { rect: centre_in(main, s.2, s.3), maximized, centred: true }
        }
        Some(area) => {
            let (aw, ah) = (area.2 - area.0, area.3 - area.1);
            let (w, h) = (s.2.min(aw), s.3.min(ah));
            let x = s.0.clamp(area.0, (area.2 - w).max(area.0));
            let y = s.1.clamp(area.1, (area.3 - h).max(area.1));
            Placement { rect: (x, y, w, h), maximized, centred: false }
        }
    }
}

#[cfg(windows)]
mod sys {
    use super::Area;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};

    /// The work area of every screen and which one is the main screen.
    pub fn work_areas() -> (Vec<Area>, usize) {
        unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
            let list = &mut *(data.0 as *mut Vec<(Area, bool)>);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(m, &mut mi).as_bool() {
                list.push(((mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom), mi.dwFlags & 1 != 0));
            }
            true.into()
        }
        let mut list: Vec<(Area, bool)> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
        }
        let primary = list.iter().position(|(_, p)| *p).unwrap_or(0);
        (list.into_iter().map(|(a, _)| a).collect(), primary)
    }
}

#[cfg(windows)]
pub use sys::work_areas;

/// The remembered placement of program `app`: `%APPDATA%\NuViewer\<app>.window.ini` (`rect = x,y,w,h`, `maximized = 0 | 1`).
pub fn file_of(app: &str) -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("NuViewer").join(format!("{}.window.ini", app.to_ascii_lowercase()))
}

pub fn parse(text: &str) -> (Option<(i32, i32, i32, i32)>, bool) {
    let (mut rect, mut max) = (None, false);
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "rect" => {
                let n: Vec<i32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
                if let [x, y, w, h] = n[..] {
                    rect = Some((x, y, w, h));
                }
            }
            "maximized" => max = matches!(v.trim(), "1" | "true"),
            _ => {}
        }
    }
    (rect, max)
}

pub fn serialize(rect: (i32, i32, i32, i32), maximized: bool) -> String {
    format!("rect = {},{},{},{}\nmaximized = {}\n", rect.0, rect.1, rect.2, rect.3, if maximized { 1 } else { 0 })
}

/// x, y, width, height for a new window of program `app`: where it was, or centred, as `fit` says.
#[cfg(windows)]
pub fn place(app: &str, default_w: i32, default_h: i32) -> (i32, i32, i32, i32) {
    let (rect, max) = std::fs::read_to_string(file_of(app)).map(|t| parse(&t)).unwrap_or((None, false));
    let (works, primary) = work_areas();
    fit(rect, max, &works, primary, (default_w, default_h)).rect
}

/// Show window `hwnd` of program `app`: maximised when it was.
#[cfg(windows)]
pub fn show(app: &str, hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::Graphics::Gdi::UpdateWindow;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOW, SW_SHOWMAXIMIZED};
    let (_, max) = std::fs::read_to_string(file_of(app)).map(|t| parse(&t)).unwrap_or((None, false));
    unsafe {
        let _ = ShowWindow(hwnd, if max { SW_SHOWMAXIMIZED } else { SW_SHOW });
        let _ = UpdateWindow(hwnd);
    }
}

/// Write down where window `hwnd` of program `app` is (its normal rectangle, and whether it is maximised).
#[cfg(windows)]
pub fn save(app: &str, hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, IsIconic, IsWindow, SW_SHOWMAXIMIZED, WINDOWPLACEMENT};
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() || IsIconic(hwnd).as_bool() {
            return;
        }
        let mut p = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        if GetWindowPlacement(hwnd, &mut p).is_err() {
            return;
        }
        let r = p.rcNormalPosition;
        let text = serialize((r.left, r.top, r.right - r.left, r.bottom - r.top), p.showCmd == SW_SHOWMAXIMIZED.0 as u32);
        let path = file_of(app);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::read_to_string(&path).map(|old| old != text).unwrap_or(true) {
            let _ = std::fs::write(path, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 1920x1080 main screen with a 40 px taskbar at the bottom, and a second 1280x1024 screen to its left.
    const MAIN: Area = (0, 0, 1920, 1040);
    const LEFT: Area = (-1280, 0, 0, 1024);

    #[test]
    fn nothing_remembered_opens_in_the_middle_of_the_work_area() {
        let p = fit(None, false, &[MAIN], 0, (1280, 800));
        assert_eq!(p.rect, (320, 120, 1280, 800), "centred in 1920 x 1040, not 1080: the taskbar is not counted");
        assert!(p.centred);
        // A window bigger than the screen shrinks to it.
        assert_eq!(fit(None, false, &[MAIN], 0, (3000, 2000)).rect, (0, 0, 1920, 1040));
        // The main screen is the centre even when it is not the first in the list.
        assert_eq!(fit(None, false, &[LEFT, MAIN], 1, (1000, 600)).rect, (460, 220, 1000, 600));
    }

    #[test]
    fn a_remembered_place_is_kept_when_it_still_fits() {
        let p = fit(Some((200, 100, 900, 700)), false, &[MAIN], 0, (1280, 800));
        assert_eq!((p.rect, p.centred, p.maximized), ((200, 100, 900, 700), false, false));
        // Maximised comes back maximised, over the rectangle it returns to.
        assert!(fit(Some((200, 100, 900, 700)), true, &[MAIN], 0, (1280, 800)).maximized);
        // On the second screen.
        assert_eq!(fit(Some((-1200, 50, 800, 600)), false, &[MAIN, LEFT], 0, (1280, 800)).rect, (-1200, 50, 800, 600));
    }

    #[test]
    fn a_window_that_hangs_off_a_screen_is_pulled_back_in() {
        // Half out to the right and below.
        assert_eq!(fit(Some((1500, 800, 900, 700)), false, &[MAIN], 0, (1280, 800)).rect, (1020, 340, 900, 700));
        // Title bar above the top of the screen.
        assert_eq!(fit(Some((100, -80, 900, 700)), false, &[MAIN], 0, (1280, 800)).rect.1, 0);
        // Bigger than the work area (it was saved on a bigger screen): shrunk, then placed.
        assert_eq!(fit(Some((0, 0, 2560, 1400)), false, &[MAIN], 0, (1280, 800)).rect, (0, 0, 1920, 1040));
    }

    #[test]
    fn a_window_on_a_screen_that_is_gone_is_centred_on_the_main_one() {
        // It was on the left screen, which is no longer there.
        let p = fit(Some((-1200, 50, 800, 600)), false, &[MAIN], 0, (1280, 800));
        assert_eq!(p.rect, (560, 220, 800, 600), "at its own size, in the middle of what is left");
        assert!(p.centred);
        // Hidden away at the Windows 'minimised' position.
        assert!(fit(Some((-32000, -32000, 900, 700)), false, &[MAIN], 0, (1280, 800)).centred);
        // Only a sliver on a screen is not enough to count as being there.
        assert!(fit(Some((1900, 100, 900, 700)), false, &[MAIN], 0, (1280, 800)).rect.0 <= 1920 - 900);
    }

    #[test]
    fn nonsense_in_the_file_is_ignored() {
        assert_eq!(fit(Some((10, 10, 50, 40)), false, &[MAIN], 0, (1280, 800)).centred, true, "too small to be a window");
        assert_eq!(parse("rect = 1,2,3"), (None, false));
        assert_eq!(parse("rect = 10, 20, 800, 600\nmaximized = 1\n"), (Some((10, 20, 800, 600)), true));
        let (r, m) = parse(&serialize((-5, 7, 640, 480), true));
        assert_eq!((r, m), (Some((-5, 7, 640, 480)), true));
        assert_eq!(file_of("NuNote").file_name().unwrap(), "nunote.window.ini");
        // No screens at all: the numbers are trusted.
        assert_eq!(fit(Some((1, 2, 300, 300)), false, &[], 0, (1280, 800)).rect, (1, 2, 300, 300));
    }

    #[test]
    fn the_real_screens_make_sense() {
        let (works, primary) = work_areas();
        assert!(!works.is_empty() && primary < works.len());
        let (l, t, r, b) = works[primary];
        assert!(r > l && b > t);
    }
}
