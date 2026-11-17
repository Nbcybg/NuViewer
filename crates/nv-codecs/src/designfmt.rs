//! Painting, drawing and design programs' own files:
//!  * Krita (`kra`) and OpenRaster (`ora`): zip files with the merged picture inside,
//!  * GIMP (`xcf`): the layers are read and composited (normal, multiply, screen, overlay ... modes, opacity, masks, groups' visibility),
//!  * Paint Shop Pro (`psp` `pspimage`): the JPEG composite stored in the file, else any JPEG / PNG found inside,
//!  * CorelDRAW (`cdr` `cdt`): the thumbnail of the zip kind (X4 and later) or the DISP preview of the older RIFF kind,
//!  * Chasys Draw IES (`cd5`): a card (no published format) unless a picture is stored inside,
//!  * XPS / OpenXPS / DWFx (`xps` `oxps` `dwfx`) and DWF (`dwf`): the thumbnail or largest picture in the package, else a card,
//!  * Universal 3D (`u3d`): a card with the scene's parts (the geometry is compressed in a way that is not drawn here),
//!  * Lattice XVL (`xvl` `xv3` `xv0`): a card (closed format).

use crate::docthumb::{accent_for, fmt_bytes, render_card, Card, Zip};
use crate::Image;
use std::path::Path;

pub const EXTS: &[&str] = &["kra", "ora", "xcf", "psp", "pspimage", "cdr", "cdt", "cd5", "xps", "oxps", "dwfx", "dwf", "u3d", "xvl", "xv3", "xv0"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    if !is_ext(ext) {
        return None;
    }
    Some(match ext {
        "kra" | "ora" => zip_picture(path, ext, &["mergedimage.png", "preview.png", "thumbnails/thumbnail.png", "thumbnail.png"], first_only),
        "xcf" => crate::progress::read_file(path).and_then(|b| xcf(&b)),
        "psp" | "pspimage" => crate::progress::read_file(path).and_then(|b| psp(&b, path, first_only)),
        "cdr" | "cdt" => cdr(path, ext, first_only),
        "cd5" => crate::progress::read_file(path).and_then(|b| cd5(&b, path, first_only)),
        "xps" | "oxps" | "dwfx" => package(path, ext, first_only),
        "dwf" => dwf(path, first_only),
        "u3d" => crate::progress::read_file(path).and_then(|b| u3d(&b, path, first_only)),
        _ => lattice(path, ext, first_only),
    })
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn card(path: &Path, ext: &str, kind: &str, lines: Vec<String>, size: u64, first_only: bool, note: &str) -> Image {
    let mut all = vec![name_of(path), kind.to_string()];
    all.extend(lines);
    let all: Vec<String> = all.into_iter().map(|l| crate::fontview::clip(&l, 44)).collect();
    let mut img = render_card(&Card { badge: ext.to_ascii_uppercase(), accent: accent_for(ext), title: name_of(path), subtitle: fmt_bytes(size), lines: all }, if first_only { 1 } else { 2 });
    img.colour_note = Some(note.to_string());
    img
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

// ---- zip files with a picture in a known place ---------------------------------------------------------------------------------

fn zip_picture(path: &Path, ext: &str, preferred: &[&str], first_only: bool) -> Result<Image, String> {
    let mut zip = Zip::open(path)?;
    let entries = zip.entries.clone();
    let pick = |want: &str| entries.iter().find(|e| e.name.eq_ignore_ascii_case(want)).cloned();
    for p in preferred {
        if let Some(e) = pick(p) {
            if let Ok(bytes) = zip.read(&e, 256 << 20) {
                if let Ok(mut img) = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&e.name)) {
                    img.profile_assumable = true;
                    img.colour_note = Some(format!("{} picture: {} (the merged image stored in the file)", ext.to_uppercase(), e.name));
                    return Ok(img);
                }
            }
        }
    }
    // Otherwise the biggest picture inside.
    let mut pics: Vec<_> = entries.iter().filter(|e| crate::docthumb::is_image_name(&e.name)).cloned().collect();
    pics.sort_by(|a, b| b.size.cmp(&a.size));
    for e in pics.into_iter().take(3) {
        if let Ok(bytes) = zip.read(&e, 128 << 20) {
            if let Ok(mut img) = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&e.name)) {
                img.profile_assumable = true;
                img.colour_note = Some(format!("{} picture: {} (the biggest picture inside)", ext.to_uppercase(), e.name));
                return Ok(img);
            }
        }
    }
    Ok(card(path, ext, "Zip package, no picture inside", vec![format!("{} files", entries.len())], size_of(path), first_only, "a card: no picture in this file"))
}

// ---- GIMP XCF ------------------------------------------------------------------------------------------------------------------

struct Xr<'a> {
    b: &'a [u8],
    v: u32,
}

impl<'a> Xr<'a> {
    fn u32(&self, o: usize) -> Option<u32> {
        self.b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&self, o: usize) -> Option<i32> {
        self.u32(o).map(|v| v as i32)
    }
    fn f32(&self, o: usize) -> Option<f32> {
        self.u32(o).map(f32::from_bits)
    }
    /// A file pointer: 32 bits before version 11, 64 after.
    fn ptr(&self, o: usize) -> Option<(usize, usize)> {
        if self.v >= 11 {
            self.b.get(o..o + 8).map(|s| (u64::from_be_bytes(s.try_into().unwrap()) as usize, 8))
        } else {
            self.u32(o).map(|v| (v as usize, 4))
        }
    }
    fn string(&self, o: usize) -> Option<(String, usize)> {
        let n = self.u32(o)? as usize;
        if n == 0 {
            return Some((String::new(), 4));
        }
        let s = self.b.get(o + 4..o + 4 + n)?;
        Some((String::from_utf8_lossy(&s[..n.saturating_sub(1)]).into_owned(), 4 + n))
    }
}

#[derive(Default, Clone)]
struct Props {
    opacity: f32,
    visible: bool,
    mode: u32,
    off: (i32, i32),
    apply_mask: bool,
    compression: u8,
    group: bool,
    path: Option<Vec<u32>>,
    colormap: Vec<u8>,
}

/// The property list at `o`; returns the properties and where the list ends.
fn xcf_props(r: &Xr, mut o: usize, defaults: Props) -> Option<(Props, usize)> {
    let mut p = defaults;
    for _ in 0..1000 {
        let ty = r.u32(o)?;
        let size = r.u32(o + 4)? as usize;
        let d = o + 8;
        match ty {
            0 => return Some((p, o + 8)),
            1 => {
                let n = r.u32(d)? as usize;
                p.colormap = r.b.get(d + 4..d + 4 + n * 3)?.to_vec();
            }
            6 => p.opacity = r.u32(d)? as f32 / 255.0,
            33 => p.opacity = r.f32(d)?.clamp(0.0, 1.0),
            7 => p.mode = r.u32(d)?,
            8 => p.visible = r.u32(d)? != 0,
            10 => p.apply_mask = r.u32(d)? != 0,
            15 => p.off = (r.i32(d)?, r.i32(d + 4)?),
            17 => p.compression = *r.b.get(d)?,
            29 => p.group = true,
            30 => p.path = Some((0..size / 4).filter_map(|i| r.u32(d + i * 4)).collect()),
            _ => {}
        }
        o = d + size;
    }
    None
}

fn rle_tile(src: &[u8], planes: usize, n: usize) -> Option<Vec<u8>> {
    // Each plane (all the red bytes, then all the green ...) is a run-length coded stream.
    let mut out = vec![0u8; n * planes];
    let mut at = 0usize;
    for pl in 0..planes {
        let mut got = 0usize;
        while got < n {
            let c = *src.get(at)? as usize;
            at += 1;
            if c >= 128 {
                let mut len = 256 - c;
                if len == 128 {
                    len = ((*src.get(at)? as usize) << 8) | *src.get(at + 1)? as usize;
                    at += 2;
                }
                if got + len > n {
                    return None;
                }
                out[pl * n + got..pl * n + got + len].copy_from_slice(src.get(at..at + len)?);
                at += len;
                got += len;
            } else {
                let mut len = c + 1;
                if len == 128 {
                    len = ((*src.get(at)? as usize) << 8) | *src.get(at + 1)? as usize;
                    at += 2;
                }
                let v = *src.get(at)?;
                at += 1;
                if got + len > n {
                    return None;
                }
                out[pl * n + got..pl * n + got + len].fill(v);
                got += len;
            }
        }
    }
    // Planar to interleaved.
    let mut inter = vec![0u8; n * planes];
    for i in 0..n {
        for pl in 0..planes {
            inter[i * planes + pl] = out[pl * n + i];
        }
    }
    Some(inter)
}

fn srgb_from_linear(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// One pixel plane of a hierarchy (a layer's picture or a mask): `w * h * bpp` interleaved bytes, tiles of 64 x 64.
fn xcf_hierarchy(r: &Xr, o: usize, compression: u8) -> Result<(usize, usize, usize, Vec<u8>), String> {
    let (w, h, bpp) = (r.u32(o).ok_or("xcf: hierarchy cut off")? as usize, r.u32(o + 4).unwrap_or(0) as usize, r.u32(o + 8).unwrap_or(0) as usize);
    crate::check_size(w as u64, h as u64)?;
    if bpp == 0 || bpp > 16 {
        return Err("xcf: unusual pixel size".into());
    }
    let (level, _) = r.ptr(o + 12).ok_or("xcf: no levels")?;
    let (lw, lh) = (r.u32(level).ok_or("xcf: level cut off")? as usize, r.u32(level + 4).unwrap_or(0) as usize);
    if (lw, lh) != (w, h) {
        return Err("xcf: level size differs from the layer's".into());
    }
    let mut out = vec![0u8; w * h * bpp];
    let (tx, ty) = (w.div_ceil(64), h.div_ceil(64));
    let psz = if r.v >= 11 { 8 } else { 4 };
    for t in 0..tx * ty {
        let (tp, _) = r.ptr(level + 8 + t * psz).ok_or("xcf: tile table cut off")?;
        if tp == 0 {
            break;
        }
        let (col, row) = (t % tx, t / tx);
        let (tw, th) = ((w - col * 64).min(64), (h - row * 64).min(64));
        let n = tw * th;
        let src = r.b.get(tp..).ok_or("xcf: tile outside the file")?;
        let data = match compression {
            0 => src.get(..n * bpp).ok_or("xcf: tile cut off")?.to_vec(),
            1 => rle_tile(src, bpp, n).ok_or("xcf: damaged RLE tile")?,
            2 => {
                use std::io::Read;
                let mut v = Vec::with_capacity(n * bpp);
                flate2::read::ZlibDecoder::new(src).take((n * bpp) as u64).read_to_end(&mut v).map_err(|e| format!("xcf: zlib: {e}"))?;
                if v.len() != n * bpp {
                    return Err("xcf: tile did not unpack to its size".into());
                }
                v
            }
            c => return Err(format!("xcf: compression {c} is not supported")),
        };
        for y in 0..th {
            let d = ((row * 64 + y) * w + col * 64) * bpp;
            out[d..d + tw * bpp].copy_from_slice(&data[y * tw * bpp..(y + 1) * tw * bpp]);
        }
    }
    Ok((w, h, bpp, out))
}

/// 8-bit straight RGBA from a layer's raw samples.
fn xcf_to_rgba(raw: &[u8], bpp: usize, ltype: u32, precision: u32, colormap: &[u8]) -> Result<Vec<u8>, String> {
    let comps = match ltype {
        0 => 3,
        1 => 4,
        2 | 4 => 1,
        3 | 5 => 2,
        _ => return Err("xcf: unknown layer type".into()),
    };
    let bytes = bpp / comps;
    let linear = matches!(precision, 100 | 200 | 300 | 500 | 600 | 700);
    let n = raw.len() / bpp;
    let mut out = vec![255u8; n * 4];
    let get = |px: &[u8], c: usize| -> f32 {
        match bytes {
            1 => px[c] as f32 / 255.0,
            2 if matches!(precision, 500 | 550) => half(u16::from_be_bytes([px[c * 2], px[c * 2 + 1]])),
            2 => u16::from_be_bytes([px[c * 2], px[c * 2 + 1]]) as f32 / 65535.0,
            4 => f32::from_be_bytes([px[c * 4], px[c * 4 + 1], px[c * 4 + 2], px[c * 4 + 3]]),
            _ => 0.0,
        }
    };
    if !matches!(bytes, 1 | 2 | 4) {
        return Err("xcf: unusual sample size".into());
    }
    for i in 0..n {
        let px = &raw[i * bpp..(i + 1) * bpp];
        let o = &mut out[i * 4..i * 4 + 4];
        let conv = |v: f32| -> u8 { ((if linear && bytes > 1 || bytes == 1 && linear { srgb_from_linear(v) } else { v.clamp(0.0, 1.0) }) * 255.0 + 0.5) as u8 };
        match ltype {
            0 | 1 => {
                o[0] = conv(get(px, 0));
                o[1] = conv(get(px, 1));
                o[2] = conv(get(px, 2));
                if ltype == 1 {
                    o[3] = (get(px, 3).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            2 | 3 => {
                let g = conv(get(px, 0));
                o[..3].fill(g);
                if ltype == 3 {
                    o[3] = (get(px, 1).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            _ => {
                let idx = px[0] as usize;
                if let Some(c) = colormap.get(idx * 3..idx * 3 + 3) {
                    o[..3].copy_from_slice(c);
                }
                if ltype == 5 {
                    o[3] = px[1];
                }
            }
        }
    }
    Ok(out)
}

fn half(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) & 1, (h >> 10) & 0x1F, h & 0x3FF);
    let v = match e {
        0 => (m as f32) * 2f32.powi(-24),
        31 => f32::INFINITY,
        e => (1.0 + m as f32 / 1024.0) * 2f32.powi(e as i32 - 15),
    };
    if s == 1 {
        -v
    } else {
        v
    }
}

fn blend_channel(mode: u32, d: f32, s: f32) -> f32 {
    match mode {
        3 => d * s,
        4 => 1.0 - (1.0 - d) * (1.0 - s),
        5 | 23 => {
            // overlay (GIMP's soft-light-ish legacy 5 is close enough to the usual overlay)
            if d < 0.5 {
                2.0 * d * s
            } else {
                1.0 - 2.0 * (1.0 - d) * (1.0 - s)
            }
        }
        6 => (d - s).abs(),
        7 => (d + s).min(1.0),
        8 => (d - s).max(0.0),
        9 => d.min(s),
        10 => d.max(s),
        15 => {
            if s <= 0.0 {
                1.0
            } else {
                (d / s).min(1.0)
            }
        }
        16 => {
            if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }
        17 => {
            if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }
        18 => {
            if s < 0.5 {
                2.0 * d * s
            } else {
                1.0 - 2.0 * (1.0 - d) * (1.0 - s)
            }
        }
        19 => {
            let r = 1.0 - (1.0 - d) * (1.0 - s);
            (1.0 - d) * d * s + d * r
        }
        20 => (d - s + 0.5).clamp(0.0, 1.0),
        21 => (d + s - 0.5).clamp(0.0, 1.0),
        _ => s,
    }
}

pub fn xcf(b: &[u8]) -> Result<Image, String> {
    if !b.starts_with(b"gimp xcf ") || b.len() < 30 {
        return Err("xcf: not a GIMP XCF file".into());
    }
    let ver = &b[9..13];
    let version = if ver == b"file" { 0 } else { std::str::from_utf8(&ver[1..4]).ok().and_then(|s| s.parse::<u32>().ok()).ok_or("xcf: unreadable version")? };
    let r = Xr { b, v: version };
    let (w, h, base) = (r.u32(14).ok_or("xcf: short")? as usize, r.u32(18).ok_or("xcf: short")? as usize, r.u32(22).ok_or("xcf: short")?);
    crate::check_size(w as u64, h as u64)?;
    let (precision, mut o) = if version >= 4 { (r.u32(26).ok_or("xcf: short")?, 30) } else { (150, 26) };
    let image_defaults = Props { opacity: 1.0, visible: true, apply_mask: true, ..Default::default() };
    let (image_props, end) = xcf_props(&r, o, image_defaults.clone()).ok_or("xcf: damaged property list")?;
    o = end;
    let mut layer_ptrs = Vec::new();
    for _ in 0..4096 {
        let (p, sz) = r.ptr(o).ok_or("xcf: layer table cut off")?;
        o += sz;
        if p == 0 {
            break;
        }
        layer_ptrs.push(p);
    }
    if layer_ptrs.is_empty() {
        return Err("xcf: this image has no layers".into());
    }
    let _ = base;
    struct L {
        p: Props,
        ptr: usize,
        w: usize,
        h: usize,
        ltype: u32,
    }
    let mut layers = Vec::new();
    for &lp in &layer_ptrs {
        let (lw, lh, lt) = (r.u32(lp).ok_or("xcf: layer cut off")? as usize, r.u32(lp + 4).ok_or("xcf: layer cut off")? as usize, r.u32(lp + 8).ok_or("xcf: layer cut off")?);
        let (_, nlen) = r.string(lp + 12).ok_or("xcf: layer name cut off")?;
        let (p, end) = xcf_props(&r, lp + 12 + nlen, Props { compression: image_props.compression, colormap: image_props.colormap.clone(), ..image_defaults.clone() }).ok_or("xcf: damaged layer properties")?;
        layers.push(L { p, ptr: end, w: lw, h: lh, ltype: lt });
    }
    // Groups' visibility, by item path.
    let group_visible: Vec<(Vec<u32>, bool)> = layers.iter().filter(|l| l.p.group).filter_map(|l| l.p.path.clone().map(|p| (p, l.p.visible))).collect();
    let effective = |l: &L| -> bool {
        if !l.p.visible || l.p.group {
            return false;
        }
        if let Some(path) = &l.p.path {
            for k in 1..path.len() {
                if group_visible.iter().any(|(gp, vis)| gp.as_slice() == &path[..k] && !vis) {
                    return false;
                }
            }
        }
        true
    };
    let mut canvas = vec![0f32; w * h * 4];
    let mut used = 0usize;
    for l in layers.iter().rev().filter(|l| effective(l)) {
        if l.w == 0 || l.h == 0 {
            continue;
        }
        let (hp, _) = r.ptr(l.ptr).ok_or("xcf: layer pointers cut off")?;
        let (mp, msz) = r.ptr(l.ptr + if version >= 11 { 8 } else { 4 }).unwrap_or((0, 0));
        let _ = msz;
        let (_, _, bpp, raw) = xcf_hierarchy(&r, hp, l.p.compression)?;
        let rgba = xcf_to_rgba(&raw, bpp, l.ltype, precision, &l.p.colormap)?;
        // The mask: a grey hierarchy of the same size.
        let mask: Option<Vec<u8>> = if mp != 0 && l.p.apply_mask {
            (|| {
                let (_, nl) = r.string(mp + 8)?;
                let (_, end) = xcf_props(&r, mp + 8 + nl, Props { compression: l.p.compression, ..image_defaults.clone() })?;
                let (mh, _) = r.ptr(end)?;
                let (_, _, mb, mraw) = xcf_hierarchy(&r, mh, l.p.compression).ok()?;
                (mb == 1).then_some(mraw)
            })()
        } else {
            None
        };
        used += 1;
        for y in 0..l.h {
            let cy = y as i64 + l.p.off.1 as i64;
            if cy < 0 || cy >= h as i64 {
                continue;
            }
            for x in 0..l.w {
                let cx = x as i64 + l.p.off.0 as i64;
                if cx < 0 || cx >= w as i64 {
                    continue;
                }
                let s = &rgba[(y * l.w + x) * 4..(y * l.w + x) * 4 + 4];
                let m = mask.as_ref().map(|m| m[y * l.w + x] as f32 / 255.0).unwrap_or(1.0);
                let sa = s[3] as f32 / 255.0 * l.p.opacity * m;
                if sa <= 0.0 {
                    continue;
                }
                let d = &mut canvas[(cy as usize * w + cx as usize) * 4..(cy as usize * w + cx as usize) * 4 + 4];
                let da = d[3];
                let oa = sa + da * (1.0 - sa);
                for c in 0..3 {
                    let sc = s[c] as f32 / 255.0;
                    let bl = if da > 0.0 && !matches!(l.p.mode, 0 | 28) { blend_channel(l.p.mode, d[c], sc) } else { sc };
                    d[c] = ((1.0 - sa) * da * d[c] + sa * ((1.0 - da) * sc + da * bl)) / oa;
                }
                d[3] = oa;
            }
        }
    }
    if used == 0 {
        return Err("xcf: every layer is hidden".into());
    }
    let mut rgba = vec![0u8; w * h * 4];
    for (o, c) in rgba.chunks_exact_mut(4).zip(canvas.chunks_exact(4)) {
        for k in 0..4 {
            o[k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.profile_assumable = true;
    img.colour_note = Some(format!("GIMP XCF v{version}: {used} visible layer(s) composited (blend modes are approximated)"));
    Ok(img)
}

// ---- Paint Shop Pro ------------------------------------------------------------------------------------------------------------

/// All JPEG streams (SOI ... EOI) found in `b[from..to]`, as (start, end).
fn jpeg_spans(b: &[u8], from: usize, to: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = from;
    let to = to.min(b.len());
    while i + 4 < to {
        if b[i] == 0xFF && b[i + 1] == 0xD8 && b[i + 2] == 0xFF {
            // Walk the segments to the end of the image data, then to the EOI.
            let mut p = i + 2;
            let mut ok = false;
            while p + 4 <= to {
                if b[p] != 0xFF {
                    break;
                }
                let m = b[p + 1];
                if m == 0xDA {
                    ok = true;
                    break;
                }
                if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 {
                    p += 2;
                    continue;
                }
                p += 2 + ((b[p + 2] as usize) << 8 | b[p + 3] as usize);
            }
            if ok {
                // After the scan header: entropy-coded data until a marker that is not RST / stuffing; the EOI ends it.
                let mut q = p + 2 + ((b[p + 2] as usize) << 8 | b[p + 3] as usize);
                let mut end = None;
                while q + 1 < to {
                    if b[q] == 0xFF && b[q + 1] == 0xD9 {
                        end = Some(q + 2);
                        break;
                    }
                    q += 1;
                }
                if let Some(e) = end {
                    out.push((i, e));
                    i = e;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn psp(b: &[u8], path: &Path, first_only: bool) -> Result<Image, String> {
    const SIG: &[u8] = b"Paint Shop Pro Image File\n\x1a";
    if !b.starts_with(SIG) || b.len() < 36 {
        return Err("psp: not a Paint Shop Pro file".into());
    }
    let major = u16::from_le_bytes([b[32], b[33]]);
    let minor = u16::from_le_bytes([b[34], b[35]]);
    // Blocks: "~BK\0", id, [initial length (version 4+)], total length, data. The composite image bank is block 16.
    let (hdr, id_at) = if major >= 4 { (14usize, 4usize) } else { (10, 4) };
    let mut pos = 36usize;
    let mut bank: Option<(usize, usize)> = None;
    for _ in 0..4096 {
        if b.get(pos..pos + 4) != Some(b"~BK\0") {
            break;
        }
        let id = u16::from_le_bytes([b[pos + id_at], b[pos + id_at + 1]]);
        let total = u32::from_le_bytes(b.get(pos + hdr - 4..pos + hdr).ok_or("psp: cut off")?.try_into().unwrap()) as usize;
        if id == 16 {
            bank = Some((pos + hdr, (pos + hdr + total).min(b.len())));
            break;
        }
        pos = pos + hdr + total;
    }
    let mut best: Option<Image> = None;
    let try_range = |from: usize, to: usize, best: &mut Option<Image>| {
        for (s, e) in jpeg_spans(b, from, to) {
            if let Ok(img) = crate::decode_image_bytes(&b[s..e], "jpg") {
                if best.as_ref().is_none_or(|x| img.width as u64 * img.height as u64 > x.width as u64 * x.height as u64) {
                    *best = Some(img);
                }
            }
        }
    };
    if let Some((s, e)) = bank {
        try_range(s, e, &mut best);
    }
    if best.is_none() {
        try_range(36, b.len(), &mut best);
    }
    match best {
        Some(mut img) => {
            img.profile_assumable = true;
            img.colour_note = Some(format!("Paint Shop Pro {major}.{minor}: the JPEG composite stored in the file"));
            Ok(img)
        }
        None => Ok(card(path, "psp", &format!("Paint Shop Pro {major}.{minor} image"), vec!["No JPEG composite inside:".into(), "layers are in PSP's own format.".into(), "Export PNG / TIFF from PSP".into()], b.len() as u64, first_only, "a card: no composite picture in this PSP file")),
    }
}

// ---- CorelDRAW -----------------------------------------------------------------------------------------------------------------

fn cdr(path: &Path, ext: &str, first_only: bool) -> Result<Image, String> {
    let size = size_of(path);
    if let Ok(mut zip) = Zip::open(path) {
        // X4 and later: a zip with the thumbnail in metadata/thumbnails.
        let entries = zip.entries.clone();
        let mut cands: Vec<_> = entries.iter().filter(|e| {
            let n = e.name.to_ascii_lowercase();
            matches!(crate::docthumb::ext_of(&n).as_str(), "bmp" | "png" | "jpg" | "jpeg") && (n.contains("thumbnail") || n.contains("preview"))
        }).cloned().collect();
        cands.sort_by(|a, b| b.size.cmp(&a.size));
        for e in cands {
            if let Ok(bytes) = zip.read(&e, 64 << 20) {
                if let Ok(mut img) = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&e.name)) {
                    img.colour_note = Some(format!("CorelDRAW thumbnail ({}), the drawing itself is in CorelDRAW's own format", e.name));
                    img.profile_assumable = true;
                    return Ok(img);
                }
            }
        }
        return Ok(card(path, ext, "CorelDRAW drawing (X4 or later)", vec![format!("{} files in the package,", entries.len()), "no thumbnail saved inside".into()], size, first_only, "a card: no thumbnail in this CorelDRAW file"));
    }
    // Older files: RIFF "CDR?" with a DISP chunk holding a bitmap (a DIB without its file header).
    let bytes = crate::progress::read_file(path)?;
    if bytes.starts_with(b"RIFF") && bytes.get(8..11) == Some(b"CDR") {
        let mut from = 12;
        while let Some(at) = bytes.get(from..).and_then(|s| s.windows(4).position(|w| w == b"DISP")) {
            let pos = from + at;
            let len = u32::from_le_bytes(bytes.get(pos + 4..pos + 8).ok_or("cdr: cut off")?.try_into().unwrap()) as usize;
            if let Some(chunk) = bytes.get(pos + 8..pos + 8 + len) {
                for skip in [0usize, 4] {
                    if let Some(d) = chunk.get(skip..) {
                        if let Ok(mut img) = crate::icon::decode_dib(d, false, None) {
                            img.colour_note = Some("CorelDRAW preview (DISP) stored in the file".into());
                            img.profile_assumable = true;
                            return Ok(img);
                        }
                    }
                }
            }
            from = pos + 4;
        }
        return Ok(card(path, ext, &format!("CorelDRAW drawing (RIFF {})", String::from_utf8_lossy(&bytes[8..12])), vec!["No preview picture inside".into()], size, first_only, "a card: no preview in this CorelDRAW file"));
    }
    Err("cdr: not a CorelDRAW file".into())
}

// ---- Chasys Draw IES and Lattice XVL: closed formats ---------------------------------------------------------------------------

/// A PNG or JPEG stored anywhere in the first 8 MB, else None.
fn stored_picture(b: &[u8]) -> Option<Image> {
    let head = &b[..b.len().min(8 << 20)];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mut from = 0;
    while let Some(p) = head.get(from..).and_then(|s| s.windows(8).position(|w| w == PNG)) {
        let start = from + p;
        let mut at = start + 8;
        while at + 12 <= head.len() {
            let len = u32::from_be_bytes(head[at..at + 4].try_into().ok()?) as usize;
            let kind = &head[at + 4..at + 8];
            at = at.checked_add(12 + len)?;
            if kind == b"IEND" {
                if let Ok(img) = crate::decode_image_bytes(head.get(start..at)?, "png") {
                    return Some(img);
                }
                break;
            }
        }
        from = start + 8;
    }
    for (s, e) in jpeg_spans(head, 0, head.len()) {
        if e - s > 4000 {
            if let Ok(img) = crate::decode_image_bytes(&head[s..e], "jpg") {
                return Some(img);
            }
        }
    }
    None
}

fn cd5(b: &[u8], path: &Path, first_only: bool) -> Result<Image, String> {
    if let Some(mut img) = stored_picture(b) {
        img.colour_note = Some("a picture stored inside this Chasys Draw IES file".into());
        img.profile_assumable = true;
        return Ok(img);
    }
    Ok(card(path, "cd5", "Chasys Draw IES image (CD5)", vec!["Layered, tag-based format with no".into(), "published specification and no".into(), "preview inside.".into(), "Export PNG from Chasys Draw IES".into()], b.len() as u64, first_only, "a card: CD5 is a closed format"))
}

fn lattice(path: &Path, ext: &str, first_only: bool) -> Result<Image, String> {
    let b = crate::progress::read_file(path)?;
    if let Some(mut img) = stored_picture(&b) {
        img.colour_note = Some("a picture stored inside this Lattice XVL file".into());
        img.profile_assumable = true;
        return Ok(img);
    }
    Ok(card(path, ext, "Lattice3D XVL model", vec!["A closed 3D format (Lattice".into(), "Technology): no preview inside.".into(), "Export glTF, OBJ or STL from".into(), "XVL Studio to look at the model".into()], b.len() as u64, first_only, "a card: XVL is a closed format"))
}

// ---- XPS, OpenXPS, DWFx, DWF ---------------------------------------------------------------------------------------------------

fn package(path: &Path, ext: &str, first_only: bool) -> Result<Image, String> {
    let size = size_of(path);
    let mut zip = Zip::open(path)?;
    let entries = zip.entries.clone();
    let pages = entries.iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".fpage")).count();
    // The thumbnail the producer wrote, else the biggest picture used by the document.
    let mut cands: Vec<_> = entries.iter().filter(|e| crate::docthumb::is_image_name(&e.name) && e.name.to_ascii_lowercase().contains("thumbnail")).cloned().collect();
    cands.sort_by(|a, b| b.size.cmp(&a.size));
    let mut rest: Vec<_> = entries.iter().filter(|e| crate::docthumb::is_image_name(&e.name) && !e.name.to_ascii_lowercase().contains("thumbnail")).cloned().collect();
    rest.sort_by(|a, b| b.size.cmp(&a.size));
    for (i, e) in cands.into_iter().chain(rest.into_iter().take(2)).enumerate() {
        if let Ok(bytes) = zip.read(&e, 128 << 20) {
            if let Ok(mut img) = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&e.name)) {
                img.profile_assumable = true;
                img.colour_note = Some(format!("{} {}: {}", ext.to_uppercase(), if i == 0 { "thumbnail" } else { "picture used in the document" }, e.name));
                return Ok(img);
            }
        }
    }
    #[cfg(windows)]
    if ext != "dwfx" || pages > 0 {
        if let Ok(mut img) = crate::xpsdraw::render_first_page(path, first_only) {
            img.colour_note = Some(format!("{}: first page drawn by Windows' XPS renderer ({} page(s))", ext.to_uppercase(), pages.max(1)));
            return Ok(img);
        }
    }
    Ok(card(path, ext, "XPS document package", vec![format!("{pages} page(s), {} parts", entries.len()), "No thumbnail inside".into()], size, first_only, "a card: no thumbnail in this package"))
}

fn dwf(path: &Path, first_only: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    let size = bytes.len() as u64;
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(16)]).into_owned();
    let version = head.trim_start_matches('(').split(')').next().unwrap_or("").to_string();
    // DWF 6 and later: a zip (possibly after the "(DWF V06.00)" tag). Cut the tag off into a temporary file so the zip reader's offsets hold.
    if let Some(at) = bytes.get(..bytes.len().min(64)).and_then(|s| s.windows(4).position(|w| w == b"PK\x03\x04")) {
        let tmp = std::env::temp_dir().join(format!("nv_dwf_{}_{}.zip", std::process::id(), bytes.len()));
        if std::fs::write(&tmp, &bytes[at..]).is_ok() {
            let r = package(&tmp, "dwf", first_only);
            let _ = std::fs::remove_file(&tmp);
            if let Ok(mut img) = r {
                if img.colour_note.as_deref().is_some_and(|n| n.starts_with("DWF")) {
                    return Ok(img);
                }
                // A card made for the temporary name: draw one with the real name.
                img.colour_note = None;
            }
        }
    }
    Ok(card(path, "dwf", &format!("Autodesk Design Web Format {version}"), vec!["The drawing is stored as compressed".into(), "2D / 3D streams (W2D / W3D); no".into(), "preview picture inside".into()], size, first_only, "a card: no preview picture in this DWF"))
}

// ---- Universal 3D --------------------------------------------------------------------------------------------------------------

fn u3d(b: &[u8], path: &Path, first_only: bool) -> Result<Image, String> {
    let le32 = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if le32(0) != Some(0x0044_3355) {
        return Err("u3d: not a Universal 3D file (no U3D header block)".into());
    }
    let mut pos = 0usize;
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    let mut meshes: Vec<String> = Vec::new();
    let mut nodes: Vec<String> = Vec::new();
    let mut version = String::new();
    let (mut faces, mut verts) = (0u64, 0u64);
    for _ in 0..200_000 {
        let (Some(ty), Some(dsize), Some(msize)) = (le32(pos), le32(pos + 4), le32(pos + 8)) else { break };
        let (dsize, msize) = (dsize as usize, msize as usize);
        let data = pos + 12;
        let Some(d) = b.get(data..data + dsize) else { break };
        let kind = match ty {
            0x0044_3355 => {
                version = format!("{}.{}", u16::from_le_bytes([d[0], d[1]]), u16::from_le_bytes([d[2], d[3]]));
                "file header"
            }
            0xFFFF_FF21 => "group node",
            0xFFFF_FF22 => "model node",
            0xFFFF_FF23 => "light node",
            0xFFFF_FF24 => "view node",
            0xFFFF_FF31 => "mesh",
            0xFFFF_FF36 => "point set",
            0xFFFF_FF37 => "line set",
            0xFFFF_FF3B | 0xFFFF_FF3C => "mesh data",
            0xFFFF_FF45 => "light resource",
            0xFFFF_FF42 => "view resource",
            0xFFFF_FF54 => "shader",
            0xFFFF_FF53 => "material",
            0xFFFF_FF55 => "texture",
            0xFFFF_FF56 => "motion",
            0xFFFF_FF14 => "modifier chain",
            _ => "other",
        };
        *counts.entry(kind).or_insert(0) += 1;
        let name = || -> Option<String> {
            let n = u16::from_le_bytes([*d.first()?, *d.get(1)?]) as usize;
            Some(String::from_utf8_lossy(d.get(2..2 + n)?).into_owned())
        };
        match ty {
            0xFFFF_FF21..=0xFFFF_FF24 => nodes.extend(name()),
            0xFFFF_FF31 => {
                if let Some(n) = name() {
                    // After the name: chain index, then the maximum mesh description (attributes, faces, positions ...).
                    let at = 2 + n.len() + 4;
                    let get = |o: usize| d.get(at + o..at + o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64);
                    let (f, v) = (get(4).unwrap_or(0), get(8).unwrap_or(0));
                    faces += f;
                    verts += v;
                    meshes.push(format!("{n} ({f} faces)"));
                }
            }
            _ => {}
        }
        let pad = |n: usize| n.div_ceil(4) * 4;
        pos = data + pad(dsize) + pad(msize);
    }
    let mut lines = vec![format!("Universal 3D, version {version}")];
    if !meshes.is_empty() {
        lines.push(format!("{} meshes, {faces} faces, {verts} vertices", meshes.len()));
        lines.extend(meshes.iter().take(4).cloned());
    }
    if !nodes.is_empty() {
        lines.push(format!("Nodes: {}", nodes.iter().take(5).cloned().collect::<Vec<_>>().join(", ")));
    }
    let summary: Vec<String> = counts.iter().filter(|(k, _)| **k != "other" && **k != "file header").map(|(k, n)| format!("{n} {k}")).collect();
    if !summary.is_empty() {
        lines.push(summary.join(", "));
    }
    lines.push("The geometry is stored compressed;".into());
    lines.push("it is not drawn here".into());
    Ok(card(path, "u3d", "Universal 3D (ECMA-363)", lines, b.len() as u64, first_only, "a card: the parts of this U3D scene (its compressed geometry is not drawn)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    /// A small XCF (version 3: 32-bit pointers, RLE or raw tiles): one background layer and one red half-transparent layer on top.
    fn make_xcf(rle: bool) -> Vec<u8> {
        let mut f: Vec<u8> = Vec::new();
        f.extend(b"gimp xcf v003\0");
        let (w, h) = (4u32, 3u32);
        f.extend(be(w));
        f.extend(be(h));
        f.extend(be(0)); // RGB
        // properties: compression, end
        f.extend(be(17));
        f.extend(be(1));
        f.push(if rle { 1 } else { 0 });
        f.extend(be(0));
        f.extend(be(0));
        let layer_table_at = f.len();
        f.extend(vec![0u8; 4 * 3]); // two layers + terminator
        f.extend(be(0)); // channels terminator
        let mut ptrs = Vec::new();
        // Layers: first in the file is the top one.
        for (colour, alpha_prop) in [([255u8, 0, 0], true), ([0u8, 0, 255], false)] {
            ptrs.push(f.len() as u32);
            f.extend(be(w));
            f.extend(be(h));
            f.extend(be(1)); // RGBA
            f.extend(be(2));
            f.extend(b"L\0"); // name
            f.extend(be(8)); // visible
            f.extend(be(4));
            f.extend(be(1));
            if alpha_prop {
                f.extend(be(6)); // opacity 128
                f.extend(be(4));
                f.extend(be(128));
            }
            f.extend(be(0));
            f.extend(be(0));
            let hier_ptr_at = f.len();
            f.extend(be(0)); // hierarchy
            f.extend(be(0)); // mask
            let hier = f.len() as u32;
            f[hier_ptr_at..hier_ptr_at + 4].copy_from_slice(&be(hier));
            f.extend(be(w));
            f.extend(be(h));
            f.extend(be(4)); // bpp
            let level_ptr_at = f.len();
            f.extend(be(0));
            f.extend(be(0));
            let level = f.len() as u32;
            f[level_ptr_at..level_ptr_at + 4].copy_from_slice(&be(level));
            f.extend(be(w));
            f.extend(be(h));
            let tile_ptr_at = f.len();
            f.extend(be(0));
            f.extend(be(0));
            let tile = f.len() as u32;
            f[tile_ptr_at..tile_ptr_at + 4].copy_from_slice(&be(tile));
            let n = (w * h) as usize;
            if rle {
                // planar, each plane one run
                for v in [colour[0], colour[1], colour[2], 255u8] {
                    f.push((n - 1) as u8); // run of n
                    f.push(v);
                }
            } else {
                for _ in 0..n {
                    f.extend([colour[0], colour[1], colour[2], 255]);
                }
            }
        }
        f[layer_table_at..layer_table_at + 4].copy_from_slice(&be(ptrs[0]));
        f[layer_table_at + 4..layer_table_at + 8].copy_from_slice(&be(ptrs[1]));
        f
    }

    #[test]
    fn xcf_layers_are_composited_raw_and_rle() {
        for rle in [false, true] {
            let img = xcf(&make_xcf(rle)).unwrap();
            assert_eq!((img.width, img.height), (4, 3));
            // Red at 128/255 over opaque blue: about half and half.
            let p = &img.rgba[0..4];
            assert!((p[0] as i32 - 128).abs() <= 2 && (p[2] as i32 - 127).abs() <= 2 && p[3] == 255, "{p:?} rle={rle}");
        }
        assert!(xcf(b"not an xcf file at all, really not one").is_err());
    }

    #[test]
    fn rle_and_blend_modes() {
        // literal run of 3 then repeat of 2 (one plane of 5 bytes)
        let src = [253u8, 1, 2, 3, 1, 9];
        assert_eq!(rle_tile(&src, 1, 5).unwrap(), vec![1, 2, 3, 9, 9]);
        assert!((blend_channel(3, 0.5, 0.5) - 0.25).abs() < 1e-6);
        assert!((blend_channel(4, 0.5, 0.5) - 0.75).abs() < 1e-6);
        assert_eq!(half(0x3C00), 1.0);
    }

    #[test]
    fn u3d_header_and_nodes() {
        let mut v = Vec::new();
        let block = |ty: u32, data: &[u8]| {
            let mut b = ty.to_le_bytes().to_vec();
            b.extend((data.len() as u32).to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(data);
            while b.len() % 4 != 0 {
                b.push(0);
            }
            b
        };
        let mut hdr = vec![0u8; 24];
        hdr[0] = 0;
        hdr[1] = 0;
        hdr[2] = 4;
        hdr[3] = 0;
        v.extend(block(0x0044_3355, &hdr));
        let mut model = (5u16.to_le_bytes()).to_vec();
        model.extend(b"Scene");
        v.extend(block(0xFFFF_FF22, &model));
        let mut mesh = (4u16.to_le_bytes()).to_vec();
        mesh.extend(b"Mesh");
        mesh.extend(0u32.to_le_bytes());
        for x in [0u32, 12, 8] {
            mesh.extend(x.to_le_bytes());
        }
        v.extend(block(0xFFFF_FF31, &mesh));
        let p = std::env::temp_dir().join(format!("nv_u3d_{}.u3d", std::process::id()));
        let img = u3d(&v, &p, true).unwrap();
        assert!(img.colour_note.unwrap().contains("U3D"));
    }

    #[test]
    fn jpeg_spans_find_a_jpeg_in_junk() {
        let mut jpg = Vec::new();
        jpeg_encoder::Encoder::new(&mut jpg, 90).encode(&[200u8; 16 * 16 * 3], 16, 16, jpeg_encoder::ColorType::Rgb).unwrap();
        let mut file = vec![1u8, 2, 3, 4, 5];
        file.extend(&jpg);
        file.extend([9u8; 10]);
        let spans = jpeg_spans(&file, 0, file.len());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].0, 5);
        assert!(crate::decode_image_bytes(&file[spans[0].0..spans[0].1], "jpg").is_ok());
    }
}
