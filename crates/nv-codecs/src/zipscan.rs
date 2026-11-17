//! Reading a zip from the front, without its central directory. A zip that was streamed (a download made on the fly, `zip -`
//! from a pipe) or that was cut off has no table of contents at its end, but every entry still has its own header in front of
//! its data; 7-Zip and Windows' own tools walk those headers, and so does this. The data of an entry that says "sizes follow
//! the data" (flag bit 3) is inflated to find where it ends, then the little record after it is read.
//!
//! `walk` hands each entry's decoded bytes to a callback; what the callback does with them (count them, write them to a file,
//! decode a picture from the first few kilobytes) is its own business. An entry cut off in the middle gives the callback a read
//! error after the bytes that did arrive, and the walk ends there (`complete == false`).

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// What an entry's local header says.
#[derive(Clone, Debug)]
pub struct Header {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    /// Sizes from the header (None when they follow the data).
    pub csize: Option<u64>,
    pub size: Option<u64>,
    pub is_dir: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Entries that were visited (the cut-off one included).
    pub entries: usize,
    /// The walk reached the central directory (the zip is whole) rather than the end of the file or something unreadable.
    pub complete: bool,
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn le64(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4], b[i + 5], b[i + 6], b[i + 7]])
}

fn is_sig(s: &[u8]) -> bool {
    s.len() >= 4 && matches!(&s[..4], b"PK\x03\x04" | b"PK\x01\x02" | b"PK\x05\x06" | b"PK\x06\x06")
}

/// Read up to `buf.len()` bytes (fewer only at the end of the file).
fn read_upto(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// The name of an entry: UTF-8 when flagged (or when it happens to be valid); otherwise the legacy way zips from Japanese, Chinese
/// and Korean programs wrote names, in the code pages that Windows knows (Shift-JIS, GBK, Korean, Big5: the first one the bytes are
/// valid in), else the bytes as Latin-1. A name that is only ASCII is the same in all of them.
pub fn decode_name(bytes: &[u8], flags: u16) -> String {
    if flags & 0x800 != 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(windows)]
    for cp in [932u32, 936, 949, 950] {
        if let Some(s) = from_code_page(bytes, cp) {
            return s;
        }
    }
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(windows)]
fn from_code_page(bytes: &[u8], cp: u32) -> Option<String> {
    use windows::Win32::Globalization::{MultiByteToWideChar, MB_ERR_INVALID_CHARS};
    unsafe {
        let n = MultiByteToWideChar(cp, MB_ERR_INVALID_CHARS, bytes, None);
        if n <= 0 {
            return None;
        }
        let mut buf = vec![0u16; n as usize];
        let m = MultiByteToWideChar(cp, MB_ERR_INVALID_CHARS, bytes, Some(&mut buf));
        if m <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..m as usize]))
    }
}

fn entry_name(bytes: &[u8], flags: u16) -> String {
    decode_name(bytes, flags)
}

/// After an entry whose sizes follow its data: the record that holds them. `used` is how many compressed bytes the entry
/// had. Moves the reader past the record; false when nothing sensible is there (the file ends, or it is not a zip).
fn skip_descriptor(r: &mut BufReader<File>, used: u64) -> io::Result<bool> {
    let pos = r.stream_position()?;
    let mut b = [0u8; 28];
    let n = read_upto(r, &mut b)?;
    let b = &b[..n];
    // Where the next record may start: a signature, or the end of the file.
    let next_ok = |off: usize| n == off || (n >= off + 4 && is_sig(&b[off..]));
    let sig = n >= 4 && &b[..4] == b"PK\x07\x08";
    let mut take: Option<usize> = None;
    if sig {
        if n >= 16 && le32(b, 8) as u64 == used & 0xFFFF_FFFF && next_ok(16) {
            take = Some(16);
        } else if n >= 24 && le64(b, 8) == used && next_ok(24) {
            take = Some(24);
        }
    }
    if take.is_none() {
        if n >= 12 && le32(b, 4) as u64 == used & 0xFFFF_FFFF && next_ok(12) {
            take = Some(12);
        } else if n >= 20 && le64(b, 4) == used && next_ok(20) {
            take = Some(20);
        }
    }
    match take {
        Some(k) => {
            r.seek(SeekFrom::Start(pos + k as u64))?;
            Ok(true)
        }
        None => {
            r.seek(SeekFrom::Start(pos))?;
            Ok(false)
        }
    }
}

/// Walk the entries of `path` from the front. `on_entry(header, decoded_bytes)` returns false to stop the walk (whatever it did
/// not read of the entry is skipped).
pub fn walk(path: &Path, on_entry: &mut dyn FnMut(&Header, &mut dyn Read) -> bool) -> io::Result<Outcome> {
    let mut r = BufReader::with_capacity(256 * 1024, File::open(path)?);
    let mut entries = 0usize;
    loop {
        let mut sig = [0u8; 4];
        if read_upto(&mut r, &mut sig)? < 4 {
            return Ok(Outcome { entries, complete: false });
        }
        match &sig {
            b"PK\x03\x04" => {}
            b"PK\x01\x02" | b"PK\x05\x06" | b"PK\x06\x06" => return Ok(Outcome { entries, complete: true }),
            _ => return Ok(Outcome { entries, complete: false }),
        }
        let mut h = [0u8; 26];
        if read_upto(&mut r, &mut h)? < 26 {
            return Ok(Outcome { entries, complete: false });
        }
        let (flags, method) = (le16(&h, 2), le16(&h, 4));
        let (mut csize, mut size) = (le32(&h, 14) as u64, le32(&h, 18) as u64);
        let (nl, el) = (le16(&h, 22) as usize, le16(&h, 24) as usize);
        let mut name_b = vec![0u8; nl];
        let mut extra = vec![0u8; el];
        if read_upto(&mut r, &mut name_b)? < nl || read_upto(&mut r, &mut extra)? < el {
            return Ok(Outcome { entries, complete: false });
        }
        // ZIP64: the values that did not fit in 32 bits, in the order size, compressed size.
        let mut x = 0usize;
        while x + 4 <= extra.len() {
            let (id, sz) = (le16(&extra, x), le16(&extra, x + 2) as usize);
            if id == 1 {
                let mut p = x + 4;
                if size == 0xFFFF_FFFF && p + 8 <= extra.len() {
                    size = le64(&extra, p);
                    p += 8;
                }
                if csize == 0xFFFF_FFFF && p + 8 <= extra.len() {
                    csize = le64(&extra, p);
                }
            }
            x += 4 + sz;
        }
        if flags & 1 != 0 {
            return Ok(Outcome { entries, complete: false }); // encrypted entries can't be walked without the table
        }
        let streamed = flags & 8 != 0;
        let name = entry_name(&name_b, flags);
        let header = Header { is_dir: name.ends_with('/'), name, method, flags, csize: (!streamed).then_some(csize), size: (!streamed).then_some(size) };
        entries += 1;
        let start = r.stream_position()?;
        match method {
            0 => {
                if streamed {
                    return Ok(Outcome { entries, complete: false }); // where a stored entry ends is only in the table
                }
                let mut t = (&mut r).take(csize);
                let go = on_entry(&header, &mut t);
                let _ = io::copy(&mut t, &mut io::sink());
                let end = start + csize;
                if r.stream_position()? != end {
                    // The file ended inside the entry.
                    return Ok(Outcome { entries, complete: false });
                }
                if !go {
                    return Ok(Outcome { entries, complete: false });
                }
            }
            8 => {
                let mut dec = flate2::bufread::DeflateDecoder::new(&mut r);
                let go = on_entry(&header, &mut dec);
                let broken = io::copy(&mut dec, &mut io::sink()).is_err();
                let used = dec.total_in();
                drop(dec);
                if broken {
                    return Ok(Outcome { entries, complete: false });
                }
                if streamed {
                    if !skip_descriptor(&mut r, used)? {
                        return Ok(Outcome { entries, complete: false });
                    }
                } else {
                    r.seek(SeekFrom::Start(start + csize))?;
                }
                if !go {
                    return Ok(Outcome { entries, complete: false });
                }
            }
            _ => {
                if streamed {
                    return Ok(Outcome { entries, complete: false });
                }
                r.seek(SeekFrom::Start(start + csize))?;
            }
        }
    }
}

/// Decode as much of `rd` as there is into `out` (when given); the count of bytes and whether the entry ended cleanly.
pub fn drain(rd: &mut dyn Read, mut out: Option<&mut dyn io::Write>) -> (u64, bool) {
    let mut buf = vec![0u8; 128 * 1024];
    let mut n = 0u64;
    loop {
        match rd.read(&mut buf) {
            Ok(0) => return (n, true),
            Ok(k) => {
                if let Some(o) = out.as_deref_mut() {
                    if o.write_all(&buf[..k]).is_err() {
                        return (n, false);
                    }
                }
                n += k as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return (n, false),
        }
    }
}

/// The first picture of a zip without a table of contents: the first entry (in the file's order) whose name `is_picture`
/// accepts and whose decoded bytes are at most `limit`. Returns its name and bytes.
pub fn first_picture(path: &Path, limit: u64, is_picture: &dyn Fn(&str) -> bool) -> Result<(String, Vec<u8>), String> {
    let mut found: Option<(String, Vec<u8>)> = None;
    let mut seen = 0usize;
    let r = walk(path, &mut |h, rd| {
        seen += 1;
        if h.is_dir || !is_picture(&h.name) {
            return seen < 4000;
        }
        let mut bytes = Vec::new();
        let mut take = rd.take(limit + 1);
        let (_, ok) = drain(&mut take, Some(&mut bytes));
        if ok && bytes.len() as u64 <= limit && !bytes.is_empty() {
            found = Some((h.name.clone(), bytes));
        }
        false
    });
    match (found, r) {
        (Some(f), _) => Ok(f),
        (None, Ok(_)) => Err("no picture could be read from this damaged zip".into()),
        (None, Err(e)) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::DeflateEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn crc(data: &[u8]) -> u32 {
        let mut c = flate2::Crc::new();
        c.update(data);
        c.sum()
    }

    /// A streamed entry: flag bit 3, sizes zero in the header, a descriptor after the data.
    fn streamed_entry(name: &str, data: &[u8], with_sig: bool) -> Vec<u8> {
        let mut e = DeflateEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        let comp = e.finish().unwrap();
        let mut v = Vec::new();
        v.extend_from_slice(b"PK\x03\x04");
        v.extend_from_slice(&[20, 0, 8, 0, 8, 0, 0, 0, 0, 0]); // version, flags=8, method 8, time, date
        v.extend_from_slice(&[0u8; 12]); // crc, csize, size = 0
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(&comp);
        if with_sig {
            v.extend_from_slice(b"PK\x07\x08");
        }
        v.extend_from_slice(&crc(data).to_le_bytes());
        v.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v
    }

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("nv_zipscan_{name}_{}.zip", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn noise(n: usize) -> Vec<u8> {
        let mut x = 0x1234_5678u32;
        (0..n).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect()
    }

    #[test]
    fn legacy_japanese_names_are_read_in_shift_jis() {
        // Shift-JIS bytes of the three katakana "te su to" and ".png", no UTF-8 flag.
        let sjis = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, b'.', b'p', b'n', b'g'];
        let name = decode_name(&sjis, 0);
        assert_eq!(name.chars().count(), 7);
        assert!(name.ends_with(".png") && name.chars().next().is_some_and(|c| ('\u{30A0}'..='\u{30FF}').contains(&c)), "{name}");
        assert_eq!(decode_name("\u{30C6}.png".as_bytes(), 0), "\u{30C6}.png", "valid UTF-8 stays UTF-8");
        assert_eq!(decode_name(b"plain.png", 0), "plain.png");
    }

    #[test]
    fn streamed_entries_are_found_without_a_table() {
        let (a, b) = (noise(200_000), b"second entry second entry second entry".to_vec());
        let mut z = streamed_entry("a/one.png", &a, true);
        z.extend(streamed_entry("two.txt", &b, false));
        let p = temp("whole", &z);
        let mut seen = Vec::new();
        let out = walk(&p, &mut |h, rd| {
            let (n, ok) = drain(rd, None);
            seen.push((h.name.clone(), n, ok));
            true
        })
        .unwrap();
        assert_eq!(seen, vec![("a/one.png".to_string(), 200_000, true), ("two.txt".to_string(), b.len() as u64, true)]);
        assert_eq!(out.entries, 2);
        assert!(!out.complete, "no central directory: the walk ends at the end of the file");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn a_cut_off_last_entry_keeps_what_arrived() {
        let (a, b) = (noise(100_000), noise(300_000));
        let mut z = streamed_entry("a.bin", &a, true);
        let second = streamed_entry("b.bin", &b, true);
        z.extend_from_slice(&second[..second.len() / 2]);
        let p = temp("cut", &z);
        let mut seen = Vec::new();
        let out = walk(&p, &mut |h, rd| {
            let (n, ok) = drain(rd, None);
            seen.push((h.name.clone(), n, ok));
            true
        })
        .unwrap();
        assert_eq!(seen[0], ("a.bin".to_string(), 100_000, true));
        assert_eq!(seen[1].0, "b.bin");
        assert!(!seen[1].2 && seen[1].1 > 0 && seen[1].1 < 300_000, "partial: {:?}", seen[1]);
        assert_eq!(out.entries, 2);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn the_first_picture_is_read_from_a_damaged_zip() {
        let mut z = streamed_entry("readme.txt", b"hello", true);
        z.extend(streamed_entry("pics/1.png", &noise(5000), true));
        let p = temp("pic", &z);
        let (name, bytes) = first_picture(&p, 1 << 20, &|n| n.ends_with(".png")).unwrap();
        assert_eq!((name.as_str(), bytes.len()), ("pics/1.png", 5000));
        let _ = std::fs::remove_file(p);
    }
}
