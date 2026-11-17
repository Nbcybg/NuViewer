//! Archives as folders: list one, unpack the pictures of one, with a password when it needs one. ZIP / CBZ is read natively
//! (stored, deflate, ZIP64, the classic ZipCrypto password); every other format (7z, rar, tar, gz, xz, bz2, zst, iso ...) and
//! AES-encrypted zips go through 7-Zip when it is installed (or named in Settings) and through Windows' own `tar.exe`
//! when it is not (no passwords there). Nothing is read into memory whole: entries are streamed to files.

use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Path inside the archive, `/` separators.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub encrypted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArcError {
    /// It needs a password (or the one given is wrong).
    Password,
    Other(String),
}

impl std::fmt::Display for ArcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArcError::Password => write!(f, "a password is needed (or this one is wrong)"),
            ArcError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for ArcError {
    fn from(e: io::Error) -> Self {
        ArcError::Other(e.to_string())
    }
}

type Res<T> = Result<T, ArcError>;

/// Extensions of files that are archives.
pub const ARCHIVE_EXTS: &[&str] = &[
    "zip", "cbz", "7z", "cb7", "rar", "cbr", "tar", "tgz", "gz", "bz2", "xz", "zst", "cab", "dmg", "iso", "wim", "esd", "swm", "cpio", "arj", "lzh", "lha", "z", "taz", "lz", "lzma", "lz4", "tbz", "tbz2", "tb2", "txz", "tlz", "tzst", "rpm", "deb", "ar", "zipx", "jar", "war", "ear", "apk", "aab", "xpi", "vsix", "nupkg", "whl", "ipa", "crx", "appx", "msix", "kmz", "pk3", "pk4",
];

/// Is `path` an archive (by name; `.tar.gz` and friends included)?
pub fn is_archive(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).map(|e| ARCHIVE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

fn is_zip(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("zip" | "cbz"))
}

// ---- finding the tools -------------------------------------------------------------------------------------------------

/// 7-Zip: the path from Settings if it exists, else the usual places and PATH.
pub fn seven_zip(configured: &str) -> Option<PathBuf> {
    let c = configured.trim().trim_matches('"');
    if !c.is_empty() {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut places: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            places.push(base.join("7-Zip").join("7z.exe"));
            places.push(base.join("Programs").join("7-Zip").join("7z.exe"));
            places.push(base.join("PeaZip").join("res").join("bin").join("7z").join("7z.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for exe in ["7z.exe", "7zz.exe", "7za.exe"] {
                places.push(dir.join(exe));
            }
        }
    }
    places.into_iter().find(|p| p.is_file())
}

/// Windows' own bsdtar (Windows 10 and later).
pub fn tar_exe() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from)?;
    Some(root.join("System32").join("tar.exe")).filter(|p| p.is_file())
}

fn quiet(mut c: Command) -> Command {
    use std::os::windows::process::CommandExt;
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(0x0800_0000 /* CREATE_NO_WINDOW */);
    c
}

// ---- listing ---------------------------------------------------------------------------------------------------------------

/// Everything in the archive. `tool` is the 7-Zip path from Settings (may be empty).
pub fn list(path: &Path, password: Option<&str>, tool: &str) -> Res<Vec<Entry>> {
    list_ex(path, password, tool).map(|(v, _)| v)
}

/// Like `list`, and says whether the archive is damaged (cut off, or without its table of contents) so that only what could
/// be read from the front is listed.
pub fn list_ex(path: &Path, password: Option<&str>, tool: &str) -> Res<(Vec<Entry>, bool)> {
    let mut zip_error: Option<String> = None;
    if is_zip(path) {
        match zip_list(path) {
            Ok(v) if !v.is_empty() => return Ok((v, false)),
            Ok(_) => {}
            Err(ArcError::Other(e)) => zip_error = Some(e),
            Err(e) => return Err(e),
        }
        // No usable table of contents: walk the entries from the front, like 7-Zip does.
        if let Ok(v) = recover_list(path) {
            if !v.is_empty() {
                return Ok((v, true));
            }
        }
        if let (Some(e), None, None) = (&zip_error, seven_zip(tool), tar_exe()) {
            return Err(ArcError::Other(e.clone()));
        }
    }
    if let Some(z) = seven_zip(tool) {
        return seven_list(&z, path, password);
    }
    if let Some(t) = tar_exe() {
        let mut c = Command::new(t);
        c.arg("-tvf").arg(path);
        let out = quiet(c).output()?;
        if !out.status.success() {
            return Err(ArcError::Other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
        }
        // bsdtar -tv: "-rw-r--r--  0 user group   1234 Jan  1 12:00 name"
        let text = String::from_utf8_lossy(&out.stdout);
        return Ok((
            text.lines()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.split_whitespace().collect();
                    let size = parts.get(4)?.parse::<u64>().ok()?;
                    let name = l.splitn(9, char::is_whitespace).last().map(|s| s.trim().to_string()).filter(|_| parts.len() >= 9)?;
                    Some(Entry { is_dir: name.ends_with('/'), name, size, encrypted: false })
                })
                .collect(),
            false,
        ));
    }
    Err(ArcError::Other("this kind of archive needs 7-Zip (set its path in Settings > Archives & PDF)".into()))
}

fn seven_list(exe: &Path, archive: &Path, password: Option<&str>) -> Res<(Vec<Entry>, bool)> {
    let mut c = Command::new(exe);
    c.args(["l", "-slt", "-ba", "-sccUTF-8", "-y"]).arg(format!("-p{}", password.unwrap_or(""))).arg(archive);
    let out = quiet(c).output()?;
    let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
    let text = String::from_utf8_lossy(&out.stdout);
    let mut entries = Vec::new();
    let (mut name, mut size, mut dir, mut enc) = (String::new(), 0u64, false, false);
    let flush = |name: &mut String, size: &mut u64, dir: &mut bool, enc: &mut bool, entries: &mut Vec<Entry>| {
        if !name.is_empty() {
            entries.push(Entry { name: name.replace('\\', "/"), size: *size, is_dir: *dir, encrypted: *enc });
        }
        *name = String::new();
        *size = 0;
        *dir = false;
        *enc = false;
    };
    for line in text.lines() {
        if line.is_empty() {
            flush(&mut name, &mut size, &mut dir, &mut enc, &mut entries);
        } else if let Some(v) = line.strip_prefix("Path = ") {
            if !name.is_empty() {
                flush(&mut name, &mut size, &mut dir, &mut enc, &mut entries);
            }
            name = v.to_string();
        } else if let Some(v) = line.strip_prefix("Size = ") {
            size = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Folder = ") {
            dir = v.trim() == "+";
        } else if let Some(v) = line.strip_prefix("Attributes = ") {
            dir |= v.starts_with('D');
        } else if let Some(v) = line.strip_prefix("Encrypted = ") {
            enc = v.trim() == "+";
        }
    }
    flush(&mut name, &mut size, &mut dir, &mut enc, &mut entries);
    // 7-Zip lists the part it can read of a cut-off or damaged archive and exits with an error code: that is not "can't open".
    // (The archive's own line "Path = file.zip" is the first block when there is a header; -ba drops it, so nothing to skip.)
    if !out.status.success() {
        if entries.is_empty() {
            if err.contains("password") || err.contains("encrypted") || err.contains("wrong") {
                return Err(ArcError::Password);
            }
            return Err(ArcError::Other(String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("7-Zip could not read this archive").to_string()));
        }
        return Ok((entries, true));
    }
    Ok((entries, false))
}

// ---- unpacking -------------------------------------------------------------------------------------------------------------

/// A name from an archive made safe to join onto a folder: no `..`, no drive, no characters Windows refuses.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') {
            return None;
        }
        let clean: String = part.chars().map(|c| if "<>\"|?*".contains(c) || (c as u32) < 32 { '_' } else { c }).collect();
        out.push(clean.trim_end_matches(['.', ' ']));
    }
    (out.components().count() > 0).then_some(out)
}

/// Unpack the entries whose names are in `wanted` (all files when empty) into `dest`. `progress(done, total)` is called after
/// each file; return false from `keep_going` to stop.
pub fn extract(path: &Path, dest: &Path, wanted: &[String], password: Option<&str>, tool: &str, progress: &mut dyn FnMut(usize, usize) -> bool) -> Res<()> {
    std::fs::create_dir_all(dest)?;
    if is_zip(path) {
        let has_table = zip_entries(path).map(|t| !t.is_empty()).unwrap_or(false);
        if has_table {
            match zip_extract(path, dest, wanted, password, progress) {
                Err(ArcError::Other(e)) if e.starts_with("needs-tool") => {} // AES: below
                other => return other,
            }
        } else {
            // No table of contents (cut off, or streamed): unpack what can be read from the front.
            match recover_extract(path, dest, wanted, progress) {
                Ok(n) if n > 0 => return Ok(()),
                Err(ArcError::Other(e)) if e == "stopped" => return Err(ArcError::Other(e)),
                _ => {}
            }
        }
    }
    if let Some(z) = seven_zip(tool) {
        let mut c = Command::new(z);
        c.args(["x", "-y", "-aoa", "-sccUTF-8", "-scsUTF-8"]).arg(format!("-o{}", dest.display())).arg(format!("-p{}", password.unwrap_or(""))).arg(path);
        let list_file;
        if !wanted.is_empty() {
            list_file = dest.join(".want.txt");
            std::fs::write(&list_file, wanted.join("\n"))?;
            c.arg(format!("@{}", list_file.display()));
        }
        let out = quiet(c).output()?;
        let _ = std::fs::remove_file(dest.join(".want.txt"));
        if out.status.success() {
            progress(1, 1);
            return Ok(());
        }
        // A damaged archive: 7-Zip unpacks what it can and exits with an error code. That is still a result.
        if out.status.code() == Some(1) || (out.status.code() == Some(2) && has_files(dest)) {
            progress(1, 1);
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase() + &String::from_utf8_lossy(&out.stdout).to_lowercase();
        if err.contains("wrong password") || err.contains("password") {
            return Err(ArcError::Password);
        }
        return Err(ArcError::Other(String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("7-Zip could not unpack this archive").to_string()));
    }
    if let Some(t) = tar_exe() {
        let mut c = Command::new(t);
        c.arg("-xf").arg(path).arg("-C").arg(dest);
        if let Some(p) = password.filter(|p| !p.is_empty()) {
            c.arg("--passphrase").arg(p);
        }
        for w in wanted {
            c.arg(w);
        }
        let out = quiet(c).output()?;
        if out.status.success() {
            progress(1, 1);
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        if err.contains("passphrase") || err.contains("password") || err.contains("encrypted") {
            return Err(ArcError::Password);
        }
        return Err(ArcError::Other(String::from_utf8_lossy(&out.stderr).trim().lines().next().unwrap_or("tar could not unpack this archive").to_string()));
    }
    Err(ArcError::Other("this kind of archive needs 7-Zip (set its path in Settings > Archives & PDF)".into()))
}

/// Files being unpacked carry this extension until they are whole (the gallery's folder listing leaves them out).
pub const PART_EXT: &str = "nvpart";

fn part_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{PART_EXT}"));
    target.with_file_name(name)
}

/// Is there any file (not a folder) under `dir`?
fn has_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|rd| rd.flatten().any(|e| match e.metadata() { Ok(m) if m.is_dir() => has_files(&e.path()), Ok(m) => m.len() > 0 && e.path().extension().map_or(true, |x| x != PART_EXT), Err(_) => false })).unwrap_or(false)
}

/// The entries of a zip without a (usable) table of contents, found by walking it from the front. A cut-off last entry is
/// listed with the size that could be read.
fn recover_list(path: &Path) -> Res<Vec<Entry>> {
    let mut out: Vec<Entry> = Vec::new();
    nv_codecs::zipscan::walk(path, &mut |h, rd| {
        let (n, _) = nv_codecs::zipscan::drain(rd, None);
        if !h.is_dir && (n > 0 || h.size == Some(0)) || h.is_dir {
            out.push(Entry { name: h.name.replace('\\', "/"), size: n, is_dir: h.is_dir, encrypted: false });
        }
        true
    })?;
    Ok(out)
}

/// Unpack the entries named in `wanted` (all when empty) of a zip without a table of contents, in the order they are in the
/// file. Returns how many files were written; a cut-off last entry is written as far as it goes.
fn recover_extract(path: &Path, dest: &Path, wanted: &[String], progress: &mut dyn FnMut(usize, usize) -> bool) -> Res<usize> {
    let mut count = 0usize;
    let mut failure: Option<ArcError> = None;
    let total = wanted.len();
    nv_codecs::zipscan::walk(path, &mut |h, rd| {
        if h.is_dir {
            return true;
        }
        let name = h.name.replace('\\', "/");
        if !wanted.is_empty() && !wanted.iter().any(|w| *w == name) {
            return true;
        }
        let Some(rel) = safe_relative(&name) else { return true };
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                failure = Some(e.into());
                return false;
            }
        }
        let part = part_path(&target);
        let file = match std::fs::File::create(&part) {
            Ok(f) => f,
            Err(e) => {
                failure = Some(e.into());
                return false;
            }
        };
        let mut w = std::io::BufWriter::new(file);
        let (n, whole) = nv_codecs::zipscan::drain(rd, Some(&mut w));
        let _ = w.flush();
        drop(w);
        if n == 0 && !whole {
            let _ = std::fs::remove_file(&part);
            return false;
        }
        if std::fs::rename(&part, &target).is_err() {
            let _ = std::fs::remove_file(&part);
            return false;
        }
        count += 1;
        if !progress(count, total.max(count)) {
            failure = Some(ArcError::Other("stopped".into()));
            return false;
        }
        whole
    })?;
    match failure {
        Some(e) => Err(e),
        None => Ok(count),
    }
}

// ---- native ZIP --------------------------------------------------------------------------------------------------------------

struct ZipEntry {
    name: String,
    method: u16,
    flags: u16,
    crc: u32,
    comp: u64,
    size: u64,
    offset: u64,
    dos_time: u16,
    is_dir: bool,
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7]])
}

fn zip_entries(path: &Path) -> Res<Vec<ZipEntry>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let tail_len = len.min(65_557 + 22);
    f.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail)?;
    let eocd = (0..tail.len().saturating_sub(21)).rev().find(|&i| &tail[i..i + 4] == b"PK\x05\x06").ok_or_else(|| ArcError::Other("not a zip file".into()))?;
    let (mut count, mut cd_size, mut cd_off) = (u16le(&tail, eocd + 10) as u64, u32le(&tail, eocd + 12) as u64, u32le(&tail, eocd + 16) as u64);
    if (count == 0xFFFF || cd_off == 0xFFFF_FFFF) && eocd >= 20 && &tail[eocd - 20..eocd - 16] == b"PK\x06\x07" {
        let loc = u64le(&tail, eocd - 12);
        f.seek(SeekFrom::Start(loc))?;
        let mut z = [0u8; 56];
        f.read_exact(&mut z)?;
        if &z[0..4] == b"PK\x06\x06" {
            count = u64le(&z, 32);
            cd_size = u64le(&z, 40);
            cd_off = u64le(&z, 48);
        }
    }
    if cd_size > 256 * 1024 * 1024 {
        return Err(ArcError::Other("the zip's directory is too big".into()));
    }
    f.seek(SeekFrom::Start(cd_off))?;
    let mut cd = vec![0u8; cd_size as usize];
    f.read_exact(&mut cd)?;
    let mut out = Vec::new();
    let mut p = 0usize;
    for _ in 0..count {
        if p + 46 > cd.len() || &cd[p..p + 4] != b"PK\x01\x02" {
            break;
        }
        let (flags, method, dos_time, crc) = (u16le(&cd, p + 8), u16le(&cd, p + 10), u16le(&cd, p + 12), u32le(&cd, p + 16));
        let (mut comp, mut size, mut offset) = (u32le(&cd, p + 20) as u64, u32le(&cd, p + 24) as u64, u32le(&cd, p + 42) as u64);
        let (nl, el, cl) = (u16le(&cd, p + 28) as usize, u16le(&cd, p + 30) as usize, u16le(&cd, p + 32) as usize);
        let name_b = cd.get(p + 46..p + 46 + nl).ok_or_else(|| ArcError::Other("damaged zip directory".into()))?;
        let extra = cd.get(p + 46 + nl..p + 46 + nl + el).unwrap_or(&[]);
        // ZIP64 extra field: the values that did not fit, in order.
        let mut e = 0usize;
        while e + 4 <= extra.len() {
            let (id, sz) = (u16le(extra, e), u16le(extra, e + 2) as usize);
            if id == 1 {
                let mut q = e + 4;
                if size == 0xFFFF_FFFF && q + 8 <= extra.len() {
                    size = u64le(extra, q);
                    q += 8;
                }
                if comp == 0xFFFF_FFFF && q + 8 <= extra.len() {
                    comp = u64le(extra, q);
                    q += 8;
                }
                if offset == 0xFFFF_FFFF && q + 8 <= extra.len() {
                    offset = u64le(extra, q);
                }
            }
            e += 4 + sz;
        }
        let name = nv_codecs::zipscan::decode_name(name_b, flags).replace('\\', "/");
        out.push(ZipEntry { is_dir: name.ends_with('/'), name, method, flags, crc, comp, size, offset, dos_time });
        p += 46 + nl + el + cl;
    }
    Ok(out)
}

fn zip_list(path: &Path) -> Res<Vec<Entry>> {
    Ok(zip_entries(path)?.into_iter().map(|e| Entry { name: e.name, size: e.size, is_dir: e.is_dir, encrypted: e.flags & 1 != 0 }).collect())
}

/// The classic zip password cipher.
struct ZipCrypto {
    k: [u32; 3],
}

fn crc_step(crc: u32, b: u8) -> u32 {
    let mut c = (crc ^ b as u32) & 0xFF;
    for _ in 0..8 {
        c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
    }
    c ^ (crc >> 8)
}

impl ZipCrypto {
    fn new(password: &[u8]) -> ZipCrypto {
        let mut z = ZipCrypto { k: [0x1234_5678, 0x2345_6789, 0x3456_7890] };
        for &b in password {
            z.update(b);
        }
        z
    }
    fn update(&mut self, b: u8) {
        self.k[0] = crc_step(self.k[0], b);
        self.k[1] = self.k[1].wrapping_add(self.k[0] & 0xFF).wrapping_mul(134_775_813).wrapping_add(1);
        self.k[2] = crc_step(self.k[2], (self.k[1] >> 24) as u8);
    }
    fn next(&self) -> u8 {
        let t = (self.k[2] | 2) as u16;
        (t.wrapping_mul(t ^ 1) >> 8) as u8
    }
    fn decrypt(&mut self, c: u8) -> u8 {
        let p = c ^ self.next();
        self.update(p);
        p
    }
}

/// A reader that undoes ZipCrypto on the way through.
struct Decrypting<R: Read> {
    inner: R,
    z: ZipCrypto,
}

impl<R: Read> Read for Decrypting<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        for b in &mut buf[..n] {
            *b = self.z.decrypt(*b);
        }
        Ok(n)
    }
}

fn zip_extract(path: &Path, dest: &Path, wanted: &[String], password: Option<&str>, progress: &mut dyn FnMut(usize, usize) -> bool) -> Res<()> {
    let entries = zip_entries(path)?;
    let mut f = std::io::BufReader::with_capacity(256 * 1024, std::fs::File::open(path)?);
    let todo: Vec<&ZipEntry> = entries.iter().filter(|e| !e.is_dir && (wanted.is_empty() || wanted.iter().any(|w| w == &e.name))).collect();
    let total = todo.len();
    for (i, e) in todo.iter().enumerate() {
        let Some(rel) = safe_relative(&e.name) else { continue };
        if e.method != 0 && e.method != 8 {
            return Err(ArcError::Other(if e.method == 99 { "needs-tool: AES".into() } else { format!("needs-tool: zip method {}", e.method) }));
        }
        f.seek(SeekFrom::Start(e.offset))?;
        let mut lh = [0u8; 30];
        f.read_exact(&mut lh)?;
        if &lh[0..4] != b"PK\x03\x04" {
            return Err(ArcError::Other("damaged zip entry".into()));
        }
        let skip = u16le(&lh, 26) as i64 + u16le(&lh, 28) as i64;
        f.seek(SeekFrom::Current(skip))?;
        let encrypted = e.flags & 1 != 0;
        let mut raw: Box<dyn Read> = if encrypted {
            let pw = password.ok_or(ArcError::Password)?;
            let mut z = ZipCrypto::new(pw.as_bytes());
            let mut head = [0u8; 12];
            f.read_exact(&mut head)?;
            let mut last = 0u8;
            for b in head {
                last = z.decrypt(b);
            }
            // The last header byte is the high byte of the CRC (or of the time, when sizes follow the data).
            let check = if e.flags & 8 != 0 { (e.dos_time >> 8) as u8 } else { (e.crc >> 24) as u8 };
            if last != check {
                return Err(ArcError::Password);
            }
            Box::new(Decrypting { inner: (&mut f).take(e.comp.saturating_sub(12)), z })
        } else {
            Box::new((&mut f).take(e.comp))
        };
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Written under a temporary name and renamed when whole, so a folder that is being read while the archive is still
        // unpacking (see `PART_EXT`) never shows half a picture.
        let part = part_path(&target);
        let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
        let r = if e.method == 8 { io::copy(&mut flate2::read::DeflateDecoder::new(&mut raw), &mut out) } else { io::copy(&mut raw, &mut out) };
        if let Err(err) = r {
            drop(out);
            let _ = std::fs::remove_file(&part);
            // A wrong password that passed the one-byte check shows up as garbage in the deflate stream.
            return Err(if encrypted { ArcError::Password } else { ArcError::Other(err.to_string()) });
        }
        out.flush()?;
        drop(out);
        std::fs::rename(&part, &target)?;
        if !progress(i + 1, total) {
            return Err(ArcError::Other("stopped".into()));
        }
    }
    Ok(())
}

// ---- cache of unpacked archives ------------------------------------------------------------------------------------------------

/// Folder where the pictures of `archive` are unpacked: one folder per archive version (path, size, date).
pub fn cache_dir(archive: &Path) -> PathBuf {
    let meta = std::fs::metadata(archive).ok();
    let (size, mtime) = meta.map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))).unwrap_or((0, 0));
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in archive.to_string_lossy().to_lowercase().bytes().chain(size.to_le_bytes()).chain(mtime.to_le_bytes()) {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
    }
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("arc").join(format!("{h:016x}"))
}

/// Delete the least recently used unpacked archives until the cache is under `max_bytes`.
pub fn prune_cache(max_bytes: u64) {
    let root = cache_dir(Path::new("x")).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let Ok(rd) = std::fs::read_dir(&root) else { return };
    let mut dirs: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), dir_size(&e.path()), e.path()))
        .collect();
    let mut total: u64 = dirs.iter().map(|d| d.1).sum();
    dirs.sort_by_key(|d| d.0);
    for (_, size, p) in dirs {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_dir_all(&p).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

fn dir_size(p: &Path) -> u64 {
    std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| match e.metadata() { Ok(m) if m.is_dir() => dir_size(&e.path()), Ok(m) => m.len(), Err(_) => 0 }).sum()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_arc_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_relative("a/b/c.jpg"), Some(PathBuf::from("a").join("b").join("c.jpg")));
        assert_eq!(safe_relative("../evil.jpg"), None);
        assert_eq!(safe_relative("C:/x.jpg"), None);
        assert_eq!(safe_relative("a/b?.jpg"), Some(PathBuf::from("a").join("b_.jpg")));
        assert_eq!(safe_relative("/"), None);
    }

    /// Build a zip with our own writer, read it back through the native reader.
    #[test]
    fn zip_round_trip() {
        let dir = temp("zip");
        let src = dir.join("pics");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "hello hello hello hello hello").unwrap();
        std::fs::write(src.join("sub").join("b.png"), vec![7u8; 5000]).unwrap();
        let zip = dir.join("t.zip");
        crate::archive::write_zip(&zip, &crate::archive::collect(&[src.clone()]), 5, &mut |_, _| true).unwrap();
        let list = list(&zip, None, "").unwrap();
        assert!(list.iter().any(|e| e.name == "pics/a.txt" && e.size == 29 && !e.encrypted));
        let out = dir.join("out");
        extract(&zip, &out, &[], None, "", &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read(out.join("pics").join("sub").join("b.png")).unwrap(), vec![7u8; 5000]);
        assert_eq!(std::fs::read_to_string(out.join("pics").join("a.txt")).unwrap(), "hello hello hello hello hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A zip with one ZipCrypto-encrypted stored entry, made here with the same cipher.
    fn encrypted_zip(password: &str, data: &[u8]) -> Vec<u8> {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc = crc_step(crc, b);
        }
        let crc = !crc;
        let mut z = ZipCrypto::new(password.as_bytes());
        let mut enc = |b: u8, z: &mut ZipCrypto| {
            let c = b ^ z.next();
            z.update(b);
            c
        };
        let mut body = Vec::new();
        for i in 0..12u8 {
            let plain = if i == 11 { (crc >> 24) as u8 } else { i.wrapping_mul(37) };
            body.push(enc(plain, &mut z));
        }
        for &b in data {
            body.push(enc(b, &mut z));
        }
        let name = b"secret.txt";
        let mut v = Vec::new();
        v.extend_from_slice(b"PK\x03\x04");
        v.extend_from_slice(&[20, 0, 1, 0, 0, 0, 0, 0, 0, 0]); // version, flags=1 (encrypted), method 0, time, date
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(name);
        v.extend_from_slice(&body);
        let cd_off = v.len() as u32;
        v.extend_from_slice(b"PK\x01\x02");
        v.extend_from_slice(&[20, 0, 20, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&(name.len() as u16).to_le_bytes());
        v.extend_from_slice(&[0u8; 12]); // extra, comment, disk, attributes
        v.extend_from_slice(&0u32.to_le_bytes()); // offset of the local header
        v.extend_from_slice(name);
        let cd_size = v.len() as u32 - cd_off;
        v.extend_from_slice(b"PK\x05\x06\0\0\0\0\x01\0\x01\0");
        v.extend_from_slice(&cd_size.to_le_bytes());
        v.extend_from_slice(&cd_off.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v
    }

    #[test]
    fn zipcrypto_password() {
        let dir = temp("crypto");
        let zip = dir.join("e.zip");
        std::fs::write(&zip, encrypted_zip("sesame", b"top secret picture bytes")).unwrap();
        let l = list(&zip, None, "").unwrap();
        assert!(l[0].encrypted);
        let out = dir.join("out");
        assert_eq!(extract(&zip, &out, &[], None, "", &mut |_, _| true), Err(ArcError::Password));
        assert_eq!(extract(&zip, &out, &[], Some("wrong"), "", &mut |_, _| true), Err(ArcError::Password));
        extract(&zip, &out, &[], Some("sesame"), "", &mut |_, _| true).unwrap();
        assert_eq!(std::fs::read(out.join("secret.txt")).unwrap(), b"top secret picture bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
