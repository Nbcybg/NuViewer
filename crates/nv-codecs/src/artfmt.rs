//! Drawing-program and 3D-content files that keep a picture of their own inside, or a description worth a card:
//! PaintTool SAI (`sai`, see `sai.rs`), Clip Studio Paint (`clip`: the canvas preview in the SQLite database inside),
//! openCanvas (`oci`: a card, the picture data is a closed compressed format), Daz Studio (`duf`: the thumbnail next to it,
//! else a card made from the JSON) and `lip` (whatever picture is stored in it).

use crate::docthumb::{accent_for, fmt_bytes, render_card, Card};
use crate::Image;
use std::path::Path;

pub const EXTS: &[&str] = &["sai", "clip", "lip", "oci", "duf"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// `None` when `ext` is not one of ours.
pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    Some(match ext {
        "sai" => crate::progress::read_file(path).and_then(|b| crate::sai::decode(&b, first_only)),
        "clip" => clip(path),
        "lip" => lip(path, first_only),
        "oci" => oci(path, first_only),
        "duf" => duf(path, first_only),
        _ => return None,
    })
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn card(path: &Path, ext: &str, subtitle: String, lines: Vec<String>, first_only: bool, note: &str) -> Image {
    let mut img = render_card(&Card { badge: ext.to_uppercase(), accent: accent_for(ext), title: name_of(path), subtitle, lines }, if first_only { 1 } else { 2 });
    img.colour_note = Some(note.to_string());
    img
}

// ---- Clip Studio Paint ----------------------------------------------------------------------------------------------

/// Clip Studio Paint: `CSFCHUNK`, then chunks (tag 8 bytes, size u64 big-endian, data). `CHNKSQLi` is a SQLite database
/// whose `CanvasPreview` table holds the preview PNG. The canvas itself is in the huge `CHNKExta` chunks, in the program's
/// own block format, which is not read: the preview is what is shown.
fn clip(path: &Path) -> Result<Image, String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| format!("read: {e}"))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = [0u8; 24];
    f.read_exact(&mut head).map_err(|_| "clip: file too short".to_string())?;
    if &head[..8] != b"CSFCHUNK" {
        return Err("clip: not a Clip Studio Paint file (no CSFCHUNK header)".into());
    }
    let mut db: Option<Vec<u8>> = None;
    let mut pos = 24u64;
    while pos + 16 <= len {
        f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        let mut h = [0u8; 16];
        if f.read_exact(&mut h).is_err() {
            break;
        }
        let size = u64::from_be_bytes(h[8..16].try_into().unwrap());
        if &h[..8] == b"CHNKSQLi" {
            if size > 1 << 30 || pos + 16 + size > len {
                break;
            }
            let mut buf = vec![0u8; size as usize];
            f.read_exact(&mut buf).map_err(|e| format!("read: {e}"))?;
            db = Some(buf);
            break;
        }
        if &h[..4] != b"CHNK" || &h[..8] == b"CHNKFoot" {
            break;
        }
        pos += 16 + size;
    }
    // A layout this walk does not know: look for the database by its signature.
    if db.is_none() && len < 1 << 29 {
        let all = crate::progress::read_file(path)?;
        if let Some(at) = all.windows(16).position(|w| w == b"SQLite format 3\0") {
            db = Some(all[at..].to_vec());
        }
    }
    let db = db.ok_or("clip: no preview database inside this Clip Studio file")?;
    let png = sqlite_png(&db).ok_or("clip: this file stores no canvas preview")?;
    let mut img = crate::decode_image_bytes(&png, "png")?;
    img.colour_note = Some("the canvas preview stored in this Clip Studio Paint file (the layers are in the program's own format)".into());
    img.profile_assumable = true;
    Ok(img)
}

fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..9 {
        let c = *b.get(*at)?;
        *at += 1;
        if i == 8 {
            return Some((v << 8) | c as u64);
        }
        v = (v << 7) | (c & 0x7F) as u64;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

struct Db<'a> {
    d: &'a [u8],
    page: usize,
    usable: usize,
}

impl<'a> Db<'a> {
    fn open(d: &'a [u8]) -> Option<Db<'a>> {
        if !d.starts_with(b"SQLite format 3\0") {
            return None;
        }
        let ps = u16::from_be_bytes([d[16], d[17]]) as usize;
        let page = if ps == 1 { 65536 } else { ps };
        if !page.is_power_of_two() || page < 512 {
            return None;
        }
        Some(Db { d, page, usable: page - *d.get(20)? as usize })
    }

    fn page_bytes(&self, n: usize) -> Option<&'a [u8]> {
        self.d.get((n.checked_sub(1)?) * self.page..n * self.page)
    }

    /// Every row of a table b-tree: the record's columns as raw values.
    fn rows(&self, root: usize, limit: usize) -> Vec<Vec<Val>> {
        let mut out = Vec::new();
        self.walk(root, &mut out, limit, 0);
        out
    }

    fn walk(&self, n: usize, out: &mut Vec<Vec<Val>>, limit: usize, depth: usize) {
        if depth > 12 || out.len() >= limit {
            return;
        }
        let Some(p) = self.page_bytes(n) else { return };
        let h = if n == 1 { 100 } else { 0 };
        let kind = p[h];
        let cells = u16::from_be_bytes([p[h + 3], p[h + 4]]) as usize;
        let ptrs = h + if kind == 5 { 12 } else { 8 };
        for i in 0..cells {
            let Some(o) = p.get(ptrs + i * 2..ptrs + i * 2 + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize) else { return };
            if kind == 5 {
                if let Some(c) = p.get(o..o + 4) {
                    self.walk(u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as usize, out, limit, depth + 1);
                }
            } else if kind == 13 {
                if let Some(row) = self.leaf_cell(p, o) {
                    out.push(row);
                }
            }
            if out.len() >= limit {
                return;
            }
        }
        if kind == 5 {
            if let Some(c) = p.get(h + 8..h + 12) {
                self.walk(u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as usize, out, limit, depth + 1);
            }
        }
    }

    /// The payload of a leaf cell (local part plus the overflow pages), parsed into values.
    fn leaf_cell(&self, p: &[u8], mut at: usize) -> Option<Vec<Val>> {
        let total = varint(p, &mut at)? as usize;
        let _rowid = varint(p, &mut at)?;
        let u = self.usable;
        let max_local = u - 35;
        let local = if total <= max_local {
            total
        } else {
            let min_local = (u - 12) * 32 / 255 - 23;
            let k = min_local + (total - min_local) % (u - 4);
            if k <= max_local { k } else { min_local }
        };
        let first = p.get(at..at + local)?;
        if local == total {
            return record_owned(first);
        }
        let mut payload = first.to_vec();
        let mut next = u32::from_be_bytes(p.get(at + local..at + local + 4)?.try_into().ok()?) as usize;
        let mut guard = 0;
        while next != 0 && payload.len() < total && guard < 1 << 20 {
            guard += 1;
            let op = self.page_bytes(next)?;
            let take = (total - payload.len()).min(u - 4);
            payload.extend_from_slice(op.get(4..4 + take)?);
            next = u32::from_be_bytes([op[0], op[1], op[2], op[3]]) as usize;
        }
        if payload.len() < total {
            return None;
        }
        record_owned(&payload)
    }
}

#[derive(Clone, Debug)]
enum Val {
    Null,
    Int(i64),
    #[allow(dead_code)]
    Float(f64),
    Text(String),
    Blob(Vec<u8>),
}

fn record_owned(b: &[u8]) -> Option<Vec<Val>> {
    let mut at = 0;
    let hlen = varint(b, &mut at)? as usize;
    let mut types = Vec::new();
    while at < hlen {
        types.push(varint(b, &mut at)?);
    }
    let mut pos = hlen;
    let mut out = Vec::with_capacity(types.len());
    for t in types {
        let n = match t {
            0 | 8 | 9 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 6,
            6 | 7 => 8,
            t if t >= 12 && t % 2 == 0 => ((t - 12) / 2) as usize,
            t if t >= 13 => ((t - 13) / 2) as usize,
            _ => return None,
        };
        let s = b.get(pos..pos + n)?;
        pos += n;
        out.push(match t {
            0 => Val::Null,
            8 => Val::Int(0),
            9 => Val::Int(1),
            1..=6 => {
                let mut v: i64 = if s[0] & 0x80 != 0 { -1 } else { 0 };
                for &c in s {
                    v = (v << 8) | c as i64;
                }
                Val::Int(v)
            }
            7 => Val::Float(f64::from_bits(u64::from_be_bytes(s.try_into().ok()?))),
            t if t % 2 == 0 => Val::Blob(s.to_vec()),
            _ => Val::Text(String::from_utf8_lossy(s).into_owned()),
        });
    }
    Some(out)
}

/// The largest PNG stored in a `CanvasPreview`-like table of the database (any table, if that one is missing).
fn sqlite_png(d: &[u8]) -> Option<Vec<u8>> {
    let db = Db::open(d)?;
    let master = db.rows(1, 4096);
    // (type, name, tbl_name, rootpage, sql)
    let mut roots: Vec<(String, usize)> = master
        .iter()
        .filter_map(|r| match (r.first(), r.get(1), r.get(3)) {
            (Some(Val::Text(t)), Some(Val::Text(name)), Some(Val::Int(root))) if t == "table" => Some((name.clone(), *root as usize)),
            _ => None,
        })
        .collect();
    // The preview table first.
    roots.sort_by_key(|(n, _)| !n.eq_ignore_ascii_case("CanvasPreview"));
    let mut best: Option<Vec<u8>> = None;
    for (name, root) in roots.iter().take(40) {
        let preview = name.eq_ignore_ascii_case("CanvasPreview");
        for row in db.rows(*root, if preview { 64 } else { 16 }) {
            for v in row {
                if let Val::Blob(b) = v {
                    if b.starts_with(&[0x89, b'P', b'N', b'G']) && best.as_ref().is_none_or(|x| b.len() > x.len()) {
                        best = Some(b);
                    }
                }
            }
        }
        if preview && best.is_some() {
            break;
        }
    }
    best
}

// ---- generic: the picture stored inside ----------------------------------------------------------------------------

fn lip(path: &Path, first_only: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    if let Some(mut img) = crate::model::more::embedded_picture(&bytes) {
        img.colour_note = Some("the biggest picture stored inside this .lip file".into());
        return Ok(img);
    }
    Ok(card(path, "lip", fmt_bytes(bytes.len() as u64), Vec::new(), first_only, "a card: this .lip file stores no picture NuViewer can find"))
}

// ---- openCanvas -----------------------------------------------------------------------------------------------------

fn oci(path: &Path, first_only: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    let (w, h) = if bytes.starts_with(b"OPIM") && bytes.len() > 16 { (u32::from_le_bytes(bytes[8..12].try_into().unwrap()), u32::from_le_bytes(bytes[12..16].try_into().unwrap())) } else { (0, 0) };
    if w == 0 || h == 0 || w > 100_000 || h > 100_000 {
        return Err("oci: not an openCanvas picture (no OPIM header)".into());
    }
    let lines = vec!["openCanvas picture".to_string(), format!("{w} x {h} px"), String::new(), "The pixels are stored in".to_string(), "openCanvas's own compressed format,".to_string(), "which nothing outside it reads.".to_string(), "Export a PNG from openCanvas".to_string(), "to see the picture.".to_string()];
    Ok(card(path, "oci", format!("{} · {w} x {h} px", fmt_bytes(bytes.len() as u64)), lines, first_only, "a card: openCanvas's compressed picture data cannot be read, only its size"))
}

// ---- Daz Studio -----------------------------------------------------------------------------------------------------

fn duf(path: &Path, first_only: bool) -> Result<Image, String> {
    // Daz keeps the thumbnail next to the file: `name.duf.png` (or `name.png`).
    let name = name_of(path);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    for cand in [format!("{name}.png"), format!("{stem}.png"), format!("{stem}.tip.png"), format!("{name}.jpg"), format!("{stem}.jpg")] {
        let p = dir.join(&cand);
        if p.is_file() {
            if let Ok(bytes) = std::fs::read(&p) {
                if let Ok(mut img) = crate::decode_image_bytes(&bytes, "png") {
                    img.colour_note = Some(format!("the thumbnail {cand} that Daz Studio keeps next to this file"));
                    img.profile_assumable = true;
                    return Ok(img);
                }
            }
        }
    }
    let raw = crate::progress::read_file(path)?;
    let json_bytes = if raw.starts_with(&[0x1F, 0x8B]) {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(flate2::read::GzDecoder::new(&raw[..]), 512 << 20), &mut out).map_err(|e| format!("duf: the compressed data is damaged ({e})"))?;
        out
    } else {
        raw.clone()
    };
    let v: serde_json::Value = serde_json::from_slice(&json_bytes).map_err(|e| format!("duf: not a Daz scene description ({e})"))?;
    let s = |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).unwrap_or("").to_string();
    let info = v.get("asset_info");
    let count = |k: &str| v.get(k).and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
    let mut lines = vec![format!("Daz Studio {}", { let t = s(info.and_then(|i| i.get("type"))); if t.is_empty() { "file".to_string() } else { t } })];
    let author = s(info.and_then(|i| i.get("contributor")).and_then(|c| c.get("author")));
    if !author.is_empty() {
        lines.push(format!("by {author}"));
    }
    let modified = s(info.and_then(|i| i.get("modified")));
    if !modified.is_empty() {
        lines.push(format!("modified {}", modified.split('T').next().unwrap_or(&modified)));
    }
    lines.push(String::new());
    for (k, label) in [("geometry_library", "Geometries"), ("node_library", "Nodes"), ("modifier_library", "Modifiers"), ("material_library", "Materials"), ("image_library", "Images"), ("uv_set_library", "UV sets")] {
        let n = count(k);
        if n > 0 {
            lines.push(format!("{label}: {n}"));
        }
    }
    if let Some(nodes) = v.get("scene").and_then(|s| s.get("nodes")).and_then(|n| n.as_array()) {
        lines.push(String::new());
        lines.push(format!("{} node{} in the scene", nodes.len(), if nodes.len() == 1 { "" } else { "s" }));
        for n in nodes.iter().take(5) {
            let id = s(n.get("url")).trim_start_matches('#').to_string();
            if !id.is_empty() {
                lines.push(format!("  {id}"));
            }
        }
    }
    Ok(card(path, "duf", fmt_bytes(raw.len() as u64), lines, first_only, "a card made from the Daz scene description (no thumbnail next to the file)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_varints_and_records() {
        let mut at = 0;
        assert_eq!(varint(&[0x81, 0x00], &mut at), Some(128));
        let mut at = 0;
        assert_eq!(varint(&[0x7F], &mut at), Some(127));
        // record: header (len 3: types 1 = int8, 17 = text of 2), values 5, "hi"
        let r = record_owned(&[3, 1, 17, 5, b'h', b'i']).unwrap();
        assert!(matches!(r[0], Val::Int(5)));
        assert!(matches!(&r[1], Val::Text(t) if t == "hi"));
    }

    #[test]
    fn a_non_sqlite_blob_gives_no_preview() {
        assert!(sqlite_png(b"nothing here").is_none());
    }
}
