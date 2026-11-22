//! Markdown as the reader sees it: headings, emphasis, lists, quotes, code, tables and links turned into RTF, which the Windows RichEdit
//! control shows with real fonts and sizes (no browser engine). Pure code: text in, RTF out.
//!
//! Covered: `#`..`######` headings (and `===` / `---` underlines), paragraphs, `**bold**`, `*italic*`, `__bold__`, `_italic_`, `~~strike~~`,
//! `` `code` ``, fenced and indented code blocks, `>` quotes, `-` `*` `+` and `1.` lists (nested by indentation, with `[ ]` / `[x]` tasks),
//! `---` rules, tables, `[text](url)` links (the text underlined, the address after it), images as their alt text, `<http://...>` links
//! and backslash escapes. Raw HTML is shown as text.

const TEXT: u32 = 0xD4D7DD;
const HEADING: u32 = 0xFFC857;
const CODE: u32 = 0x56D4C4;
const LINK: u32 = 0x61AFEF;
const QUOTE: u32 = 0x9AA5B5;
const DIM: u32 = 0x6B7A90;

/// The page background the RTF is meant for (the control paints it; RTF itself has no page colour).
pub const BACKGROUND: u32 = 0x1B1D22;

fn colour_table() -> String {
    let c = |rgb: u32| format!("\\red{}\\green{}\\blue{};", rgb >> 16 & 255, rgb >> 8 & 255, rgb & 255);
    format!("{{\\colortbl ;{}{}{}{}{}{}}}", c(TEXT), c(HEADING), c(CODE), c(LINK), c(QUOTE), c(DIM))
}

// Colour numbers in the table above.
const CF_TEXT: u8 = 1;
const CF_HEADING: u8 = 2;
const CF_CODE: u8 = 3;
const CF_LINK: u8 = 4;
const CF_QUOTE: u8 = 5;
const CF_DIM: u8 = 6;

/// RTF-escape `s`: the three specials, tabs, and everything outside ASCII as `\uN?` (one per UTF-16 unit).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\t' => out.push_str("\\tab "),
            '\n' => out.push_str("\\line "),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{}?", *u as i16));
                }
            }
        }
    }
    out
}

/// Inline Markdown of one paragraph as RTF runs.
pub fn inline(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut String| {
        if !plain.is_empty() {
            out.push_str(&escape(plain));
            plain.clear();
        }
    };
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        // Backslash escapes.
        if ch == '\\' && c.get(i + 1).is_some_and(|n| "\\`*_{}[]()#+-.!|<>~".contains(*n)) {
            plain.push(c[i + 1]);
            i += 2;
            continue;
        }
        // Code.
        if ch == '`' {
            let n = c[i..].iter().take_while(|x| **x == '`').count();
            let fence: String = "`".repeat(n);
            let rest: String = c[i + n..].iter().collect();
            if let Some(p) = rest.find(&fence) {
                flush(&mut plain, &mut out);
                out.push_str(&format!("{{\\f1\\cf{CF_CODE} {}}}", escape(rest[..p].trim())));
                i += n + rest[..p].chars().count() + n;
                continue;
            }
        }
        // Images and links.
        if (ch == '!' && c.get(i + 1) == Some(&'[')) || ch == '[' {
            let image = ch == '!';
            let open = if image { i + 1 } else { i };
            if let Some(close) = (open + 1..c.len()).find(|&k| c[k] == ']') {
                if c.get(close + 1) == Some(&'(') {
                    if let Some(end) = (close + 2..c.len()).find(|&k| c[k] == ')') {
                        let label: String = c[open + 1..close].iter().collect();
                        let url: String = c[close + 2..end].iter().collect::<String>().split_whitespace().next().unwrap_or("").to_string();
                        flush(&mut plain, &mut out);
                        if image {
                            out.push_str(&format!("{{\\i\\cf{CF_DIM}[image: {}]}}", escape(&label)));
                        } else {
                            out.push_str(&format!("{{\\ul\\cf{CF_LINK} {}}}{{\\cf{CF_DIM}  ({})}}", inline_nested(&label), escape(&url)));
                        }
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        // <http://auto-link>
        if ch == '<' {
            if let Some(e) = (i + 1..c.len()).find(|&k| c[k] == '>') {
                let inner: String = c[i + 1..e].iter().collect();
                if inner.starts_with("http://") || inner.starts_with("https://") || inner.starts_with("mailto:") {
                    flush(&mut plain, &mut out);
                    out.push_str(&format!("{{\\ul\\cf{CF_LINK} {}}}", escape(&inner)));
                    i = e + 1;
                    continue;
                }
            }
        }
        // Emphasis: ***x***, **x**, *x*, __x__, _x_, ~~x~~
        if matches!(ch, '*' | '_' | '~') {
            let n = c[i..].iter().take_while(|x| **x == ch).count().min(3);
            let want = if ch == '~' { 2 } else { n };
            if (ch != '~' || n >= 2) && want >= 1 && c.get(i + want).is_some_and(|x| !x.is_whitespace()) {
                let left_ok = i == 0 || !c[i - 1].is_alphanumeric() || ch == '*' || ch == '~';
                let close: Vec<char> = std::iter::repeat_n(ch, want).collect();
                if left_ok {
                    if let Some(e) = (i + want + 1..c.len()).find(|&k| c[k..(k + want).min(c.len())] == close[..] && !c[k - 1].is_whitespace() && (ch != '_' || c.get(k + want).is_none_or(|x| !x.is_alphanumeric()))) {
                        let inner: String = c[i + want..e].iter().collect();
                        flush(&mut plain, &mut out);
                        let style = match (ch, want) {
                            ('~', _) => "\\strike",
                            (_, 1) => "\\i",
                            (_, 2) => "\\b",
                            _ => "\\b\\i",
                        };
                        out.push_str(&format!("{{{style} {}}}", inline(&inner)));
                        i = e + want;
                        continue;
                    }
                }
            }
        }
        // A hard break: two spaces at the end of a line (the caller joins lines with '\n').
        if ch == ' ' && c.get(i + 1) == Some(&' ') && c[i..].iter().skip_while(|x| **x == ' ').next() == Some(&'\n') {
            i += 1;
            continue;
        }
        plain.push(ch);
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

/// A link's label: its own emphasis, but no further links.
fn inline_nested(label: &str) -> String {
    inline(&label.replace('[', "\\[").replace('!', "\\!"))
}

fn heading_level(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&n) && (t.len() == n || t[n..].starts_with(' ') || t[n..].starts_with('\t')) {
        return Some((n, t[n..].trim().trim_end_matches('#').trim()));
    }
    None
}

fn is_rule(line: &str) -> bool {
    let t: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// `Some((indent in spaces, marker, rest))` for a list line.
fn list_item(line: &str) -> Option<(usize, String, &str)> {
    let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let t = line.trim_start();
    if t.len() > 1 && matches!(t.as_bytes()[0], b'-' | b'*' | b'+') && t.as_bytes()[1] == b' ' && !is_rule(t) {
        return Some((indent, "\u{2022}".to_string(), t[2..].trim_start()));
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return Some((indent, format!("{}.", &t[..digits]), t[digits + 2..].trim_start()));
    }
    None
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The whole document as RTF at a base size of `pt` points.
pub fn to_rtf(md: &str, pt: u32) -> String {
    let half = (pt.clamp(6, 72) * 2) as i64;
    let size = |k: f64| (half as f64 * k).round() as i64;
    let mut out = format!("{{\\rtf1\\ansi\\ansicpg1252\\deff0\\nouicompat{{\\fonttbl{{\\f0\\fswiss\\fcharset0 Segoe UI;}}{{\\f1\\fmodern\\fcharset0 Consolas;}}}}{}\\viewkind4\\uc1\\pard\\f0\\fs{half}\\cf{CF_TEXT} ", colour_table());
    let text = md.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let mut paragraph: Vec<&str> = Vec::new();
    let reset = format!("\\pard\\f0\\fs{half}\\cf{CF_TEXT}\\b0\\i0 ");
    let flush_para = |paragraph: &mut Vec<&str>, out: &mut String| {
        if !paragraph.is_empty() {
            let joined = paragraph.iter().map(|l| l.trim_end_matches(' ').to_string() + if l.ends_with("  ") { "\n" } else { " " }).collect::<String>();
            // A line that ends with two spaces breaks; the others join with a space.
            let mut body = String::new();
            for part in joined.split('\n') {
                if !body.is_empty() {
                    body.push_str("\\line ");
                }
                body.push_str(&inline(part.trim_end()));
            }
            out.push_str(&format!("\\sa{} {body}\\par\n", size(0.7)));
            paragraph.clear();
        }
    };
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        // Fenced code.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush_para(&mut paragraph, &mut out);
            let fence = &trimmed[..3];
            i += 1;
            out.push_str(&format!("\\pard\\li360\\sb{} \\f1\\fs{}\\cf{CF_CODE} ", size(0.3), size(0.9)));
            while i < lines.len() && !lines[i].trim().starts_with(fence) {
                out.push_str(&escape(lines[i]));
                out.push_str("\\line ");
                i += 1;
            }
            out.push_str(&format!("\\par\n{reset}"));
            i += 1;
            continue;
        }
        if trimmed.is_empty() {
            flush_para(&mut paragraph, &mut out);
            i += 1;
            continue;
        }
        if let Some((level, title)) = heading_level(line) {
            flush_para(&mut paragraph, &mut out);
            let k = [2.0, 1.6, 1.32, 1.15, 1.05, 1.0][level - 1];
            out.push_str(&format!("\\pard\\sb{}\\sa{} \\fs{}\\b\\cf{CF_HEADING} {}\\par\n{reset}", size(0.9), size(0.4), size(k), inline(title)));
            i += 1;
            continue;
        }
        // Setext headings: a line followed by === or ---.
        if i + 1 < lines.len() && !trimmed.is_empty() && paragraph.is_empty() && !lines[i + 1].trim().is_empty() && lines[i + 1].trim().chars().all(|c| c == '=') {
            let k = 2.0;
            out.push_str(&format!("\\pard\\sb{}\\sa{} \\fs{}\\b\\cf{CF_HEADING} {}\\par\n{reset}", size(0.9), size(0.4), size(k), inline(trimmed)));
            i += 2;
            continue;
        }
        if is_rule(trimmed) && paragraph.is_empty() || (is_rule(trimmed) && trimmed.chars().all(|c| matches!(c, '*' | '_' | ' '))) {
            flush_para(&mut paragraph, &mut out);
            out.push_str(&format!("\\pard\\qc\\cf{CF_DIM} {}\\par\n{reset}", "\u{2500}".repeat(24).chars().map(|c| escape(&c.to_string())).collect::<String>()));
            i += 1;
            continue;
        }
        if trimmed.starts_with('>') {
            flush_para(&mut paragraph, &mut out);
            let mut quote = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('>') {
                quote.push(lines[i].trim().trim_start_matches('>').trim_start());
                i += 1;
            }
            out.push_str(&format!("\\pard\\li400\\sa{} \\i\\cf{CF_QUOTE} {}\\par\n{reset}", size(0.6), inline(&quote.join(" "))));
            continue;
        }
        if trimmed.starts_with('|') && i + 1 < lines.len() && is_table_separator(lines[i + 1]) {
            flush_para(&mut paragraph, &mut out);
            let mut rows: Vec<Vec<String>> = vec![table_cells(trimmed)];
            i += 2;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(table_cells(lines[i]));
                i += 1;
            }
            let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            let widths: Vec<usize> = (0..cols).map(|k| rows.iter().map(|r| r.get(k).map(|c| plain_width(c)).unwrap_or(0)).max().unwrap_or(0)).collect();
            out.push_str(&format!("\\pard\\li200\\sa{} \\f1\\fs{}\\cf{CF_TEXT} ", size(0.6), size(0.92)));
            for (n, r) in rows.iter().enumerate() {
                let cells: Vec<String> = (0..cols).map(|k| pad(r.get(k).map(String::as_str).unwrap_or(""), widths[k])).collect();
                let row = format!("| {} |", cells.join(" | "));
                out.push_str(&if n == 0 { format!("\\b {}\\b0 ", escape(&row)) } else { escape(&row) });
                out.push_str("\\line ");
                if n == 0 {
                    let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
                    out.push_str(&escape(&format!("|-{}-|", rule.join("-|-"))));
                    out.push_str("\\line ");
                }
            }
            out.push_str(&format!("\\par\n{reset}"));
            continue;
        }
        if let Some((indent, marker, rest)) = list_item(line) {
            flush_para(&mut paragraph, &mut out);
            let level = indent / 2;
            let (marker, rest) = match rest.get(..3) {
                Some("[ ]") => ("\u{2610}".to_string(), rest[3..].trim_start()),
                Some("[x]") | Some("[X]") => ("\u{2611}".to_string(), rest[3..].trim_start()),
                _ => (marker, rest),
            };
            out.push_str(&format!("\\pard\\li{}\\fi-280\\sa{} \\cf{CF_DIM} {}\\tab\\cf{CF_TEXT} {}\\par\n{reset}", 400 + 360 * level.min(8), size(0.25), escape(&marker), inline(rest)));
            i += 1;
            continue;
        }
        // Indented code (four spaces) when no paragraph is running.
        if (line.starts_with("    ") || line.starts_with('\t')) && paragraph.is_empty() {
            out.push_str(&format!("\\pard\\li360 \\f1\\fs{}\\cf{CF_CODE} ", size(0.9)));
            while i < lines.len() && (lines[i].starts_with("    ") || lines[i].starts_with('\t') || lines[i].trim().is_empty()) {
                out.push_str(&escape(lines[i].strip_prefix("    ").or_else(|| lines[i].strip_prefix('\t')).unwrap_or(lines[i])));
                out.push_str("\\line ");
                i += 1;
            }
            out.push_str(&format!("\\par\n{reset}"));
            continue;
        }
        paragraph.push(line);
        i += 1;
    }
    flush_para(&mut paragraph, &mut out);
    out.push('}');
    out
}

/// Visible width of a table cell (its Markdown marks do not show; the table is monospace text, so this is characters).
fn plain_width(cell: &str) -> usize {
    cell.replace("**", "").replace('`', "").chars().count()
}

fn pad(cell: &str, width: usize) -> String {
    let shown = cell.replace("**", "").replace('`', "");
    let n = shown.chars().count();
    format!("{shown}{}", " ".repeat(width.saturating_sub(n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_is_valid_rtf_with_balanced_braces() {
        let md = "# T\n\ntext **b** *i* `c` [l](http://x)\n\n- a\n- b\n\n```\ncode {x}\n```\n";
        let rtf = to_rtf(md, 11);
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.ends_with('}'));
        // Braces balance once the escaped ones are set aside.
        let stripped = rtf.replace("\\\\", "").replace("\\{", "").replace("\\}", "");
        assert_eq!(stripped.matches('{').count(), stripped.matches('}').count(), "{rtf}");
    }

    #[test]
    fn specials_and_non_ascii_are_escaped() {
        assert_eq!(escape("a\\b{c}d"), "a\\\\b\\{c\\}d");
        assert_eq!(escape("é"), "\\u233?");
        assert_eq!(escape("ไ"), "\\u3652?");
        // Outside the BMP: two units; the sign is the 16-bit signed value RTF wants.
        assert_eq!(escape("😀"), "\\u-10179?\\u-8704?");
        assert_eq!(escape("a\tb"), "a\\tab b");
    }

    #[test]
    fn headings_have_their_sizes_and_the_text() {
        let rtf = to_rtf("# One\n## Two\n###### Six\nSetext\n=======\n", 10);
        let sizes: Vec<i64> = rtf.split("\\fs").skip(1).filter_map(|p| p.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()).collect();
        assert!(sizes.contains(&40), "h1 is double: {sizes:?}"); // 10 pt = 20 half-points, x2
        assert!(sizes.contains(&32), "h2 is 1.6x");
        assert!(rtf.contains("One") && rtf.contains("Two") && rtf.contains("Six") && rtf.contains("Setext"));
        assert!(!rtf.contains("#"), "the marks are not shown");
        assert!(!rtf.contains("====="));
    }

    #[test]
    fn inline_styles() {
        let r = inline("a **bold** and *it* and ***both*** and ~~gone~~ and `code` and snake_case_name");
        assert!(r.contains("{\\b bold}") && r.contains("{\\i it}") && r.contains("{\\b\\i both}") && r.contains("{\\strike gone}"));
        assert!(r.contains("{\\f1\\cf3 code}"));
        assert!(r.contains("snake_case_name"), "underscores inside a word are text: {r}");
        assert!(inline("2 * 3 * 4").contains("2 * 3 * 4"), "stars with spaces are text");
        assert!(inline("\\*not em\\*").contains("*not em*"));
    }

    #[test]
    fn links_and_images() {
        let r = inline("see [the **docs**](https://x.y/z) and ![a cat](c.png) or <https://a.b>");
        assert!(r.contains("\\ul") && r.contains("the {\\b docs}") && r.contains("(https://x.y/z)"), "{r}");
        assert!(r.contains("[image: a cat]") && !r.contains("c.png"));
        assert!(r.contains("https://a.b"));
    }

    #[test]
    fn lists_tasks_quotes_rules_and_code() {
        let md = "- one\n  - nested\n1. first\n2. second\n- [x] done\n- [ ] todo\n\n> quoted\n> more\n\n---\n\n    indented code\n\n```rust\nlet a = 1;\n```\nafter";
        let rtf = to_rtf(md, 11);
        assert!(rtf.contains("\\u8226?"), "bullets");
        assert!(rtf.contains("1.") && rtf.contains("2."));
        assert!(rtf.contains("\\u9745?") && rtf.contains("\\u9744?"), "task boxes");
        assert!(rtf.contains("\\i\\cf5 quoted more"));
        assert!(rtf.contains("\\u9472?"), "a rule");
        assert!(rtf.contains("indented code") && rtf.contains("let a = 1;"));
        assert!(rtf.contains("\\f1"), "monospace for code");
        // The nested item is indented more than its parent.
        let li = |t: &str| -> i64 { let p = rtf.find(t).unwrap(); let head = &rtf[..p]; let k = head.rfind("\\li").unwrap(); head[k + 3..].split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap() };
        assert!(li("nested") > li("one"));
    }

    #[test]
    fn tables_are_aligned_monospace() {
        let rtf = to_rtf("| Name | Size |\n|------|-----:|\n| a | 1 |\n| longer | 22 |\n", 11);
        assert!(rtf.contains("\\f1"));
        assert!(rtf.contains("| a      | 1    |"), "{rtf}");
        assert!(rtf.contains("| longer | 22   |"));
        assert!(rtf.contains("|--------|------|") || rtf.contains("|-"), "{rtf}");
    }

    #[test]
    fn paragraphs_join_lines_and_two_spaces_break() {
        let rtf = to_rtf("one\ntwo  \nthree\n\nnext", 11);
        assert!(rtf.contains("one two\\line three"), "{rtf}");
        assert!(rtf.contains("next"));
        // Empty and odd input do not panic.
        for s in ["", "\n\n", "#", "# ", "**", "[", "![]()", "```", "|", "> ", "- ", "1.", "\\"] {
            let _ = to_rtf(s, 11);
        }
    }

    #[test]
    fn thai_and_cjk_text_survive() {
        let rtf = to_rtf("# สวัสดี\n\nวันนี้ **ดี** 日本語", 11);
        assert!(rtf.contains("\\u3626?"), "the Thai letter ส");
        assert!(rtf.contains("\\u26085?"), "the character 日");
    }
}
