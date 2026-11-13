//! Photoshop's merged ("flattened") picture read straight from the file: PSD and, above all, **PSB** (the "large document"
//! format, version 2 of the same layout, which the `psd` crate refuses). Only the final composite is needed, so the layers
//! are skipped by length and the image-data section is decoded: raw, RLE (PackBits) or ZIP, 8 or 16 bits per channel,
//! in bitmap, greyscale, indexed, RGB, CMYK and Lab modes.
//!
//! Layout (big-endian): header 26 bytes (`8BPS`, version 1|2, 6 reserved, channels, height, width, depth, mode), colour
//! mode data (length u32), image resources (length u32), layer and mask info (length u32 for PSD, **u64 for PSB**), then
//! the image data: compression u16, and the channels one after the other, plane by plane.

use crate::Image;
use std::io::Read;

struct Header {
    psb: bool,
    channels: usize,
    height: usize,
    width: usize,
    depth: usize,
    mode: u16,
}

fn be16(b: &[u8], i: usize) -> Result<u16, String> {
    b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).ok_or_else(truncated)
}

fn be32(b: &[u8], i: usize) -> Result<u32, String> {
    b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(truncated)
}

fn be64(b: &[u8], i: usize) -> Result<u64, String> {
    b.get(i..i + 8).map(|s| u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])).ok_or_else(truncated)
}

fn truncated() -> String {
    "psd: unexpected end of file".to_string()
}

fn parse_header(b: &[u8]) -> Result<Header, String> {
    if b.len() < 26 || &b[0..4] != b"8BPS" {
        return Err("psd: not a Photoshop file (bad signature)".into());
    }
    let version = be16(b, 4)?;
    if version != 1 && version != 2 {
        return Err(format!("psd: unsupported version {version}"));
    }
    let (channels, height, width, depth, mode) = (be16(b, 12)? as usize, be32(b, 14)? as usize, be32(b, 18)? as usize, be16(b, 22)? as usize, be16(b, 24)?);
    let max = if version == 2 { 300_000 } else { 30_000 };
    if width == 0 || height == 0 || width > max || height > max {
        return Err(format!("psd: invalid size {width}x{height}"));
    }
    crate::check_size(width as u64, height as u64)?;
    if !(1..=56).contains(&channels) {
        return Err(format!("psd: invalid channel count {channels}"));
    }
    Ok(Header { psb: version == 2, channels, height, width, depth, mode })
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let h = parse_header(bytes)?;
    if !matches!(h.depth, 1 | 8 | 16) {
        return Err(format!("psd: unsupported bit depth {} (8 and 16 bits per channel are supported)", h.depth));
    }
    if h.depth == 1 && h.mode != 0 {
        return Err("psd: unsupported bit depth 1 outside bitmap mode".into());
    }
    // Colour mode data.
    let mut at = 26;
    let cm_len = be32(bytes, at)? as usize;
    let cm_data = bytes.get(at + 4..at + 4 + cm_len).ok_or_else(truncated)?;
    at += 4 + cm_len;
    // Image resources, then layers and masks: skipped.
    let res_len = be32(bytes, at)? as usize;
    at += 4 + res_len;
    let layer_len = if h.psb { be64(bytes, at)? as usize } else { be32(bytes, at)? as usize };
    at += if h.psb { 8 } else { 4 };
    at = at.checked_add(layer_len).ok_or_else(truncated)?;
    let compression = be16(bytes, at)?;
    at += 2;
    let data = bytes.get(at..).ok_or_else(truncated)?;

    let (w, hgt, ch) = (h.width, h.height, h.channels);
    let row_bytes = if h.depth == 1 { w.div_ceil(8) } else { w * h.depth / 8 };
    let plane = row_bytes * hgt;
    let total = plane.checked_mul(ch).filter(|t| *t <= 3_000_000_000).ok_or("psd: image too large to open")?;
    crate::progress::decoding();
    let planes: Vec<u8> = match compression {
        0 => data.get(..total).ok_or_else(truncated)?.to_vec(),
        1 => unpack_rle(data, &h, row_bytes)?,
        2 | 3 => {
            let mut out = Vec::with_capacity(total);
            flate2::read::ZlibDecoder::new(data).take(total as u64).read_to_end(&mut out).map_err(|e| format!("psd: zip data is damaged ({e})"))?;
            if out.len() < total {
                return Err(truncated());
            }
            if compression == 3 {
                undo_prediction(&mut out, row_bytes, h.depth, hgt * ch);
            }
            out
        }
        c => return Err(format!("psd: unsupported compression {c}")),
    };
    to_rgba(&h, &planes, plane, row_bytes, cm_data)
}

/// PackBits rows: all the byte counts first (u16 each in PSD, u32 in PSB), then the data of every scanline of every channel.
fn unpack_rle(data: &[u8], h: &Header, row_bytes: usize) -> Result<Vec<u8>, String> {
    let rows = h.height * h.channels;
    let count_size = if h.psb { 4 } else { 2 };
    let mut pos = rows * count_size;
    let mut out = vec![0u8; row_bytes * rows];
    for r in 0..rows {
        let len = if h.psb { be32(data, r * 4)? as usize } else { be16(data, r * 2)? as usize };
        let src = data.get(pos..pos + len).ok_or_else(truncated)?;
        pos += len;
        let dst = &mut out[r * row_bytes..(r + 1) * row_bytes];
        let (mut i, mut o) = (0, 0);
        while i < src.len() && o < row_bytes {
            let n = src[i] as i8;
            i += 1;
            if n >= 0 {
                let run = n as usize + 1;
                let take = run.min(src.len() - i).min(row_bytes - o);
                dst[o..o + take].copy_from_slice(&src[i..i + take]);
                i += run;
                o += take;
            } else if n != -128 {
                let run = 1 - n as isize;
                let Some(&v) = src.get(i) else { break };
                i += 1;
                let take = (run as usize).min(row_bytes - o);
                dst[o..o + take].fill(v);
                o += take;
            }
        }
    }
    Ok(out)
}

/// ZIP-with-prediction stores each row as differences from the previous sample (bytes, or 16-bit words).
fn undo_prediction(buf: &mut [u8], row_bytes: usize, depth: usize, rows: usize) {
    for r in 0..rows {
        let row = &mut buf[r * row_bytes..(r + 1) * row_bytes];
        if depth == 16 {
            for i in (2..row.len().saturating_sub(1)).step_by(2) {
                let prev = u16::from_be_bytes([row[i - 2], row[i - 1]]);
                let cur = u16::from_be_bytes([row[i], row[i + 1]]).wrapping_add(prev);
                row[i..i + 2].copy_from_slice(&cur.to_be_bytes());
            }
        } else {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
        }
    }
}

/// One sample of channel `c` at pixel (x, y) as 0..=255 (16-bit samples keep their high byte).
struct Planes<'a> {
    data: &'a [u8],
    plane: usize,
    row_bytes: usize,
    depth: usize,
    width: usize,
}

impl Planes<'_> {
    fn at(&self, c: usize, x: usize, y: usize) -> u8 {
        let base = c * self.plane + y * self.row_bytes;
        match self.depth {
            16 => self.data[base + x * 2],
            1 => {
                // Bitmap: 1 = black.
                let bit = (self.data[base + x / 8] >> (7 - (x % 8))) & 1;
                if bit == 1 { 0 } else { 255 }
            }
            _ => self.data[base + x],
        }
    }
}

fn to_rgba(h: &Header, data: &[u8], plane: usize, row_bytes: usize, color_data: &[u8]) -> Result<Image, String> {
    let (w, hgt) = (h.width, h.height);
    let p = Planes { data, plane, row_bytes, depth: h.depth, width: w };
    let _ = p.width;
    let ch = h.channels;
    let mut rgba = vec![255u8; w * hgt * 4];
    // Which channel is the transparency, if the merged picture has one (the first channel after the colour ones).
    let (colours, needs): (usize, usize) = match h.mode {
        0 | 1 | 2 | 8 | 7 => (1, 1),
        3 | 9 => (3, 3),
        4 => (4, 4),
        m => return Err(format!("psd: unsupported colour mode {m}")),
    };
    if ch < needs {
        return Err(format!("psd: mode {} needs {needs} channels, the file has {ch}", h.mode));
    }
    let has_alpha = ch > colours && matches!(h.mode, 1 | 3 | 4) && h.depth != 1;
    for y in 0..hgt {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let px: [u8; 3] = match h.mode {
                0 | 1 | 8 | 7 => {
                    let g = p.at(0, x, y);
                    [g, g, g]
                }
                2 => {
                    // Indexed: 768 bytes of palette, all reds, then all greens, then all blues.
                    let i = p.at(0, x, y) as usize;
                    let get = |k: usize| color_data.get(k * 256 + i).copied().unwrap_or(i as u8);
                    [get(0), get(1), get(2)]
                }
                3 => [p.at(0, x, y), p.at(1, x, y), p.at(2, x, y)],
                4 => {
                    // Photoshop keeps CMYK inverted: 255 means no ink.
                    let (c, m, yy, k) = (p.at(0, x, y) as u32, p.at(1, x, y) as u32, p.at(2, x, y) as u32, p.at(3, x, y) as u32);
                    [(c * k / 255) as u8, (m * k / 255) as u8, (yy * k / 255) as u8]
                }
                _ => lab_to_rgb(p.at(0, x, y), p.at(1, x, y), p.at(2, x, y)),
            };
            rgba[o..o + 3].copy_from_slice(&px);
            if has_alpha {
                rgba[o + 3] = p.at(colours, x, y);
            }
        }
    }
    let mut img = Image::new(w as u32, hgt as u32, rgba);
    img.profile_assumable = true;
    Ok(img)
}

/// Photoshop's 8-bit Lab (L 0..255 = 0..100, a and b offset by 128) to sRGB, D50 white.
fn lab_to_rgb(l8: u8, a8: u8, b8: u8) -> [u8; 3] {
    let l = l8 as f32 / 255.0 * 100.0;
    let (a, b) = (a8 as f32 - 128.0, b8 as f32 - 128.0);
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    let inv = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    // D50 reference white.
    let (x, y, z) = (0.9642 * inv(fx), inv(fy), 0.8249 * inv(fz));
    // XYZ (D50) to linear sRGB (Bradford-adapted matrix).
    let r = 3.1339 * x - 1.6169 * y - 0.4906 * z;
    let g = -0.9788 * x + 1.9161 * y + 0.0335 * z;
    let bl = 0.0719 * x - 0.2290 * y + 1.4052 * z;
    let enc = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u8
    };
    [enc(r), enc(g), enc(bl)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PSD/PSB: `version` 1 or 2, RGB 8-bit, `w` x `h`, the given channel planes, raw or RLE.
    fn make(version: u16, w: u32, h: u32, channels: u16, mode: u16, depth: u16, compression: u16, image: &[u8]) -> Vec<u8> {
        let mut v = b"8BPS".to_vec();
        v.extend(version.to_be_bytes());
        v.extend([0u8; 6]);
        v.extend(channels.to_be_bytes());
        v.extend(h.to_be_bytes());
        v.extend(w.to_be_bytes());
        v.extend(depth.to_be_bytes());
        v.extend(mode.to_be_bytes());
        v.extend(0u32.to_be_bytes()); // colour mode data
        v.extend(0u32.to_be_bytes()); // image resources
        if version == 2 {
            v.extend(0u64.to_be_bytes()); // layer and mask info: 8-byte length in PSB
        } else {
            v.extend(0u32.to_be_bytes());
        }
        v.extend(compression.to_be_bytes());
        v.extend(image);
        v
    }

    #[test]
    fn psb_raw_rgb_decodes() {
        // 2x1: pixel 0 red, pixel 1 blue. Planes: R = [255, 0], G = [0, 0], B = [0, 255].
        let psb = make(2, 2, 1, 3, 3, 8, 0, &[255, 0, 0, 0, 0, 255]);
        let img = decode(&psb).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn psd_version_one_decodes_too() {
        let psd = make(1, 1, 1, 3, 3, 8, 0, &[10, 20, 30]);
        assert_eq!(&decode(&psd).unwrap().rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn psb_rle_uses_four_byte_row_counts() {
        // 4x1 grey, one channel: counts (u32) = 2, then PackBits "repeat 4 times 77": [0xFD, 77].
        let mut image = 2u32.to_be_bytes().to_vec();
        image.extend([0xFD, 77]);
        let img = decode(&make(2, 4, 1, 1, 1, 8, 1, &image)).unwrap();
        assert!(img.rgba.chunks_exact(4).all(|p| p == [77, 77, 77, 255]));
        // The same in a PSD uses two-byte counts.
        let mut image = 2u16.to_be_bytes().to_vec();
        image.extend([0xFD, 77]);
        let img = decode(&make(1, 4, 1, 1, 1, 8, 1, &image)).unwrap();
        assert!(img.rgba.chunks_exact(4).all(|p| p == [77, 77, 77, 255]));
    }

    #[test]
    fn sixteen_bit_keeps_the_high_byte_and_cmyk_is_inverted() {
        // 1x1 16-bit grey 0xAB12 -> 0xAB.
        let img = decode(&make(2, 1, 1, 1, 1, 16, 0, &[0xAB, 0x12])).unwrap();
        assert_eq!(&img.rgba[..4], &[0xAB, 0xAB, 0xAB, 255]);
        // CMYK with no ink at all (255 everywhere) is white; full black ink (K = 0) is black.
        let white = decode(&make(2, 1, 1, 4, 4, 8, 0, &[255, 255, 255, 255])).unwrap();
        assert_eq!(&white.rgba[..4], &[255, 255, 255, 255]);
        let black = decode(&make(2, 1, 1, 4, 4, 8, 0, &[255, 255, 255, 0])).unwrap();
        assert_eq!(&black.rgba[..4], &[0, 0, 0, 255]);
    }

    #[test]
    fn a_fourth_channel_is_transparency() {
        let img = decode(&make(2, 1, 1, 4, 3, 8, 0, &[1, 2, 3, 128])).unwrap();
        assert_eq!(&img.rgba[..4], &[1, 2, 3, 128]);
    }

    #[test]
    fn damaged_files_are_errors() {
        assert!(decode(b"not a psd").is_err());
        assert!(decode(&make(2, 4, 4, 3, 3, 8, 0, &[1, 2, 3])).is_err(), "pixel data too short");
        assert!(decode(&make(2, 2, 2, 3, 3, 32, 0, &[0; 48])).is_err(), "32-bit is not supported");
        assert!(decode(&make(2, 2, 2, 3, 3, 8, 9, &[0; 12])).is_err(), "unknown compression");
        assert!(decode(&make(2, 0, 2, 3, 3, 8, 0, &[])).is_err(), "empty size");
    }

    #[test]
    fn lab_white_is_white() {
        let [r, g, b] = lab_to_rgb(255, 128, 128);
        assert!(r > 250 && g > 250 && b > 245, "{r} {g} {b}");
    }
}
