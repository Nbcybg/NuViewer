//! PaintTool SAI (version 1) documents. A `.sai` file is an encrypted little file system of 4096-byte pages; the cipher, the
//! key table and the layout are those published by the MIT-licensed libsai project (https://github.com/Wunkolo/libsai,
//! Copyright (c) 2017-2023 Wunkolo). What is read here: the stored thumbnail, and the canvas rebuilt from the raster layers
//! (normal and the common blend modes, opacity, visibility, folders, clipping). Vector linework layers and masks are not
//! read; a drawing made only of those is shown by its thumbnail.

use crate::sai_keys::USER_KEY;
use crate::Image;
use std::collections::HashMap;
use std::sync::Mutex;

const PAGE: usize = 0x1000;
const SPAN: usize = PAGE / 8; // a table page describes this many pages (itself first)

fn keysum(p: u32) -> u32 {
    USER_KEY[(p >> 24) as usize & 0xFF].wrapping_add(USER_KEY[(p >> 16) as usize & 0xFF]).wrapping_add(USER_KEY[(p >> 8) as usize & 0xFF]).wrapping_add(USER_KEY[p as usize & 0xFF])
}

fn words(page: &[u8]) -> Vec<u32> {
    page.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// A table page (every 512th page): checksum / next-page pairs.
fn decrypt_table(raw: &[u8], index: u32) -> Vec<u32> {
    let mut prev = index & !0x1FF;
    words(raw)
        .into_iter()
        .map(|cur| {
            let x = prev ^ cur ^ keysum(prev);
            prev = cur;
            x.rotate_left(16)
        })
        .collect()
}

/// A data page, given the checksum its table lists.
fn decrypt_data(raw: &[u8], checksum: u32) -> Vec<u8> {
    let mut prev = checksum;
    let mut out = Vec::with_capacity(PAGE);
    for cur in words(raw) {
        out.extend_from_slice(&cur.wrapping_sub(prev ^ keysum(prev)).to_le_bytes());
        prev = cur;
    }
    out
}

pub struct Fs<'a> {
    d: &'a [u8],
    tables: Mutex<HashMap<u32, Option<Vec<u32>>>>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
    pub page: u32,
    pub size: u32,
}

impl<'a> Fs<'a> {
    pub fn open(d: &'a [u8]) -> Option<Fs<'a>> {
        if d.len() < PAGE * 3 || d.len() % PAGE != 0 {
            return None;
        }
        let fs = Fs { d, tables: Mutex::new(HashMap::new()) };
        // The root folder is page 2: its first entry must have sane flags and a name.
        let root = fs.page(2)?;
        let flags = u32::from_le_bytes([root[0], root[1], root[2], root[3]]);
        (flags != 0 && root[4] != 0 && root[4].is_ascii()).then_some(fs)
    }

    fn table(&self, index: u32) -> Option<Vec<u32>> {
        let mut t = self.tables.lock().ok()?;
        t.entry(index).or_insert_with(|| self.d.get(index as usize * PAGE..(index as usize + 1) * PAGE).map(|raw| decrypt_table(raw, index))).clone()
    }

    fn page(&self, index: u32) -> Option<Vec<u8>> {
        let raw = self.d.get(index as usize * PAGE..(index as usize + 1) * PAGE)?;
        if index as usize % SPAN == 0 {
            return Some(self.table(index)?.iter().flat_map(|w| w.to_le_bytes()).collect());
        }
        let table = self.table(index / SPAN as u32 * SPAN as u32)?;
        let checksum = *table.get((index as usize % SPAN) * 2)?;
        Some(decrypt_data(raw, checksum))
    }

    fn next(&self, index: u32) -> Option<u32> {
        let table = self.table(index / SPAN as u32 * SPAN as u32)?;
        table.get((index as usize % SPAN) * 2 + 1).copied()
    }

    fn entries(&self, page: u32) -> Vec<Entry> {
        let mut out = Vec::new();
        let mut at = page;
        let mut guard = 0;
        while at != 0 && guard < 4096 {
            guard += 1;
            let Some(p) = self.page(at) else { break };
            for e in p.chunks_exact(64) {
                if u32::from_le_bytes([e[0], e[1], e[2], e[3]]) == 0 {
                    return out;
                }
                let name_end = e[4..36].iter().position(|&b| b == 0).unwrap_or(32);
                out.push(Entry { name: String::from_utf8_lossy(&e[4..4 + name_end]).into_owned(), folder: e[38] == 0x10, page: u32::from_le_bytes([e[40], e[41], e[42], e[43]]), size: u32::from_le_bytes([e[44], e[45], e[46], e[47]]) });
            }
            at = self.next(at).unwrap_or(0);
        }
        out
    }

    /// The entry at a path like `thumbnail` or `/layers/00000002`.
    pub fn find(&self, path: &str) -> Option<Entry> {
        let mut dir = 2u32;
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        for (i, part) in parts.iter().enumerate() {
            let e = self.entries(dir).into_iter().find(|e| e.name == *part)?;
            if i + 1 == parts.len() {
                return Some(e);
            }
            if !e.folder {
                return None;
            }
            dir = e.page;
        }
        None
    }

    pub fn read(&self, e: &Entry) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(e.size as usize);
        let mut at = e.page;
        let mut guard = 0u64;
        while out.len() < e.size as usize && guard < (1 << 24) {
            guard += 1;
            let p = self.page(at)?;
            let take = (e.size as usize - out.len()).min(PAGE);
            out.extend_from_slice(&p[..take]);
            if out.len() < e.size as usize {
                at = self.next(at).filter(|n| *n != 0)?;
            }
        }
        Some(out)
    }

    pub fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.read(&self.find(path)?)
    }
}

fn le32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// The stored preview: width, height, "BM32", BGRA pixels.
pub fn thumbnail(fs: &Fs) -> Option<Image> {
    let t = fs.file("thumbnail")?;
    let (w, h) = (le32(&t, 0)? as usize, le32(&t, 4)? as usize);
    if t.get(8..12)? != b"BM32" || w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let px = t.get(12..12 + w * h * 4)?;
    let mut rgba = Vec::with_capacity(px.len());
    for p in px.chunks_exact(4) {
        // Premultiplied BGRA in the file.
        let a = p[3] as u32;
        let un = |c: u8| if a == 0 || a == 255 { c } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
        rgba.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
    }
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.profile_assumable = true;
    Some(img)
}

pub fn canvas_size(fs: &Fs) -> Option<(u32, u32)> {
    let c = fs.file("canvas")?;
    Some((le32(&c, 4)?, le32(&c, 8)?))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Sub,
}

fn blend_of(tag: [u8; 4]) -> Blend {
    let mut r = tag;
    r.reverse();
    match (&tag, &r) {
        (b"mul ", _) | (_, b"mul ") => Blend::Multiply,
        (b"scrn", _) | (_, b"scrn") => Blend::Screen,
        (b"over", _) | (_, b"over") => Blend::Overlay,
        (b"add ", _) | (_, b"add ") => Blend::Add,
        (b"sub ", _) | (_, b"sub ") => Blend::Sub,
        _ => Blend::Normal,
    }
}

struct LayerInfo {
    id: u32,
    kind: u16,
}

struct Layer {
    x: i32,
    y: i32,
    w: usize,
    h: usize,
    opacity: u8,
    visible: bool,
    clipping: bool,
    blend: Blend,
    parent: u32,
    pixels: Vec<u8>, // straight RGBA, w * h
}

fn unpack_rle(src: &[u8], out: &mut [u8; 0x1000], channel: usize) {
    // 1024 pixels, one channel of the interleaved BGRA tile.
    let (mut i, mut written) = (0usize, 0usize);
    while written < 1024 && i < src.len() {
        let n = src[i];
        i += 1;
        if n == 128 {
            continue;
        }
        if n < 128 {
            let run = n as usize + 1;
            for _ in 0..run {
                if written >= 1024 || i >= src.len() {
                    break;
                }
                out[written * 4 + channel] = src[i];
                i += 1;
                written += 1;
            }
        } else {
            let run = (n ^ 0xFF) as usize + 2;
            let Some(&v) = src.get(i) else { break };
            i += 1;
            for _ in 0..run {
                if written >= 1024 {
                    break;
                }
                out[written * 4 + channel] = v;
                written += 1;
            }
        }
    }
}

fn read_layer(file: &[u8]) -> Option<Layer> {
    let kind = le32(file, 0)?;
    if kind != 3 {
        return None;
    }
    let (x, y) = (le32(file, 8)? as i32, le32(file, 12)? as i32);
    let (w, h) = (le32(file, 16)? as usize, le32(file, 20)? as usize);
    let (opacity, visible, clipping) = (*file.get(28)?, *file.get(29)? != 0, *file.get(31)? != 0);
    let blend = blend_of(file.get(33..37)?.try_into().ok()?);
    // Serial stream: tag, size, data ... until a zero tag.
    let mut at = 37;
    let mut parent = 0;
    loop {
        let tag = le32(file, at)?;
        at += 4;
        if tag == 0 {
            break;
        }
        let size = le32(file, at)? as usize;
        at += 4;
        let bytes = file.get(at..at + size)?;
        if bytes.len() == 4 && (tag.to_le_bytes() == *b"pfid" || tag.to_be_bytes() == *b"pfid") {
            parent = le32(bytes, 0)?;
        }
        at += size;
    }
    if w == 0 || h == 0 || w > 32768 || h > 32768 || w * h > 400_000_000 {
        return None;
    }
    let (bw, bh) = (w / 32, h / 32);
    if bw == 0 || bh == 0 {
        return None;
    }
    let map = file.get(at..at + bw * bh)?;
    at += bw * bh;
    let mut px = vec![0u8; bw * 32 * bh * 32 * 4];
    let stride = bw * 32 * 4;
    let mut tile = [0u8; 0x1000];
    for ty in 0..bh {
        for tx in 0..bw {
            if map[ty * bw + tx] == 0 {
                continue;
            }
            tile.fill(0);
            let mut channel = 0;
            loop {
                let size = u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]) as usize;
                at += 2;
                if size == 0 {
                    break;
                }
                let src = file.get(at..at + size)?;
                at += size;
                if channel < 4 {
                    unpack_rle(src, &mut tile, channel);
                }
                channel += 1;
                if channel >= 4 {
                    // The rest of the streams (four more) are skipped.
                    for _ in 0..4 {
                        let s = u16::from_le_bytes([*file.get(at)?, *file.get(at + 1)?]) as usize;
                        at += 2 + s;
                    }
                    break;
                }
            }
            for row in 0..32 {
                for col in 0..32 {
                    let s = (row * 32 + col) * 4;
                    let (b, g, r, a) = (tile[s] as u32, tile[s + 1] as u32, tile[s + 2] as u32, tile[s + 3] as u32);
                    let un = |c: u32| if a == 0 { 0 } else if a == 255 { c as u8 } else { ((c * 255 + a / 2) / a).min(255) as u8 };
                    let d = (ty * 32 + row) * stride + (tx * 32 + col) * 4;
                    px[d..d + 4].copy_from_slice(&[un(r), un(g), un(b), a as u8]);
                }
            }
        }
    }
    Some(Layer { x, y, w: bw * 32, h: bh * 32, opacity, visible, clipping, blend, parent, pixels: px })
}

fn blend_channel(mode: Blend, cb: f32, cs: f32) -> f32 {
    match mode {
        Blend::Normal => cs,
        Blend::Multiply => cb * cs,
        Blend::Screen => cb + cs - cb * cs,
        Blend::Overlay => {
            if cb <= 0.5 {
                2.0 * cb * cs
            } else {
                1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
            }
        }
        Blend::Add => (cb + cs).min(1.0),
        Blend::Sub => (cb - cs).max(0.0),
    }
}

/// The canvas rebuilt from the raster layers, bottom to top. `None` when there is nothing to show.
pub fn composite(fs: &Fs) -> Option<Image> {
    let (cw, ch) = canvas_size(fs)?;
    let (cw, ch) = (cw as usize, ch as usize);
    if cw == 0 || ch == 0 || cw > 32768 || ch > 32768 {
        return None;
    }
    crate::check_size(cw as u64, ch as u64).ok()?;
    let table = fs.file("laytbl")?;
    let count = le32(&table, 0)? as usize;
    let infos: Vec<LayerInfo> = (0..count.min(4096)).filter_map(|i| Some(LayerInfo { id: le32(&table, 4 + i * 8)?, kind: u16::from_le_bytes([*table.get(8 + i * 8)?, *table.get(9 + i * 8)?]) })).collect();
    // Folders: their visibility hides what is inside.
    let mut folder_visible: HashMap<u32, (bool, u32)> = HashMap::new();
    let mut layers: Vec<Layer> = Vec::new();
    let mut ids = Vec::new();
    for info in &infos {
        let Some(file) = fs.file(&format!("/layers/{:08x}", info.id)) else { continue };
        if info.kind == 8 {
            let visible = file.get(29).map(|v| *v != 0).unwrap_or(true);
            let mut parent = 0;
            let mut at = 37;
            while let Some(tag) = le32(&file, at) {
                at += 4;
                if tag == 0 {
                    break;
                }
                let Some(size) = le32(&file, at).map(|s| s as usize) else { break };
                at += 4;
                if size == 4 && (tag.to_le_bytes() == *b"pfid" || tag.to_be_bytes() == *b"pfid") {
                    parent = le32(&file, at).unwrap_or(0);
                }
                at += size;
            }
            folder_visible.insert(info.id, (visible, parent));
            continue;
        }
        if let Some(l) = read_layer(&file) {
            ids.push(info.id);
            layers.push(l);
        }
    }
    let hidden_by_folder = |mut parent: u32| -> bool {
        for _ in 0..32 {
            match folder_visible.get(&parent) {
                Some((vis, up)) => {
                    if !vis {
                        return true;
                    }
                    parent = *up;
                }
                None => return false,
            }
        }
        false
    };
    let mut canvas = vec![0u8; cw * ch * 4];
    let mut base_alpha = vec![0u8; cw * ch]; // the alpha of the last layer that is not clipped, for clipping layers above it
    let mut drew = false;
    for l in &layers {
        if !l.visible || l.opacity == 0 || hidden_by_folder(l.parent) {
            continue;
        }
        let op = l.opacity as f32 / 100.0;
        let op = op.clamp(0.0, 1.0);
        let new_base = !l.clipping;
        for row in 0..l.h {
            let cy = l.y as i64 + row as i64;
            if cy < 0 || cy >= ch as i64 {
                continue;
            }
            for col in 0..l.w {
                let cx = l.x as i64 + col as i64;
                if cx < 0 || cx >= cw as i64 {
                    continue;
                }
                let s = (row * l.w + col) * 4;
                let sa0 = l.pixels[s + 3];
                let ci = cy as usize * cw + cx as usize;
                if new_base {
                    base_alpha[ci] = 0;
                }
                if sa0 == 0 {
                    continue;
                }
                let mut sa = sa0 as f32 / 255.0 * op;
                if l.clipping {
                    sa *= base_alpha[ci] as f32 / 255.0;
                }
                if sa <= 0.0 {
                    continue;
                }
                if new_base {
                    base_alpha[ci] = (sa * 255.0 + 0.5) as u8;
                }
                let d = ci * 4;
                let ba = canvas[d + 3] as f32 / 255.0;
                let oa = sa + ba * (1.0 - sa);
                if l.blend == Blend::Normal && ba == 0.0 {
                    canvas[d..d + 3].copy_from_slice(&l.pixels[s..s + 3]);
                    canvas[d + 3] = (oa * 255.0 + 0.5) as u8;
                    drew = true;
                    continue;
                }
                for c in 0..3 {
                    let cs = l.pixels[s + c] as f32 / 255.0;
                    let cb = canvas[d + c] as f32 / 255.0;
                    let mixed = (1.0 - ba) * cs + ba * blend_channel(l.blend, cb, cs);
                    let out = (sa * mixed + (1.0 - sa) * ba * cb) / oa;
                    canvas[d + c] = (out.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
                canvas[d + 3] = (oa * 255.0 + 0.5) as u8;
                drew = true;
            }
        }
    }
    if !drew {
        return None;
    }
    let mut img = Image::new(cw as u32, ch as u32, canvas);
    img.profile_assumable = true;
    Some(img)
}

/// The picture of a SAI file: the canvas from its layers (or only the thumbnail when `thumb_only`, or when the layers give
/// nothing, for example a drawing made of linework). The note says which one it is.
pub fn decode(bytes: &[u8], thumb_only: bool) -> Result<Image, String> {
    let fs = Fs::open(bytes).ok_or("not a PaintTool SAI (version 1) document, or it is damaged (SAI2 files use another format)")?;
    if !thumb_only {
        if let Some(mut img) = composite(&fs) {
            img.colour_note = Some("PaintTool SAI canvas rebuilt from its raster layers (linework and mask layers are not drawn)".into());
            return Ok(img);
        }
    }
    let mut img = thumbnail(&fs).ok_or("this SAI file stores no readable thumbnail and no raster layers")?;
    img.colour_note = Some("the thumbnail stored in this SAI file".into());
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_cipher_round_trips_a_known_shape() {
        // Encrypt a page the way the decryptor undoes it, then decrypt: the plain text must come back.
        let plain: Vec<u32> = (0..1024u32).map(|i| i.wrapping_mul(0x9E37_79B1) ^ 0x1234_5678).collect();
        let index = 1024u32;
        let mut prev = index & !0x1FF;
        let mut cipher = Vec::new();
        for &p in &plain {
            // decrypt: x = prev ^ cur ^ keysum(prev); plain = rotl(x, 16)  =>  cur = rotr(plain, 16) ^ prev ^ keysum(prev)
            let cur = p.rotate_right(16) ^ prev ^ keysum(prev);
            cipher.push(cur);
            prev = cur;
        }
        let raw: Vec<u8> = cipher.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(decrypt_table(&raw, index), plain);
    }

    #[test]
    fn rle_tiles_unpack_runs() {
        let mut tile = [0u8; 0x1000];
        // 0x81 = 129: (129 ^ 0xFF) + 2 = 128 repeats of the next byte; eight of them fill the 1024 pixels.
        let mut src = Vec::new();
        for _ in 0..8 {
            src.extend([0x81u8, 7]);
        }
        unpack_rle(&src, &mut tile, 2);
        assert_eq!(tile[2], 7);
        assert_eq!(tile[1023 * 4 + 2], 7);
        assert_eq!(tile[3], 0, "other channels stay empty");
        // A copy run: 0x02 = copy the next 3 bytes.
        let mut tile = [0u8; 0x1000];
        unpack_rle(&[0x02, 9, 8, 7], &mut tile, 0);
        assert_eq!((tile[0], tile[4], tile[8]), (9, 8, 7));
    }
}
