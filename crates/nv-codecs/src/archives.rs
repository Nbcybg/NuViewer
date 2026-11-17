//! What is inside an archive, for the viewer and the gallery's thumbnails, read from the file's own table of contents without
//! unpacking (the gallery unpacks and browses through 7-Zip / Windows' tar): tar (also inside gzip and zstd), gzip, CAB, ISO 9660
//! (+ Joliet, El Torito, UDF flag), Apple DMG (partitions, compression, size), cpio, ar / deb, RPM, WIM, ARJ, LZH / LHA, RAR 4 / 5,
//! and the header facts of 7z, xz, bzip2, zstd, lz4, lzip, lzma and compress (`.Z`) files. Also `pak` game packs (Quake / Sin,
//! Chromium resource packs, Unreal Engine).
//!
//! The thumbnail is a compact card; the viewer gets a page with the whole listing (up to 70 entries).

use crate::docthumb::{accent_for, fmt_bytes, render_card, Card};
use crate::Image;
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub dir: bool,
    /// Where the data starts in the (decompressed) stream: only tracked by the tar reader.
    pos: u64,
}

#[derive(Default)]
pub struct Info {
    pub kind: String,
    pub facts: Vec<String>,
    pub entries: Vec<Entry>,
    /// The number of entries in the archive when it is known (else only the listed ones are counted).
    pub total: Option<u64>,
    /// The listing stops early (a limit was reached, or the archive is cut).
    pub more: bool,
    /// A picture found inside (a resource pack's PNG, a game pack's texture): shown instead of the listing.
    pub picture: Option<Image>,
}

fn le16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn le32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn le64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}
fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}
fn be64(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_be_bytes(s.try_into().unwrap()))
}

/// Text of a name field: UTF-8 when it is, else one byte per character (the old code pages mostly agree with Latin-1 for ASCII).
fn text(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|&c| c as char).collect(),
    }
}

fn cstr(b: &[u8]) -> String {
    text(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())])
}

fn read_at(f: &mut File, off: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut v = Vec::new();
    f.take(len as u64).read_to_end(&mut v).ok()?;
    Some(v)
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---- sources the tar reader can walk -----------------------------------------------------------------------------------------

trait Src: Read {
    fn skip(&mut self, n: u64) -> io::Result<()>;
}

impl Src for BufReader<File> {
    fn skip(&mut self, n: u64) -> io::Result<()> {
        self.seek_relative(n as i64)
    }
}

impl Src for Cursor<Vec<u8>> {
    fn skip(&mut self, n: u64) -> io::Result<()> {
        self.set_position(self.position() + n);
        Ok(())
    }
}

/// A decompressor: skipping is reading and throwing away, and at most `left` bytes are ever produced (a listing, not an unpack).
struct Capped<R: Read> {
    r: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.left as usize);
        if n == 0 {
            return Ok(0);
        }
        let got = self.r.read(&mut buf[..n])?;
        self.left -= got as u64;
        Ok(got)
    }
}

impl<R: Read> Src for Capped<R> {
    fn skip(&mut self, n: u64) -> io::Result<()> {
        let copied = io::copy(&mut self.by_ref().take(n), &mut io::sink())?;
        if copied < n {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(())
    }
}

// ---- tar --------------------------------------------------------------------------------------------------------------------

fn octal(b: &[u8]) -> Option<u64> {
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        // GNU base-256 for sizes that do not fit in octal.
        let mut v = (b[0] & 0x7F) as u64;
        for &c in &b[1..] {
            v = v.checked_mul(256)?.checked_add(c as u64)?;
        }
        return Some(v);
    }
    let s: String = b.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    let s = s.trim();
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s, 8).ok()
}

fn tar_header_ok(h: &[u8]) -> bool {
    if h.len() < 512 {
        return false;
    }
    let stored = octal(&h[148..156]);
    let sum: u64 = h.iter().enumerate().map(|(i, &c)| if (148..156).contains(&i) { 32 } else { c as u64 }).sum();
    stored == Some(sum)
}

/// The entries of a tar stream, at most `max_entries`. Returns them and whether the listing is complete.
fn tar_list<S: Src>(s: &mut S, max_entries: usize) -> (Vec<Entry>, bool) {
    let mut entries = Vec::new();
    let mut pos = 0u64;
    let mut long_name: Option<String> = None;
    let mut pax_size: Option<u64> = None;
    loop {
        if entries.len() >= max_entries {
            return (entries, false);
        }
        let mut h = [0u8; 512];
        if s.read_exact(&mut h).is_err() {
            return (entries, false);
        }
        pos += 512;
        if h.iter().all(|&c| c == 0) {
            return (entries, true);
        }
        if !tar_header_ok(&h) {
            let empty = entries.is_empty();
            return (entries, empty);
        }
        let kind = h[156];
        let size = pax_size.take().or_else(|| octal(&h[124..136])).unwrap_or(0);
        let padded = size.div_ceil(512) * 512;
        match kind {
            b'L' | b'x' | b'g' => {
                if size > 1 << 20 {
                    return (entries, false);
                }
                let mut data = vec![0u8; size as usize];
                if s.read_exact(&mut data).is_err() || s.skip(padded - size).is_err() {
                    return (entries, false);
                }
                pos += padded;
                if kind == b'L' {
                    long_name = Some(cstr(&data));
                } else if kind == b'x' {
                    for rec in text(&data).split('\n') {
                        if let Some((_, kv)) = rec.split_once(' ') {
                            if let Some(v) = kv.strip_prefix("path=") {
                                long_name = Some(v.to_string());
                            } else if let Some(v) = kv.strip_prefix("size=") {
                                pax_size = v.parse().ok();
                            }
                        }
                    }
                }
                continue;
            }
            _ => {}
        }
        let mut name = long_name.take().unwrap_or_else(|| {
            let n = cstr(&h[0..100]);
            if &h[257..262] == b"ustar" && h[345] != 0 {
                format!("{}/{}", cstr(&h[345..500]), n)
            } else {
                n
            }
        });
        let dir = kind == b'5' || name.ends_with('/');
        if kind == b'2' || kind == b'1' {
            name = format!("{name} → {}", cstr(&h[157..257]));
        }
        entries.push(Entry { name: name.trim_start_matches("./").to_string(), size: if matches!(kind, b'0' | 0 | b'7') { size } else { 0 }, dir, pos });
        if s.skip(padded).is_err() {
            return (entries, false);
        }
        pos += padded;
    }
}

fn tar_info(kind: &str, entries: Vec<Entry>, complete: bool, how: &str) -> Info {
    let files = entries.iter().filter(|e| !e.dir).count() as u64;
    let bytes: u64 = entries.iter().map(|e| e.size).sum();
    let mut facts = vec![format!("{}, {}{}", plural(files, "file", "files"), fmt_bytes(bytes), if complete { String::new() } else { " (the first entries only)".to_string() })];
    if !how.is_empty() {
        facts.insert(0, how.to_string());
    }
    Info { kind: kind.to_string(), facts, total: complete.then_some(entries.len() as u64), more: !complete, entries, picture: None }
}

fn plain_tar(f: &mut File, thumb: bool) -> Option<Info> {
    f.seek(SeekFrom::Start(0)).ok()?;
    let mut first = [0u8; 512];
    f.read_exact(&mut first).ok()?;
    if !tar_header_ok(&first) {
        return None;
    }
    f.seek(SeekFrom::Start(0)).ok()?;
    let mut r = BufReader::new(f.try_clone().ok()?);
    let (entries, complete) = tar_list(&mut r, if thumb { 2000 } else { 20000 });
    Some(tar_info("tar archive", entries, complete, "Tape archive (tar)"))
}

// ---- gzip, zstd, xz, bzip2 and friends ---------------------------------------------------------------------------------------

fn gzip(f: &mut File, len: u64, head: &[u8], thumb: bool) -> Option<Info> {
    let flg = *head.get(3)?;
    let mtime = le32(head, 4)?;
    let mut pos = 10usize;
    if flg & 4 != 0 {
        pos += 2 + le16(head, pos)? as usize;
    }
    let stored_name = if flg & 8 != 0 { head.get(pos..).map(cstr).filter(|n| !n.is_empty()) } else { None };
    let tail = read_at(f, len.saturating_sub(4), 4)?;
    let isize = le32(&tail, 0).unwrap_or(0) as u64;
    f.seek(SeekFrom::Start(0)).ok()?;
    let limit = if thumb { 4 << 20 } else { 64 << 20 };
    let mut dec = Capped { r: flate2::read::GzDecoder::new(BufReader::new(f.try_clone().ok()?)), left: limit };
    let mut first = [0u8; 512];
    let is_tar = dec.read_exact(&mut first).is_ok() && tar_header_ok(&first);
    let mut facts = Vec::new();
    if let Some(n) = &stored_name {
        facts.push(format!("Packed from: {n}"));
    }
    if mtime != 0 {
        facts.push(format!("Packed on {}", unix_date(mtime as u64)));
    }
    if is_tar {
        f.seek(SeekFrom::Start(0)).ok()?;
        let mut dec = Capped { r: flate2::read::GzDecoder::new(BufReader::new(f.try_clone().ok()?)), left: limit };
        let (entries, complete) = tar_list(&mut dec, if thumb { 1000 } else { 5000 });
        let mut info = tar_info("gzip-compressed tar archive", entries, complete, "tar archive compressed with gzip");
        if complete {
            info.facts.push(format!("Unpacked about {} ({:.1}x smaller)", fmt_bytes(isize), isize as f64 / len.max(1) as f64));
        }
        info.facts.extend(facts);
        return Some(info);
    }
    facts.insert(0, format!("Unpacks to {}", fmt_bytes(isize)));
    let name = stored_name.unwrap_or_else(|| "(the packed file)".to_string());
    Some(Info { kind: "gzip file".into(), facts, entries: vec![Entry { name, size: isize, ..Default::default() }], total: Some(1), ..Default::default() })
}

fn unix_date(t: u64) -> String {
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let days = (t / 86400) as i64;
    let secs = t % 86400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y + (m <= 2) as i64, m, d, secs / 3600, secs % 3600 / 60)
}

fn zstd(f: &mut File, len: u64, head: &[u8], thumb: bool, ext: &str) -> Option<Info> {
    let fhd = *head.get(4)?;
    let single = fhd & 0x20 != 0;
    let mut p = 5 + (!single) as usize;
    p += [0, 1, 2, 4][(fhd & 3) as usize];
    let fcs = match fhd >> 6 {
        0 if single => head.get(p).map(|&b| b as u64),
        0 => None,
        1 => le16(head, p).map(|v| v as u64 + 256),
        2 => le32(head, p).map(|v| v as u64),
        _ => le64(head, p),
    };
    f.seek(SeekFrom::Start(0)).ok()?;
    let mut facts = vec!["Zstandard compressed".to_string()];
    if let Some(s) = fcs {
        facts.push(format!("Unpacks to {} ({:.1}x smaller)", fmt_bytes(s), s as f64 / len.max(1) as f64));
    }
    let limit = if thumb { 4 << 20 } else { 32 << 20 };
    if let Ok(d) = ruzstd::decoding::StreamingDecoder::new(BufReader::new(f.try_clone().ok()?)) {
        let mut d = Capped { r: d, left: limit };
        let mut first = [0u8; 512];
        if d.read_exact(&mut first).is_ok() && tar_header_ok(&first) {
            f.seek(SeekFrom::Start(0)).ok()?;
            if let Ok(d) = ruzstd::decoding::StreamingDecoder::new(BufReader::new(f.try_clone().ok()?)) {
                let mut d = Capped { r: d, left: limit };
                let (entries, complete) = tar_list(&mut d, if thumb { 1000 } else { 5000 });
                let mut info = tar_info("Zstandard-compressed tar archive", entries, complete, "tar archive compressed with Zstandard");
                info.facts.extend(facts.into_iter().skip(1));
                return Some(info);
            }
        }
    }
    let _ = ext;
    Some(Info { kind: "Zstandard file".into(), facts, ..Default::default() })
}

fn vli(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..9 {
        let c = *b.get(*at)?;
        *at += 1;
        v |= ((c & 0x7F) as u64) << (7 * i);
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

fn xz(f: &mut File, len: u64, head: &[u8]) -> Option<Info> {
    let check = match head.get(7)? & 0x0F {
        0 => "no integrity check",
        1 => "CRC32",
        4 => "CRC64",
        10 => "SHA-256",
        _ => "an unusual check",
    };
    let mut facts = vec![format!("XZ (LZMA2) compressed, {check}")];
    // The index at the end lists every block's unpacked size: the total without decompressing.
    if len > 32 {
        if let Some(foot) = read_at(f, len - 12, 12) {
            if foot.get(10..12) == Some(b"YZ") {
                let back = (le32(&foot, 4)? as u64 + 1) * 4;
                if back < len {
                    if let Some(idx) = read_at(f, len - 12 - back, back as usize) {
                        let mut at = 1usize;
                        if idx.first() == Some(&0) {
                            let n = vli(&idx, &mut at)?;
                            let mut total = 0u64;
                            for _ in 0..n.min(100_000) {
                                vli(&idx, &mut at)?;
                                total += vli(&idx, &mut at)?;
                            }
                            facts.push(format!("Unpacks to {} ({:.1}x smaller), {}", fmt_bytes(total), total as f64 / len.max(1) as f64, plural(n, "block", "blocks")));
                        }
                    }
                }
            }
        }
    }
    Some(Info { kind: "xz file".into(), facts, ..Default::default() })
}

fn simple_compressed(f: &mut File, len: u64, head: &[u8], ext: &str) -> Option<Info> {
    if head.starts_with(b"BZh") {
        let level = head.get(3).filter(|c| c.is_ascii_digit()).map(|c| (c - b'0') as u32 * 100)?;
        return Some(Info { kind: "bzip2 file".into(), facts: vec![format!("bzip2 compressed, blocks of {level} KB")], ..Default::default() });
    }
    if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        let (flg, bd) = (*head.get(4)?, *head.get(5)?);
        let block = match (bd >> 4) & 7 {
            4 => "64 KB",
            5 => "256 KB",
            6 => "1 MB",
            7 => "4 MB",
            _ => "?",
        };
        let mut facts = vec![format!("LZ4 frame, blocks of {block}{}", if flg & 0x10 != 0 { ", block checksums" } else { "" })];
        if flg & 0x08 != 0 {
            if let Some(s) = le64(head, 6) {
                facts.push(format!("Unpacks to {}", fmt_bytes(s)));
            }
        }
        return Some(Info { kind: "LZ4 file".into(), facts, ..Default::default() });
    }
    if head.starts_with(b"LZIP") {
        let dict = 1u64 << (head.get(5)? & 0x1F);
        let mut facts = vec![format!("lzip compressed, dictionary {}", fmt_bytes(dict))];
        if len > 20 {
            if let Some(foot) = read_at(f, len - 20, 20) {
                if let Some(s) = le64(&foot, 4) {
                    facts.push(format!("Unpacks to {}", fmt_bytes(s)));
                }
            }
        }
        return Some(Info { kind: "lzip file".into(), facts, ..Default::default() });
    }
    if head.starts_with(&[0x1F, 0x9D]) {
        let flags = *head.get(2)?;
        return Some(Info { kind: "compress (.Z) file".into(), facts: vec![format!("Unix compress, up to {} bits per code", flags & 0x1F)], ..Default::default() });
    }
    // Raw LZMA ("LZMA alone"): a properties byte below 225, the dictionary size, the unpacked size (or unknown).
    if matches!(ext, "lzma" | "tlz") && head.first().is_some_and(|&p| p < 225) {
        let dict = le32(head, 1)? as u64;
        let mut facts = vec![format!("LZMA compressed, dictionary {}", fmt_bytes(dict))];
        if let Some(s) = le64(head, 5).filter(|&s| s != u64::MAX) {
            facts.push(format!("Unpacks to {}", fmt_bytes(s)));
        }
        return Some(Info { kind: "LZMA file".into(), facts, ..Default::default() });
    }
    None
}

// ---- 7z and RAR -------------------------------------------------------------------------------------------------------------

fn sevenz(head: &[u8]) -> Option<Info> {
    let (major, minor) = (*head.get(6)?, *head.get(7)?);
    Some(Info { kind: "7-Zip archive".into(), facts: vec![format!("7z format {major}.{minor}"), "The file names are kept compressed: open it in NuGallery (7-Zip) to browse".into()], ..Default::default() })
}

fn rar(f: &mut File, len: u64, head: &[u8]) -> Option<Info> {
    let v5 = head.get(6..8) == Some(&[1, 0]);
    let mut info = Info { kind: if v5 { "RAR 5 archive".into() } else { "RAR archive".into() }, ..Default::default() };
    let mut pos = if v5 { 8u64 } else { 7 };
    let mut entries = Vec::new();
    let mut guard = 0;
    while pos + 16 < len && entries.len() < 5000 && guard < 20000 {
        guard += 1;
        let buf = read_at(f, pos, 4096)?;
        if v5 {
            let mut at = 4usize; // after the CRC
            let hsize = vint(&buf, &mut at)?;
            let body = at;
            let ty = vint(&buf, &mut at)?;
            let flags = vint(&buf, &mut at)?;
            let extra = if flags & 1 != 0 { vint(&buf, &mut at)? } else { 0 };
            let data = if flags & 2 != 0 { vint(&buf, &mut at)? } else { 0 };
            let _ = extra;
            if ty == 2 || ty == 3 {
                let fflags = vint(&buf, &mut at)?;
                let usize_ = vint(&buf, &mut at)?;
                vint(&buf, &mut at)?;
                if fflags & 2 != 0 {
                    at += 4;
                }
                if fflags & 4 != 0 {
                    at += 4;
                }
                vint(&buf, &mut at)?;
                vint(&buf, &mut at)?;
                let nlen = vint(&buf, &mut at)? as usize;
                let name = text(buf.get(at..at + nlen)?);
                if ty == 2 {
                    entries.push(Entry { name, size: usize_, dir: fflags & 1 != 0, pos: 0 });
                }
            } else if ty == 4 {
                info.facts.push("The file names are encrypted: a password is needed".into());
                break;
            } else if ty == 5 {
                break;
            }
            pos += (body as u64) + hsize + data;
        } else {
            let (ty, flags, hsize) = (*buf.get(2)?, le16(&buf, 3)?, le16(&buf, 5)? as u64);
            if hsize < 7 {
                break;
            }
            let mut add = 0u64;
            if ty == 0x74 {
                let pack = le32(&buf, 7)? as u64;
                let unp = le32(&buf, 11)? as u64;
                let nlen = le16(&buf, 26)? as usize;
                let mut at = 32usize;
                let (mut hp, mut hu) = (0u64, 0u64);
                if flags & 0x100 != 0 {
                    hp = le32(&buf, at)? as u64;
                    hu = le32(&buf, at + 4)? as u64;
                    at += 8;
                }
                let name = text(buf.get(at..at + nlen)?);
                let name = name.split('\0').next().unwrap_or("").to_string();
                add = pack | (hp << 32);
                entries.push(Entry { name: name.replace('\\', "/"), size: unp | (hu << 32), dir: flags & 0xE0 == 0xE0, pos: 0 });
            } else if ty == 0x73 && flags & 0x80 != 0 {
                info.facts.push("The file names are encrypted: a password is needed".into());
                break;
            } else if ty == 0x7B {
                break;
            } else if flags & 0x8000 != 0 {
                add = le32(&buf, 7).unwrap_or(0) as u64;
            }
            pos += hsize + add;
        }
    }
    let files = entries.iter().filter(|e| !e.dir).count() as u64;
    if !entries.is_empty() {
        info.facts.insert(0, format!("{}, {}", plural(files, "file", "files"), fmt_bytes(entries.iter().map(|e| e.size).sum())));
        info.total = Some(entries.len() as u64);
    }
    info.entries = entries;
    Some(info)
}

fn vint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for i in 0..10 {
        let c = *b.get(*at)?;
        *at += 1;
        v |= ((c & 0x7F) as u64) << (7 * i);
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

// ---- CAB --------------------------------------------------------------------------------------------------------------------

fn cab(f: &mut File, len: u64, head: &[u8]) -> Option<Info> {
    let (files_off, minor, major, nfolders, nfiles, flags, set_id, index) = (le32(head, 16)? as u64, *head.get(24)?, *head.get(25)?, le16(head, 26)?, le16(head, 28)?, le16(head, 30)?, le16(head, 32)?, le16(head, 34)?);
    let mut p = 36usize;
    let mut cb_folder = 0usize;
    if flags & 4 != 0 {
        let cb_header = le16(head, 36)? as usize;
        cb_folder = *head.get(38)? as usize;
        p = 40 + cb_header;
    }
    let nul = |b: &[u8]| b.iter().position(|&c| c == 0);
    let (mut prev, mut next) = (None, None);
    if flags & 1 != 0 {
        let n = head.get(p..)?;
        let l = nul(n)?;
        prev = Some(text(&n[..l]));
        p += l + 1;
        p += nul(head.get(p..)?)? + 1;
    }
    if flags & 2 != 0 {
        let n = head.get(p..)?;
        let l = nul(n)?;
        next = Some(text(&n[..l]));
        p += l + 1;
        p += nul(head.get(p..)?)? + 1;
    }
    let folder_table = p;
    let mut kinds = std::collections::BTreeSet::new();
    for i in 0..(nfolders as usize).min(64) {
        if let Some(t) = le16(head, folder_table + i * (8 + cb_folder) + 6) {
            kinds.insert(match t & 0x0F {
                0 => "stored (no compression)",
                1 => "MSZIP",
                2 => "Quantum",
                3 => "LZX",
                _ => "an unknown compression",
            });
        }
    }
    let mut info = Info { kind: "Microsoft Cabinet (CAB)".into(), ..Default::default() };
    info.facts.push(format!("Cabinet version {major}.{minor}, {}, {}", plural(nfolders as u64, "folder", "folders"), plural(nfiles as u64, "file", "files")));
    if !kinds.is_empty() {
        info.facts.push(format!("Compression: {}", kinds.into_iter().collect::<Vec<_>>().join(", ")));
    }
    if flags & 3 != 0 {
        info.facts.push(format!("Part {} of a set (id {set_id}){}{}", index + 1, prev.map(|p| format!(", previous: {p}")).unwrap_or_default(), next.map(|n| format!(", next: {n}")).unwrap_or_default()));
    }
    info.facts.push(format!("Cabinet size {}", fmt_bytes(len)));
    // The file table.
    let want = (nfiles as usize).min(5000);
    let buf = read_at(f, files_off, (want * 64).min(4 << 20))?;
    let mut at = 0usize;
    let mut total = 0u64;
    for _ in 0..want {
        let (size, attribs) = (le32(&buf, at)? as u64, le16(&buf, at + 14)?);
        let rest = buf.get(at + 16..)?;
        let n = rest.iter().position(|&c| c == 0)?;
        let name = if attribs & 0x80 != 0 { text(&rest[..n]) } else { rest[..n].iter().map(|&c| c as char).collect() };
        total += size;
        info.entries.push(Entry { name: name.replace('\\', "/"), size, ..Default::default() });
        at += 16 + n + 1;
    }
    info.total = Some(nfiles as u64);
    info.more = (nfiles as usize) > want;
    info.facts.push(format!("Unpacks to {}", fmt_bytes(total)));
    Some(info)
}

// ---- ISO 9660 ---------------------------------------------------------------------------------------------------------------

fn trim_id(b: &[u8]) -> String {
    text(b).trim().to_string()
}

fn iso(f: &mut File, len: u64) -> Option<Info> {
    let mut primary: Option<Vec<u8>> = None;
    let mut joliet: Option<Vec<u8>> = None;
    let mut boot = false;
    let mut udf = false;
    for i in 0..32u64 {
        let d = read_at(f, 0x8000 + i * 2048, 2048)?;
        if d.len() < 2048 {
            break;
        }
        let is_term = &d[1..6] == b"CD001" && d[0] == 255;
        match (&d[1..6], d[0]) {
            (b"CD001", 1) => primary = Some(d),
            (b"CD001", 2) if d[88..91].starts_with(b"%/") => joliet = Some(d),
            (b"CD001", 0) if d[7..30].starts_with(b"EL TORITO SPECIFICATION") => boot = true,
            (b"CD001", 255) => {}
            (b"BEA01", _) | (b"NSR02", _) | (b"NSR03", _) => udf = true,
            _ => {}
        }
        if is_term {
            // Keep reading a few sectors more: UDF's descriptors follow the ISO ones.
            for j in 1..8 {
                if let Some(u) = read_at(f, 0x8000 + (i + j) * 2048, 8) {
                    if matches!(&u[1..6.min(u.len())], b"BEA01" | b"NSR02" | b"NSR03") {
                        udf = true;
                    }
                }
            }
            break;
        }
    }
    if primary.is_none() && !udf {
        return None;
    }
    let mut info = Info { kind: "disc image (ISO)".into(), ..Default::default() };
    let mut systems = Vec::new();
    if primary.is_some() {
        systems.push("ISO 9660");
    }
    if joliet.is_some() {
        systems.push("Joliet");
    }
    if udf {
        systems.push("UDF");
    }
    info.facts.push(format!("File systems: {}{}", systems.join(" + "), if boot { ", bootable (El Torito)" } else { "" }));
    if let Some(p) = &primary {
        // Joliet keeps the name in Unicode; the primary one is in whatever code page the disc was made with.
        let wide_label = joliet.as_ref().map(|j| String::from_utf16_lossy(&j[40..72].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>()).trim().to_string()).filter(|l| !l.is_empty() && !l.chars().any(|c| c.is_control() || c == '\u{FFFD}'));
        let label = wide_label.unwrap_or_else(|| trim_id(&p[40..72]));
        if !label.is_empty() {
            info.facts.push(format!("Disc name: {label}"));
        }
        let (blocks, bs) = (le32(p, 80)? as u64, le16(p, 128)? as u64);
        info.facts.push(format!("Size {} in the volume", fmt_bytes(blocks * bs)));
        for (o, n, what) in [(318usize, 128usize, "Publisher"), (446, 128, "Prepared by"), (574, 128, "Application")] {
            let v = trim_id(&p[o..o + n]);
            if !v.is_empty() && !v.starts_with('_') {
                info.facts.push(format!("{what}: {v}"));
            }
        }
        let created = text(&p[813..829]);
        if created.len() >= 12 && created.bytes().take(12).all(|c| c.is_ascii_digit()) {
            info.facts.push(format!("Made {}-{}-{} {}:{}", &created[0..4], &created[4..6], &created[6..8], &created[8..10], &created[10..12]));
        }
    }
    // The root directory (Joliet's names when there are, they keep the long names and case).
    let (rootd, wide) = match (&joliet, &primary) {
        (Some(j), _) => (Some(j), true),
        (None, Some(p)) => (Some(p), false),
        _ => (None, false),
    };
    if let Some(d) = rootd {
        let bs = le16(d, 128)? as u64;
        let (lba, dlen) = (le32(d, 158)? as u64, le32(d, 166)? as usize);
        if let Some(dir) = read_at(f, lba * bs, dlen.min(256 << 10)) {
            let mut at = 0usize;
            while at < dir.len() {
                let rl = dir[at] as usize;
                if rl == 0 {
                    // Records do not cross sector borders: the rest of the sector is padding.
                    at = (at / 2048 + 1) * 2048;
                    continue;
                }
                if at + rl > dir.len() || rl < 34 {
                    break;
                }
                let r = &dir[at..at + rl];
                let (flags, nl) = (r[25], r[32] as usize);
                let raw = &r[33..33 + nl.min(rl - 33)];
                if !(nl == 1 && (raw[0] == 0 || raw[0] == 1)) {
                    let name = if wide {
                        String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>())
                    } else {
                        text(raw)
                    };
                    let name = name.split(';').next().unwrap_or("").to_string();
                    info.entries.push(Entry { name, size: le32(r, 10).unwrap_or(0) as u64, dir: flags & 2 != 0, pos: 0 });
                }
                at += rl;
                if info.entries.len() >= 500 {
                    info.more = true;
                    break;
                }
            }
        }
    }
    info.total = Some(info.entries.len() as u64);
    info.facts.push("Entries below: the top level of the disc".into());
    let _ = len;
    Some(info)
}

// ---- Apple DMG --------------------------------------------------------------------------------------------------------------

fn base64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

fn dmg(f: &mut File, len: u64) -> Option<Info> {
    let head = read_at(f, 0, 8)?;
    if head.starts_with(b"encrcdsa") || head.starts_with(b"cdsaencr") {
        return Some(Info { kind: "Apple disk image (DMG)".into(), facts: vec!["Encrypted disk image (AES): a password is needed to open it".into()], ..Default::default() });
    }
    if len < 512 {
        return None;
    }
    let koly = read_at(f, len - 512, 512)?;
    if !koly.starts_with(b"koly") {
        return None;
    }
    let (version, sectors, data_len, seg_count) = (be32(&koly, 4)?, be64(&koly, 492)?, be64(&koly, 32)?, be32(&koly, 60)?);
    let (xml_off, xml_len) = (be64(&koly, 216)?, be64(&koly, 224)?);
    let mut info = Info { kind: "Apple disk image (DMG)".into(), ..Default::default() };
    info.facts.push(format!("UDIF version {version}, disk of {} ({} sectors), packed into {}", fmt_bytes(sectors * 512), sectors, fmt_bytes(data_len)));
    if seg_count > 1 {
        info.facts.push(format!("Part of a set of {seg_count} segments"));
    }
    let mut kinds = std::collections::BTreeSet::new();
    if xml_len > 0 && xml_len < 8 << 20 && xml_off + xml_len <= len {
        if let Some(xml) = read_at(f, xml_off, xml_len as usize) {
            let xml = String::from_utf8_lossy(&xml).into_owned();
            if let Ok(doc) = roxmltree::Document::parse_with_options(&xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }) {
                // <key>blkx</key><array><dict><key>Name</key><string>..</string><key>Data</key><data>..</data>...
                let blkx = doc.descendants().find(|n| n.has_tag_name("key") && n.text() == Some("blkx")).and_then(|k| k.next_siblings().find(|s| s.has_tag_name("array")));
                if let Some(arr) = blkx {
                    for dict in arr.children().filter(|c| c.has_tag_name("dict")) {
                        let mut name = String::new();
                        let mut data = Vec::new();
                        let mut it = dict.children().filter(|c| c.is_element());
                        while let Some(k) = it.next() {
                            let Some(v) = it.next() else { break };
                            match k.text() {
                                Some("Name") => name = v.text().unwrap_or("").to_string(),
                                Some("Data") => data = base64(v.text().unwrap_or("")),
                                _ => {}
                            }
                        }
                        if data.starts_with(b"mish") {
                            let n = be32(&data, 200).unwrap_or(0) as usize;
                            for i in 0..n.min(100_000) {
                                match be32(&data, 204 + i * 40) {
                                    Some(0x8000_0004) => {
                                        kinds.insert("ADC");
                                    }
                                    Some(0x8000_0005) => {
                                        kinds.insert("zlib");
                                    }
                                    Some(0x8000_0006) => {
                                        kinds.insert("bzip2");
                                    }
                                    Some(0x8000_0007) => {
                                        kinds.insert("LZFSE");
                                    }
                                    Some(0x8000_0008) => {
                                        kinds.insert("LZMA");
                                    }
                                    Some(1) => {
                                        kinds.insert("uncompressed");
                                    }
                                    _ => {}
                                }
                            }
                        }
                        if !name.is_empty() {
                            let size = if data.starts_with(b"mish") { be64(&data, 16).unwrap_or(0) * 512 } else { 0 };
                            info.entries.push(Entry { name, size, ..Default::default() });
                        }
                    }
                }
            }
        }
    }
    if !kinds.is_empty() {
        info.facts.push(format!("Compression: {}", kinds.into_iter().collect::<Vec<_>>().join(", ")));
    }
    info.total = Some(info.entries.len() as u64);
    info.facts.push("Entries below: the disk's partitions (the files inside are in HFS+ / APFS: open in NuGallery with 7-Zip)".into());
    Some(info)
}

// ---- cpio, ar, deb, RPM, WIM, ARJ, LZH ---------------------------------------------------------------------------------------

fn cpio(f: &mut File, len: u64, head: &[u8]) -> Option<Info> {
    let newc = head.starts_with(b"070701") || head.starts_with(b"070702");
    let odc = head.starts_with(b"070707");
    let mut info = Info { kind: "cpio archive".into(), ..Default::default() };
    if !newc && !odc {
        info.facts.push("Binary cpio format: listing not read".into());
        return Some(info);
    }
    info.facts.push(if newc { "cpio, new ASCII format".into() } else { "cpio, old portable ASCII format".into() });
    let mut pos = 0u64;
    let hex = |b: &[u8]| u64::from_str_radix(&text(b), if newc { 16 } else { 8 }).ok();
    while pos + 76 < len && info.entries.len() < 5000 {
        let hd = read_at(f, pos, 110)?;
        let (mode, size, nsize, hlen) = if newc { (hex(hd.get(14..22)?)?, hex(hd.get(54..62)?)?, hex(hd.get(94..102)?)? as usize, 110u64) } else { (hex(hd.get(18..24)?)?, hex(hd.get(65..76)?)?, hex(hd.get(59..65)?)? as usize, 76u64) };
        let nm = read_at(f, pos + hlen, nsize)?;
        let name = cstr(&nm);
        if name == "TRAILER!!!" {
            info.total = Some(info.entries.len() as u64);
            break;
        }
        let dir = mode & 0o170000 == 0o040000;
        info.entries.push(Entry { name: name.trim_start_matches("./").to_string(), size: if dir { 0 } else { size }, dir, pos: 0 });
        let al = |n: u64| if newc { n.div_ceil(4) * 4 } else { n };
        pos += al(hlen + nsize as u64) + al(size);
        if newc {
            // The header + name is padded so the data starts on a 4-byte border (already in `al`).
        }
    }
    if info.total.is_none() {
        info.more = true;
    }
    let bytes: u64 = info.entries.iter().map(|e| e.size).sum();
    info.facts.insert(1, format!("{}, {}", plural(info.entries.iter().filter(|e| !e.dir).count() as u64, "file", "files"), fmt_bytes(bytes)));
    Some(info)
}

fn ar(f: &mut File, len: u64, thumb: bool) -> Option<Info> {
    let mut info = Info { kind: "ar archive".into(), ..Default::default() };
    let mut pos = 8u64;
    let mut long_names: Vec<u8> = Vec::new();
    let mut control: Option<(String, u64, u64)> = None;
    let mut is_deb = false;
    while pos + 60 <= len && info.entries.len() < 2000 {
        let h = read_at(f, pos, 60)?;
        if h.get(58..60) != Some(b"`\n") {
            break;
        }
        let mut name = text(&h[0..16]).trim_end().to_string();
        let size: u64 = text(&h[48..58]).trim().parse().ok()?;
        let data = pos + 60;
        if name == "//" {
            long_names = read_at(f, data, size.min(1 << 20) as usize)?;
        } else if let Some(n) = name.strip_prefix('/').and_then(|n| n.trim().parse::<usize>().ok()) {
            if let Some(rest) = long_names.get(n..) {
                name = text(&rest[..rest.iter().position(|&c| c == b'\n').unwrap_or(rest.len())]).trim_end_matches('/').to_string();
            }
        }
        let name = name.trim_end_matches('/').to_string();
        if name == "debian-binary" {
            is_deb = true;
        }
        if name.starts_with("control.tar") {
            control = Some((name.clone(), data, size));
        }
        if !name.is_empty() && name != "/" && name != "//" && !name.starts_with("__.SYMDEF") {
            info.entries.push(Entry { name, size, ..Default::default() });
        }
        pos = data + size + (size & 1);
    }
    info.total = Some(info.entries.len() as u64);
    if is_deb {
        info.kind = "Debian package (deb)".into();
        info.facts.push("Debian package: control, data".into());
        if let Some((name, off, size)) = control.filter(|(_, _, s)| *s < 8 << 20) {
            if let Some(blob) = read_at(f, off, size as usize) {
                let tar_bytes: Option<Vec<u8>> = if name.ends_with(".gz") {
                    let mut v = Vec::new();
                    flate2::read::GzDecoder::new(&blob[..]).take(4 << 20).read_to_end(&mut v).ok().map(|_| v)
                } else if name.ends_with(".zst") {
                    ruzstd::decoding::StreamingDecoder::new(&blob[..]).ok().and_then(|mut d| {
                        let mut v = Vec::new();
                        (&mut d).take(4 << 20).read_to_end(&mut v).ok().map(|_| v)
                    })
                } else if name.ends_with(".tar") {
                    Some(blob.clone())
                } else {
                    None
                };
                if let Some(tb) = tar_bytes {
                    let mut c = Cursor::new(tb.clone());
                    let (ents, _) = tar_list(&mut c, 100);
                    if let Some(e) = ents.iter().find(|e| e.name == "control") {
                        let data = tb.get(e.pos as usize..(e.pos + e.size) as usize).map(text).unwrap_or_default();
                        for line in data.lines() {
                            for key in ["Package", "Version", "Architecture", "Maintainer", "Installed-Size", "Homepage"] {
                                if let Some(v) = line.strip_prefix(&format!("{key}:")) {
                                    info.facts.push(format!("{key}: {}", v.trim()));
                                }
                            }
                            if let Some(v) = line.strip_prefix("Description:") {
                                info.facts.push(format!("Description: {}", v.trim()));
                            }
                        }
                    }
                }
            }
        }
    } else {
        info.facts.push(format!("{} (static library or Unix archive)", plural(info.entries.len() as u64, "member", "members")));
    }
    let _ = thumb;
    Some(info)
}

fn rpm(f: &mut File) -> Option<Info> {
    let lead = read_at(f, 0, 96)?;
    let mut info = Info { kind: "RPM package".into(), ..Default::default() };
    info.facts.push(format!("RPM {}.{}, {}", lead[4], lead[5], if be16(&lead, 6)? == 1 { "source package" } else { "binary package" }));
    let mut at = 96u64;
    for pass in 0..2 {
        let h = read_at(f, at, 16)?;
        if h.get(0..4) != Some(&[0x8E, 0xAD, 0xE8, 0x01]) {
            return Some(info);
        }
        let (n, dsize) = (be32(&h, 8)? as usize, be32(&h, 12)? as usize);
        if n > 4096 || dsize > 4 << 20 {
            return Some(info);
        }
        let idx = read_at(f, at + 16, n * 16)?;
        let store = read_at(f, at + 16 + (n * 16) as u64, dsize)?;
        if pass == 1 {
            for i in 0..n {
                let (tag, ty, off, cnt) = (be32(&idx, i * 16)?, be32(&idx, i * 16 + 4)?, be32(&idx, i * 16 + 8)? as usize, be32(&idx, i * 16 + 12)? as usize);
                let s = || store.get(off..).map(cstr).unwrap_or_default();
                let label = match tag {
                    1000 => "Name",
                    1001 => "Version",
                    1002 => "Release",
                    1004 => "Summary",
                    1009 => "Installed size",
                    1014 => "Licence",
                    1016 => "Group",
                    1020 => "URL",
                    1022 => "Architecture",
                    1011 => "Vendor",
                    _ => continue,
                };
                let v = match ty {
                    6 | 9 => s(),
                    4 => be32(&store, off).map(|v| if tag == 1009 { fmt_bytes(v as u64) } else { v.to_string() }).unwrap_or_default(),
                    _ => continue,
                };
                let _ = cnt;
                if !v.is_empty() {
                    info.facts.push(format!("{label}: {v}"));
                }
            }
            return Some(info);
        }
        // The signature header is padded to a multiple of 8 bytes.
        at += 16 + (n * 16) as u64 + dsize as u64;
        at = at.div_ceil(8) * 8;
    }
    Some(info)
}

fn wim(f: &mut File, len: u64) -> Option<Info> {
    let h = read_at(f, 0, 208)?;
    let flags = le32(&h, 16)?;
    let comp = if flags & 0x2 == 0 {
        "uncompressed"
    } else if flags & 0x40000 != 0 {
        "LZX"
    } else if flags & 0x20000 != 0 {
        "XPRESS"
    } else if flags & 0x80000 != 0 {
        "LZMS"
    } else {
        "compressed"
    };
    let images = le32(&h, 44)?;
    let mut info = Info { kind: "Windows Imaging (WIM)".into(), ..Default::default() };
    info.facts.push(format!("WIM version {:#x}, {}, {}, {}", le32(&h, 12)?, plural(images as u64, "image", "images"), comp, fmt_bytes(len)));
    if le16(&h, 42)? > 1 {
        info.facts.push(format!("Part {} of {}", le16(&h, 40)?, le16(&h, 42)?));
    }
    // The XML description of the images (UTF-16): name, description, edition, version, size of each.
    let xml_size = le64(&h, 72)? & 0x00FF_FFFF_FFFF_FFFF;
    let xml_off = le64(&h, 80)?;
    if xml_size > 0 && xml_size < 4 << 20 && xml_off + xml_size <= len {
        if let Some(raw) = read_at(f, xml_off, xml_size as usize) {
            let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let xml = String::from_utf16_lossy(&units);
            let xml = xml.trim_start_matches('\u{feff}');
            if let Ok(doc) = roxmltree::Document::parse(xml) {
                for img in doc.descendants().filter(|n| n.has_tag_name("IMAGE")) {
                    let get = |t: &str| img.descendants().find(|n| n.has_tag_name(t)).and_then(|n| n.text()).unwrap_or("").to_string();
                    let name = {
                        let n = get("NAME");
                        if n.is_empty() { get("DISPLAYNAME") } else { n }
                    };
                    let size: u64 = get("TOTALBYTES").parse().unwrap_or(0);
                    let ed = get("EDITIONID");
                    let label = if ed.is_empty() { name } else { format!("{name} ({ed})") };
                    info.entries.push(Entry { name: format!("#{} {}", img.attribute("INDEX").unwrap_or("?"), label), size, ..Default::default() });
                }
            }
        }
    }
    info.total = Some(info.entries.len() as u64);
    info.facts.push("Entries below: the images (editions) inside".into());
    Some(info)
}

fn arj(f: &mut File, len: u64) -> Option<Info> {
    let mut info = Info { kind: "ARJ archive".into(), ..Default::default() };
    let mut pos = 0u64;
    let mut first = true;
    while pos + 4 < len && info.entries.len() < 5000 {
        let h = read_at(f, pos, 4)?;
        if h.get(0..2) != Some(&[0x60, 0xEA]) {
            break;
        }
        let bsize = le16(&h, 2)? as u64;
        if bsize == 0 {
            break;
        }
        let body = read_at(f, pos + 4, bsize as usize)?;
        let after = pos + 4 + bsize + 4; // the header CRC
        // Extended headers: a 16-bit size (0 = none), each followed by its own CRC.
        let mut next = after;
        loop {
            let e = read_at(f, next, 2)?;
            let esz = le16(&e, 0)? as u64;
            next += 2;
            if esz == 0 {
                break;
            }
            next += esz + 4;
        }
        if first {
            first = false;
            let name = body.get(*body.first()? as usize..).map(cstr).unwrap_or_default();
            info.facts.push(format!("ARJ, made by version {}{}", body.get(1)?, if name.is_empty() { String::new() } else { format!(", archive name {name}") }));
            pos = next;
            continue;
        }
        let first_size = *body.first()? as usize;
        let (method, ftype, comp, orig) = (*body.get(5)?, *body.get(6)?, le32(&body, 12)? as u64, le32(&body, 16)? as u64);
        let name = body.get(first_size..).map(cstr).unwrap_or_default();
        info.entries.push(Entry { name: name.replace('\\', "/"), size: orig, dir: ftype == 3, ..Default::default() });
        let _ = method;
        pos = next + comp;
    }
    info.total = Some(info.entries.len() as u64);
    let bytes: u64 = info.entries.iter().map(|e| e.size).sum();
    info.facts.insert(0, format!("{}, {}", plural(info.entries.len() as u64, "file", "files"), fmt_bytes(bytes)));
    Some(info)
}

fn lzh(f: &mut File, len: u64) -> Option<Info> {
    let mut info = Info { kind: "LZH / LHA archive".into(), ..Default::default() };
    let mut pos = 0u64;
    let mut methods = std::collections::BTreeSet::new();
    while pos + 22 < len && info.entries.len() < 5000 {
        let h = read_at(f, pos, 4096)?;
        let level = *h.get(20)?;
        if h.get(2..4) != Some(b"-l") && h.get(2..4) != Some(b"-L") {
            break;
        }
        methods.insert(text(h.get(2..7)?));
        let (comp, orig) = (le32(&h, 7)? as u64, le32(&h, 11)? as u64);
        let (name, header_total, ext_total): (String, u64, u64) = match level {
            0 | 1 => {
                let hsize = *h.first()? as u64;
                let nlen = *h.get(21)? as usize;
                let name = text(h.get(22..22 + nlen)?);
                let mut ext = 0u64;
                if level == 1 {
                    // After the name: crc16, os id, then next-header size words; their data is part of the packed size.
                    let mut at = 22 + nlen + 3;
                    loop {
                        let sz = le16(&h, at)? as u64;
                        if sz == 0 {
                            break;
                        }
                        ext += sz;
                        at += sz as usize;
                        if at > h.len() {
                            break;
                        }
                    }
                }
                (name, hsize + 2, ext)
            }
            2 => {
                let hsize = le16(&h, 0)? as u64;
                let mut at = 24usize;
                let mut name = String::new();
                let mut dir = String::new();
                loop {
                    let sz = le16(&h, at)? as usize;
                    if sz < 3 || at + sz > h.len() {
                        break;
                    }
                    let ty = *h.get(at + 2)?;
                    match ty {
                        1 => name = text(&h[at + 3..at + sz]),
                        2 => dir = text(&h[at + 3..at + sz]).replace('\u{ff}', "/"),
                        _ => {}
                    }
                    at += sz;
                }
                (format!("{dir}{name}"), hsize, 0)
            }
            _ => break,
        };
        let dir = h.get(2..7).is_some_and(|m| m == b"-lhd-");
        info.entries.push(Entry { name: name.replace('\\', "/"), size: orig, dir, ..Default::default() });
        pos += header_total + ext_total.min(0) + comp;
        if level == 1 {
            pos = pos.saturating_sub(0);
        }
    }
    info.total = Some(info.entries.len() as u64);
    info.facts.insert(0, format!("{}, {}; methods {}", plural(info.entries.len() as u64, "file", "files"), fmt_bytes(info.entries.iter().map(|e| e.size).sum()), methods.into_iter().collect::<Vec<_>>().join(" ")));
    Some(info)
}

// ---- game packs (.pak) ------------------------------------------------------------------------------------------------------

fn pak(f: &mut File, len: u64, head: &[u8], thumb: bool) -> Option<Info> {
    // Quake (PACK, 64-byte entries) / Sin (SPAK, 128) / Daikatana (PACK, 72).
    if head.starts_with(b"PACK") || head.starts_with(b"SPAK") {
        let (dir_off, dir_len) = (le32(head, 4)? as u64, le32(head, 8)? as u64);
        let esize = if head.starts_with(b"SPAK") { 128u64 } else if dir_len % 64 == 0 { 64 } else if dir_len % 72 == 0 { 72 } else { return None };
        if dir_off + dir_len > len || dir_len > 32 << 20 {
            return None;
        }
        let dir = read_at(f, dir_off, dir_len as usize)?;
        let name_len = (esize - 8) as usize;
        let mut info = Info { kind: "Quake-style game pack (PAK)".into(), ..Default::default() };
        let mut picture: Option<(u64, Image, String)> = None;
        for e in dir.chunks_exact(esize as usize) {
            let name = cstr(&e[..name_len]);
            let (off, size) = (le32(e, name_len)? as u64, le32(e, name_len + 4)? as u64);
            info.entries.push(Entry { name: name.clone(), size, ..Default::default() });
            let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !thumb || picture.is_none() {
                if matches!(ext.as_str(), "pcx" | "tga" | "png" | "jpg" | "bmp") && size < 4 << 20 && size > 200 && picture.as_ref().is_none_or(|p| size > p.0) {
                    if let Some(b) = read_at(f, off, size as usize) {
                        if let Ok(img) = crate::decode_image_bytes(&b, &ext) {
                            picture = Some((size, img, name));
                        }
                    }
                }
            }
        }
        info.total = Some(info.entries.len() as u64);
        let bytes: u64 = info.entries.iter().map(|e| e.size).sum();
        info.facts.push(format!("{}, {}", plural(info.entries.len() as u64, "file", "files"), fmt_bytes(bytes)));
        if let Some((_, mut img, name)) = picture {
            img.colour_note = Some(format!("PAK: the largest picture inside ({name}) of {} files", info.entries.len()));
            info.picture = Some(img);
        }
        return Some(info);
    }
    // Chromium / Electron / CEF resource packs: version 5 (or 4), then ids with offsets; the pictures are plain PNG files.
    let version = le32(head, 0)?;
    if version == 5 || version == 4 {
        let (count, hdr, entry0) = if version == 5 { (le16(head, 8)? as usize, 12usize, 12usize) } else { (le32(head, 4)? as usize, 9usize, 9usize) };
        if count > 0 && count < 200_000 {
            let table = read_at(f, entry0 as u64, (count + 1) * 6)?;
            let _ = hdr;
            let mut info = Info { kind: "Chromium resource pack (PAK)".into(), ..Default::default() };
            let mut pngs = 0usize;
            let mut best: Option<(usize, u32)> = None;
            let mut offs = Vec::new();
            for i in 0..=count {
                offs.push((le16(&table, i * 6)? as u32, le32(&table, i * 6 + 2)? as u64));
            }
            for w in offs.windows(2) {
                let (id, off) = w[0];
                let end = w[1].1;
                if end < off || end > len {
                    continue;
                }
                let size = end - off;
                if size > 8 && size < 8 << 20 {
                    if let Some(sig) = read_at(f, off, 8) {
                        if sig == b"\x89PNG\r\n\x1a\n" {
                            pngs += 1;
                            if best.is_none_or(|(s, _)| size as usize > s) && (!thumb || best.is_none()) {
                                best = Some((size as usize, id));
                            }
                        }
                    }
                }
            }
            info.facts.push(format!("Resource pack version {version}, {}, {}", plural(count as u64, "resource", "resources"), plural(pngs as u64, "PNG picture", "PNG pictures")));
            info.total = Some(count as u64);
            if let Some((_, id)) = best {
                let (off, end) = offs.windows(2).find(|w| w[0].0 == id).map(|w| (w[0].1, w[1].1))?;
                if let Some(b) = read_at(f, off, (end - off) as usize) {
                    if let Ok(mut img) = crate::decode_image_bytes(&b, "png") {
                        img.colour_note = Some(format!("PAK: the largest PNG inside (resource {id}) of {count} resources"));
                        info.picture = Some(img);
                    }
                }
            }
            return Some(info);
        }
    }
    // Unreal Engine 4/5 pak: a footer with the magic 0x5A6F12E1.
    for back in [44u64, 45, 61, 53, 221] {
        if len > back {
            if let Some(t) = read_at(f, len - back, back as usize) {
                if let Some(pos) = (0..t.len().saturating_sub(3)).find(|&i| le32(&t, i) == Some(0x5A6F_12E1)) {
                    let version = le32(&t, pos + 4)?;
                    let (ioff, isz) = (le64(&t, pos + 8)?, le64(&t, pos + 16)?);
                    let mut info = Info { kind: "Unreal Engine pak".into(), ..Default::default() };
                    info.facts.push(format!("Unreal Engine pak, version {version}, index {} at {ioff}", fmt_bytes(isz)));
                    info.facts.push("The index lists the files inside; it is often encrypted, so no listing here".into());
                    return Some(info);
                }
            }
        }
    }
    None
}

// ---- finding out what a file is -----------------------------------------------------------------------------------------------

/// Read what `path` is by its content (the extension only breaks ties). `None` when it is not an archive we read.
pub fn inspect(path: &Path, ext: &str, thumb: bool) -> Option<Info> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let head = read_at(&mut f, 0, 4096)?;
    if ext == "pak" {
        return pak(&mut f, len, &head, thumb);
    }
    if head.starts_with(b"MSCF") {
        return cab(&mut f, len, &head);
    }
    if head.starts_with(b"!<arch>\n") {
        return ar(&mut f, len, thumb);
    }
    if head.starts_with(&[0xED, 0xAB, 0xEE, 0xDB]) {
        return rpm(&mut f);
    }
    if head.starts_with(b"MSWIM\0\0\0") {
        return wim(&mut f, len);
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return gzip(&mut f, len, &head, thumb);
    }
    if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
        return xz(&mut f, len, &head);
    }
    if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return zstd(&mut f, len, &head, thumb, ext);
    }
    if head.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return sevenz(&head);
    }
    if head.starts_with(b"Rar!\x1A\x07") {
        return rar(&mut f, len, &head);
    }
    if head.starts_with(&[0x60, 0xEA]) {
        return arj(&mut f, len);
    }
    if head.get(2..4).is_some_and(|m| m == b"-l" || m == b"-L") && head.get(6) == Some(&b'-') {
        return lzh(&mut f, len);
    }
    if head.starts_with(b"070701") || head.starts_with(b"070702") || head.starts_with(b"070707") || head.starts_with(&[0xC7, 0x71]) || head.starts_with(&[0x71, 0xC7]) {
        return cpio(&mut f, len, &head);
    }
    if let Some(i) = simple_compressed(&mut f, len, &head, ext) {
        return Some(i);
    }
    if len > 0x8800 {
        if let Some(i) = iso(&mut f, len) {
            return Some(i);
        }
    }
    if let Some(i) = dmg(&mut f, len) {
        return Some(i);
    }
    plain_tar(&mut f, thumb)
}

// ---- the picture --------------------------------------------------------------------------------------------------------------

fn badge_of(ext: &str) -> String {
    ext.to_ascii_uppercase()
}

fn accent(ext: &str) -> u32 {
    match ext {
        "cab" | "msi" | "wim" | "esd" | "swm" => 0x0078D4,
        "dmg" | "pkg" => 0x6E6E73,
        "iso" | "img" => 0x7A5AF8,
        "deb" | "rpm" | "ar" | "cpio" | "xar" => 0xB45309,
        "pak" => 0xB91C1C,
        "7z" | "cb7" | "rar" | "cbr" | "arj" | "lzh" | "lha" => 0xD97706,
        "tar" | "tgz" | "gz" | "taz" | "bz2" | "tbz" | "tbz2" | "tb2" | "xz" | "txz" | "lzma" | "tlz" | "zst" | "tzst" | "lz4" | "lz" | "z" => 0x0F766E,
        e => accent_for(e),
    }
}

/// A compact card: the thumbnail (and what the viewer shows for a thumbnail-sized request).
fn card(path: &Path, ext: &str, info: &Info, size: u64, scale: i32) -> Image {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut lines = vec![name.clone()];
    lines.push(info.kind.clone());
    for f in info.facts.iter().take(2) {
        lines.push(f.clone());
    }
    if !info.entries.is_empty() {
        lines.push(String::new());
        for e in info.entries.iter().take(6) {
            lines.push(format!("{}{}", e.name, if e.dir && !e.name.ends_with('/') { "/" } else { "" }));
        }
        let total = info.total.unwrap_or(info.entries.len() as u64);
        if total > 6 || info.more {
            lines.push(format!("… {}", if info.more { "and more".to_string() } else { format!("{} more", total - 6) }));
        }
    }
    let lines: Vec<String> = lines.into_iter().map(|l| crate::fontview::clip(&l, 44)).collect();
    // The paper-card layout leaves out empty lines on its own; keep the blank as a thin spacer by using a space.
    let lines = lines.into_iter().map(|l| if l.is_empty() { " ".to_string() } else { l }).collect();
    render_card(&Card { badge: badge_of(ext), accent: accent(ext), title: name, subtitle: fmt_bytes(size), lines }, scale)
}

/// The viewer's page: header, facts, and the listing in two columns of name and size.
fn sheet(path: &Path, ext: &str, info: &Info, size: u64) -> Result<Image, String> {
    use crate::fontview::{clip, esc, ui_fonts};
    use resvg::{tiny_skia, usvg};
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let w = 1100u32;
    let ui = "font-family=\"Segoe UI, Leelawadee UI, Tahoma\"";
    let mono = "font-family=\"Consolas, Segoe UI, Leelawadee UI, MS Gothic\"";
    let acc = accent(ext);
    let mut s = String::new();
    s.push_str(&format!("<rect width=\"{w}\" height=\"14\" fill=\"#{acc:06X}\"/>"));
    let mut y = 52.0f32;
    s.push_str(&format!("<text x=\"48\" y=\"{y}\" {ui} font-size=\"15\" font-weight=\"700\" fill=\"#{acc:06X}\" letter-spacing=\"1.5\">{}</text>", esc(&badge_of(ext))));
    y += 40.0;
    s.push_str(&format!("<text x=\"48\" y=\"{y}\" {ui} font-size=\"34\" font-weight=\"700\" fill=\"#1B1B1B\">{}</text>", esc(&clip(&name, 62))));
    y += 28.0;
    s.push_str(&format!("<text x=\"48\" y=\"{y}\" {ui} font-size=\"18\" fill=\"#6B6B6B\">{}  ·  {}</text>", esc(&info.kind), esc(&fmt_bytes(size))));
    y += 20.0;
    for f in &info.facts {
        for (i, line) in wrap_chars(f, 120).into_iter().take(3).enumerate() {
            y += 24.0;
            s.push_str(&format!("<text x=\"{}\" y=\"{y}\" {ui} font-size=\"16\" fill=\"#3A3A3A\">{}</text>", if i == 0 { 48 } else { 64 }, esc(&line)));
        }
    }
    y += 20.0;
    if !info.entries.is_empty() {
        s.push_str(&format!("<rect x=\"48\" y=\"{y}\" width=\"{}\" height=\"1\" fill=\"#D6D3C8\"/>", w - 96));
        y += 8.0;
        let shown = info.entries.len().min(70);
        for e in info.entries.iter().take(shown) {
            y += 25.0;
            let mut n = e.name.clone();
            if e.dir && !n.ends_with('/') {
                n.push('/');
            }
            s.push_str(&format!("<text x=\"48\" y=\"{y}\" {mono} font-size=\"15\" fill=\"{}\">{}</text>", if e.dir { "#0F5F8F" } else { "#1B1B1B" }, esc(&clip(&n, 100))));
            if !e.dir {
                s.push_str(&format!("<text x=\"{}\" y=\"{y}\" {ui} font-size=\"14\" text-anchor=\"end\" fill=\"#6B6B6B\">{}</text>", w - 48, esc(&fmt_bytes(e.size))));
            }
        }
        let total = info.total.unwrap_or(info.entries.len() as u64);
        if total as usize > shown || info.more {
            y += 30.0;
            let more = if info.more && total as usize <= shown { "more entries not listed".to_string() } else { format!("{} more entries", total as usize - shown) };
            s.push_str(&format!("<text x=\"48\" y=\"{y}\" {ui} font-size=\"15\" font-style=\"italic\" fill=\"#6B6B6B\">… and {}</text>", esc(&more)));
        }
    }
    let h = (y + 44.0).ceil() as u32;
    let h = h.max(420);
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">{s}</svg>");
    let mut opt = usvg::Options::default();
    opt.fontdb = ui_fonts();
    opt.font_family = "Segoe UI".into();
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("archive page: {e}"))?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("archive page too large")?;
    pixmap.fill(tiny_skia::Color::from_rgba8(0xF6, 0xF5, 0xF0, 255));
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    Ok(Image::new(w, h, pixmap.take()))
}

fn wrap_chars(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > cols {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A picture for an archive file: the picture inside it when a game pack has one, a card for thumbnails, a listing page for the viewer.
/// `None` when this is not something `inspect` reads (the caller draws its plain card).
pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    let size = std::fs::metadata(path).ok()?.len();
    let mut info = std::panic::catch_unwind(|| inspect(path, ext, first_only)).ok().flatten()?;
    if let Some(img) = info.picture.take() {
        return Some(Ok(img));
    }
    let mut img = if first_only {
        card(path, ext, &info, size, 1)
    } else {
        match sheet(path, ext, &info, size) {
            Ok(i) => i,
            Err(_) => card(path, ext, &info, size, 2),
        }
    };
    img.colour_note = Some(format!("{}: read from the file's own table of contents (nothing was unpacked)", info.kind));
    Some(Ok(img))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(tag: &str, ext: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("nv_arc_{tag}_{}.{ext}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn tar_block(name: &str, data: &[u8], kind: u8) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        h[156] = kind;
        h[257..262].copy_from_slice(b"ustar");
        h[148..156].fill(32);
        let sum: u32 = h.iter().map(|&c| c as u32).sum();
        h[148..156].copy_from_slice(format!("{:06o}\0 ", sum).as_bytes());
        let mut out = h;
        out.extend(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
        out
    }

    fn make_tar() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend(tar_block("docs/", b"", b'5'));
        t.extend(tar_block("docs/readme.txt", b"hello world", b'0'));
        t.extend(tar_block("photo.jpg", &[7u8; 1500], b'0'));
        t.extend(vec![0u8; 1024]);
        t
    }

    #[test]
    fn tar_plain_and_gzip_are_listed() {
        let t = make_tar();
        let p = temp("t", "tar", &t);
        let info = inspect(&p, "tar", false).unwrap();
        assert_eq!(info.entries.len(), 3);
        assert_eq!(info.entries[1].name, "docs/readme.txt");
        assert_eq!(info.entries[2].size, 1500);
        assert!(info.entries[0].dir && info.total == Some(3));
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&t).unwrap();
        let gz = z.finish().unwrap();
        let p2 = temp("tgz", "tgz", &gz);
        let info = inspect(&p2, "tgz", false).unwrap();
        assert_eq!(info.kind, "gzip-compressed tar archive");
        assert_eq!(info.entries.len(), 3);
        // A plain gzip file reports its unpacked size.
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&[1u8; 5000]).unwrap();
        let p3 = temp("g", "gz", &z.finish().unwrap());
        let info = inspect(&p3, "gz", false).unwrap();
        assert_eq!(info.kind, "gzip file");
        assert_eq!(info.entries[0].size, 5000);
        for p in [p, p2, p3] {
            std::fs::remove_file(p).ok();
        }
    }

    #[test]
    fn cab_listing() {
        // One folder (MSZIP), two files.
        let names = [("a.txt", 100u32), ("sub\\b.bin", 2000)];
        let mut files = Vec::new();
        for (n, s) in names {
            files.extend(s.to_le_bytes());
            files.extend(0u32.to_le_bytes());
            files.extend(0u16.to_le_bytes());
            files.extend([0u8; 4]);
            files.extend(0u16.to_le_bytes());
            files.extend(n.as_bytes());
            files.push(0);
        }
        let mut c = Vec::new();
        c.extend(b"MSCF");
        c.extend(0u32.to_le_bytes());
        c.extend(0u32.to_le_bytes()); // size, patched
        c.extend(0u32.to_le_bytes());
        c.extend((36u32 + 8).to_le_bytes()); // files offset
        c.extend(0u32.to_le_bytes());
        c.extend([3u8, 1]); // minor, major
        c.extend(1u16.to_le_bytes());
        c.extend(2u16.to_le_bytes());
        c.extend(0u16.to_le_bytes());
        c.extend(7u16.to_le_bytes());
        c.extend(0u16.to_le_bytes());
        c.extend(0u32.to_le_bytes()); // folder: data offset
        c.extend(1u16.to_le_bytes());
        c.extend(1u16.to_le_bytes()); // MSZIP
        c.extend(files);
        let p = temp("c", "cab", &c);
        let info = inspect(&p, "cab", false).unwrap();
        assert_eq!(info.entries.len(), 2);
        assert_eq!(info.entries[1].name, "sub/b.bin");
        assert!(info.facts.iter().any(|f| f.contains("MSZIP")), "{:?}", info.facts);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn cpio_ar_and_pak() {
        // newc cpio with one file.
        let mut c = Vec::new();
        let hdr = |name: &str, size: usize, mode: u32| {
            let mut h = String::from("070701");
            for v in [1u32, mode, 0, 0, 1, 0, size as u32, 0, 0, 0, 0, name.len() as u32 + 1, 0] {
                h.push_str(&format!("{v:08X}"));
            }
            let mut b = h.into_bytes();
            b.extend(name.as_bytes());
            b.push(0);
            while b.len() % 4 != 0 {
                b.push(0);
            }
            b
        };
        c.extend(hdr("etc/passwd", 5, 0o100644));
        c.extend(b"abcde\0\0\0");
        c.extend(hdr("TRAILER!!!", 0, 0));
        let p = temp("cp", "cpio", &c);
        let info = inspect(&p, "cpio", false).unwrap();
        assert_eq!(info.entries.len(), 1);
        assert_eq!((info.entries[0].name.as_str(), info.entries[0].size), ("etc/passwd", 5));
        // ar
        let mut a = b"!<arch>\n".to_vec();
        a.extend(format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", "hello.o/", 0, 0, 0, 644, 4).as_bytes());
        a.extend(b"ABCD");
        let pa = temp("ar", "a", &a);
        let info = inspect(&pa, "a", false).unwrap();
        assert_eq!(info.entries[0].name, "hello.o");
        // Quake pak
        let mut k = b"PACK".to_vec();
        k.extend(12u32.to_le_bytes());
        k.extend(64u32.to_le_bytes());
        k.extend(vec![0u8; 0]);
        let mut e = vec![0u8; 64];
        e[..9].copy_from_slice(b"maps/e1m1");
        e[56..60].copy_from_slice(&0u32.to_le_bytes());
        e[60..64].copy_from_slice(&10u32.to_le_bytes());
        k.extend(e);
        let pk = temp("pk", "pak", &k);
        let info = inspect(&pk, "pak", false).unwrap();
        assert_eq!(info.entries[0].name, "maps/e1m1");
        for p in [p, pa, pk] {
            std::fs::remove_file(p).ok();
        }
    }

    #[test]
    fn iso_label_and_root() {
        let mut img = vec![0u8; 0x8000 + 2048 * 4 + 2048 * 20];
        let pvd = 0x8000;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        img[pvd + 6] = 1;
        img[pvd + 40..pvd + 72].copy_from_slice(format!("{:<32}", "MY_DISC").as_bytes());
        img[pvd + 80..pvd + 84].copy_from_slice(&30u32.to_le_bytes());
        img[pvd + 128..pvd + 130].copy_from_slice(&2048u16.to_le_bytes());
        // Root directory at sector 20 with one file record.
        let root = 20u32;
        img[pvd + 158..pvd + 162].copy_from_slice(&root.to_le_bytes());
        img[pvd + 166..pvd + 170].copy_from_slice(&2048u32.to_le_bytes());
        let term = 0x8000 + 2048;
        img[term] = 255;
        img[term + 1..term + 6].copy_from_slice(b"CD001");
        let dir = root as usize * 2048;
        let name = b"README.TXT;1";
        let rl = 33 + name.len() + (name.len() + 1) % 2;
        img[dir] = rl as u8;
        img[dir + 10..dir + 14].copy_from_slice(&1234u32.to_le_bytes());
        img[dir + 32] = name.len() as u8;
        img[dir + 33..dir + 33 + name.len()].copy_from_slice(name);
        let p = temp("iso", "iso", &img);
        let info = inspect(&p, "iso", false).unwrap();
        assert!(info.facts.iter().any(|f| f.contains("MY_DISC")), "{:?}", info.facts);
        assert_eq!(info.entries.len(), 1);
        assert_eq!((info.entries[0].name.as_str(), info.entries[0].size), ("README.TXT", 1234));
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn small_compressed_headers_and_a_non_archive() {
        let p = temp("bz", "bz2", b"BZh9\x31\x41\x59\x26\x53\x59 rest");
        assert!(inspect(&p, "bz2", true).unwrap().facts[0].contains("900 KB"));
        let p2 = temp("junk", "tar", &vec![b'x'; 2000]);
        assert!(inspect(&p2, "tar", true).is_none());
        assert_eq!(unix_date(0), "1970-01-01 00:00");
        assert_eq!(unix_date(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(base64("SGVsbG8="), b"Hello");
        for p in [p, p2] {
            std::fs::remove_file(p).ok();
        }
    }

    #[test]
    fn the_viewer_page_and_the_card_draw() {
        let t = make_tar();
        let p = temp("pg", "tar", &t);
        let page = decode(&p, "tar", false).unwrap().unwrap();
        assert_eq!(page.width, 1100);
        assert!(page.rgba.chunks_exact(4).filter(|px| px[0] < 100).count() > 1000, "has text");
        let card = decode(&p, "tar", true).unwrap().unwrap();
        assert!(card.width > 0);
        std::fs::remove_file(p).ok();
    }
}
