//! Video and sound files as the picture programs show for them. The playing itself is `nv-media`; here a media file is
//! only classified, and its still picture (the "poster") is made: a WAV draws its waveform natively, everything else asks
//! a decoder the program registers (`set_poster`), so a program that never plays media never loads Media Foundation.

use crate::Image;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "m4v", "mov", "mkv", "avi", "wmv", "webm", "mpg", "mpeg", "mpe", "ts", "mts", "m2ts", "m2t", "3gp", "3g2", "flv", "f4v", "ogv", "ogm", "vob", "asf", "dv", "rm", "rmvb", "divx", "m2v", "mod", "tod", "wtv",
];
pub const AUDIO_EXTS: &[&str] = &[
    "wav", "mp3", "flac", "aac", "m4a", "m4b", "ogg", "oga", "opus", "wma", "aif", "aiff", "aifc", "au", "snd", "pcm", "alac", "ape", "dsf", "dff", "dsd", "wv", "tta", "mka", "mpc", "mp2", "amr", "ac3", "dts", "caf", "weba",
];

/// Formats Windows has no player for: these go straight to the ffmpeg helper (a playable copy, kept in a cache) instead of
/// being tried in Media Foundation first. Everything else is tried natively and falls back to the helper when that fails.
pub const HELPER_EXTS: &[&str] = &[
    "ogg", "oga", "opus", "weba", "ape", "dsf", "dff", "dsd", "wv", "tta", "mka", "mpc", "au", "snd", "pcm", "caf", "amr", "ac3", "dts", "alac", "mp2", "flv", "f4v", "ogv", "ogm", "vob", "rm", "rmvb", "divx", "m2v", "wtv", "dv", "tod", "mod",
];

pub fn needs_helper(ext: &str) -> bool {
    HELPER_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn needs_helper_path(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(needs_helper)
}
/// Camera / broadcast formats Windows has no codec for: recognised and described, played through a proxy when there is one.
pub const PRO_EXTS: &[&str] = &["r3d", "mxf", "braw"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MediaKind {
    Video,
    Audio,
    /// R3D, MXF, BRAW.
    Pro,
}

pub fn media_kind(ext: &str) -> Option<MediaKind> {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    if VIDEO_EXTS.contains(&e) {
        Some(MediaKind::Video)
    } else if AUDIO_EXTS.contains(&e) {
        Some(MediaKind::Audio)
    } else if PRO_EXTS.contains(&e) {
        Some(MediaKind::Pro)
    } else {
        None
    }
}

pub fn media_kind_of(path: &Path) -> Option<MediaKind> {
    path.extension().and_then(|e| e.to_str()).and_then(media_kind)
}

type PosterFn = fn(&Path) -> Result<Image, String>;
static POSTER: OnceLock<PosterFn> = OnceLock::new();

/// A program that can play media tells the decoder how to get a picture out of a video (a frame) or a sound (its waveform).
pub fn set_poster(f: PosterFn) {
    let _ = POSTER.set(f);
}

/// What the file is, from its first bytes (None = not one of the professional formats).
pub fn describe_pro(head: &[u8]) -> Option<&'static str> {
    if head.len() >= 8 && (&head[4..8] == b"RED2" || &head[4..8] == b"RED1") {
        Some("RED R3D clip")
    } else if head.starts_with(&[0x06, 0x0E, 0x2B, 0x34]) {
        Some("MXF (SMPTE 377) clip")
    } else if head.len() >= 8 && (&head[4..8] == b"braw" || head.windows(4).take(8).any(|w| w == b"braw")) {
        Some("Blackmagic RAW clip")
    } else {
        None
    }
}

/// A playable stand-in for a professional clip: a file with the same name and a common extension next to it, or in a
/// `Proxy` folder beside it (what cameras and editing programs write), or `name_proxy.ext`.
pub fn proxy_for(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let dir = path.parent()?;
    let mut places = vec![dir.to_path_buf()];
    for d in ["Proxy", "proxy", "Proxies", "PROXY"] {
        places.push(dir.join(d));
    }
    for place in places {
        for name in [stem.clone(), format!("{stem}_proxy"), format!("{stem}_Proxy"), format!("{stem}-proxy")] {
            for ext in ["mp4", "mov", "m4v"] {
                let p = place.join(format!("{name}.{ext}"));
                if p.is_file() && p != path {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// The picture for a media file (`first_only` is accepted for symmetry: a poster is one picture anyway).
pub fn decode(path: &Path, _first_only: bool) -> Result<Image, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if ext == "wav" {
        match crate::wave::waveform(path) {
            Ok(img) => return Ok(img),
            Err(e) => {
                // A compressed WAV (ADPCM, MP3-in-WAV): let the sound decoder have it.
                if let Some(f) = POSTER.get() {
                    return f(path).map_err(|e2| format!("{e}; {e2}"));
                }
                return Err(e);
            }
        }
    }
    let kind = media_kind(&ext).ok_or("not a media file")?;
    if kind == MediaKind::Pro {
        let mut head = [0u8; 64];
        let n = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        let what = describe_pro(&head[..n]).unwrap_or("professional video clip");
        if let (Some(f), Some(proxy)) = (POSTER.get(), proxy_for(path)) {
            return f(&proxy);
        }
        return Err(format!("{what}: Windows has no codec for it. Put a proxy (same name, .mp4 or .mov) next to it, in a Proxy folder, to see and play it here; the original needs its maker's decoder"));
    }
    match POSTER.get() {
        Some(f) => f(path),
        None => Err("this program does not play video or sound (NuViewer does)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_pro_headers() {
        assert_eq!(media_kind("MP4"), Some(MediaKind::Video));
        assert_eq!(media_kind("flac"), Some(MediaKind::Audio));
        assert_eq!(media_kind("r3d"), Some(MediaKind::Pro));
        assert_eq!(media_kind("jpg"), None);
        for e in ["mp3", "ogg", "aiff", "au", "pcm", "flac", "alac", "ape", "opus", "aac", "dsf", "dff", "wma", "m4a", "mp4", "webm", "mov", "mkv", "avi", "mpeg", "mpg", "flv", "mts", "m2ts"] {
            assert!(media_kind(e).is_some(), "{e}");
        }
        assert!(needs_helper("OGG") && needs_helper("ape") && !needs_helper("mp3") && !needs_helper("mp4"));
        let mut r3d = vec![0, 0, 0, 0x20];
        r3d.extend_from_slice(b"RED2");
        assert_eq!(describe_pro(&r3d), Some("RED R3D clip"));
        assert_eq!(describe_pro(&[0x06, 0x0E, 0x2B, 0x34, 2, 5, 1, 1]), Some("MXF (SMPTE 377) clip"));
        assert_eq!(describe_pro(b"hello world"), None);
    }
}
