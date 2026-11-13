//! Picture size from the first bytes of a file, without decoding it. Read mode uses it to tell a wide double-page
//! scan from a single page for every file of a book, which would be far too slow if each page were decoded.

use std::io::Read;
use std::path::Path;

/// How much of the file is read: JPEG keeps its size after the EXIF and ICC blocks, which can be large.
const HEAD: usize = 256 * 1024;

/// (width, height) in stored pixels (EXIF orientation not applied) for JPEG, PNG, GIF, WebP and BMP; `None` for anything
/// else or when the header cannot be read.
pub fn peek_size(path: &Path) -> Option<(u32, u32)> {
    let mut head = Vec::with_capacity(HEAD);
    std::fs::File::open(path).ok()?.take(HEAD as u64).read_to_end(&mut head).ok()?;
    size_from_header(&head)
}

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as u32)
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]) as u32)
}

fn le24(b: &[u8], i: usize) -> Option<u32> {
    Some(*b.get(i)? as u32 | (*b.get(i + 1)? as u32) << 8 | (*b.get(i + 2)? as u32) << 16)
}

fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(le16(b, i)? | le16(b, i + 2)? << 16)
}

/// The size a file's first bytes announce (see `peek_size`).
pub fn size_from_header(b: &[u8]) -> Option<(u32, u32)> {
    let size = if b.starts_with(&[0xFF, 0xD8]) {
        jpeg_size(b)?
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) && b.get(12..16) == Some(b"IHDR") {
        (be32(b, 16)?, be32(b, 20)?)
    } else if b.starts_with(b"GIF8") {
        (le16(b, 6)?, le16(b, 8)?)
    } else if b.len() >= 16 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        webp_size(b)?
    } else if b.starts_with(b"BM") {
        (le32(b, 18)?, (le32(b, 22)? as i32).unsigned_abs())
    } else {
        return None;
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        match m {
            0xFF => i += 1, // fill byte
            0x01 | 0xD0..=0xD8 => i += 2, // markers without a length
            0xD9 | 0xDA => return None,   // end of image / start of scan: no frame header came first
            // Start-of-frame markers (not DHT, JPG, DAC).
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => return Some((be16(b, i + 7)?, be16(b, i + 5)?)),
            _ => i += 2 + be16(b, i + 2)? as usize,
        }
    }
    None
}

fn webp_size(b: &[u8]) -> Option<(u32, u32)> {
    match b.get(12..16)? {
        b"VP8 " if b.get(23..26) == Some(&[0x9D, 0x01, 0x2A]) => Some((le16(b, 26)? & 0x3FFF, le16(b, 28)? & 0x3FFF)),
        b"VP8L" if *b.get(20)? == 0x2F => {
            let (b1, b2, b3, b4) = (*b.get(21)? as u32, *b.get(22)? as u32, *b.get(23)? as u32, *b.get(24)? as u32);
            Some((1 + (b1 | (b2 & 0x3F) << 8), 1 + ((b2 >> 6) | b3 << 2 | (b4 & 0x0F) << 10)))
        }
        b"VP8X" => Some((1 + le24(b, 24)?, 1 + le24(b, 27)?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_size_comes_after_other_segments() {
        let mut j = vec![0xFF, 0xD8];
        j.extend([0xFF, 0xE1, 0x00, 0x0A, 1, 2, 3, 4, 5, 6, 7, 8]); // an APP1 block of 8 bytes
        j.extend([0xFF, 0xC4, 0x00, 0x04, 0, 0]); // DHT must not be mistaken for a frame header
        j.extend([0xFF, 0xC0, 0x00, 0x11, 8, 0x03, 0x00, 0x04, 0x00, 3]); // 768 high, 1024 wide
        assert_eq!(size_from_header(&j), Some((1024, 768)));
    }

    #[test]
    fn jpeg_without_a_frame_header_is_unknown() {
        assert_eq!(size_from_header(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), None);
        assert_eq!(size_from_header(&[0xFF, 0xD8]), None);
    }

    #[test]
    fn png_gif_bmp_sizes() {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        p.extend(640u32.to_be_bytes());
        p.extend(480u32.to_be_bytes());
        assert_eq!(size_from_header(&p), Some((640, 480)));
        assert_eq!(size_from_header(b"GIF89a\x40\x01\xF0\x00"), Some((320, 240)));
        let mut bmp = vec![0u8; 26];
        bmp[0] = b'B';
        bmp[1] = b'M';
        bmp[18..22].copy_from_slice(&100u32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-50i32).to_le_bytes()); // top-down bitmaps store a negative height
        assert_eq!(size_from_header(&bmp), Some((100, 50)));
    }

    #[test]
    fn webp_sizes() {
        let mut x = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        x.extend([10, 0, 0, 0, 0, 0, 0, 0]); // chunk size + flags + reserved
        x.extend([0x1F, 0x03, 0x00]); // width - 1 = 799
        x.extend([0x57, 0x02, 0x00]); // height - 1 = 599
        assert_eq!(size_from_header(&x), Some((800, 600)));
        let mut l = b"RIFF\0\0\0\0WEBPVP8L".to_vec();
        l.extend([5, 0, 0, 0, 0x2F]);
        let (w, h) = (300u32 - 1, 200u32 - 1);
        let bits = w | h << 14;
        l.extend(bits.to_le_bytes());
        assert_eq!(size_from_header(&l), Some((300, 200)));
    }

    #[test]
    fn anything_else_is_unknown() {
        assert_eq!(size_from_header(b"not a picture at all, just text"), None);
        assert_eq!(size_from_header(&[]), None);
        assert_eq!(peek_size(Path::new("Z:/definitely/not/here.jpg")), None);
    }
}
