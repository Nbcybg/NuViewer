//! A minimal reader for OLE compound files (the container of 3ds Max scenes, old Office files, ...): enough to pull one named
//! stream out, and the picture a "SummaryInformation" stream may hold (3ds Max keeps a small preview there).

use crate::Image;

fn u16le(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u64le(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}

const FREE: u32 = 0xFFFF_FFFA; // sector numbers from here on are markers, not sectors

struct Entry {
    name: String,
    kind: u8,
    start: u32,
    size: u64,
}

pub struct Ole<'a> {
    d: &'a [u8],
    sector: usize,
    mini_sector: usize,
    cutoff: u64,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    entries: Vec<Entry>,
    ministream: Vec<u8>,
}

impl<'a> Ole<'a> {
    pub fn open(d: &'a [u8]) -> Option<Ole<'a>> {
        if !d.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            return None;
        }
        let sector = 1usize << u16le(d, 30)?.min(16);
        let mini_sector = 1usize << u16le(d, 32)?.min(16);
        let nfat = u32le(d, 44)? as usize;
        let dir_start = u32le(d, 48)?;
        let cutoff = u32le(d, 56)? as u64;
        let minifat_start = u32le(d, 60)?;
        let nminifat = u32le(d, 64)?;
        let mut difat_sector = u32le(d, 68)?;
        let ndifat = u32le(d, 72)?;
        let mut difat: Vec<u32> = (0..109).filter_map(|i| u32le(d, 76 + i * 4)).collect();
        for _ in 0..ndifat.min(4096) {
            let o = (difat_sector as usize + 1) * sector;
            for i in 0..sector / 4 - 1 {
                difat.push(u32le(d, o + i * 4)?);
            }
            difat_sector = u32le(d, o + sector - 4)?;
        }
        let mut me = Ole { d, sector, mini_sector, cutoff, fat: Vec::new(), minifat: Vec::new(), entries: Vec::new(), ministream: Vec::new() };
        for &s in difat.iter().take(nfat) {
            if s >= FREE {
                continue;
            }
            let o = (s as usize + 1) * sector;
            for i in 0..sector / 4 {
                me.fat.push(u32le(d, o + i * 4)?);
            }
        }
        let dir = me.chain(dir_start, usize::MAX)?;
        for i in 0..dir.len() / 128 {
            let e = &dir[i * 128..(i + 1) * 128];
            let nl = (u16le(e, 64)? as usize).saturating_sub(2).min(62);
            let name: String = char::decode_utf16(e[..nl].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))).map(|c| c.unwrap_or('?')).collect();
            me.entries.push(Entry { name, kind: e[66], start: u32le(e, 116)?, size: u64le(e, 120)? });
        }
        if nminifat > 0 {
            let mf = me.chain(minifat_start, usize::MAX)?;
            me.minifat = mf.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        }
        let root = me.entries.first()?;
        let (rs, rz) = (root.start, root.size as usize);
        if rz > 0 {
            me.ministream = me.chain(rs, rz.div_ceil(sector))?;
            me.ministream.truncate(rz);
        }
        Some(me)
    }

    /// The bytes of a sector chain (at most `max_sectors` of them).
    fn chain(&self, start: u32, max_sectors: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let (mut s, mut n) = (start, 0usize);
        while s < FREE && n < max_sectors && n < 1 << 24 {
            let o = (s as usize + 1) * self.sector;
            out.extend_from_slice(self.d.get(o..o + self.sector)?);
            s = *self.fat.get(s as usize)?;
            n += 1;
        }
        Some(out)
    }

    /// The contents of the stream called `name`.
    pub fn stream(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.entries.iter().find(|e| e.kind == 2 && e.name == name)?;
        if e.size < self.cutoff {
            let mut out = Vec::new();
            let mut s = e.start;
            while s < FREE && out.len() < e.size as usize {
                let o = s as usize * self.mini_sector;
                out.extend_from_slice(self.ministream.get(o..o + self.mini_sector)?);
                s = *self.minifat.get(s as usize)?;
            }
            out.truncate(e.size as usize);
            Some(out)
        } else {
            let mut v = self.chain(e.start, (e.size as usize).div_ceil(self.sector))?;
            v.truncate(e.size as usize);
            Some(v)
        }
    }
}

/// The clipboard-format property (id 17, the thumbnail) of a summary-information stream: (format id, data).
fn thumbnail_property(si: &[u8]) -> Option<(i32, &[u8])> {
    let sets = u32le(si, 24)?;
    if sets == 0 {
        return None;
    }
    let off = u32le(si, 44)? as usize;
    let count = u32le(si, off + 4)? as usize;
    for i in 0..count.min(256) {
        let pid = u32le(si, off + 8 + i * 8)?;
        let poff = off + u32le(si, off + 12 + i * 8)? as usize;
        if pid != 17 || u32le(si, poff)? != 71 {
            continue;
        }
        // VT_CF: byte count, then the clipboard format (-1 = a Windows format whose id follows), then the data.
        let n = u32le(si, poff + 4)? as usize;
        let fmt = u32le(si, poff + 8)? as i32;
        let body = si.get(poff + 12..(poff + 8 + n).min(si.len()))?;
        return if fmt == -1 { Some((u32le(body, 0)? as i32, body.get(4..)?)) } else { Some((fmt, body)) };
    }
    None
}

/// The DIB drawn by the first stretch-blit record of a Windows metafile.
fn dib_in_wmf(w: &[u8]) -> Option<&[u8]> {
    let mut pos = u16le(w, 2)? as usize * 2; // header size in words
    while pos + 6 <= w.len() {
        let size = u32le(w, pos)? as usize * 2;
        let func = u16le(w, pos + 4)?;
        if size < 6 {
            return None;
        }
        let body = w.get(pos + 6..(pos + size).min(w.len()))?;
        match func {
            0x0F43 => return body.get(22..), // StretchDIBits: rop, usage, 8 coordinates
            0x0B41 => return body.get(20..), // DibStretchBlt: rop, 8 coordinates
            0x0000 => return None,
            _ => {}
        }
        pos += size;
    }
    None
}

/// The preview stored in an OLE file's summary information (a 3ds Max scene keeps one, a few dozen pixels wide).
pub fn summary_preview(file: &[u8]) -> Option<Image> {
    let ole = Ole::open(file)?;
    let si = ole.stream("\u{5}SummaryInformation")?;
    let (fmt, data) = thumbnail_property(&si)?;
    let dib: &[u8] = match fmt {
        8 => data,
        3 => {
            // METAFILEPICT, then the metafile itself.
            let at = data.windows(6).position(|w| w == [0x01, 0x00, 0x09, 0x00, 0x00, 0x03])?;
            dib_in_wmf(&data[at..])?
        }
        _ => return None,
    };
    let mut img = crate::icon::decode_dib(dib, false, None).ok()?;
    img.profile_assumable = true;
    // A few dozen pixels wide: enlarged smoothly so it can be looked at.
    let long = img.width.max(img.height);
    if long < 400 {
        img = crate::gpufit::upscale_smooth(&img, (1200 / long.max(1)).clamp(2, 10));
    }
    Some(img)
}
