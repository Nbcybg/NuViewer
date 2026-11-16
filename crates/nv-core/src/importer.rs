//! NuImporter's logic, with no window: where an imported picture goes and what it is called, the formats it can be saved in,
//! the pictures inside a PowerPoint / Word / zip / ISO, the folder a tethered camera writes to, and the Windows Image
//! Acquisition scripts for a scanner or a USB camera. The window and the screen / clipboard calls are in `nuimporter`.

use crate::prefs::Prefs;
use nv_editor::convert::{self, Out};
use nv_editor::raster::Pixels;
use std::path::{Path, PathBuf};

/// Where a picture came from (it is in the file name and in the list).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Scanner,
    Screenshot,
    Clipboard,
    Camera,
    Archive,
    Folder,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Scanner => "Scan",
            Source::Screenshot => "Screenshot",
            Source::Clipboard => "Clipboard",
            Source::Camera => "Camera",
            Source::Archive => "Import",
            Source::Folder => "Import",
        }
    }
}

/// What happens to a picture when it arrives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    /// Saved at once.
    Save,
    /// Opened in NuEditor first; saved (or not) when it is closed.
    Confirm,
    /// Put on the clipboard, nothing saved.
    Clipboard,
}

impl After {
    pub fn from_id(s: &str) -> After {
        match s {
            "save" => After::Save,
            "clipboard" => After::Clipboard,
            _ => After::Confirm,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            After::Save => "save",
            After::Confirm => "confirm",
            After::Clipboard => "clipboard",
        }
    }
}

/// The formats an import can be saved in (the first word is the settings value).
pub const FORMATS: &[(&str, &str)] = &[("png", "PNG (lossless)"), ("jpg", "JPEG"), ("webp", "WebP (lossless)"), ("bmp", "BMP"), ("tiff", "TIFF"), ("qoi", "QOI (lossless)"), ("tga", "TGA"), ("same", "As it was (pictures inside a file keep their own format)")];

pub fn out_for(format: &str, quality: u8) -> Out {
    match format {
        "jpg" => Out::Jpeg(quality),
        "webp" => Out::Webp,
        "bmp" => Out::Bmp,
        "tiff" => Out::Tiff,
        "qoi" => Out::Qoi,
        "tga" => Out::Tga,
        _ => Out::Png,
    }
}

/// The folder imports go to: the setting, else `Pictures\NuViewer Imports`.
pub fn folder_for(prefs: &Prefs, pictures: Option<PathBuf>) -> PathBuf {
    let d = prefs.import_dir.trim();
    if !d.is_empty() {
        return PathBuf::from(d);
    }
    pictures.unwrap_or_else(|| PathBuf::from(".")).join("NuViewer Imports")
}

/// The file name for an import: `{source}`, `{date}` (2025-06-01), `{time}` (14-30-05), `{n}` (a running number).
pub fn file_name(pattern: &str, source: &str, unix: i64, utc_offset_minutes: i32, n: usize, ext: &str) -> String {
    let (y, mo, d, h, mi, s) = crate::batch::civil_from_unix(unix, utc_offset_minutes);
    let name = pattern
        .replace("{source}", source)
        .replace("{date}", &format!("{y:04}-{mo:02}-{d:02}"))
        .replace("{time}", &format!("{h:02}-{mi:02}-{s:02}"))
        .replace("{n}", &format!("{n:03}"));
    let name: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let name = name.trim().trim_end_matches('.').to_string();
    format!("{}.{ext}", if name.is_empty() { source.to_string() } else { name })
}

/// Save RGBA pixels as an import. Returns the new file.
pub fn save_picture(prefs: &Prefs, pictures: Option<PathBuf>, source: Source, w: u32, h: u32, rgba: Vec<u8>, utc_offset_minutes: i32) -> Result<PathBuf, String> {
    let format = if prefs.import_format == "same" { "png" } else { prefs.import_format.as_str() };
    let out = out_for(format, prefs.import_quality);
    let bytes = convert::encode(&Pixels::new(w, h, rgba), out, [255, 255, 255], None)?;
    let dir = folder_for(prefs, pictures);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let unix = crate::batch::unix_of(std::time::SystemTime::now());
    let name = file_name(&prefs.import_name, source.name(), unix, utc_offset_minutes, 1, out.ext());
    let dst = crate::fileops::unique_path(&dir, std::ffi::OsStr::new(&name));
    nv_editor::save::write_atomic(&dst, &bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok(dst)
}

/// Copy a picture file as it is (or converted, unless the setting is "as it was") into the import folder.
pub fn save_file(prefs: &Prefs, pictures: Option<PathBuf>, source: Source, name_hint: &str, bytes: &[u8], utc_offset_minutes: i32) -> Result<PathBuf, String> {
    let dir = folder_for(prefs, pictures.clone());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if prefs.import_format == "same" {
        let clean = Path::new(name_hint).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "picture.png".into());
        let dst = crate::fileops::unique_path(&dir, std::ffi::OsStr::new(&clean));
        nv_editor::save::write_atomic(&dst, bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
        return Ok(dst);
    }
    let ext = Path::new(name_hint).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let img = nv_codecs::decode_bytes_hint(bytes, &ext)?.finish_for_cpu();
    save_picture(prefs, pictures, source, img.width, img.height, img.rgba, utc_offset_minutes)
}

// ---- pictures inside a container ---------------------------------------------------------------------------------

const PICTURE_EXTS: &[&str] = &["jpg", "jpeg", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff", "jxl", "avif", "heic", "emf", "wmf", "svg"];

pub fn is_picture_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    !n.contains("__macosx/") && n.rsplit('.').next().is_some_and(|e| PICTURE_EXTS.contains(&e))
}

/// Containers whose pictures can be taken out: Office files, OpenDocument, zip / cbz / epub, and ISO images.
pub fn is_container(path: &Path) -> bool {
    let e = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(e.as_str(), "pptx" | "pptm" | "ppsx" | "potx" | "docx" | "docm" | "xlsx" | "xlsm" | "odp" | "odt" | "ods" | "odg" | "zip" | "cbz" | "epub" | "iso" | "img" | "vsdx" | "pages" | "key" | "numbers" | "xps")
}

/// One picture of a container: its name inside and its bytes.
pub struct Piece {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Every picture inside `path`, biggest files last-cut at `limit` pieces. Office documents keep pictures in `ppt/media`,
/// `word/media` ...; any zip is searched whole; an ISO is read natively (ISO 9660 with Joliet names), anything else (UDF,
/// DVD images) goes through 7-Zip when it is installed.
pub fn pictures_in(path: &Path, limit: usize, seven_zip: &str) -> Result<Vec<Piece>, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if matches!(ext.as_str(), "iso" | "img") {
        return match iso_pictures(path, limit) {
            Ok(v) if !v.is_empty() => Ok(v),
            first => {
                if let Some(sz) = crate::archive_tools::seven_zip(seven_zip) {
                    seven_zip_pictures(path, &sz, limit)
                } else {
                    first.and_then(|_| Err("no pictures found (a UDF-only disc image needs 7-Zip to be read)".to_string()))
                }
            }
        };
    }
    nv_codecs::zip_pictures(path, limit).map(|v| v.into_iter().map(|(name, bytes)| Piece { name, bytes }).collect())
}

fn seven_zip_pictures(iso: &Path, sz: &Path, limit: usize) -> Result<Vec<Piece>, String> {
    let tmp = std::env::temp_dir().join(format!("nuviewer-iso-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut c = std::process::Command::new(sz);
    c.arg("x").arg("-y").arg(format!("-o{}", tmp.display()));
    for e in PICTURE_EXTS {
        c.arg(format!("-ir!*.{e}"));
    }
    c.arg(iso);
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let _ = c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    fn walk(d: &Path, out: &mut Vec<Piece>, limit: usize) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                if out.len() >= limit {
                    return;
                }
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out, limit);
                } else if is_picture_name(&p.to_string_lossy()) {
                    if let Ok(b) = std::fs::read(&p) {
                        out.push(Piece { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), bytes: b });
                    }
                }
            }
        }
    }
    walk(&tmp, &mut out, limit);
    let _ = std::fs::remove_dir_all(&tmp);
    if out.is_empty() { Err("no pictures found in this disc image".into()) } else { Ok(out) }
}

// ---- ISO 9660 ---------------------------------------------------------------------------------------------------------

const SECTOR: u64 = 2048;

struct Iso {
    file: std::fs::File,
    joliet: bool,
    root: (u32, u32),
}

fn read_at(f: &mut std::fs::File, off: u64, len: usize) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; len];
    f.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

impl Iso {
    fn open(path: &Path) -> Result<Iso, String> {
        let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut best: Option<(bool, (u32, u32))> = None;
        for sector in 16..40u64 {
            let Ok(d) = read_at(&mut file, sector * SECTOR, 2048) else { break };
            if &d[1..6] != b"CD001" {
                break;
            }
            let le32 = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
            let root = (le32(156 + 2), le32(156 + 10));
            match d[0] {
                1 if best.is_none() => best = Some((false, root)),
                2 if d[88] == b'%' && d[89] == b'/' => best = Some((true, root)), // Joliet: long Unicode names
                255 => break,
                _ => {}
            }
        }
        let (joliet, root) = best.ok_or("not an ISO 9660 disc image")?;
        Ok(Iso { file, joliet, root })
    }

    fn dir(&mut self, lba: u32, size: u32) -> Result<Vec<(String, bool, u32, u32)>, String> {
        let data = read_at(&mut self.file, lba as u64 * SECTOR, (size as usize).min(1 << 20))?;
        let mut out = Vec::new();
        let mut p = 0;
        while p < data.len() {
            let len = data[p] as usize;
            if len == 0 {
                p = (p / 2048 + 1) * 2048; // records never cross a sector: skip the padding
                continue;
            }
            if p + len > data.len() || len < 34 {
                break;
            }
            let r = &data[p..p + len];
            let (ext, sz, flags, nl) = (u32::from_le_bytes([r[2], r[3], r[4], r[5]]), u32::from_le_bytes([r[10], r[11], r[12], r[13]]), r[25], r[32] as usize);
            if 33 + nl <= len && nl > 0 && !(nl == 1 && (r[33] == 0 || r[33] == 1)) {
                let raw = &r[33..33 + nl];
                let name = if self.joliet {
                    let u: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    String::from_utf16_lossy(&u)
                } else {
                    String::from_utf8_lossy(raw).into_owned()
                };
                let name = name.split(';').next().unwrap_or("").to_string();
                out.push((name, flags & 2 != 0, ext, sz));
            }
            p += len;
        }
        Ok(out)
    }
}

pub fn iso_pictures(path: &Path, limit: usize) -> Result<Vec<Piece>, String> {
    let mut iso = Iso::open(path)?;
    let mut out = Vec::new();
    let mut stack = vec![(iso.root, 0usize)];
    let mut visited = 0;
    while let Some(((lba, size), depth)) = stack.pop() {
        visited += 1;
        if visited > 20_000 || depth > 12 {
            break;
        }
        for (name, is_dir, ext_lba, sz) in iso.dir(lba, size).unwrap_or_default() {
            if is_dir {
                stack.push(((ext_lba, sz), depth + 1));
            } else if is_picture_name(&name) && sz < 200 << 20 && out.len() < limit {
                if let Ok(bytes) = read_at(&mut iso.file, ext_lba as u64 * SECTOR, sz as usize) {
                    out.push(Piece { name, bytes });
                }
            }
        }
    }
    Ok(out)
}

// ---- camera on a cable or a card: a folder the pictures appear in ---------------------------------------------------------

/// What a tether watch has seen: files whose size has stopped changing are reported once.
#[derive(Default)]
pub struct Watch {
    seen: std::collections::HashSet<PathBuf>,
    pending: std::collections::HashMap<PathBuf, u64>,
}

impl Watch {
    /// Start from what is there now (only pictures that arrive afterwards count).
    pub fn new(dir: &Path) -> Watch {
        let mut w = Watch::default();
        for p in list_pictures(dir) {
            w.seen.insert(p);
        }
        w
    }

    /// Call every half second or so: the new, finished pictures since the last call.
    pub fn poll(&mut self, dir: &Path) -> Vec<PathBuf> {
        let mut ready = Vec::new();
        for p in list_pictures(dir) {
            if self.seen.contains(&p) {
                continue;
            }
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            match self.pending.get(&p) {
                Some(&was) if was == size && size > 0 => {
                    self.pending.remove(&p);
                    self.seen.insert(p.clone());
                    ready.push(p);
                }
                _ => {
                    self.pending.insert(p, size);
                }
            }
        }
        ready
    }
}

fn list_pictures(dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().is_some_and(|x| nv_codecs::is_supported_ext(&x.to_string_lossy())) && !nv_codecs::is_code_ext(&p.extension().unwrap().to_string_lossy()) {
                v.push(p);
            }
        }
    }
    v
}

// ---- Windows Image Acquisition (scanner, USB camera) via PowerShell ---------------------------------------------------------------

/// A PowerShell script that shows Windows' own scan / camera dialog and saves the result as PNG at `out` (the file is not
/// there when the user cancelled). `camera` = pictures from a camera instead of a scanner.
pub fn wia_script(out: &Path, camera: bool) -> String {
    format!(
        "$ErrorActionPreference='Stop'\n$dlg = New-Object -ComObject WIA.CommonDialog\n$img = $dlg.ShowAcquireImage({}, 1, 131072, '{{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}}', $false, $true, $false)\nif ($img -ne $null) {{ $img.SaveFile('{}') }}\n",
        if camera { 2 } else { 1 },
        out.display().to_string().replace('\'', "''")
    )
}

/// A script that takes a picture with the first WIA camera that supports it and saves it at `out`.
pub fn wia_take_picture_script(out: &Path) -> String {
    format!(
        "$ErrorActionPreference='Stop'\n$mgr = New-Object -ComObject WIA.DeviceManager\n$info = $mgr.DeviceInfos | Where-Object {{ $_.Type -eq 2 }} | Select-Object -First 1\nif ($info -eq $null) {{ Write-Output 'NO-CAMERA'; exit 3 }}\n$dev = $info.Connect()\n$before = $dev.Items.Count\n$null = $dev.ExecuteCommand('{{AF933CAC-ACAD-11D2-A093-00C04F72DC3C}}')\nStart-Sleep -Milliseconds 800\n$item = $dev.Items | Select-Object -Last 1\n$img = $item.Transfer('{{B96B3CAF-0728-11D3-9D7B-0000F81EF32E}}')\n$img.SaveFile('{}')\n",
        out.display().to_string().replace('\'', "''")
    )
}

/// A script that prints one line per WIA device: `scanner|name` or `camera|name`.
pub fn wia_list_script() -> &'static str {
    "$ErrorActionPreference='SilentlyContinue'\n$mgr = New-Object -ComObject WIA.DeviceManager\nforeach ($d in $mgr.DeviceInfos) { $t = if ($d.Type -eq 1) { 'scanner' } elseif ($d.Type -eq 2) { 'camera' } else { 'other' }; Write-Output ($t + '|' + $d.Properties.Item('Name').Value) }\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_pattern_and_stay_safe() {
        let n = file_name("{source} {date} {time} {n}", "Screenshot", 1_748_790_605, 0, 7, "png");
        assert_eq!(n, "Screenshot 2025-06-01 15-10-05 007.png");
        assert_eq!(file_name("a/b:{source}", "Scan", 0, 0, 1, "jpg"), "a_b_Scan.jpg");
        assert_eq!(file_name("  ", "Scan", 0, 0, 1, "jpg"), "Scan.jpg");
    }

    #[test]
    fn a_picture_is_saved_in_the_chosen_format_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("nv_import_{}", std::process::id()));
        let mut prefs = Prefs::default();
        prefs.import_dir = dir.display().to_string();
        prefs.import_name = "pic".into();
        prefs.import_format = "qoi".into();
        let rgba = vec![200u8; 4 * 4 * 4];
        let a = save_picture(&prefs, None, Source::Clipboard, 4, 4, rgba.clone(), 0).unwrap();
        let b = save_picture(&prefs, None, Source::Clipboard, 4, 4, rgba, 0).unwrap();
        assert_eq!(a.file_name().unwrap(), "pic.qoi");
        assert_eq!(b.file_name().unwrap(), "pic (2).qoi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_iso_9660_image_gives_its_pictures() {
        // Build a tiny ISO by hand: PVD at sector 16, terminator at 17, root directory at 20 with one file, data at 21.
        let png = {
            let mut v = Vec::new();
            let mut e = png::Encoder::new(&mut v, 2, 2);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[255u8; 12]).unwrap();
            v
        };
        let mut iso = vec![0u8; 22 * 2048 + 2048];
        let rec = |name: &str, lba: u32, size: u32, dir: bool| {
            let mut r = vec![0u8; 33 + name.len() + (name.len() + 1) % 2];
            r[0] = r.len() as u8;
            r[2..6].copy_from_slice(&lba.to_le_bytes());
            r[10..14].copy_from_slice(&size.to_le_bytes());
            r[25] = if dir { 2 } else { 0 };
            r[32] = name.len() as u8;
            r[33..33 + name.len()].copy_from_slice(name.as_bytes());
            r
        };
        let pvd = 16 * 2048;
        iso[pvd] = 1;
        iso[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let root = rec("\0", 20, 2048, true);
        iso[pvd + 156..pvd + 156 + root.len()].copy_from_slice(&root);
        iso[17 * 2048] = 255;
        iso[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"CD001");
        let file = rec("PHOTO.PNG;1", 21, png.len() as u32, false);
        iso[20 * 2048..20 * 2048 + file.len()].copy_from_slice(&file);
        iso[21 * 2048..21 * 2048 + png.len()].copy_from_slice(&png);
        let p = std::env::temp_dir().join(format!("nv_iso_{}.iso", std::process::id()));
        std::fs::write(&p, &iso).unwrap();
        let pieces = iso_pictures(&p, 10).unwrap();
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].name, "PHOTO.PNG");
        assert_eq!(pieces[0].bytes, png);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn a_tether_watch_reports_a_picture_once_it_stops_growing() {
        let dir = std::env::temp_dir().join(format!("nv_tether_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old.png"), b"x").unwrap();
        let mut w = Watch::new(&dir);
        assert!(w.poll(&dir).is_empty(), "what was there before the watch is not new");
        std::fs::write(dir.join("new.jpg"), b"abc").unwrap();
        assert!(w.poll(&dir).is_empty(), "first sight: the size may still be changing");
        std::fs::write(dir.join("new.jpg"), b"abcdef").unwrap();
        assert!(w.poll(&dir).is_empty(), "it grew");
        let ready = w.poll(&dir);
        assert_eq!(ready.len(), 1);
        assert!(w.poll(&dir).is_empty(), "reported once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scripts_name_the_wia_objects_and_quote_paths() {
        let s = wia_script(Path::new("C:\\Users\\O'Neil\\scan.png"), false);
        assert!(s.contains("WIA.CommonDialog") && s.contains("O''Neil"));
        assert!(wia_script(Path::new("x"), true).contains("ShowAcquireImage(2"));
        assert!(wia_take_picture_script(Path::new("x")).contains("AF933CAC"));
        assert!(wia_list_script().contains("DeviceInfos"));
    }

    #[test]
    fn container_and_picture_names() {
        assert!(is_container(Path::new("a.PPTX")) && is_container(Path::new("d.iso")) && !is_container(Path::new("a.jpg")));
        assert!(is_picture_name("ppt/media/image1.PNG") && !is_picture_name("ppt/slides/slide1.xml") && !is_picture_name("__MACOSX/x.jpg"));
    }
}
