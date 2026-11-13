//! Colour settings -> profiles. The settings name where a profile comes from (built in, the Windows display profile,
//! a file); this turns that into `nv_codecs` profiles and hands them over.

use nv_codecs::Profile;
use nv_core::settings::{ProfileChoice, Settings};
use nv_win::WindowHandle;
use std::path::{Path, PathBuf};

/// A profile and a line saying where it came from, e.g. `DELL U2723QE  (C:\Windows\...\DELL.icm)`.
pub struct Found {
    /// None = plain sRGB (nothing to convert to).
    pub profile: Option<Profile>,
    pub text: String,
}

fn from_file(path: &Path) -> Result<Found, String> {
    let p = Profile::from_file(path)?;
    let text = format!("{}  ({})", p.name, path.display());
    Ok(Found { profile: Some(p), text })
}

/// The profile `choice` stands for. `file` is the path for `ProfileChoice::File`; `hwnd` picks the display for
/// `ProfileChoice::System` (None = the primary one).
pub fn resolve(choice: ProfileChoice, file: &str, hwnd: Option<WindowHandle>) -> Result<Found, String> {
    match choice {
        ProfileChoice::Srgb => Ok(Found { profile: None, text: "sRGB".into() }),
        ProfileChoice::DisplayP3 => Ok(Found { profile: Some(Profile::display_p3()), text: "Display P3".into() }),
        ProfileChoice::AdobeRgb => Ok(Found { profile: Some(Profile::adobe_rgb()), text: "Adobe RGB (1998)".into() }),
        ProfileChoice::File => {
            let file = file.trim().trim_matches('"');
            if file.is_empty() {
                return Err("no profile file chosen".into());
            }
            from_file(&PathBuf::from(file))
        }
        ProfileChoice::System => {
            let path = nv_win::display_profile_path(hwnd).ok_or("Windows doesn't name a colour profile for this display")?;
            from_file(&path)
        }
    }
}

/// Make the settings' colour choices the active ones. On a problem the previous (or default) behaviour stays and the
/// message says what is wrong.
pub fn apply(s: &Settings, hwnd: Option<WindowHandle>) -> Result<(), String> {
    let output = resolve(s.icc_output, &s.icc_output_file, hwnd).map_err(|e| format!("Output profile: {e}"))?;
    let untagged = resolve(s.icc_untagged, &s.icc_untagged_file, hwnd).map_err(|e| format!("Profile for pictures without one: {e}"))?;
    nv_codecs::configure_colour(s.icc, output.profile, untagged.profile)
}

/// Would `apply` accept these settings? Changes nothing (the Settings form asks before it lets OK through).
pub fn check(s: &Settings, hwnd: Option<WindowHandle>) -> Result<(), String> {
    let output = resolve(s.icc_output, &s.icc_output_file, hwnd).map_err(|e| format!("Output profile: {e}"))?;
    let untagged = resolve(s.icc_untagged, &s.icc_untagged_file, hwnd).map_err(|e| format!("Profile for pictures without one: {e}"))?;
    nv_codecs::check_colour(output.profile, untagged.profile)
}

/// At start-up: like `apply`, but a profile that can't be used must not stop the viewer, so fall back to plain sRGB.
pub fn apply_at_startup(s: &Settings) {
    if apply(s, None).is_err() {
        let _ = nv_codecs::configure_colour(s.icc, None, None);
    }
}
