//! Who made NuViewer and what it stands on: the credit block shown at the bottom of every help page (viewer, gallery,
//! editor). One place, so a change is made once.

/// The author line of the credits.
pub const AUTHOR: &str = "Nbcybg";

/// The programs of the family, in the order the credits list them.
pub const PROGRAMS: [&str; 4] = ["NuViewer", "NuGallery", "NuEditor", "NuImporter"];

/// Open-source pieces and what they are used for: (name, what for). Their licences allow this use; their authors get the credit.
pub const LIBRARIES: &[(&str, &str)] = &[
    ("zune-jpeg", "JPEG decoding"),
    ("png", "PNG / APNG"),
    ("image, image-webp, gif", "WebP, GIF, TGA, PNM, HDR, EXR, QOI"),
    ("jpeg-encoder, jpeg-decoder", "JPEG writing / DCT scaling"),
    ("moxcms", "ICC colour management"),
    ("rawler", "camera RAW sensor data"),
    ("resvg", "SVG"),
    ("jxl-oxide, hayro-jpeg2000, flif, icns, psd", "JPEG XL, JPEG 2000, FLIF, ICNS, PSD"),
    ("kamadak-exif", "EXIF"),
    ("fbxcel-dom", "FBX models"),
    ("windows-rs", "Win32, Direct3D 11, WIC, Windows.Data.Pdf"),
];

/// The credit block as text lines (a blank line first, so it can be appended to a list of help lines).
pub fn credit_lines() -> Vec<String> {
    let version = env!("CARGO_PKG_VERSION");
    let mut v = vec![
        String::new(),
        format!("Credits  —  {} {version}", PROGRAMS.join(" · ")),
        format!("Made by {AUTHOR}."),
        "Built on Rust, Direct3D 11 and the Windows imaging components, and on these open-source libraries:".to_string(),
    ];
    // Two to a line keeps the block short.
    for pair in LIBRARIES.chunks(2) {
        v.push(pair.iter().map(|(n, what)| format!("{n} ({what})")).collect::<Vec<_>>().join("   ·   "));
    }
    v.push("Thank you to everyone who wrote them.".to_string());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_name_the_author_the_version_and_the_libraries() {
        let text = credit_lines().join("\n");
        assert!(text.contains(AUTHOR));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        for (name, _) in LIBRARIES {
            assert!(text.contains(name), "{name}");
        }
        assert!(credit_lines().iter().all(|l| l.chars().count() < 160), "lines must fit a help screen");
    }
}
