//! Writing a .zip: the "Compress" command. Plain ZIP (no ZIP64), UTF-8 names, deflate for files that compress and
//! "stored" for those that are already compressed (JPEG, PNG, ...), so a folder of photos zips at disk speed.

use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// One thing to put in the archive.
#[derive(Clone, Debug, PartialEq)]
pub struct ZipEntry {
    pub source: PathBuf,
    /// Name inside the archive, with `/` separators (a folder ends with `/`).
    pub name: String,
    pub is_dir: bool,
}

const ZIP_MAX: u64 = 0xFFFF_FFFF - 1024 * 1024;

/// Extensions whose data is compressed already: deflating them again only costs time.
fn is_compressed_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "jpg" | "jpeg" | "jpe" | "jfif" | "png" | "webp" | "avif" | "heic" | "heif" | "jxl" | "gif" | "zip" | "7z" | "rar" | "gz" | "xz" | "bz2" | "zst" | "mp3" | "mp4" | "m4a" | "mkv" | "mov" | "webm" | "avi" | "docx" | "xlsx" | "pptx" | "jp2" | "raf" | "cr3" | "arw" | "nef"
    )
}

/// The entries for `paths` (files and folders): folders are walked, names are relative to the parent of each selected path,
/// so zipping `C:\Pics\Trip` gives `Trip/a.jpg`. Unreadable folders are skipped.
pub fn collect(paths: &[PathBuf]) -> Vec<ZipEntry> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<ZipEntry>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        if items.is_empty() {
            out.push(ZipEntry { source: dir.to_path_buf(), name: format!("{prefix}/"), is_dir: true });
        }
        for e in items {
            let name = format!("{prefix}/{}", e.file_name().to_string_lossy());
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path(), &name, out),
                Ok(t) if t.is_file() => out.push(ZipEntry { source: e.path(), name, is_dir: false }),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for p in paths {
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if p.is_dir() {
            walk(p, &name, &mut out);
        } else if p.is_file() {
            out.push(ZipEntry { source: p.clone(), name, is_dir: false });
        }
    }
    out
}

struct Counting<W: Write> {
    inner: W,
    count: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.count += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn dos_time(unix: i64, utc_offset_minutes: i32) -> (u16, u16) {
    let (y, mo, d, h, mi, s) = crate::batch::civil_from_unix(unix, utc_offset_minutes);
    let y = y.clamp(1980, 2107);
    (((h as u16) << 11) | ((mi as u16) << 5) | (s as u16 / 2), (((y - 1980) as u16) << 9) | ((mo as u16) << 5) | d as u16)
}

struct Done {
    name: String,
    method: u16,
    time: u16,
    date: u16,
    crc: u32,
    csize: u32,
    usize: u32,
    offset: u32,
    dir: bool,
}

fn put16(v: &mut Vec<u8>, n: u16) {
    v.extend(n.to_le_bytes());
}
fn put32(v: &mut Vec<u8>, n: u32) {
    v.extend(n.to_le_bytes());
}

/// Write `entries` to `dest`. `progress(done, total)` is called after every entry; return false from it to cancel (the
/// half-written file is removed and `Err("cancelled")` comes back).
pub fn write_zip(dest: &Path, entries: &[ZipEntry], utc_offset_minutes: i32, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<(), String> {
    if entries.len() >= 65_535 {
        return Err("too many files for a .zip (65534 at most)".into());
    }
    let result = write_inner(dest, entries, utc_offset_minutes, progress);
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn write_inner(dest: &Path, entries: &[ZipEntry], utc_offset_minutes: i32, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<(), String> {
    let file = std::fs::File::create(dest).map_err(|e| format!("can't create {}: {e}", dest.display()))?;
    let mut out = BufWriter::with_capacity(256 * 1024, file);
    let mut done: Vec<Done> = Vec::with_capacity(entries.len());
    let mut pos: u64 = 0;
    let mut buf = vec![0u8; 256 * 1024];
    for (n, e) in entries.iter().enumerate() {
        if pos > ZIP_MAX {
            return Err("the archive would be larger than 4 GB (not supported)".into());
        }
        let meta = std::fs::metadata(&e.source).map_err(|er| format!("{}: {er}", e.source.display()))?;
        let modified = meta.modified().map(crate::batch::unix_of).unwrap_or(0);
        let (time, date) = dos_time(modified, utc_offset_minutes);
        let name = e.name.replace('\\', "/");
        let offset = pos as u32;
        let method: u16 = if e.is_dir || e.source.extension().map(|x| is_compressed_ext(&x.to_string_lossy())).unwrap_or(false) { 0 } else { 8 };
        // Local header with zero sizes; patched below once they are known.
        let mut h = Vec::with_capacity(30 + name.len());
        put32(&mut h, 0x0403_4B50);
        put16(&mut h, 20);
        put16(&mut h, 0x0800); // UTF-8 names
        put16(&mut h, method);
        put16(&mut h, time);
        put16(&mut h, date);
        put32(&mut h, 0);
        put32(&mut h, 0);
        put32(&mut h, 0);
        put16(&mut h, name.len() as u16);
        put16(&mut h, 0);
        h.extend(name.as_bytes());
        out.write_all(&h).map_err(|er| er.to_string())?;
        pos += h.len() as u64;

        let (mut crc, mut csize, mut usize_) = (0u32, 0u64, 0u64);
        if !e.is_dir {
            let mut src = std::fs::File::open(&e.source).map_err(|er| format!("{}: {er}", e.source.display()))?;
            let mut crc_calc = flate2::Crc::new();
            if method == 0 {
                loop {
                    let got = src.read(&mut buf).map_err(|er| format!("{}: {er}", e.source.display()))?;
                    if got == 0 {
                        break;
                    }
                    crc_calc.update(&buf[..got]);
                    out.write_all(&buf[..got]).map_err(|er| er.to_string())?;
                    usize_ += got as u64;
                }
                csize = usize_;
            } else {
                let mut enc = flate2::write::DeflateEncoder::new(Counting { inner: &mut out, count: 0 }, flate2::Compression::new(6));
                loop {
                    let got = src.read(&mut buf).map_err(|er| format!("{}: {er}", e.source.display()))?;
                    if got == 0 {
                        break;
                    }
                    crc_calc.update(&buf[..got]);
                    enc.write_all(&buf[..got]).map_err(|er| er.to_string())?;
                    usize_ += got as u64;
                }
                let counting = enc.finish().map_err(|er| er.to_string())?;
                csize = counting.count;
            }
            crc = crc_calc.sum();
            if usize_ > ZIP_MAX || csize > ZIP_MAX {
                return Err(format!("{} is larger than 4 GB (not supported)", e.source.display()));
            }
            pos += csize;
            // Fill in the header.
            out.flush().map_err(|er| er.to_string())?;
            let f = out.get_mut();
            f.seek(SeekFrom::Start(offset as u64 + 14)).map_err(|er| er.to_string())?;
            let mut patch = Vec::with_capacity(12);
            put32(&mut patch, crc);
            put32(&mut patch, csize as u32);
            put32(&mut patch, usize_ as u32);
            f.write_all(&patch).map_err(|er| er.to_string())?;
            f.seek(SeekFrom::Start(pos)).map_err(|er| er.to_string())?;
        }
        done.push(Done { name, method, time, date, crc, csize: csize as u32, usize: usize_ as u32, offset, dir: e.is_dir });
        if !progress(n + 1, entries.len()) {
            return Err("cancelled".into());
        }
    }
    // Central directory.
    let cd_start = pos;
    let mut cd = Vec::new();
    for d in &done {
        put32(&mut cd, 0x0201_4B50);
        put16(&mut cd, 20);
        put16(&mut cd, 20);
        put16(&mut cd, 0x0800);
        put16(&mut cd, d.method);
        put16(&mut cd, d.time);
        put16(&mut cd, d.date);
        put32(&mut cd, d.crc);
        put32(&mut cd, d.csize);
        put32(&mut cd, d.usize);
        put16(&mut cd, d.name.len() as u16);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put16(&mut cd, 0);
        put32(&mut cd, if d.dir { 0x10 } else { 0x20 });
        put32(&mut cd, d.offset);
        cd.extend(d.name.as_bytes());
    }
    // The size of the directory proper (without the end record that follows) goes in the end record itself.
    let cd_len = cd.len() as u32;
    put32(&mut cd, 0x0605_4B50);
    put16(&mut cd, 0);
    put16(&mut cd, 0);
    put16(&mut cd, done.len() as u16);
    put16(&mut cd, done.len() as u16);
    put32(&mut cd, cd_len);
    put32(&mut cd, cd_start as u32);
    put16(&mut cd, 0);
    out.write_all(&cd).map_err(|er| er.to_string())?;
    out.flush().map_err(|er| er.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_zip_{tag}_{}_{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn u16_at(b: &[u8], i: usize) -> usize {
        u16::from_le_bytes([b[i], b[i + 1]]) as usize
    }
    fn u32_at(b: &[u8], i: usize) -> usize {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
    }

    /// Reads the archive back by hand: (name, bytes) of every entry, checking the CRC.
    fn read_back(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let eocd = zip.len() - 22;
        assert_eq!(u32_at(zip, eocd), 0x0605_4B50);
        let (count, cd_len, cd_off) = (u16_at(zip, eocd + 10), u32_at(zip, eocd + 12), u32_at(zip, eocd + 16));
        assert_eq!(cd_off + cd_len, eocd, "directory size and offset must meet the end record");
        let mut out = Vec::new();
        let mut p = cd_off;
        for _ in 0..count {
            assert_eq!(u32_at(zip, p), 0x0201_4B50);
            let (method, crc, csize, usize_) = (u16_at(zip, p + 10), u32_at(zip, p + 16), u32_at(zip, p + 20), u32_at(zip, p + 24));
            let (nl, el, cl, local) = (u16_at(zip, p + 28), u16_at(zip, p + 30), u16_at(zip, p + 32), u32_at(zip, p + 42));
            let name = String::from_utf8(zip[p + 46..p + 46 + nl].to_vec()).unwrap();
            assert_eq!(u32_at(zip, local), 0x0403_4B50);
            let (lnl, lel) = (u16_at(zip, local + 26), u16_at(zip, local + 28));
            assert_eq!(u32_at(zip, local + 14), crc, "local header crc");
            let data = &zip[local + 30 + lnl + lel..][..csize];
            let bytes = if method == 0 {
                data.to_vec()
            } else {
                let mut v = Vec::new();
                flate2::read::DeflateDecoder::new(data).read_to_end(&mut v).unwrap();
                v
            };
            assert_eq!(bytes.len(), usize_);
            let mut c = flate2::Crc::new();
            c.update(&bytes);
            assert_eq!(c.sum() as usize, crc, "crc of {name}");
            out.push((name, bytes));
            p += 46 + nl + el + cl;
        }
        out
    }

    #[test]
    fn a_folder_and_files_round_trip() {
        let d = temp_dir("rt");
        let tree = d.join("Trip");
        fs::create_dir_all(tree.join("sub")).unwrap();
        fs::create_dir_all(tree.join("empty")).unwrap();
        let text: Vec<u8> = (0..50_000u32).flat_map(|i| format!("line {i}\n").into_bytes()).collect();
        fs::write(tree.join("a.txt"), &text).unwrap();
        fs::write(tree.join("sub").join("รูป.jpg"), vec![7u8; 3000]).unwrap();
        fs::write(d.join("loose.bin"), b"hello").unwrap();
        fs::write(d.join("zero.txt"), b"").unwrap();

        let entries = collect(&[tree.clone(), d.join("loose.bin"), d.join("zero.txt")]);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Trip/a.txt", "Trip/empty/", "Trip/sub/รูป.jpg", "loose.bin", "zero.txt"]);

        let zip = d.join("out.zip");
        let mut seen = Vec::new();
        write_zip(&zip, &entries, 0, &mut |done, total| {
            seen.push((done, total));
            true
        })
        .unwrap();
        assert_eq!(seen.last(), Some(&(5, 5)));
        let bytes = fs::read(&zip).unwrap();
        let back = read_back(&bytes);
        assert_eq!(back.len(), 5);
        assert_eq!(back[0], ("Trip/a.txt".to_string(), text));
        assert_eq!(back[1], ("Trip/empty/".to_string(), Vec::new()));
        assert_eq!(back[2].1, vec![7u8; 3000]);
        assert_eq!(back[3].1, b"hello");
        assert_eq!(back[4].1, Vec::<u8>::new());
        assert!(bytes.len() < 400_000, "the text part must have been deflated ({} bytes)", bytes.len());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn cancelling_removes_the_partial_file() {
        let d = temp_dir("cancel");
        fs::write(d.join("a.txt"), "a").unwrap();
        fs::write(d.join("b.txt"), "b").unwrap();
        let entries = collect(&[d.join("a.txt"), d.join("b.txt")]);
        let zip = d.join("x.zip");
        let r = write_zip(&zip, &entries, 0, &mut |done, _| done < 1);
        assert_eq!(r, Err("cancelled".to_string()));
        assert!(!zip.exists());
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn already_compressed_files_are_stored() {
        assert!(is_compressed_ext("JPG") && is_compressed_ext("png") && !is_compressed_ext("txt") && !is_compressed_ext("tif"));
    }
}
