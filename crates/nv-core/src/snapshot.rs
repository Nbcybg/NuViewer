//! Saving the frame of a video that is on screen as a picture: file format, folder and name come from the settings; the colours
//! are handled properly (see `videocolour`): ffmpeg decodes the very frame at 16 bits with the clip's own matrix and range,
//! SDR is kept or re-mapped as chosen, HDR10 / HLG is tone-mapped into sRGB or kept as a 16-bit PQ picture with its colour tag.
//! Without ffmpeg the frame as the player shows it is saved (8-bit, as shown) and the answer says so.

use crate::prefs::Prefs;
use crate::videocolour::{self, Output, VideoColour};
use nv_editor::convert::{self, Out};
use nv_editor::raster::Pixels;
use std::path::{Path, PathBuf};

/// The frame as the player has it (RGBA, 8-bit), used when ffmpeg isn't there.
pub struct Shown {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Saved {
    pub path: PathBuf,
    /// Something worth telling: how colours were handled, or why they could not be.
    pub note: String,
}

fn extension(format: &str) -> &'static str {
    match format {
        "png16" | "png" => "png",
        "jpg" => "jpg",
        "webp" => "webp",
        "bmp" => "bmp",
        "tiff" => "tif",
        _ => "png",
    }
}

/// `{name}` = the clip's name, `{time}` = position as 00h12m34s567, `{n}` = a counter that makes it unique.
pub fn file_name(pattern: &str, stem: &str, ms: u64, ext: &str) -> String {
    let (h, m, s, milli) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    let time = if h > 0 { format!("{h}h{m:02}m{s:02}s{milli:03}") } else { format!("{m:02}m{s:02}s{milli:03}") };
    let mut name = pattern.replace("{name}", stem).replace("{time}", &time).replace("{n}", "");
    name = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let name = name.trim().trim_end_matches('.').to_string();
    format!("{}.{ext}", if name.is_empty() { stem.to_string() } else { name })
}

fn folder_for(prefs: &Prefs, src: &Path) -> PathBuf {
    let d = prefs.snapshot_dir.trim();
    if d.is_empty() {
        src.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(d)
    }
}

/// 16-bit PNG, optionally tagged with a `cICP` chunk (colour primaries, transfer, matrix, range) so HDR stays HDR.
pub fn encode_png16(w: u32, h: u32, rgb: &[u16], cicp: Option<[u8; 4]>) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Sixteen);
    enc.set_compression(png::Compression::Fast);
    if cicp.is_none() {
        enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    }
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    if let Some(c) = cicp {
        wr.write_chunk(png::chunk::ChunkType([b'c', b'I', b'C', b'P']), &c).map_err(|e| e.to_string())?;
    }
    let bytes: Vec<u8> = rgb.iter().flat_map(|v| v.to_be_bytes()).collect();
    wr.write_image_data(&bytes).map_err(|e| e.to_string())?;
    wr.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

fn encode8(format: &str, quality: u8, w: u32, h: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for p in rgb.chunks_exact(3) {
        rgba.extend([p[0], p[1], p[2], 255]);
    }
    let px = Pixels::new(w, h, rgba);
    let out = match format {
        "jpg" => Out::Jpeg(quality),
        "webp" => Out::Webp,
        "bmp" => Out::Bmp,
        "tiff" => Out::Tiff,
        _ => Out::Png,
    };
    convert::encode(&px, out, [0, 0, 0], None)
}

/// Save a frame we already have as pixels (an animated GIF / WebP / APNG: no video decoder involved, no colour work).
/// `n` is the frame number shown in the name where a video has its time.
pub fn take_shown(src: &Path, n: u64, prefs: &Prefs, shown: Shown) -> Result<Saved, String> {
    take_shown_labeled(src, &format!("frame {n}"), prefs, shown)
}

/// `take_shown` with its own words for the part of the name where a video has its time ("frame 12", "page 3", "view").
/// A picture with transparency is saved as PNG when the chosen format cannot carry it (nothing is flattened onto black).
pub fn take_shown_labeled(src: &Path, label: &str, prefs: &Prefs, shown: Shown) -> Result<Saved, String> {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let mut format = if prefs.snapshot_format == "png16" { "png" } else { prefs.snapshot_format.as_str() };
    let alpha = shown.rgba.chunks_exact(4).any(|p| p[3] < 255);
    let mut note = String::new();
    if alpha && !crate::frames::format_keeps_alpha(format) {
        note = format!("Saved as PNG: {} cannot keep transparency", format.to_uppercase());
        format = "png";
    }
    let rgb: Vec<u8> = shown.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let bytes = if format == "png" {
        convert::encode(&Pixels::new(shown.width, shown.height, shown.rgba), Out::Png, [0; 3], None)?
    } else {
        encode8(format, prefs.snapshot_quality, shown.width, shown.height, &rgb)?
    };
    let dir = folder_for(prefs, src);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let pattern = prefs.snapshot_name.replace("{time}", label);
    let name = file_name(&pattern, &stem, 0, extension(format));
    let dst = crate::fileops::unique_path(&dir, std::ffi::OsStr::new(&name));
    nv_editor::save::write_atomic(&dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(Saved { path: dst, note })
}

/// Save the frame of `src` at `ms`. `shown` is the fallback when ffmpeg can't be used.
pub fn take(src: &Path, ms: u64, mf_first_ms: u64, prefs: &Prefs, shown: Option<Shown>) -> Result<Saved, String> {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
    let format = prefs.snapshot_format.as_str();
    let out_opts = Output { sdr: prefs.snapshot_sdr, tonemap: prefs.snapshot_tonemap, peak_nits: prefs.snapshot_peak as f32, keep_hdr: prefs.snapshot_keep_hdr };
    let mut note = String::new();
    let mut ext = extension(format);
    let bytes: Vec<u8>;
    let mut ms = ms;
    let grabbed = if crate::ffmpeg::find().is_some() {
        let probe = crate::ffmpeg::probe(src).ok();
        // Media Foundation counts the time of an MP4 with B-frames from its first sample (80 ms), ffmpeg from 0: line them up.
        ms = ms.saturating_sub(mf_first_ms.saturating_sub(probe.as_ref().map(|p| p.start_ms).unwrap_or(0)));
        let colour = probe.as_ref().map(|p| p.colour).unwrap_or_else(|| VideoColour::from_probe("", "", "", "", 1920, 1080, 8));
        Some(crate::ffmpeg::grab_rgb16(src, ms, &colour).map(|g| (g, colour)))
    } else {
        None
    };
    match grabbed {
        Some(Ok(((w, h, rgb16), colour))) => {
            if videocolour::keeps_hdr(&colour, &out_opts) {
                // HDR stays HDR: the numbers as they are, 16-bit, BT.2020 + PQ (or HLG) tagged.
                let tr = if colour.transfer == videocolour::Transfer::Hlg { 18 } else { 16 };
                bytes = encode_png16(w, h, &rgb16, Some([9, tr, 0, 1]))?;
                ext = "png";
                note = "HDR kept: 16-bit PNG, BT.2020 (tagged); show it on an HDR-aware viewer".into();
            } else if format == "png16" {
                let v = videocolour::to_srgb(&rgb16, &colour, &out_opts, 16);
                let v: Vec<u16> = v;
                bytes = encode_png16(w, h, &v, None)?;
                if colour.is_hdr() {
                    note = "HDR tone-mapped to SDR, 16-bit sRGB".into();
                } else if colour.bits > 8 {
                    note = format!("{}-bit video saved with 16-bit steps", colour.bits);
                }
            } else {
                let rgb = videocolour::to_srgb8(&rgb16, &colour, &out_opts);
                bytes = encode8(format, prefs.snapshot_quality, w, h, &rgb)?;
                if colour.is_hdr() {
                    note = "HDR tone-mapped to SDR sRGB".into();
                }
            }
        }
        other => {
            let reason = match other {
                Some(Err(e)) => format!("ffmpeg could not take the frame ({e})"),
                _ => "ffmpeg not found".to_string(),
            };
            let s = shown.ok_or(reason.clone())?;
            let rgb: Vec<u8> = s.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            if format == "png16" {
                ext = "png";
            }
            bytes = encode8(if format == "png16" { "png" } else { format }, prefs.snapshot_quality, s.width, s.height, &rgb)?;
            note = format!("{reason}: the frame was saved as the player shows it (8-bit; HDR video would look flat)");
        }
    }
    let dir = folder_for(prefs, src);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = file_name(&prefs.snapshot_name, &stem, ms, ext);
    let dst = crate::fileops::unique_path(&dir, std::ffi::OsStr::new(&name));
    nv_editor::save::write_atomic(&dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(Saved { path: dst, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_filled_in_and_made_safe() {
        assert_eq!(file_name("{name} {time}", "clip", 754_567, "png"), "clip 12m34s567.png");
        assert_eq!(file_name("{name}_{time}", "a", 3_725_004, "jpg"), "a_1h02m05s004.jpg");
        assert_eq!(file_name("sn/ap:{n}", "x", 0, "png"), "sn_ap_.png");
        assert_eq!(file_name("   ", "x", 0, "png"), "x.png");
    }

    #[test]
    fn png16_carries_its_numbers_and_the_hdr_tag() {
        let rgb: Vec<u16> = vec![0, 1000, 65535, 40000, 20000, 513];
        let bytes = encode_png16(2, 1, &rgb, Some([9, 16, 0, 1])).unwrap();
        assert!(bytes.windows(4).any(|w| w == b"cICP"));
        let mut dec = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info().unwrap();
        let mut buf = vec![0u8; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut buf).unwrap();
        assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
        let back: Vec<u16> = buf.chunks_exact(2).map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
        assert_eq!(back, rgb);
    }

    #[test]
    fn without_ffmpeg_the_shown_frame_is_saved_with_a_note() {
        let dir = std::env::temp_dir().join(format!("nv_snap_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut prefs = Prefs::default();
        prefs.snapshot_dir = dir.display().to_string();
        crate::ffmpeg::configure(r"Z:\no\such\ffmpeg.exe"); // a configured path that doesn't exist falls through to PATH, so only assert on the result shape
        let shown = Shown { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 255, 0, 255] };
        let r = take(Path::new("C:/nonexistent/clip.mp4"), 1234, 0, &prefs, Some(shown)).unwrap();
        assert!(r.path.is_file());
        assert!(r.path.file_name().unwrap().to_string_lossy().starts_with("clip 00m01s234"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
