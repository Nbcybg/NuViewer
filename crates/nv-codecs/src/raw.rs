//! Camera RAW: show the largest JPEG preview embedded in the file.
//!
//! TIFF-based RAWs (CR2/NEF/ARW/DNG/ORF/RW2/PEF...), CR3 (ISO-BMFF), RAF and CRW all carry one or more
//! full JPEG streams. Rather than parse every vendor's container we scan for JPEG start-of-image markers,
//! read each candidate's frame size from its SOF header, and decode the biggest that works.
//! Develop-from-sensor-data is deliberately not done here.

use crate::{jpeg, orient, Image};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// (width, height) from a baseline/extended/progressive JPEG header starting at `b[0]`.
/// Lossless-JPEG (SOF3), used for raw sensor data in DNG/CR2, is rejected on purpose.
fn jpeg_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2;
    for _ in 0..64 {
        if i + 4 > b.len() || b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        if m == 0xFF {
            i += 1; // fill byte
            continue;
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
            i += 2;
            continue;
        }
        if matches!(m, 0xC0 | 0xC1 | 0xC2) {
            if i + 9 > b.len() {
                return None;
            }
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return (w >= 16 && h >= 16).then_some((w, h));
        }
        if m == 0xDA || m == 0xD9 || m == 0xC3 {
            return None;
        }
        i += 2 + u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
    }
    None
}

pub fn decode_preview(bytes: &[u8]) -> Result<Image, String> {
    // Orientation of the RAW itself, for previews that don't carry their own tag.
    decode_preview_with(bytes, orient::exif_orientation(bytes))
}

/// `decode_preview` with the container's orientation supplied by the caller (for when `bytes` is only part of the file).
fn decode_preview_with(bytes: &[u8], container_orientation: Option<u32>) -> Result<Image, String> {
    // Candidate JPEG streams: (offset, pixel count).
    let mut cands: Vec<(usize, u64)> = Vec::new();
    let mut p = 0;
    while p + 3 < bytes.len() {
        match bytes[p..].iter().position(|&c| c == 0xFF) {
            None => break,
            Some(off) => p += off,
        }
        if p + 3 < bytes.len() && bytes[p + 1] == 0xD8 && bytes[p + 2] == 0xFF {
            if let Some((w, h)) = jpeg_dims(&bytes[p..]) {
                cands.push((p, w as u64 * h as u64));
            }
            p += 3;
        } else {
            p += 1;
        }
    }
    if cands.is_empty() {
        return Err("raw: no embedded JPEG preview".into());
    }
    cands.sort_by(|a, b| b.1.cmp(&a.1));

    let mut last_err = String::new();
    for &(off, _) in cands.iter().take(4) {
        match jpeg::decode(&bytes[off..], container_orientation) {
            Ok(img) => return Ok(img),
            Err(e) => last_err = e,
        }
    }
    Err(format!("raw: preview decode failed ({last_err})"))
}

/// Fujifilm RAF: the 160-byte header starts with "FUJIFILMCCD-RAW" and stores, as big-endian u32s, the offset (byte 84)
/// and length (byte 88) of the full-size JPEG preview. That JPEG is ~10% of the file, so there is no need to read the
/// sensor data behind it. Returns `(offset, length)` only if the range is plausible and lies inside the file.
pub fn raf_preview_range(header: &[u8], file_len: u64) -> Option<(u64, usize)> {
    if header.len() < 92 || !header.starts_with(b"FUJIFILMCCD-RAW") {
        return None;
    }
    let off = u32::from_be_bytes([header[84], header[85], header[86], header[87]]) as u64;
    let len = u32::from_be_bytes([header[88], header[89], header[90], header[91]]) as u64;
    if off < 92 || !(1024..=(256 << 20)).contains(&len) || off.checked_add(len)? > file_len {
        return None;
    }
    Some((off, len as usize))
}

/// Read only the embedded JPEG of a RAF (a header read plus one ranged read). `None` means "not a RAF we understand":
/// the caller then falls back to reading the whole file and scanning it.
pub fn read_raf_preview(path: &Path) -> Option<Vec<u8>> {
    let mut f = std::fs::File::open(path).ok()?;
    let file_len = f.metadata().ok()?.len();
    let mut head = [0u8; 160];
    f.read_exact(&mut head).ok()?;
    let (off, len) = raf_preview_range(&head, file_len)?;
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    buf.starts_with(&[0xFF, 0xD8]).then_some(buf)
}

// ---- TIFF-based RAWs (NEF, CR2, DNG, ORF, PEF, RW2, ...) -----------------------------------------------------------
//
// These files are a TIFF IFD tree. Full-size previews are listed in it either as JPEGInterchangeFormat/Length
// (tags 513/514) or as a single JPEG strip (compression 6/7), in IFD0 or one of its SubIFDs. Walking the tree only
// needs a few tiny reads, after which we fetch just the chosen JPEG instead of the whole 20-60 MB file.

/// A preview smaller than this is not worth showing instead of scanning the whole file for something bigger
/// (for example a Sony preview that lives in the MakerNote, which this IFD walk does not look into).
const MIN_PREVIEW_PX: u64 = 1_000_000;

fn read_at<R: Read + Seek>(r: &mut R, off: u64, len: usize) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(off)).ok()?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Byte order of a TIFF container.
struct Endian(bool); // true = little endian

impl Endian {
    fn u16(&self, b: &[u8]) -> u32 {
        (if self.0 { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) }) as u32
    }
    fn u32(&self, b: &[u8]) -> u32 {
        if self.0 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }
    }
}

#[derive(Default)]
struct IfdInfo {
    orientation: Option<u32>,
    compression: Option<u32>,
    dims: Option<(u32, u32)>,
    jpeg_off: Option<u64>,
    jpeg_len: Option<u64>,
    strip_offsets: Vec<u64>,
    strip_counts: Vec<u64>,
    tile_offsets: Vec<u64>,
    tile_counts: Vec<u64>,
    subifds: Vec<u64>,
    next: u64,
}

/// Read the handful of tags we care about from the IFD at `off`. Anything malformed gives `None`.
fn parse_ifd<R: Read + Seek>(r: &mut R, e: &Endian, off: u64, file_len: u64) -> Option<IfdInfo> {
    if off < 8 || off.checked_add(2)? > file_len {
        return None;
    }
    let n = e.u16(&read_at(r, off, 2)?) as usize;
    if n == 0 || n > 512 {
        return None;
    }
    let body = read_at(r, off + 2, n * 12 + 4)?;
    let mut info = IfdInfo { next: e.u32(&body[n * 12..]) as u64, ..Default::default() };
    let (mut w, mut h) = (None, None);
    for i in 0..n {
        let ent = &body[i * 12..i * 12 + 12];
        let (tag, typ, cnt) = (e.u16(&ent[0..2]), e.u16(&ent[2..4]), e.u32(&ent[4..8]) as usize);
        // Strip/tile tables can be long (a 24 MP DNG has 384 tiles); everything else is a handful of values.
        let cap = if matches!(tag, 273 | 279 | 324 | 325) { 4096 } else { 64 };
        if !matches!(tag, 256 | 257 | 259 | 273 | 274 | 279 | 324 | 325 | 330 | 513 | 514) || cnt == 0 || cnt > cap {
            continue;
        }
        let unit = match typ {
            3 => 2,       // SHORT
            4 | 13 => 4,  // LONG / IFD
            _ => continue,
        };
        let size = cnt * unit;
        let raw = if size <= 4 {
            ent[8..8 + size].to_vec()
        } else {
            match read_at(r, e.u32(&ent[8..12]) as u64, size) {
                Some(v) => v,
                None => continue,
            }
        };
        let vals: Vec<u64> = raw.chunks_exact(unit).map(|c| if unit == 2 { e.u16(c) as u64 } else { e.u32(c) as u64 }).collect();
        match tag {
            256 => w = Some(vals[0] as u32),
            257 => h = Some(vals[0] as u32),
            259 => info.compression = Some(vals[0] as u32),
            274 => info.orientation = Some(vals[0] as u32),
            273 => info.strip_offsets = vals,
            279 => info.strip_counts = vals,
            324 => info.tile_offsets = vals,
            325 => info.tile_counts = vals,
            330 => info.subifds = vals,
            513 => info.jpeg_off = Some(vals[0]),
            _ => info.jpeg_len = Some(vals[0]), // 514
        }
    }
    info.dims = w.zip(h);
    Some(info)
}

/// What a walk over the IFD tree learned about the file.
struct Walk {
    /// IFD0's Orientation, which applies to previews that carry no orientation of their own.
    orientation: Option<u32>,
    /// Largest image (in pixels) any IFD claims: normally the sensor.
    max_px: u64,
    /// JPEG-looking `(offset, length)` ranges listed by the IFDs.
    cands: Vec<(u64, u64)>,
    /// Data blocks `(image pixels, start, end)` that IFDs point at (strips or tiles): the sensor data lives here.
    raws: Vec<(u64, u64, u64)>,
}

/// Walk IFD0, its SubIFDs and the IFD chain with tiny reads. `None` if this isn't a TIFF-style file we understand.
fn walk<R: Read + Seek>(r: &mut R, file_len: u64) -> Option<Walk> {
    let head = read_at(r, 0, 8)?;
    let e = match &head[..2] {
        b"II" => Endian(true),
        b"MM" => Endian(false),
        _ => return None,
    };
    // 42 = TIFF; 0x55 = Panasonic RW2; 0x4F52 / 0x5352 = Olympus ORF.
    if !matches!(e.u16(&head[2..4]), 42 | 0x55 | 0x4F52 | 0x5352) {
        return None;
    }
    let mut queue = vec![e.u32(&head[4..8]) as u64];
    let mut seen: Vec<u64> = Vec::new();
    let (mut orientation, mut max_px) = (None, 0u64);
    let mut cands: Vec<(u64, u64)> = Vec::new();
    let mut raws: Vec<(u64, u64, u64)> = Vec::new();
    let mut qi = 0;
    while qi < queue.len() && seen.len() < 24 {
        let off = queue[qi];
        qi += 1;
        if seen.contains(&off) {
            continue;
        }
        seen.push(off);
        let info = match parse_ifd(r, &e, off, file_len) {
            Some(i) => i,
            None if seen.len() == 1 => return None, // IFD0 itself is broken
            None => continue,
        };
        if seen.len() == 1 {
            orientation = info.orientation;
        }
        if let Some((w, h)) = info.dims {
            max_px = max_px.max(w as u64 * h as u64);
        }
        if let (Some(o), Some(l)) = (info.jpeg_off, info.jpeg_len) {
            cands.push((o, l));
        }
        if matches!(info.compression, Some(6) | Some(7)) && info.strip_offsets.len() == 1 && info.strip_counts.len() == 1 {
            cands.push((info.strip_offsets[0], info.strip_counts[0]));
        }
        let (offs, cnts) = if !info.tile_offsets.is_empty() { (&info.tile_offsets, &info.tile_counts) } else { (&info.strip_offsets, &info.strip_counts) };
        if !offs.is_empty() && offs.len() == cnts.len() {
            let start = offs.iter().copied().min().unwrap_or(0);
            let end = offs.iter().zip(cnts.iter()).map(|(o, c)| o.saturating_add(*c)).max().unwrap_or(0);
            raws.push((info.dims.map_or(0, |(w, h)| w as u64 * h as u64), start, end));
        }
        queue.extend(info.subifds);
        if info.next != 0 {
            queue.push(info.next);
        }
    }
    Some(Walk { orientation, max_px, cands, raws })
}

/// Find the best embedded JPEG preview from the IFD tree, reading only that JPEG.
/// Returns the JPEG bytes and the container's (IFD0) orientation, or `None` if the file isn't a TIFF-style RAW we
/// understand or its best preview is too small / small compared with the sensor (then the caller scans the whole file).
fn tiff_preview<R: Read + Seek>(r: &mut R, file_len: u64, min_px: u64) -> Option<(Vec<u8>, Option<u32>)> {
    let Walk { orientation, max_px, mut cands, .. } = walk(r, file_len)?;
    cands.retain(|&(o, l)| o >= 8 && (1024..=(256u64 << 20)).contains(&l) && o.checked_add(l).is_some_and(|end| end <= file_len));
    cands.sort_by(|a, b| b.1.cmp(&a.1));
    cands.dedup();
    // Check each plausible candidate's real frame size from its header (this also rejects lossless sensor-data JPEGs).
    let mut best: Option<(u64, u64, u64)> = None; // (pixels, offset, length)
    for &(o, l) in cands.iter().take(6) {
        let Some(head) = read_at(r, o, l.min(128 * 1024) as usize) else { continue };
        if let Some((w, h)) = jpeg_dims(&head) {
            let px = w as u64 * h as u64;
            if best.map_or(true, |b| px > b.0) {
                best = Some((px, o, l));
            }
        }
    }
    let (px, o, l) = best?;
    if px < min_px || (max_px > 0 && px * 4 < max_px) {
        return None;
    }
    let bytes = read_at(r, o, l as usize)?;
    bytes.starts_with(&[0xFF, 0xD8]).then_some((bytes, orientation))
}

/// Ranged-read version of the preview lookup for TIFF-style RAW files (see `tiff_preview`).
pub fn read_tiff_preview(path: &Path) -> Option<(Vec<u8>, Option<u32>)> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    tiff_preview(&mut f, len, MIN_PREVIEW_PX)
}

// Second stage, for files whose IFDs don't list a big enough JPEG (for example Sony ARW keeps its preview in the
// MakerNote). The sensor data is one big contiguous block; everything outside it is metadata and previews. So read
// only the bytes *before* the block and the bytes *after* it, and run the usual "largest embedded JPEG" scan on those.
// That finds exactly what a whole-file scan would, without ever touching the sensor data.

/// Don't treat anything smaller than this as the sensor block.
const MIN_SENSOR_BLOCK: u64 = 4 << 20;
/// Most we're willing to read before the sensor block, and after it (otherwise: fall back to reading everything).
const MAX_HEAD: u64 = 24 << 20;
const MAX_TAIL: u64 = 4 << 20;

fn sensor_skip_scan<R: Read + Seek>(r: &mut R, file_len: u64, w: &Walk) -> Option<Image> {
    // The sensor image: the IFD with the most pixels that points at a data block.
    let &(_, start, end) = w.raws.iter().max_by_key(|x| (x.0, x.2.saturating_sub(x.1)))?;
    let size = end.checked_sub(start)?;
    let plausible = start >= 8 && end <= file_len && size >= MIN_SENSOR_BLOCK && size * 10 >= file_len * 4 && start <= MAX_HEAD && file_len - end <= MAX_TAIL;
    if !plausible {
        return None;
    }
    // A block that opens with a real (baseline/progressive) JPEG is a preview, not sensor data: never skip those.
    if jpeg_dims(&read_at(r, start, size.min(128 * 1024) as usize)?).is_some() {
        return None;
    }
    let mut best = decode_preview_with(&read_at(r, 0, start as usize)?, w.orientation).ok();
    let tail_len = file_len - end;
    if tail_len >= 2048 {
        let tail = read_at(r, end, tail_len as usize).and_then(|t| decode_preview_with(&t, w.orientation).ok());
        if let Some(t) = tail {
            let px = |i: &Image| i.width as u64 * i.height as u64;
            if best.as_ref().map_or(true, |b| px(&t) > px(b)) {
                best = Some(t);
            }
        }
    }
    best
}

/// Scan only the non-sensor parts of a TIFF-style RAW (see `sensor_skip_scan`). `None` means "can't do it safely".
pub fn decode_tiff_skipping_sensor(path: &Path) -> Option<Image> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let w = walk(&mut f, len)?;
    sensor_skip_scan(&mut f, len, &w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_bytes(w: u16, h: u16) -> Vec<u8> {
        let rgb: Vec<u8> = (0..w as usize * h as usize * 3).map(|i| (i % 251) as u8).collect();
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 80).encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb).unwrap();
        out
    }

    #[test]
    fn picks_largest_embedded_jpeg() {
        let mut file = vec![0x49, 0x49, 0x2A, 0x00, 1, 2, 3, 4, 0xFF, 0x00, 0xFF, 0xD8, 0x00];
        file.extend(jpeg_bytes(32, 24)); // small thumbnail
        file.extend([0u8; 50]);
        file.extend(jpeg_bytes(160, 120)); // the preview
        file.extend([9u8; 20]);
        let img = decode_preview(&file).unwrap();
        assert_eq!((img.width, img.height), (160, 120));
    }

    /// A RAF-shaped file: 160-byte header pointing at a JPEG, then fake "sensor data" that must never be read.
    fn fake_raf(jpeg: &[u8], tail: usize) -> Vec<u8> {
        let mut f = vec![0u8; 160];
        f[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
        f[84..88].copy_from_slice(&160u32.to_be_bytes());
        f[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
        f.extend_from_slice(jpeg);
        f.extend(vec![0xABu8; tail]);
        f
    }

    #[test]
    fn raf_header_range_is_validated() {
        let jpeg = jpeg_bytes(64, 48);
        let file = fake_raf(&jpeg, 5000);
        assert_eq!(raf_preview_range(&file[..160], file.len() as u64), Some((160, jpeg.len())));
        // Range running past the end of the file, wrong magic, tiny header and absurd lengths are all refused.
        assert_eq!(raf_preview_range(&file[..160], 200), None);
        let mut bad = file[..160].to_vec();
        bad[0] = b'X';
        assert_eq!(raf_preview_range(&bad, file.len() as u64), None);
        assert_eq!(raf_preview_range(&file[..50], file.len() as u64), None);
        let mut zero = file[..160].to_vec();
        zero[88..92].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(raf_preview_range(&zero, file.len() as u64), None);
        let mut huge = file[..160].to_vec();
        huge[88..92].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(raf_preview_range(&huge, u64::MAX), None);
    }

    #[test]
    fn raf_ranged_read_returns_exactly_the_jpeg() {
        let jpeg = jpeg_bytes(96, 64);
        let dir = std::env::temp_dir().join(format!("nv_raf_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.raf");
        std::fs::write(&path, fake_raf(&jpeg, 20_000)).unwrap();
        assert_eq!(read_raf_preview(&path).as_deref(), Some(&jpeg[..]));
        // Same picture as the slow whole-file scan.
        let full = decode_preview(&std::fs::read(&path).unwrap()).unwrap();
        let fast = crate::decode_file(&path).unwrap();
        assert_eq!((fast.width, fast.height, &fast.rgba), (full.width, full.height, &full.rgba));
        // A file that merely ends in .raf but isn't one is not claimed.
        std::fs::write(&path, vec![0u8; 400]).unwrap();
        assert_eq!(read_raf_preview(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- synthetic TIFF-style RAWs ----

    fn put16(le: bool, v: &mut Vec<u8>, x: u16) {
        v.extend(if le { x.to_le_bytes() } else { x.to_be_bytes() });
    }
    fn put32(le: bool, v: &mut Vec<u8>, x: u32) {
        v.extend(if le { x.to_le_bytes() } else { x.to_be_bytes() });
    }
    /// One IFD entry whose value fits inline (a SHORT when `typ` is 3, otherwise a LONG).
    fn entry(le: bool, v: &mut Vec<u8>, tag: u16, typ: u16, cnt: u32, val: u32) {
        put16(le, v, tag);
        put16(le, v, typ);
        put32(le, v, cnt);
        if typ == 3 {
            put16(le, v, val as u16);
            v.extend([0, 0]);
        } else {
            put32(le, v, val);
        }
    }
    fn header(le: bool) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend(if le { *b"II" } else { *b"MM" });
        put16(le, &mut f, 42);
        put32(le, &mut f, 8);
        f
    }

    /// NEF-like: IFD0 (orientation 6, two SubIFDs); SubIFD A is the JPEG strip, SubIFD B the sensor image.
    fn nef_like(le: bool, jpeg: &[u8], jpeg_dims: (u32, u32), sensor: (u32, u32), tail: usize) -> Vec<u8> {
        let mut f = header(le);
        put16(le, &mut f, 2); // IFD0 @8, 2 entries
        entry(le, &mut f, 274, 3, 1, 6);
        entry(le, &mut f, 330, 4, 2, 38);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 38);
        put32(le, &mut f, 46); // SubIFD offsets
        put32(le, &mut f, 112);
        put16(le, &mut f, 5); // SubIFD A @46: the JPEG
        entry(le, &mut f, 256, 4, 1, jpeg_dims.0);
        entry(le, &mut f, 257, 4, 1, jpeg_dims.1);
        entry(le, &mut f, 259, 3, 1, 7);
        entry(le, &mut f, 273, 4, 1, 154);
        entry(le, &mut f, 279, 4, 1, jpeg.len() as u32);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 112);
        put16(le, &mut f, 3); // SubIFD B @112: the raw image
        entry(le, &mut f, 256, 4, 1, sensor.0);
        entry(le, &mut f, 257, 4, 1, sensor.1);
        entry(le, &mut f, 259, 3, 1, 1);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 154);
        f.extend_from_slice(jpeg);
        f.extend(vec![0xCDu8; tail]);
        f
    }

    /// CR2-like: IFD0 describes the sensor and carries the preview as JPEGInterchangeFormat/Length.
    fn cr2_like(le: bool, jpeg: &[u8], sensor: (u32, u32)) -> Vec<u8> {
        let mut f = header(le);
        put16(le, &mut f, 4);
        entry(le, &mut f, 256, 4, 1, sensor.0);
        entry(le, &mut f, 257, 4, 1, sensor.1);
        entry(le, &mut f, 513, 4, 1, 62);
        entry(le, &mut f, 514, 4, 1, jpeg.len() as u32);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 62);
        f.extend_from_slice(jpeg);
        f.extend(vec![0xCDu8; 50_000]);
        f
    }

    struct Counting {
        inner: std::io::Cursor<Vec<u8>>,
        bytes: usize,
    }
    impl Read for Counting {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(b)?;
            self.bytes += n;
            Ok(n)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(p)
        }
    }
    fn run(file: Vec<u8>, min_px: u64) -> (Option<(Vec<u8>, Option<u32>)>, usize) {
        let len = file.len() as u64;
        let mut r = Counting { inner: std::io::Cursor::new(file), bytes: 0 };
        (tiff_preview(&mut r, len, min_px), r.bytes)
    }

    #[test]
    fn tiff_subifd_preview_is_found_in_both_byte_orders_without_reading_the_rest() {
        let jpeg = jpeg_bytes(320, 240);
        for le in [true, false] {
            let file = nef_like(le, &jpeg, (320, 240), (600, 400), 200_000);
            let (got, read) = run(file, 1000);
            let (bytes, orientation) = got.expect("preview found");
            assert_eq!(bytes, jpeg, "le={le}");
            assert_eq!(orientation, Some(6), "container orientation is carried over, le={le}");
            assert!(read < jpeg.len() * 2 + 2000, "read {read} bytes for a {} byte preview", jpeg.len());
        }
    }

    #[test]
    fn tiff_jpeg_interchange_format_preview_is_found() {
        let jpeg = jpeg_bytes(320, 240);
        for le in [true, false] {
            let (got, _) = run(cr2_like(le, &jpeg, (600, 400)), 1000);
            assert_eq!(got.map(|g| g.0), Some(jpeg.clone()), "le={le}");
        }
    }

    #[test]
    fn tiff_tiny_or_small_previews_fall_back_to_the_full_scan() {
        // A thumbnail next to a big sensor image, and a preview under the absolute minimum: both defer to the scan.
        assert!(run(cr2_like(true, &jpeg_bytes(160, 120), (4000, 3000)), 1000).0.is_none());
        assert!(run(cr2_like(true, &jpeg_bytes(160, 120), (200, 150)), 100_000).0.is_none());
    }

    #[test]
    fn tiff_garbage_never_panics_or_loops() {
        assert!(run(vec![0u8; 100], 1000).0.is_none());
        assert!(run(b"II*\0\xFF\xFF\xFF\x7F....".to_vec(), 1000).0.is_none()); // IFD0 far outside the file
        // IFD0 whose "next" points back at itself.
        let mut f = header(true);
        put16(true, &mut f, 1);
        entry(true, &mut f, 256, 4, 1, 10);
        put32(true, &mut f, 8);
        f.extend(vec![0u8; 100]);
        assert!(run(f, 1000).0.is_none());
        // Not a TIFF at all.
        assert!(run(b"FUJIFILMCCD-RAW ................".to_vec(), 1000).0.is_none());
    }

    #[test]
    fn tiff_ranged_read_matches_whole_file_decode_for_a_real_path() {
        let jpeg = jpeg_bytes(320, 240);
        let dir = std::env::temp_dir().join(format!("nv_tiff_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.nef");
        std::fs::write(&path, nef_like(true, &jpeg, (320, 240), (600, 400), 5_000)).unwrap();
        // The real entry point uses a 1 MP minimum, so this small preview defers to the scan...
        assert!(read_tiff_preview(&path).is_none());
        // ...which still finds the same picture, with the container's orientation.
        let img = crate::decode_file(&path).unwrap();
        assert_eq!((img.width, img.height, img.orientation), (320, 240, 6));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ARW-like: a small JPEG preview before a big sensor strip, then a few trailing bytes.
    /// `raw_first_bytes` is what the sensor block starts with (a real JPEG would mean it's not sensor data).
    fn tiff_with_sensor(le: bool, jpeg: &[u8], raw_len: usize, raw_first_bytes: &[u8], tail: usize) -> Vec<u8> {
        let mut f = header(le);
        put16(le, &mut f, 2);
        entry(le, &mut f, 274, 3, 1, 6);
        entry(le, &mut f, 330, 4, 2, 38);
        put32(le, &mut f, 0);
        put32(le, &mut f, 46);
        put32(le, &mut f, 112);
        put16(le, &mut f, 5); // SubIFD A @46: the small JPEG
        entry(le, &mut f, 256, 4, 1, 160);
        entry(le, &mut f, 257, 4, 1, 120);
        entry(le, &mut f, 259, 3, 1, 7);
        entry(le, &mut f, 273, 4, 1, 178);
        entry(le, &mut f, 279, 4, 1, jpeg.len() as u32);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 112);
        put16(le, &mut f, 5); // SubIFD B @112: the sensor strip
        entry(le, &mut f, 256, 4, 1, 6000);
        entry(le, &mut f, 257, 4, 1, 4000);
        entry(le, &mut f, 259, 3, 1, 1);
        entry(le, &mut f, 273, 4, 1, (178 + jpeg.len()) as u32);
        entry(le, &mut f, 279, 4, 1, raw_len as u32);
        put32(le, &mut f, 0);
        assert_eq!(f.len(), 178);
        f.extend_from_slice(jpeg);
        let mut raw = vec![0xCDu8; raw_len];
        raw[..raw_first_bytes.len()].copy_from_slice(raw_first_bytes);
        f.extend(raw);
        f.extend(vec![0x77u8; tail]);
        f
    }

    fn skip(file: Vec<u8>) -> (Option<Image>, usize) {
        let len = file.len() as u64;
        let mut r = Counting { inner: std::io::Cursor::new(file), bytes: 0 };
        let w = walk(&mut r, len);
        (w.and_then(|w| sensor_skip_scan(&mut r, len, &w)), r.bytes)
    }

    #[test]
    fn sensor_block_is_skipped_but_the_preview_around_it_is_found() {
        let jpeg = jpeg_bytes(160, 120);
        for le in [true, false] {
            let file = tiff_with_sensor(le, &jpeg, 5 << 20, &[1, 2, 3, 4], 300);
            let total = file.len();
            // Stage 1 refuses it (160x120 against a 6000x4000 sensor)...
            assert!(run(file.clone(), 1000).0.is_none());
            // ...stage 2 finds the same picture as a whole-file scan, with the container orientation, reading little.
            let (img, read) = skip(file.clone());
            let img = img.expect("found by skipping the sensor");
            let full = decode_preview_with(&file, Some(6)).unwrap();
            assert_eq!((img.width, img.height, img.orientation, &img.rgba), (full.width, full.height, full.orientation, &full.rgba), "le={le}");
            assert_eq!((img.width, img.height, img.orientation), (160, 120, 6));
            assert!(read < 300_000 && read * 10 < total, "read {read} of {total} bytes");
        }
    }

    #[test]
    fn sensor_skip_refuses_when_it_cannot_be_sure() {
        let jpeg = jpeg_bytes(160, 120);
        // Too little data to be a sensor block.
        assert!(skip(tiff_with_sensor(true, &jpeg, 1 << 20, &[], 300)).0.is_none());
        // A big unexplained tail could hide a larger preview.
        assert!(skip(tiff_with_sensor(true, &jpeg, 5 << 20, &[], 6 << 20)).0.is_none());
        // The "sensor" block is itself a JPEG: that's a preview, never skip it.
        let real_jpeg = jpeg_bytes(64, 64);
        assert!(skip(tiff_with_sensor(true, &jpeg, 5 << 20, &real_jpeg, 300)).0.is_none());
    }

    #[test]
    fn no_jpeg_is_an_error() {
        assert!(decode_preview(&[0u8; 1000]).is_err());
    }

    #[test]
    fn dims_reader_matches_encoder() {
        assert_eq!(jpeg_dims(&jpeg_bytes(77, 55)), Some((77, 55)));
    }
}
