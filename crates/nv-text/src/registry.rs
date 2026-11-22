//! The languages the programs are prepared for. This is the mock-up stage: the seven languages below have a place (a code, a
//! name in their own script, a plural rule, a writing direction, a number format) and an empty-but-valid `lang/<code>.txt`, so
//! picking one is possible and shows English until the translation is filled in. Thai (the author's language) has a real
//! starter file. Nothing here translates anything.
//!
//! The seven: English, Chinese (Simplified), Spanish, Japanese, German, French and Portuguese (Brazil) - the languages most
//! software is localised into, together covering most of the world's screens. Thai is kept as an eighth, home, language.

/// How far a language is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Every key has a text.
    Complete,
    /// Some keys have a text (a starter file); the rest shows English.
    Starter,
    /// The slot exists (the file has a name and nothing else): it shows English. Not translated yet.
    Mockup,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Complete => "complete",
            Status::Starter => "partly translated",
            Status::Mockup => "mock-up, shows English",
        }
    }
}

/// Which words get which plural form: `plural_form(rule, n)` picks the index into a list of forms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plural {
    /// Chinese, Japanese, Thai: no plural forms (always form 0).
    None,
    /// English, Spanish, German: 1 is form 0, everything else form 1.
    OneOther,
    /// French and Brazilian Portuguese: 0 and 1 are form 0, everything else form 1.
    ZeroOneOther,
}

pub fn plural_form(rule: Plural, n: u64) -> usize {
    match rule {
        Plural::None => 0,
        Plural::OneOther => usize::from(n != 1),
        Plural::ZeroOneOther => usize::from(n > 1),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LangInfo {
    /// The file name (`lang/<code>.txt`) and the value of `language =` in the settings.
    pub code: &'static str,
    /// The name in its own language (what the language list shows).
    pub native: &'static str,
    pub english: &'static str,
    pub status: Status,
    pub plural: Plural,
    /// Written right to left (none of the seven; room for Arabic and Hebrew).
    pub rtl: bool,
    /// Decimal and thousands separators of numbers.
    pub decimal: char,
    pub thousands: char,
    /// Font families that carry the script, tried in this order (the UI font falls back to the first one that exists).
    pub fonts: &'static [&'static str],
}

pub const LANGUAGES: [LangInfo; 8] = [
    LangInfo { code: "en", native: "English", english: "English", status: Status::Complete, plural: Plural::OneOther, rtl: false, decimal: '.', thousands: ',', fonts: &["Segoe UI"] },
    LangInfo { code: "zh-CN", native: "简体中文", english: "Chinese (Simplified)", status: Status::Mockup, plural: Plural::None, rtl: false, decimal: '.', thousands: ',', fonts: &["Microsoft YaHei UI", "Segoe UI"] },
    LangInfo { code: "es", native: "Español", english: "Spanish", status: Status::Mockup, plural: Plural::OneOther, rtl: false, decimal: ',', thousands: '.', fonts: &["Segoe UI"] },
    LangInfo { code: "ja", native: "日本語", english: "Japanese", status: Status::Mockup, plural: Plural::None, rtl: false, decimal: '.', thousands: ',', fonts: &["Yu Gothic UI", "Meiryo UI", "Segoe UI"] },
    LangInfo { code: "de", native: "Deutsch", english: "German", status: Status::Mockup, plural: Plural::OneOther, rtl: false, decimal: ',', thousands: '.', fonts: &["Segoe UI"] },
    LangInfo { code: "fr", native: "Français", english: "French", status: Status::Mockup, plural: Plural::ZeroOneOther, rtl: false, decimal: ',', thousands: '\u{202F}', fonts: &["Segoe UI"] },
    LangInfo { code: "pt-BR", native: "Português (Brasil)", english: "Portuguese (Brazil)", status: Status::Mockup, plural: Plural::ZeroOneOther, rtl: false, decimal: ',', thousands: '.', fonts: &["Segoe UI"] },
    LangInfo { code: "th", native: "ไทย", english: "Thai", status: Status::Starter, plural: Plural::None, rtl: false, decimal: '.', thousands: ',', fonts: &["Leelawadee UI", "Segoe UI"] },
];

/// The registry entry for a code or a Windows language tag ("ja-JP" finds "ja", "pt-PT" finds "pt-BR", "zh-TW" and "zh-Hans" find "zh-CN").
pub fn find(code: &str) -> Option<&'static LangInfo> {
    let c = code.trim().replace('_', "-").to_ascii_lowercase();
    if c.is_empty() {
        return None;
    }
    if let Some(l) = LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(&c)) {
        return Some(l);
    }
    let short = c.split('-').next().unwrap_or("");
    LANGUAGES.iter().find(|l| l.code.split('-').next().map(|s| s.eq_ignore_ascii_case(short)).unwrap_or(false))
}

/// "日本語  (Japanese, mock-up)" for the language list: the name in its own script first.
pub fn list_label(l: &LangInfo) -> String {
    match l.status {
        Status::Complete => format!("{}  ({})", l.native, l.english),
        _ if l.native == l.english => l.native.to_string(),
        s => format!("{}  ({}, {})", l.native, l.english, s.label()),
    }
}

/// A number in the language's own way: 1234567.5 -> "1,234,567.5" / "1.234.567,5".
pub fn format_number(l: &LangInfo, value: f64, decimals: usize) -> String {
    let s = format!("{:.*}", decimals, value.abs());
    let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), Some(b.to_string()))).unwrap_or((s.clone(), None));
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(l.thousands);
        }
        grouped.push(c);
    }
    let sign = if value < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') { "-" } else { "" };
    match frac {
        Some(f) => format!("{sign}{grouped}{}{f}", l.decimal),
        None => format!("{sign}{grouped}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_international_languages_and_thai() {
        assert_eq!(LANGUAGES.len(), 8);
        let codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
        for c in ["en", "zh-CN", "es", "ja", "de", "fr", "pt-BR", "th"] {
            assert!(codes.contains(&c), "{c}");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(codes.iter().all(|c| seen.insert(c.to_ascii_lowercase())), "codes are unique");
        assert_eq!(LANGUAGES.iter().filter(|l| l.status == Status::Mockup).count(), 6);
    }

    #[test]
    fn windows_tags_find_their_language() {
        assert_eq!(find("ja-JP").unwrap().code, "ja");
        assert_eq!(find("pt-PT").unwrap().code, "pt-BR");
        assert_eq!(find("zh_TW").unwrap().code, "zh-CN");
        assert_eq!(find("PT-br").unwrap().code, "pt-BR");
        assert_eq!(find("en-GB").unwrap().code, "en");
        assert_eq!(find("th-TH").unwrap().code, "th");
        assert!(find("xx").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn plural_rules() {
        assert_eq!([0, 1, 2, 5].map(|n| plural_form(Plural::OneOther, n)), [1, 0, 1, 1]);
        assert_eq!([0, 1, 2, 5].map(|n| plural_form(Plural::ZeroOneOther, n)), [0, 0, 1, 1]);
        assert_eq!([0, 1, 2, 5].map(|n| plural_form(Plural::None, n)), [0, 0, 0, 0]);
    }

    #[test]
    fn numbers_follow_the_language() {
        let en = find("en").unwrap();
        let de = find("de").unwrap();
        let fr = find("fr").unwrap();
        assert_eq!(format_number(en, 1234567.5, 1), "1,234,567.5");
        assert_eq!(format_number(de, 1234567.5, 1), "1.234.567,5");
        assert_eq!(format_number(fr, 1234.0, 0), "1\u{202F}234");
        assert_eq!(format_number(en, -12.0, 0), "-12");
        assert_eq!(format_number(en, 999.0, 0), "999");
        assert_eq!(format_number(en, -0.0, 0), "0");
    }

    #[test]
    fn list_labels_say_what_state_a_language_is_in() {
        assert_eq!(list_label(find("en").unwrap()), "English  (English)");
        assert!(list_label(find("ja").unwrap()).contains("mock-up"));
        assert!(list_label(find("th").unwrap()).contains("partly"));
    }

    #[test]
    fn every_registered_language_has_a_file_with_its_own_name() {
        for l in LANGUAGES.iter().filter(|l| l.code != "en") {
            let path = format!("{}/lang/{}.txt", env!("CARGO_MANIFEST_DIR"), l.code);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("missing {path}"));
            let m = crate::parse(&text);
            assert_eq!(m.get("lang.name").copied(), Some(l.native), "{path}: lang.name");
            let en = crate::english_keys();
            for k in m.keys() {
                assert!(en.contains(k), "{path} has a key English doesn't: {k}");
            }
        }
    }
}
