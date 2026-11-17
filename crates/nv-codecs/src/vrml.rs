//! VRML (`.wrl`, `.vrml`, gzip-packed `.wrz`): VRML 97 (version 2) and the older VRML 1.0. The text is read into a tree of nodes
//! (DEF / USE shared), then walked with the transforms of Transform / Separator nodes: IndexedFaceSet, ElevationGrid, Box, Sphere,
//! Cylinder, Cone and VRML 1's Cube become triangles; every Shape's diffuse colour becomes the colour of its part of the model.
//! Textures, lights, animation and scripts are ignored.

use super::{Mesh, Part};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug)]
enum Value {
    Nums(Vec<f32>),
    Words(#[allow(dead_code)] Vec<String>),
    Nodes(Vec<Rc<Node>>),
}

#[derive(Debug, Default)]
struct Node {
    kind: String,
    fields: Vec<(String, Value)>,
}

impl Node {
    fn nums(&self, name: &str) -> Option<&[f32]> {
        self.fields.iter().find(|(n, _)| n == name).and_then(|(_, v)| if let Value::Nums(n) = v { Some(n.as_slice()) } else { None })
    }
    fn nodes(&self, name: &str) -> Vec<Rc<Node>> {
        self.fields.iter().filter(|(n, _)| n == name).flat_map(|(_, v)| if let Value::Nodes(n) = v { n.clone() } else { Vec::new() }).collect()
    }
    fn num(&self, name: &str, default: f32) -> f32 {
        self.nums(name).and_then(|n| n.first().copied()).unwrap_or(default)
    }
}

struct Parser {
    toks: Vec<String>,
    at: usize,
    defs: HashMap<String, Rc<Node>>,
    budget: usize,
}

fn tokenize(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = src.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_whitespace() || c == ',' || c == '\u{feff}' {
            it.next();
        } else if c == '#' {
            while let Some(&c) = it.peek() {
                if c == '\n' || c == '\r' {
                    break;
                }
                it.next();
            }
        } else if matches!(c, '{' | '}' | '[' | ']') {
            out.push(c.to_string());
            it.next();
        } else if c == '"' {
            it.next();
            let mut s = String::from("\"");
            while let Some(c) = it.next() {
                if c == '\\' {
                    if let Some(n) = it.next() {
                        s.push(n);
                    }
                } else if c == '"' {
                    break;
                } else {
                    s.push(c);
                }
            }
            out.push(s);
        } else {
            let mut s = String::new();
            while let Some(&c) = it.peek() {
                if c.is_whitespace() || matches!(c, ',' | '{' | '}' | '[' | ']' | '#' | '"') {
                    break;
                }
                s.push(c);
                it.next();
            }
            out.push(s);
        }
    }
    out
}

fn is_num(t: &str) -> bool {
    t.parse::<f32>().is_ok() && !t.eq_ignore_ascii_case("nan") && !t.eq_ignore_ascii_case("inf") && !t.eq_ignore_ascii_case("infinity")
}

impl Parser {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.at).map(String::as_str)
    }
    fn next(&mut self) -> Option<String> {
        let t = self.toks.get(self.at).cloned();
        self.at += 1;
        t
    }
    /// Is the next thing a node (`Type {`, `DEF name Type {`, `USE name`)?
    fn at_node(&self) -> bool {
        match self.peek() {
            Some("DEF") | Some("USE") => true,
            Some(t) if !t.starts_with('"') && t != "{" && t != "}" && t != "[" && t != "]" => self.toks.get(self.at + 1).map(String::as_str) == Some("{"),
            _ => false,
        }
    }

    fn node(&mut self) -> Result<Option<Rc<Node>>, String> {
        self.budget = self.budget.checked_sub(1).ok_or("vrml: too many nodes")?;
        let t = self.next().ok_or("vrml: unexpected end")?;
        let mut def = None;
        let mut kind = t.clone();
        if t == "DEF" {
            def = self.next();
            kind = self.next().ok_or("vrml: unexpected end")?;
        } else if t == "USE" {
            let name = self.next().ok_or("vrml: unexpected end")?;
            return Ok(self.defs.get(&name).cloned());
        }
        if kind == "NULL" {
            return Ok(None);
        }
        if self.next().as_deref() != Some("{") {
            return Err(format!("vrml: expected '{{' after {kind}"));
        }
        let mut node = Node { kind, fields: Vec::new() };
        loop {
            match self.peek() {
                None => return Err("vrml: a node is not closed".into()),
                Some("}") => {
                    self.next();
                    break;
                }
                Some(_) => {}
            }
            // VRML 1 bodies are lists of nodes, not fields: `Type {` here is a child (a field name is never followed by a brace).
            if self.at_node() {
                if let Some(n) = self.node()? {
                    node.fields.push(("_child".to_string(), Value::Nodes(vec![n])));
                }
                continue;
            }
            let name = self.next().unwrap();
            // Scripts and prototypes carry code and declarations this reader does not interpret: skip their body.
            if matches!(name.as_str(), "eventIn" | "eventOut" | "field" | "exposedField") {
                self.next();
                self.next();
                if self.at_value_start() {
                    self.skip_value();
                }
                continue;
            }
            let v = self.value()?;
            node.fields.push((name, v));
        }
        let node = Rc::new(node);
        if let Some(d) = def {
            self.defs.insert(d, node.clone());
        }
        Ok(Some(node))
    }

    fn at_value_start(&self) -> bool {
        matches!(self.peek(), Some(t) if t != "}")
    }

    fn skip_value(&mut self) {
        if self.peek() == Some("[") {
            let mut depth = 0;
            while let Some(t) = self.next() {
                match t.as_str() {
                    "[" | "{" => depth += 1,
                    "]" | "}" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        } else if self.at_node() {
            let _ = self.node();
        } else {
            self.next();
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        if self.peek() == Some("[") {
            self.next();
            let mut nums = Vec::new();
            let mut words = Vec::new();
            let mut nodes = Vec::new();
            loop {
                match self.peek() {
                    None => return Err("vrml: a list is not closed".into()),
                    Some("]") => {
                        self.next();
                        break;
                    }
                    Some(t) if is_num(t) => {
                        nums.push(t.parse::<f32>().unwrap());
                        self.next();
                    }
                    Some(_) if self.at_node() => {
                        if let Some(n) = self.node()? {
                            nodes.push(n);
                        }
                    }
                    Some(_) => words.push(self.next().unwrap()),
                }
            }
            return Ok(if !nodes.is_empty() {
                Value::Nodes(nodes)
            } else if !nums.is_empty() || words.is_empty() {
                Value::Nums(nums)
            } else {
                Value::Words(words)
            });
        }
        if self.at_node() {
            return Ok(Value::Nodes(self.node()?.into_iter().collect()));
        }
        if self.peek().is_some_and(is_num) {
            let mut nums = Vec::new();
            while let Some(t) = self.peek() {
                if !is_num(t) {
                    break;
                }
                nums.push(t.parse::<f32>().unwrap());
                self.next();
            }
            return Ok(Value::Nums(nums));
        }
        Ok(Value::Words(self.next().into_iter().collect()))
    }
}

// ---- transforms ---------------------------------------------------------------------------------------------------------------

type M = [[f32; 4]; 4]; // row-vector convention: p' = p * M

const ID: M = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];

fn mul(a: &M, b: &M) -> M {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

fn translate(t: [f32; 3]) -> M {
    let mut m = ID;
    m[3][..3].copy_from_slice(&t);
    m
}

fn scale(s: [f32; 3]) -> M {
    let mut m = ID;
    for i in 0..3 {
        m[i][i] = s[i];
    }
    m
}

fn rotation(axis: [f32; 3], angle: f32) -> M {
    let l = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if l < 1e-9 || angle == 0.0 {
        return ID;
    }
    let (x, y, z) = (axis[0] / l, axis[1] / l, axis[2] / l);
    let (c, s) = (angle.cos(), angle.sin());
    let t = 1.0 - c;
    // Rodrigues' formula, transposed for row vectors.
    [[t * x * x + c, t * x * y + s * z, t * x * z - s * y, 0.], [t * x * y - s * z, t * y * y + c, t * y * z + s * x, 0.], [t * x * z + s * y, t * y * z - s * x, t * z * z + c, 0.], [0., 0., 0., 1.]]
}

fn apply(m: &M, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|j| p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j])
}

/// VRML 97 Transform: P' = T * C * R * SR * S * -SR * -C * P (column vectors); here composed for row vectors, innermost first.
fn transform_matrix(n: &Node) -> M {
    let v3 = |name: &str, d: [f32; 3]| n.nums(name).filter(|v| v.len() >= 3).map(|v| [v[0], v[1], v[2]]).unwrap_or(d);
    let rot = |name: &str| n.nums(name).filter(|v| v.len() >= 4).map(|v| rotation([v[0], v[1], v[2]], v[3])).unwrap_or(ID);
    let c = v3("center", [0.0; 3]);
    let neg = [-c[0], -c[1], -c[2]];
    let sr = rot("scaleOrientation");
    let mut inv_sr = sr;
    for i in 0..3 {
        for j in 0..3 {
            inv_sr[i][j] = sr[j][i];
        }
    }
    let mut m = translate(neg);
    m = mul(&m, &inv_sr);
    m = mul(&m, &scale(v3("scale", [1.0; 3])));
    m = mul(&m, &sr);
    m = mul(&m, &rot("rotation"));
    m = mul(&m, &translate(c));
    mul(&m, &translate(v3("translation", [0.0; 3])))
}

// ---- geometry -----------------------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Build {
    pos: Vec<[f32; 3]>,
    idx: Vec<u32>,
    parts: Vec<Part>,
}

impl Build {
    fn add(&mut self, pos: Vec<[f32; 3]>, tris: Vec<[u32; 3]>, m: &M, color: [f32; 3]) {
        let base = self.pos.len() as u32;
        let first = self.idx.len() as u32;
        self.pos.extend(pos.iter().map(|p| apply(m, *p)));
        for t in tris {
            self.idx.extend(t.map(|i| base + i));
        }
        let count = self.idx.len() as u32 - first;
        if count > 0 {
            self.parts.push(Part { first, count, texture: None, color, alpha_cut: false, normal_map: None });
        }
    }
}

fn faces(coord_index: &[f32], vertex_count: usize) -> Vec<[u32; 3]> {
    let mut tris = Vec::new();
    let mut poly: Vec<u32> = Vec::new();
    let flush = |poly: &mut Vec<u32>, tris: &mut Vec<[u32; 3]>| {
        for k in 1..poly.len().saturating_sub(1) {
            tris.push([poly[0], poly[k], poly[k + 1]]);
        }
        poly.clear();
    };
    for &v in coord_index {
        if v < 0.0 {
            flush(&mut poly, &mut tris);
        } else if (v as usize) < vertex_count {
            poly.push(v as u32);
        } else {
            poly.clear(); // a face that points outside the point list is dropped
        }
    }
    flush(&mut poly, &mut tris);
    tris
}

fn cuboid(sx: f32, sy: f32, sz: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let (x, y, z) = (sx / 2.0, sy / 2.0, sz / 2.0);
    let p = vec![[-x, -y, -z], [x, -y, -z], [x, y, -z], [-x, y, -z], [-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]];
    let q = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
    let mut t = Vec::new();
    for f in q {
        t.push([f[0], f[1], f[2]]);
        t.push([f[0], f[2], f[3]]);
    }
    (p, t)
}

fn sphere(r: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let (nu, nv) = (24u32, 14u32);
    let mut p = Vec::new();
    for j in 0..=nv {
        let th = std::f32::consts::PI * j as f32 / nv as f32;
        for i in 0..nu {
            let ph = 2.0 * std::f32::consts::PI * i as f32 / nu as f32;
            p.push([r * th.sin() * ph.cos(), r * th.cos(), r * th.sin() * ph.sin()]);
        }
    }
    let mut t = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            let (a, b) = (j * nu + i, j * nu + (i + 1) % nu);
            t.push([a, a + nu, b]);
            t.push([b, a + nu, b + nu]);
        }
    }
    (p, t)
}

/// A cone (`top_r` = 0) or cylinder along Y, with caps.
fn lathe(bottom_r: f32, top_r: f32, h: f32) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let n = 24u32;
    let mut p = Vec::new();
    for i in 0..n {
        let a = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
        p.push([bottom_r * a.cos(), -h / 2.0, bottom_r * a.sin()]);
    }
    for i in 0..n {
        let a = 2.0 * std::f32::consts::PI * i as f32 / n as f32;
        p.push([top_r * a.cos(), h / 2.0, top_r * a.sin()]);
    }
    let (cb, ct) = (p.len() as u32, p.len() as u32 + 1);
    p.push([0.0, -h / 2.0, 0.0]);
    p.push([0.0, h / 2.0, 0.0]);
    let mut t = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        t.push([i, j, n + i]);
        t.push([j, n + j, n + i]);
        t.push([cb, j, i]);
        t.push([ct, n + i, n + j]);
    }
    (p, t)
}

fn colour_of(shape: &Node) -> [f32; 3] {
    for a in shape.nodes("appearance") {
        for m in a.nodes("material") {
            if let Some(c) = m.nums("diffuseColor").filter(|c| c.len() >= 3) {
                return [c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)];
            }
        }
    }
    [0.72, 0.74, 0.78]
}

struct Walk {
    b: Build,
    v1: bool,
    /// VRML 1: the current coordinate list, colour.
    v1_coords: Vec<[f32; 3]>,
    v1_colour: [f32; 3],
}

impl Walk {
    fn geometry(&mut self, g: &Node, m: &M, colour: [f32; 3]) {
        if self.b.pos.len() > 4_000_000 {
            return;
        }
        match g.kind.as_str() {
            "IndexedFaceSet" | "IndexedTriangleSet" => {
                let coords: Vec<[f32; 3]> = if self.v1 {
                    self.v1_coords.clone()
                } else {
                    g.nodes("coord").iter().filter_map(|c| c.nums("point")).flat_map(|p| p.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>()).collect()
                };
                if let Some(ci) = g.nums("coordIndex").or_else(|| g.nums("index")) {
                    let tris = if g.kind == "IndexedTriangleSet" { ci.chunks_exact(3).map(|c| [c[0] as u32, c[1] as u32, c[2] as u32]).filter(|t| t.iter().all(|&i| (i as usize) < coords.len())).collect() } else { faces(ci, coords.len()) };
                    self.b.add(coords, tris, m, colour);
                }
            }
            "ElevationGrid" => {
                let (nx, nz) = (g.num("xDimension", 0.0) as usize, g.num("zDimension", 0.0) as usize);
                let (sx, sz) = (g.num("xSpacing", 1.0), g.num("zSpacing", 1.0));
                if let Some(h) = g.nums("height").filter(|h| nx >= 2 && nz >= 2 && h.len() >= nx * nz) {
                    let mut p = Vec::new();
                    for z in 0..nz {
                        for x in 0..nx {
                            p.push([x as f32 * sx, h[z * nx + x], z as f32 * sz]);
                        }
                    }
                    let mut t = Vec::new();
                    for z in 0..nz - 1 {
                        for x in 0..nx - 1 {
                            let a = (z * nx + x) as u32;
                            let (b, c, d) = (a + 1, a + nx as u32, a + nx as u32 + 1);
                            t.push([a, c, b]);
                            t.push([b, c, d]);
                        }
                    }
                    self.b.add(p, t, m, colour);
                }
            }
            "Box" | "Cube" => {
                let (x, y, z) = match g.nums("size") {
                    Some(s) if s.len() >= 3 => (s[0], s[1], s[2]),
                    _ => (g.num("width", 2.0), g.num("height", 2.0), g.num("depth", 2.0)),
                };
                let (p, t) = cuboid(x, y, z);
                self.b.add(p, t, m, colour);
            }
            "Sphere" => {
                let (p, t) = sphere(g.num("radius", 1.0));
                self.b.add(p, t, m, colour);
            }
            "Cylinder" => {
                let r = g.num("radius", 1.0);
                let (p, t) = lathe(r, r, g.num("height", 2.0));
                self.b.add(p, t, m, colour);
            }
            "Cone" => {
                let (p, t) = lathe(g.num("bottomRadius", 1.0), 0.0, g.num("height", 2.0));
                self.b.add(p, t, m, colour);
            }
            _ => {}
        }
    }

    fn walk(&mut self, n: &Rc<Node>, m: &M, depth: usize) {
        if depth > 64 {
            return;
        }
        match n.kind.as_str() {
            "Shape" => {
                let c = colour_of(n);
                for g in n.nodes("geometry") {
                    self.geometry(&g, m, c);
                }
            }
            "Transform" => {
                let t = mul(&transform_matrix(n), m);
                for c in n.nodes("children") {
                    self.walk(&c, &t, depth + 1);
                }
            }
            "Group" | "Anchor" | "Collision" | "Billboard" | "Switch" | "LOD" | "Separator" | "TransformSeparator" | "WWWAnchor" | "Level" => {
                let kids = {
                    let mut k = n.nodes("children");
                    k.extend(n.nodes("choice"));
                    k.extend(n.nodes("level"));
                    k
                };
                if self.v1 {
                    // VRML 1: children in order, transform nodes change the state for the ones after them; the state is restored at the end.
                    let saved = (self.v1_coords.clone(), self.v1_colour);
                    let mut cur = *m;
                    for c in n.fields.iter().filter_map(|(_, v)| if let Value::Nodes(v) = v { Some(v) } else { None }).flatten() {
                        cur = self.v1_node(c, cur, depth);
                    }
                    (self.v1_coords, self.v1_colour) = saved;
                } else {
                    let take: Vec<Rc<Node>> = if n.kind == "Switch" || n.kind == "LOD" { kids.into_iter().take(1).collect() } else { kids };
                    for c in take {
                        self.walk(&c, m, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }

    /// One node of a VRML 1 Separator; returns the transform for the nodes that follow it.
    fn v1_node(&mut self, n: &Rc<Node>, m: M, depth: usize) -> M {
        let v3 = |name: &str, d: [f32; 3]| n.nums(name).filter(|v| v.len() >= 3).map(|v| [v[0], v[1], v[2]]).unwrap_or(d);
        match n.kind.as_str() {
            "Translation" => mul(&translate(v3("translation", [0.0; 3])), &m),
            "Scale" | "ScaleFactor" => mul(&scale(v3("scaleFactor", [1.0; 3])), &m),
            "Rotation" => {
                let r = n.nums("rotation").filter(|v| v.len() >= 4).map(|v| rotation([v[0], v[1], v[2]], v[3])).unwrap_or(ID);
                mul(&r, &m)
            }
            "Transform" => mul(&transform_matrix(n), &m),
            "Material" => {
                if let Some(c) = n.nums("diffuseColor").filter(|c| c.len() >= 3) {
                    self.v1_colour = [c[0], c[1], c[2]];
                }
                m
            }
            "Coordinate3" => {
                self.v1_coords = n.nums("point").map(|p| p.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()).unwrap_or_default();
                m
            }
            "IndexedFaceSet" | "Cube" | "Sphere" | "Cylinder" | "Cone" => {
                let c = self.v1_colour;
                self.geometry(n, &m, c);
                m
            }
            "Separator" | "Group" | "TransformSeparator" => {
                self.walk(n, &m, depth + 1);
                m
            }
            _ => m,
        }
    }
}

pub fn parse(bytes: &[u8]) -> Result<Mesh, String> {
    // .wrz is gzip-packed VRML.
    let unpacked;
    let bytes = if bytes.starts_with(&[0x1F, 0x8B]) {
        use std::io::Read;
        let mut v = Vec::new();
        flate2::read::GzDecoder::new(bytes).take(256 << 20).read_to_end(&mut v).map_err(|e| format!("vrml: gunzip: {e}"))?;
        unpacked = v;
        &unpacked[..]
    } else {
        bytes
    };
    let src = String::from_utf8_lossy(bytes);
    let head = src.lines().next().unwrap_or("");
    if !head.starts_with("#VRML") && !head.starts_with("\u{feff}#VRML") && !head.starts_with("#X3D") {
        return Err("vrml: not a VRML file (no #VRML header)".into());
    }
    let v1 = head.contains("V1.0");
    let mut p = Parser { toks: tokenize(&src), at: 0, defs: HashMap::new(), budget: 3_000_000 };
    let mut roots = Vec::new();
    while p.peek().is_some() {
        if p.at_node() {
            if let Some(n) = p.node()? {
                roots.push(n);
            }
        } else if matches!(p.peek(), Some("PROTO") | Some("EXTERNPROTO")) {
            // Prototype declarations are skipped (their instances are not expanded).
            p.next();
            p.next();
            p.skip_value();
            if p.peek() == Some("[") {
                p.skip_value();
            }
            if p.peek() == Some("{") {
                let mut depth = 0;
                while let Some(t) = p.next() {
                    match t.as_str() {
                        "{" => depth += 1,
                        "}" => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else if p.peek() == Some("ROUTE") {
            p.at += 4;
        } else {
            p.next();
        }
    }
    let mut w = Walk { b: Build::default(), v1, v1_coords: Vec::new(), v1_colour: [0.72, 0.74, 0.78] };
    for r in &roots {
        if v1 {
            w.v1_node(r, ID, 0);
        } else {
            w.walk(r, &ID, 0);
        }
    }
    let Build { pos, idx, parts, .. } = w.b;
    let mut mesh = Mesh::build(pos, Vec::new(), idx, true)?;
    if parts.len() > 1 || parts.first().is_some_and(|p| p.color != [0.72, 0.74, 0.78]) {
        mesh.parts = parts;
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_transforms_and_primitives() {
        let src = "#VRML V2.0 utf8\n# a comment\nDEF Tri Shape { appearance Appearance { material Material { diffuseColor 1 0 0 } } geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0 ] } coordIndex [ 0 1 2 3 -1 ] } }\nTransform { translation 5 0 0 children [ USE Tri  Shape { geometry Box { size 2 2 2 } } ] }\n";
        let m = parse(src.as_bytes()).unwrap();
        // The quad is 2 triangles, used twice (4), plus the box's 12.
        assert_eq!(m.triangles(), 16);
        assert!((m.max[0] - 6.0).abs() < 1e-3, "{:?}", m.max);
        assert_eq!(m.parts.len(), 3);
        assert_eq!(m.parts[0].color, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn rotation_scale_and_vrml1() {
        let src = "#VRML V2.0 utf8\nTransform { rotation 0 0 1 1.5707963 scale 2 2 2 children Shape { geometry Box { size 1 1 1 } } }";
        let m = parse(src.as_bytes()).unwrap();
        assert!((m.max[0] - 1.0).abs() < 1e-3 && (m.max[1] - 1.0).abs() < 1e-3);
        let v1 = "#VRML V1.0 ascii\nSeparator { Material { diffuseColor 0 1 0 } Translation { translation 0 3 0 } Coordinate3 { point [0 0 0, 1 0 0, 0 1 0] } IndexedFaceSet { coordIndex [0, 1, 2, -1] } }";
        let m = parse(v1.as_bytes()).unwrap();
        assert_eq!(m.triangles(), 1);
        assert!((m.min[1] - 3.0).abs() < 1e-3);
        assert!(parse(b"hello").is_err());
    }

    #[test]
    fn sphere_cone_cylinder_and_grid() {
        let src = "#VRML V2.0 utf8\nGroup { children [ Shape { geometry Sphere { radius 1 } } Shape { geometry Cone { height 2 bottomRadius 1 } } Shape { geometry Cylinder {} } Shape { geometry ElevationGrid { xDimension 2 zDimension 2 height [0 1 1 0] } } ] }";
        let m = parse(src.as_bytes()).unwrap();
        assert!(m.triangles() > 100);
        assert_eq!(m.parts.len(), 1.max(m.parts.len()));
    }
}
