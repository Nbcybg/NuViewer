//! DICOM (`.dcm`): medical images. A small reader for the first frame, covering what files in the wild mostly hold:
//! uncompressed or RLE pixel data in implicit / explicit little-endian syntaxes (and the deflated one), greyscale
//! (MONOCHROME1 / 2, with rescale and window centre / width), palette colour, RGB and YBR, and JPEG baseline or JPEG 2000
//! pixel data. JPEG lossless / JPEG-LS and big-endian files are refused with a clear message.
//!
//! The data set is a list of elements: group u16, element u16, then (explicit VR) a two-letter VR and a 16-bit length,
//! or a 32-bit length after two reserved bytes for the long VRs, or (implicit VR) just a 32-bit length. A length of
//! 0xFFFFFFFF means "until the delimiter", used by sequences and by compressed pixel data.

use crate::Image;
use std::io::Read;

const UNDEFINED: u32 = 0xFFFF_FFFF;

#[derive(Default)]
struct Attrs {
    samples: usize,
    photometric: String,
    planar: usize,
    rows: usize,
    cols: usize,
    bits_alloc: usize,
    bits_stored: usize,
    signed: bool,
    center: Option<f32>,
    width: Option<f32>,
    intercept: f32,
    slope: Option<f32>,
    /// Palette descriptor (entries, first value, bits) and the three LUTs, once seen.
    palette_desc: Option<(usize, i32, usize)>,
    palette: [Vec<u16>; 3],
    /// Native pixel bytes, or the fragments of encapsulated pixel data.
    pixels: Option<Vec<u8>>,
    fragments: Vec<Vec<u8>>,
    encapsulated: bool,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    explicit: bool,
}

fn eof() -> String {
    "dicom: unexpected end of file".to_string()
}

impl<'a> Reader<'a> {
    fn u16(&mut self) -> Result<u16, String> {
        let v = self.b.get(self.pos..self.pos + 2).ok_or_else(eof)?;
        self.pos += 2;
        Ok(u16::from_le_bytes([v[0], v[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let v = self.b.get(self.pos..self.pos + 4).ok_or_else(eof)?;
        self.pos += 4;
        Ok(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).ok_or_else(eof)?;
        let v = self.b.get(self.pos..end).ok_or_else(eof)?;
        self.pos = end;
        Ok(v)
    }

    /// The next element's header: (group, element, VR, length).
    fn header(&mut self, force_explicit: bool) -> Result<(u16, u16, [u8; 2], u32), String> {
        let (g, e) = (self.u16()?, self.u16()?);
        // Item and delimiter tags never carry a VR.
        if g == 0xFFFE {
            return Ok((g, e, [0, 0], self.u32()?));
        }
        if self.explicit || force_explicit {
            let vr = self.take(2)?;
            let vr = [vr[0], vr[1]];
            let long = matches!(&vr, b"OB" | b"OW" | b"OF" | b"OD" | b"OL" | b"OV" | b"SQ" | b"UN" | b"UT" | b"UC" | b"UR" | b"SV" | b"UV");
            let len = if long {
                self.take(2)?;
                self.u32()?
            } else {
                self.u16()? as u32
            };
            Ok((g, e, vr, len))
        } else {
            let len = self.u32()?;
            Ok((g, e, if g == 0x7FE0 { *b"OW" } else { [0, 0] }, len))
        }
    }

    /// Skip a sequence of undefined length: items until the sequence delimiter (nesting handled).
    fn skip_sequence(&mut self) -> Result<(), String> {
        loop {
            let (g, e, _, len) = self.header(false)?;
            match (g, e) {
                (0xFFFE, 0xE0DD) => return Ok(()),
                (0xFFFE, 0xE000) => {
                    if len == UNDEFINED {
                        self.skip_item()?;
                    } else {
                        self.take(len as usize)?;
                    }
                }
                _ => return Err("dicom: damaged sequence".into()),
            }
        }
    }

    /// Skip the elements of an item of undefined length, up to the item delimiter.
    fn skip_item(&mut self) -> Result<(), String> {
        loop {
            let (g, e, _, len) = self.header(false)?;
            if (g, e) == (0xFFFE, 0xE00D) {
                return Ok(());
            }
            if len == UNDEFINED {
                self.skip_sequence()?;
            } else {
                self.take(len as usize)?;
            }
        }
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string()
}

/// First number of a DS / IS value ("40\\80" -> 40).
fn number(b: &[u8]) -> Option<f32> {
    text(b).split('\\').next().and_then(|s| s.trim().parse().ok())
}

fn us(b: &[u8], i: usize) -> Option<u16> {
    b.get(i * 2..i * 2 + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

pub fn is_dicom(b: &[u8]) -> bool {
    b.get(128..132) == Some(b"DICM")
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let (start, has_preamble) = if is_dicom(bytes) { (132, true) } else { (0, false) };
    let mut syntax = String::new();
    // File meta group (0002): always explicit little endian.
    let mut r = Reader { b: bytes, pos: start, explicit: true };
    if has_preamble {
        loop {
            let save = r.pos;
            let Ok((g, e, _, len)) = r.header(true) else { break };
            if g != 0x0002 {
                r.pos = save;
                break;
            }
            let v = r.take(len as usize)?;
            if e == 0x0010 {
                syntax = text(v);
            }
        }
    }
    let syntax = if syntax.is_empty() { "1.2.840.10008.1.2".to_string() } else { syntax };
    if syntax == "1.2.840.10008.1.2.2" {
        return Err("dicom: big-endian transfer syntax is not supported".into());
    }
    let inflated;
    let (body, explicit) = match syntax.as_str() {
        "1.2.840.10008.1.2" => (&bytes[r.pos..], false),
        "1.2.840.10008.1.2.1.99" => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(&bytes[r.pos..]).take(2_000_000_000).read_to_end(&mut out).map_err(|e| format!("dicom: the deflated data set is damaged ({e})"))?;
            inflated = out;
            (&inflated[..], true)
        }
        _ => (&bytes[r.pos..], true),
    };
    let mut r = Reader { b: body, pos: 0, explicit };
    let mut a = Attrs { samples: 1, bits_alloc: 16, bits_stored: 16, intercept: 0.0, ..Default::default() };
    if !has_preamble {
        // No preamble: the first elements tell whether this is DICOM at all (a file starts with group 0008 or 0002).
        let (g, _) = (u16::from_le_bytes([*bytes.first().unwrap_or(&0xFF), *bytes.get(1).unwrap_or(&0xFF)]), 0);
        if g != 0x0008 && g != 0x0002 {
            return Err("dicom: not a DICOM file (bad header)".into());
        }
    }
    read_dataset(&mut r, &mut a)?;
    build(&a, &syntax)
}

fn read_dataset(r: &mut Reader, a: &mut Attrs) -> Result<(), String> {
    while r.pos < r.b.len() {
        let (g, e, vr, len) = match r.header(false) {
            Ok(h) => h,
            Err(_) => break,
        };
        if (g, e) == (0x7FE0, 0x0010) {
            if len == UNDEFINED {
                // Encapsulated: a basic offset table item, then fragment items, then the sequence delimiter.
                a.encapsulated = true;
                let mut first = true;
                loop {
                    let (ig, ie, _, ilen) = r.header(false)?;
                    match (ig, ie) {
                        (0xFFFE, 0xE000) => {
                            let data = r.take(ilen as usize)?;
                            if !std::mem::take(&mut first) {
                                a.fragments.push(data.to_vec());
                            }
                        }
                        _ => break,
                    }
                }
            } else {
                a.pixels = Some(r.take(len as usize)?.to_vec());
            }
            return Ok(());
        }
        if len == UNDEFINED {
            r.skip_sequence()?;
            continue;
        }
        if &vr == b"SQ" {
            r.take(len as usize)?;
            continue;
        }
        let v = r.take(len as usize)?;
        match (g, e) {
            (0x0028, 0x0002) => a.samples = us(v, 0).unwrap_or(1) as usize,
            (0x0028, 0x0004) => a.photometric = text(v),
            (0x0028, 0x0006) => a.planar = us(v, 0).unwrap_or(0) as usize,
            (0x0028, 0x0010) => a.rows = us(v, 0).unwrap_or(0) as usize,
            (0x0028, 0x0011) => a.cols = us(v, 0).unwrap_or(0) as usize,
            (0x0028, 0x0100) => a.bits_alloc = us(v, 0).unwrap_or(16) as usize,
            (0x0028, 0x0101) => a.bits_stored = us(v, 0).unwrap_or(16) as usize,
            (0x0028, 0x0103) => a.signed = us(v, 0) == Some(1),
            (0x0028, 0x1050) => a.center = number(v),
            (0x0028, 0x1051) => a.width = number(v),
            (0x0028, 0x1052) => a.intercept = number(v).unwrap_or(0.0),
            (0x0028, 0x1053) => a.slope = number(v),
            (0x0028, 0x1101) => a.palette_desc = Some((us(v, 0).unwrap_or(0) as usize, us(v, 1).unwrap_or(0) as i32, us(v, 2).unwrap_or(8) as usize)),
            (0x0028, 0x1201) => a.palette[0] = (0..v.len() / 2).filter_map(|i| us(v, i)).collect(),
            (0x0028, 0x1202) => a.palette[1] = (0..v.len() / 2).filter_map(|i| us(v, i)).collect(),
            (0x0028, 0x1203) => a.palette[2] = (0..v.len() / 2).filter_map(|i| us(v, i)).collect(),
            _ => {}
        }
    }
    Ok(())
}

fn build(a: &Attrs, syntax: &str) -> Result<Image, String> {
    if a.rows == 0 || a.cols == 0 {
        return Err("dicom: the file has no picture (no rows / columns)".into());
    }
    crate::check_size(a.cols as u64, a.rows as u64)?;
    crate::progress::decoding();
    let mono1 = a.photometric == "MONOCHROME1";
    let mut img = if a.encapsulated {
        let data: Vec<u8> = a.fragments.concat();
        match syntax {
            "1.2.840.10008.1.2.4.50" | "1.2.840.10008.1.2.4.51" => crate::jpeg::decode(&data, None)?,
            "1.2.840.10008.1.2.4.90" | "1.2.840.10008.1.2.4.91" => crate::jp2::decode(&data)?,
            "1.2.840.10008.1.2.5" => rle(a, &data)?,
            s => return Err(format!("dicom: unsupported transfer syntax {s} (JPEG lossless, JPEG-LS and others are not supported)")),
        }
    } else {
        native(a)?
    };
    if mono1 && a.encapsulated {
        for p in img.rgba.chunks_exact_mut(4) {
            p[0] = 255 - p[0];
            p[1] = 255 - p[1];
            p[2] = 255 - p[2];
        }
    }
    img.profile_assumable = false;
    Ok(img)
}

/// Sample `i` of 16- or 8-bit greyscale data as a number (sign and bit mask applied).
fn sample(data: &[u8], i: usize, a: &Attrs) -> f32 {
    let raw: i32 = if a.bits_alloc == 16 {
        let v = u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]) as i32;
        let bits = a.bits_stored.clamp(1, 16);
        let v = v & ((1i32 << bits) - 1);
        if a.signed && v & (1 << (bits - 1)) != 0 { v - (1 << bits) } else { v }
    } else {
        let v = data[i] as i32;
        if a.signed { v as i8 as i32 } else { v }
    };
    raw as f32
}

fn native(a: &Attrs) -> Result<Image, String> {
    let data = a.pixels.as_deref().ok_or("dicom: the file has no pixel data")?;
    let (w, h) = (a.cols, a.rows);
    let px = w * h;
    let per = a.bits_alloc / 8;
    if !matches!(a.bits_alloc, 8 | 16) {
        return Err(format!("dicom: unsupported bit depth {} (8 and 16 are supported)", a.bits_alloc));
    }
    let need = px * a.samples * per;
    if data.len() < need {
        return Err(format!("dicom: pixel data is {} bytes, {need} expected (cut short?)", data.len()));
    }
    let mut rgba = vec![255u8; px * 4];
    let photometric = a.photometric.as_str();
    if a.samples == 1 && photometric == "PALETTE COLOR" {
        let (n, first, bits) = a.palette_desc.ok_or("dicom: palette colour without a palette")?;
        let n = if n == 0 { 65536 } else { n };
        let unsigned = Attrs { signed: false, ..shallow(a) };
        for i in 0..px {
            let idx = sample(data, i, &unsigned) as i32 - first;
            let idx = idx.clamp(0, n as i32 - 1) as usize;
            for c in 0..3 {
                let v = a.palette[c].get(idx).copied().unwrap_or(0);
                rgba[i * 4 + c] = if bits == 16 { (v >> 8) as u8 } else { v as u8 };
            }
        }
    } else if a.samples == 1 {
        let slope = a.slope.unwrap_or(1.0);
        let values: Vec<f32> = (0..px).map(|i| sample(data, i, a) * slope + a.intercept).collect();
        let (lo, hi) = match (a.center, a.width) {
            (Some(c), Some(wd)) if wd >= 1.0 => (c - 0.5 - (wd - 1.0) / 2.0, c - 0.5 + (wd - 1.0) / 2.0),
            _ => {
                let lo = values.iter().cloned().fold(f32::INFINITY, f32::min);
                let hi = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                (lo, hi)
            }
        };
        let span = (hi - lo).max(1e-6);
        let invert = photometric == "MONOCHROME1";
        for (i, v) in values.iter().enumerate() {
            let mut g = (((v - lo) / span).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            if invert {
                g = 255 - g;
            }
            rgba[i * 4..i * 4 + 3].fill(g);
        }
    } else if a.samples == 3 {
        if a.bits_alloc != 8 {
            return Err("dicom: 16-bit colour pictures are not supported".into());
        }
        let ybr = photometric.starts_with("YBR");
        if photometric == "YBR_FULL_422" || photometric == "YBR_PARTIAL_420" {
            return Err(format!("dicom: {photometric} data is only supported inside JPEG"));
        }
        for i in 0..px {
            let (c0, c1, c2) = if a.planar == 1 { (data[i], data[px + i], data[2 * px + i]) } else { (data[i * 3], data[i * 3 + 1], data[i * 3 + 2]) };
            let rgb = if ybr {
                let (y, cb, cr) = (c0 as f32, c1 as f32 - 128.0, c2 as f32 - 128.0);
                [y + 1.402 * cr, y - 0.344_136 * cb - 0.714_136 * cr, y + 1.772 * cb]
            } else {
                [c0 as f32, c1 as f32, c2 as f32]
            };
            for c in 0..3 {
                rgba[i * 4 + c] = rgb[c].clamp(0.0, 255.0) as u8;
            }
        }
    } else {
        return Err(format!("dicom: {} samples per pixel is not supported", a.samples));
    }
    Ok(Image::new(w as u32, h as u32, rgba))
}

/// A copy of the plain numbers of `a` (without the pixel buffers) for `sample` on palette indices.
fn shallow(a: &Attrs) -> Attrs {
    Attrs { bits_alloc: a.bits_alloc, bits_stored: a.bits_stored, ..Default::default() }
}

/// RLE lossless: 64-byte header (segment count, 15 offsets), each segment is one byte plane in PackBits.
fn rle(a: &Attrs, data: &[u8]) -> Result<Image, String> {
    let n = u32::from_le_bytes(data.get(0..4).ok_or_else(eof)?.try_into().unwrap()) as usize;
    let (w, h) = (a.cols, a.rows);
    let px = w * h;
    if n == 0 || n > 15 {
        return Err("dicom: damaged RLE header".into());
    }
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for s in 0..n {
        let off = u32::from_le_bytes(data.get(4 + s * 4..8 + s * 4).ok_or_else(eof)?.try_into().unwrap()) as usize;
        let src = data.get(off..).ok_or_else(eof)?;
        let mut out = Vec::with_capacity(px);
        let mut i = 0;
        while out.len() < px && i < src.len() {
            let c = src[i] as i8;
            i += 1;
            if c >= 0 {
                let run = c as usize + 1;
                out.extend_from_slice(src.get(i..i + run).ok_or_else(eof)?);
                i += run;
            } else if c != -128 {
                let v = *src.get(i).ok_or_else(eof)?;
                i += 1;
                out.resize(out.len() + (1 - c as isize) as usize, v);
            }
        }
        out.truncate(px);
        if out.len() < px {
            return Err("dicom: RLE segment is cut short".into());
        }
        planes.push(out);
    }
    // Re-interleave into native pixel bytes and reuse the ordinary path.
    let per = a.bits_alloc / 8;
    let mut native_bytes = vec![0u8; px * a.samples * per];
    if a.samples == 1 && per == 2 && planes.len() >= 2 {
        for i in 0..px {
            native_bytes[i * 2] = planes[1][i]; // least significant byte
            native_bytes[i * 2 + 1] = planes[0][i];
        }
    } else if per == 1 && planes.len() >= a.samples {
        for i in 0..px {
            for s in 0..a.samples {
                native_bytes[i * a.samples + s] = planes[s][i];
            }
        }
    } else {
        return Err("dicom: unsupported RLE layout".into());
    }
    let b = Attrs { pixels: Some(native_bytes), planar: 0, photometric: a.photometric.clone(), palette_desc: a.palette_desc, palette: a.palette.clone(), samples: a.samples, ..shallow_all(a) };
    native(&b)
}

fn shallow_all(a: &Attrs) -> Attrs {
    Attrs { rows: a.rows, cols: a.cols, bits_alloc: a.bits_alloc, bits_stored: a.bits_stored, signed: a.signed, center: a.center, width: a.width, intercept: a.intercept, slope: a.slope, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element_explicit(g: u16, e: u16, vr: &[u8; 2], value: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend(g.to_le_bytes());
        v.extend(e.to_le_bytes());
        v.extend(vr);
        if matches!(vr, b"OB" | b"OW" | b"SQ" | b"UN" | b"UT") {
            v.extend([0, 0]);
            v.extend((value.len() as u32).to_le_bytes());
        } else {
            v.extend((value.len() as u16).to_le_bytes());
        }
        v.extend(value);
        v
    }

    /// A 2x2 16-bit greyscale explicit-VR little-endian file with the given pixels and window.
    fn file(pixels: [u16; 4], photometric: &str, window: Option<(&str, &str)>) -> Vec<u8> {
        let mut v = vec![0u8; 128];
        v.extend(b"DICM");
        v.extend(element_explicit(0x0002, 0x0010, b"UI", b"1.2.840.10008.1.2.1\0"));
        v.extend(element_explicit(0x0028, 0x0002, b"US", &1u16.to_le_bytes()));
        let mut pm = photometric.as_bytes().to_vec();
        if pm.len() % 2 == 1 {
            pm.push(b' ');
        }
        v.extend(element_explicit(0x0028, 0x0004, b"CS", &pm));
        v.extend(element_explicit(0x0028, 0x0010, b"US", &2u16.to_le_bytes()));
        v.extend(element_explicit(0x0028, 0x0011, b"US", &2u16.to_le_bytes()));
        v.extend(element_explicit(0x0028, 0x0100, b"US", &16u16.to_le_bytes()));
        v.extend(element_explicit(0x0028, 0x0101, b"US", &12u16.to_le_bytes()));
        v.extend(element_explicit(0x0028, 0x0103, b"US", &0u16.to_le_bytes()));
        if let Some((c, w)) = window {
            v.extend(element_explicit(0x0028, 0x1050, b"DS", c.as_bytes()));
            v.extend(element_explicit(0x0028, 0x1051, b"DS", w.as_bytes()));
        }
        let px: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        v.extend(element_explicit(0x7FE0, 0x0010, b"OW", &px));
        v
    }

    #[test]
    fn greyscale_is_windowed() {
        // Window centre 100, width 100 (the text of a DS value is padded to even length): 50 -> black, 150 -> white.
        let f = file([50, 100, 150, 200], "MONOCHROME2", Some(("100", "100")));
        assert!(is_dicom(&f));
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        let g: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(g[0], 0);
        assert!(g[1] > 100 && g[1] < 155, "{}", g[1]);
        assert_eq!(g[2], 255);
        assert_eq!(g[3], 255, "above the window is white");
    }

    #[test]
    fn monochrome1_is_inverted_and_no_window_uses_the_range() {
        let f = file([0, 1000, 2000, 4000], "MONOCHROME1", None);
        let img = decode(&f).unwrap();
        let g: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(g[0], 255, "the smallest value is white in MONOCHROME1");
        assert_eq!(g[3], 0);
    }

    #[test]
    fn implicit_vr_without_a_preamble() {
        // Implicit little endian: group, element, 32-bit length.
        let imp = |g: u16, e: u16, value: &[u8]| {
            let mut v = Vec::new();
            v.extend(g.to_le_bytes());
            v.extend(e.to_le_bytes());
            v.extend((value.len() as u32).to_le_bytes());
            v.extend(value);
            v
        };
        let mut f = imp(0x0008, 0x0060, b"CT");
        f.extend(imp(0x0028, 0x0002, &1u16.to_le_bytes()));
        f.extend(imp(0x0028, 0x0004, b"MONOCHROME2"));
        f.extend(imp(0x0028, 0x0010, &1u16.to_le_bytes()));
        f.extend(imp(0x0028, 0x0011, &2u16.to_le_bytes()));
        f.extend(imp(0x0028, 0x0100, &8u16.to_le_bytes()));
        f.extend(imp(0x0028, 0x0101, &8u16.to_le_bytes()));
        f.extend(imp(0x7FE0, 0x0010, &[0, 255]));
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!((img.rgba[0], img.rgba[4]), (0, 255));
    }

    #[test]
    fn unsupported_and_damaged_files_are_errors() {
        assert!(decode(b"definitely not dicom").is_err());
        assert!(decode(&[]).is_err());
        let mut f = file([1, 2, 3, 4], "MONOCHROME2", None);
        f.truncate(f.len() - 4);
        assert!(decode(&f).is_err(), "pixel data cut short");
        let mut be = vec![0u8; 128];
        be.extend(b"DICM");
        be.extend(element_explicit(0x0002, 0x0010, b"UI", b"1.2.840.10008.1.2.2\0"));
        assert!(decode(&be).err().unwrap_or_default().contains("big-endian"));
    }
}
