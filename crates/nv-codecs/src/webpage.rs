//! Web pages (`.html` `.htm` `.xhtml` `.mhtml` `.mht`) shown as a picture of the page as a browser draws it.
//!
//! There is no HTML engine in the program (and there should not be: it would be bigger than everything else together). Windows
//! always ships one, Microsoft Edge, and Chrome is often installed too, so the page is handed to a **headless browser** that
//! writes a screenshot, which is kept in `%LOCALAPPDATA%\NuViewer\pages` (a page is drawn once, then it is a PNG).
//!
//! The browser runs with its name resolution switched off: a downloaded page is drawn from what is on the disk, it cannot
//! phone home or pull trackers just because it was previewed in a gallery. Scripts run (many pages draw themselves with
//! them) for at most a few virtual seconds. Without any browser the picture is a card with the page's title and text.

use crate::docthumb::{accent_for, fmt_bytes, render_card, text_lines, Card};
use crate::Image;
use std::path::{Path, PathBuf};

pub const EXTS: &[&str] = &["html", "htm", "xhtml", "mhtml", "mht"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// The page as a picture. Never fails for a readable file: no browser (or a page that hangs it) gives a text card.
#[allow(unused_assignments)]
pub(crate) fn decode(path: &Path, first_only: bool) -> Result<Image, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let size = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?.len();
    let mut why = String::new();
    #[cfg(windows)]
    match shot(path) {
        Ok(png) => match crate::decode_image_bytes(&png, "png") {
            Ok(mut img) => {
                img.colour_note = Some("the page as Microsoft Edge / Chrome draws it (offline, first screen)".into());
                img.profile_assumable = true;
                return Ok(img);
            }
            Err(e) => why = e,
        },
        Err(e) => why = e,
    }
    let head = std::fs::read(path).map(|mut b| {
        b.truncate(512 * 1024);
        b
    });
    let text = head.map(|b| plain_text(&String::from_utf8_lossy(&b))).unwrap_or_default();
    let (title, body) = text;
    let lines = text_lines(&body, 14);
    let scale = if first_only { 1 } else { 2 };
    let mut img = render_card(&Card { badge: "HTML".into(), accent: accent_for("html"), title: if title.is_empty() { name } else { title }, subtitle: fmt_bytes(size), lines }, scale);
    img.colour_note = Some(if why.is_empty() { "a card made from the page's text".to_string() } else { format!("a card made from the page's text (the page could not be drawn: {why})") });
    Ok(img)
}

/// Title and visible text of an HTML source, roughly: scripts, styles and tags dropped, entities for the common characters
/// resolved, one line per block element.
pub(crate) fn plain_text(html: &str) -> (String, String) {
    let lower = html.to_ascii_lowercase();
    let title = lower
        .find("<title")
        .and_then(|s| lower[s..].find('>').map(|g| s + g + 1))
        .and_then(|from| lower[from..].find("</title").map(|e| html[from..from + e].trim().to_string()))
        .map(|t| unescape(&t))
        .unwrap_or_default();
    let mut out = String::new();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Skip whole <script> / <style> / <head> blocks.
            let mut skipped = false;
            for tag in ["script", "style", "head", "!--"] {
                if lower[i + 1..].starts_with(tag) {
                    let end_mark = if tag == "!--" { "-->".to_string() } else { format!("</{tag}") };
                    match lower[i..].find(&end_mark) {
                        Some(e) => {
                            let after = i + e;
                            i = lower[after..].find('>').map(|g| after + g + 1).unwrap_or(bytes.len());
                        }
                        None => i = bytes.len(),
                    }
                    skipped = true;
                    break;
                }
            }
            if skipped {
                continue;
            }
            let close = lower[i..].find('>').map(|g| i + g + 1).unwrap_or(bytes.len());
            let tag = lower[i + 1..close.saturating_sub(1).max(i + 1)].trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            if matches!(tag, "p" | "br" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "table" | "section" | "article" | "header" | "footer" | "pre" | "blockquote") {
                out.push('\n');
            }
            i = close;
        } else {
            let next = html[i..].find('<').map(|n| i + n).unwrap_or(html.len());
            let chunk = &html[i..next];
            out.push_str(&chunk.split_whitespace().collect::<Vec<_>>().join(" "));
            if chunk.ends_with(char::is_whitespace) {
                out.push(' ');
            }
            i = next;
        }
    }
    (title, unescape(&out))
}

fn unescape(s: &str) -> String {
    s.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

// ---------------------------------------------------------------------------------------------------- the browser

#[cfg(windows)]
fn browser() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NUVIEWER_BROWSER").map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut c: Vec<PathBuf> = Vec::new();
    for root in [env("ProgramFiles(x86)"), env("ProgramFiles")].into_iter().flatten() {
        c.push(root.join("Microsoft").join("Edge").join("Application").join("msedge.exe"));
    }
    for root in [env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA")].into_iter().flatten() {
        c.push(root.join("Google").join("Chrome").join("Application").join("chrome.exe"));
    }
    c.into_iter().find(|p| p.is_file())
}

/// `file:///C:/a%20b/c.html` for a Windows path.
#[cfg(windows)]
fn file_url(path: &Path) -> String {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = full.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    let mut out = String::from("file:///");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(windows)]
fn cache_path(src: &Path) -> PathBuf {
    let meta = std::fs::metadata(src).ok();
    let (size, mtime) = meta.map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))).unwrap_or((0, 0));
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in src.to_string_lossy().to_lowercase().bytes().chain(size.to_le_bytes()).chain(mtime.to_le_bytes()).chain(*b"v1") {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("pages").join(format!("{h:016x}.png"))
}

/// PNG bytes of the page: from the cache, else from the headless browser (one at a time: a gallery of pages would
/// otherwise start a dozen browsers at once).
#[cfg(windows)]
fn shot(path: &Path) -> Result<Vec<u8>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    let out = cache_path(path);
    if let Ok(b) = std::fs::read(&out) {
        if b.len() > 64 {
            return Ok(b);
        }
    }
    let exe = browser().ok_or("no Edge or Chrome found")?;
    let _lock = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // Another thread may have made it while this one waited.
    if let Ok(b) = std::fs::read(&out) {
        if b.len() > 64 {
            return Ok(b);
        }
    }
    let dir = out.parent().ok_or("no cache folder")?.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cache folder: {e}"))?;
    let profile = dir.join(format!("profile-{}", std::process::id()));
    let tmp = dir.join(format!("tmp-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut c = Command::new(&exe);
    c.arg("--headless=new")
        .args(["--disable-gpu", "--no-first-run", "--no-default-browser-check", "--hide-scrollbars", "--disable-extensions", "--disable-sync", "--mute-audio"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--window-size=1280,960")
        .arg("--virtual-time-budget=3000")
        // No name resolution: the page can only use what is on this disk.
        .arg("--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost")
        .arg(format!("--screenshot={}", tmp.display()))
        .arg(file_url(path))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = c.spawn().map_err(|e| format!("could not start the browser: {e}"))?;
    let start = std::time::Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Ok(None) if start.elapsed() > std::time::Duration::from_secs(25) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("the browser took longer than 25 seconds".to_string());
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
            Err(e) => break Err(e.to_string()),
        }
    };
    let png = std::fs::read(&tmp);
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_dir_all(&profile);
    result?;
    let png = png.map_err(|_| "the browser wrote no picture".to_string())?;
    if png.len() < 64 {
        return Err("the browser wrote an empty picture".into());
    }
    let _ = std::fs::write(&out, &png);
    prune(&dir);
    Ok(png)
}

/// Keep the page pictures to a few hundred: the oldest go first.
#[cfg(windows)]
fn prune(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "png") && e.file_name().to_string_lossy().len() == 20).filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path()))).collect();
    if files.len() <= 500 {
        return;
    }
    files.sort();
    for (_, p) in files.iter().take(files.len() - 400) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_of_a_page() {
        let (title, text) = plain_text("<!doctype html><html><head><title>My &amp; page</title><style>p{color:red}</style></head><body><h1>Head</h1><p>one  two<br>three &lt;3</p><script>var x = '<p>no</p>';</script><!-- hidden --><div>last</div></body></html>");
        assert_eq!(title, "My & page");
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        assert_eq!(lines, ["Head", "one two", "three <3", "last"]);
    }

    #[test]
    fn extensions() {
        assert!(is_ext("HTML") && is_ext("htm") && is_ext("mhtml") && !is_ext("xml"));
    }

    #[cfg(windows)]
    #[test]
    fn file_urls_are_escaped() {
        let p = std::env::temp_dir().join("a b").join("ü.html");
        let u = file_url(&p);
        assert!(u.starts_with("file:///") && u.contains("a%20b") && u.ends_with("%C3%BC.html"), "{u}");
    }
}
