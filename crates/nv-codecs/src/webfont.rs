//! Web and embedded fonts turned back into plain sfnt (TrueType / OpenType) bytes, so one reader (`fontview`) handles them all:
//! WOFF (every table zlib-compressed on its own), WOFF2 (one Brotli stream, with the `glyf`, `loca` and `hmtx` tables stored in a
//! more compact "transformed" form that is rebuilt here), and EOT (a header, then the font, optionally XOR-ed; the MicroType
//! compressed kind is not read).

use std::io::Read;

fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// One table of the font being rebuilt.
struct Table {
    tag: [u8; 4],
    data: Vec<u8>,
}

/// Write the tables as an sfnt file: offset table, sorted directory with checksums, 4-byte aligned data.
fn build_sfnt(flavor: u32, mut tables: Vec<Table>) -> Vec<u8> {
    tables.sort_by(|a, b| a.tag.cmp(&b.tag));
    let n = tables.len() as u16;
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= n as u32 {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let mut out = Vec::new();
    out.extend(flavor.to_be_bytes());
    out.extend(n.to_be_bytes());
    out.extend(search_range.to_be_bytes());
    out.extend(entry_selector.to_be_bytes());
    out.extend((n * 16 - search_range).to_be_bytes());
    let mut offset = 12 + tables.len() * 16;
    let mut dir = Vec::new();
    let mut body = Vec::new();
    for t in &tables {
        let mut sum = 0u32;
        for c in t.data.chunks(4) {
            let mut w = [0u8; 4];
            w[..c.len()].copy_from_slice(c);
            sum = sum.wrapping_add(u32::from_be_bytes(w));
        }
        dir.extend(t.tag);
        dir.extend(sum.to_be_bytes());
        dir.extend((offset as u32).to_be_bytes());
        dir.extend((t.data.len() as u32).to_be_bytes());
        body.extend(&t.data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + tables.len() * 16 + body.len();
    }
    out.extend(dir);
    out.extend(body);
    out
}

// ---- WOFF ---------------------------------------------------------------------------------------------------------------

pub fn is_woff(b: &[u8]) -> bool {
    b.starts_with(b"wOFF")
}
pub fn is_woff2(b: &[u8]) -> bool {
    b.starts_with(b"wOF2")
}

pub fn woff_to_sfnt(b: &[u8]) -> Result<Vec<u8>, String> {
    let flavor = be32(b, 4).ok_or("woff: file too short")?;
    let count = be16(b, 12).ok_or("woff: file too short")? as usize;
    let mut tables = Vec::new();
    for i in 0..count {
        let e = 44 + i * 20;
        let tag: [u8; 4] = b.get(e..e + 4).ok_or("woff: table directory cut off")?.try_into().unwrap();
        let (off, comp, orig) = (be32(b, e + 4).unwrap() as usize, be32(b, e + 8).unwrap() as usize, be32(b, e + 12).unwrap() as usize);
        let raw = b.get(off..off.checked_add(comp).ok_or("woff: bad table")?).ok_or("woff: table data cut off")?;
        if orig > 256 << 20 {
            return Err("woff: a table claims to be larger than 256 MB".into());
        }
        let data = if comp < orig {
            let mut v = Vec::with_capacity(orig);
            flate2::read::ZlibDecoder::new(raw).take(orig as u64 + 1).read_to_end(&mut v).map_err(|e| format!("woff: inflate: {e}"))?;
            if v.len() != orig {
                return Err("woff: a table did not unpack to its stated size".into());
            }
            v
        } else {
            raw.to_vec()
        };
        tables.push(Table { tag, data });
    }
    Ok(build_sfnt(flavor, tables))
}

// ---- WOFF2 --------------------------------------------------------------------------------------------------------------

const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS",
    b"GSUB", b"EBSC", b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar", b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak",
    b"Zapf", b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill",
];

struct Cursor<'a> {
    d: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(d: &'a [u8]) -> Self {
        Cursor { d, at: 0 }
    }
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.d.get(self.at).ok_or("woff2: data ends too early")?;
        self.at += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(((self.u8()? as u16) << 8) | self.u8()? as u16)
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.d.get(self.at..self.at.checked_add(n).ok_or("woff2: bad length")?).ok_or("woff2: data ends too early")?;
        self.at += n;
        Ok(s)
    }
    /// UIntBase128: seven bits per byte, big-endian, high bit = another byte follows.
    fn base128(&mut self) -> Result<u32, String> {
        let mut v = 0u32;
        for i in 0..5 {
            let c = self.u8()?;
            if i == 0 && c == 0x80 {
                return Err("woff2: bad variable-length number".into());
            }
            if v & 0xFE00_0000 != 0 {
                return Err("woff2: variable-length number overflows".into());
            }
            v = (v << 7) | (c & 0x7F) as u32;
            if c & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("woff2: variable-length number too long".into())
    }
    /// 255UInt16: one byte, or 253 + two bytes, or 255 / 254 + a byte with an offset.
    fn u255(&mut self) -> Result<u16, String> {
        let c = self.u8()?;
        Ok(match c {
            253 => self.u16()?,
            255 => 253 + self.u8()? as u16,
            254 => 506 + self.u8()? as u16,
            c => c as u16,
        })
    }
}

fn with_sign(flag: u8, v: i32) -> i32 {
    if flag & 1 != 0 {
        v
    } else {
        -v
    }
}

/// Rebuild `glyf` and `loca` from the transformed `glyf`; also returns every glyph's xMin (for the transformed `hmtx`).
fn rebuild_glyf(t: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<i16>), String> {
    let mut h = Cursor::new(t);
    h.u16()?;
    let option_flags = h.u16()?;
    let num_glyphs = h.u16()? as usize;
    let index_format = h.u16()?;
    let mut sizes = [0usize; 7];
    for s in sizes.iter_mut() {
        *s = ((h.u16()? as usize) << 16) | h.u16()? as usize;
    }
    let [n_contour, n_points, n_flags, n_glyph, n_comp, n_bbox, n_instr] = sizes;
    let mut contours = Cursor::new(h.take(n_contour)?);
    let mut points = Cursor::new(h.take(n_points)?);
    let mut flags = Cursor::new(h.take(n_flags)?);
    let mut glyphs = Cursor::new(h.take(n_glyph)?);
    let mut comps = Cursor::new(h.take(n_comp)?);
    let bbox_all = h.take(n_bbox)?;
    let mut instrs = Cursor::new(h.take(n_instr)?);
    let overlap: &[u8] = if option_flags & 1 != 0 { h.take(num_glyphs.div_ceil(8)).unwrap_or(&[]) } else { &[] };
    let bitmap_len = 4 * num_glyphs.div_ceil(32);
    if bbox_all.len() < bitmap_len {
        return Err("woff2: bounding box stream too short".into());
    }
    let (bbox_bitmap, bbox_data) = bbox_all.split_at(bitmap_len);
    let mut bboxes = Cursor::new(bbox_data);

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = Vec::with_capacity(num_glyphs + 1);
    let mut xmins = Vec::with_capacity(num_glyphs);
    for g in 0..num_glyphs {
        loca.push(glyf.len() as u32);
        let n = contours.i16()?;
        let has_bbox = bbox_bitmap[g >> 3] & (0x80 >> (g & 7)) != 0;
        if n == 0 {
            if has_bbox {
                return Err("woff2: an empty glyph has a bounding box".into());
            }
            xmins.push(0);
            continue;
        }
        if n > 0 {
            let mut end_pts = Vec::new();
            let mut total = 0usize;
            for _ in 0..n {
                total += points.u255()? as usize;
                if total > 65535 {
                    return Err("woff2: glyph with too many points".into());
                }
                end_pts.push((total - 1) as u16);
            }
            let mut pts: Vec<(i32, i32, bool)> = Vec::with_capacity(total);
            let (mut x, mut y) = (0i32, 0i32);
            for _ in 0..total {
                let f = flags.u8()?;
                let on = f & 0x80 == 0;
                let f = f & 0x7F;
                let (dx, dy) = if f < 10 {
                    (0, with_sign(f, (((f as i32) & 14) << 7) + glyphs.u8()? as i32))
                } else if f < 20 {
                    (with_sign(f, ((((f - 10) as i32) & 14) << 7) + glyphs.u8()? as i32), 0)
                } else if f < 84 {
                    let (b0, b1) = ((f - 20) as i32, glyphs.u8()? as i32);
                    (with_sign(f, 1 + (b0 & 0x30) + (b1 >> 4)), with_sign(f >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F)))
                } else if f < 120 {
                    let b0 = (f - 84) as i32;
                    let (b1, b2) = (glyphs.u8()? as i32, glyphs.u8()? as i32);
                    (with_sign(f, 1 + ((b0 / 12) << 8) + b1), with_sign(f >> 1, 1 + (((b0 % 12) >> 2) << 8) + b2))
                } else if f < 124 {
                    let (b1, b2, b3) = (glyphs.u8()? as i32, glyphs.u8()? as i32, glyphs.u8()? as i32);
                    (with_sign(f, (b1 << 4) + (b2 >> 4)), with_sign(f >> 1, ((b2 & 0x0F) << 8) + b3))
                } else {
                    let (b1, b2, b3, b4) = (glyphs.u8()? as i32, glyphs.u8()? as i32, glyphs.u8()? as i32, glyphs.u8()? as i32);
                    (with_sign(f, (b1 << 8) + b2), with_sign(f >> 1, (b3 << 8) + b4))
                };
                x += dx;
                y += dy;
                pts.push((x, y, on));
            }
            let ilen = glyphs.u255()? as usize;
            let instr = instrs.take(ilen)?;
            let (xmin, ymin, xmax, ymax) = if has_bbox {
                (bboxes.i16()?, bboxes.i16()?, bboxes.i16()?, bboxes.i16()?)
            } else {
                let xs = pts.iter().map(|p| p.0);
                let ys = pts.iter().map(|p| p.1);
                (xs.clone().min().unwrap_or(0) as i16, ys.clone().min().unwrap_or(0) as i16, xs.max().unwrap_or(0) as i16, ys.max().unwrap_or(0) as i16)
            };
            xmins.push(xmin);
            glyf.extend(n.to_be_bytes());
            for v in [xmin, ymin, xmax, ymax] {
                glyf.extend(v.to_be_bytes());
            }
            for e in &end_pts {
                glyf.extend(e.to_be_bytes());
            }
            glyf.extend((ilen as u16).to_be_bytes());
            glyf.extend(instr);
            let (mut fl, mut xb, mut yb) = (Vec::new(), Vec::new(), Vec::new());
            let (mut px, mut py) = (0i32, 0i32);
            for (i, &(x, y, on)) in pts.iter().enumerate() {
                let mut f = if on { 1u8 } else { 0 };
                if i == 0 && overlap.get(g >> 3).is_some_and(|b| b & (0x80 >> (g & 7)) != 0) {
                    f |= 0x40;
                }
                let (dx, dy) = (x - px, y - py);
                (px, py) = (x, y);
                if dx == 0 {
                    f |= 0x10;
                } else if dx.abs() < 256 {
                    f |= 0x02 | if dx > 0 { 0x10 } else { 0 };
                    xb.push(dx.unsigned_abs() as u8);
                } else {
                    xb.extend((dx as i16).to_be_bytes());
                }
                if dy == 0 {
                    f |= 0x20;
                } else if dy.abs() < 256 {
                    f |= 0x04 | if dy > 0 { 0x20 } else { 0 };
                    yb.push(dy.unsigned_abs() as u8);
                } else {
                    yb.extend((dy as i16).to_be_bytes());
                }
                fl.push(f);
            }
            glyf.extend(fl);
            glyf.extend(xb);
            glyf.extend(yb);
        } else {
            // A composite: the component records are copied as they are; the box is stored, never computed.
            if !has_bbox {
                return Err("woff2: a composite glyph has no bounding box".into());
            }
            let (xmin, ymin, xmax, ymax) = (bboxes.i16()?, bboxes.i16()?, bboxes.i16()?, bboxes.i16()?);
            xmins.push(xmin);
            glyf.extend((-1i16).to_be_bytes());
            for v in [xmin, ymin, xmax, ymax] {
                glyf.extend(v.to_be_bytes());
            }
            let start = comps.at;
            let mut instructions = false;
            loop {
                let fl = comps.u16()?;
                comps.u16()?;
                comps.take(if fl & 0x0001 != 0 { 4 } else { 2 })?;
                comps.take(if fl & 0x0008 != 0 { 2 } else if fl & 0x0040 != 0 { 4 } else if fl & 0x0080 != 0 { 8 } else { 0 })?;
                instructions |= fl & 0x0100 != 0;
                if fl & 0x0020 == 0 {
                    break;
                }
            }
            glyf.extend(&comps.d[start..comps.at]);
            if instructions {
                let ilen = glyphs.u255()? as usize;
                glyf.extend((ilen as u16).to_be_bytes());
                glyf.extend(instrs.take(ilen)?);
            }
        }
        while glyf.len() % 4 != 0 {
            glyf.push(0);
        }
    }
    loca.push(glyf.len() as u32);
    let mut loca_bytes = Vec::new();
    for o in &loca {
        if index_format == 0 {
            loca_bytes.extend(((o / 2) as u16).to_be_bytes());
        } else {
            loca_bytes.extend(o.to_be_bytes());
        }
    }
    Ok((glyf, loca_bytes, xmins))
}

/// Rebuild `hmtx` from its transformed form: left side bearings left out are the glyphs' xMin.
fn rebuild_hmtx(t: &[u8], num_glyphs: usize, num_hmetrics: usize, xmins: &[i16]) -> Result<Vec<u8>, String> {
    let mut c = Cursor::new(t);
    let flags = c.u8()?;
    let mut advances = Vec::with_capacity(num_hmetrics);
    for _ in 0..num_hmetrics {
        advances.push(c.u16()?);
    }
    let mut lsb = Vec::with_capacity(num_glyphs);
    for i in 0..num_hmetrics {
        lsb.push(if flags & 1 == 0 { c.i16()? } else { xmins.get(i).copied().unwrap_or(0) });
    }
    for i in num_hmetrics..num_glyphs {
        lsb.push(if flags & 2 == 0 { c.i16()? } else { xmins.get(i).copied().unwrap_or(0) });
    }
    let mut out = Vec::new();
    for i in 0..num_glyphs {
        if i < num_hmetrics {
            out.extend(advances[i].to_be_bytes());
        }
        out.extend(lsb[i].to_be_bytes());
    }
    Ok(out)
}

pub fn woff2_to_sfnt(b: &[u8]) -> Result<Vec<u8>, String> {
    let flavor = be32(b, 4).ok_or("woff2: file too short")?;
    let count = be16(b, 12).ok_or("woff2: file too short")? as usize;
    let comp_size = be32(b, 20).ok_or("woff2: file too short")? as usize;
    let mut c = Cursor::new(b);
    c.at = 48;
    struct Entry {
        tag: [u8; 4],
        orig: usize,
        len: usize,
        transformed: bool,
    }
    let mut entries = Vec::new();
    for _ in 0..count {
        let flags = c.u8()?;
        let idx = flags & 0x3F;
        let tag: [u8; 4] = if idx == 63 { c.take(4)?.try_into().unwrap() } else { **KNOWN_TAGS.get(idx as usize).ok_or("woff2: unknown table number")? };
        let version = flags >> 6;
        let orig = c.base128()? as usize;
        let is_glyf_loca = &tag == b"glyf" || &tag == b"loca";
        let transformed = if is_glyf_loca { version == 0 } else { version != 0 };
        let len = if transformed { c.base128()? as usize } else { orig };
        if orig > 256 << 20 || len > 256 << 20 {
            return Err("woff2: a table claims to be larger than 256 MB".into());
        }
        entries.push(Entry { tag, orig, len, transformed });
    }
    // TrueType collections inside a WOFF2 are not unpacked.
    if flavor == 0x7474_6366 {
        return Err("woff2: a font collection inside a WOFF2 file is not supported".into());
    }
    let comp = b.get(c.at..c.at.checked_add(comp_size).ok_or("woff2: bad size")?).ok_or("woff2: compressed data cut off")?;
    let total: usize = entries.iter().map(|e| e.len).sum();
    let mut stream = Vec::with_capacity(total);
    brotli_decompressor::Decompressor::new(comp, 1 << 16).take(total as u64 + 1).read_to_end(&mut stream).map_err(|e| format!("woff2: brotli: {e}"))?;
    if stream.len() != total {
        return Err("woff2: the tables did not unpack to their stated size".into());
    }
    let mut at = 0usize;
    let mut tables: Vec<Table> = Vec::new();
    let mut by_tag: std::collections::HashMap<[u8; 4], usize> = Default::default();
    let mut transformed_glyf: Option<Vec<u8>> = None;
    let mut transformed_hmtx: Option<Vec<u8>> = None;
    for e in &entries {
        let data = stream[at..at + e.len].to_vec();
        at += e.len;
        if e.transformed && &e.tag == b"glyf" {
            transformed_glyf = Some(data);
            continue;
        }
        if e.transformed && &e.tag == b"loca" {
            continue; // rebuilt together with glyf
        }
        if e.transformed && &e.tag == b"hmtx" {
            transformed_hmtx = Some(data);
            continue;
        }
        if e.transformed {
            return Err(format!("woff2: unknown transform of the {} table", String::from_utf8_lossy(&e.tag)));
        }
        let _ = e.orig;
        by_tag.insert(e.tag, tables.len());
        tables.push(Table { tag: e.tag, data });
    }
    let mut xmins = Vec::new();
    if let Some(t) = transformed_glyf {
        let (glyf, loca, xm) = rebuild_glyf(&t)?;
        xmins = xm;
        tables.push(Table { tag: *b"glyf", data: glyf });
        tables.push(Table { tag: *b"loca", data: loca });
    }
    if let Some(t) = transformed_hmtx {
        let num_glyphs = tables.iter().find(|t| &t.tag == b"maxp").and_then(|m| be16(&m.data, 4)).ok_or("woff2: no maxp table to rebuild hmtx from")? as usize;
        let num_hm = tables.iter().find(|t| &t.tag == b"hhea").and_then(|m| be16(&m.data, 34)).ok_or("woff2: no hhea table to rebuild hmtx from")? as usize;
        tables.push(Table { tag: *b"hmtx", data: rebuild_hmtx(&t, num_glyphs, num_hm, &xmins)? });
    }
    Ok(build_sfnt(flavor, tables))
}

// ---- EOT ----------------------------------------------------------------------------------------------------------------

pub fn is_eot(b: &[u8]) -> bool {
    b.len() > 82 && b.get(34..36) == Some(&[0x4C, 0x50]) && b.get(8..12).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]])).is_some_and(|v| matches!(v, 0x0001_0000 | 0x0002_0001 | 0x0002_0002))
}

pub fn eot_to_sfnt(b: &[u8]) -> Result<Vec<u8>, String> {
    let le32 = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    let (size, data_size, flags) = (le32(0).ok_or("eot: file too short")? as usize, le32(4).ok_or("eot: file too short")? as usize, le32(12).ok_or("eot: file too short")?);
    if flags & 0x4 != 0 {
        return Err("eot: this font is MicroType Express compressed, which is not read (convert it with a font tool)".into());
    }
    let size = size.min(b.len());
    let start = size.checked_sub(data_size).ok_or("eot: bad sizes")?;
    let mut data = b.get(start..size).ok_or("eot: font data cut off")?.to_vec();
    if flags & 0x1000_0000 != 0 {
        data.iter_mut().for_each(|v| *v ^= 0x50);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny font: the tables are made up, only the container matters here.
    fn fake_tables() -> Vec<Table> {
        vec![Table { tag: *b"head", data: vec![1; 54] }, Table { tag: *b"cmap", data: vec![2; 20] }]
    }

    #[test]
    fn woff_round_trip() {
        use std::io::Write;
        let tables = fake_tables();
        let mut out = Vec::new();
        out.extend(b"wOFF");
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend(0u32.to_be_bytes());
        out.extend((tables.len() as u16).to_be_bytes());
        out.extend([0u8; 2]);
        out.extend([0u8; 4 + 2 + 2 + 5 * 4]);
        let mut dir = Vec::new();
        let mut body = Vec::new();
        let base = 44 + tables.len() * 20;
        for (i, t) in tables.iter().enumerate() {
            let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(&t.data).unwrap();
            let zd = z.finish().unwrap();
            let (store, comp) = if zd.len() < t.data.len() { (zd.clone(), zd.len()) } else { (t.data.clone(), t.data.len()) };
            dir.extend(t.tag);
            dir.extend(((base + body.len()) as u32).to_be_bytes());
            dir.extend((comp as u32).to_be_bytes());
            dir.extend((t.data.len() as u32).to_be_bytes());
            dir.extend(0u32.to_be_bytes());
            body.extend(store);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            let _ = i;
        }
        out.extend(dir);
        out.extend(body);
        let sfnt = woff_to_sfnt(&out).unwrap();
        assert_eq!(&sfnt[..4], &[0, 1, 0, 0]);
        assert_eq!(be16(&sfnt, 4), Some(2));
        // The directory is sorted by tag: cmap first, 20 bytes of 2s.
        assert_eq!(&sfnt[12..16], b"cmap");
        let off = be32(&sfnt, 12 + 8).unwrap() as usize;
        assert_eq!(&sfnt[off..off + 20], &[2u8; 20][..]);
        assert!(woff_to_sfnt(&out[..60]).is_err());
    }

    #[test]
    fn eot_xor_and_compressed() {
        let font = vec![0, 1, 0, 0, 9, 9, 9, 9];
        let mut e = vec![0u8; 82];
        let total = (82 + font.len()) as u32;
        e[0..4].copy_from_slice(&total.to_le_bytes());
        e[4..8].copy_from_slice(&(font.len() as u32).to_le_bytes());
        e[8..12].copy_from_slice(&0x0002_0001u32.to_le_bytes());
        e[12..16].copy_from_slice(&0x1000_0000u32.to_le_bytes());
        e[34] = 0x4C;
        e[35] = 0x50;
        e.extend(font.iter().map(|b| b ^ 0x50));
        assert!(is_eot(&e));
        assert_eq!(eot_to_sfnt(&e).unwrap(), font);
        e[12..16].copy_from_slice(&0x4u32.to_le_bytes());
        assert!(eot_to_sfnt(&e).unwrap_err().contains("MicroType"));
    }

    #[test]
    fn triplets_and_numbers() {
        let mut c = Cursor::new(&[0x81, 0x00, 253, 1, 2, 255, 7, 254, 3]);
        assert_eq!(c.base128().unwrap(), 128);
        assert_eq!(c.u255().unwrap(), 0x0102);
        assert_eq!(c.u255().unwrap(), 253 + 7);
        assert_eq!(c.u255().unwrap(), 506 + 3);
        assert_eq!(with_sign(1, 5), 5);
        assert_eq!(with_sign(2, 5), -5);
    }
}
