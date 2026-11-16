//! What a JPEG file says about itself without decoding it: size, number of components, EXIF orientation. Used to put a
//! JPEG into a PDF byte for byte when that is possible.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpegInfo {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    pub bits: u8,
    /// EXIF orientation 1..=8 (1 when there is none).
    pub orientation: u8,
}

pub fn jpeg_info(b: &[u8]) -> Option<JpegInfo> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let (mut p, mut orientation) = (2usize, 1u8);
    while p + 4 <= b.len() {
        if b[p] != 0xFF {
            p += 1;
            continue;
        }
        let m = b[p + 1];
        if m == 0xFF {
            p += 1;
            continue;
        }
        if (0xD0..=0xD9).contains(&m) || m == 0x01 {
            p += 2;
            continue;
        }
        let len = u16::from_be_bytes([b[p + 2], b[p + 3]]) as usize;
        let seg = b.get(p + 4..p + 2 + len)?;
        match m {
            0xE1 if seg.starts_with(b"Exif\0\0") => orientation = exif_orientation(&seg[6..]).unwrap_or(1),
            0xC0..=0xCF if !matches!(m, 0xC4 | 0xC8 | 0xCC) => {
                return Some(JpegInfo { bits: seg[0], height: u16::from_be_bytes([seg[1], seg[2]]) as u32, width: u16::from_be_bytes([seg[3], seg[4]]) as u32, components: seg[5], orientation });
            }
            0xDA => return None,
            _ => {}
        }
        p += 2 + len;
    }
    None
}

fn exif_orientation(t: &[u8]) -> Option<u8> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let s = t.get(o..o + 2)?;
        Some(if le { u16::from_le_bytes([s[0], s[1]]) } else { u16::from_be_bytes([s[0], s[1]]) })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let s = t.get(o..o + 4)?;
        Some(if le { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) })
    };
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    for i in 0..n.min(64) {
        let e = ifd + 2 + i * 12;
        if u16_at(e)? == 0x0112 {
            return u16_at(e + 8).filter(|o| (1..=8).contains(o)).map(|o| o as u8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_components_and_orientation() {
        // SOI, APP1 Exif (big endian, orientation 6), SOF0 16x8 RGB.
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE1];
        let tiff: Vec<u8> = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 0];
        let len = (2 + 6 + tiff.len()) as u16;
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(b"Exif\0\0");
        v.extend_from_slice(&tiff);
        v.extend_from_slice(&[0xFF, 0xC0, 0, 17, 8, 0, 8, 0, 16, 3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        let i = jpeg_info(&v).unwrap();
        assert_eq!((i.width, i.height, i.components, i.bits, i.orientation), (16, 8, 3, 8, 6));
        assert!(jpeg_info(b"not a jpeg").is_none());
    }
}
