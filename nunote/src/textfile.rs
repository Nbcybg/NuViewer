//! A text file as bytes <-> text: which encoding it is in, which line ends it uses, so saving writes back what was read. Pure code.

/// The encoding of a file on disk.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// Anything that is not valid UTF-8 and has no byte order mark: the Windows ANSI code page (Latin-1 as the portable stand-in here).
    Ansi,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Ansi => "ANSI",
        }
    }
}

/// The line ends of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineEnd {
    Crlf,
    Lf,
    Cr,
}

impl LineEnd {
    pub fn label(self) -> &'static str {
        match self {
            LineEnd::Crlf => "Windows (CRLF)",
            LineEnd::Lf => "Unix (LF)",
            LineEnd::Cr => "Mac (CR)",
        }
    }

    fn text(self) -> &'static str {
        match self {
            LineEnd::Crlf => "\r\n",
            LineEnd::Lf => "\n",
            LineEnd::Cr => "\r",
        }
    }
}

/// What was read: the text with every line end as a single `\n`, and how to write it back.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub text: String,
    pub encoding: Encoding,
    pub line_end: LineEnd,
}

/// Decode `bytes`. A byte order mark decides; else valid UTF-8 is UTF-8; else the ANSI code page. A NUL byte in what would be UTF-8 means
/// the file is not text: `Err`.
pub fn decode(bytes: &[u8]) -> Result<Loaded, String> {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), Encoding::Utf8Bom)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (utf16(rest, true), Encoding::Utf16Le)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (utf16(rest, false), Encoding::Utf16Be)
    } else {
        // A file of UTF-16 without a mark: every other byte is NUL.
        if bytes.len() >= 4 && bytes.iter().skip(1).step_by(2).take(64).all(|&b| b == 0) && bytes.iter().step_by(2).take(64).any(|&b| b != 0) {
            (utf16(bytes, true), Encoding::Utf16Le)
        } else {
            match std::str::from_utf8(bytes) {
                Ok(s) => (s.to_string(), Encoding::Utf8),
                Err(_) => (bytes.iter().map(|&b| ansi_char(b)).collect(), Encoding::Ansi),
            }
        }
    };
    if text.chars().take(8000).any(|c| c == '\0') {
        return Err("this does not look like a text file (it has NUL bytes)".into());
    }
    let (crlf, lf, cr) = count_ends(&text);
    let line_end = if crlf >= lf && crlf >= cr && crlf > 0 {
        LineEnd::Crlf
    } else if lf >= cr && lf > 0 {
        LineEnd::Lf
    } else if cr > 0 {
        LineEnd::Cr
    } else {
        LineEnd::Crlf // a file without line ends: the Windows way for the lines typed in
    };
    Ok(Loaded { text: normalise(&text), encoding, line_end })
}

fn utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| if little { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
    String::from_utf16_lossy(&units)
}

/// A byte of Windows-1252 as a character (0x80..0x9F are the punctuation of that code page, not control characters).
fn ansi_char(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}', '\u{017D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}',
        '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{9D}', '\u{017E}', '\u{0178}',
    ];
    match b {
        0x80..=0x9F => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

fn ansi_byte(c: char) -> Option<u8> {
    match c as u32 {
        0..=0x7F | 0xA0..=0xFF => Some(c as u8),
        _ => (0x80u8..=0x9F).find(|&b| ansi_char(b) == c),
    }
}

fn count_ends(text: &str) -> (usize, usize, usize) {
    let b = text.as_bytes();
    let (mut crlf, mut lf, mut cr) = (0, 0, 0);
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' if b.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    (crlf, lf, cr)
}

/// Every line end as a single `\n`.
pub fn normalise(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The bytes to write for `text` (line ends as `\n`) in the given encoding and with the given line ends. `Err` when the text has a
/// character the ANSI code page cannot hold (the caller offers UTF-8 instead).
pub fn encode(text: &str, encoding: Encoding, line_end: LineEnd) -> Result<Vec<u8>, String> {
    let body = if line_end == LineEnd::Lf { text.to_string() } else { text.replace('\n', line_end.text()) };
    Ok(match encoding {
        Encoding::Utf8 => body.into_bytes(),
        Encoding::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], body.as_bytes()].concat(),
        Encoding::Utf16Le => [&[0xFF, 0xFE][..], &body.encode_utf16().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>()].concat(),
        Encoding::Utf16Be => [&[0xFE, 0xFF][..], &body.encode_utf16().flat_map(|u| u.to_be_bytes()).collect::<Vec<u8>>()].concat(),
        Encoding::Ansi => {
            let mut out = Vec::with_capacity(body.len());
            for c in body.chars() {
                out.push(ansi_byte(c).ok_or_else(|| format!("the character '{c}' cannot be written in the ANSI code page"))?);
            }
            out
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_are_recognised_and_written_back_the_same() {
        let sample = "héllo\r\nวันนี้ 日本\r\nend";
        for enc in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be] {
            let bytes = encode(&normalise(sample), enc, LineEnd::Crlf).unwrap();
            let back = decode(&bytes).unwrap();
            assert_eq!(back.encoding, if enc == Encoding::Utf8 { Encoding::Utf8 } else { enc }, "{enc:?}");
            assert_eq!(back.text, "héllo\nวันนี้ 日本\nend");
            assert_eq!(back.line_end, LineEnd::Crlf);
            assert_eq!(encode(&back.text, back.encoding, back.line_end).unwrap(), bytes, "round trip of {enc:?}");
        }
    }

    #[test]
    fn ansi_files_and_their_limits() {
        let bytes = b"caf\xE9 \x93quoted\x94";
        let l = decode(bytes).unwrap();
        assert_eq!(l.encoding, Encoding::Ansi);
        assert_eq!(l.text, "café \u{201C}quoted\u{201D}");
        assert_eq!(encode(&l.text, Encoding::Ansi, LineEnd::Lf).unwrap(), bytes);
        assert!(encode("日本", Encoding::Ansi, LineEnd::Lf).is_err());
    }

    #[test]
    fn line_ends_are_counted_and_kept() {
        assert_eq!(decode(b"a\nb\nc").unwrap().line_end, LineEnd::Lf);
        assert_eq!(decode(b"a\r\nb\r\n").unwrap().line_end, LineEnd::Crlf);
        assert_eq!(decode(b"a\rb\rc").unwrap().line_end, LineEnd::Cr);
        // A mix: the majority wins; what is written afterwards is uniform.
        let l = decode(b"a\nb\nc\r\nd").unwrap();
        assert_eq!(l.line_end, LineEnd::Lf);
        assert_eq!(l.text, "a\nb\nc\nd");
        assert_eq!(decode(b"no line ends").unwrap().line_end, LineEnd::Crlf);
        assert_eq!(encode("x\ny", Encoding::Utf8, LineEnd::Cr).unwrap(), b"x\ry");
    }

    #[test]
    fn utf16_without_a_mark_and_binary_files() {
        let mut b = Vec::new();
        for u in "plain text here".encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        let l = decode(&b).unwrap();
        assert_eq!((l.encoding, l.text.as_str()), (Encoding::Utf16Le, "plain text here"));
        assert!(decode(b"MZ\x90\x00\x03\x00\x00\x00binary").is_err());
        assert_eq!(decode(b"").unwrap().text, "");
    }
}
