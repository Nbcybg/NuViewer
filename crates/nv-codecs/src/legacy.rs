//! Older raster formats and game / film archives of pictures:
//! Kodak Photo CD (`pcd`: the 768 x 512 base picture), multi-page PCX (`dcx`: the first page), the Windows Clipboard file (`clp`),
//! Cineon (`cin`) and SMPTE DPX (`dpx`) film frames, Paint.NET (`pdn`: the stored preview), Quake 2 textures (`wal`),
//! texture archives (`wad`: Half-Life / Quake WAD3 / WAD2, Doom IWAD / PWAD: a contact sheet of what is inside), and Corel
//! PHOTO-PAINT (`cpt`: a card; the picture data is a closed format).

use crate::docthumb::{accent_for, fmt_bytes, render_card, Card};
use crate::Image;
use std::path::Path;

pub const EXTS: &[&str] = &["pcd", "dcx", "clp", "cin", "dpx", "pdn", "wal", "wad", "cpt"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    if !is_ext(ext) {
        return None;
    }
    let bytes = match crate::progress::read_file(path) {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    Some(decode_bytes(&bytes, ext, path, first_only))
}

pub fn decode_bytes(b: &[u8], ext: &str, path: &Path, first_only: bool) -> Result<Image, String> {
    match ext {
        "pcd" => pcd(b),
        "dcx" => dcx(b),
        "clp" => clp(b),
        "cin" => cin(b),
        "dpx" => dpx(b),
        "pdn" => pdn(b),
        "wal" => wal(b, path),
        "wad" => wad(b, path, first_only),
        "cpt" => Ok(cpt(b, path, first_only)),
        _ => Err(format!("unsupported format: .{ext}")),
    }
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn le16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn srgb_encode(l: f32) -> u8 {
    let c = l.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn named(mut img: Image, note: &str) -> Image {
    img.profile_assumable = true;
    img.colour_note = Some(note.to_string());
    img
}

// ---- Kodak Photo CD -----------------------------------------------------------------------------------------------------

/// The base picture of an `IMG....PCD`: 768 x 512, Photo YCC 4:2:0 (two luma rows, then one row of each chroma, for every pair of
/// rows), at 0x30000. Rec. 709 primaries, so the chroma is un-subsampled with a smooth filter and converted by the Photo CD formulas.
fn pcd(b: &[u8]) -> Result<Image, String> {
    if b.len() < 0x30000 + 768 * 512 * 3 / 2 || b.get(0x800..0x807) != Some(b"PCD_IPI") {
        return Err("pcd: not a Kodak Photo CD image file (no PCD_IPI header, or the base resolution is missing)".into());
    }
    let rotate = b[0x0E02] & 3;
    let (w, h) = (768usize, 512usize);
    let data = &b[0x30000..0x30000 + w * h * 3 / 2];
    let mut rgba = vec![255u8; w * h * 4];
    for pair in 0..h / 2 {
        let base = pair * (w * 2 + w);
        let (y0, y1) = (&data[base..base + w], &data[base + w..base + 2 * w]);
        let c1 = &data[base + 2 * w..base + 2 * w + w / 2];
        let c2 = &data[base + 2 * w + w / 2..base + 3 * w];
        for (row, y) in [(2 * pair, y0), (2 * pair + 1, y1)] {
            for x in 0..w {
                let l = y[x] as f32 * 1.402 / 255.0; // Photo YCC luma is encoded for 0..1.402
                let cb = (c1[x / 2] as f32 - 156.0) / 111.40;
                let cr = (c2[x / 2] as f32 - 137.0) / 135.64;
                let (r, bl) = (l + cr, l + cb);
                let g = (l - 0.299 * r - 0.114 * bl) / 0.587;
                let o = (row * w + x) * 4;
                // Rec. 709 coded values are close enough to sRGB's for a picture to be looked at.
                rgba[o] = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                rgba[o + 1] = (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                rgba[o + 2] = (bl.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        }
    }
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.orientation = match rotate {
        1 => 6,
        2 => 3,
        3 => 8,
        _ => 1,
    };
    Ok(named(img, "Kodak Photo CD base resolution (768 x 512)"))
}

// ---- DCX ----------------------------------------------------------------------------------------------------------------

fn dcx(b: &[u8]) -> Result<Image, String> {
    if le32(b, 0) != Some(0x3ADE_68B1) {
        return Err("dcx: not a DCX file (bad magic number)".into());
    }
    let off = le32(b, 4).filter(|o| *o != 0).ok_or("dcx: no pages")? as usize;
    let pages = (0..1023).take_while(|i| le32(b, 4 + i * 4).is_some_and(|o| o != 0)).count();
    let mut img = crate::pcx::decode(b.get(off..).ok_or("dcx: the first page starts past the end of the file")?)?;
    img.colour_note = Some(format!("DCX fax / multi-page PCX: page 1 of {pages}"));
    Ok(img)
}

// ---- Windows Clipboard file ---------------------------------------------------------------------------------------------

fn clp(b: &[u8]) -> Result<Image, String> {
    let id = le16(b, 0).unwrap_or(0);
    if !matches!(id, 0xC350..=0xC352) {
        return Err("clp: not a Windows Clipboard file".into());
    }
    let count = le16(b, 2).unwrap_or(0) as usize;
    let mut best: Option<(u16, &[u8])> = None;
    for i in 0..count.min(64) {
        let e = 4 + i * 89;
        let (fmt, len, off) = (le16(b, e).ok_or("clp: truncated")?, le32(b, e + 2).unwrap_or(0) as usize, le32(b, e + 6).unwrap_or(0) as usize);
        let Some(data) = b.get(off..off + len) else { continue };
        // CF_DIBV5 (17) and CF_DIB (8) are pictures we can draw; a bitmap DDB (2) and metafiles are not read.
        if matches!(fmt, 8 | 17) && best.is_none_or(|(f, _)| f != 17) {
            best = Some((fmt, data));
        }
    }
    let (_, dib) = best.ok_or("clp: this clipboard file holds no bitmap (DIB) format")?;
    let img = crate::icon::decode_dib(dib, false, None)?;
    Ok(named(img, "picture saved from the Windows Clipboard"))
}

// ---- Cineon and DPX -----------------------------------------------------------------------------------------------------

/// Cineon 10-bit log code (685 = reference white, 95 = reference black, 0.6 negative gamma) to display light.
fn film_log_to_linear(code: f32, max_code: f32) -> f32 {
    let c = code / max_code * 1023.0;
    let offset = 10f32.powf((95.0 - 685.0) * 0.002 / 0.6);
    ((10f32.powf((c - 685.0) * 0.002 / 0.6) - offset) / (1.0 - offset)).max(0.0)
}

struct Samples<'a> {
    data: &'a [u8],
    be: bool,
}

impl Samples<'_> {
    fn word(&self, o: usize) -> Option<u32> {
        let s = self.data.get(o..o + 4)?;
        Some(if self.be { u32::from_be_bytes([s[0], s[1], s[2], s[3]]) } else { u32::from_le_bytes([s[0], s[1], s[2], s[3]]) })
    }
    fn half(&self, o: usize) -> Option<u16> {
        let s = self.data.get(o..o + 2)?;
        Some(if self.be { u16::from_be_bytes([s[0], s[1]]) } else { u16::from_le_bytes([s[0], s[1]]) })
    }
}

/// `channels` samples per pixel read from `data` (lines padded to 32 bits): returns code values and the maximum code.
fn film_samples(s: &Samples, w: usize, h: usize, channels: usize, bits: u8, packing: u16) -> Result<(Vec<u16>, f32), String> {
    let n = w * h * channels;
    let mut out = Vec::with_capacity(n);
    match bits {
        8 => {
            let line = (w * channels).div_ceil(4) * 4;
            for y in 0..h {
                let row = s.data.get(y * line..y * line + w * channels).ok_or("film frame: the picture data is cut short")?;
                out.extend(row.iter().map(|&v| v as u16));
            }
            Ok((out, 255.0))
        }
        16 => {
            let line = w * channels * 2;
            for y in 0..h {
                for i in 0..w * channels {
                    out.push(s.half(y * line + i * 2).ok_or("film frame: the picture data is cut short")?);
                }
            }
            Ok((out, 65535.0))
        }
        10 | 12 => {
            if bits == 12 {
                return Err("film frame: 12-bit samples are not supported (10, 8 and 16 bits are)".into());
            }
            if packing == 0 {
                return Err("film frame: tightly packed 10-bit samples are not supported (word-aligned ones are)".into());
            }
            // Packing 1: samples left-justified in the word (padding in the low bits); 2: right-justified (padding on top).
            let left_justified = packing != 2;
            // Three samples per 32-bit word; a line starts on a word boundary.
            let words_per_line = (w * channels).div_ceil(3);
            for y in 0..h {
                let mut left = w * channels;
                for k in 0..words_per_line {
                    let word = s.word((y * words_per_line + k) * 4).ok_or("film frame: the picture data is cut short")?;
                    for j in 0..3.min(left) {
                        let shift = if left_justified { 22 - 10 * j } else { 20 - 10 * j };
                        out.push(((word >> shift) & 0x3FF) as u16);
                    }
                    left = left.saturating_sub(3);
                }
            }
            Ok((out, 1023.0))
        }
        b => Err(format!("film frame: {b}-bit samples are not supported")),
    }
}

fn film_to_rgba(codes: &[u16], max: f32, w: usize, h: usize, channels: usize, log: bool, linear: bool) -> Image {
    let mut rgba = vec![255u8; w * h * 4];
    let conv = |c: u16| -> u8 {
        let v = c as f32;
        if log {
            srgb_encode(film_log_to_linear(v, max))
        } else if linear {
            srgb_encode(v / max)
        } else {
            (v / max * 255.0 + 0.5) as u8
        }
    };
    for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
        match channels {
            1 => {
                let g = conv(codes[i]);
                px[..3].copy_from_slice(&[g, g, g]);
            }
            _ => {
                for c in 0..3 {
                    px[c] = conv(codes[i * channels + c]);
                }
                if channels == 4 {
                    px[3] = (codes[i * 4 + 3] as f32 / max * 255.0 + 0.5) as u8;
                }
            }
        }
    }
    Image::new(w as u32, h as u32, rgba)
}

fn cin(b: &[u8]) -> Result<Image, String> {
    if be32(b, 0) != Some(0x802A_5FD7) {
        return Err("cin: not a Cineon file (bad magic number)".into());
    }
    let offset = be32(b, 4).ok_or("cin: truncated")? as usize;
    let channels = *b.get(193).ok_or("cin: truncated")? as usize;
    if !(1..=4).contains(&channels) {
        return Err(format!("cin: {channels} channels are not supported"));
    }
    let bits = *b.get(196 + 2).ok_or("cin: truncated")?;
    let (w, h) = (be32(b, 196 + 4).ok_or("cin: truncated")? as usize, be32(b, 196 + 8).ok_or("cin: truncated")? as usize);
    crate::check_size(w as u64, h as u64)?;
    let data = b.get(offset..).ok_or("cin: the picture data starts past the end of the file")?;
    let (codes, max) = film_samples(&Samples { data, be: true }, w, h, channels, bits, 1)?;
    Ok(named(film_to_rgba(&codes, max, w, h, channels, true, false), "Cineon film frame (log, converted with the usual 685 / 95 reference points)"))
}

fn dpx(b: &[u8]) -> Result<Image, String> {
    let be = match b.get(..4) {
        Some(b"SDPX") => true,
        Some(b"XPDS") => false,
        _ => return Err("dpx: not a DPX file (bad magic number)".into()),
    };
    let r32 = |o: usize| if be { be32(b, o) } else { le32(b, o) };
    let r16 = |o: usize| if be { be16(b, o) } else { le16(b, o) };
    let offset = r32(4).ok_or("dpx: truncated")? as usize;
    let (w, h) = (r32(772).ok_or("dpx: truncated")? as usize, r32(776).ok_or("dpx: truncated")? as usize);
    crate::check_size(w as u64, h as u64)?;
    let descriptor = *b.get(800).ok_or("dpx: truncated")?;
    let transfer = *b.get(801).ok_or("dpx: truncated")?;
    let bits = *b.get(803).ok_or("dpx: truncated")?;
    let packing = r16(804).ok_or("dpx: truncated")?;
    let encoding = r16(806).ok_or("dpx: truncated")?;
    if encoding != 0 {
        return Err("dpx: run-length encoded DPX is not supported".into());
    }
    let channels = match descriptor {
        50 => 3,
        51 | 52 => 4,
        6 | 1 | 2 | 3 | 4 => 1,
        d => return Err(format!("dpx: image descriptor {d} (a YCbCr or multi-element file) is not supported")),
    };
    let data = b.get(offset..).ok_or("dpx: the picture data starts past the end of the file")?;
    let (codes, max) = film_samples(&Samples { data, be }, w, h, channels, bits, packing)?;
    let log = matches!(transfer, 1 | 3);
    let linear = transfer == 2;
    let what = if log { "log (converted with the usual 685 / 95 reference points)" } else if linear { "linear light" } else { "as stored" };
    Ok(named(film_to_rgba(&codes, max, w, h, channels, log, linear), &format!("DPX film frame, {bits}-bit, {what}")))
}

// ---- Paint.NET ----------------------------------------------------------------------------------------------------------

/// `PDN3`, a 3-byte length, an XML header whose `<thumb png="...">` holds a PNG preview (at most 256 px); the layers are a gzip stream of
/// .NET-serialised data that is not read. The preview is enlarged smoothly toward the picture's real size.
fn pdn(b: &[u8]) -> Result<Image, String> {
    if b.get(..4) != Some(b"PDN3") {
        return Err("pdn: not a Paint.NET file (no PDN3 header)".into());
    }
    let len = u32::from_le_bytes([b[4], b[5], b[6], 0]) as usize;
    let xml = String::from_utf8_lossy(b.get(7..7 + len).ok_or("pdn: truncated header")?).into_owned();
    let attr = |key: &str, from: usize| -> Option<String> {
        let tag = format!("{key}=\"");
        let i = xml[from..].find(&tag)? + from + tag.len();
        let j = xml[i..].find('"')? + i;
        Some(xml[i..j].to_string())
    };
    let (full_w, full_h): (u32, u32) = (attr("width", 0).and_then(|v| v.parse().ok()).unwrap_or(0), attr("height", 0).and_then(|v| v.parse().ok()).unwrap_or(0));
    let thumb = xml.find("<thumb").and_then(|i| attr("png", i)).ok_or("pdn: this file stores no preview picture")?;
    let png = base64(&thumb).ok_or("pdn: the stored preview is damaged")?;
    let mut img = crate::decode_image_bytes(&png, "png")?;
    let k = if full_w > 0 { ((full_w as f32 / img.width as f32).round() as u32).clamp(1, 8) } else { 1 };
    if k > 1 {
        img = crate::gpufit::upscale_smooth(&img, k);
    }
    Ok(named(img, &format!("the preview stored in this Paint.NET file ({full_w} x {full_h} px, the layers are not read)")))
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    Some(out)
}

// ---- Quake 2 textures ---------------------------------------------------------------------------------------------------

fn find_palette(path: &Path) -> Option<Vec<u8>> {
    // Quake 2 keeps its palette in pics/colormap.pcx: look beside the texture and in the folders above it.
    let mut dir = path.parent();
    for _ in 0..8 {
        let d = dir?;
        for cand in [d.join("colormap.pcx"), d.join("pics").join("colormap.pcx")] {
            if let Ok(bytes) = std::fs::read(&cand) {
                if bytes.len() > 769 && bytes[bytes.len() - 769] == 12 {
                    return Some(bytes[bytes.len() - 768..].to_vec());
                }
            }
        }
        dir = d.parent();
    }
    None
}

fn wal(b: &[u8], path: &Path) -> Result<Image, String> {
    let (w, h, off) = (le32(b, 32).ok_or("wal: truncated")? as usize, le32(b, 36).ok_or("wal: truncated")? as usize, le32(b, 40).ok_or("wal: truncated")? as usize);
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return Err("wal: not a Quake 2 texture (bad size)".into());
    }
    let px = b.get(off..off + w * h).ok_or("wal: the pixel data is cut short")?;
    let pal = find_palette(path);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &i in px {
        match &pal {
            Some(p) => rgba.extend_from_slice(&[p[i as usize * 3], p[i as usize * 3 + 1], p[i as usize * 3 + 2], 255]),
            None => rgba.extend_from_slice(&[i, i, i, 255]),
        }
    }
    let note = if pal.is_some() { "Quake 2 texture, coloured with the colormap.pcx found beside it" } else { "Quake 2 texture in grey: its palette (pics/colormap.pcx) was not found next to it" };
    Ok(named(Image::new(w as u32, h as u32, rgba), note))
}

// ---- WAD archives ---------------------------------------------------------------------------------------------------------

/// A picture drawn into a `cell` x `cell` square (shrunk to fit, never enlarged beyond 2x), centred on `bg`.
fn contact_sheet(items: &[Image], cell: usize, cols: usize) -> Image {
    let rows = items.len().div_ceil(cols).max(1);
    let (w, h) = (cols * cell, rows * cell);
    let mut px = vec![0u8; w * h * 4];
    for p in px.chunks_exact_mut(4) {
        p.copy_from_slice(&[0x1E, 0x20, 0x26, 255]);
    }
    for (i, it) in items.iter().enumerate() {
        let (cx, cy) = ((i % cols) * cell, (i / cols) * cell);
        let inner = cell - 8;
        let s = (inner as f32 / it.width as f32).min(inner as f32 / it.height as f32).min(2.0);
        let (tw, th) = (((it.width as f32 * s).round() as usize).max(1), ((it.height as f32 * s).round() as usize).max(1));
        let (ox, oy) = (cx + (cell - tw) / 2, cy + (cell - th) / 2);
        for y in 0..th {
            for x in 0..tw {
                // Nearest sample for enlarged (pixel-art) pictures, a small box for shrunk ones.
                let (sx0, sy0) = ((x as f32 / s) as usize, (y as f32 / s) as usize);
                let (sx1, sy1) = ((((x + 1) as f32 / s).ceil() as usize).min(it.width as usize).max(sx0 + 1), (((y + 1) as f32 / s).ceil() as usize).min(it.height as usize).max(sy0 + 1));
                let (mut acc, mut n) = ([0u32; 4], 0u32);
                for yy in sy0..sy1.min(it.height as usize) {
                    for xx in sx0..sx1.min(it.width as usize) {
                        let p = &it.rgba[(yy * it.width as usize + xx) * 4..][..4];
                        let a = p[3] as u32;
                        acc[0] += p[0] as u32 * a;
                        acc[1] += p[1] as u32 * a;
                        acc[2] += p[2] as u32 * a;
                        acc[3] += a;
                        n += 1;
                    }
                }
                if n == 0 || acc[3] == 0 {
                    continue;
                }
                let a = acc[3] as f32 / (n * 255) as f32;
                let o = ((oy + y) * w + ox + x) * 4;
                for c in 0..3 {
                    let v = acc[c] as f32 / acc[3] as f32;
                    px[o + c] = (v * a + px[o + c] as f32 * (1.0 - a)) as u8;
                }
            }
        }
    }
    Image::new(w as u32, h as u32, px)
}

fn name_of(raw: &[u8]) -> String {
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn wad(b: &[u8], path: &Path, first_only: bool) -> Result<Image, String> {
    let magic = b.get(..4).ok_or("wad: truncated")?;
    let count = le32(b, 4).ok_or("wad: truncated")? as usize;
    let dir_at = le32(b, 8).ok_or("wad: truncated")? as usize;
    let limit = if first_only { 16 } else { 150 };
    let (cell, cols) = if first_only { (64, 4) } else { (136, 10) };
    let (items, what): (Vec<Image>, String) = match magic {
        b"WAD3" | b"WAD2" => {
            let mut items = Vec::new();
            let mut palette: Option<Vec<u8>> = None;
            let entry = |i: usize| -> Option<(usize, usize, u8, String)> {
                let e = dir_at.checked_add(i.checked_mul(32)?)?;
                Some((le32(b, e)? as usize, le32(b, e + 4)? as usize, *b.get(e + 12)?, name_of(b.get(e + 16..e + 32)?)))
            };
            // Quake's wad2 palette lump (type 0x40).
            for i in 0..count.min(100_000) {
                if let Some((pos, size, kind, _)) = entry(i) {
                    if kind == 0x40 && size >= 768 {
                        palette = b.get(pos..pos + 768).map(|p| p.to_vec());
                    }
                }
            }
            let total = (0..count.min(100_000)).filter(|&i| entry(i).is_some_and(|e| matches!(e.2, 0x42 | 0x43 | 0x44))).count();
            for i in 0..count.min(100_000) {
                if items.len() >= limit {
                    break;
                }
                let Some((pos, size, kind, _name)) = entry(i) else { continue };
                let Some(lump) = b.get(pos..pos.saturating_add(size)) else { continue };
                match kind {
                    0x43 | 0x44 => {
                        let (w, h) = (le32(lump, 16).unwrap_or(0) as usize, le32(lump, 20).unwrap_or(0) as usize);
                        if w == 0 || h == 0 || w > 2048 || h > 2048 {
                            continue;
                        }
                        let Some(px) = lump.get(le32(lump, 24).unwrap_or(0) as usize..).and_then(|s| s.get(..w * h)) else { continue };
                        // WAD3 appends its own palette after the four mip levels.
                        let tail = le32(lump, 24).unwrap_or(0) as usize + w * h + (w / 2) * (h / 2) + (w / 4) * (h / 4) + (w / 8) * (h / 8) + 2;
                        let own = (kind == 0x43).then(|| lump.get(tail..tail + 768)).flatten();
                        let pal = own.or(palette.as_deref());
                        // Names starting with '{' are transparent where the last palette entry is used.
                        let masked = name_of(lump.get(..16).unwrap_or(&[])).starts_with('{');
                        let mut rgba = Vec::with_capacity(w * h * 4);
                        for &i in px {
                            let (r, g, bl) = match pal {
                                Some(p) => (p[i as usize * 3], p[i as usize * 3 + 1], p[i as usize * 3 + 2]),
                                None => (i, i, i),
                            };
                            rgba.extend_from_slice(&[r, g, bl, if masked && i == 255 { 0 } else { 255 }]);
                        }
                        items.push(Image::new(w as u32, h as u32, rgba));
                    }
                    0x42 => {
                        // qpic: width, height, pixels (palette from the wad).
                        let (w, h) = (le32(lump, 0).unwrap_or(0) as usize, le32(lump, 4).unwrap_or(0) as usize);
                        let Some(px) = lump.get(8..8 + w * h) else { continue };
                        let rgba: Vec<u8> = px.iter().flat_map(|&i| match &palette { Some(p) => [p[i as usize * 3], p[i as usize * 3 + 1], p[i as usize * 3 + 2], 255], None => [i, i, i, 255] }).collect();
                        items.push(Image::new(w as u32, h as u32, rgba));
                    }
                    _ => {}
                }
            }
            (items, format!("{} WAD with {total} pictures", if magic == b"WAD3" { "Half-Life (WAD3)" } else { "Quake (WAD2)" }))
        }
        b"IWAD" | b"PWAD" => doom(b, count, dir_at, limit)?,
        _ => {
            return Ok(card_for(path, "wad", b.len() as u64, vec!["WAD archive".into(), String::new(), "Not a Doom, Quake or Half-Life".into(), "picture archive: this .wad is".into(), "another program's data package.".into()], first_only, "a card: this .wad is not a Doom / Quake / Half-Life archive"));
        }
    };
    if items.is_empty() {
        return Ok(card_for(path, "wad", b.len() as u64, vec!["WAD archive".into(), String::new(), "No pictures found inside.".into()], first_only, "a card: no pictures in this archive"));
    }
    let n = items.len();
    let rows_cols = if n < cols { n.max(1) } else { cols };
    let img = contact_sheet(&items, cell, rows_cols);
    Ok(named(img, &format!("{what}; {n} shown in the sheet")))
}

fn card_for(path: &Path, ext: &str, size: u64, lines: Vec<String>, first_only: bool, note: &str) -> Image {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut img = render_card(&Card { badge: ext.to_uppercase(), accent: accent_for(ext), title: name, subtitle: fmt_bytes(size), lines }, if first_only { 1 } else { 2 });
    img.colour_note = Some(note.to_string());
    img
}

/// Doom IWAD / PWAD: the first PLAYPAL, then the flats (between F_START and F_END) and the patches / sprites in picture format.
fn doom(b: &[u8], count: usize, dir_at: usize, limit: usize) -> Result<(Vec<Image>, String), String> {
    let lumps: Vec<(usize, usize, String)> = (0..count.min(200_000))
        .filter_map(|i| {
            let e = dir_at.checked_add(i.checked_mul(16)?)?;
            Some((le32(b, e)? as usize, le32(b, e + 4)? as usize, name_of(b.get(e + 8..e + 16)?)))
        })
        .collect();
    let pal: Vec<u8> = lumps.iter().find(|l| l.2 == "PLAYPAL").and_then(|(p, s, _)| b.get(*p..*p + (*s).min(768))).map(|p| p.to_vec()).filter(|p| p.len() == 768).unwrap_or_else(|| (0..768).map(|i| (i / 3) as u8).collect());
    let mut items = Vec::new();
    let mut in_flats = false;
    let mut in_sprites = false;
    let mut kinds = (0, 0);
    for (pos, size, name) in &lumps {
        match name.as_str() {
            "F_START" | "FF_START" | "F1_START" | "F2_START" | "F3_START" => in_flats = true,
            "F_END" | "FF_END" | "F1_END" | "F2_END" | "F3_END" => in_flats = false,
            "S_START" | "SS_START" | "P_START" | "PP_START" | "P1_START" | "P2_START" | "P3_START" => in_sprites = true,
            "S_END" | "SS_END" | "P_END" | "PP_END" | "P1_END" | "P2_END" | "P3_END" => in_sprites = false,
            _ if items.len() >= limit => break,
            _ if in_flats && *size == 4096 => {
                if let Some(px) = b.get(*pos..*pos + 4096) {
                    let rgba: Vec<u8> = px.iter().flat_map(|&i| [pal[i as usize * 3], pal[i as usize * 3 + 1], pal[i as usize * 3 + 2], 255]).collect();
                    items.push(Image::new(64, 64, rgba));
                    kinds.0 += 1;
                }
            }
            _ if in_sprites && *size > 8 => {
                if let Some(img) = b.get(*pos..*pos + *size).and_then(|l| doom_patch(l, &pal)) {
                    items.push(img);
                    kinds.1 += 1;
                }
            }
            _ => {}
        }
    }
    Ok((items, format!("Doom WAD: {} flats and {} patches / sprites shown (of {} lumps)", kinds.0, kinds.1, lumps.len())))
}

/// Doom's picture format: width, height, offsets, a column table; each column a list of posts (top, length, pad, pixels, pad).
fn doom_patch(l: &[u8], pal: &[u8]) -> Option<Image> {
    let (w, h) = (le16(l, 0)? as usize, le16(l, 2)? as usize);
    if w == 0 || h == 0 || w > 1024 || h > 1024 || l.len() < 8 + w * 4 {
        return None;
    }
    let mut rgba = vec![0u8; w * h * 4];
    for x in 0..w {
        let mut at = le32(l, 8 + x * 4)? as usize;
        loop {
            let top = *l.get(at)? as usize;
            if top == 0xFF {
                break;
            }
            let len = *l.get(at + 1)? as usize;
            for k in 0..len {
                let i = *l.get(at + 3 + k)? as usize;
                let y = top + k;
                if y < h {
                    let o = (y * w + x) * 4;
                    rgba[o..o + 4].copy_from_slice(&[pal[i * 3], pal[i * 3 + 1], pal[i * 3 + 2], 255]);
                }
            }
            at += len + 4;
        }
    }
    Some(Image::new(w as u32, h as u32, rgba))
}

// ---- Corel PHOTO-PAINT ----------------------------------------------------------------------------------------------------

fn cpt(b: &[u8], path: &Path, first_only: bool) -> Image {
    let version = if b.starts_with(b"CPT9FILE") { "9" } else if b.starts_with(b"CPT") { "(older)" } else { "?" };
    let lines = vec!["Corel PHOTO-PAINT image".to_string(), format!("format version {version}"), String::new(), "The pixels are stored in Corel's".to_string(), "own closed format. Export a PNG,".to_string(), "TIFF or PSD from PHOTO-PAINT to".to_string(), "see the picture.".to_string()];
    card_for(path, "cpt", b.len() as u64, lines, first_only, "a card: the picture data of a .cpt file is a closed format")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_ycc_grey_and_chroma() {
        // A PCD with neutral chroma: luma 182 is white (1.0), 0 is black.
        let mut b = vec![0u8; 0x30000 + 768 * 512 * 3 / 2];
        b[0x800..0x807].copy_from_slice(b"PCD_IPI");
        let data = &mut b[0x30000..];
        for pair in 0..256 {
            let base = pair * (768 * 3);
            data[base..base + 768].fill(182);
            data[base + 768..base + 1536].fill(0);
            data[base + 1536..base + 1536 + 384].fill(156);
            data[base + 1536 + 384..base + 2304].fill(137);
        }
        let img = pcd(&b).unwrap();
        assert_eq!((img.width, img.height), (768, 512));
        assert!(img.rgba[..3].iter().all(|&v| v >= 253), "{:?}", &img.rgba[..4]);
        assert!(img.rgba[768 * 4..768 * 4 + 3].iter().all(|&v| v <= 2));
    }

    #[test]
    fn dpx_10_bit_log_and_8_bit_planar_rgb() {
        // 2x1 10-bit RGB, method A: each pixel is one 32-bit big-endian word R<<22 | G<<12 | B<<2.
        let mut f = vec![0u8; 2048];
        f[..4].copy_from_slice(b"SDPX");
        f[4..8].copy_from_slice(&2048u32.to_be_bytes());
        f[772..776].copy_from_slice(&2u32.to_be_bytes());
        f[776..780].copy_from_slice(&1u32.to_be_bytes());
        f[800] = 50;
        f[801] = 2; // linear
        f[803] = 10;
        f[804..806].copy_from_slice(&1u16.to_be_bytes());
        // 3 samples per word: pixel 0 = (1023, 0, 0), pixel 1 = (0, 1023, 0): six samples = two words.
        let w0: u32 = (1023 << 22) | (0 << 12) | (0 << 2);
        let w1: u32 = (0 << 22) | (1023 << 12) | (0 << 2);
        f.extend(w0.to_be_bytes());
        f.extend(w1.to_be_bytes());
        let img = dpx(&f).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&img.rgba[4..8], &[0, 255, 0, 255]);
    }

    #[test]
    fn cineon_white_is_white_and_black_is_black() {
        assert!(film_log_to_linear(685.0, 1023.0) > 0.99);
        assert!(film_log_to_linear(95.0, 1023.0) < 0.001);
    }

    #[test]
    fn clipboard_file_with_a_dib() {
        // A 1x1 24-bit DIB.
        let mut dib = Vec::new();
        dib.extend(40u32.to_le_bytes());
        dib.extend(1i32.to_le_bytes());
        dib.extend(1i32.to_le_bytes());
        dib.extend([1, 0, 24, 0]);
        dib.extend([0u8; 24]);
        dib.extend([10, 20, 30, 0]); // B G R + row padding
        let mut f = vec![0x50, 0xC3, 1, 0];
        let off = 4 + 89;
        f.extend(8u16.to_le_bytes());
        f.extend((dib.len() as u32).to_le_bytes());
        f.extend((off as u32).to_le_bytes());
        f.extend([0u8; 79]);
        f.extend(&dib);
        let img = clp(&f).unwrap();
        assert_eq!(&img.rgba[..4], &[30, 20, 10, 255]);
    }

    #[test]
    fn wad3_texture_sheet_and_not_a_wad() {
        // One 8x8 WAD3 miptex with its own palette: every pixel palette index 1 = (200, 100, 50).
        let mut lump = Vec::new();
        lump.extend([b'T', b'E', b'X', 0]);
        lump.extend([0u8; 12]);
        lump.extend(8u32.to_le_bytes());
        lump.extend(8u32.to_le_bytes());
        let base = 40u32;
        lump.extend(base.to_le_bytes());
        lump.extend((base + 64).to_le_bytes());
        lump.extend((base + 64 + 16).to_le_bytes());
        lump.extend((base + 64 + 16 + 4).to_le_bytes());
        lump.extend(vec![1u8; 64 + 16 + 4 + 1]);
        lump.extend(256u16.to_le_bytes());
        let mut pal = vec![0u8; 768];
        pal[3..6].copy_from_slice(&[200, 100, 50]);
        lump.extend(pal);
        let mut f = b"WAD3".to_vec();
        f.extend(1u32.to_le_bytes());
        f.extend(((12 + lump.len()) as u32).to_le_bytes());
        f.extend(&lump);
        f.extend(12u32.to_le_bytes());
        f.extend((lump.len() as u32).to_le_bytes());
        f.extend((lump.len() as u32).to_le_bytes());
        f.extend([0x43, 0, 0, 0]);
        f.extend([b'T', b'E', b'X']);
        f.extend([0u8; 13]);
        let img = wad(&f, Path::new("x.wad"), false).unwrap();
        assert!(img.rgba.chunks_exact(4).any(|p| p[..3] == [200, 100, 50]));
        assert!(base64("aGk=").is_some_and(|v| v == b"hi"));
    }

    #[test]
    fn doom_patch_columns() {
        // 1x2 patch: one column with one post of two pixels (palette indices 1, 2).
        let mut l = Vec::new();
        l.extend(1u16.to_le_bytes());
        l.extend(2u16.to_le_bytes());
        l.extend([0u8; 4]);
        l.extend(12u32.to_le_bytes());
        l.extend([0, 2, 0, 1, 2, 0, 0xFF]);
        let pal: Vec<u8> = (0..768).map(|i| (i / 3) as u8).collect();
        let img = doom_patch(&l, &pal).unwrap();
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(&img.rgba[..4], &[1, 1, 1, 255]);
        assert_eq!(&img.rgba[4..8], &[2, 2, 2, 255]);
    }
}
