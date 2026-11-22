//! Who made NuViewer and what it stands on: the credit block shown at the bottom of every help page (viewer, gallery,
//! editor), centred, with the website and the support link (clickable). One place, so a change is made once.

/// The maker.
pub const STUDIO: &str = "Noobcyborg studio";
/// Where to reach the project and where to support it.
pub const WEBSITE: &str = "https://nuviewer.noobcyborg.com";
/// Help and questions: the NuViewer channel on Discord.
pub const SUPPORT: &str = "https://discord.com/channels/1572474506872750212/1572474507363491842";
/// Bug reports, ideas and "what stops you switching": the same channel.
pub const FEEDBACK: &str = "https://discord.com/channels/1572474506872750212/1572474507363491842";

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
    ("resvg, roxmltree", "SVG, XML (3MF, AMF, Collada)"),
    ("jxl-oxide, hayro-jpeg2000, flif, icns, psd", "JPEG XL, JPEG 2000, FLIF, ICNS, PSD"),
    ("kamadak-exif", "EXIF"),
    ("fbxcel-dom", "FBX models"),
    ("windows-rs", "Win32, Direct3D 11, WIC, Media Foundation, Windows.Data.Pdf"),
    ("FFmpeg (optional, if installed)", "video and sound Windows can't play, snapshots"),
    ("Keyline Icons (MIT)", "the two-tone icons"),
];

/// The credit block as text lines, to be drawn centred (a blank line first, so it can follow a list of help lines).
pub fn credit_lines() -> Vec<String> {
    use crate::i18n::{t, tf};
    let version = env!("CARGO_PKG_VERSION");
    let mut v = vec![String::new(), format!("{}  {version}", PROGRAMS.join("  ·  ")), tf("about.made_by", &[&STUDIO]), format!("{}   {WEBSITE}", t("about.website")), format!("{}   {SUPPORT}", t("about.support")), format!("{}   {FEEDBACK}", t("about.feedback")), String::new(), t("about.built_on").to_string()];
    // Two to a line keeps the block short.
    for pair in LIBRARIES.chunks(2) {
        v.push(pair.iter().map(|(n, what)| format!("{n} ({what})")).collect::<Vec<_>>().join("   ·   "));
    }
    v.push(t("about.thanks").to_string());
    v
}

/// The web address in a credit line, if it has one (a click on the line opens it).
pub fn link_of(line: &str) -> Option<String> {
    let p = line.find("https://")?;
    Some(line[p..].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_name_the_studio_the_version_the_links_and_the_libraries() {
        let lines = credit_lines();
        let text = lines.join("\n");
        assert!(text.contains(STUDIO));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(text.contains(WEBSITE) && text.contains(SUPPORT) && text.contains(FEEDBACK));
        assert_eq!(WEBSITE, "https://nuviewer.noobcyborg.com");
        assert!(SUPPORT.starts_with("https://discord.com/channels/") && FEEDBACK.starts_with("https://discord.com/channels/"));
        for l in lines.iter().filter(|l| l.contains("https://")) {
            assert_eq!(l.matches("https://").count(), 1, "one link per line, so a click is unambiguous: {l}");
        }
        for (name, _) in LIBRARIES {
            assert!(text.contains(name), "{name}");
        }
        assert!(lines.iter().all(|l| l.chars().count() < 170), "lines must fit a help screen");
        assert_eq!(link_of(lines.iter().find(|l| l.contains(WEBSITE)).unwrap()), Some(WEBSITE.to_string()));
        assert_eq!(link_of("no link here"), None);
    }
}
