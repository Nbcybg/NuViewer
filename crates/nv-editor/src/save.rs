//! Writing an edited picture back to disk: encoding, the EXIF block, a backup of the original, and an atomic replace.

use crate::raster::Pixels;
use std::io::Write;
use std::path::{Path, PathBuf};

/// JPEG quality used when a JPEG is rewritten (4:4:4, so colours stay as sharp as the encoder allows).
pub const JPEG_QUALITY: u8 = 95;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Jpeg,
    Png,
}

impl Format {
    /// The format a file of this extension can be rewritten in, if any.
    pub fn of_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jpe" | "jfif" => Some(Format::Jpeg),
            "png" => Some(Format::Png),
            _ => None,
        }
    }

    pub fn of_path(path: &Path) -> Option<Format> {
        path.extension().and_then(|e| e.to_str()).and_then(Self::of_extension)
    }
}

/// Where an edited picture goes: over the original, or (for formats we can't write) next to it as a PNG copy.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    Overwrite { path: PathBuf, format: Format },
    Copy { path: PathBuf },
}

impl Target {
    pub fn path(&self) -> &Path {
        match self {
            Target::Overwrite { path, .. } | Target::Copy { path } => path,
        }
    }
}

/// Decide where `original` is saved. `taken` says whether a path is already in use (so a copy never overwrites anything).
pub fn target_for(original: &Path, taken: impl Fn(&Path) -> bool) -> Target {
    if let Some(format) = Format::of_path(original) {
        return Target::Overwrite { path: original.to_path_buf(), format };
    }
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let dir = original.parent().unwrap_or(Path::new("."));
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}_edited.png") } else { format!("{stem}_edited ({n}).png") };
        let p = dir.join(name);
        if !taken(&p) {
            return Target::Copy { path: p };
        }
        n += 1;
    }
}

/// Encode as `format`. `exif` (the TIFF part of an EXIF block, see [`exif_of_jpeg`]) is embedded in JPEGs.
pub fn encode(p: &Pixels, format: Format, exif: Option<&[u8]>) -> Result<Vec<u8>, String> {
    match format {
        Format::Jpeg => {
            if p.w > 65_535 || p.h > 65_535 {
                return Err(format!("{}x{} is too large for a JPEG (65535 pixels a side)", p.w, p.h));
            }
            let mut out = Vec::with_capacity(p.rgba.len() / 8);
            let mut enc = jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY);
            enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
            if let Some(e) = exif {
                // Too big for one segment: better no metadata than no picture.
                let _ = enc.add_exif_metadata(e);
            }
            enc.encode(&p.rgba, p.w as u16, p.h as u16, jpeg_encoder::ColorType::Rgba).map_err(|e| format!("JPEG encoder: {e}"))?;
            Ok(out)
        }
        Format::Png => {
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, p.w, p.h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut w = enc.write_header().map_err(|e| format!("PNG encoder: {e}"))?;
            w.write_image_data(&p.rgba).map_err(|e| format!("PNG encoder: {e}"))?;
            w.finish().map_err(|e| format!("PNG encoder: {e}"))?;
            Ok(out)
        }
    }
}

/// The EXIF block of a JPEG (the bytes after "Exif\0\0"), with the orientation set to "upright" and the pixel size
/// replaced by `new_size`, ready to go into the rewritten file. None if the file has no EXIF.
pub fn exif_of_jpeg(jpeg: &[u8], new_size: (u32, u32)) -> Option<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
        let marker = jpeg[i + 1];
        if marker == 0xDA || marker == 0xD9 {
            break; // image data: no more headers
        }
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if len < 2 || i + 2 + len > jpeg.len() {
            break;
        }
        let body = &jpeg[i + 4..i + 2 + len];
        if marker == 0xE1 && body.starts_with(b"Exif\0\0") {
            let mut tiff = body[6..].to_vec();
            patch_exif(&mut tiff, new_size);
            return Some(tiff);
        }
        i += 2 + len;
    }
    None
}

/// Set Orientation (0x0112) to 1 and PixelXDimension / PixelYDimension (0xA002 / 0xA003) to `size`, in place.
/// Anything unexpected leaves the block as it is.
pub fn patch_exif(tiff: &mut [u8], size: (u32, u32)) {
    if tiff.len() < 8 {
        return;
    }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return,
    };
    let rd16 = |b: &[u8], o: usize| -> Option<u16> { b.get(o..o + 2).map(|s| if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) }) };
    let rd32 = |b: &[u8], o: usize| -> Option<u32> { b.get(o..o + 4).map(|s| if le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) }) };
    if rd16(tiff, 2) != Some(42) {
        return;
    }
    // Collect (offset of the value field, type) for what we change, then write.
    let mut edits: Vec<(usize, u16, u32)> = Vec::new();
    let scan = |ifd: usize, edits: &mut Vec<(usize, u16, u32)>, tiff: &[u8]| -> Option<usize> {
        let n = rd16(tiff, ifd)? as usize;
        let mut exif_ifd = None;
        for k in 0..n {
            let e = ifd + 2 + k * 12;
            let tag = rd16(tiff, e)?;
            let ty = rd16(tiff, e + 2)?;
            let count = rd32(tiff, e + 4)?;
            match tag {
                0x0112 if count == 1 && ty == 3 => edits.push((e + 8, ty, 1)),
                0xA002 if count == 1 && (ty == 3 || ty == 4) => edits.push((e + 8, ty, size.0)),
                0xA003 if count == 1 && (ty == 3 || ty == 4) => edits.push((e + 8, ty, size.1)),
                0x8769 => exif_ifd = rd32(tiff, e + 8).map(|o| o as usize),
                _ => {}
            }
        }
        exif_ifd
    };
    let Some(ifd0) = rd32(tiff, 4).map(|o| o as usize) else { return };
    let sub = scan(ifd0, &mut edits, tiff);
    if let Some(sub) = sub {
        scan(sub, &mut edits, tiff);
    }
    for (at, ty, value) in edits {
        if ty == 3 {
            let bytes = if le { (value.min(65_535) as u16).to_le_bytes() } else { (value.min(65_535) as u16).to_be_bytes() };
            if let Some(dst) = tiff.get_mut(at..at + 2) {
                dst.copy_from_slice(&bytes);
            }
        } else if let Some(dst) = tiff.get_mut(at..at + 4) {
            dst.copy_from_slice(&if le { value.to_le_bytes() } else { value.to_be_bytes() });
        }
    }
}

/// Copy `src` into `dir` (created if needed) under a name that can't clash; returns the copy's path. Used to keep the
/// original of a file about to be rewritten, so the edit can be undone.
pub fn backup_copy(src: &Path, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let name = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let dst = dir.join(format!("{stamp:x}_{name}"));
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

/// Remove backups in `dir` older than `max_age`. Failures are ignored: this is housekeeping.
pub fn prune_backups(dir: &Path, max_age: std::time::Duration) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let now = std::time::SystemTime::now();
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| now.duration_since(t).ok()).map(|age| age > max_age).unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Write `bytes` to `path` without ever leaving a half-written file: a temporary file next to it, then a rename over it.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let tmp = dir.join(format!(".{name}.nuviewer-tmp"));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_editor_{tag}_{}_{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A tiny little-endian TIFF: IFD0 { Orientation = 6, ExifIFD -> { PixelXDimension = 40 (SHORT), PixelYDimension = 30 (LONG) } }.
    fn sample_exif(be: bool) -> Vec<u8> {
        let w16 = |v: u16| if be { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
        let w32 = |v: u32| if be { v.to_be_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
        let mut t = Vec::new();
        t.extend_from_slice(if be { b"MM" } else { b"II" });
        t.extend(w16(42));
        t.extend(w32(8));
        // IFD0 at 8: 2 entries
        t.extend(w16(2));
        t.extend(w16(0x0112));
        t.extend(w16(3));
        t.extend(w32(1));
        t.extend(w16(6));
        t.extend(w16(0));
        t.extend(w16(0x8769));
        t.extend(w16(4));
        t.extend(w32(1));
        t.extend(w32(8 + 2 + 24 + 4)); // exif IFD right after IFD0 (+ its next-IFD pointer)
        t.extend(w32(0));
        // Exif IFD: 2 entries
        t.extend(w16(2));
        t.extend(w16(0xA002));
        t.extend(w16(3));
        t.extend(w32(1));
        t.extend(w16(40));
        t.extend(w16(0));
        t.extend(w16(0xA003));
        t.extend(w16(4));
        t.extend(w32(1));
        t.extend(w32(30));
        t.extend(w32(0));
        t
    }

    fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        let len = (2 + 6 + tiff.len()) as u16;
        j.extend_from_slice(&[0xFF, 0xE1]);
        j.extend_from_slice(&len.to_be_bytes());
        j.extend_from_slice(b"Exif\0\0");
        j.extend_from_slice(tiff);
        j.extend_from_slice(&[0xFF, 0xD9]);
        j
    }

    #[test]
    fn exif_orientation_and_size_are_rewritten_in_both_byte_orders() {
        for be in [false, true] {
            let jpeg = jpeg_with_exif(&sample_exif(be));
            let out = exif_of_jpeg(&jpeg, (1234, 567)).expect("exif found");
            let rd16 = |o: usize| if be { u16::from_be_bytes([out[o], out[o + 1]]) } else { u16::from_le_bytes([out[o], out[o + 1]]) };
            let rd32 = |o: usize| if be { u32::from_be_bytes([out[o], out[o + 1], out[o + 2], out[o + 3]]) } else { u32::from_le_bytes([out[o], out[o + 1], out[o + 2], out[o + 3]]) };
            assert_eq!(rd16(8 + 2 + 8), 1, "orientation is now upright (be={be})");
            let exif_ifd = 8 + 2 + 24 + 4;
            assert_eq!(rd16(exif_ifd + 2 + 8), 1234, "PixelXDimension (SHORT) is clamped to 16 bits and replaced");
            assert_eq!(rd32(exif_ifd + 2 + 12 + 8), 567, "PixelYDimension (LONG)");
        }
        assert!(exif_of_jpeg(&[0xFF, 0xD8, 0xFF, 0xD9], (1, 1)).is_none());
        assert!(exif_of_jpeg(b"not a jpeg", (1, 1)).is_none());
    }

    #[test]
    fn garbage_exif_is_left_alone() {
        let mut junk = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9];
        let before = junk.clone();
        patch_exif(&mut junk, (10, 10));
        assert_eq!(junk, before);
        let mut cut = sample_exif(false);
        cut.truncate(20);
        patch_exif(&mut cut, (10, 10)); // must not panic
    }

    #[test]
    fn jpeg_and_png_round_trip_through_the_encoders() {
        let (w, h) = (24u32, 16u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                rgba.extend_from_slice(&[(x * 10) as u8, (y * 15) as u8, 90, 255]);
            }
        }
        let p = Pixels::new(w, h, rgba);
        let png_bytes = encode(&p, Format::Png, None).unwrap();
        let dec = png::Decoder::new(std::io::Cursor::new(&png_bytes));
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (w, h));
        assert_eq!(&buf[..(w * h * 4) as usize], &p.rgba[..], "PNG is lossless");

        let exif = sample_exif(false);
        let jpg = encode(&p, Format::Jpeg, Some(&exif)).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
        assert!(jpg.windows(6).any(|w| w == b"Exif\0\0"), "the EXIF block was embedded");
        assert!(exif_of_jpeg(&jpg, (w, h)).is_some());
    }

    #[test]
    fn targets_overwrite_known_formats_and_copy_the_rest() {
        let t = target_for(Path::new("C:/pics/a.JPG"), |_| false);
        assert_eq!(t, Target::Overwrite { path: PathBuf::from("C:/pics/a.JPG"), format: Format::Jpeg });
        let t = target_for(Path::new("C:/pics/b.webp"), |_| false);
        assert_eq!(t, Target::Copy { path: PathBuf::from("C:/pics/b_edited.png") });
        let t = target_for(Path::new("C:/pics/b.webp"), |p| p.ends_with("b_edited.png"));
        assert_eq!(t, Target::Copy { path: PathBuf::from("C:/pics/b_edited (2).png") });
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp_file() {
        let d = temp_dir("atomic");
        let f = d.join("x.png");
        std::fs::write(&f, b"old").unwrap();
        write_atomic(&f, b"new contents").unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"new contents");
        let names: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["x.png".to_string()]);
        let b = backup_copy(&f, &d.join("bak")).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"new contents");
        prune_backups(&d.join("bak"), std::time::Duration::from_secs(3600));
        assert!(b.exists(), "fresh backups stay");
        prune_backups(&d.join("bak"), std::time::Duration::ZERO);
        let _ = std::fs::remove_dir_all(&d);
    }
}
