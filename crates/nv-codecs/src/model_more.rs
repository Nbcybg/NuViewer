//! More 3D formats, read natively: 3DS, 3MF, AMF, Collada (DAE), LAS point clouds, DXF (3DFACE / polyface / MESH), LightWave
//! (LWO), ASCII USD and USDZ (with an ASCII layer), Maya ASCII (MA) and Unity packages (the model inside).
//!
//! Formats that are closed or need a CAD kernel (STEP, IGES, Alembic, binary USD, DWG) go through a converter if one is
//! installed (see `external`); the proprietary scene formats (3ds Max, Cinema 4D, Modo, ZBrush, Substance Painter, Marmoset,
//! Softimage, Unreal assets) show the picture stored inside the file, or say plainly what to export from.

use super::Mesh;
use std::path::{Path, PathBuf};

pub const NATIVE: &[&str] = &["3ds", "3mf", "amf", "dae", "las", "dxf", "lwo", "lwo2", "usda", "usdz", "ma", "unitypackage", "wrl", "vrml", "wrz"];
/// Needs a converter (assimp, Blender, FreeCAD, ODA) that is not part of NuViewer; the file is listed and the message says which.
pub const CONVERT: &[&str] = &["step", "stp", "iges", "igs", "abc", "usd", "usdc", "dwg"];
/// Scene files of programs whose format is closed: the picture stored inside is shown when there is one.
pub const CLOSED: &[(&str, &str)] = &[
    ("max", "3ds Max"),
    ("c4d", "Cinema 4D"),
    ("lxo", "Modo"),
    ("ztl", "ZBrush"),
    ("zpr", "ZBrush"),
    ("spp", "Substance 3D Painter"),
    ("ms", "Marmoset Toolbag"),
    ("hrc", "Softimage"),
    ("uasset", "Unreal Engine"),
    ("mb", "Maya"),
    ("f3d", "Fusion 360"),
];

pub fn is_native(ext: &str) -> bool {
    NATIVE.contains(&ext)
}

pub fn closed_app(ext: &str) -> Option<&'static str> {
    CLOSED.iter().find(|(e, _)| *e == ext).map(|(_, a)| *a)
}

pub fn all_exts() -> Vec<&'static str> {
    NATIVE.iter().copied().chain(CONVERT.iter().copied()).chain(CLOSED.iter().map(|(e, _)| *e)).collect()
}

fn f32be(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u16le(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn f32le(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn f64le(b: &[u8], o: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// Triangles and positions collected from any number of objects.
#[derive(Default)]
struct Soup {
    pos: Vec<[f32; 3]>,
    idx: Vec<u32>,
    colors: Vec<[u8; 4]>,
}

impl Soup {
    fn base(&self) -> u32 {
        self.pos.len() as u32
    }
    /// A polygon (any number of corners, local indices) as a fan of triangles.
    fn polygon(&mut self, base: u32, corners: &[u32]) {
        for k in 1..corners.len().saturating_sub(1) {
            self.idx.extend([base + corners[0], base + corners[k], base + corners[k + 1]]);
        }
    }
    fn finish(self, y_up: bool) -> Result<Mesh, String> {
        let mut m = Mesh::build(self.pos, Vec::new(), self.idx, y_up)?;
        if !self.colors.is_empty() && self.colors.len() == m.positions.len() {
            m.colors = self.colors;
        }
        Ok(m)
    }
}

// ---- 3DS ---------------------------------------------------------------------------------------

pub fn parse_3ds(b: &[u8]) -> Result<Mesh, String> {
    if u16le(b, 0) != Some(0x4D4D) {
        return Err("not a 3DS file".into());
    }
    let mut soup = Soup::default();
    // Walk chunks; descend into the containers that lead to meshes.
    fn walk(b: &[u8], mut at: usize, end: usize, soup: &mut Soup, base: &mut u32) {
        while at + 6 <= end {
            let (id, len) = (u16le(b, at).unwrap_or(0), u32le(b, at + 2).unwrap_or(0) as usize);
            if len < 6 || at + len > end.max(at + 6) && at + len > b.len() {
                return;
            }
            let (body, stop) = (at + 6, (at + len).min(end));
            match id {
                0x4D4D | 0x3D3D => walk(b, body, stop, soup, base),
                0x4000 => {
                    // object: a zero-terminated name, then its sub-chunks
                    let mut p = body;
                    while p < stop && b[p] != 0 {
                        p += 1;
                    }
                    walk(b, p + 1, stop, soup, base);
                }
                0x4100 => {
                    *base = soup.base();
                    walk(b, body, stop, soup, base);
                }
                0x4110 => {
                    let n = u16le(b, body).unwrap_or(0) as usize;
                    for i in 0..n {
                        let o = body + 2 + i * 12;
                        if let (Some(x), Some(y), Some(z)) = (f32le(b, o), f32le(b, o + 4), f32le(b, o + 8)) {
                            soup.pos.push([x, y, z]);
                        }
                    }
                }
                0x4120 => {
                    let n = u16le(b, body).unwrap_or(0) as usize;
                    for i in 0..n {
                        let o = body + 2 + i * 8;
                        if let (Some(a), Some(c), Some(d)) = (u16le(b, o), u16le(b, o + 2), u16le(b, o + 4)) {
                            soup.idx.extend([*base + a as u32, *base + c as u32, *base + d as u32]);
                        }
                    }
                }
                _ => {}
            }
            at += len;
        }
    }
    let mut base = 0;
    walk(b, 0, b.len(), &mut soup, &mut base);
    soup.finish(false)
}

// ---- XML formats: 3MF, AMF, Collada ----------------------------------------------------------------

fn floats(s: &str) -> Vec<f32> {
    s.split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect()
}

fn ints(s: &str) -> Vec<u32> {
    s.split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect()
}

fn attr_f(n: &roxmltree::Node, name: &str) -> f32 {
    n.attribute(name).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

fn xml(bytes: &[u8]) -> Result<String, String> {
    let s = String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes)).into_owned();
    Ok(s)
}

fn xml_opts() -> roxmltree::ParsingOptions {
    roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }
}

/// 3MF: a zip with `3D/3dmodel.model`. Objects with meshes, and the build items (with their transforms) that place them.
/// Slicer files (Bambu Studio, Orca, PrusaSlicer) use the production extension: the objects of the root model are lists of
/// components whose meshes live in other model files (`p:path="/3D/Objects/object_3.model"`).
pub fn parse_3mf(path: &Path) -> Result<Mesh, String> {
    let mut zip = crate::docthumb::Zip::open(path)?;
    let norm = |n: &str| n.trim_start_matches('/').to_ascii_lowercase();
    let root_entry = zip.entries.iter().find(|e| norm(&e.name) == "3d/3dmodel.model").or_else(|| zip.entries.iter().find(|e| norm(&e.name).ends_with(".model"))).cloned().ok_or("no 3D model inside this 3MF")?;
    struct Obj {
        id: String,
        pos: Vec<[f32; 3]>,
        tri: Vec<[u32; 3]>,
        /// (file of the component's object, "" = same file; object id; transform)
        comps: Vec<(String, String, [f32; 12])>,
    }
    let ident = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    let tf = |s: Option<&str>| -> [f32; 12] {
        let v = s.map(floats).unwrap_or_default();
        if v.len() == 12 { std::array::from_fn(|i| v[i]) } else { ident }
    };
    let load = |zip: &mut crate::docthumb::Zip, entry: &crate::docthumb::Entry| -> Result<(Vec<Obj>, Vec<(String, [f32; 12])>), String> {
        let text = xml(&zip.read(entry, 512 << 20)?)?;
        let doc = roxmltree::Document::parse_with_options(&text, xml_opts()).map_err(|e| format!("3MF: {e}"))?;
        let objs = doc
            .descendants()
            .filter(|n| n.has_tag_name("object"))
            .map(|o| {
                let mesh = o.children().find(|c| c.has_tag_name("mesh"));
                let pos = mesh.map(|m| m.descendants().filter(|n| n.has_tag_name("vertex")).map(|v| [attr_f(&v, "x"), attr_f(&v, "y"), attr_f(&v, "z")]).collect()).unwrap_or_default();
                let tri = mesh.map(|m| m.descendants().filter(|n| n.has_tag_name("triangle")).map(|t| [attr_f(&t, "v1") as u32, attr_f(&t, "v2") as u32, attr_f(&t, "v3") as u32]).collect()).unwrap_or_default();
                let comps = o
                    .descendants()
                    .filter(|n| n.has_tag_name("component"))
                    .map(|c| {
                        // The production extension's path attribute, whatever prefix it was bound to.
                        let file = c.attributes().find(|a| a.name() == "path").map(|a| a.value().to_string()).unwrap_or_default();
                        (file, c.attribute("objectid").unwrap_or("").to_string(), tf(c.attribute("transform")))
                    })
                    .collect();
                Obj { id: o.attribute("id").unwrap_or("").to_string(), pos, tri, comps }
            })
            .collect();
        let items = doc.descendants().filter(|n| n.has_tag_name("item")).map(|i| (i.attribute("objectid").unwrap_or("").to_string(), tf(i.attribute("transform")))).collect();
        Ok((objs, items))
    };
    let (root_objs, items) = load(&mut zip, &root_entry)?;
    // Model files named by components, loaded once each.
    let mut files: std::collections::HashMap<String, Vec<Obj>> = std::collections::HashMap::new();
    files.insert(String::new(), root_objs);
    let wanted: Vec<String> = files[""].iter().flat_map(|o| o.comps.iter().map(|c| c.0.clone())).filter(|f| !f.is_empty()).collect();
    for f in wanted {
        let key = norm(&f);
        if files.contains_key(&key) {
            continue;
        }
        if let Some(e) = zip.entries.iter().find(|e| norm(&e.name) == key).cloned() {
            if let Ok((objs, _)) = load(&mut zip, &e) {
                files.insert(key, objs);
            }
        }
    }
    fn apply(t: &[f32; 12], p: [f32; 3]) -> [f32; 3] {
        // 3MF matrices are row-vector: [x y z 1] * M, rows m00 m01 m02 / m10 .. / m20 .. / m30 m31 m32
        [p[0] * t[0] + p[1] * t[3] + p[2] * t[6] + t[9], p[0] * t[1] + p[1] * t[4] + p[2] * t[7] + t[10], p[0] * t[2] + p[1] * t[5] + p[2] * t[8] + t[11]]
    }
    fn emit(files: &std::collections::HashMap<String, Vec<Obj>>, file: &str, id: &str, t: &[f32; 12], soup: &mut Soup, depth: usize) {
        if depth > 8 {
            return;
        }
        let Some(o) = files.get(file).and_then(|objs| objs.iter().find(|o| o.id == id)) else { return };
        let base = soup.base();
        soup.pos.extend(o.pos.iter().map(|p| apply(t, *p)));
        for tr in &o.tri {
            if tr.iter().all(|&i| (i as usize) < o.pos.len()) {
                soup.idx.extend([base + tr[0], base + tr[1], base + tr[2]]);
            }
        }
        for (cfile, cid, ct) in &o.comps {
            // child transform first, then ours (row-vector convention)
            let mut m = [0.0f32; 12];
            let (a, b) = (ct, t);
            // combine: p' = (p * A) * B
            for r in 0..3 {
                for c in 0..3 {
                    m[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
                }
            }
            for c in 0..3 {
                m[9 + c] = a[9] * b[c] + a[10] * b[3 + c] + a[11] * b[6 + c] + b[9 + c];
            }
            let key = if cfile.is_empty() { String::new() } else { cfile.trim_start_matches('/').to_ascii_lowercase() };
            emit(files, &key, cid, &m, soup, depth + 1);
        }
    }
    let mut soup = Soup::default();
    if items.is_empty() {
        for o in &files[""] {
            emit(&files, "", &o.id, &ident, &mut soup, 0);
        }
    } else {
        for (id, t) in items {
            emit(&files, "", &id, &t, &mut soup, 0);
        }
    }
    soup.finish(false)
}

/// The picture a 3MF project carries (slicers store the plate render as a thumbnail): used when the model itself can't be drawn.
pub fn parse_3mf_picture(path: &Path) -> Option<crate::Image> {
    let mut zip = crate::docthumb::Zip::open(path).ok()?;
    let best = zip
        .entries
        .iter()
        .filter(|e| crate::docthumb::is_image_name(&e.name) && !e.name.to_ascii_lowercase().contains("profile"))
        .max_by_key(|e| {
            let n = e.name.to_ascii_lowercase();
            (n.contains("thumbnail") || n.contains("plate") || n.contains("preview"), e.size)
        })
        .cloned()?;
    let bytes = zip.read(&best, 64 << 20).ok()?;
    let mut img = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&best.name)).ok()?;
    img.colour_note = Some(format!("the picture stored in this 3MF project ({})", best.name));
    Some(img)
}

/// AMF: XML, often zipped.
pub fn parse_amf(path: &Path, bytes: &[u8]) -> Result<Mesh, String> {
    let text = if bytes.starts_with(b"PK") {
        let mut zip = crate::docthumb::Zip::open(path)?;
        let e = zip.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".amf")).cloned().ok_or("no .amf inside")?;
        xml(&zip.read(&e, 512 << 20)?)?
    } else {
        xml(bytes)?
    };
    let doc = roxmltree::Document::parse_with_options(&text, xml_opts()).map_err(|e| format!("AMF: {e}"))?;
    let mut soup = Soup::default();
    for mesh in doc.descendants().filter(|n| n.has_tag_name("mesh")) {
        let base = soup.base();
        let coord = |v: roxmltree::Node, name: &str| v.descendants().find(|n| n.has_tag_name(name)).and_then(|n| n.text()).and_then(|t| t.trim().parse::<f32>().ok()).unwrap_or(0.0);
        if let Some(vs) = mesh.children().find(|c| c.has_tag_name("vertices")) {
            for v in vs.children().filter(|c| c.has_tag_name("vertex")) {
                soup.pos.push([coord(v, "x"), coord(v, "y"), coord(v, "z")]);
            }
        }
        for vol in mesh.children().filter(|c| c.has_tag_name("volume")) {
            for t in vol.children().filter(|c| c.has_tag_name("triangle")) {
                let i = |n: &str| t.descendants().find(|x| x.has_tag_name(n)).and_then(|x| x.text()).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
                soup.idx.extend([base + i("v1"), base + i("v2"), base + i("v3")]);
            }
        }
    }
    soup.finish(false)
}

/// Collada: geometry only (positions and the faces that use them); the scene's node transforms are not applied.
pub fn parse_dae(bytes: &[u8]) -> Result<Mesh, String> {
    let text = xml(bytes)?;
    let doc = roxmltree::Document::parse_with_options(&text, xml_opts()).map_err(|e| format!("Collada: {e}"))?;
    let y_up = !doc.descendants().any(|n| n.has_tag_name("up_axis") && n.text().is_some_and(|t| t.trim().starts_with('Z')));
    let mut soup = Soup::default();
    for mesh in doc.descendants().filter(|n| n.has_tag_name("mesh")) {
        // the POSITION source of this mesh
        let id_of = |s: &str| s.trim_start_matches('#').to_string();
        let vertices: std::collections::HashMap<String, String> = mesh
            .children()
            .filter(|c| c.has_tag_name("vertices"))
            .filter_map(|v| {
                let pos = v.children().find(|i| i.has_tag_name("input") && i.attribute("semantic") == Some("POSITION"))?;
                Some((v.attribute("id")?.to_string(), id_of(pos.attribute("source")?)))
            })
            .collect();
        let source = |id: &str| -> Option<(Vec<f32>, usize)> {
            let s = mesh.children().find(|c| c.has_tag_name("source") && c.attribute("id") == Some(id))?;
            let arr = s.children().find(|c| c.has_tag_name("float_array"))?;
            let stride = s.descendants().find(|c| c.has_tag_name("accessor")).and_then(|a| a.attribute("stride")).and_then(|v| v.parse().ok()).unwrap_or(3);
            Some((floats(arr.text().unwrap_or("")), stride))
        };
        for prim in mesh.children().filter(|c| matches!(c.tag_name().name(), "triangles" | "polylist" | "polygons" | "trifans" | "tristrips")) {
            let inputs: Vec<roxmltree::Node> = prim.children().filter(|c| c.has_tag_name("input")).collect();
            let Some(vi) = inputs.iter().find(|i| i.attribute("semantic") == Some("VERTEX")) else { continue };
            let stride = inputs.iter().filter_map(|i| i.attribute("offset").and_then(|o| o.parse::<usize>().ok())).max().unwrap_or(0) + 1;
            let offset: usize = vi.attribute("offset").and_then(|o| o.parse().ok()).unwrap_or(0);
            let Some(src_id) = vi.attribute("source").map(id_of).and_then(|s| vertices.get(&s).cloned().or(Some(s))) else { continue };
            let Some((fl, comp)) = source(&src_id) else { continue };
            let base = soup.base();
            for p in fl.chunks_exact(comp.max(3)) {
                soup.pos.push([p[0], p[1], p[2]]);
            }
            let nverts = fl.len() / comp.max(3);
            let corner = |p: &[u32], k: usize| p.get(k * stride + offset).copied().filter(|&i| (i as usize) < nverts);
            match prim.tag_name().name() {
                "triangles" => {
                    for p in prim.children().filter(|c| c.has_tag_name("p")) {
                        let p = ints(p.text().unwrap_or(""));
                        for t in 0..p.len() / stride / 3 {
                            if let (Some(a), Some(b), Some(c)) = (corner(&p, t * 3), corner(&p, t * 3 + 1), corner(&p, t * 3 + 2)) {
                                soup.idx.extend([base + a, base + b, base + c]);
                            }
                        }
                    }
                }
                "polylist" => {
                    let counts = prim.children().find(|c| c.has_tag_name("vcount")).map(|v| ints(v.text().unwrap_or(""))).unwrap_or_default();
                    let p = prim.children().find(|c| c.has_tag_name("p")).map(|v| ints(v.text().unwrap_or(""))).unwrap_or_default();
                    let mut k = 0;
                    for n in counts {
                        let face: Vec<u32> = (0..n as usize).filter_map(|j| corner(&p, k + j)).collect();
                        if face.len() == n as usize {
                            soup.polygon(base, &face);
                        }
                        k += n as usize;
                    }
                }
                "polygons" => {
                    for p in prim.children().filter(|c| c.has_tag_name("p")) {
                        let p = ints(p.text().unwrap_or(""));
                        let face: Vec<u32> = (0..p.len() / stride).filter_map(|j| corner(&p, j)).collect();
                        soup.polygon(base, &face);
                    }
                }
                _ => {}
            }
        }
    }
    soup.finish(y_up)
}

// ---- LAS point clouds ------------------------------------------------------------------------------

/// Points as small tetrahedra (a point has no surface): at most this many are drawn (an even sample of the file).
const MAX_POINTS: usize = 250_000;

pub fn parse_las(b: &[u8]) -> Result<Mesh, String> {
    if b.get(..4) != Some(b"LASF") {
        return Err("not a LAS file (compressed LAZ files can't be read)".into());
    }
    let (minor, header) = (b[25], u16le(b, 94).unwrap_or(227) as usize);
    let _ = header;
    let offset = u32le(b, 96).ok_or("short file")? as usize;
    let format = b[104] & 0x3F;
    if b[104] & 0x80 != 0 {
        return Err("compressed LAZ point clouds can't be read; convert to LAS".into());
    }
    let rec = u16le(b, 105).ok_or("short file")? as usize;
    let mut n = u32le(b, 107).unwrap_or(0) as usize;
    if n == 0 && minor >= 4 {
        n = f64le(b, 247).map(|v| v as usize).unwrap_or(0);
    }
    if n == 0 || rec < 12 {
        return Err("no points in this file".into());
    }
    let n = n.min((b.len().saturating_sub(offset)) / rec);
    let (sx, sy, sz) = (f64le(b, 131).unwrap_or(1.0), f64le(b, 139).unwrap_or(1.0), f64le(b, 147).unwrap_or(1.0));
    let (ox, oy, oz) = (f64le(b, 155).unwrap_or(0.0), f64le(b, 163).unwrap_or(0.0), f64le(b, 171).unwrap_or(0.0));
    let colour_at = match format {
        2 | 3 | 5 => Some(if format == 2 { 20 } else { 28 }),
        7 | 8 | 10 => Some(30),
        _ => None,
    };
    let step = n.div_ceil(MAX_POINTS).max(1);
    let mut pts: Vec<([f32; 3], [u8; 4])> = Vec::with_capacity(n / step + 1);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for i in (0..n).step_by(step) {
        let o = offset + i * rec;
        let (Some(x), Some(y), Some(z)) = (u32le(b, o), u32le(b, o + 4), u32le(b, o + 8)) else { break };
        let p = [(x as i32 as f64 * sx + ox) as f32, (y as i32 as f64 * sy + oy) as f32, (z as i32 as f64 * sz + oz) as f32];
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
        let c = colour_at.and_then(|c| Some([u16le(b, o + c)?, u16le(b, o + c + 2)?, u16le(b, o + c + 4)?])).filter(|c| c.iter().any(|&v| v != 0)).map(|c| {
            // 16-bit colour (some writers store 8-bit values in it)
            let sh = if c.iter().all(|&v| v < 256) { 0 } else { 8 };
            [(c[0] >> sh) as u8, (c[1] >> sh) as u8, (c[2] >> sh) as u8, 255]
        });
        pts.push((p, c.unwrap_or([0, 0, 0, 0])));
    }
    if pts.is_empty() {
        return Err("no points could be read".into());
    }
    let diag = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt().max(1e-6);
    let r = diag * 0.0035;
    let mut soup = Soup::default();
    let zr = (hi[2] - lo[2]).max(1e-6);
    for (p, c) in &pts {
        let base = soup.base();
        soup.pos.extend([[p[0] + r, p[1], p[2] - r * 0.6], [p[0] - r * 0.5, p[1] + r * 0.87, p[2] - r * 0.6], [p[0] - r * 0.5, p[1] - r * 0.87, p[2] - r * 0.6], [p[0], p[1], p[2] + r * 1.4]]);
        soup.idx.extend([base, base + 1, base + 2, base, base + 3, base + 1, base + 1, base + 3, base + 2, base + 2, base + 3, base]);
        // Without colours the height is the colour: blue low, red high.
        let col = if c[3] != 0 {
            *c
        } else {
            let t = ((p[2] - lo[2]) / zr).clamp(0.0, 1.0);
            [(255.0 * (t * 2.0).min(1.0)) as u8, (255.0 * (1.0 - (t - 0.5).abs() * 2.0).max(0.0)) as u8, (255.0 * (1.0 - t * 2.0).max(0.0)) as u8, 255]
        };
        soup.colors.extend([col; 4]);
    }
    soup.finish(false)
}

// ---- DXF ---------------------------------------------------------------------------------------------

pub fn parse_dxf(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return Err("binary DXF isn't supported; save it as ASCII DXF".into());
    }
    let text = String::from_utf8_lossy(bytes);
    let mut it = text.lines();
    let mut pairs: Vec<(i32, String)> = Vec::new();
    while let (Some(c), Some(v)) = (it.next(), it.next()) {
        if let Ok(code) = c.trim().parse::<i32>() {
            pairs.push((code, v.trim().to_string()));
        }
    }
    let mut soup = Soup::default();
    let num = |s: &str| s.parse::<f32>().unwrap_or(0.0);
    let mut i = 0;
    while i < pairs.len() {
        if pairs[i].0 != 0 {
            i += 1;
            continue;
        }
        let kind = pairs[i].1.clone();
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        let body = &pairs[i + 1..j];
        match kind.as_str() {
            "3DFACE" | "SOLID" | "TRACE" => {
                let mut p = [[0.0f32; 3]; 4];
                for (c, v) in body {
                    let (k, axis) = match *c {
                        10..=13 => ((*c - 10) as usize, 0),
                        20..=23 => ((*c - 20) as usize, 1),
                        30..=33 => ((*c - 30) as usize, 2),
                        _ => continue,
                    };
                    p[k][axis] = num(v);
                }
                let base = soup.base();
                soup.pos.extend(p);
                // the 4th corner repeats the 3rd for a triangle
                if kind == "SOLID" || kind == "TRACE" {
                    soup.polygon(base, &[0, 1, 3, 2]); // SOLID corners are in "bow-tie" order
                } else if p[3] == p[2] {
                    soup.polygon(base, &[0, 1, 2]);
                } else {
                    soup.polygon(base, &[0, 1, 2, 3]);
                }
            }
            "MESH" => {
                // 92 = vertex count, then 10/20/30 per vertex; 93 = size of face list, then 90 = counts / indices
                let mut verts: Vec<[f32; 3]> = Vec::new();
                let mut faces: Vec<u32> = Vec::new();
                let mut in_faces = false;
                for (c, v) in body {
                    match *c {
                        10 => verts.push([num(v), 0.0, 0.0]),
                        20 => {
                            if let Some(l) = verts.last_mut() {
                                l[1] = num(v)
                            }
                        }
                        30 => {
                            if let Some(l) = verts.last_mut() {
                                l[2] = num(v)
                            }
                        }
                        93 => in_faces = true,
                        94 => in_faces = false,
                        90 if in_faces => faces.push(v.parse().unwrap_or(0)),
                        _ => {}
                    }
                }
                let base = soup.base();
                soup.pos.extend(verts.iter().copied());
                let mut k = 0;
                while k < faces.len() {
                    let n = faces[k] as usize;
                    if k + 1 + n > faces.len() {
                        break;
                    }
                    let f: Vec<u32> = faces[k + 1..k + 1 + n].iter().copied().filter(|&v| (v as usize) < verts.len()).collect();
                    if f.len() == n {
                        soup.polygon(base, &f);
                    }
                    k += 1 + n;
                }
            }
            "POLYLINE" => {
                let flags: i32 = body.iter().find(|(c, _)| *c == 70).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
                // vertices follow as VERTEX entities until SEQEND
                let mut verts: Vec<[f32; 3]> = Vec::new();
                let mut faces: Vec<Vec<u32>> = Vec::new();
                let (m, n) = (body.iter().find(|(c, _)| *c == 71).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0), body.iter().find(|(c, _)| *c == 72).and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0));
                let mut k = j;
                while k < pairs.len() && !(pairs[k].0 == 0 && pairs[k].1 == "SEQEND") {
                    if pairs[k].0 == 0 && pairs[k].1 == "VERTEX" {
                        let mut e = k + 1;
                        let (mut p, mut vflags, mut idx) = ([0.0f32; 3], 0, [0i32; 4]);
                        while e < pairs.len() && pairs[e].0 != 0 {
                            let (c, v) = (&pairs[e].0, &pairs[e].1);
                            match *c {
                                10 => p[0] = num(v),
                                20 => p[1] = num(v),
                                30 => p[2] = num(v),
                                70 => vflags = v.parse().unwrap_or(0),
                                71..=74 => idx[(*c - 71) as usize] = v.parse().unwrap_or(0),
                                _ => {}
                            }
                            e += 1;
                        }
                        if flags & 64 != 0 && vflags & 128 != 0 && vflags & 64 == 0 {
                            faces.push(idx.iter().filter(|&&v| v != 0).map(|&v| v.unsigned_abs() - 1).collect());
                        } else {
                            verts.push(p);
                        }
                        k = e;
                    } else {
                        k += 1;
                    }
                }
                let base = soup.base();
                if flags & 64 != 0 {
                    soup.pos.extend(verts.iter().copied());
                    for f in faces {
                        if f.iter().all(|&v| (v as usize) < verts.len()) {
                            soup.polygon(base, &f);
                        }
                    }
                } else if flags & 16 != 0 && m * n == verts.len() && m > 1 && n > 1 {
                    soup.pos.extend(verts.iter().copied());
                    for a in 0..m - 1 {
                        for b2 in 0..n - 1 {
                            let q = |x: usize, y: usize| (x * n + y) as u32;
                            soup.polygon(base, &[q(a, b2), q(a + 1, b2), q(a + 1, b2 + 1), q(a, b2 + 1)]);
                        }
                    }
                }
            }
            _ => {}
        }
        i = j;
    }
    soup.finish(false).map_err(|_| "no 3D faces in this DXF (it may hold only 2D drawing lines)".to_string())
}

// ---- LightWave ---------------------------------------------------------------------------------------

pub fn parse_lwo(b: &[u8]) -> Result<Mesh, String> {
    if b.get(..4) != Some(b"FORM") {
        return Err("not a LightWave object".into());
    }
    let kind = b.get(8..12).ok_or("short file")?;
    let v2 = kind == b"LWO2" || kind == b"LWO3";
    if !v2 && kind != b"LWOB" {
        return Err("unknown LightWave version".into());
    }
    let be32 = |o: usize| b.get(o..o + 4).map(|x| u32::from_be_bytes(x.try_into().unwrap()) as usize);
    let be16 = |o: usize| b.get(o..o + 2).map(|x| u16::from_be_bytes(x.try_into().unwrap()) as usize);
    let mut soup = Soup::default();
    let mut base = 0u32;
    let mut at = 12;
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = be32(at + 4).unwrap_or(0);
        let body = at + 8;
        let end = (body + len).min(b.len());
        match id {
            b"LAYR" => base = soup.base(),
            b"PNTS" => {
                for o in (body..end.saturating_sub(11)).step_by(12) {
                    if let (Some(x), Some(y), Some(z)) = (f32be(b, o), f32be(b, o + 4), f32be(b, o + 8)) {
                        soup.pos.push([x, y, z]);
                    }
                }
            }
            b"POLS" => {
                let mut p = body;
                if v2 {
                    if &b[p..(p + 4).min(b.len())] != b"FACE" {
                        at = body + len + (len & 1);
                        continue;
                    }
                    p += 4;
                }
                while p + 2 <= end {
                    let head = be16(p).unwrap_or(0);
                    p += 2;
                    let n = if v2 { head & 0x3FF } else { head };
                    let mut face = Vec::with_capacity(n);
                    for _ in 0..n {
                        if v2 {
                            if b.get(p) == Some(&0xFF) {
                                face.push(be32(p).unwrap_or(0) as u32 & 0x00FF_FFFF);
                                p += 4;
                            } else {
                                face.push(be16(p).unwrap_or(0) as u32);
                                p += 2;
                            }
                        } else {
                            face.push(be16(p).unwrap_or(0) as u32);
                            p += 2;
                        }
                    }
                    if !v2 {
                        p += 2; // surface number
                    }
                    if face.iter().all(|&i| base + i < soup.base()) {
                        soup.polygon(base, &face);
                    }
                }
            }
            _ => {}
        }
        at = body + len + (len & 1);
    }
    soup.finish(true)
}

// ---- USD (ASCII) and USDZ -----------------------------------------------------------------------------

fn numbers_in(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E') {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                if let Ok(v) = cur.parse::<f32>() {
                    out.push(v);
                }
                cur.clear();
            }
        }
    }
    if let Ok(v) = cur.parse::<f32>() {
        out.push(v);
    }
    out
}

/// The text of the bracketed array that follows `name = ` inside `block`.
fn usd_array<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    let p = block.find(&format!("{name} ="))?;
    let rest = &block[p..];
    let open = rest.find('[')?;
    let close = rest[open..].find(']')? + open;
    Some(&rest[open + 1..close])
}

pub fn parse_usda(text: &str) -> Result<Mesh, String> {
    if text.starts_with("PXR-USDC") {
        return Err("binary USD (usdc) needs a converter: install Blender or assimp".into());
    }
    let y_up = !text.contains("upAxis = \"Z\"");
    let mut soup = Soup::default();
    let mut from = 0;
    while let Some(p) = text[from..].find("def Mesh") {
        let start = from + p;
        // the block ends at the next "def " at the same or lower level: approximate with the next "def Mesh" / end
        let end = text[start + 8..].find("def Mesh").map(|e| start + 8 + e).unwrap_or(text.len());
        let block = &text[start..end];
        from = end;
        let (Some(pts), Some(counts), Some(indices)) = (usd_array(block, "points"), usd_array(block, "faceVertexCounts"), usd_array(block, "faceVertexIndices")) else { continue };
        let pts = numbers_in(pts);
        let counts: Vec<usize> = numbers_in(counts).into_iter().map(|v| v as usize).collect();
        let indices: Vec<u32> = numbers_in(indices).into_iter().map(|v| v as u32).collect();
        let base = soup.base();
        let n = pts.len() / 3;
        soup.pos.extend(pts.chunks_exact(3).map(|p| [p[0], p[1], p[2]]));
        let mut k = 0;
        for c in counts {
            if k + c > indices.len() {
                break;
            }
            let f = &indices[k..k + c];
            if f.iter().all(|&v| (v as usize) < n) {
                soup.polygon(base, f);
            }
            k += c;
        }
    }
    soup.finish(y_up)
}

pub fn parse_usdz(path: &Path) -> Result<Mesh, String> {
    let mut zip = crate::docthumb::Zip::open(path)?;
    let entries: Vec<_> = zip.entries.clone();
    for e in &entries {
        let n = e.name.to_ascii_lowercase();
        if n.ends_with(".usda") || n.ends_with(".usd") || n.ends_with(".usdc") {
            let data = zip.read(e, 512 << 20)?;
            if data.starts_with(b"PXR-USDC") {
                continue;
            }
            if let Ok(m) = parse_usda(&String::from_utf8_lossy(&data)) {
                return Ok(m);
            }
        }
    }
    Err("this USDZ holds binary USD (usdc): install Blender or assimp to open it, or export glTF / OBJ".into())
}

// ---- Maya ASCII -----------------------------------------------------------------------------------------

pub fn parse_ma(bytes: &[u8]) -> Result<Mesh, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut soup = Soup::default();
    let up_z = text.contains("-ut \"") && false;
    // Statements end with ';'.
    let mut verts: Vec<[f32; 3]> = Vec::new();
    let mut edges: Vec<(i64, i64)> = Vec::new();
    let mut faces: Vec<Vec<i64>> = Vec::new();
    let flush = |soup: &mut Soup, verts: &mut Vec<[f32; 3]>, edges: &mut Vec<(i64, i64)>, faces: &mut Vec<Vec<i64>>| {
        if !verts.is_empty() && !faces.is_empty() {
            let base = soup.base();
            soup.pos.extend(verts.iter().copied());
            for f in faces.iter() {
                let poly: Vec<u32> = f
                    .iter()
                    .filter_map(|&e| {
                        let (id, rev) = if e < 0 { ((-e - 1) as usize, true) } else { (e as usize, false) };
                        let (a, b) = *edges.get(id)?;
                        Some((if rev { b } else { a }) as u32)
                    })
                    .collect();
                if poly.len() == f.len() && poly.iter().all(|&v| (v as usize) < verts.len()) {
                    soup.polygon(base, &poly);
                }
            }
        }
        verts.clear();
        edges.clear();
        faces.clear();
    };
    for stmt in text.split(';') {
        let s = stmt.trim();
        if s.starts_with("createNode") {
            flush(&mut soup, &mut verts, &mut edges, &mut faces);
        } else if s.starts_with("setAttr") {
            if s.contains("\".vt[") {
                let nums = numbers_after_attr(s);
                verts.extend(nums.chunks_exact(3).map(|p| [p[0], p[1], p[2]]));
            } else if s.contains("\".ed[") {
                let nums = numbers_after_attr(s);
                edges.extend(nums.chunks_exact(3).map(|p| (p[0] as i64, p[1] as i64)));
            } else if s.contains("polyFaces") {
                // f <n> e1 .. en  (then mu / mc / h groups that we skip)
                let body = &s[s.find("polyFaces").unwrap() + 9..];
                let toks: Vec<&str> = body.split_ascii_whitespace().collect();
                let mut k = 0;
                while k < toks.len() {
                    match toks[k] {
                        "f" => {
                            let n: usize = toks.get(k + 1).and_then(|t| t.parse().ok()).unwrap_or(0);
                            let f: Vec<i64> = toks.iter().skip(k + 2).take(n).filter_map(|t| t.parse().ok()).collect();
                            if f.len() == n {
                                faces.push(f);
                            }
                            k += 2 + n;
                        }
                        "mu" | "mc" => {
                            let n: usize = toks.get(k + 2).and_then(|t| t.parse().ok()).unwrap_or(0);
                            k += 3 + n;
                        }
                        "h" => {
                            let n: usize = toks.get(k + 1).and_then(|t| t.parse().ok()).unwrap_or(0);
                            k += 2 + n;
                        }
                        _ => k += 1,
                    }
                }
            }
        }
    }
    flush(&mut soup, &mut verts, &mut edges, &mut faces);
    soup.finish(!up_z).map_err(|_| "no polygon meshes in this Maya ASCII file (NURBS and subdivision surfaces aren't read)".to_string())
}

/// The numbers after the attribute name of a `setAttr` line (skipping flags such as `-s 8` or `-type "..."`).
fn numbers_after_attr(s: &str) -> Vec<f32> {
    let Some(q) = s.find("\".") else { return Vec::new() };
    let after = &s[q..];
    let Some(e) = after[1..].find('"') else { return Vec::new() };
    after[e + 2..].split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect()
}

// ---- Unity packages ----------------------------------------------------------------------------------------

/// A `.unitypackage` is a gzipped tar: `<guid>/asset` (the file), `<guid>/pathname` (where it lives in the project).
/// The biggest model inside is opened; with none, the biggest `preview.png`.
pub fn parse_unitypackage(path: &Path) -> Result<crate::Image, String> {
    use std::io::Read;
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > 400 << 20 {
        return Err("this Unity package is too big to look into (over 400 MB)".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut gz = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    let mut assets: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut previews: Vec<Vec<u8>> = Vec::new();
    let model_ext = |n: &str| matches!(n.rsplit('.').next().map(str::to_ascii_lowercase).as_deref(), Some("fbx" | "obj" | "dae" | "gltf" | "glb" | "stl" | "ply" | "3ds" | "lwo" | "3mf"));
    let mut total = 0u64;
    loop {
        let mut head = [0u8; 512];
        if gz.read_exact(&mut head).is_err() || head.iter().all(|&b| b == 0) {
            break;
        }
        let name = String::from_utf8_lossy(&head[..100]).trim_end_matches('\0').to_string();
        let len = usize::from_str_radix(String::from_utf8_lossy(&head[124..135]).trim().trim_end_matches('\0'), 8).unwrap_or(0);
        let padded = len.div_ceil(512) * 512;
        total += padded as u64;
        if total > 1 << 30 {
            break;
        }
        let (guid, file) = name.trim_start_matches("./").split_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or_default();
        let keep = matches!(file.as_str(), "asset" | "pathname" | "preview.png") && len < (200 << 20);
        let mut data = vec![0u8; if keep { padded } else { 0 }];
        if keep {
            gz.read_exact(&mut data).map_err(|e| e.to_string())?;
            data.truncate(len);
        } else if std::io::copy(&mut gz.by_ref().take(padded as u64), &mut std::io::sink()).is_err() {
            break;
        }
        match file.as_str() {
            "pathname" if keep => {
                names.insert(guid, String::from_utf8_lossy(&data).lines().next().unwrap_or("").to_string());
            }
            "asset" if keep => {
                assets.insert(guid, data);
            }
            "preview.png" if keep => previews.push(data),
            _ => {}
        }
    }
    let best = assets.iter().filter(|(g, _)| names.get(*g).is_some_and(|n| model_ext(n))).max_by_key(|(_, d)| d.len());
    if let Some((g, data)) = best {
        let name = names.get(g).cloned().unwrap_or_default();
        let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mesh = match ext.as_str() {
            "fbx" => super::parse_fbx(data, None)?,
            "obj" => super::parse_obj(data, None)?,
            "dae" => parse_dae(data)?,
            "gltf" | "glb" => super::parse_gltf(data, None)?,
            "stl" => super::parse_stl(data)?,
            "ply" => super::parse_ply(data, None)?,
            "3ds" => parse_3ds(data)?,
            "lwo" => parse_lwo(data)?,
            other => return Err(format!("the model inside is .{other}, which can't be opened from a package")),
        };
        let mut img = super::preview_image(mesh, false);
        img.colour_note = Some(format!("3D model inside the Unity package: {name}"));
        return Ok(img);
    }
    if let Some(p) = previews.iter().max_by_key(|p| p.len()) {
        let mut img = crate::decode_image_bytes(p, "png")?;
        img.colour_note = Some("preview picture stored in the Unity package (no 3D model inside)".into());
        return Ok(img);
    }
    Err("no 3D model or preview picture inside this Unity package".into())
}

// ---- pictures stored inside closed scene files -----------------------------------------------------------------

/// The biggest PNG or JPEG stored inside the file (scene files keep a thumbnail): found by their signatures.
pub fn embedded_picture(bytes: &[u8]) -> Option<crate::Image> {
    let mut best: Option<crate::Image> = None;
    let mut consider = |img: crate::Image| {
        if img.width >= 48 && img.height >= 48 && best.as_ref().is_none_or(|b| img.width as u64 * img.height as u64 > b.width as u64 * b.height as u64) {
            best = Some(img);
        }
    };
    // PNG: signature .. IEND chunk
    let sig = b"\x89PNG\r\n\x1a\n";
    let mut from = 0;
    while let Some(p) = find(bytes, sig, from) {
        let end = find(bytes, b"IEND", p).map(|e| (e + 8).min(bytes.len())).unwrap_or(bytes.len());
        if let Ok(img) = crate::decode_image_bytes(&bytes[p..end], "png") {
            consider(img);
        }
        from = p + 8;
    }
    // JPEG: FF D8 FF .. up to the next SOI or the end
    let mut from = 0;
    let mut count = 0;
    while let Some(p) = find(bytes, &[0xFF, 0xD8, 0xFF], from) {
        let next = find(bytes, &[0xFF, 0xD8, 0xFF], p + 3).unwrap_or(bytes.len());
        if let Ok(img) = crate::decode_jpeg(&bytes[p..next]) {
            consider(img);
        }
        from = p + 3;
        count += 1;
        if count > 64 {
            break;
        }
    }
    best
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

// ---- converters ------------------------------------------------------------------------------------------------

pub mod external {
    //! STEP / IGES / Alembic / binary USD / DWG through a tool that is installed: assimp, Blender, FreeCAD or the ODA File
    //! Converter. The result (glb, stl or dxf) is cached in `%LOCALAPPDATA%\NuViewer\model-cache`.
    use super::*;
    use std::process::{Command, Stdio};

    fn first_file(places: &[PathBuf]) -> Option<PathBuf> {
        places.iter().find(|p| p.is_file()).cloned()
    }

    fn on_path(name: &str) -> Option<PathBuf> {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|p| p.is_file()))
    }

    fn program_files(sub: &str, exe: &str) -> Vec<PathBuf> {
        let mut v = Vec::new();
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
                let root = if var == "LOCALAPPDATA" { base.join("Programs") } else { base };
                if let Ok(rd) = std::fs::read_dir(&root) {
                    for e in rd.flatten() {
                        let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                        if n.contains(&sub.to_ascii_lowercase()) {
                            for d in ["", "bin", "bin/"] {
                                v.push(e.path().join(d).join(exe));
                            }
                            // Blender keeps a versioned folder inside "Blender Foundation"
                            if let Ok(inner) = std::fs::read_dir(e.path()) {
                                for i in inner.flatten() {
                                    v.push(i.path().join(exe));
                                    v.push(i.path().join("bin").join(exe));
                                }
                            }
                        }
                    }
                }
            }
        }
        v
    }

    pub fn assimp() -> Option<PathBuf> {
        on_path("assimp.exe").or_else(|| first_file(&program_files("assimp", "assimp.exe")))
    }
    pub fn blender() -> Option<PathBuf> {
        on_path("blender.exe").or_else(|| first_file(&program_files("blender", "blender.exe")))
    }
    pub fn freecad() -> Option<PathBuf> {
        on_path("FreeCADCmd.exe").or_else(|| first_file(&program_files("freecad", "FreeCADCmd.exe")))
    }
    pub fn oda() -> Option<PathBuf> {
        on_path("ODAFileConverter.exe").or_else(|| first_file(&program_files("oda", "ODAFileConverter.exe")))
    }

    fn cache_file(src: &Path, ext: &str) -> PathBuf {
        let meta = std::fs::metadata(src).ok();
        let (size, mtime) = meta.map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))).unwrap_or((0, 0));
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for b in src.to_string_lossy().to_lowercase().bytes().chain(size.to_le_bytes()).chain(mtime.to_le_bytes()) {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01B3);
        }
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("NuViewer").join("model-cache").join(format!("{h:016x}.{ext}"))
    }

    fn run(mut c: Command) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).creation_flags(0x0800_0000);
        let mut child = c.spawn().map_err(|e| e.to_string())?;
        let start = std::time::Instant::now();
        loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(s) if s.success() => return Ok(()),
                Some(s) => return Err(format!("the converter stopped with {s}")),
                None => {
                    if start.elapsed() > std::time::Duration::from_secs(90) {
                        let _ = child.kill();
                        return Err("the converter took longer than 90 seconds".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }
    }

    /// Convert `src` to something we read natively (returns the file and its extension). Err says what to install.
    pub fn convert(src: &Path, ext: &str) -> Result<(PathBuf, &'static str), String> {
        let out_dir = cache_file(src, "x").parent().map(Path::to_path_buf).unwrap_or_default();
        let _ = std::fs::create_dir_all(&out_dir);
        let try_assimp = |src: &Path| -> Option<Result<(PathBuf, &'static str), String>> {
            let a = assimp()?;
            let dst = cache_file(src, "glb");
            if dst.is_file() {
                return Some(Ok((dst, "glb")));
            }
            let mut c = Command::new(a);
            c.arg("export").arg(src).arg(&dst).arg("-fglb2");
            Some(run(c).and_then(|_| if dst.is_file() { Ok((dst, "glb")) } else { Err("assimp wrote nothing".into()) }))
        };
        match ext {
            "step" | "stp" | "iges" | "igs" => {
                let dst = cache_file(src, "stl");
                if dst.is_file() {
                    return Ok((dst, "stl"));
                }
                if let Some(f) = freecad() {
                    let script = format!(
                        "import FreeCAD, Part, Mesh\ns = Part.read(r'{}')\nm = Mesh.Mesh(s.tessellate(0.1))\nm.write(r'{}')\n",
                        src.display(),
                        dst.display()
                    );
                    let sp = out_dir.join("convert.py");
                    std::fs::write(&sp, script).map_err(|e| e.to_string())?;
                    let mut c = Command::new(f);
                    c.arg("-c").arg(format!("exec(open(r'{}').read())", sp.display()));
                    run(c)?;
                    if dst.is_file() {
                        return Ok((dst, "stl"));
                    }
                }
                if let Some(r) = try_assimp(src) {
                    return r;
                }
                Err("STEP / IGES are CAD solids: install FreeCAD (or assimp) and NuViewer will convert them, or export the part as STL / OBJ / glTF".into())
            }
            "abc" | "usd" | "usdc" | "usdz" => {
                let dst = cache_file(src, "glb");
                if dst.is_file() {
                    return Ok((dst, "glb"));
                }
                if let Some(b) = blender() {
                    let op = if ext == "abc" { "bpy.ops.wm.alembic_import(filepath=r'%IN%')" } else { "bpy.ops.wm.usd_import(filepath=r'%IN%')" };
                    let code = format!("import bpy\nbpy.ops.wm.read_factory_settings(use_empty=True)\n{}\nbpy.ops.export_scene.gltf(filepath=r'{}', export_format='GLB')\n", op.replace("%IN%", &src.display().to_string()), dst.display());
                    let sp = out_dir.join("convert_blender.py");
                    std::fs::write(&sp, code).map_err(|e| e.to_string())?;
                    let mut c = Command::new(b);
                    c.args(["-b", "-noaudio", "--python"]).arg(&sp);
                    run(c)?;
                    if dst.is_file() {
                        return Ok((dst, "glb"));
                    }
                }
                if let Some(r) = try_assimp(src) {
                    return r;
                }
                Err("Alembic and binary USD need Blender (or assimp) installed: NuViewer converts through it; or export glTF / OBJ".into())
            }
            "dwg" => {
                let o = oda().ok_or("DWG is Autodesk's closed format: install the free ODA File Converter and NuViewer converts it to DXF, or save the drawing as DXF from AutoCAD")?;
                let in_dir = src.parent().ok_or("no folder")?;
                let work = out_dir.join(format!("dwg-{}", std::process::id()));
                let _ = std::fs::create_dir_all(&work);
                let mut c = Command::new(o);
                c.arg(in_dir).arg(&work).args(["ACAD2013", "DXF", "0", "1"]).arg(src.file_name().unwrap_or_default());
                run(c)?;
                let stem = src.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let made = work.join(format!("{stem}.dxf"));
                let dst = cache_file(src, "dxf");
                std::fs::copy(&made, &dst).map_err(|_| "the ODA File Converter made no DXF".to_string())?;
                let _ = std::fs::remove_dir_all(&work);
                Ok((dst, "dxf"))
            }
            other => Err(format!("no converter for .{other}")),
        }
    }
}

/// The whole job for one file of these formats: a mesh image (through `model::preview_image`) or a stored picture.
pub fn decode(path: &Path, ext: &str, thumb: bool) -> Result<crate::Image, String> {
    if ext == "unitypackage" {
        return parse_unitypackage(path);
    }
    if let Some(app) = closed_app(ext) {
        let bytes = crate::progress::read_file(path)?;
        // 3ds Max keeps a small preview in the OLE summary information; other scene formats a PNG / JPEG somewhere inside.
        if let Some(mut img) = crate::ole::summary_preview(&bytes) {
            img.colour_note = Some(format!("the small preview stored in this {app} file (the 3D scene itself is {app}'s closed format)"));
            return Ok(img);
        }
        if let Some(mut img) = embedded_picture(&bytes) {
            img.colour_note = Some(format!("the preview picture stored in this {app} file (the 3D scene itself is {app}'s closed format)"));
            return Ok(img);
        }
        return Err(format!("{app} scene (.{ext}): its format is closed and this file stores no preview picture. Export the scene as glTF, FBX or OBJ to look at the model here"));
    }
    if CONVERT.contains(&ext) {
        let (file, kind) = external::convert(path, ext)?;
        return match kind {
            "glb" => Ok(super::preview_image(super::parse_gltf(&std::fs::read(&file).map_err(|e| e.to_string())?, None)?, thumb)),
            "stl" => Ok(super::preview_image(super::parse_stl(&std::fs::read(&file).map_err(|e| e.to_string())?)?, thumb)),
            _ => Ok(super::preview_image(parse_dxf(&std::fs::read(&file).map_err(|e| e.to_string())?)?, thumb)),
        };
    }
    let bytes = crate::progress::read_file(path)?;
    let mesh = match ext {
        "3ds" => parse_3ds(&bytes)?,
        "3mf" => match parse_3mf(path) {
            Ok(m) => m,
            // A slicer project whose models can't be drawn still carries the picture of its plate.
            Err(e) => return parse_3mf_picture(path).ok_or(e),
        },
        "amf" => parse_amf(path, &bytes)?,
        "dae" => parse_dae(&bytes)?,
        "las" => parse_las(&bytes)?,
        // A drawing without faces (plans, clipart: lines and curves) is drawn flat instead.
        "dxf" => match parse_dxf(&bytes) {
            Ok(m) => m,
            Err(e) => return crate::vector2d::dxf_2d(&bytes).and_then(|d| d.render(thumb)).map_err(|_| e),
        },
        "lwo" | "lwo2" => parse_lwo(&bytes)?,
        "usda" => parse_usda(&String::from_utf8_lossy(&bytes))?,
        "usdz" => parse_usdz(path)?,
        "ma" => parse_ma(&bytes)?,
        "wrl" | "vrml" | "wrz" => super::vrml::parse(&bytes)?,
        _ => return Err(format!("unsupported 3D format: .{ext}")),
    };
    Ok(super::preview_image(mesh, thumb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_mesh(m: &Mesh, tris: usize) {
        assert_eq!(m.indices.len(), tris * 3);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
    }

    #[test]
    fn a_3ds_file_with_one_triangle() {
        let mut v = Vec::new();
        let chunk = |id: u16, body: &[u8]| {
            let mut c = id.to_le_bytes().to_vec();
            c.extend(((body.len() + 6) as u32).to_le_bytes());
            c.extend(body);
            c
        };
        let mut verts = 3u16.to_le_bytes().to_vec();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in p {
                verts.extend(c.to_le_bytes());
            }
        }
        let mut faces = 1u16.to_le_bytes().to_vec();
        for i in [0u16, 1, 2, 0] {
            faces.extend(i.to_le_bytes());
        }
        let mesh_body: Vec<u8> = [chunk(0x4110, &verts), chunk(0x4120, &faces)].concat();
        let mut obj = b"Box\0".to_vec();
        obj.extend(chunk(0x4100, &mesh_body));
        let editor = chunk(0x4000, &obj);
        v.extend(chunk(0x4D4D, &chunk(0x3D3D, &editor)));
        tri_mesh(&parse_3ds(&v).unwrap(), 1);
    }

    #[test]
    fn collada_triangles_and_polylist_with_z_up() {
        let dae = r##"<COLLADA><asset><up_axis>Z_UP</up_axis></asset><library_geometries><geometry id="g"><mesh>
            <source id="p"><float_array id="pa" count="12">0 0 0 1 0 0 1 1 0 0 1 0</float_array><technique_common><accessor source="#pa" count="4" stride="3"/></technique_common></source>
            <vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
            <polylist count="1"><input semantic="VERTEX" source="#v" offset="0"/><vcount>4</vcount><p>0 1 2 3</p></polylist>
            </mesh></geometry></library_geometries></COLLADA>"##;
        let m = parse_dae(dae.as_bytes()).unwrap();
        tri_mesh(&m, 2);
        assert!(!m.y_up);
    }

    #[test]
    fn dxf_3dface_and_polyface() {
        let dxf = "0\nSECTION\n2\nENTITIES\n0\n3DFACE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n0\n31\n0\n12\n1\n22\n1\n32\n0\n13\n0\n23\n1\n33\n0\n0\nENDSEC\n0\nEOF\n";
        tri_mesh(&parse_dxf(dxf.as_bytes()).unwrap(), 2);
        let poly = "0\nPOLYLINE\n70\n64\n71\n3\n72\n1\n0\nVERTEX\n10\n0\n20\n0\n30\n0\n70\n192\n0\nVERTEX\n10\n1\n20\n0\n30\n0\n70\n192\n0\nVERTEX\n10\n0\n20\n1\n30\n0\n70\n192\n0\nVERTEX\n70\n128\n71\n1\n72\n2\n73\n3\n0\nSEQEND\n";
        tri_mesh(&parse_dxf(poly.as_bytes()).unwrap(), 1);
    }

    #[test]
    fn usda_mesh_and_lwo2_and_maya() {
        let usda = "#usda 1.0\n(upAxis = \"Z\")\ndef Mesh \"m\" {\n int[] faceVertexCounts = [3, 3]\n int[] faceVertexIndices = [0, 1, 2, 0, 2, 3]\n point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]\n}\n";
        let m = parse_usda(usda).unwrap();
        tri_mesh(&m, 2);
        assert!(!m.y_up);

        // LWO2: FORM, PNTS (3 points), POLS FACE (one triangle)
        let mut pnts = Vec::new();
        for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
            for c in p {
                pnts.extend(c.to_be_bytes());
            }
        }
        let mut pols = b"FACE".to_vec();
        pols.extend(3u16.to_be_bytes());
        for i in [0u16, 1, 2] {
            pols.extend(i.to_be_bytes());
        }
        let ck = |id: &[u8; 4], body: &[u8]| {
            let mut c = id.to_vec();
            c.extend((body.len() as u32).to_be_bytes());
            c.extend(body);
            if body.len() % 2 == 1 {
                c.push(0);
            }
            c
        };
        let mut form = b"LWO2".to_vec();
        form.extend(ck(b"PNTS", &pnts));
        form.extend(ck(b"POLS", &pols));
        let mut file = b"FORM".to_vec();
        file.extend((form.len() as u32).to_be_bytes());
        file.extend(form);
        tri_mesh(&parse_lwo(&file).unwrap(), 1);

        let ma = "createNode mesh -n \"s\";\n setAttr -s 4 \".vt[0:3]\"  0 0 0 1 0 0 1 1 0 0 1 0;\n setAttr -s 4 \".ed[0:3]\"  0 1 0 1 2 0 2 3 0 3 0 0;\n setAttr -s 1 \".fc[0]\" -type \"polyFaces\" f 4 0 1 2 3 mu 0 4 0 1 2 3;\n";
        tri_mesh(&parse_ma(ma.as_bytes()).unwrap(), 2);
    }

    #[test]
    fn las_points_become_small_solids_coloured_by_height() {
        let mut b = vec![0u8; 227];
        b[..4].copy_from_slice(b"LASF");
        b[24] = 1;
        b[25] = 2;
        b[94..96].copy_from_slice(&227u16.to_le_bytes());
        b[96..100].copy_from_slice(&227u32.to_le_bytes());
        b[104] = 0;
        b[105..107].copy_from_slice(&20u16.to_le_bytes());
        b[107..111].copy_from_slice(&3u32.to_le_bytes());
        for (o, v) in [(131, 0.01f64), (139, 0.01), (147, 0.01), (155, 0.0), (163, 0.0), (171, 0.0)] {
            b[o..o + 8].copy_from_slice(&v.to_le_bytes());
        }
        for p in [[0i32, 0, 0], [1000, 0, 500], [0, 1000, 1000]] {
            let mut rec = vec![0u8; 20];
            for (i, c) in p.iter().enumerate() {
                rec[i * 4..i * 4 + 4].copy_from_slice(&c.to_le_bytes());
            }
            b.extend(rec);
        }
        let m = parse_las(&b).unwrap();
        tri_mesh(&m, 12);
        assert_eq!(m.colors.len(), m.positions.len());
        assert!(m.colors[0][2] > m.colors[0][0], "lowest point is blue-ish");
    }

    #[test]
    fn closed_formats_are_named_and_a_stored_picture_is_found() {
        assert_eq!(closed_app("c4d"), Some("Cinema 4D"));
        assert!(all_exts().contains(&"unitypackage") && all_exts().contains(&"step") && all_exts().contains(&"spp"));
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 64, 64);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&vec![200u8; 64 * 64 * 3]).unwrap();
        }
        let mut file = vec![0u8; 100];
        file.extend(&png);
        file.extend(vec![1u8; 50]);
        let img = embedded_picture(&file).unwrap();
        assert_eq!((img.width, img.height), (64, 64));
    }
}
