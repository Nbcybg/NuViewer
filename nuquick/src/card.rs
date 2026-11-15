//! The info card: what Space shows for something that is not a picture we can decode (a folder, a document, a video, a program).
//! Like Quick Look's: the file's own icon (or Explorer's thumbnail when it has one), then a few facts. Built on the decode thread
//! (the shell may take a while to make a thumbnail), never on the main thread.

use crate::view::{human_size, Preview};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::*;

const BG: u32 = 0x2A;

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// The icon (or thumbnail) of `path` as top-down BGRA, `px` square at most. `icon_only` skips thumbnails (folders).
fn shell_image(path: &Path, px: i32, icon_only: bool) -> Option<(i32, i32, Vec<u8>)> {
    unsafe {
        let w = wide(path);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let flags = if icon_only { SIIGBF_ICONONLY } else { SIIGBF_BIGGERSIZEOK };
        let hbm = factory.GetImage(SIZE { cx: px, cy: px }, flags).ok()?;
        let mut bm = BITMAP::default();
        GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        let (bw, bh) = (bm.bmWidth, bm.bmHeight);
        if bw <= 0 || bh <= 0 || bw > 4096 || bh > 4096 {
            let _ = DeleteObject(hbm.into());
            return None;
        }
        let mut bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: bw, biHeight: -bh, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
        let mut buf = vec![0u8; (bw * bh * 4) as usize];
        let dc = GetDC(None);
        let got = GetDIBits(dc, hbm, 0, bh as u32, Some(buf.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(hbm.into());
        (got != 0).then_some((bw, bh, buf))
    }
}

/// "JPEG image", "File folder", "Microsoft Word Document": Windows' own name for the kind of file.
fn kind_of(path: &Path, is_dir: bool) -> String {
    unsafe {
        let w = wide(path);
        let mut sfi = SHFILEINFOW::default();
        let r = SHGetFileInfoW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut sfi), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_TYPENAME);
        let end = sfi.szTypeName.iter().position(|&c| c == 0).unwrap_or(sfi.szTypeName.len());
        let name = String::from_utf16_lossy(&sfi.szTypeName[..end]);
        if r != 0 && !name.is_empty() {
            return name;
        }
    }
    if is_dir {
        return "Folder".to_string();
    }
    match path.extension() {
        Some(e) => format!("{} file", e.to_string_lossy().to_ascii_uppercase()),
        None => "File".to_string(),
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Local date and time, "16 Nov 2026, 02:55".
fn when(t: std::time::SystemTime) -> String {
    let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) else { return String::new() };
    let secs = d.as_secs() as i64 + nv_win::utc_offset_minutes() as i64 * 60;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 -> civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{day} {} {year}, {:02}:{:02}", MONTHS[(month - 1) as usize], rem / 3600, rem % 3600 / 60)
}

fn rows_for(path: &Path, note: Option<&str>) -> (Vec<(String, String)>, String) {
    let md = std::fs::metadata(path).ok();
    let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or_else(|| path.is_dir());
    let kind = kind_of(path, is_dir);
    let mut rows = Vec::new();
    if let Some(n) = note {
        rows.push(("Preview".to_string(), n.to_string()));
    }
    rows.push(("Kind".to_string(), kind.clone()));
    if is_dir {
        // Counting stops at a few thousand: a huge folder on a slow drive must not hold the card up.
        let n = std::fs::read_dir(path).map(|rd| rd.take(5000).count()).unwrap_or(0);
        rows.push(("Contains".to_string(), match n {
            1 => "1 item".to_string(),
            5000 => "5,000 or more items".to_string(),
            n => format!("{} items", thousands(n as u64)),
        }));
    } else if let Some(m) = &md {
        rows.push(("Size".to_string(), format!("{} ({} bytes)", human_size(m.len()), thousands(m.len()))));
    }
    if let Some(m) = &md {
        if let Ok(c) = m.created() {
            rows.push(("Created".to_string(), when(c)));
        }
        if let Ok(t) = m.modified() {
            rows.push(("Modified".to_string(), when(t)));
        }
    }
    if let Some(parent) = path.parent() {
        rows.push(("Where".to_string(), parent.display().to_string()));
    }
    (rows, kind)
}

/// The card for `path`; `note` says why there is no picture ("Can't show this picture") when it is a picture that failed.
pub fn build(path: &Path, note: Option<&str>) -> Preview {
    let t0 = std::time::Instant::now();
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    let s = |v: f32| (v * scale).round() as i32;
    let is_dir = path.is_dir();
    let (rows, kind) = rows_for(path, note);
    let (w, icon_px, row_h) = (s(520.0), s(176.0), s(26.0));
    let top_gap = s(26.0);
    let row_top = top_gap + icon_px + s(20.0);
    let h = row_top + rows.len() as i32 * row_h + s(22.0);
    let mut bgra = [BG as u8, BG as u8, BG as u8, 0xFF].repeat((w * h) as usize);
    let mut big = None;
    match shell_image(path, icon_px, is_dir) {
        Some((iw, ih, px)) => {
            // Draw it centred in the icon area, composited over the card's colour (the bitmap is premultiplied; an all-zero alpha
            // channel means it has none).
            let opaque = px.chunks_exact(4).all(|p| p[3] == 0);
            let (x0, y0) = ((w - iw) / 2, top_gap + (icon_px - ih) / 2);
            for y in 0..ih {
                for x in 0..iw {
                    let (dx, dy) = (x0 + x, y0 + y);
                    if dx < 0 || dy < 0 || dx >= w || dy >= h {
                        continue;
                    }
                    let p = &px[((y * iw + x) * 4) as usize..][..4];
                    let a = if opaque { 255 } else { p[3] as u32 };
                    let d = &mut bgra[((dy * w + dx) * 4) as usize..][..4];
                    for c in 0..3 {
                        d[c] = (p[c] as u32 + (BG * (255 - a) + 127) / 255).min(255) as u8;
                    }
                }
            }
        }
        None => {
            let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_uppercase()).unwrap_or_default();
            big = Some(if is_dir { "FOLDER".to_string() } else if ext.is_empty() { "FILE".to_string() } else { ext });
        }
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    Preview { w, h, bgra, name, info: kind, decode_ms: t0.elapsed().as_secs_f64() * 1000.0, big, rows, row_top, row_h }
}
