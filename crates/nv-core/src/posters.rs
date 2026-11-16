//! The still picture of a video or a sound (a frame; the waveform), made with Media Foundation through `nv-media` and
//! handed to the decoder. A program that shows media (viewer, gallery) calls `register()` once at start; Media Foundation
//! itself is only loaded when the first video is looked at.

use nv_codecs::Image;
use std::path::Path;

fn poster(path: &Path) -> Result<Image, String> {
    let why = match nv_media::poster_frame(path) {
        Ok(f) => {
            let mut rgba = f.bgra;
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            return Ok(Image::new(f.width, f.height, rgba));
        }
        Err(e) => e,
    };
    // A video track Windows has no decoder for: ffmpeg, which seeks straight to the picture (the sound track of a big camera file
    // would mean reading all of it).
    if !why.contains("no picture in this file") {
        if let Ok(img) = crate::ffmpeg::poster(path) {
            return Ok(img);
        }
        // No ffmpeg: the waveform of its sound, unless the file is so big that drawing it means reading it all.
        let big = std::fs::metadata(path).map(|m| m.len() > 256 * 1024 * 1024).unwrap_or(false);
        if !big {
            if let Ok(peaks) = nv_media::audio_peaks(path, nv_codecs::wave::WIDTH) {
                return Ok(nv_codecs::wave::draw(&peaks));
            }
        }
        return Err(format!("{why}. {}", crate::ffmpeg::poster(path).err().unwrap_or_default()));
    }
    // No picture in it: it is sound.
    match nv_media::audio_peaks(path, nv_codecs::wave::WIDTH) {
        Ok(peaks) => Ok(nv_codecs::wave::draw(&peaks)),
        Err(e) => crate::ffmpeg::poster(path).map_err(|_| e), // a sound format Windows can't decode, with cover art or a picture track
    }
}

pub fn register() {
    nv_codecs::media::set_poster(poster);
}
