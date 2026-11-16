//! Data and configuration text (JSON, XML, YAML, TOML, INI, CSV ...) as a picture: the file re-indented where that is safe
//! (JSON), coloured by what each piece is, line numbers on the left, on a dark page. A JSON file is also checked: the subtitle
//! says whether it is valid or where it is not. Only the start of a big file is read.

use crate::Image;
use std::io::Read;
use std::path::Path;

pub const CODE_EXTS: &[&str] = &["json", "jsonc", "json5", "geojson", "ndjson", "jsonl", "xml", "xsd", "xaml", "plist", "yaml", "yml", "toml", "ini", "cfg", "conf", "tsv", "nfo", "properties", "env"];

pub fn is_code_ext(ext: &str) -> bool {
    CODE_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

const READ_LIMIT: usize = 512 * 1024;
const MAX_LINES: usize = 120;
const MAX_COLS: usize = 110;

// Colours (0xRRGGBB) on the dark page.
const C_TEXT: u32 = 0xD4D7DD;
const C_PUNCT: u32 = 0x8B93A1;
const C_KEY: u32 = 0x7CB7FF;
const C_STRING: u32 = 0x9BD18B;
const C_NUMBER: u32 = 0xF2A66B;
const C_WORD: u32 = 0xC792EA;
const C_COMMENT: u32 = 0x6B7280;
const C_TAG: u32 = 0xE06C75;

pub type Span = (String, u32);

/// Re-indent JSON. Returns None when the text has comments or anything else that a plain re-indent would damage.
pub fn json_pretty(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() * 2);
    let (mut indent, mut in_str, mut esc) = (0usize, false, false);
    let chars: Vec<char> = text.chars().collect();
    let nl = |out: &mut String, indent: usize| {
        out.push('\n');
        for _ in 0..indent {
            out.push_str("  ");
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                    out.push(chars[j]);
                    i = j + 1;
                } else {
                    indent += 1;
                    nl(&mut out, indent);
                }
            }
            '}' | ']' => {
                indent = indent.saturating_sub(1);
                nl(&mut out, indent);
                out.push(c);
            }
            ',' => {
                out.push(',');
                nl(&mut out, indent);
            }
            ':' => out.push_str(": "),
            '/' => return None, // a comment: leave the file as it is
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

fn push(spans: &mut Vec<Span>, s: &str, colour: u32) {
    if !s.is_empty() {
        spans.push((s.to_string(), colour));
    }
}

fn json_line(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            let s: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let key = j < chars.len() && chars[j] == ':';
            push(&mut spans, &s, if key { C_KEY } else { C_STRING });
        } else if c.is_ascii_digit() || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | 'e' | 'E' | '+' | '-')) {
                i += 1;
            }
            push(&mut spans, &chars[start..i].iter().collect::<String>(), C_NUMBER);
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            push(&mut spans, &chars[start..i].iter().collect::<String>(), C_WORD);
        } else if matches!(c, '{' | '}' | '[' | ']' | ',' | ':') {
            push(&mut spans, &c.to_string(), C_PUNCT);
            i += 1;
        } else {
            let start = i;
            while i < chars.len() && !matches!(chars[i], '"' | '{' | '}' | '[' | ']' | ',' | ':') && !chars[i].is_ascii_alphanumeric() && chars[i] != '-' {
                i += 1;
            }
            if i == start {
                i += 1;
            }
            push(&mut spans, &chars[start..i].iter().collect::<String>(), C_TEXT);
        }
    }
    spans
}

fn xml_line(line: &str, in_comment: &mut bool) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if *in_comment {
            match rest.find("-->") {
                Some(p) => {
                    push(&mut spans, &rest[..p + 3], C_COMMENT);
                    rest = &rest[p + 3..];
                    *in_comment = false;
                }
                None => {
                    push(&mut spans, rest, C_COMMENT);
                    break;
                }
            }
        } else if let Some(p) = rest.find('<') {
            push(&mut spans, &rest[..p], C_TEXT);
            rest = &rest[p..];
            if rest.starts_with("<!--") {
                *in_comment = true;
                continue;
            }
            let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            let tag = &rest[..end];
            // <name attr="value" ...>
            let name_end = tag.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(tag.len()).max(1);
            push(&mut spans, &tag[..name_end], C_TAG);
            let mut t = &tag[name_end..];
            while !t.is_empty() {
                if let Some(q) = t.find('"') {
                    let (before, after) = t.split_at(q);
                    push(&mut spans, before, C_KEY);
                    let close = after[1..].find('"').map(|c| c + 2).unwrap_or(after.len());
                    push(&mut spans, &after[..close], C_STRING);
                    t = &after[close..];
                } else {
                    push(&mut spans, t, if t.trim() == ">" || t.trim() == "/>" { C_TAG } else { C_KEY });
                    break;
                }
            }
            rest = &rest[end..];
        } else {
            push(&mut spans, rest, C_TEXT);
            break;
        }
    }
    spans
}

fn config_line(line: &str) -> Vec<Span> {
    let t = line.trim_start();
    let lead = &line[..line.len() - t.len()];
    let mut spans = Vec::new();
    push(&mut spans, lead, C_TEXT);
    if t.starts_with('#') || t.starts_with(';') || t.starts_with("//") {
        push(&mut spans, t, C_COMMENT);
    } else if t.starts_with('[') {
        push(&mut spans, t, C_TAG);
    } else if let Some(p) = t.find([':', '=']) {
        let (k, v) = t.split_at(p);
        push(&mut spans, k, C_KEY);
        push(&mut spans, &v[..1], C_PUNCT);
        let v = &v[1..];
        let vt = v.trim();
        let colour = if vt.parse::<f64>().is_ok() {
            C_NUMBER
        } else if matches!(vt, "true" | "false" | "null" | "yes" | "no" | "on" | "off") {
            C_WORD
        } else if vt.starts_with('"') || vt.starts_with('\'') {
            C_STRING
        } else {
            C_TEXT
        };
        push(&mut spans, v, colour);
    } else {
        push(&mut spans, t, C_TEXT);
    }
    spans
}

/// The text as coloured lines, and what is known about it (for the subtitle).
pub fn lines_of(ext: &str, text: &str, truncated: bool) -> (Vec<Vec<Span>>, String) {
    let ext = ext.to_ascii_lowercase();
    let mut note = String::new();
    let body: String;
    let mut mode = "config";
    match ext.as_str() {
        "json" | "geojson" | "json5" | "jsonc" => {
            mode = "json";
            if !truncated && ext != "jsonc" && ext != "json5" {
                note = match serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) {
                    Ok(_) => "valid JSON".into(),
                    Err(e) => format!("not valid JSON: line {}, column {}: {e}", e.line(), e.column()),
                };
            }
            body = json_pretty(text.trim_start_matches('\u{feff}')).unwrap_or_else(|| text.to_string());
        }
        "ndjson" | "jsonl" => {
            mode = "json";
            body = text.to_string();
        }
        "xml" | "xsd" | "xaml" | "plist" => {
            mode = "xml";
            body = text.to_string();
        }
        _ => body = text.to_string(),
    }
    let mut in_comment = false;
    let mut out = Vec::new();
    for raw in body.lines().take(MAX_LINES) {
        let line: String = raw.replace('\t', "    ").chars().filter(|c| !c.is_control()).collect();
        let line: String = line.chars().take(MAX_COLS).collect();
        out.push(match mode {
            "json" => json_line(&line),
            "xml" => xml_line(&line, &mut in_comment),
            _ => config_line(&line),
        });
    }
    (out, note)
}

fn read_head(path: &Path) -> Option<(String, bool)> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(READ_LIMIT as u64 + 1).read_to_end(&mut buf).ok()?;
    let truncated = buf.len() > READ_LIMIT;
    buf.truncate(READ_LIMIT);
    if buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF]) {
        let be = buf[0] == 0xFE;
        let units: Vec<u16> = buf[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        return Some((String::from_utf16_lossy(&units), truncated));
    }
    let b = buf.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&buf);
    Some((String::from_utf8_lossy(b).into_owned(), truncated))
}

fn fmt_bytes(n: u64) -> String {
    match n {
        0..=1023 => format!("{n} B"),
        1024..=1_048_575 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// The picture for a data / config file. `thumb` = the small card the gallery uses (the first lines, plain).
pub(crate) fn decode(path: &Path, thumb: bool) -> Result<Image, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    let (text, truncated) = read_head(path).ok_or("can't read the file")?;
    let (lines, note) = lines_of(&ext, &text, truncated);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut sub = fmt_bytes(size);
    if !note.is_empty() {
        sub = format!("{sub}  ·  {note}");
    }
    if truncated {
        sub = format!("{sub}  ·  first {} shown", fmt_bytes(READ_LIMIT as u64));
    }
    let mut img = render(&lines, &ext.to_uppercase(), &name, &sub, if thumb { 1 } else { 2 });
    img.colour_note = Some(format!("{} as text, coloured{}", ext.to_uppercase(), if note.is_empty() { String::new() } else { format!(" ({note})") }));
    Ok(img)
}

#[cfg(windows)]
fn render(lines: &[Vec<Span>], badge: &str, title: &str, subtitle: &str, scale: i32) -> Image {
    use std::ffi::c_void;
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, RECT, SIZE};
    use windows::Win32::Graphics::Gdi::*;

    let s = scale.max(1);
    let font_px = 13 * s;
    let lh = font_px * 14 / 10;
    let gutter_cols = 4usize;
    let shown = if scale == 1 { lines.len().min(14) } else { lines.len().max(1) };
    let cols = MAX_COLS.min(if scale == 1 { 40 } else { MAX_COLS });
    let bgr = |rgb: u32| COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF));
    unsafe {
        let dc = CreateCompatibleDC(None);
        let mono = CreateFontW(-font_px, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Consolas"));
        let ui = CreateFontW(-14 * s, 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, w!("Segoe UI"));
        let old_font = SelectObject(dc, mono.into());
        let mut sz = SIZE::default();
        let _ = GetTextExtentPoint32W(dc, &[b'M' as u16], &mut sz);
        let adv = sz.cx.max(4);
        let pad = 14 * s;
        let header = 44 * s;
        let (w, h) = (pad * 2 + adv * (gutter_cols + cols) as i32, header + pad + lh * shown as i32 + pad);
        let bmi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: 0, ..Default::default() }, ..Default::default() };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        if let (Ok(dib), false) = (CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0), bits.is_null()) {
            let old_bmp = SelectObject(dc, dib.into());
            let fill = |r: RECT, rgb: u32| {
                let b = CreateSolidBrush(bgr(rgb));
                FillRect(dc, &r, b);
                let _ = DeleteObject(b.into());
            };
            fill(RECT { left: 0, top: 0, right: w, bottom: h }, 0x1E1F22);
            fill(RECT { left: 0, top: 0, right: w, bottom: header }, 0x2B2D31);
            SetBkMode(dc, TRANSPARENT);
            let draw = |t: &str, x: i32, y: i32, rgb: u32| {
                SetTextColor(dc, bgr(rgb));
                let wide: Vec<u16> = t.encode_utf16().collect();
                let _ = TextOutW(dc, x, y, &wide);
            };
            // header: badge, name, subtitle
            SelectObject(dc, ui.into());
            fill(RECT { left: pad, top: 10 * s, right: pad + 8 * s + adv * badge.chars().count() as i32 + 8 * s, bottom: header - 10 * s }, 0x3D8BFF);
            draw(badge, pad + 8 * s, 12 * s, 0xFFFFFF);
            draw(&title.chars().take(if scale == 1 { 22 } else { 70 }).collect::<String>(), pad + 24 * s + adv * badge.chars().count() as i32 + 8 * s, 12 * s, 0xF2F2F2);
            if scale > 1 {
                let sw = {
                    let mut z = SIZE::default();
                    let wide: Vec<u16> = subtitle.encode_utf16().collect();
                    let _ = GetTextExtentPoint32W(dc, &wide, &mut z);
                    z.cx
                };
                draw(subtitle, (w - pad - sw).max(pad), 12 * s, 0xA5ABB3);
            }
            SelectObject(dc, mono.into());
            for (n, spans) in lines.iter().take(shown).enumerate() {
                let y = header + pad + lh * n as i32;
                draw(&format!("{:>3}", n + 1), pad, y, 0x4E5560);
                let mut x = pad + adv * gutter_cols as i32;
                for (t, c) in spans {
                    draw(t, x, y, *c);
                    x += adv * t.chars().count() as i32;
                    if x > w - pad {
                        break;
                    }
                }
            }
            let _ = GdiFlush();
            let src = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
            for (d, p) in rgba.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d.copy_from_slice(&[p[2], p[1], p[0], 255]);
            }
            SelectObject(dc, old_bmp);
            let _ = DeleteObject(dib.into());
        }
        SelectObject(dc, old_font);
        let _ = DeleteObject(mono.into());
        let _ = DeleteObject(ui.into());
        let _ = DeleteDC(dc);
        Image::new(w as u32, h as u32, rgba)
    }
}

#[cfg(not(windows))]
fn render(_lines: &[Vec<Span>], _badge: &str, _title: &str, _subtitle: &str, scale: i32) -> Image {
    let s = scale.max(1) as u32;
    Image::new(256 * s, 192 * s, vec![0x1E; (256 * s * 192 * s * 4) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_reindented_and_comments_are_left_alone() {
        let p = json_pretty("{\"a\":1,\"b\":[1,2,{\"c\":\"x, y\"}],\"d\":{},\"e\":[]}").unwrap();
        assert_eq!(p, "{\n  \"a\": 1,\n  \"b\": [\n    1,\n    2,\n    {\n      \"c\": \"x, y\"\n    }\n  ],\n  \"d\": {},\n  \"e\": []\n}");
        assert!(json_pretty("{\"a\": 1 // note\n}").is_none());
        assert_eq!(json_pretty("\"a\\\"b\"").unwrap(), "\"a\\\"b\"");
    }

    #[test]
    fn json_lines_get_key_string_number_and_word_colours() {
        let spans = json_line("  \"name\": \"Nu\", \"n\": -12.5e3, \"ok\": true");
        let has = |t: &str, c: u32| spans.iter().any(|(s, col)| s == t && *col == c);
        assert!(has("\"name\"", C_KEY) && has("\"Nu\"", C_STRING) && has("-12.5e3", C_NUMBER) && has("true", C_WORD));
        let joined: String = spans.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "  \"name\": \"Nu\", \"n\": -12.5e3, \"ok\": true", "colouring never loses or adds text");
    }

    #[test]
    fn validity_is_reported_with_the_place_of_the_error() {
        let (_, ok) = lines_of("json", "{\"a\": [1, 2]}", false);
        assert_eq!(ok, "valid JSON");
        let (_, bad) = lines_of("json", "{\"a\": [1, 2}", false);
        assert!(bad.starts_with("not valid JSON: line 1"), "{bad}");
    }

    #[test]
    fn xml_and_config_lines_keep_their_text() {
        let mut c = false;
        let xml = xml_line("<a href=\"x\">hi</a> <!-- note -->", &mut c);
        assert_eq!(xml.iter().map(|(s, _)| s.as_str()).collect::<String>(), "<a href=\"x\">hi</a> <!-- note -->");
        let cfg = config_line("  port = 8080");
        assert!(cfg.iter().any(|(s, c)| s.trim() == "8080" && *c == C_NUMBER));
    }

    #[test]
    fn a_json_file_becomes_a_picture() {
        let p = std::env::temp_dir().join(format!("nv_codepage_{}.json", std::process::id()));
        std::fs::write(&p, "{\"hello\": [1, 2, 3], \"th\": \"สวัสดี\"}").unwrap();
        let img = decode(&p, false).unwrap();
        assert!(img.width > 400 && img.height > 100);
        let thumb = decode(&p, true).unwrap();
        assert!(thumb.width < img.width);
        let _ = std::fs::remove_file(&p);
    }
}
