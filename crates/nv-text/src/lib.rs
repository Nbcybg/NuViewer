//! Text for the screen, kept out of the code: every program asks `t("key")` for what to show (and `tr("English phrase")` for
//! text that predates the keys: the phrase itself is the key, so a translation file can cover every window, menu and message
//! without touching the code). The English text lives in
//! `lang/en.txt` (compiled in as the fallback); a translation is a file of the same shape, `lang/<code>.txt`, found next to
//! the program or in `%APPDATA%\NuViewer\lang`. A key a translation doesn't have shows in English, a key nobody has shows
//! as itself. Nothing here needs a restart to add a language: put the file there and set `language = th` in the settings.
//!
//! File format: `key = text`, one per line; `#` starts a comment line; `\n` in the text is a line break, `\\` a backslash.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

const ENGLISH: &str = include_str!("../lang/en.txt");

/// True once a language other than English is loaded: `tr` does nothing before that (English costs nothing).
static TRANSLATING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static TABLE: OnceLock<RwLock<HashMap<String, &'static str>>> = OnceLock::new();
static LANG: RwLock<String> = RwLock::new(String::new());

fn table() -> &'static RwLock<HashMap<String, &'static str>> {
    TABLE.get_or_init(|| RwLock::new(parse(ENGLISH)))
}

/// Parse a language file into key -> text (the text is leaked: tables are loaded once or twice per run, never in a loop).
pub fn parse(text: &str) -> HashMap<String, &'static str> {
    let mut m = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let mut out = String::with_capacity(v.len());
        let mut chars = v.trim().chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some(o) => {
                        out.push('\\');
                        out.push(o)
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        m.insert(k.trim().to_string(), &*Box::leak(out.into_boxed_str()));
    }
    m
}

/// The folders a translation is looked for in.
fn dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = std::env::var_os("NUVIEWER_LANG_DIR") {
        v.push(PathBuf::from(d));
    }
    if let Some(a) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(a).join("NuViewer").join("lang"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("lang"));
        }
    }
    v
}

/// Choose the language: `code` is "en", "th", "pt-BR" ...; "auto" or "" takes `system` (the Windows display language).
/// Loads `<code>.txt` (then `<language part>.txt`) over the English text. Safe to call again when the setting changes.
pub fn init(code: &str, system: &str) {
    let want = if code.trim().is_empty() || code.eq_ignore_ascii_case("auto") { system } else { code }.trim().to_string();
    let short = want.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase();
    if short.is_empty() || short == "en" {
        // English is the built-in table: nothing to load (and no cost at start-up until the first text is asked for).
        if TABLE.get().is_some() {
            *table().write().unwrap() = parse(ENGLISH);
        }
        *LANG.write().unwrap() = want;
        TRANSLATING.store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    let mut merged = parse(ENGLISH);
    {
        for name in [short.clone(), want.to_ascii_lowercase()] {
            for d in dirs() {
                if let Ok(text) = std::fs::read_to_string(d.join(format!("{name}.txt"))) {
                    merged.extend(parse(&text));
                }
            }
        }
    }
    *table().write().unwrap() = merged;
    *LANG.write().unwrap() = want;
    TRANSLATING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The text for `key`.
pub fn t(key: &str) -> &'static str {
    match table().read().unwrap().get(key) {
        Some(s) => s,
        None => Box::leak(key.to_string().into_boxed_str()), // a missing key shows as itself (and is easy to spot)
    }
}

/// A phrase written in English in the code, in the language in use: the translation file has the phrase as a key
/// (`Previous picture = ...`); without one the phrase itself comes back. Anything after a tab (a shortcut) is kept as it is.
pub fn tr(phrase: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if !TRANSLATING.load(std::sync::atomic::Ordering::Relaxed) || phrase.is_empty() {
        return Cow::Borrowed(phrase);
    }
    let (text, tail) = match phrase.split_once(char::from(9)) {
        Some((a, b)) => (a, Some(b)),
        None => (phrase, None),
    };
    // Menu labels carry '&' for the key letter: the phrase in the file is without it, and so is the shortcut key of the translation.
    let found = table().read().unwrap().get(text).copied();
    match (found, tail) {
        (Some(t), Some(tail)) => Cow::Owned(format!("{t}{}{tail}", char::from(9))),
        (Some(t), None) => Cow::Borrowed(t),
        (None, _) => Cow::Borrowed(phrase),
    }
}

/// `t(key)` with `{0}`, `{1}` ... replaced.
pub fn tf(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = t(key).to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

/// The language in use ("en", "th", ...).
pub fn language() -> String {
    LANG.read().unwrap().clone()
}

/// Languages that have a file in one of the folders, plus English.
pub fn available() -> Vec<String> {
    let mut v = vec!["en".to_string()];
    for d in dirs() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("txt") {
                    if let Some(s) = p.file_stem().and_then(|s| s.to_str()) {
                        if !v.iter().any(|x| x == s) {
                            v.push(s.to_string());
                        }
                    }
                }
            }
        }
    }
    v
}

/// Keys of the English file (for the audit and for checking a translation).
pub fn english_keys() -> Vec<String> {
    parse(ENGLISH).into_keys().collect()
}

/// Keys the translation file at `path` lacks, and keys it has that English doesn't.
pub fn compare(path: &Path) -> (Vec<String>, Vec<String>) {
    let en = parse(ENGLISH);
    let other = std::fs::read_to_string(path).map(|t| parse(&t)).unwrap_or_default();
    let mut missing: Vec<String> = en.keys().filter(|k| !other.contains_key(*k)).cloned().collect();
    let mut extra: Vec<String> = other.keys().filter(|k| !en.contains_key(*k)).cloned().collect();
    missing.sort();
    extra.sort();
    (missing, extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_escapes_and_bom() {
        let m = parse("\u{feff}# c\na.b = Hello\\nWorld\nx=1\n\nbad line\nq = back\\\\slash");
        assert_eq!(m["a.b"], "Hello\nWorld");
        assert_eq!(m["x"], "1");
        assert_eq!(m["q"], "back\\slash");
        assert!(!m.contains_key("bad line"));
    }

    #[test]
    fn english_has_no_duplicate_or_empty_keys() {
        let mut seen = std::collections::HashSet::new();
        for line in ENGLISH.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (k, v) = line.split_once('=').unwrap_or_else(|| panic!("no '=' in {line}"));
            assert!(seen.insert(k.trim().to_string()), "duplicate key {k}");
            assert!(!v.trim().is_empty(), "empty text for {k}");
        }
    }

    #[test]
    fn missing_key_shows_itself_and_args_fill_in() {
        assert_eq!(t("no.such.key"), "no.such.key");
        init("en", "en");
        assert_eq!(tf("test.args", &[&1, &"two"]), "test.args");
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    #[test]
    fn the_thai_file_parses_and_only_uses_known_keys_or_phrases() {
        let th = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/lang/th.txt")).unwrap();
        let t = parse(&th);
        assert!(t.len() > 80);
        let en = parse(ENGLISH);
        for k in t.keys() {
            assert!(en.contains_key(k), "th.txt has a key English doesn't: {k}");
        }
    }

    #[test]
    fn a_translation_replaces_keys_and_phrases_and_keeps_shortcuts() {
        let dir = std::env::temp_dir().join(format!("nv_lang_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/lang/th.txt"), dir.join("th.txt")).unwrap();
        std::env::set_var("NUVIEWER_LANG_DIR", &dir);
        init("th", "en");
        assert_eq!(t("menu.snapshot"), "บันทึกเฟรมนี้เป็นภาพ");
        assert_eq!(tr("Fit to screen"), "พอดีหน้าจอ");
        assert_eq!(tr("Fit to screen\tF"), "พอดีหน้าจอ\tF");
        assert_eq!(tr("Not in the file at all"), "Not in the file at all");
        assert_eq!(t("importer.title"), "NuImporter", "what th.txt leaves out stays English");
        init("en", "en");
        assert_eq!(tr("Fit to screen"), "Fit to screen");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
