//! Syntax colouring: which stretches of a text are comments, strings, numbers, keywords, keys, tags ... for the language of the file.
//! Pure code. Positions are in UTF-16 code units of the text with every line end a single `\n` (what the edit control counts).

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Plain,
    Json,
    JavaScript,
    Html,
    Xml,
    Css,
    Markdown,
    /// INI / config / .env / properties.
    Ini,
    Toml,
    Yaml,
    Rust,
    Python,
    /// C, C++, C#, Java, Go, Swift, Kotlin, PHP: braces and `//` comments.
    CLike,
    Shell,
    Sql,
    Batch,
    PowerShell,
}

impl Lang {
    pub const ALL: [Lang; 17] = [Lang::Plain, Lang::Json, Lang::JavaScript, Lang::Html, Lang::Xml, Lang::Css, Lang::Markdown, Lang::Ini, Lang::Toml, Lang::Yaml, Lang::Rust, Lang::Python, Lang::CLike, Lang::Shell, Lang::Sql, Lang::Batch, Lang::PowerShell];

    pub fn label(self) -> &'static str {
        match self {
            Lang::Plain => "Plain text",
            Lang::Json => "JSON",
            Lang::JavaScript => "JavaScript / TypeScript",
            Lang::Html => "HTML",
            Lang::Xml => "XML / SVG",
            Lang::Css => "CSS",
            Lang::Markdown => "Markdown",
            Lang::Ini => "INI / config",
            Lang::Toml => "TOML",
            Lang::Yaml => "YAML",
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::CLike => "C / C++ / C# / Java / Go",
            Lang::Shell => "Shell",
            Lang::Sql => "SQL",
            Lang::Batch => "Batch",
            Lang::PowerShell => "PowerShell",
        }
    }
}

/// The language of a file by its extension (lower case, no dot).
pub fn lang_of_ext(ext: &str) -> Lang {
    match ext.to_ascii_lowercase().as_str() {
        "json" | "jsonc" | "json5" | "geojson" | "webmanifest" | "ndjson" | "jsonl" => Lang::Json,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "mts" | "cts" => Lang::JavaScript,
        "html" | "htm" | "xhtml" | "vue" | "svelte" => Lang::Html,
        "xml" | "svg" | "xsd" | "xaml" | "plist" | "csproj" | "config" | "rss" | "atom" | "xsl" | "xslt" | "resx" => Lang::Xml,
        "css" | "scss" | "less" => Lang::Css,
        "md" | "markdown" | "mdown" | "mkd" => Lang::Markdown,
        "ini" | "cfg" | "conf" | "properties" | "env" | "desktop" | "editorconfig" | "gitconfig" => Lang::Ini,
        "toml" => Lang::Toml,
        "yaml" | "yml" => Lang::Yaml,
        "rs" => Lang::Rust,
        "py" | "pyw" | "pyi" => Lang::Python,
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "cs" | "java" | "go" | "swift" | "kt" | "kts" | "php" | "dart" | "scala" | "m" => Lang::CLike,
        "sh" | "bash" | "zsh" | "fish" => Lang::Shell,
        "sql" => Lang::Sql,
        "bat" | "cmd" => Lang::Batch,
        "ps1" | "psm1" | "psd1" => Lang::PowerShell,
        _ => Lang::Plain,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Comment,
    Str,
    Number,
    Keyword,
    /// A key of an object / a setting.
    Key,
    Tag,
    Attr,
    Punct,
    Heading,
    Emphasis,
    Code,
    Link,
}

/// A coloured stretch: `start` and `len` in UTF-16 units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: usize,
    pub len: usize,
    pub kind: Kind,
}

/// Colours (0xRRGGBB) on the dark page.
pub fn colour(kind: Kind) -> u32 {
    match kind {
        Kind::Comment => 0x6B7A90,
        Kind::Str => 0x9BD18B,
        Kind::Number => 0xF2A66B,
        Kind::Keyword => 0xC792EA,
        Kind::Key => 0x7CB7FF,
        Kind::Tag => 0xE06C75,
        Kind::Attr => 0xE5C07B,
        Kind::Punct => 0x8B93A1,
        Kind::Heading => 0xFFC857,
        Kind::Emphasis => 0xF4A6C0,
        Kind::Code => 0x56D4C4,
        Kind::Link => 0x61AFEF,
    }
}

/// Texts longer than this are not coloured (a multi-megabyte log would take seconds and nobody reads it by its colours).
pub const MAX_COLOURED_CHARS: usize = 400_000;

/// The coloured stretches of `text`, in order, none overlapping.
pub fn spans(text: &str, lang: Lang) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > MAX_COLOURED_CHARS || lang == Lang::Plain {
        return Vec::new();
    }
    // UTF-16 offset of every char index (plus the end).
    let mut u16at = Vec::with_capacity(chars.len() + 1);
    let mut n = 0;
    for c in &chars {
        u16at.push(n);
        n += c.len_utf16();
    }
    u16at.push(n);
    let mut out: Vec<(usize, usize, Kind)> = Vec::new();
    run(&chars, 0, chars.len(), lang, &mut out);
    out.sort_by_key(|s| s.0);
    out.into_iter().filter(|s| s.1 > s.0).map(|(a, b, kind)| Span { start: u16at[a], len: u16at[b] - u16at[a], kind }).collect()
}

fn run(c: &[char], from: usize, to: usize, lang: Lang, out: &mut Vec<(usize, usize, Kind)>) {
    match lang {
        Lang::Plain => {}
        Lang::Json => json(c, from, to, out),
        Lang::Html | Lang::Xml => markup(c, from, to, lang == Lang::Html, out),
        Lang::Markdown => markdown(c, from, to, out),
        Lang::Ini | Lang::Toml | Lang::Yaml => config(c, from, to, lang, out),
        other => code(c, from, to, &syntax_of(other), out),
    }
}

// ---- C-like languages ------------------------------------------------------------------------------------------------------

struct Syntax {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    keywords: &'static [&'static str],
    /// A backslash escapes the next character inside strings (not in batch files and SQL).
    escapes: bool,
    case_insensitive: bool,
}

const C_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "goto", "class", "struct", "enum", "union", "interface", "namespace", "using", "import", "package", "public", "private", "protected", "static", "final", "const", "void", "int", "long", "short", "float",
    "double", "char", "bool", "boolean", "byte", "var", "val", "let", "fun", "func", "new", "delete", "this", "super", "try", "catch", "finally", "throw", "throws", "true", "false", "null", "nil", "typedef", "extends", "implements", "override", "virtual", "abstract", "async", "await", "string", "object", "readonly",
    "go", "defer", "chan", "map", "range", "select", "type", "fn", "mut", "in", "is", "as", "where", "yield", "foreach", "echo", "function", "namespace",
];
const JS_KEYWORDS: &[&str] = &[
    "function", "const", "let", "var", "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return", "class", "extends", "new", "this", "super", "import", "export", "from", "as", "try", "catch", "finally", "throw", "typeof", "instanceof", "in", "of", "async", "await", "yield",
    "true", "false", "null", "undefined", "void", "delete", "static", "get", "set", "interface", "type", "enum", "implements", "public", "private", "protected", "readonly", "abstract", "namespace", "declare", "module",
];
const RUST_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "const", "static", "if", "else", "match", "for", "while", "loop", "break", "continue", "return", "struct", "enum", "impl", "trait", "pub", "use", "mod", "crate", "self", "Self", "super", "as", "in", "where", "type", "unsafe", "async", "await", "move", "ref", "dyn", "true", "false", "extern", "box",
];
const PY_KEYWORDS: &[&str] = &[
    "def", "class", "if", "elif", "else", "for", "while", "break", "continue", "return", "import", "from", "as", "try", "except", "finally", "raise", "with", "pass", "lambda", "yield", "global", "nonlocal", "in", "is", "not", "and", "or", "None", "True", "False", "async", "await", "del", "assert", "self",
];
const SH_KEYWORDS: &[&str] = &["if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac", "function", "in", "return", "exit", "export", "local", "echo", "source", "set", "unset", "cd", "test"];
const SQL_KEYWORDS: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create", "table", "alter", "drop", "index", "view", "join", "inner", "outer", "left", "right", "on", "group", "by", "order", "having", "limit", "offset", "union", "as", "and", "or", "not", "null", "is", "in", "like", "between",
    "distinct", "primary", "key", "foreign", "references", "default", "constraint", "begin", "commit", "rollback", "case", "when", "then", "else", "end", "exists", "count", "sum", "avg", "min", "max",
];
const BAT_KEYWORDS: &[&str] = &["echo", "set", "if", "else", "for", "in", "do", "goto", "call", "exit", "rem", "pause", "cd", "copy", "del", "move", "mkdir", "rmdir", "start", "setlocal", "endlocal", "not", "exist", "errorlevel", "defined", "equ", "neq"];
const PS_KEYWORDS: &[&str] = &["function", "param", "if", "elseif", "else", "foreach", "for", "while", "do", "switch", "break", "continue", "return", "try", "catch", "finally", "throw", "in", "begin", "process", "end", "filter", "class", "enum", "using", "$true", "$false", "$null"];
const CSS_KEYWORDS: &[&str] = &["important", "inherit", "initial", "none", "auto", "solid", "block", "inline", "flex", "grid", "absolute", "relative", "fixed", "sticky", "hidden", "visible", "var", "calc", "url", "rgb", "rgba", "hsl", "hsla"];

fn syntax_of(lang: Lang) -> Syntax {
    match lang {
        Lang::JavaScript => Syntax { line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\'', '`'], keywords: JS_KEYWORDS, escapes: true, case_insensitive: false },
        Lang::Css => Syntax { line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\''], keywords: CSS_KEYWORDS, escapes: true, case_insensitive: true },
        Lang::Rust => Syntax { line: &["//"], block: Some(("/*", "*/")), quotes: &['"'], keywords: RUST_KEYWORDS, escapes: true, case_insensitive: false },
        Lang::Python => Syntax { line: &["#"], block: None, quotes: &['"', '\''], keywords: PY_KEYWORDS, escapes: true, case_insensitive: false },
        Lang::Shell => Syntax { line: &["#"], block: None, quotes: &['"', '\''], keywords: SH_KEYWORDS, escapes: true, case_insensitive: false },
        Lang::Sql => Syntax { line: &["--"], block: Some(("/*", "*/")), quotes: &['\''], keywords: SQL_KEYWORDS, escapes: false, case_insensitive: true },
        Lang::Batch => Syntax { line: &["::", "rem ", "REM "], block: None, quotes: &['"'], keywords: BAT_KEYWORDS, escapes: false, case_insensitive: true },
        Lang::PowerShell => Syntax { line: &["#"], block: Some(("<#", "#>")), quotes: &['"', '\''], keywords: PS_KEYWORDS, escapes: false, case_insensitive: true },
        _ => Syntax { line: &["//"], block: Some(("/*", "*/")), quotes: &['"', '\'', '`'], keywords: C_KEYWORDS, escapes: true, case_insensitive: false },
    }
}

fn starts(c: &[char], i: usize, s: &str) -> bool {
    s.chars().enumerate().all(|(k, ch)| c.get(i + k) == Some(&ch))
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

fn code(c: &[char], from: usize, to: usize, sx: &Syntax, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        let ch = c[i];
        // Comments.
        if let Some(p) = sx.line.iter().find(|p| starts(c, i, p)) {
            // A comment marker that is a word (`rem ` in a batch file) counts only at the start of a line (after blanks).
            let is_word_marker = p.chars().next().is_some_and(char::is_alphabetic);
            let at_line_start = c[from..i].iter().rev().take_while(|x| **x != '\n').all(|x| *x == ' ' || *x == '\t');
            if !is_word_marker || at_line_start {
                let end = (i..to).find(|&k| c[k] == '\n').unwrap_or(to);
                out.push((i, end, Kind::Comment));
                i = end;
                continue;
            }
        }
        if let Some((open, close)) = sx.block {
            if starts(c, i, open) {
                let end = (i + open.chars().count()..to).find(|&k| starts(c, k, close)).map(|k| k + close.chars().count()).unwrap_or(to);
                out.push((i, end, Kind::Comment));
                i = end;
                continue;
            }
        }
        // Strings (Python's triple quotes too).
        if sx.quotes.contains(&ch) {
            let triple = (ch == '"' || ch == '\'') && sx.keywords.contains(&"def") && c.get(i + 1) == Some(&ch) && c.get(i + 2) == Some(&ch);
            let mut k = i + if triple { 3 } else { 1 };
            while k < to {
                if sx.escapes && c[k] == '\\' {
                    k += 2;
                    continue;
                }
                if triple {
                    if c[k] == ch && c.get(k + 1) == Some(&ch) && c.get(k + 2) == Some(&ch) {
                        k += 3;
                        break;
                    }
                } else if c[k] == ch {
                    k += 1;
                    break;
                } else if c[k] == '\n' && ch != '`' {
                    break; // an unterminated string ends with the line
                }
                k += 1;
            }
            let k = k.min(to);
            out.push((i, k, Kind::Str));
            i = k;
            continue;
        }
        // Numbers.
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit()) && !c.get(i.wrapping_sub(1)).copied().is_some_and(is_word)) {
            if i == from || !is_word(c[i - 1]) {
                let mut k = i + 1;
                while k < to && (c[k].is_ascii_alphanumeric() || c[k] == '.' || c[k] == '_') {
                    k += 1;
                }
                out.push((i, k, Kind::Number));
                i = k;
                continue;
            }
        }
        // Words.
        if is_word(ch) {
            let mut k = i + 1;
            while k < to && is_word(c[k]) {
                k += 1;
            }
            let word: String = c[i..k].iter().collect();
            let hit = if sx.case_insensitive { sx.keywords.iter().any(|w| w.eq_ignore_ascii_case(&word)) } else { sx.keywords.contains(&word.as_str()) };
            if hit {
                out.push((i, k, Kind::Keyword));
            }
            i = k;
            continue;
        }
        i += 1;
    }
}

// ---- JSON --------------------------------------------------------------------------------------------------------------------

fn json(c: &[char], from: usize, to: usize, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        let ch = c[i];
        if ch == '"' {
            let mut k = i + 1;
            while k < to && c[k] != '"' && c[k] != '\n' {
                k += if c[k] == '\\' { 2 } else { 1 };
            }
            let end = (k + 1).min(to);
            // A key: a string followed (after blanks) by a colon.
            let after = (end..to).find(|&q| !matches!(c[q], ' ' | '\t' | '\r' | '\n')).map(|q| c[q]);
            out.push((i, end, if after == Some(':') { Kind::Key } else { Kind::Str }));
            i = end;
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            let end = (i..to).find(|&k| c[k] == '\n').unwrap_or(to);
            out.push((i, end, Kind::Comment));
            i = end;
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            let end = (i + 2..to).find(|&k| c[k] == '*' && c.get(k + 1) == Some(&'/')).map(|k| k + 2).unwrap_or(to);
            out.push((i, end, Kind::Comment));
            i = end;
        } else if ch == '-' || ch.is_ascii_digit() {
            let mut k = i + 1;
            while k < to && (c[k].is_ascii_digit() || matches!(c[k], '.' | 'e' | 'E' | '+' | '-')) {
                k += 1;
            }
            if k > i + 1 || ch.is_ascii_digit() {
                out.push((i, k, Kind::Number));
            }
            i = k;
        } else if ch.is_ascii_alphabetic() {
            let mut k = i + 1;
            while k < to && c[k].is_ascii_alphabetic() {
                k += 1;
            }
            let w: String = c[i..k].iter().collect();
            if matches!(w.as_str(), "true" | "false" | "null") {
                out.push((i, k, Kind::Keyword));
            }
            i = k;
        } else if matches!(ch, '{' | '}' | '[' | ']' | ',' | ':') {
            out.push((i, i + 1, Kind::Punct));
            i += 1;
        } else {
            i += 1;
        }
    }
}

// ---- HTML / XML --------------------------------------------------------------------------------------------------------------

fn markup(c: &[char], from: usize, to: usize, html: bool, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        if starts(c, i, "<!--") {
            let end = (i + 4..to).find(|&k| starts(c, k, "-->")).map(|k| k + 3).unwrap_or(to);
            out.push((i, end, Kind::Comment));
            i = end;
        } else if starts(c, i, "<![CDATA[") {
            let end = (i + 9..to).find(|&k| starts(c, k, "]]>")).map(|k| k + 3).unwrap_or(to);
            out.push((i, end, Kind::Str));
            i = end;
        } else if c[i] == '<' && c.get(i + 1).is_some_and(|n| n.is_alphabetic() || matches!(n, '/' | '!' | '?')) {
            // A tag: name, attributes, close.
            let mut k = i + 1;
            if matches!(c[k], '/' | '!' | '?') {
                k += 1;
            }
            let name_start = k;
            while k < to && (is_word(c[k]) || matches!(c[k], '-' | ':' | '.')) {
                k += 1;
            }
            let name: String = c[name_start..k].iter().collect::<String>().to_ascii_lowercase();
            out.push((i, k, if c[i + 1] == '!' { Kind::Keyword } else { Kind::Tag }));
            while k < to && c[k] != '>' {
                if c[k].is_whitespace() || c[k] == '/' || c[k] == '=' || c[k] == '?' {
                    k += 1;
                } else if c[k] == '"' || c[k] == '\'' {
                    let q = c[k];
                    let end = (k + 1..to).find(|&e| c[e] == q).map(|e| e + 1).unwrap_or(to);
                    out.push((k, end, Kind::Str));
                    k = end;
                } else {
                    let s = k;
                    while k < to && !c[k].is_whitespace() && !matches!(c[k], '=' | '>' | '/' | '"' | '\'') {
                        k += 1;
                    }
                    out.push((s, k, Kind::Attr));
                    if k == s {
                        k += 1;
                    }
                }
            }
            if k < to {
                out.push((k, k + 1, Kind::Tag));
                k += 1;
            }
            i = k;
            // The inside of <script> and <style> is code of its own.
            if html && c[name_start - 1] != '/' && (name == "script" || name == "style") {
                let close = format!("</{name}");
                let end = (i..to).find(|&q| c[q] == '<' && c[q..to.min(q + close.len())].iter().collect::<String>().eq_ignore_ascii_case(&close)).unwrap_or(to);
                run(c, i, end, if name == "script" { Lang::JavaScript } else { Lang::Css }, out);
                i = end;
            }
        } else if c[i] == '&' {
            let end = (i + 1..to.min(i + 12)).find(|&k| c[k] == ';').map(|k| k + 1);
            match end {
                Some(e) if c[i + 1..e - 1].iter().all(|x| x.is_alphanumeric() || *x == '#') => {
                    out.push((i, e, Kind::Number));
                    i = e;
                }
                _ => i += 1,
            }
        } else {
            i += 1;
        }
    }
}

// ---- Markdown (the source, coloured) -----------------------------------------------------------------------------------------------

fn markdown(c: &[char], from: usize, to: usize, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    let mut fenced = false;
    while i < to {
        let end = (i..to).find(|&k| c[k] == '\n').unwrap_or(to);
        let line = &c[i..end];
        let trimmed_start = line.iter().position(|x| !x.is_whitespace()).unwrap_or(line.len());
        let t = &line[trimmed_start..];
        let at = i + trimmed_start;
        if t.starts_with(&['`', '`', '`']) || t.starts_with(&['~', '~', '~']) {
            out.push((i, end, Kind::Code));
            fenced = !fenced;
        } else if fenced {
            out.push((i, end, Kind::Code));
        } else if t.first() == Some(&'#') && t.iter().take_while(|x| **x == '#').count() <= 6 && t.get(t.iter().take_while(|x| **x == '#').count()) == Some(&' ') {
            out.push((at, end, Kind::Heading));
        } else if t.first() == Some(&'>') {
            out.push((at, end, Kind::Comment));
        } else if t.len() >= 3 && t.iter().all(|x| matches!(x, '-' | '*' | '_' | ' ')) && t.iter().filter(|x| **x != ' ').count() >= 3 {
            out.push((at, end, Kind::Punct));
        } else {
            // A list marker.
            let mut body = at;
            if matches!(t.first(), Some('-' | '*' | '+')) && t.get(1) == Some(&' ') {
                out.push((at, at + 1, Kind::Punct));
                body = at + 2;
            } else {
                let digits = t.iter().take_while(|x| x.is_ascii_digit()).count();
                if digits > 0 && t.get(digits) == Some(&'.') && t.get(digits + 1) == Some(&' ') {
                    out.push((at, at + digits + 1, Kind::Punct));
                    body = at + digits + 2;
                }
            }
            inline_md(c, body, end, out);
        }
        i = end + 1;
    }
}

fn inline_md(c: &[char], from: usize, to: usize, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        match c[i] {
            '`' => {
                if let Some(e) = (i + 1..to).find(|&k| c[k] == '`') {
                    out.push((i, e + 1, Kind::Code));
                    i = e + 1;
                    continue;
                }
            }
            '*' | '_' => {
                let double = c.get(i + 1) == Some(&c[i]);
                let n = if double { 2 } else { 1 };
                let word_edge = i == from || !c[i - 1].is_alphanumeric();
                if word_edge && c.get(i + n).is_some_and(|x| !x.is_whitespace()) {
                    let close: Vec<char> = std::iter::repeat_n(c[i], n).collect();
                    if let Some(e) = (i + n + 1..to).find(|&k| c[k..to.min(k + n)] == close[..] && !c[k - 1].is_whitespace()) {
                        out.push((i, e + n, Kind::Emphasis));
                        i = e + n;
                        continue;
                    }
                }
            }
            '[' => {
                if let Some(e) = (i + 1..to).find(|&k| c[k] == ']') {
                    if c.get(e + 1) == Some(&'(') {
                        if let Some(p) = (e + 2..to).find(|&k| c[k] == ')') {
                            out.push((i, p + 1, Kind::Link));
                            i = p + 1;
                            continue;
                        }
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

// ---- INI / TOML / YAML -------------------------------------------------------------------------------------------------------

fn config(c: &[char], from: usize, to: usize, lang: Lang, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        let end = (i..to).find(|&k| c[k] == '\n').unwrap_or(to);
        let line = &c[i..end];
        let ws = line.iter().position(|x| !x.is_whitespace()).unwrap_or(line.len());
        let at = i + ws;
        let t = &line[ws..];
        if t.is_empty() {
            // nothing
        } else if t[0] == '#' || (lang == Lang::Ini && t[0] == ';') {
            out.push((at, end, Kind::Comment));
        } else if t[0] == '[' && lang != Lang::Yaml {
            out.push((at, end, Kind::Heading));
        } else if lang == Lang::Yaml && (t.starts_with(&['-', '-', '-']) || t.starts_with(&['.', '.', '.'])) {
            out.push((at, end, Kind::Punct));
        } else {
            let mut body = at;
            let mut start_value = None;
            // `- item` in YAML.
            let mut kstart = at;
            if lang == Lang::Yaml && t[0] == '-' && t.get(1) == Some(&' ') {
                out.push((at, at + 1, Kind::Punct));
                kstart = at + 2;
                body = kstart;
            }
            let sep = if lang == Lang::Yaml { ':' } else { '=' };
            if let Some(k) = (kstart..end).find(|&k| c[k] == sep && (lang != Lang::Yaml || c.get(k + 1).map_or(true, |x| x.is_whitespace()))) {
                let mut key_end = k;
                while key_end > kstart && c[key_end - 1].is_whitespace() {
                    key_end -= 1;
                }
                if key_end > kstart {
                    out.push((kstart, key_end, Kind::Key));
                }
                out.push((k, k + 1, Kind::Punct));
                start_value = Some(k + 1);
            }
            let vstart = start_value.unwrap_or(body);
            if start_value.is_some() || lang == Lang::Yaml {
                value_run(c, vstart, end, out);
            }
        }
        i = end + 1;
    }
}

fn value_run(c: &[char], from: usize, to: usize, out: &mut Vec<(usize, usize, Kind)>) {
    let mut i = from;
    while i < to {
        if c[i] == '#' && (i == from || c[i - 1].is_whitespace()) {
            out.push((i, to, Kind::Comment));
            return;
        }
        if c[i] == '"' || c[i] == '\'' {
            let q = c[i];
            let mut k = i + 1;
            while k < to && c[k] != q {
                k += if c[k] == '\\' && q == '"' { 2 } else { 1 };
            }
            let e = (k + 1).min(to);
            out.push((i, e, Kind::Str));
            i = e;
        } else if c[i].is_ascii_digit() || (c[i] == '-' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            if i == from || !is_word(c[i - 1]) {
                let mut k = i + 1;
                while k < to && (c[k].is_ascii_alphanumeric() || matches!(c[k], '.' | '_' | '-' | ':')) {
                    k += 1;
                }
                out.push((i, k, Kind::Number));
                i = k;
                continue;
            }
            i += 1;
        } else if c[i].is_ascii_alphabetic() {
            let mut k = i + 1;
            while k < to && c[k].is_ascii_alphanumeric() {
                k += 1;
            }
            let w: String = c[i..k].iter().collect::<String>().to_ascii_lowercase();
            if matches!(w.as_str(), "true" | "false" | "null" | "yes" | "no" | "on" | "off") && (i == from || !is_word(c[i - 1])) {
                out.push((i, k, Kind::Keyword));
            }
            i = k;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (text of the span, kind) pairs.
    fn pieces(text: &str, lang: Lang) -> Vec<(String, Kind)> {
        let u: Vec<u16> = text.encode_utf16().collect();
        spans(text, lang).into_iter().map(|s| (String::from_utf16_lossy(&u[s.start..s.start + s.len]), s.kind)).collect()
    }

    fn has(p: &[(String, Kind)], text: &str, kind: Kind) -> bool {
        p.iter().any(|(t, k)| t == text && *k == kind)
    }

    #[test]
    fn json_keys_strings_numbers_and_words() {
        let p = pieces("{\"name\": \"NuViewer\", \"n\": -12.5e3, \"ok\": true, \"x\": null, \"a\": [1, 2]}", Lang::Json);
        assert!(has(&p, "\"name\"", Kind::Key));
        assert!(has(&p, "\"NuViewer\"", Kind::Str));
        assert!(has(&p, "-12.5e3", Kind::Number));
        assert!(has(&p, "true", Kind::Keyword) && has(&p, "null", Kind::Keyword));
        assert!(has(&p, "{", Kind::Punct));
        assert!(!p.iter().any(|(t, _)| t == "name"), "no piece inside a string");
    }

    #[test]
    fn javascript_comments_strings_and_keywords() {
        let p = pieces("// hi\nconst x = `a ${b}` + 'c' + 42; /* block\nmore */ if (x) return;", Lang::JavaScript);
        assert!(has(&p, "// hi", Kind::Comment));
        assert!(has(&p, "const", Kind::Keyword) && has(&p, "if", Kind::Keyword) && has(&p, "return", Kind::Keyword));
        assert!(has(&p, "`a ${b}`", Kind::Str) && has(&p, "'c'", Kind::Str));
        assert!(has(&p, "42", Kind::Number));
        assert!(has(&p, "/* block\nmore */", Kind::Comment));
        // A keyword inside a name or a string is not one.
        let q = pieces("let iffy = \"if\";", Lang::JavaScript);
        assert!(!q.iter().any(|(t, _)| t == "iffy"));
        assert!(has(&q, "\"if\"", Kind::Str));
    }

    #[test]
    fn html_tags_attributes_and_embedded_code() {
        let src = "<!-- c --><div class=\"a b\" id='x'>t &amp; u</div><script>var n = 1; // js</script><style>p { color: red; }</style>";
        let p = pieces(src, Lang::Html);
        assert!(has(&p, "<!-- c -->", Kind::Comment));
        assert!(has(&p, "<div", Kind::Tag) && has(&p, "</div", Kind::Tag));
        assert!(has(&p, "class", Kind::Attr) && has(&p, "\"a b\"", Kind::Str) && has(&p, "id", Kind::Attr));
        assert!(has(&p, "&amp;", Kind::Number));
        assert!(has(&p, "var", Kind::Keyword) && has(&p, "// js", Kind::Comment), "the script is JavaScript: {p:?}");
        assert!(has(&p, "1", Kind::Number));
        assert!(p.iter().any(|(t, k)| t == "<script" && *k == Kind::Tag));
    }

    #[test]
    fn markdown_source_headings_code_emphasis_and_links() {
        let src = "# Title\n\nSome **bold** and `code` and [a link](http://x.y).\n\n- item *em*\n1. first\n```rust\nlet x = 1;\n```\n> quote\n---\n";
        let p = pieces(src, Lang::Markdown);
        assert!(has(&p, "# Title", Kind::Heading));
        assert!(has(&p, "**bold**", Kind::Emphasis) && has(&p, "*em*", Kind::Emphasis));
        assert!(has(&p, "`code`", Kind::Code));
        assert!(has(&p, "[a link](http://x.y)", Kind::Link));
        assert!(has(&p, "```rust", Kind::Code) && has(&p, "let x = 1;", Kind::Code), "a fenced block is code to its end: {p:?}");
        assert!(has(&p, "> quote", Kind::Comment));
        assert!(has(&p, "-", Kind::Punct) && has(&p, "1.", Kind::Punct));
    }

    #[test]
    fn ini_toml_and_yaml_keys_values_and_sections() {
        let toml = pieces("# c\n[package]\nname = \"nv\" # why\nversion = 1.2\nflag = true\n", Lang::Toml);
        assert!(has(&toml, "# c", Kind::Comment) && has(&toml, "[package]", Kind::Heading));
        assert!(has(&toml, "name", Kind::Key) && has(&toml, "\"nv\"", Kind::Str) && has(&toml, "# why", Kind::Comment));
        assert!(has(&toml, "1.2", Kind::Number) && has(&toml, "true", Kind::Keyword));
        let yaml = pieces("---\nname: nv\nitems:\n  - one\n  - 2\nurl: http://a:80/b\n", Lang::Yaml);
        assert!(has(&yaml, "name", Kind::Key) && has(&yaml, "items", Kind::Key) && has(&yaml, "url", Kind::Key));
        assert!(has(&yaml, "2", Kind::Number));
        let ini = pieces("; note\n[a]\nk=v\n", Lang::Ini);
        assert!(has(&ini, "; note", Kind::Comment) && has(&ini, "[a]", Kind::Heading) && has(&ini, "k", Kind::Key));
    }

    #[test]
    fn rust_python_sql_and_batch_have_their_own_comments_and_words() {
        assert!(has(&pieces("fn main() { let mut x = 1; } // end", Lang::Rust), "fn", Kind::Keyword));
        assert!(has(&pieces("def f(self):\n    \"\"\"doc\n    more\"\"\"\n    return None # x", Lang::Python), "\"\"\"doc\n    more\"\"\"", Kind::Str));
        assert!(has(&pieces("SELECT a FROM t -- c\nWHERE b = 'x';", Lang::Sql), "SELECT", Kind::Keyword));
        assert!(has(&pieces("SELECT a FROM t -- c", Lang::Sql), "-- c", Kind::Comment));
        let bat = pieces("@echo off\nrem a comment\nset X=1\necho remember", Lang::Batch);
        assert!(has(&bat, "rem a comment", Kind::Comment));
        assert!(!bat.iter().any(|(t, k)| t.starts_with("rem") && *k == Kind::Comment && t.contains("ember")), "a word that merely starts with rem is not a comment");
    }

    #[test]
    fn positions_are_utf16_and_line_ends_count_one() {
        // "é" is one unit, a character outside the BMP is two, a line end is one.
        let text = "é😀\n\"k\": 1";
        let s = spans(text, Lang::Json);
        let key = s.iter().find(|s| s.kind == Kind::Key).unwrap();
        assert_eq!((key.start, key.len), (4, 3), "é(1) + 😀(2) + newline(1) = 4");
        // Spans never overlap and come in order.
        for lang in Lang::ALL {
            let p = spans("a 1 \"s\" // c\n# h\n<t a=\"b\">", lang);
            assert!(p.windows(2).all(|w| w[0].start + w[0].len <= w[1].start), "{lang:?}: {p:?}");
        }
    }

    #[test]
    fn languages_by_extension_and_limits() {
        assert_eq!(lang_of_ext("JSON"), Lang::Json);
        assert_eq!(lang_of_ext("ts"), Lang::JavaScript);
        assert_eq!(lang_of_ext("svg"), Lang::Xml);
        assert_eq!(lang_of_ext("md"), Lang::Markdown);
        assert_eq!(lang_of_ext("rs"), Lang::Rust);
        assert_eq!(lang_of_ext("xyz"), Lang::Plain);
        assert!(spans(&"a".repeat(MAX_COLOURED_CHARS + 1), Lang::Json).is_empty(), "a huge text is left plain");
        assert!(spans("anything", Lang::Plain).is_empty());
        assert!(Lang::ALL.iter().all(|l| !l.label().is_empty()));
    }
}
