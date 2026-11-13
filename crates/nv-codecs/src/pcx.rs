//! ZSoft PCX (`.pcx`): the DOS-era paint format. RLE compressed scanlines made of colour planes: 24-bit RGB (3 planes),
//! RGBA (4 planes), 256-colour palette (palette at the end of the file), and the 1, 2 and 4 bit EGA / CGA layouts
//! (bit planes indexing the 16-colour palette in the header).

use crate::Image;

fn le16(b: &[u8], i: usize) -> Result<usize, String> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]]) as usize).ok_or_else(|| "pcx: unexpected end of data".to_string())
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() < 128 || bytes[0] != 0x0A || bytes[2] != 1 || bytes[1] > 5 {
        return Err("pcx: not a PCX file (bad header)".into());
    }
    let bpp = bytes[3] as usize; // bits per pixel in each plane
    let (x0, y0, x1, y1) = (le16(bytes, 4)?, le16(bytes, 6)?, le16(bytes, 8)?, le16(bytes, 10)?);
    if x1 < x0 || y1 < y0 {
        return Err("pcx: invalid size".into());
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    crate::check_size(w as u64, h as u64)?;
    let planes = bytes[65] as usize;
    let stride = le16(bytes, 66)?; // bytes per plane per scanline (always even)
    if !matches!(bpp, 1 | 2 | 4 | 8) || !(1..=4).contains(&planes) || stride == 0 {
        return Err(format!("pcx: unsupported layout ({bpp} bits, {planes} planes)"));
    }
    if stride * 8 < w * bpp {
        return Err("pcx: scanline too short for the width".into());
    }
    // Unpack the RLE data: `planes * stride` bytes for each of the `h` scanlines.
    let line = planes * stride;
    let mut data = Vec::with_capacity(line * h);
    let mut i = 128;
    let end = bytes.len();
    // A 256-colour palette sits in the last 769 bytes (0x0C then 768 bytes); keep the RLE reader out of it.
    let has_palette = bpp == 8 && planes == 1 && end >= 129 + 768 && bytes[end - 769] == 0x0C;
    let data_end = if has_palette { end - 769 } else { end };
    crate::progress::decoding();
    while data.len() < line * h && i < data_end {
        let b = bytes[i];
        i += 1;
        if b >= 0xC0 {
            let run = (b & 0x3F) as usize;
            let v = *bytes.get(i).ok_or("pcx: unexpected end of data")?;
            i += 1;
            let take = run.min(line * h - data.len());
            data.resize(data.len() + take, v);
        } else {
            data.push(b);
        }
    }
    if data.len() < line * h {
        return Err("pcx: unexpected end of data (the image is cut short)".into());
    }
    let ega: Vec<[u8; 3]> = (0..16).map(|k| [bytes[16 + k * 3], bytes[17 + k * 3], bytes[18 + k * 3]]).collect();
    let mut rgba = vec![255u8; w * h * 4];
    for y in 0..h {
        let row = &data[y * line..(y + 1) * line];
        for x in 0..w {
            let o = (y * w + x) * 4;
            let px: [u8; 4] = if bpp == 8 && planes >= 3 {
                [row[x], row[stride + x], row[2 * stride + x], if planes == 4 { row[3 * stride + x] } else { 255 }]
            } else if bpp == 8 && planes == 1 {
                let i = row[x] as usize;
                if has_palette {
                    let p = end - 768 + i * 3;
                    [bytes[p], bytes[p + 1], bytes[p + 2], 255]
                } else {
                    [i as u8, i as u8, i as u8, 255]
                }
            } else {
                // Bit planes: each plane contributes `bpp` bits of the colour index (1 plane of 4 bits = packed nibbles).
                let mut idx = 0usize;
                for p in 0..planes {
                    let bit_pos = x * bpp;
                    let byte = row[p * stride + bit_pos / 8];
                    let v = (byte >> (8 - bpp - (bit_pos % 8))) as usize & ((1 << bpp) - 1);
                    idx |= v << (p * bpp);
                }
                let total_bits = planes * bpp;
                if total_bits == 1 {
                    // Monochrome: the palette's first two entries are rarely set; use black and white.
                    let c = if idx == 0 { 0 } else { 255 };
                    [c, c, c, 255]
                } else {
                    let c = ega[idx.min(15)];
                    [c[0], c[1], c[2], 255]
                }
            };
            rgba[o..o + 4].copy_from_slice(&px);
        }
    }
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.profile_assumable = true;
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(bpp: u8, planes: u8, w: u16, h: u16, stride: u16) -> Vec<u8> {
        let mut v = vec![0u8; 128];
        v[0] = 0x0A;
        v[1] = 5;
        v[2] = 1;
        v[3] = bpp;
        v[8..10].copy_from_slice(&(w - 1).to_le_bytes());
        v[10..12].copy_from_slice(&(h - 1).to_le_bytes());
        v[65] = planes;
        v[66..68].copy_from_slice(&stride.to_le_bytes());
        v
    }

    #[test]
    fn rgb_planes_with_rle() {
        // 2x1 24-bit: scanline = R plane (2 bytes), G plane, B plane. Pixel 0 red, pixel 1 blue.
        // R: 255, 0   G: 0 0 (a run)   B: 0, 255 — stride 2.
        let mut v = header(8, 3, 2, 1, 2);
        v.extend([0xC1 + 0, 255]); // run of 1: 255 (an encoded byte >= 0xC0 needs the run form)
        v.extend([0x00]); // R[1] = 0
        v.extend([0xC2, 0x00]); // G: two zeros
        v.extend([0x00, 0xC1, 0xFF]); // B[0] = 0, B[1] = 255 (run of 1)
        let img = decode(&v).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 0, 255, 255]);
    }

    #[test]
    fn palette_at_the_end() {
        // 2x1, 8 bit, one plane: indices 1, 0. Palette entry 0 = green, 1 = red.
        let mut v = header(8, 1, 2, 1, 2);
        v.extend([1, 0]);
        v.push(0x0C);
        let mut pal = vec![0u8; 768];
        pal[0..3].copy_from_slice(&[0, 255, 0]);
        pal[3..6].copy_from_slice(&[255, 0, 0]);
        v.extend(pal);
        let img = decode(&v).unwrap();
        assert_eq!(&img.rgba[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn one_bit_monochrome() {
        // 8x1, 1 bit, 1 plane: 0b1010_0000 -> white, black, white, black ...
        let mut v = header(1, 1, 8, 1, 2);
        v.extend([0b1010_0000, 0]);
        let img = decode(&v).unwrap();
        let px: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(&px[..4], &[255, 0, 255, 0]);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode(b"nope").is_err());
        assert!(decode(&header(8, 3, 4, 4, 4)).is_err(), "no data after the header");
        let mut bad = header(7, 1, 2, 1, 2);
        bad.extend([0, 0]);
        assert!(decode(&bad).is_err());
    }
}
