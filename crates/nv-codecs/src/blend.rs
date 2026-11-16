//! Blender `.blend` files: the preview picture Blender stores in the file (the one Explorer's thumbnails show). The
//! scene itself is not read (a .blend is a dump of Blender's memory, only Blender can make sense of all of it): export to
//! glTF / OBJ / STL / FBX / VRM from Blender to see the model in 3D.
//! Plain, gzip (Blender 2.x) and zstd (3.0+) files all work; only the first blocks are decompressed.

use crate::Image;
use std::io::Read;
use std::path::Path;

/// The first `max` bytes of the file's content, decompressed when it is compressed.
fn head(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).map_err(|e| e.to_string())?;
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(max.min(1 << 20));
    if magic[0] == 0x1F && magic[1] == 0x8B {
        flate2::read::GzDecoder::new(f).take(max as u64).read_to_end(&mut out).map_err(|e| e.to_string())?;
    } else if magic == [0x28, 0xB5, 0x2F, 0xFD] {
        let mut d = ruzstd::decoding::StreamingDecoder::new(f).map_err(|e| format!("zstd: {e}"))?;
        (&mut d).take(max as u64).read_to_end(&mut out).map_err(|e| e.to_string())?;
    } else {
        (&mut f).take(max as u64).read_to_end(&mut out).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Header facts: (pointer size, little endian, version text, block header size).
struct Header {
    ptr: usize,
    le: bool,
    version: String,
    /// Offset of the first block.
    start: usize,
    large: bool,
}

fn parse_header(b: &[u8]) -> Result<Header, String> {
    if b.len() < 12 || &b[0..7] != b"BLENDER" {
        return Err("not a Blender file".into());
    }
    // Blender 5: "BLENDER17-01v0500" (header size, format version, endian, version).
    if b[7].is_ascii_digit() && b[8].is_ascii_digit() && b.len() >= 17 {
        let size: usize = std::str::from_utf8(&b[7..9]).ok().and_then(|s| s.parse().ok()).unwrap_or(17);
        return Ok(Header { ptr: 8, le: b[12] == b'v', version: String::from_utf8_lossy(&b[13..17]).into_owned(), start: size, large: true });
    }
    let ptr = match b[7] {
        b'_' => 4,
        b'-' => 8,
        _ => return Err("damaged Blender header".into()),
    };
    let le = b[8] == b'v';
    let v = String::from_utf8_lossy(&b[9..12]).into_owned();
    Ok(Header { ptr, le, version: format!("{}.{}", &v[..1], &v[1..]), start: 12, large: false })
}

/// The picture stored in the file, upright.
pub fn decode(path: &Path) -> Result<Image, String> {
    let b = head(path, 3 * 1024 * 1024)?;
    let h = parse_header(&b)?;
    let rd = |o: usize, n: usize| -> Option<u64> {
        let s = b.get(o..o + n)?;
        let mut v = 0u64;
        for (i, &x) in s.iter().enumerate() {
            let sh = if h.le { i } else { n - 1 - i } * 8;
            v |= (x as u64) << sh;
        }
        Some(v)
    };
    let mut pos = h.start;
    for _ in 0..16 {
        let code = b.get(pos..pos + 4).ok_or("this .blend is cut short")?;
        let (len, head_len) = if h.large {
            (rd(pos + 16, 8).ok_or("cut short")? as usize, 32)
        } else {
            (rd(pos + 4, 4).ok_or("cut short")? as usize, 16 + h.ptr)
        };
        if code == b"TEST" {
            let d = pos + head_len;
            let (w, hh) = (rd(d, 4).ok_or("cut short")? as usize, rd(d + 4, 4).ok_or("cut short")? as usize);
            if w == 0 || hh == 0 || w > 4096 || hh > 4096 || len < 8 + w * hh * 4 {
                return Err("the preview in this .blend is damaged".into());
            }
            let px = b.get(d + 8..d + 8 + w * hh * 4).ok_or("the preview in this .blend is cut short")?;
            // Stored bottom row first.
            let mut rgba = Vec::with_capacity(w * hh * 4);
            for y in (0..hh).rev() {
                rgba.extend_from_slice(&px[y * w * 4..(y + 1) * w * 4]);
            }
            return Image::from_frames(w as u32, hh as u32, vec![(rgba, 0)]).ok_or_else(|| "empty preview".to_string());
        }
        if code == b"DNA1" || code == b"ENDB" {
            break;
        }
        pos += head_len + len;
    }
    Err(format!("Blender {} saved this file without a preview picture; export the scene (glTF, OBJ, STL, FBX or VRM) to see it in 3D", h.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(le_ptr8: bool) -> Vec<u8> {
        let mut v = b"BLENDER".to_vec();
        v.push(if le_ptr8 { b'-' } else { b'_' });
        v.extend_from_slice(b"v293");
        let (w, h) = (2u32, 2u32);
        let mut data = Vec::new();
        data.extend_from_slice(&w.to_le_bytes());
        data.extend_from_slice(&h.to_le_bytes());
        // bottom row red, top row blue
        for c in [[255u8, 0, 0, 255], [255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]] {
            data.extend_from_slice(&c);
        }
        v.extend_from_slice(b"TEST");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend(std::iter::repeat(0u8).take(if le_ptr8 { 8 } else { 4 }));
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&data);
        v
    }

    #[test]
    fn reads_the_preview_of_both_pointer_sizes() {
        let dir = std::env::temp_dir().join("nv_blend_test");
        std::fs::create_dir_all(&dir).unwrap();
        for p8 in [true, false] {
            let f = dir.join(format!("t{p8}.blend"));
            std::fs::write(&f, synthetic(p8)).unwrap();
            let img = decode(&f).unwrap();
            assert_eq!((img.width, img.height), (2, 2));
            assert_eq!(&img.rgba[0..4], &[0, 0, 255, 255], "top row is blue after the flip");
        }
        let none = dir.join("none.blend");
        std::fs::write(&none, b"BLENDER-v300ENDB\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0").unwrap();
        assert!(decode(&none).err().unwrap().contains("without a preview"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
