//! Vector clipart and drawings from older programs, drawn through the SVG renderer: 2D **DXF** drawings (lines, polylines, circles,
//! arcs, ellipses, splines as control polygons, solids, block inserts), **HP-GL** plotter files (`hpgl hgl hpg plt`: pen up / down, absolute
//! and relative moves, circles, arcs, pen colours) and binary-encoded **CGM** (Computer Graphics Metafile: polylines, polygons,
//! rectangles, circles, arcs, ellipses, line / fill / edge colours, colour tables). Text is not drawn.

use crate::Image;

pub const EXTS: &[&str] = &["hpgl", "hgl", "hpg", "plt", "cgm"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn decode(path: &std::path::Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    if !is_ext(ext) {
        return None;
    }
    let bytes = match crate::progress::read_file(path) {
        Ok(b) => b,
        Err(e) => return Some(Err(e)),
    };
    Some(match ext {
        "cgm" => cgm(&bytes),
        _ => hpgl(&bytes),
    }
    .and_then(|c| c.render(first_only)))
}

#[derive(Clone)]
pub struct Shape {
    pub pts: Vec<(f64, f64)>,
    pub closed: bool,
    pub stroke: Option<[u8; 3]>,
    pub fill: Option<[u8; 3]>,
}

#[derive(Default)]
pub struct Drawing {
    pub shapes: Vec<Shape>,
    /// An explicit extent (CGM's VDC extent): x1, y1, x2, y2.
    pub extent: Option<(f64, f64, f64, f64)>,
    pub what: String,
}

impl Drawing {
    pub fn render(&self, thumb: bool) -> Result<Image, String> {
        let svg = self.to_svg().ok_or("nothing drawable in this file (2D lines, curves and polygons are drawn; text is not)")?;
        let mut img = crate::svgdec::decode(svg.as_bytes(), thumb)?;
        img.colour_note = Some(format!("{} drawn by NuViewer (text is not drawn)", self.what));
        Ok(img)
    }

    fn bounds(&self) -> Option<(f64, f64, f64, f64)> {
        if let Some(e) = self.extent {
            let (x0, x1) = (e.0.min(e.2), e.0.max(e.2));
            let (y0, y1) = (e.1.min(e.3), e.1.max(e.3));
            if x1 > x0 && y1 > y0 {
                return Some((x0, y0, x1, y1));
            }
        }
        let mut b: Option<(f64, f64, f64, f64)> = None;
        for p in self.shapes.iter().flat_map(|s| s.pts.iter()) {
            if !p.0.is_finite() || !p.1.is_finite() {
                continue;
            }
            b = Some(match b {
                None => (p.0, p.1, p.0, p.1),
                Some((a, c, d, e)) => (a.min(p.0), c.min(p.1), d.max(p.0), e.max(p.1)),
            });
        }
        b
    }

    /// An SVG of the drawing (y up in the data, down in the SVG), on white, longest side 1600 units.
    pub fn to_svg(&self) -> Option<String> {
        let (x0, y0, x1, y1) = self.bounds()?;
        let (mut w, mut h) = (x1 - x0, y1 - y0);
        if w <= 0.0 && h <= 0.0 {
            return None;
        }
        let m = w.max(h) * 0.03;
        let (x0, y0) = (x0 - m, y0 - m);
        w += 2.0 * m;
        h += 2.0 * m;
        w = w.max(1e-9);
        h = h.max(1e-9);
        let s = 1600.0 / w.max(h);
        let sw = 1.0f64.max(1600.0 / 900.0);
        let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {:.0} {:.0}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>", (w * s).max(1.0), (h * s).max(1.0), (w * s).max(1.0), (h * s).max(1.0));
        let mut any = false;
        for sh in &self.shapes {
            if sh.pts.len() < 2 && sh.fill.is_none() {
                continue;
            }
            let mut d = String::new();
            for (i, p) in sh.pts.iter().enumerate() {
                if !p.0.is_finite() || !p.1.is_finite() {
                    continue;
                }
                d.push_str(&format!("{}{:.1} {:.1} ", if i == 0 { "M" } else { "L" }, (p.0 - x0) * s, (h - (p.1 - y0)) * s));
            }
            if sh.closed {
                d.push('Z');
            }
            let col = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
            let fill = sh.fill.map(col).unwrap_or_else(|| "none".into());
            let stroke = sh.stroke.map(col).unwrap_or_else(|| "none".into());
            out.push_str(&format!("<path d=\"{d}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{sw:.2}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>"));
            any = true;
        }
        out.push_str("</svg>");
        any.then_some(out)
    }
}

fn arc_points(cx: f64, cy: f64, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64) -> Vec<(f64, f64)> {
    let sweep = a1 - a0;
    let n = ((sweep.abs() / std::f64::consts::TAU * 96.0).ceil() as usize).clamp(8, 160);
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            let (x, y) = (rx * a.cos(), ry * a.sin());
            (cx + x * rot.cos() - y * rot.sin(), cy + x * rot.sin() + y * rot.cos())
        })
        .collect()
}

// ---- DXF (2D) ---------------------------------------------------------------------------------------------------------------

fn aci(c: i32) -> [u8; 3] {
    match c {
        1 => [220, 30, 30],
        2 => [200, 170, 0],
        3 => [20, 160, 40],
        4 => [0, 160, 170],
        5 => [30, 60, 220],
        6 => [190, 30, 190],
        8 => [128, 128, 128],
        9 => [180, 180, 180],
        _ => [20, 20, 20],
    }
}

type Tf = (f64, f64, f64, f64, f64); // sx, sy, rotation (radians), dx, dy

fn apply(t: &Tf, p: (f64, f64)) -> (f64, f64) {
    let (x, y) = (p.0 * t.0, p.1 * t.1);
    (x * t.2.cos() - y * t.2.sin() + t.3, x * t.2.sin() + y * t.2.cos() + t.4)
}

/// A 2D drawing from an ASCII DXF, or `Err` when it has no 2D entities (the 3D reader handles meshes first).
pub fn dxf_2d(bytes: &[u8]) -> Result<Drawing, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let mut pairs: Vec<(i32, String)> = Vec::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        match c.trim().parse::<i32>() {
            Ok(code) => pairs.push((code, v.trim().to_string())),
            Err(_) => return Err("dxf: not an ASCII DXF file".into()),
        }
    }
    // Entities as (type, codes), per section.
    let mut section = String::new();
    let mut blocks: std::collections::HashMap<String, Vec<(String, Vec<(i32, String)>)>> = std::collections::HashMap::new();
    let mut entities: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    let mut block_name = String::new();
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i].0 == 0 {
            let kind = pairs[i].1.clone();
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].0 != 0 {
                j += 1;
            }
            let body: Vec<(i32, String)> = pairs[i + 1..j].to_vec();
            match kind.as_str() {
                "SECTION" => section = body.iter().find(|p| p.0 == 2).map(|p| p.1.clone()).unwrap_or_default(),
                "ENDSEC" => section.clear(),
                "BLOCK" => block_name = body.iter().find(|p| p.0 == 2).map(|p| p.1.clone()).unwrap_or_default(),
                "ENDBLK" => block_name.clear(),
                _ if section == "ENTITIES" => entities.push((kind, body)),
                _ if section == "BLOCKS" && !block_name.is_empty() => blocks.entry(block_name.clone()).or_default().push((kind, body)),
                _ => {}
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let mut d = Drawing { what: "2D DXF drawing".into(), ..Default::default() };
    emit_dxf(&entities, &blocks, &(1.0, 1.0, 0.0, 0.0, 0.0), &mut d, 0);
    if d.shapes.is_empty() {
        return Err("dxf: no 2D entities to draw".into());
    }
    Ok(d)
}

fn emit_dxf(entities: &[(String, Vec<(i32, String)>)], blocks: &std::collections::HashMap<String, Vec<(String, Vec<(i32, String)>)>>, t: &Tf, d: &mut Drawing, depth: usize) {
    if depth > 6 || d.shapes.len() > 400_000 {
        return;
    }
    let f = |body: &[(i32, String)], code: i32| body.iter().find(|p| p.0 == code).and_then(|p| p.1.parse::<f64>().ok());
    let all = |body: &[(i32, String)], code: i32| -> Vec<f64> { body.iter().filter(|p| p.0 == code).filter_map(|p| p.1.parse().ok()).collect() };
    let mut poly3d: Option<(Vec<(f64, f64)>, bool, [u8; 3])> = None;
    let push = |d: &mut Drawing, pts: Vec<(f64, f64)>, closed: bool, col: [u8; 3]| {
        d.shapes.push(Shape { pts: pts.into_iter().map(|p| apply(t, p)).collect(), closed, stroke: Some(col), fill: None });
    };
    for (kind, body) in entities {
        let col = aci(f(body, 62).unwrap_or(0.0) as i32);
        match kind.as_str() {
            "LINE" => {
                if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (f(body, 10), f(body, 20), f(body, 11), f(body, 21)) {
                    push(d, vec![(x0, y0), (x1, y1)], false, col);
                }
            }
            "CIRCLE" => {
                if let (Some(cx), Some(cy), Some(r)) = (f(body, 10), f(body, 20), f(body, 40)) {
                    push(d, arc_points(cx, cy, r, r, 0.0, 0.0, std::f64::consts::TAU), true, col);
                }
            }
            "ARC" => {
                if let (Some(cx), Some(cy), Some(r), Some(a0), Some(a1)) = (f(body, 10), f(body, 20), f(body, 40), f(body, 50), f(body, 51)) {
                    let (a0, mut a1) = (a0.to_radians(), a1.to_radians());
                    while a1 <= a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    push(d, arc_points(cx, cy, r, r, 0.0, a0, a1), false, col);
                }
            }
            "ELLIPSE" => {
                if let (Some(cx), Some(cy), Some(mx), Some(my), Some(ratio)) = (f(body, 10), f(body, 20), f(body, 11), f(body, 21), f(body, 40)) {
                    let (a0, mut a1) = (f(body, 41).unwrap_or(0.0), f(body, 42).unwrap_or(std::f64::consts::TAU));
                    while a1 <= a0 {
                        a1 += std::f64::consts::TAU;
                    }
                    let r = (mx * mx + my * my).sqrt();
                    push(d, arc_points(cx, cy, r, r * ratio, my.atan2(mx), a0, a1), (a1 - a0 - std::f64::consts::TAU).abs() < 1e-6, col);
                }
            }
            "LWPOLYLINE" => {
                let xs = all(body, 10);
                let ys = all(body, 20);
                let pts: Vec<(f64, f64)> = xs.iter().zip(ys.iter()).map(|(x, y)| (*x, *y)).collect();
                push(d, pts, f(body, 70).unwrap_or(0.0) as i32 & 1 == 1, col);
            }
            "POLYLINE" => poly3d = Some((Vec::new(), f(body, 70).unwrap_or(0.0) as i32 & 1 == 1, col)),
            "VERTEX" => {
                if let (Some(p), Some(x), Some(y)) = (poly3d.as_mut(), f(body, 10), f(body, 20)) {
                    p.0.push((x, y));
                }
            }
            "SEQEND" => {
                if let Some((pts, closed, col)) = poly3d.take() {
                    push(d, pts, closed, col);
                }
            }
            "SPLINE" => {
                let pts: Vec<(f64, f64)> = all(body, 10).into_iter().zip(all(body, 20)).collect();
                push(d, pts, false, col);
            }
            "SOLID" | "3DFACE" | "TRACE" => {
                let mut pts = Vec::new();
                for k in 0..4 {
                    if let (Some(x), Some(y)) = (f(body, 10 + k), f(body, 20 + k)) {
                        pts.push((x, y));
                    }
                }
                if pts.len() == 4 && kind == "SOLID" {
                    pts.swap(2, 3); // SOLID lists its corners in a Z order
                }
                if pts.len() >= 3 {
                    let tp: Vec<(f64, f64)> = pts.into_iter().map(|p| apply(t, p)).collect();
                    d.shapes.push(Shape { pts: tp, closed: true, stroke: Some(col), fill: if kind == "SOLID" { Some(col) } else { None } });
                }
            }
            "INSERT" => {
                let Some(name) = body.iter().find(|p| p.0 == 2).map(|p| p.1.clone()) else { continue };
                let Some(block) = blocks.get(&name) else { continue };
                let (sx, sy) = (f(body, 41).unwrap_or(1.0), f(body, 42).unwrap_or(1.0));
                let rot = f(body, 50).unwrap_or(0.0).to_radians();
                let at = apply(t, (f(body, 10).unwrap_or(0.0), f(body, 20).unwrap_or(0.0)));
                let child: Tf = (t.0 * sx, t.1 * sy, t.2 + rot, at.0, at.1);
                emit_dxf(block, blocks, &child, d, depth + 1);
            }
            _ => {}
        }
    }
}

// ---- HP-GL ------------------------------------------------------------------------------------------------------------------

fn pen_colour(n: usize) -> [u8; 3] {
    const P: [[u8; 3]; 8] = [[20, 20, 20], [20, 20, 20], [220, 30, 30], [20, 160, 40], [200, 170, 0], [30, 60, 220], [190, 30, 190], [0, 160, 170]];
    P[n % 8]
}

pub fn hpgl(bytes: &[u8]) -> Result<Drawing, String> {
    // HP-GL/2 inside PCL / PJL: start at the first IN or "enter HP-GL/2" escape.
    let text: String = bytes.iter().map(|&b| if b == 0x1B { '\u{1}' } else if b.is_ascii() { b as char } else { ' ' }).collect();
    let start = text.find("IN;").or_else(|| text.find("IN\n")).or_else(|| text.find("SP")).or_else(|| text.find("PU")).ok_or("hpgl: no HP-GL commands (IN, SP, PU, PD ...) found")?;
    let body = &text[start..];
    let mut d = Drawing { what: "HP-GL plot".into(), ..Default::default() };
    let (mut pen, mut down, mut rel) = (1usize, false, false);
    let mut pos = (0.0f64, 0.0f64);
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let flush = |d: &mut Drawing, cur: &mut Vec<(f64, f64)>, pen: usize| {
        if cur.len() >= 2 {
            d.shapes.push(Shape { pts: std::mem::take(cur), closed: false, stroke: Some(pen_colour(pen)), fill: None });
        } else {
            cur.clear();
        }
    };
    let mut known = 0;
    for cmd in body.split([';', '\n', '\r']) {
        let cmd = cmd.trim_matches(|c: char| c.is_whitespace() || c == '\u{1}');
        if cmd.len() < 2 || !cmd.is_char_boundary(2) {
            continue;
        }
        let (op, args) = cmd.split_at(2);
        let nums: Vec<f64> = args.split(|c: char| c == ',' || c.is_whitespace()).filter_map(|t| t.parse().ok()).collect();
        match op.to_ascii_uppercase().as_str() {
            "IN" | "DF" => {
                flush(&mut d, &mut cur, pen);
                down = false;
                rel = false;
            }
            "SP" => {
                flush(&mut d, &mut cur, pen);
                pen = nums.first().map(|n| *n as usize).unwrap_or(0);
                known += 1;
            }
            "PU" | "PD" | "PA" | "PR" => {
                let o = op.to_ascii_uppercase();
                match o.as_str() {
                    "PU" => {
                        if down {
                            flush(&mut d, &mut cur, pen);
                        }
                        down = false;
                    }
                    "PD" => down = true,
                    "PA" => rel = false,
                    _ => rel = true,
                }
                known += 1;
                for xy in nums.chunks_exact(2) {
                    let np = if rel { (pos.0 + xy[0], pos.1 + xy[1]) } else { (xy[0], xy[1]) };
                    if down {
                        if cur.is_empty() {
                            cur.push(pos);
                        }
                        cur.push(np);
                    } else {
                        flush(&mut d, &mut cur, pen);
                    }
                    pos = np;
                }
                if !down && nums.len() >= 2 {
                    cur.clear();
                }
            }
            "CI" => {
                if let Some(r) = nums.first() {
                    flush(&mut d, &mut cur, pen);
                    d.shapes.push(Shape { pts: arc_points(pos.0, pos.1, *r, *r, 0.0, 0.0, std::f64::consts::TAU), closed: true, stroke: Some(pen_colour(pen)), fill: None });
                    known += 1;
                }
            }
            "AA" | "AR" if nums.len() >= 3 => {
                flush(&mut d, &mut cur, pen);
                let c = if op.eq_ignore_ascii_case("AR") { (pos.0 + nums[0], pos.1 + nums[1]) } else { (nums[0], nums[1]) };
                let (rx, ry) = (pos.0 - c.0, pos.1 - c.1);
                let (r, a0) = ((rx * rx + ry * ry).sqrt(), ry.atan2(rx));
                let sweep = nums[2].to_radians();
                let pts = arc_points(c.0, c.1, r, r, 0.0, a0, a0 + sweep);
                pos = *pts.last().unwrap_or(&pos);
                if down {
                    d.shapes.push(Shape { pts, closed: false, stroke: Some(pen_colour(pen)), fill: None });
                }
                known += 1;
            }
            _ => {}
        }
    }
    flush(&mut d, &mut cur, pen);
    if known == 0 || d.shapes.is_empty() {
        return Err("hpgl: no pen movements with the pen down (this is not an HP-GL plot, or it is empty)".into());
    }
    Ok(d)
}

// ---- CGM (binary encoding) ---------------------------------------------------------------------------------------------------

struct Cgm {
    vdc_bits: u32,
    vdc_real: bool,
    real_float: bool,
    real_bits: u32,
    direct: bool,
    col_bits: u32,
    idx_bits: u32,
    line: [u8; 3],
    fill: [u8; 3],
    edge: [u8; 3],
    interior: u16,
    edge_vis: bool,
    table: Vec<[u8; 3]>,
}

fn int_be(b: &[u8], bits: u32, signed: bool) -> Option<(i64, usize)> {
    let n = (bits / 8).max(1) as usize;
    let s = b.get(..n)?;
    let mut v: i64 = 0;
    for &c in s {
        v = (v << 8) | c as i64;
    }
    if signed && s[0] & 0x80 != 0 {
        v -= 1i64 << (8 * n);
    }
    Some((v, n))
}

impl Cgm {
    fn vdc(&self, b: &[u8]) -> Option<(f64, usize)> {
        if self.vdc_real {
            return match (self.real_float, self.real_bits) {
                (true, 32) => Some((f32::from_bits(u32::from_be_bytes(b.get(..4)?.try_into().ok()?)) as f64, 4)),
                (true, 64) => Some((f64::from_bits(u64::from_be_bytes(b.get(..8)?.try_into().ok()?)), 8)),
                (false, 32) => Some((i32::from_be_bytes(b.get(..4)?.try_into().ok()?) as f64 / 65536.0, 4)),
                (false, 64) => {
                    let w = i32::from_be_bytes(b.get(..4)?.try_into().ok()?) as f64;
                    let f = u32::from_be_bytes(b.get(4..8)?.try_into().ok()?) as f64 / 4294967296.0;
                    Some((w + f, 8))
                }
                _ => None,
            };
        }
        int_be(b, self.vdc_bits, true).map(|(v, n)| (v as f64, n))
    }

    fn points(&self, b: &[u8]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut at = 0;
        while let (Some((x, n)), true) = (self.vdc(&b[at.min(b.len())..]), at < b.len()) {
            let Some((y, m)) = self.vdc(&b[(at + n).min(b.len())..]) else { break };
            out.push((x, y));
            at += n + m;
        }
        out
    }

    fn colour(&self, b: &[u8]) -> Option<[u8; 3]> {
        if self.direct {
            let n = (self.col_bits / 8).max(1) as usize;
            let c = |k: usize| -> Option<u8> { Some(*b.get(k * n)?) }; // the high byte of each component
            Some([c(0)?, c(1)?, c(2)?])
        } else {
            let (i, _) = int_be(b, self.idx_bits, false)?;
            Some(self.table.get(i as usize).copied().unwrap_or(if i == 0 { [255, 255, 255] } else { [0, 0, 0] }))
        }
    }
}

pub fn cgm(bytes: &[u8]) -> Result<Drawing, String> {
    // BEGIN METAFILE is class 0 element 1; the clear-text encoding starts with the characters "BEGMF".
    if bytes.starts_with(b"BEGMF") || bytes.get(..16).is_some_and(|h| h.windows(5).any(|w| w == b"BEGMF")) {
        return Err("cgm: the clear-text CGM encoding is not supported (the binary encoding is)".into());
    }
    if u16::from_be_bytes([*bytes.first().ok_or("cgm: empty")?, *bytes.get(1).ok_or("cgm: truncated")?]) >> 5 != 1 {
        return Err("cgm: not a binary CGM file (it must start with BEGIN METAFILE)".into());
    }
    let mut st = Cgm { vdc_bits: 16, vdc_real: false, real_float: true, real_bits: 32, direct: false, col_bits: 8, idx_bits: 8, line: [0; 3], fill: [0; 3], edge: [0; 3], interior: 0, edge_vis: false, table: Vec::new() };
    let mut d = Drawing { what: "CGM drawing".into(), ..Default::default() };
    let mut pos = 0usize;
    let mut elements = 0;
    while pos + 2 <= bytes.len() {
        let hdr = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]);
        pos += 2;
        let (class, id) = ((hdr >> 12) as u8, ((hdr >> 5) & 0x7F) as u8);
        let mut len = (hdr & 0x1F) as usize;
        let mut data: Vec<u8> = Vec::new();
        if len == 31 {
            // Long form: 16 bits, top bit "more partitions follow".
            loop {
                let Some(l) = bytes.get(pos..pos + 2) else { break };
                let l = u16::from_be_bytes([l[0], l[1]]);
                pos += 2;
                let n = (l & 0x7FFF) as usize;
                let Some(chunk) = bytes.get(pos..pos + n) else { break };
                data.extend_from_slice(chunk);
                pos += n + (n & 1);
                if l & 0x8000 == 0 {
                    break;
                }
                // The next partition starts with a fresh length word.
            }
            len = data.len();
        } else {
            let Some(chunk) = bytes.get(pos..pos + len) else { break };
            data.extend_from_slice(chunk);
            pos += len + (len & 1);
        }
        let _ = len;
        elements += 1;
        let p = &data[..];
        match (class, id) {
            (1, 3) => st.vdc_real = int_be(p, 16, false).map(|v| v.0 == 1).unwrap_or(false),
            (1, 4) => st.vdc_bits = int_be(p, 16, false).map(|v| v.0 as u32).filter(|b| matches!(b, 8 | 16 | 24 | 32)).unwrap_or(16),
            (1, 5) => {
                // real precision: 0 floating / 1 fixed point, then two widths (exponent + mantissa, or whole + fraction)
                let kind = int_be(p, 16, false).map(|v| v.0).unwrap_or(0);
                let a = p.get(2..).and_then(|q| int_be(q, 16, false)).map(|v| v.0).unwrap_or(9);
                let b = p.get(4..).and_then(|q| int_be(q, 16, false)).map(|v| v.0).unwrap_or(23);
                st.real_float = kind == 0;
                st.real_bits = if a + b <= 32 { 32 } else { 64 };
            }
            (1, 7) => st.col_bits = int_be(p, 16, false).map(|v| v.0 as u32).unwrap_or(8),
            (1, 8) => st.idx_bits = int_be(p, 16, false).map(|v| v.0 as u32).unwrap_or(8),
            (2, 2) => st.direct = int_be(p, 16, false).map(|v| v.0 == 1).unwrap_or(false),
            (2, 6) => {
                let pts = st.points(p);
                if pts.len() == 2 {
                    d.extent = Some((pts[0].0, pts[0].1, pts[1].0, pts[1].1));
                }
            }
            (5, 4) => st.line = st.colour(p).unwrap_or(st.line),
            (5, 23) => st.fill = st.colour(p).unwrap_or(st.fill),
            (5, 29) => st.edge = st.colour(p).unwrap_or(st.edge),
            (5, 22) => st.interior = int_be(p, 16, false).map(|v| v.0 as u16).unwrap_or(0),
            (5, 30) => st.edge_vis = int_be(p, 16, false).map(|v| v.0 == 1).unwrap_or(false),
            (5, 34) => {
                if let Some((start, n)) = int_be(p, st.idx_bits, false) {
                    let cn = (st.col_bits / 8).max(1) as usize * 3;
                    for (k, ch) in p[n..].chunks_exact(cn).enumerate() {
                        let step = cn / 3;
                        let c = [ch[0], ch[step], ch[2 * step]];
                        let i = start as usize + k;
                        if st.table.len() <= i {
                            st.table.resize(i + 1, [0, 0, 0]);
                        }
                        st.table[i] = c;
                    }
                }
            }
            (4, 1) => {
                let pts = st.points(p);
                d.shapes.push(Shape { pts, closed: false, stroke: Some(st.line), fill: None });
            }
            (4, 2) => {
                for pair in st.points(p).chunks_exact(2) {
                    d.shapes.push(Shape { pts: pair.to_vec(), closed: false, stroke: Some(st.line), fill: None });
                }
            }
            (4, 7) | (4, 8) | (4, 11) | (4, 12) | (4, 15) | (4, 16) | (4, 17) => {
                let pts = st.points(p);
                let shape_pts: Option<(Vec<(f64, f64)>, bool)> = match id {
                    7 | 8 => Some((pts, true)),
                    11 if pts.len() >= 2 => Some((vec![pts[0], (pts[1].0, pts[0].1), pts[1], (pts[0].0, pts[1].1)], true)),
                    12 if !pts.is_empty() => {
                        let n = st.vdc(p).map(|v| v.1).unwrap_or(2);
                        st.vdc(p.get(2 * n..).unwrap_or(&[])).map(|(r, _)| (arc_points(pts[0].0, pts[0].1, r, r, 0.0, 0.0, std::f64::consts::TAU), true))
                    }
                    15 | 16 if pts.len() >= 3 => {
                        let n = st.vdc(p).map(|v| v.1).unwrap_or(2);
                        let r = st.vdc(p.get(6 * n..).unwrap_or(&[])).map(|v| v.0).unwrap_or(0.0);
                        let (c, s0, s1) = (pts[0], pts[1], pts[2]);
                        let (a0, a1) = (s0.1.atan2(s0.0), s1.1.atan2(s1.0));
                        let mut a1 = a1;
                        while a1 <= a0 {
                            a1 += std::f64::consts::TAU;
                        }
                        Some((arc_points(c.0, c.1, r, r, 0.0, a0, a1), id == 16))
                    }
                    17 if pts.len() >= 3 => {
                        let (c, p1, p2) = (pts[0], pts[1], pts[2]);
                        let (v1, v2) = ((p1.0 - c.0, p1.1 - c.1), (p2.0 - c.0, p2.1 - c.1));
                        Some((
                            (0..=96).map(|i| {
                                let a = std::f64::consts::TAU * i as f64 / 96.0;
                                (c.0 + v1.0 * a.cos() + v2.0 * a.sin(), c.1 + v1.1 * a.cos() + v2.1 * a.sin())
                            }).collect(),
                            true,
                        ))
                    }
                    _ => None,
                };
                if let Some((pts, closed)) = shape_pts {
                    let solid = st.interior == 1;
                    d.shapes.push(Shape { pts, closed, stroke: if st.edge_vis { Some(st.edge) } else if !solid { Some(st.fill) } else { None }, fill: solid.then_some(st.fill) });
                }
            }
            (0, 2) => break, // END METAFILE
            _ => {}
        }
    }
    if d.shapes.is_empty() {
        return Err(format!("cgm: no drawable primitives found ({elements} elements read)"));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxf_lines_circles_and_blocks_become_shapes() {
        let dxf = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB1\n0\nLINE\n10\n0\n20\n0\n11\n1\n21\n0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n10\n21\n10\n0\nCIRCLE\n10\n5\n20\n5\n40\n2\n0\nINSERT\n2\nB1\n10\n3\n20\n3\n41\n2\n42\n2\n0\nENDSEC\n0\nEOF\n";
        let d = dxf_2d(dxf.as_bytes()).unwrap();
        assert_eq!(d.shapes.len(), 3);
        let svg = d.to_svg().unwrap();
        assert!(svg.contains("<path"));
        let img = d.render(false).unwrap();
        assert!(img.rgba.chunks_exact(4).any(|p| p[0] < 100), "no lines drawn");
        assert!(dxf_2d(b"hello world").is_err());
    }

    #[test]
    fn hpgl_pen_moves_arcs_and_junk() {
        let d = hpgl(b"IN;SP1;PU0,0;PD1000,0,1000,1000;PU;PA0,0;PD;PR500,0;SP2;PU2000,2000;CI300;").unwrap();
        assert!(d.shapes.len() >= 3, "{}", d.shapes.len());
        assert!(d.render(false).is_ok());
        assert!(hpgl(b"nothing plotted here").is_err());
        // Illustrator's palette files share the .plt name: they are not HP-GL.
        assert!(hpgl(b"\x00\x01\x02 palette data without commands").is_err());
    }

    #[test]
    fn cgm_polygon_and_polyline() {
        fn elem(class: u16, id: u16, data: &[u8]) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend(((class << 12) | (id << 5) | data.len() as u16).to_be_bytes());
            v.extend(data);
            if data.len() % 2 == 1 {
                v.push(0);
            }
            v
        }
        let pts = |p: &[(i16, i16)]| -> Vec<u8> { p.iter().flat_map(|(x, y)| [x.to_be_bytes(), y.to_be_bytes()].concat()).collect() };
        let mut f = elem(0, 1, b"\x03abc");
        f.extend(elem(2, 6, &pts(&[(0, 0), (100, 100)])));
        f.extend(elem(5, 22, &1u16.to_be_bytes())); // solid fill
        f.extend(elem(2, 2, &1u16.to_be_bytes())); // direct colour
        f.extend(elem(5, 23, &[255, 0, 0]));
        f.extend(elem(4, 7, &pts(&[(10, 10), (90, 10), (50, 90)])));
        f.extend(elem(4, 1, &pts(&[(0, 0), (100, 100)])));
        f.extend(elem(0, 2, &[]));
        let d = cgm(&f).unwrap();
        assert_eq!(d.shapes.len(), 2);
        let img = d.render(false).unwrap();
        assert!(img.rgba.chunks_exact(4).any(|p| p[0] > 200 && p[1] < 60), "the red triangle is missing");
        assert!(cgm(b"BEGMF 'x';").is_err());
        assert!(cgm(b"junk bytes").is_err());
    }
}
