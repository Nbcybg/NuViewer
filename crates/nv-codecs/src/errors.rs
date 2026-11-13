//! Error codes. Every failure the user can see carries a short code, `NV-xxxx`, so a problem can be named in one word
//! ("NV-2002: HEIC without the Windows codec") instead of by whatever text a decoder library happened to produce.
//!
//! The decoders themselves still report plain text; the code is attached at the edges (`tag`, `classify`): the
//! top-level decode functions, the loader (a decoder that panicked), the GPU upload, the file operations. The first digit
//! is the family, see `TABLE`; `docs/error-codes.md` lists them for users.

/// One entry of the code table.
pub struct Info {
    pub code: u16,
    /// Short name of the problem.
    pub title: &'static str,
    /// What the user can do about it.
    pub advice: &'static str,
}

macro_rules! e {
    ($code:expr, $title:expr, $advice:expr) => {
        Info { code: $code, title: $title, advice: $advice }
    };
}

/// Every code. Families: 1xxx the file (access), 2xxx the format, 3xxx the contents, 4xxx limits, 5xxx graphics,
/// 6xxx the program itself, 7xxx file operations, 8xxx colour, 9xxx anything else.
pub const TABLE: &[Info] = &[
    e!(1001, "File not found", "The file was moved, renamed or deleted after the folder was listed. Reopen the folder."),
    e!(1002, "Access denied or file in use", "Another program has the file open or you may not read it. Close the other program or check the file's permissions."),
    e!(1003, "Reading the file failed", "The disk, network share or phone storage stopped answering. Check the connection and try again."),
    e!(1004, "The file is empty", "The file has no data (a copy or download that did not finish)."),
    e!(2001, "Unsupported file type", "NuViewer has no decoder for this format. Open it with another program (Right click > Open with)."),
    e!(2002, "Windows codec missing", "This format needs a Windows extension: HEIC/HEIF needs \"HEIF Image Extensions\" and \"HEVC Video Extensions\", AVIF needs \"AV1 Video Extension\" (Microsoft Store)."),
    e!(2003, "Unsupported variant of the format", "The file uses a compression, bit depth or colour mode that is not supported yet."),
    e!(2004, "Needs a component that is not installed", "Part of the file needs an extra program (for example Ghostscript for PostScript without a preview)."),
    e!(3000, "The file could not be decoded", "The decoder refused the file. It may be damaged, or not really this format."),
    e!(3001, "The file is damaged or incomplete", "The data ends early or does not add up. Try the original, or download or copy it again."),
    e!(3002, "Not a valid picture of this type", "The header is wrong: the extension probably does not match what is inside."),
    e!(3003, "Document or drawing could not be rendered", "The PDF, SVG, metafile or 3D model has content that the renderer cannot draw."),
    e!(4001, "Picture too large", "More pixels than NuViewer will decode (250 megapixels). Open it in a program made for huge pictures."),
    e!(4002, "Out of memory", "There is not enough free memory for this picture. Close other programs and try again."),
    e!(4003, "More than the graphics card can show", "The picture is bigger than the graphics card's texture limit; it is shown reduced when possible."),
    e!(5001, "Sending the picture to the graphics card failed", "The graphics driver refused the upload. Update the driver, or try again."),
    e!(5002, "Graphics device unavailable", "Direct3D could not start. Update the graphics driver; a remote session or a very old GPU may not be supported."),
    e!(6001, "The decoder crashed on this file", "A bug in a decoder was caught and the viewer carried on. The file is probably damaged; please report it if it opens elsewhere."),
    e!(6002, "Internal error", "A bug in the viewer (it closes cleanly; details are in %LOCALAPPDATA%\\NuViewer\\crash.log) or a decoder returned data of the wrong size. Please report it, with the file if there was one."),
    e!(6003, "Thumbnail could not be made", "The picture could not be read for the gallery; it may still open in the viewer."),
    e!(7001, "Rename failed", "The name is already used, contains a character Windows does not allow, or the file is in use."),
    e!(7002, "Delete failed", "The file is in use or protected. Close the program that has it open."),
    e!(7003, "Move or copy failed", "The destination is read-only, full, or has a file of that name that is in use."),
    e!(7004, "Saving the picture failed", "The folder is read-only or the disk is full. The original is untouched."),
    e!(7005, "Undo failed", "What the undo needs (the file or its backup) is no longer there."),
    e!(7006, "Clipboard failed", "Another program is holding the clipboard. Try again."),
    e!(7007, "Settings not saved", "The settings file could not be written (%APPDATA%\\NuViewer\\settings.ini)."),
    e!(8001, "Colour profile problem", "The ICC profile could not be read or used; the picture is shown without conversion."),
    e!(9999, "Unexpected error", "Something unexpected happened. The text after the code says what."),
];

/// The table entry for `code`.
pub fn info(code: u16) -> Option<&'static Info> {
    TABLE.iter().find(|i| i.code == code)
}

/// "NV-2002".
pub fn label(code: u16) -> String {
    format!("NV-{code:04}")
}

/// `message` with its code in front: `[NV-3001] unexpected end of file`. A message that already has one is returned as it is,
/// and so is the "cancelled" marker the loader uses internally (it is not an error).
pub fn tag(code: u16, message: &str) -> String {
    if code_of(message).is_some() || message == crate::progress::CANCELLED {
        message.to_string()
    } else {
        format!("[{}] {message}", label(code))
    }
}

/// The code a tagged message carries.
pub fn code_of(message: &str) -> Option<u16> {
    let rest = message.strip_prefix("[NV-")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    rest[digits.len()..].starts_with(']').then(|| digits.parse().ok()).flatten()
}

/// The message without its `[NV-xxxx] ` prefix.
pub fn plain(message: &str) -> &str {
    match (code_of(message), message.find("] ")) {
        (Some(_), Some(i)) => &message[i + 2..],
        _ => message,
    }
}

/// Choose the code for a decoder's error text. `ext` (lowercase, no dot) helps with the format-specific cases.
pub fn classify(message: &str, ext: &str) -> u16 {
    let m = message.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if let Some(c) = code_of(message) {
        return c;
    }
    if has(&["decoder panicked", "panicked"]) {
        6001
    } else if has(&["decoder produced"]) {
        6002
    } else if has(&["too large to open", "exceeds the limit", "limit 250 megapixels"]) {
        4001
    } else if has(&["out of memory", "not enough memory", "memory allocation", "allocation of", "failed to allocate", "os error 8)"]) {
        4002
    } else if has(&["empty file"]) {
        1004
    } else if has(&["no such file", "not found:", "cannot find the file", "cannot find the path", "(os error 2)", "(os error 3)", "system cannot find"]) {
        1001
    } else if has(&["access is denied", "permission denied", "being used by another process", "(os error 5)", "(os error 32)", "(os error 33)"]) {
        1002
    } else if has(&["no windows codec", "hevc codec", "heif image extensions", "av1 video extension", "componentnotfound"]) {
        2002
    } else if has(&["unsupported format", "no decoder for", "unknown format", "unrecognised picture format", "unrecognized"]) {
        2001
    } else if has(&["ghostscript", "needs an extra"]) {
        2004
    } else if has(&["unsupported", "not supported", "not implemented", "unimplemented", "compression", "bit depth", "colour mode", "color mode"]) {
        2003
    } else if has(&["unexpected end", "truncated", "unexpected eof", "failed to fill whole buffer", "premature", "incomplete", "corrupt", "damaged", "crc"]) {
        3001
    } else if has(&["empty image", "bad signature", "not a valid", "invalid header", "invalid signature", "bad magic", "invalid magic", "wrong signature", "missing signature", "invalid file"]) {
        3002
    } else if has(&["read:", "read error", "i/o error", "the device is not ready", "network"]) {
        1003
    } else if matches!(ext, "pdf" | "ai" | "eps" | "svg" | "svgz" | "emf" | "wmf" | "swf" | "obj" | "stl" | "ply" | "gltf" | "glb" | "fbx") {
        3003
    } else {
        3000
    }
}

/// `message` tagged with the code `classify` picks for it.
pub fn coded(message: &str, ext: &str) -> String {
    tag(classify(message, ext), message)
}

/// A one-line description for the info panel and the on-screen error: "NV-3001  The file is damaged or incomplete".
pub fn headline(message: &str) -> String {
    match code_of(message).and_then(info) {
        Some(i) => format!("{}  {}", label(i.code), i.title),
        None => message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_described() {
        let mut seen = std::collections::HashSet::new();
        for i in TABLE {
            assert!(seen.insert(i.code), "NV-{} is listed twice", i.code);
            assert!(!i.title.is_empty() && !i.advice.is_empty(), "NV-{} has no text", i.code);
            assert!((1000..=9999).contains(&i.code));
        }
        assert!(info(9999).is_some(), "there is always a catch-all");
    }

    #[test]
    fn tags_round_trip() {
        let t = tag(3001, "unexpected end of file");
        assert_eq!(t, "[NV-3001] unexpected end of file");
        assert_eq!(code_of(&t), Some(3001));
        assert_eq!(plain(&t), "unexpected end of file");
        assert_eq!(tag(2001, &t), t, "never tagged twice");
        assert_eq!(code_of("no code here"), None);
        assert_eq!(code_of("[NV-12x] nope"), None);
        assert_eq!(plain("plain text"), "plain text");
        assert_eq!(label(7), "NV-0007");
    }

    #[test]
    fn the_cancel_marker_is_never_tagged() {
        assert_eq!(tag(3000, crate::progress::CANCELLED), crate::progress::CANCELLED);
    }

    #[test]
    fn typical_messages_get_their_codes() {
        let cases: &[(&str, &str, u16)] = &[
            ("decoder panicked", "jpg", 6001),
            ("image too large to open (30000x30000, limit 250 megapixels)", "png", 4001),
            ("empty file", "jpg", 1004),
            ("read: The system cannot find the file specified. (os error 2)", "jpg", 1001),
            ("read: Access is denied. (os error 5)", "jpg", 1002),
            ("no Windows codec for this format (HEIC/AVIF need the Microsoft HEIF / AV1 image extensions)", "heic", 2002),
            ("needs the HEVC codec (install \"HEVC Video Extensions\" from the Microsoft Store)", "heic", 2002),
            ("unsupported format: .xyz", "xyz", 2001),
            ("unexpected end of file", "png", 3001),
            ("psd: unsupported colour mode", "psd", 2003),
            ("empty image", "bmp", 3002),
            ("decoder produced 12 bytes for 2x2 (expected 16)", "jpg", 6002),
            ("something odd happened", "jpg", 3000),
            ("something odd happened", "pdf", 3003),
        ];
        for (msg, ext, want) in cases {
            assert_eq!(classify(msg, ext), *want, "{msg}");
        }
        for (msg, ext, want) in cases {
            assert_eq!(code_of(&coded(msg, ext)), Some(*want));
        }
    }

    #[test]
    fn the_user_documentation_lists_every_code() {
        let page = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/error-codes.md")).expect("docs/error-codes.md");
        for i in TABLE {
            assert!(page.contains(&label(i.code)), "docs/error-codes.md is missing {}", label(i.code));
            assert!(page.contains(i.title), "docs/error-codes.md has no entry titled {:?}", i.title);
        }
    }

    #[test]
    fn headline_names_the_problem() {
        assert_eq!(headline("[NV-1001] gone"), "NV-1001  File not found");
        assert_eq!(headline("untagged"), "untagged");
    }
}
