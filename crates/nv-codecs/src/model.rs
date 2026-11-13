//! 3D models: STL (binary and ASCII), OBJ, PLY, glTF / GLB and binary FBX.
//!
//! A model decodes to an `Image` (a software-rendered preview, used for gallery thumbnails and as the first frame) that
//! also carries the `Mesh`; the viewer then draws the mesh itself with Direct3D so it can be turned and zoomed.
//! The camera maths (`Orbit`, `Camera`) lives here so the software preview and the GPU draw exactly the same picture.

use crate::Image;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Triangle soup / indexed mesh with per-vertex normals, in model space.
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Which axis the model treats as "up": true = Y (OBJ, glTF, FBX), false = Z (STL, PLY, CAD).
    pub y_up: bool,
    /// Texture coordinates per vertex (empty = none). (0,0) is the top-left of the picture, as in Direct3D.
    pub uvs: Vec<[f32; 2]>,
    /// Colour per vertex (empty = none), e.g. scanned models stored as PLY.
    pub colors: Vec<[u8; 4]>,
    /// Pictures that colour the surface (empty when the model has none or the setting is off).
    pub textures: Vec<ModelTexture>,
    /// Which triangles use which texture / colour. Empty = the whole model in the plain grey surface.
    /// Parts are consecutive, non-overlapping ranges of `indices`.
    pub parts: Vec<Part>,
}

/// A picture that colours (part of) a model: 8-bit sRGB RGBA, at most `MAX_TEXTURE_DIM` on a side.
pub struct ModelTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Textures wider or taller than this are shrunk when loaded (the GPU gets a mip chain either way).
pub const MAX_TEXTURE_DIM: u32 = 4096;
/// A model never loads more textures than this (a damaged file naming thousands of images).
const MAX_TEXTURES: usize = 48;

/// A run of triangles drawn with one material.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// First index (not triangle) in `Mesh::indices`, and how many indices.
    pub first: u32,
    pub count: u32,
    /// Index into `Mesh::textures`.
    pub texture: Option<usize>,
    /// Surface colour in sRGB numbers 0..1 (multiplies the texture).
    pub color: [f32; 3],
    /// The texture's alpha cuts holes in the surface (leaves, fences, ...).
    pub alpha_cut: bool,
    /// Index into `Mesh::textures` of a tangent-space normal map (needs texture coordinates, like `texture`).
    pub normal_map: Option<usize>,
}

/// Models with more triangles than this are refused (memory: ~72 bytes per triangle with flat shading).
pub const MAX_TRIANGLES: usize = 6_000_000;

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Build a mesh from raw geometry. Missing normals (`normals` empty or `None` entries) are computed smooth from the faces.
    fn build(positions: Vec<[f32; 3]>, normals: Vec<[f32; 3]>, indices: Vec<u32>, y_up: bool) -> Result<Mesh, String> {
        if indices.len() < 3 || positions.is_empty() {
            return Err("this model has no triangles (point clouds and empty files can't be shown)".into());
        }
        if indices.len() / 3 > MAX_TRIANGLES {
            return Err(format!("model too large to open ({} triangles, limit {})", indices.len() / 3, MAX_TRIANGLES));
        }
        let n = positions.len() as u32;
        if indices.iter().any(|&i| i >= n) {
            return Err("damaged model: a face points at a vertex that doesn't exist".into());
        }
        let mut normals = if normals.len() == positions.len() { normals } else { vec![[0.0; 3]; positions.len()] };
        // Vertices whose normal is (0,0,0) get the area-weighted average of their faces.
        let missing: Vec<bool> = normals.iter().map(|n| n[0] == 0.0 && n[1] == 0.0 && n[2] == 0.0).collect();
        if missing.iter().any(|&m| m) {
            for t in indices.chunks_exact(3) {
                let (a, b, c) = (positions[t[0] as usize], positions[t[1] as usize], positions[t[2] as usize]);
                let fnrm = cross(sub(b, a), sub(c, a)); // length = 2 * area, so bigger faces weigh more
                for &i in t {
                    if missing[i as usize] {
                        for k in 0..3 {
                            normals[i as usize][k] += fnrm[k];
                        }
                    }
                }
            }
        }
        for n in &mut normals {
            *n = normalize(*n);
        }
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        if min.iter().chain(max.iter()).any(|v| !v.is_finite()) {
            return Err("damaged model: coordinates are not finite".into());
        }
        Ok(Mesh { positions, normals, indices, min, max, y_up, uvs: Vec::new(), colors: Vec::new(), textures: Vec::new(), parts: Vec::new() })
    }

    /// Attach texture coordinates, vertex colours, textures and parts. Anything that doesn't fit the geometry
    /// (wrong vertex count, parts outside the index list, texture numbers that don't exist) is dropped instead of
    /// failing: a model with a damaged material still shows its shape.
    fn dressed(mut self, uvs: Vec<[f32; 2]>, colors: Vec<[u8; 4]>, textures: Vec<ModelTexture>, parts: Vec<Part>) -> Mesh {
        let n = self.positions.len();
        if uvs.len() == n && uvs.iter().all(|t| t[0].is_finite() && t[1].is_finite()) {
            self.uvs = uvs;
        }
        if colors.len() == n {
            self.colors = colors;
        }
        let len = self.indices.len() as u32;
        let tiles = !parts.is_empty() && parts.iter().all(|p| p.first.checked_add(p.count).map(|e| e <= len).unwrap_or(false));
        if tiles {
            let textured_ok = !self.uvs.is_empty();
            self.parts = parts
                .into_iter()
                .map(|mut p| {
                    // A texture needs coordinates, and must exist.
                    if !textured_ok || p.texture.map(|t| t >= textures.len()).unwrap_or(false) {
                        p.texture = None;
                        p.alpha_cut = false;
                    }
                    if !textured_ok || p.normal_map.map(|t| t >= textures.len()).unwrap_or(false) {
                        p.normal_map = None;
                    }
                    p
                })
                .collect();
            self.textures = textures;
        }
        self
    }

    /// True if any part is coloured by a picture.
    pub fn is_textured(&self) -> bool {
        self.parts.iter().any(|p| p.texture.is_some())
    }

    /// Number of colours/pictures the model brings with it (for the info line).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn centre(&self) -> [f32; 3] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0, (self.min[2] + self.max[2]) / 2.0]
    }

    /// Radius of the bounding sphere around `centre()`.
    pub fn radius(&self) -> f32 {
        let d = sub(self.max, self.min);
        (0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()).max(1e-6)
    }
}

// ---- small vector / matrix helpers (row-vector convention: v' = v * M, as HLSL's `mul(v, M)`) -------------

pub type Mat4 = [[f32; 4]; 4];

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalize(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l > 1e-20 { [a[0] / l, a[1] / l, a[2] / l] } else { [0.0, 0.0, 0.0] }
}

fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    r
}

/// Where the viewer looks at the model from: spherical angles around its centre plus a zoom factor.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    /// Radians around the up axis.
    pub yaw: f32,
    /// Radians above the horizon, clamped to just under +-90 degrees.
    pub pitch: f32,
    /// 1 = the whole model fits the view; bigger zooms in.
    pub zoom: f32,
    /// Treat the other axis as "up" (some files disagree with their format's convention).
    pub flip_up: bool,
    /// Slide the picture on screen: x, y as a fraction of the window (1.0 = half the window width / height).
    pub pan: [f32; 2],
}

impl Default for Orbit {
    fn default() -> Self {
        Orbit { yaw: -0.6, pitch: 0.45, zoom: 1.0, flip_up: false, pan: [0.0, 0.0] }
    }
}

impl Orbit {
    pub fn rotate(&mut self, dyaw: f32, dpitch: f32) {
        self.yaw += dyaw;
        self.pitch = (self.pitch + dpitch).clamp(-1.52, 1.52);
    }

    /// Move the picture by `dx`, `dy` pixels in a `w` x `h` window (what the pointer moved).
    pub fn pan_by(&mut self, dx: f32, dy: f32, w: f32, h: f32) {
        self.pan[0] = (self.pan[0] + 2.0 * dx / w.max(1.0)).clamp(-3.0, 3.0);
        self.pan[1] = (self.pan[1] - 2.0 * dy / h.max(1.0)).clamp(-3.0, 3.0);
    }

    pub fn zoom_by(&mut self, notches: f32) {
        self.zoom = (self.zoom * 1.12f32.powf(notches)).clamp(0.2, 60.0);
    }
}

/// How the surface is shaded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shading {
    /// Diffuse + a soft highlight + a faint rim: the default.
    Combined,
    /// Plain Lambert shading (what earlier versions did).
    Diffuse,
    /// Flat colour bands, dark rim.
    Cartoon,
    /// The surface normal as a colour (for checking a model's geometry).
    Normals,
    /// The colour or texture alone, no lighting.
    Unlit,
}

impl Shading {
    pub const ALL: [Shading; 5] = [Shading::Combined, Shading::Diffuse, Shading::Cartoon, Shading::Normals, Shading::Unlit];

    pub fn id(self) -> &'static str {
        match self {
            Shading::Combined => "combined",
            Shading::Diffuse => "diffuse",
            Shading::Cartoon => "cartoon",
            Shading::Normals => "normals",
            Shading::Unlit => "unlit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shading::Combined => "Combined (diffuse + highlight)",
            Shading::Diffuse => "Diffuse colour",
            Shading::Cartoon => "Cartoon",
            Shading::Normals => "Normals (colour = direction)",
            Shading::Unlit => "Unlit (colour / texture only)",
        }
    }

    pub fn from_id(s: &str) -> Option<Shading> {
        Self::ALL.iter().copied().find(|m| m.id().eq_ignore_ascii_case(s.trim()))
    }

    /// The mode after this one (for the cycle key).
    pub fn next(self) -> Shading {
        let i = Self::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// What the light's angles are measured against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightMode {
    /// The light is fixed to the viewer: 0/0 shines from the viewer's position, and it follows while the model turns.
    Viewer,
    /// The light is fixed in the scene: the model turns under it.
    World,
}

/// Which model axis points up when the camera orbits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpAxis {
    /// What the file format says (Y for OBJ / glTF / FBX, Z for STL / PLY).
    Auto,
    Y,
    Z,
}

/// Everything about how a 3D model is drawn that the user can change (the settings window and the T / N / W / Shift+S
/// keys). It never changes the model's data.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ModelStyle {
    pub shading: Shading,
    pub light: LightMode,
    /// Degrees to the right of straight ahead (-180..=180) and above the horizon (-90..=90).
    pub light_az: f32,
    pub light_el: f32,
    /// Orthographic instead of perspective projection.
    pub ortho: bool,
    pub up: UpAxis,
    /// Quarter turns (0..=3) applied to the model about X, then Y, then Z: fixes models that arrive face-down or lying on a side.
    pub turns: [u8; 3],
    pub texture: bool,
    pub normal_map: bool,
    pub wireframe: bool,
    pub shadow: bool,
}

impl Default for ModelStyle {
    fn default() -> Self {
        ModelStyle {
            shading: Shading::Combined,
            light: LightMode::Viewer,
            light_az: -25.0,
            light_el: 33.0,
            ortho: false,
            up: UpAxis::Auto,
            turns: [0; 3],
            texture: true,
            normal_map: true,
            wireframe: false,
            shadow: false,
        }
    }
}

impl ModelStyle {
    /// The model's own rotation as a row-vector matrix (X, then Y, then Z).
    pub fn rotation(&self) -> Mat4 {
        let q = |k: u8| -> (f32, f32) {
            match k % 4 {
                0 => (1.0, 0.0),
                1 => (0.0, 1.0),
                2 => (-1.0, 0.0),
                _ => (0.0, -1.0),
            }
        };
        let ((cx, sx), (cy, sy), (cz, sz)) = (q(self.turns[0]), q(self.turns[1]), q(self.turns[2]));
        let rx: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, cx, sx, 0.0], [0.0, -sx, cx, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let ry: Mat4 = [[cy, 0.0, -sy, 0.0], [0.0, 1.0, 0.0, 0.0], [sy, 0.0, cy, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let rz: Mat4 = [[cz, sz, 0.0, 0.0], [-sz, cz, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        mat_mul(&mat_mul(&rx, &ry), &rz)
    }

    /// Direction towards the light for the given angles, with +Z towards the viewer / +Y (or +Z) up.
    fn light_vector(&self, y_up: bool) -> [f32; 3] {
        let (az, el) = (self.light_az.to_radians(), self.light_el.to_radians());
        let (h, v) = (el.cos(), el.sin());
        // Viewer: x right, y up, z towards the viewer. World: the same with the model's up axis in place of y.
        if y_up || self.light == LightMode::Viewer { [az.sin() * h, v, az.cos() * h] } else { [az.sin() * h, az.cos() * h, v] }
    }
}

/// What the shaders need: model-view-projection, and the model + view rotation (to bring normals into view space).
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub mvp: Mat4,
    /// Rotation from model space to view space, for normals (and for position differences).
    pub view_rot: Mat4,
    /// Direction towards the light, in view space.
    pub light_view: [f32; 3],
    /// Model space -> clip space with the model flattened onto the floor along the light (None = no shadow possible).
    pub shadow_mvp: Option<Mat4>,
}

const FOV_Y: f32 = 0.6; // radians, a gentle perspective

/// Camera for `mesh` seen through `orbit` on a viewport with the given aspect ratio (width / height).
pub fn camera(mesh: &Mesh, orbit: &Orbit, aspect: f32) -> Camera {
    camera_styled(mesh, orbit, aspect, &ModelStyle::default())
}

/// The same, with the user's style: model rotation, up axis, projection, light and shadow.
pub fn camera_styled(mesh: &Mesh, orbit: &Orbit, aspect: f32, style: &ModelStyle) -> Camera {
    let c = mesh.centre();
    let r = mesh.radius();
    let base_up = match style.up {
        UpAxis::Auto => mesh.y_up,
        UpAxis::Y => true,
        UpAxis::Z => false,
    };
    let y_up = base_up != orbit.flip_up;
    let rot = style.rotation();
    // The model is moved so its centre is the origin, then turned by the style's rotation.
    let place: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [-c[0], -c[1], -c[2], 1.0]];
    let model = mat_mul(&place, &rot);

    let (cp, sp, cy, sy) = (orbit.pitch.cos(), orbit.pitch.sin(), orbit.yaw.cos(), orbit.yaw.sin());
    let (dir, up) = if y_up { ([cp * cy, sp, cp * sy], [0.0, 1.0, 0.0]) } else { ([cp * cy, cp * sy, sp], [0.0, 0.0, 1.0]) };
    // Far enough that the bounding sphere fits both ways, nearer when zoomed in.
    let t = (FOV_Y / 2.0).tan().min((FOV_Y / 2.0).tan() * aspect.max(0.05));
    let fit = r * (1.0 + t * t).sqrt() / t * 1.05;
    let dist = (fit / orbit.zoom).max(r * 0.02);
    let eye = [dir[0] * dist, dir[1] * dist, dir[2] * dist];
    // Right-handed look-at at the origin, row-vector convention.
    let z = normalize(dir);
    let x = normalize(cross(up, z));
    let y = cross(z, x);
    let view: Mat4 = [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [-dot(x, eye), -dot(y, eye), -dot(z, eye), 1.0],
    ];
    // A floor shadow can stretch well behind the model, so the far plane moves out when it is on.
    let (near, far) = ((dist - r * 1.2).max(dist * 0.02).max(r * 0.001), dist + r * if style.shadow { 6.0 } else { 1.5 });
    let h = 1.0 / (FOV_Y / 2.0).tan();
    let w = h / aspect.max(0.05);
    let proj: Mat4 = if style.ortho {
        // Same size as the perspective view at the model's centre.
        let half_h = dist / h;
        let half_w = half_h * aspect.max(0.05);
        [
            [1.0 / half_w, 0.0, 0.0, 0.0],
            [0.0, 1.0 / half_h, 0.0, 0.0],
            [0.0, 0.0, 1.0 / (near - far), 0.0],
            [0.0, 0.0, near / (near - far), 1.0],
        ]
    } else {
        [
            [w, 0.0, 0.0, 0.0],
            [0.0, h, 0.0, 0.0],
            [0.0, 0.0, far / (near - far), -1.0],
            [0.0, 0.0, near * far / (near - far), 0.0],
        ]
    };
    let mut view3 = view;
    view3[3] = [0.0, 0.0, 0.0, 1.0];
    let mut rot3 = rot;
    rot3[3] = [0.0, 0.0, 0.0, 1.0];
    let view_rot = mat_mul(&rot3, &view3);
    // Panning moves the finished picture in clip space (times w, so it is the same number of pixels at any depth).
    let slide: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [orbit.pan[0], orbit.pan[1], 0.0, 1.0]];
    let vp = mat_mul(&mat_mul(&view, &proj), &slide);

    // Light. `Viewer`: the angles are relative to the camera. `World`: relative to the scene, then brought to view space.
    let to_view = |v: [f32; 3]| normalize([dot3_col(v, &view3, 0), dot3_col(v, &view3, 1), dot3_col(v, &view3, 2)]);
    let to_world = |v: [f32; 3]| normalize([dot3_row(v, &view3, 0), dot3_row(v, &view3, 1), dot3_row(v, &view3, 2)]);
    let lv = style.light_vector(y_up);
    let (light_view, light_world) = match style.light {
        LightMode::Viewer => (normalize(lv), to_world(lv)),
        LightMode::World => (to_view(lv), normalize(lv)),
    };

    // Shadow: the model flattened onto the floor (its lowest point along the up axis) along the light.
    let shadow_mvp = style.shadow.then(|| {
        let n = up;
        let nl = dot(n, light_world);
        if nl < 0.25 {
            return None;
        }
        let mut floor = f32::MAX;
        for k in 0..8 {
            let corner = [if k & 1 == 0 { mesh.min[0] } else { mesh.max[0] }, if k & 2 == 0 { mesh.min[1] } else { mesh.max[1] }, if k & 4 == 0 { mesh.min[2] } else { mesh.max[2] }];
            let p = sub(corner, c);
            let q = [dot3_col(p, &rot3, 0), dot3_col(p, &rot3, 1), dot3_col(p, &rot3, 2)];
            floor = floor.min(dot(n, q));
        }
        let l = light_world;
        let mut flat: Mat4 = [[0.0; 4]; 4];
        for i in 0..3 {
            for j in 0..3 {
                flat[i][j] = if i == j { 1.0 } else { 0.0 } - l[j] * n[i] / nl;
            }
            flat[3][i] = l[i] * floor / nl;
        }
        flat[3][3] = 1.0;
        Some(mat_mul(&mat_mul(&model, &flat), &vp))
    });

    Camera { mvp: mat_mul(&model, &vp), view_rot, light_view, shadow_mvp: shadow_mvp.flatten() }
}

/// `v * M` for the 3x3 part of a row-vector matrix, component `j` (the column).
fn dot3_col(v: [f32; 3], m: &Mat4, j: usize) -> f32 {
    v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j]
}

/// `v * M^T` (the inverse of a rotation), component `i`.
fn dot3_row(v: [f32; 3], m: &Mat4, i: usize) -> f32 {
    v[0] * m[i][0] + v[1] * m[i][1] + v[2] * m[i][2]
}

/// Light direction in view space (from the surface towards a light up-left of the viewer) and the surface colour.
pub const LIGHT_DIR: [f32; 3] = [-0.35, 0.55, 0.76];
pub const BASE_COLOR: [f32; 3] = [0.74, 0.77, 0.84];
const BACKDROP: [u8; 3] = [34, 36, 42];

/// Lambert shading with a floor of ambient light; two-sided, since STL winding is often inconsistent.
pub fn shade(normal_view: [f32; 3]) -> f32 {
    let l = normalize(LIGHT_DIR);
    0.24 + 0.76 * dot(normalize(normal_view), l).abs()
}

/// Software rasteriser used for thumbnails and the first frame: z-buffer, Gouraud shading.
pub fn render_preview(mesh: &Mesh, orbit: &Orbit, w: u32, h: u32) -> Vec<u8> {
    let cam = camera(mesh, orbit, w as f32 / h as f32);
    let (wf, hf) = (w as f32, h as f32);
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]);
    }
    let mut zbuf = vec![f32::INFINITY; w as usize * h as usize];

    // Project every vertex once: screen x, y, depth, and the shade.
    let n = mesh.positions.len();
    let mut proj: Vec<[f32; 4]> = Vec::with_capacity(n);
    let mut inv_w: Vec<f32> = Vec::with_capacity(n);
    for (p, nrm) in mesh.positions.iter().zip(&mesh.normals) {
        let m = &cam.mvp;
        let clip = [
            p[0] * m[0][0] + p[1] * m[1][0] + p[2] * m[2][0] + m[3][0],
            p[0] * m[0][1] + p[1] * m[1][1] + p[2] * m[2][1] + m[3][1],
            p[0] * m[0][2] + p[1] * m[1][2] + p[2] * m[2][2] + m[3][2],
            p[0] * m[0][3] + p[1] * m[1][3] + p[2] * m[2][3] + m[3][3],
        ];
        let r = &cam.view_rot;
        let nv = [
            nrm[0] * r[0][0] + nrm[1] * r[1][0] + nrm[2] * r[2][0],
            nrm[0] * r[0][1] + nrm[1] * r[1][1] + nrm[2] * r[2][1],
            nrm[0] * r[0][2] + nrm[1] * r[1][2] + nrm[2] * r[2][2],
        ];
        if clip[3] <= 1e-6 {
            proj.push([f32::NAN, 0.0, 0.0, 0.0]);
            inv_w.push(0.0);
            continue;
        }
        let inv = 1.0 / clip[3];
        inv_w.push(inv);
        proj.push([(clip[0] * inv * 0.5 + 0.5) * wf, (1.0 - (clip[1] * inv * 0.5 + 0.5)) * hf, clip[2] * inv, shade(nv)]);
    }

    // Very dense meshes are sampled: a 256 px thumbnail doesn't need five million triangles.
    let tris = mesh.triangles();
    let stride = (tris / 400_000).max(1);
    let small: Vec<(u32, u32, Vec<u8>)> = mesh.textures.iter().map(|t| shrink_for_preview(t, w.max(h).min(1024))).collect();
    // The runs of triangles to draw: the parts, or everything in the plain surface.
    let plain = Part { first: 0, count: mesh.indices.len() as u32, texture: None, color: BASE_COLOR, alpha_cut: false, normal_map: None };
    let runs: Vec<&Part> = if mesh.parts.is_empty() { vec![&plain] } else { mesh.parts.iter().collect() };
    let has_uv = !mesh.uvs.is_empty();
    let has_col = !mesh.colors.is_empty();
    for part in runs {
        let tex = part.texture.filter(|_| has_uv).and_then(|t| small.get(t));
        // Vertex colours replace the default grey (a part with its own colour keeps it and multiplies).
        let base = if has_col && mesh.parts.is_empty() { [1.0; 3] } else { part.color };
        let range = &mesh.indices[part.first as usize..(part.first + part.count) as usize];
        for t in range.chunks_exact(3).step_by(stride) {
            let (a, b, c) = (proj[t[0] as usize], proj[t[1] as usize], proj[t[2] as usize]);
            if a[0].is_nan() || b[0].is_nan() || c[0].is_nan() {
                continue;
            }
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area.abs() < 1e-9 {
                continue;
            }
            let (iw0, iw1, iw2) = (inv_w[t[0] as usize], inv_w[t[1] as usize], inv_w[t[2] as usize]);
            let (x0, x1) = (a[0].min(b[0]).min(c[0]).floor().max(0.0) as i32, a[0].max(b[0]).max(c[0]).ceil().min(wf - 1.0) as i32);
            let (y0, y1) = (a[1].min(b[1]).min(c[1]).floor().max(0.0) as i32, a[1].max(b[1]).max(c[1]).ceil().min(hf - 1.0) as i32);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                    let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                    let i = y as usize * w as usize + x as usize;
                    if z < zbuf[i] {
                        // Perspective-correct weights for the attributes that live on the surface.
                        let (p0, p1, p2) = (w0 * iw0, w1 * iw1, w2 * iw2);
                        let k = 1.0 / (p0 + p1 + p2).max(1e-20);
                        let (p0, p1, p2) = (p0 * k, p1 * k, p2 * k);
                        let mut rgb = base;
                        if has_col {
                            for ch in 0..3 {
                                let v = |vi: u32| mesh.colors[vi as usize][ch] as f32 / 255.0;
                                rgb[ch] *= p0 * v(t[0]) + p1 * v(t[1]) + p2 * v(t[2]);
                            }
                        }
                        if let Some((tw, th, px8)) = tex {
                            let uv = |vi: u32| mesh.uvs[vi as usize];
                            let (u, v) = (p0 * uv(t[0])[0] + p1 * uv(t[1])[0] + p2 * uv(t[2])[0], p0 * uv(t[0])[1] + p1 * uv(t[1])[1] + p2 * uv(t[2])[1]);
                            let (fu, fv) = (u - u.floor(), v - v.floor());
                            let (tx, ty) = (((fu * *tw as f32) as u32).min(tw - 1), ((fv * *th as f32) as u32).min(th - 1));
                            let o = ((ty * tw + tx) * 4) as usize;
                            if part.alpha_cut && px8[o + 3] < 128 {
                                continue;
                            }
                            for ch in 0..3 {
                                rgb[ch] *= px8[o + ch] as f32 / 255.0;
                            }
                        }
                        zbuf[i] = z;
                        let s = (w0 * a[3] + w1 * b[3] + w2 * c[3]).clamp(0.0, 1.0);
                        for ch in 0..3 {
                            // The colours are sRGB numbers here; the GPU path treats the same numbers as linear after conversion.
                            rgba[i * 4 + ch] = (rgb[ch] * s * 255.0).clamp(0.0, 255.0).round() as u8;
                        }
                    }
                }
            }
        }
    }
    rgba
}

/// A texture reduced (box filter, whole-number factor) to at most `limit` pixels on its long side, for the software preview.
fn shrink_for_preview(t: &ModelTexture, limit: u32) -> (u32, u32, Vec<u8>) {
    let f = t.width.max(t.height).div_ceil(limit.max(1)).max(1);
    if f == 1 {
        return (t.width, t.height, t.rgba.clone());
    }
    let (w, h) = ((t.width / f).max(1), (t.height / f).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            let mut n = 0;
            for dy in 0..f {
                let sy = y * f + dy;
                if sy >= t.height {
                    break;
                }
                for dx in 0..f {
                    let sx = x * f + dx;
                    if sx >= t.width {
                        break;
                    }
                    let o = ((sy * t.width + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += t.rgba[o + c] as u32;
                    }
                    n += 1;
                }
            }
            let o = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    (w, h, out)
}

// ---- entry point ---------------------------------------------------------------------------------

/// Extensions handled here.
pub const EXTS: &[&str] = &["stl", "obj", "ply", "glb", "gltf", "fbx"];

pub fn decode(path: &Path, thumb: bool) -> Result<Image, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let bytes = crate::progress::read_file(path)?;
    let mesh = match ext.as_str() {
        "stl" => parse_stl(&bytes)?,
        "obj" => parse_obj(&bytes, path.parent())?,
        "ply" => parse_ply(&bytes, path.parent())?,
        "glb" | "gltf" => parse_gltf(&bytes, path.parent())?,
        "fbx" => parse_fbx(&bytes, path.parent())?,
        _ => return Err(format!("unsupported 3D format: .{ext}")),
    };
    let long = if thumb { 320 } else { 1200 };
    let (w, h) = (long, long * 3 / 4);
    let rgba = render_preview(&mesh, &Orbit::default(), w, h);
    let mut img = Image::new(w, h, rgba);
    img.mesh = Some(Arc::new(mesh));
    Ok(img)
}

// ---- STL -----------------------------------------------------------------------------------------

fn f32le(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn parse_stl(bytes: &[u8]) -> Result<Mesh, String> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if count > 0 && bytes.len() == 84 + count * 50 {
            return parse_stl_binary(bytes, count);
        }
        // Some exporters pad the file: accept it if the declared triangles fit and the text doesn't look like ASCII STL.
        if count > 0 && bytes.len() > 84 + count * 50 && !looks_ascii_stl(bytes) {
            return parse_stl_binary(bytes, count);
        }
    }
    if looks_ascii_stl(bytes) {
        return parse_stl_ascii(&String::from_utf8_lossy(bytes));
    }
    Err("not a valid STL file".into())
}

fn looks_ascii_stl(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(2048)]);
    head.trim_start().starts_with("solid") && head.contains("facet")
}

fn parse_stl_binary(b: &[u8], count: usize) -> Result<Mesh, String> {
    if count > MAX_TRIANGLES {
        return Err(format!("model too large to open ({count} triangles, limit {MAX_TRIANGLES})"));
    }
    let mut positions = Vec::with_capacity(count * 3);
    let mut normals = Vec::with_capacity(count * 3);
    for t in 0..count {
        let o = 84 + t * 50;
        let mut n = [f32le(b, o), f32le(b, o + 4), f32le(b, o + 8)];
        let v: [[f32; 3]; 3] = std::array::from_fn(|i| [f32le(b, o + 12 + i * 12), f32le(b, o + 16 + i * 12), f32le(b, o + 20 + i * 12)]);
        // Flat shading: every triangle keeps its own vertices and the face normal (recomputed if the file's is zero/garbage).
        if !(n[0].is_finite() && n[1].is_finite() && n[2].is_finite()) || dot(n, n) < 0.5 {
            n = normalize(cross(sub(v[1], v[0]), sub(v[2], v[0])));
        } else {
            n = normalize(n);
        }
        if n == [0.0; 3] {
            n = [0.0, 0.0, 1.0]; // degenerate triangle: any normal will do, it covers no pixels
        }
        positions.extend(v);
        normals.extend([n, n, n]);
    }
    let indices: Vec<u32> = (0..(count * 3) as u32).collect();
    Mesh::build(positions, normals, indices, false)
}

fn parse_stl_ascii(text: &str) -> Result<Mesh, String> {
    let mut positions = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        if it.next() == Some("vertex") {
            let v: Vec<f32> = it.filter_map(|t| t.parse().ok()).collect();
            if v.len() == 3 {
                positions.push([v[0], v[1], v[2]]);
            }
        }
    }
    let tri_verts = positions.len() / 3 * 3;
    positions.truncate(tri_verts);
    let mut normals = Vec::with_capacity(positions.len());
    for t in positions.chunks_exact(3) {
        let n = normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        normals.extend([n, n, n]);
    }
    let indices: Vec<u32> = (0..positions.len() as u32).collect();
    Mesh::build(positions, normals, indices, false)
}

// ---- textures ------------------------------------------------------------------------------------

/// Make a model texture out of a decoded picture (flattened to 8-bit sRGB, shrunk if huge).
fn texture_from_image(mut img: Image) -> Option<ModelTexture> {
    img.apply_icc();
    img.orientation = 1; // a texture is used as stored, whatever EXIF says
    let img = if img.width.max(img.height) > MAX_TEXTURE_DIM { crate::downscale(&img, MAX_TEXTURE_DIM) } else { img };
    (img.width > 0 && img.height > 0).then_some(ModelTexture { width: img.width, height: img.height, rgba: img.rgba })
}

fn load_texture_file(path: &Path) -> Option<ModelTexture> {
    std::panic::catch_unwind(|| crate::decode_file_first_frame(path).ok().and_then(texture_from_image)).ok().flatten()
}

fn load_texture_bytes(bytes: &[u8], ext_hint: &str) -> Option<ModelTexture> {
    std::panic::catch_unwind(|| crate::decode_image_bytes(bytes, ext_hint).ok().and_then(texture_from_image)).ok().flatten()
}

/// Find a texture file a model names. Exporters write all kinds of paths: absolute paths from the artist's machine,
/// backslashes, a different folder. Try the name as given next to the model, then just its file name next to the model
/// and in the usual texture folders, then anywhere one level down.
fn find_texture_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim().trim_matches('"');
    if name.is_empty() || name.starts_with("data:") {
        return None;
    }
    let slashed = name.replace('\\', "/");
    let file_name = slashed.rsplit('/').next().unwrap_or(&slashed);
    let candidates = [
        dir.join(&slashed),
        dir.join(file_name),
        dir.join("textures").join(file_name),
        dir.join("Textures").join(file_name),
        dir.join("texture").join(file_name),
        dir.join("maps").join(file_name),
        dir.join("images").join(file_name),
        dir.join("tex").join(file_name),
        dir.join("..").join("textures").join(file_name),
    ];
    if let Some(p) = candidates.iter().find(|p| p.is_file()) {
        return Some(p.clone());
    }
    // The extension is often wrong (.tga in the file, .png on disk): look for the same stem.
    let stem = Path::new(file_name).file_stem()?.to_string_lossy().to_lowercase();
    let mut folders = vec![dir.to_path_buf()];
    folders.extend(std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).take(24));
    for folder in folders {
        for e in std::fs::read_dir(&folder).ok()?.flatten().take(5000) {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase()).unwrap_or_default();
            if p.file_stem().map(|s| s.to_string_lossy().to_lowercase()) == Some(stem.clone()) && crate::is_supported_ext(&ext) && !EXTS.contains(&ext.as_str()) {
                return Some(p);
            }
        }
    }
    None
}

/// Loads each distinct texture file once and hands out its number.
struct TextureSet {
    list: Vec<ModelTexture>,
    by_path: HashMap<PathBuf, Option<usize>>,
}

impl TextureSet {
    fn new() -> Self {
        TextureSet { list: Vec::new(), by_path: HashMap::new() }
    }

    fn from_file(&mut self, dir: &Path, name: &str) -> Option<usize> {
        let path = find_texture_file(dir, name)?;
        if let Some(r) = self.by_path.get(&path) {
            return *r;
        }
        let r = if self.list.len() < MAX_TEXTURES {
            load_texture_file(&path).map(|t| {
                self.list.push(t);
                self.list.len() - 1
            })
        } else {
            None
        };
        self.by_path.insert(path, r);
        r
    }

    fn push(&mut self, t: ModelTexture) -> Option<usize> {
        (self.list.len() < MAX_TEXTURES).then(|| {
            self.list.push(t);
            self.list.len() - 1
        })
    }
}

/// Linear light to the sRGB numbers the viewer shades with.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

// ---- OBJ + MTL -----------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct MtlMaterial {
    kd: Option<[f32; 3]>,
    map_kd: Option<String>,
    /// `map_Bump` / `bump` / `norm`: a normal map (height-map bumps are not converted, they are used as given).
    map_bump: Option<String>,
    /// `map_d` names a picture whose alpha/greyscale cuts holes.
    cutout: bool,
}

/// The file name in an MTL texture line: skips options such as `-s 1 1 1` or `-bm 0.5`.
fn mtl_texture_name(rest: &str) -> Option<String> {
    let toks: Vec<&str> = rest.split_whitespace().collect();
    let mut i = 0;
    while i < toks.len() && toks[i].starts_with('-') {
        i += 1;
        while i < toks.len() && toks[i].parse::<f32>().is_ok() {
            i += 1;
        }
    }
    let name = toks[i..].join(" ");
    (!name.is_empty()).then_some(name)
}

fn parse_mtl(text: &str, out: &mut HashMap<String, MtlMaterial>) {
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key.to_ascii_lowercase().as_str() {
            "newmtl" => {
                cur = Some(rest.to_string());
                out.entry(rest.to_string()).or_default();
            }
            "kd" => {
                let c: Vec<f32> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if let (Some(name), true) = (&cur, c.len() >= 3) {
                    if let Some(m) = out.get_mut(name) {
                        m.kd = Some([c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)]);
                    }
                }
            }
            "map_kd" => {
                if let (Some(name), Some(tex)) = (&cur, mtl_texture_name(rest)) {
                    if let Some(m) = out.get_mut(name) {
                        m.map_kd = Some(tex);
                    }
                }
            }
            "map_bump" | "bump" | "norm" | "map_kn" => {
                if let (Some(name), Some(tex)) = (&cur, mtl_texture_name(rest)) {
                    if let Some(m) = out.get_mut(name) {
                        m.map_bump = Some(tex);
                    }
                }
            }
            "map_d" => {
                if let Some(name) = &cur {
                    if let Some(m) = out.get_mut(name) {
                        m.cutout = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// `dir`: the folder of the .obj, where its .mtl and textures are looked for. `None` = geometry only.
pub fn parse_obj(bytes: &[u8], dir: Option<&Path>) -> Result<Mesh, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut vn: Vec<[f32; 3]> = Vec::new();
    let mut vt: Vec<[f32; 2]> = Vec::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut any_uv = false;
    // Faces are collected per material, then laid out one material after the other.
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new()];
    let mut names: Vec<String> = vec![String::new()];
    let mut cur_bucket = 0usize;
    let mut mtllibs: Vec<String> = Vec::new();
    let mut seen: HashMap<(u32, u32, u32), u32> = HashMap::new();
    let mut triangles = 0usize;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f32> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if c.len() == 3 {
                    v.push([c[0], c[1], c[2]]);
                }
            }
            Some("vn") => {
                let c: Vec<f32> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if c.len() == 3 {
                    vn.push([c[0], c[1], c[2]]);
                }
            }
            Some("vt") => {
                let c: Vec<f32> = it.take(2).filter_map(|t| t.parse().ok()).collect();
                if c.len() >= 2 {
                    vt.push([c[0], c[1]]);
                } else if c.len() == 1 {
                    vt.push([c[0], 0.0]);
                }
            }
            Some("mtllib") => {
                let rest = line.trim_start().strip_prefix("mtllib").unwrap_or("").trim();
                if !rest.is_empty() {
                    mtllibs.push(rest.to_string());
                }
            }
            Some("usemtl") => {
                let name = line.trim_start().strip_prefix("usemtl").unwrap_or("").trim().to_string();
                cur_bucket = match names.iter().position(|n| *n == name) {
                    Some(i) => i,
                    None => {
                        names.push(name);
                        buckets.push(Vec::new());
                        names.len() - 1
                    }
                };
            }
            Some("f") => {
                let mut face: Vec<u32> = Vec::new();
                for tok in it {
                    let mut parts = tok.split('/');
                    let vi = parts.next().and_then(|t| t.parse::<i64>().ok());
                    let ti = parts.next().and_then(|t| t.parse::<i64>().ok());
                    let ni = parts.next().and_then(|t| t.parse::<i64>().ok());
                    let resolve = |i: i64, len: usize| -> Option<u32> {
                        let r = if i > 0 { i - 1 } else { len as i64 + i };
                        (r >= 0 && (r as usize) < len).then_some(r as u32)
                    };
                    let Some(vi) = vi.and_then(|i| resolve(i, v.len())) else { continue };
                    let ni = ni.and_then(|i| resolve(i, vn.len())).unwrap_or(u32::MAX);
                    let ti = ti.and_then(|i| resolve(i, vt.len())).unwrap_or(u32::MAX);
                    let idx = *seen.entry((vi, ti, ni)).or_insert_with(|| {
                        positions.push(v[vi as usize]);
                        // (0,0,0) means "compute from the faces" in `Mesh::build`.
                        normals.push(if ni == u32::MAX { [0.0; 3] } else { vn[ni as usize] });
                        // OBJ puts (0,0) at the bottom-left of the picture, Direct3D at the top-left.
                        uvs.push(if ti == u32::MAX { [0.0, 0.0] } else { [vt[ti as usize][0], 1.0 - vt[ti as usize][1]] });
                        any_uv |= ti != u32::MAX;
                        (positions.len() - 1) as u32
                    });
                    face.push(idx);
                }
                let bucket = &mut buckets[cur_bucket];
                for k in 1..face.len().saturating_sub(1) {
                    bucket.extend([face[0], face[k], face[k + 1]]);
                    triangles += 1;
                }
                if triangles > MAX_TRIANGLES {
                    return Err(format!("model too large to open (more than {MAX_TRIANGLES} triangles)"));
                }
            }
            _ => {}
        }
    }

    // Materials and textures (only when asked for, and only if the model sits in a folder we can look in).
    let mut mats: HashMap<String, MtlMaterial> = HashMap::new();
    let mut set = TextureSet::new();
    let want = dir.is_some() && crate::model_textures_enabled();
    if let (true, Some(dir)) = (want, dir) {
        for lib in &mtllibs {
            // A name with spaces is one file; several names on one line are several files.
            let candidates: Vec<String> = if find_texture_file(dir, lib).is_some() { vec![lib.clone()] } else { lib.split_whitespace().map(String::from).collect() };
            for c in candidates {
                if let Some(p) = find_texture_file(dir, &c) {
                    if let Ok(bytes) = std::fs::read(&p) {
                        parse_mtl(&String::from_utf8_lossy(&bytes), &mut mats);
                    }
                }
            }
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    for (name, bucket) in names.iter().zip(&buckets) {
        if bucket.is_empty() {
            continue;
        }
        let first = indices.len() as u32;
        indices.extend_from_slice(bucket);
        if let (true, Some(dir)) = (want, dir) {
            let m = mats.get(name).cloned().unwrap_or_default();
            let texture = m.map_kd.as_deref().and_then(|t| set.from_file(dir, t));
            let normal_map = m.map_bump.as_deref().and_then(|t| set.from_file(dir, t));
            parts.push(Part {
                first,
                count: bucket.len() as u32,
                texture,
                color: m.kd.unwrap_or(if texture.is_some() { [1.0; 3] } else { BASE_COLOR }),
                alpha_cut: texture.is_some() && m.cutout,
                normal_map,
            });
        }
    }
    let mesh = Mesh::build(positions, normals, indices, true)?;
    Ok(mesh.dressed(if any_uv && want { uvs } else { Vec::new() }, Vec::new(), set.list, parts))
}

// ---- PLY -----------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Fmt {
    Ascii,
    Le,
    Be,
}

#[derive(Clone, Copy)]
enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Ty {
    fn parse(s: &str) -> Option<Ty> {
        Some(match s {
            "char" | "int8" => Ty::I8,
            "uchar" | "uint8" => Ty::U8,
            "short" | "int16" => Ty::I16,
            "ushort" | "uint16" => Ty::U16,
            "int" | "int32" => Ty::I32,
            "uint" | "uint32" => Ty::U32,
            "float" | "float32" => Ty::F32,
            "double" | "float64" => Ty::F64,
            _ => return None,
        })
    }
    fn size(self) -> usize {
        match self {
            Ty::I8 | Ty::U8 => 1,
            Ty::I16 | Ty::U16 => 2,
            Ty::I32 | Ty::U32 | Ty::F32 => 4,
            Ty::F64 => 8,
        }
    }
}

struct Prop {
    name: String,
    ty: Ty,
    /// For list properties: the type of the count.
    list: Option<Ty>,
}

struct Element {
    name: String,
    count: usize,
    props: Vec<Prop>,
}

struct PlyReader<'a> {
    data: &'a [u8],
    pos: usize,
    fmt: Fmt,
    text: std::str::SplitAsciiWhitespace<'a>,
}

impl PlyReader<'_> {
    fn next(&mut self, ty: Ty) -> Option<f64> {
        if self.fmt == Fmt::Ascii {
            return self.text.next()?.parse::<f64>().ok();
        }
        let n = ty.size();
        let b = self.data.get(self.pos..self.pos + n)?;
        self.pos += n;
        let le = self.fmt == Fmt::Le;
        macro_rules! num {
            ($t:ty) => {{
                let mut a = [0u8; std::mem::size_of::<$t>()];
                a.copy_from_slice(b);
                (if le { <$t>::from_le_bytes(a) } else { <$t>::from_be_bytes(a) }) as f64
            }};
        }
        Some(match ty {
            Ty::I8 => b[0] as i8 as f64,
            Ty::U8 => b[0] as f64,
            Ty::I16 => num!(i16),
            Ty::U16 => num!(u16),
            Ty::I32 => num!(i32),
            Ty::U32 => num!(u32),
            Ty::F32 => num!(f32),
            Ty::F64 => num!(f64),
        })
    }
}

pub fn parse_ply(bytes: &[u8], dir: Option<&Path>) -> Result<Mesh, String> {
    let end = bytes.windows(10).position(|w| w == b"end_header").ok_or("ply: no header")?;
    let after = end + b"end_header".len();
    // The body starts after the newline that ends the header line (\n or \r\n).
    let mut body = after;
    if bytes.get(body) == Some(&b'\r') {
        body += 1;
    }
    if bytes.get(body) == Some(&b'\n') {
        body += 1;
    }
    let header = String::from_utf8_lossy(&bytes[..end]);
    let mut fmt = None;
    let mut elements: Vec<Element> = Vec::new();
    let mut texture_file: Option<String> = None;
    for line in header.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        match t.as_slice() {
            // MeshLab and friends: "comment TextureFile picture.png"
            [c, tf, rest @ ..] if *c == "comment" && tf.eq_ignore_ascii_case("TextureFile") && !rest.is_empty() => texture_file = Some(rest.join(" ")),
            ["format", f, ..] => {
                fmt = Some(match *f {
                    "ascii" => Fmt::Ascii,
                    "binary_little_endian" => Fmt::Le,
                    "binary_big_endian" => Fmt::Be,
                    _ => return Err("ply: unknown format".into()),
                })
            }
            ["element", name, count] => elements.push(Element { name: name.to_string(), count: count.parse().map_err(|_| "ply: bad element count")?, props: Vec::new() }),
            ["property", "list", cty, ity, name] => {
                let (c, i) = (Ty::parse(cty).ok_or("ply: bad type")?, Ty::parse(ity).ok_or("ply: bad type")?);
                elements.last_mut().ok_or("ply: property before element")?.props.push(Prop { name: name.to_string(), ty: i, list: Some(c) });
            }
            ["property", ty, name] => {
                elements.last_mut().ok_or("ply: property before element")?.props.push(Prop { name: name.to_string(), ty: Ty::parse(ty).ok_or("ply: bad type")?, list: None });
            }
            _ => {}
        }
    }
    let fmt = fmt.ok_or("ply: no format line")?;
    let body_bytes = &bytes[body.min(bytes.len())..];
    let mut rd = PlyReader { data: body_bytes, pos: 0, fmt, text: "".split_ascii_whitespace() };
    let body_text;
    if fmt == Fmt::Ascii {
        body_text = String::from_utf8_lossy(body_bytes).into_owned();
        rd.text = body_text.split_ascii_whitespace();
    }

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let (mut any_uv, mut any_color) = (false, false);
    let mut indices: Vec<u32> = Vec::new();
    // Faces that carry their own texture coordinates per corner: (vertex indices, uv per corner).
    let mut corner_faces: Vec<(Vec<u32>, Vec<[f32; 2]>)> = Vec::new();
    for el in &elements {
        if el.count > 200_000_000 {
            return Err("ply: absurd element count".into());
        }
        for _ in 0..el.count {
            let (mut p, mut n) = ([0f32; 3], [0f32; 3]);
            let (mut uv, mut col) = ([0f32; 2], [255u8, 255, 255, 255]);
            let mut has_n = false;
            let (mut face_idx, mut face_uv): (Option<Vec<u32>>, Option<Vec<[f32; 2]>>) = (None, None);
            for prop in &el.props {
                if let Some(cty) = prop.list {
                    let c = rd.next(cty).ok_or("ply: file ends early")? as usize;
                    let mut vals: Vec<f64> = Vec::with_capacity(c.min(64));
                    for _ in 0..c {
                        vals.push(rd.next(prop.ty).ok_or("ply: file ends early")?);
                    }
                    if el.name == "face" && (prop.name == "vertex_indices" || prop.name == "vertex_index") {
                        face_idx = Some(vals.iter().map(|&x| x as u32).collect());
                    } else if el.name == "face" && prop.name == "texcoord" {
                        face_uv = Some(vals.chunks_exact(2).map(|t| [t[0] as f32, t[1] as f32]).collect());
                    }
                } else {
                    let v = rd.next(prop.ty).ok_or("ply: file ends early")? as f32;
                    if el.name == "vertex" {
                        match prop.name.as_str() {
                            "x" => p[0] = v,
                            "y" => p[1] = v,
                            "z" => p[2] = v,
                            "nx" => {
                                n[0] = v;
                                has_n = true
                            }
                            "ny" => n[1] = v,
                            "nz" => n[2] = v,
                            "s" | "u" | "texture_u" | "texture_s" => {
                                uv[0] = v;
                                any_uv = true;
                            }
                            "t" | "v" | "texture_v" | "texture_t" => uv[1] = v,
                            "red" | "diffuse_red" => {
                                col[0] = v.clamp(0.0, 255.0) as u8;
                                any_color = true;
                            }
                            "green" | "diffuse_green" => col[1] = v.clamp(0.0, 255.0) as u8,
                            "blue" | "diffuse_blue" => col[2] = v.clamp(0.0, 255.0) as u8,
                            "alpha" | "diffuse_alpha" => col[3] = v.clamp(0.0, 255.0) as u8,
                            _ => {}
                        }
                    }
                }
            }
            if el.name == "vertex" {
                positions.push(p);
                normals.push(if has_n { n } else { [0.0; 3] });
                uvs.push(uv);
                colors.push(col);
            } else if let Some(face) = face_idx {
                match face_uv.filter(|u| u.len() == face.len()) {
                    Some(u) => corner_faces.push((face, u)),
                    None => {
                        for k in 1..face.len().saturating_sub(1) {
                            indices.extend([face[0], face[k], face[k + 1]]);
                        }
                    }
                }
            }
        }
        if indices.len() / 3 > MAX_TRIANGLES {
            return Err(format!("model too large to open (more than {MAX_TRIANGLES} triangles)"));
        }
    }
    // Per-corner coordinates: every corner becomes its own vertex (a vertex can have different coordinates per face).
    if !corner_faces.is_empty() {
        any_uv = true;
        let n = positions.len() as u32;
        for (face, fuv) in corner_faces {
            let base = positions.len() as u32;
            for (&vi, &t) in face.iter().zip(&fuv) {
                if vi >= n {
                    continue;
                }
                positions.push(positions[vi as usize]);
                normals.push(normals[vi as usize]);
                colors.push(colors[vi as usize]);
                uvs.push(t);
            }
            let corners = (positions.len() as u32 - base) as usize;
            if corners == face.len() {
                for k in 1..corners.saturating_sub(1) as u32 {
                    indices.extend([base, base + k, base + k + 1]);
                }
            }
            if indices.len() / 3 > MAX_TRIANGLES {
                return Err(format!("model too large to open (more than {MAX_TRIANGLES} triangles)"));
            }
        }
    }
    let mesh = Mesh::build(positions, normals, indices, false)?;
    let want = dir.is_some() && crate::model_textures_enabled();
    if !want {
        return Ok(mesh);
    }
    let mut set = TextureSet::new();
    let mut parts = Vec::new();
    if let (true, Some(dir), Some(name)) = (any_uv, dir, texture_file.as_deref()) {
        if let Some(t) = set.from_file(dir, name) {
            parts.push(Part { first: 0, count: mesh.indices.len() as u32, texture: Some(t), color: [1.0; 3], alpha_cut: false, normal_map: None });
        }
    }
    Ok(mesh.dressed(if any_uv { uvs } else { Vec::new() }, if any_color { colors } else { Vec::new() }, set.list, parts))
}

// ---- glTF / GLB ----------------------------------------------------------------------------------

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn mat4_identity() -> Mat4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// glTF stores matrices column-major; our row-vector `Mat4` is its transpose.
fn gltf_node_matrix(node: &serde_json::Value) -> Mat4 {
    if let Some(m) = node.get("matrix").and_then(|m| m.as_array()).filter(|m| m.len() == 16) {
        let f: Vec<f32> = m.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect();
        return std::array::from_fn(|r| std::array::from_fn(|c| f[r * 4 + c]));
    }
    let get = |k: &str, n: usize, d: &[f32]| -> Vec<f32> {
        node.get(k).and_then(|v| v.as_array()).filter(|a| a.len() == n).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_else(|| d.to_vec())
    };
    let (t, q, s) = (get("translation", 3, &[0.0; 3]), get("rotation", 4, &[0.0, 0.0, 0.0, 1.0]), get("scale", 3, &[1.0; 3]));
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    // Rotation matrix (column vectors) scaled, then transposed into the row-vector convention.
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = mat4_identity();
    for i in 0..3 {
        for j in 0..3 {
            m[j][i] = r[i][j] * s[j];
        }
    }
    m[3] = [t[0], t[1], t[2], 1.0];
    m
}

fn transform_point(p: [f32; 3], m: &Mat4) -> [f32; 3] {
    [
        p[0] * m[0][0] + p[1] * m[1][0] + p[2] * m[2][0] + m[3][0],
        p[0] * m[0][1] + p[1] * m[1][1] + p[2] * m[2][1] + m[3][1],
        p[0] * m[0][2] + p[1] * m[1][2] + p[2] * m[2][2] + m[3][2],
    ]
}

/// "my%20texture.png" -> "my texture.png".
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Load glTF image `ii`: embedded in a buffer view, a `data:` URI, or a file next to the model.
fn gltf_image(images: &[serde_json::Value], ii: usize, buffers: &[Vec<u8>], views: &[serde_json::Value], dir: Option<&Path>, set: &mut TextureSet) -> Option<usize> {
    let img = images.get(ii)?;
    let ext_of = |mime: &str| if mime.contains("png") { "png" } else if mime.contains("jpeg") || mime.contains("jpg") { "jpg" } else if mime.contains("webp") { "webp" } else { "" };
    if let Some(uri) = img.get("uri").and_then(|u| u.as_str()) {
        if uri.starts_with("data:") {
            let (head, data) = uri.split_once(',')?;
            let bytes = base64_decode(data)?;
            return set.push(load_texture_bytes(&bytes, ext_of(head))?);
        }
        return set.from_file(dir?, &percent_decode(uri));
    }
    let view = views.get(img.get("bufferView")?.as_u64()? as usize)?;
    let buf = buffers.get(view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize)?;
    let off = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let len = view.get("byteLength")?.as_u64()? as usize;
    let bytes = buf.get(off..off.checked_add(len)?)?;
    set.push(load_texture_bytes(bytes, ext_of(img.get("mimeType").and_then(|m| m.as_str()).unwrap_or("")))?)
}

pub fn parse_gltf(bytes: &[u8], dir: Option<&Path>) -> Result<Mesh, String> {
    use serde_json::Value;
    // GLB container: 12-byte header, then chunks (JSON, BIN).
    let (json_bytes, glb_bin): (&[u8], Option<&[u8]>) = if bytes.starts_with(b"glTF") {
        let mut off = 12;
        let (mut json, mut bin) = (None, None);
        while off + 8 <= bytes.len() {
            let len = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]]) as usize;
            let kind = &bytes[off + 4..off + 8];
            let data = bytes.get(off + 8..off + 8 + len).ok_or("glb: chunk runs past the end of the file")?;
            match kind {
                b"JSON" => json = Some(data),
                b"BIN\0" => bin = Some(data),
                _ => {}
            }
            off += 8 + len.div_ceil(4) * 4;
        }
        (json.ok_or("glb: no JSON chunk")?, bin)
    } else {
        (bytes, None)
    };
    let doc: Value = serde_json::from_slice(json_bytes).map_err(|e| format!("gltf: {e}"))?;
    let arr = |k: &str| doc.get(k).and_then(|v| v.as_array()).cloned().unwrap_or_default();

    // Buffers: the GLB's binary chunk, a data: URI, or a file next to the .gltf.
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for (i, b) in arr("buffers").iter().enumerate() {
        match b.get("uri").and_then(|u| u.as_str()) {
            None => buffers.push(glb_bin.filter(|_| i == 0).ok_or("gltf: buffer without data")?.to_vec()),
            Some(uri) if uri.starts_with("data:") => buffers.push(base64_decode(uri.split_once(',').map(|x| x.1).unwrap_or("")).ok_or("gltf: bad data URI")?),
            Some(uri) => {
                let name = percent_decode(uri);
                let p = dir.ok_or("gltf: external buffer without a folder")?.join(name);
                buffers.push(crate::progress::read_file(&p).map_err(|e| format!("gltf: can't read {}: {e}", p.display()))?);
            }
        }
    }
    let views = arr("bufferViews");
    let accessors = arr("accessors");
    // Read accessor `ai` as `comps` floats per element (positions/normals), or as integers (indices).
    let read = |ai: usize, comps: usize| -> Result<Vec<f64>, String> {
        let a = accessors.get(ai).ok_or("gltf: bad accessor")?;
        let count = a.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let ctype = a.get("componentType").and_then(|v| v.as_u64()).unwrap_or(0);
        let normalized = a.get("normalized").and_then(|v| v.as_bool()).unwrap_or(false);
        let size = match ctype {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err("gltf: unknown component type".into()),
        };
        let Some(view) = a.get("bufferView").and_then(|v| v.as_u64()).and_then(|v| views.get(v as usize)) else {
            return Ok(vec![0.0; count * comps]); // sparse / zero-filled accessor
        };
        let buf = buffers.get(view.get("buffer").and_then(|v| v.as_u64()).unwrap_or(0) as usize).ok_or("gltf: bad buffer")?;
        let base = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize + a.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let stride = view.get("byteStride").and_then(|v| v.as_u64()).map(|s| s as usize).filter(|&s| s > 0).unwrap_or(size * comps);
        let mut out = Vec::with_capacity(count * comps);
        for e in 0..count {
            for c in 0..comps {
                let o = base + e * stride + c * size;
                let b = buf.get(o..o + size).ok_or("gltf: accessor runs past its buffer")?;
                out.push(match ctype {
                    5120 => (b[0] as i8 as f64) / if normalized { 127.0 } else { 1.0 },
                    5121 => b[0] as f64 / if normalized { 255.0 } else { 1.0 },
                    5122 => (i16::from_le_bytes([b[0], b[1]]) as f64) / if normalized { 32767.0 } else { 1.0 },
                    5123 => u16::from_le_bytes([b[0], b[1]]) as f64 / if normalized { 65535.0 } else { 1.0 },
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
                });
            }
        }
        Ok(out)
    };

    // Materials: the colour and picture of each, when textures are wanted.
    struct MatInfo {
        texture: Option<usize>,
        normal_map: Option<usize>,
        texcoord: u64,
        color: [f32; 3],
        alpha_cut: bool,
    }
    let want = crate::model_textures_enabled();
    let mut set = TextureSet::new();
    let mut mats: Vec<MatInfo> = Vec::new();
    if want {
        let (images, gtex) = (arr("images"), arr("textures"));
        let mut image_cache: HashMap<usize, Option<usize>> = HashMap::new();
        for m in arr("materials") {
            let sg = m.pointer("/extensions/KHR_materials_pbrSpecularGlossiness");
            let pbr = m.get("pbrMetallicRoughness");
            let (factor, tex) = match sg {
                Some(sg) => (sg.get("diffuseFactor"), sg.get("diffuseTexture")),
                None => (pbr.and_then(|p| p.get("baseColorFactor")), pbr.and_then(|p| p.get("baseColorTexture"))),
            };
            let f: Vec<f32> = factor.and_then(|f| f.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(1.0) as f32).collect()).unwrap_or_default();
            let color = if f.len() >= 3 { [linear_to_srgb(f[0]), linear_to_srgb(f[1]), linear_to_srgb(f[2])] } else { [1.0; 3] };
            let alpha_cut = matches!(m.get("alphaMode").and_then(|a| a.as_str()), Some("MASK") | Some("BLEND"));
            let texcoord = tex.and_then(|t| t.get("texCoord")).and_then(|t| t.as_u64()).unwrap_or(0);
            let source = tex.and_then(|t| t.get("index")).and_then(|t| t.as_u64()).and_then(|ti| gtex.get(ti as usize)).and_then(|t| t.get("source")).and_then(|t| t.as_u64());
            let texture = source.and_then(|ii| *image_cache.entry(ii as usize).or_insert_with(|| gltf_image(&images, ii as usize, &buffers, &views, dir, &mut set)));
            let nsource = m.get("normalTexture").and_then(|t| t.get("index")).and_then(|t| t.as_u64()).and_then(|ti| gtex.get(ti as usize)).and_then(|t| t.get("source")).and_then(|t| t.as_u64());
            let normal_map = nsource.and_then(|ii| *image_cache.entry(ii as usize).or_insert_with(|| gltf_image(&images, ii as usize, &buffers, &views, dir, &mut set)));
            mats.push(MatInfo { texture, normal_map, texcoord, color, alpha_cut: alpha_cut && texture.is_some() });
        }
    }

    let meshes = arr("meshes");
    let nodes = arr("nodes");
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[u8; 4]> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let (mut any_uv, mut any_color) = (false, false);

    let mut add_mesh = |mi: usize, m: &Mat4| -> Result<(), String> {
        let mesh = meshes.get(mi).ok_or("gltf: bad mesh index")?;
        for prim in mesh.get("primitives").and_then(|p| p.as_array()).cloned().unwrap_or_default() {
            let mode = prim.get("mode").and_then(|v| v.as_u64()).unwrap_or(4);
            if mode != 4 && mode != 5 && mode != 6 {
                continue; // points and lines
            }
            let attrs = prim.get("attributes").ok_or("gltf: primitive without attributes")?;
            let Some(pa) = attrs.get("POSITION").and_then(|v| v.as_u64()) else { continue };
            let pos = read(pa as usize, 3)?;
            let base = positions.len() as u32;
            let nrm = match attrs.get("NORMAL").and_then(|v| v.as_u64()) {
                Some(na) => Some(read(na as usize, 3)?),
                None => None,
            };
            let count = pos.len() / 3;
            let mat = prim.get("material").and_then(|v| v.as_u64()).and_then(|i| mats.get(i as usize));
            if want {
                // Texture coordinates (the set the material asks for) and vertex colours, one entry per vertex.
                let uv_data = match attrs.get(format!("TEXCOORD_{}", mat.map(|m| m.texcoord).unwrap_or(0))).and_then(|v| v.as_u64()) {
                    Some(a) => Some(read(a as usize, 2)?),
                    None => None,
                };
                let col_data = match attrs.get("COLOR_0").and_then(|v| v.as_u64()) {
                    Some(a) => {
                        let comps = if accessors.get(a as usize).and_then(|x| x.get("type")).and_then(|t| t.as_str()) == Some("VEC3") { 3 } else { 4 };
                        Some((read(a as usize, comps)?, comps))
                    }
                    None => None,
                };
                any_uv |= uv_data.is_some();
                any_color |= col_data.is_some();
                for i in 0..count {
                    uvs.push(uv_data.as_ref().and_then(|u| u.get(i * 2 + 1).map(|_| [u[i * 2] as f32, u[i * 2 + 1] as f32])).unwrap_or([0.0, 0.0]));
                    colors.push(match &col_data {
                        Some((c, k)) if c.len() >= (i + 1) * k => {
                            let q = |x: f64| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                            [q(c[i * k]), q(c[i * k + 1]), q(c[i * k + 2]), if *k == 4 { q(c[i * k + 3]) } else { 255 }]
                        }
                        _ => [255, 255, 255, 255],
                    });
                }
            }
            let index_start = indices.len() as u32;
            // Normals are rotated by the node's matrix (assuming uniform scale, which is what almost every file has).
            let mut rot = *m;
            rot[3] = [0.0, 0.0, 0.0, 1.0];
            for i in 0..count {
                positions.push(transform_point([pos[i * 3] as f32, pos[i * 3 + 1] as f32, pos[i * 3 + 2] as f32], m));
                normals.push(match &nrm {
                    Some(n) => normalize(transform_point([n[i * 3] as f32, n[i * 3 + 1] as f32, n[i * 3 + 2] as f32], &rot)),
                    None => [0.0; 3],
                });
            }
            let idx: Vec<u32> = match prim.get("indices").and_then(|v| v.as_u64()) {
                Some(ia) => read(ia as usize, 1)?.into_iter().map(|v| v as u32).collect(),
                None => (0..count as u32).collect(),
            };
            match mode {
                4 => indices.extend(idx.chunks_exact(3).flat_map(|t| t.iter().map(|&i| base + i))),
                5 => (0..idx.len().saturating_sub(2)).for_each(|k| {
                    let (a, b, c) = if k % 2 == 0 { (idx[k], idx[k + 1], idx[k + 2]) } else { (idx[k + 1], idx[k], idx[k + 2]) };
                    indices.extend([base + a, base + b, base + c]);
                }),
                _ => (1..idx.len().saturating_sub(1)).for_each(|k| indices.extend([base + idx[0], base + idx[k], base + idx[k + 1]])),
            }
            if indices.len() / 3 > MAX_TRIANGLES {
                return Err(format!("model too large to open (more than {MAX_TRIANGLES} triangles)"));
            }
            if want {
                let added = indices.len() as u32 - index_start;
                if added > 0 {
                    parts.push(Part {
                        first: index_start,
                        count: added,
                        texture: mat.and_then(|m| m.texture),
                        color: mat.map(|m| m.color).unwrap_or(BASE_COLOR),
                        alpha_cut: mat.map(|m| m.alpha_cut).unwrap_or(false),
                        normal_map: mat.and_then(|m| m.normal_map),
                    });
                }
            }
        }
        Ok(())
    };

    // Walk the default scene's node tree applying transforms; without scenes, just add every mesh as it is.
    let scene_roots: Vec<usize> = {
        let si = doc.get("scene").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        match arr("scenes").get(si).and_then(|s| s.get("nodes")).and_then(|n| n.as_array()) {
            Some(n) => n.iter().filter_map(|v| v.as_u64()).map(|v| v as usize).collect(),
            None => Vec::new(),
        }
    };
    if scene_roots.is_empty() {
        for mi in 0..meshes.len() {
            add_mesh(mi, &mat4_identity())?;
        }
    } else {
        let mut stack: Vec<(usize, Mat4, u32)> = scene_roots.into_iter().map(|n| (n, mat4_identity(), 0)).collect();
        while let Some((ni, parent, depth)) = stack.pop() {
            let Some(node) = nodes.get(ni) else { continue };
            if depth > 64 {
                continue; // a cycle in a damaged file
            }
            let world = mat_mul(&gltf_node_matrix(node), &parent);
            if let Some(mi) = node.get("mesh").and_then(|v| v.as_u64()) {
                add_mesh(mi as usize, &world)?;
            }
            for c in node.get("children").and_then(|c| c.as_array()).into_iter().flatten().filter_map(|v| v.as_u64()) {
                stack.push((c as usize, world, depth + 1));
            }
        }
    }
    let mesh = Mesh::build(positions, normals, indices, true)?;
    Ok(mesh.dressed(if any_uv { uvs } else { Vec::new() }, if any_color { colors } else { Vec::new() }, set.list, parts))
}

// ---- FBX (binary) --------------------------------------------------------------------------------

/// Where a texture of an FBX material comes from: bytes stored inside the file, or a file name.
fn fbx_texture(mat: &fbxcel_dom::v7400::object::material::MaterialHandle<'_>, dir: Option<&Path>, set: &mut TextureSet) -> Option<usize> {
    let clip = mat.diffuse_texture()?.video_clip()?;
    let name = clip.relative_filename().unwrap_or("");
    let ext = Path::new(&name.replace('\\', "/")).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    if let Some(content) = clip.content().filter(|c| c.len() > 16) {
        if let Some(t) = load_texture_bytes(content, &ext) {
            return set.push(t);
        }
    }
    set.from_file(dir?, name)
}

pub fn parse_fbx(bytes: &[u8], dir: Option<&Path>) -> Result<Mesh, String> {
    use fbxcel_dom::any::AnyDocument;
    use fbxcel_dom::v7400::data::mesh::layer::TypedLayerElementHandle;
    use fbxcel_dom::v7400::object::{geometry::TypedGeometryHandle, TypedObjectHandle};
    if !bytes.starts_with(b"Kaydara FBX Binary") {
        return Err("only binary FBX files are supported (this one is the ASCII flavour)".into());
    }
    let doc = match AnyDocument::from_seekable_reader(std::io::Cursor::new(bytes)).map_err(|e| format!("fbx: {e}"))? {
        AnyDocument::V7400(_, doc) => doc,
        _ => return Err("this FBX version is not supported (7.x only)".into()),
    };
    let want = dir.is_some() && crate::model_textures_enabled();
    let mut set = TextureSet::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    let mut any_uv = false;
    for obj in doc.objects() {
        let TypedObjectHandle::Geometry(TypedGeometryHandle::Mesh(mesh)) = obj.get_typed() else { continue };
        let pv = mesh.polygon_vertices().map_err(|e| format!("fbx: {e}"))?;
        let base = positions.len() as u32;

        // Texture coordinates and the material of each polygon live in the geometry's layers.
        let (mut uv_layer, mut mat_layer) = (None, None);
        if want {
            for layer in mesh.layers() {
                for entry in layer.layer_element_entries() {
                    match entry.typed_layer_element() {
                        Ok(TypedLayerElementHandle::Uv(h)) if uv_layer.is_none() => uv_layer = h.uv().ok(),
                        Ok(TypedLayerElementHandle::Material(h)) if mat_layer.is_none() => mat_layer = h.materials().ok(),
                        _ => {}
                    }
                }
            }
        }
        if let Some(uv_layer) = uv_layer {
            // With coordinates, a vertex is a control point *and* its coordinates (seams split vertices).
            let tris = pv
                .triangulate_each(|_, poly, out| {
                    for k in 1..poly.len().saturating_sub(1) {
                        out.push([poly[0], poly[k], poly[k + 1]]);
                    }
                    Ok(())
                })
                .map_err(|e| format!("fbx: {e}"))?;
            let materials: Vec<_> = mesh.models().next().map(|m| m.materials().collect()).unwrap_or_default();
            let mut welded: HashMap<(u32, u32, u32), u32> = HashMap::new();
            let mut buckets: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
            let tri_vis: Vec<_> = tris.triangle_vertex_indices().collect();
            for tri in tri_vis.chunks_exact(3) {
                let material = mat_layer.and_then(|m| m.material_index(&tris, tri[0]).ok()).map(|i| i.to_u32()).unwrap_or(0);
                let mut corner = [0u32; 3];
                let mut ok = true;
                for (c, &tv) in tri.iter().enumerate() {
                    let (Some(cp), Some(p)) = (tris.control_point_index(tv), tris.control_point(tv)) else {
                        ok = false;
                        break;
                    };
                    let uv = uv_layer.uv(&tris, tv).map(|t| [t.x as f32, 1.0 - t.y as f32]).unwrap_or([0.0, 0.0]);
                    corner[c] = *welded.entry((cp.to_u32(), uv[0].to_bits(), uv[1].to_bits())).or_insert_with(|| {
                        positions.push([p.x as f32, p.y as f32, p.z as f32]);
                        uvs.push(uv);
                        (positions.len() - 1) as u32
                    });
                }
                if ok {
                    buckets.entry(material).or_default().extend(corner);
                }
            }
            // `uvs` has an entry for every vertex of the meshes so far (plain meshes get zeros below).
            any_uv = true;
            for (material, tris_of) in buckets {
                let first = indices.len() as u32;
                indices.extend_from_slice(&tris_of);
                let texture = materials.get(material as usize).and_then(|m| fbx_texture(m, dir, &mut set));
                parts.push(Part { first, count: tris_of.len() as u32, texture, color: if texture.is_some() { [1.0; 3] } else { BASE_COLOR }, alpha_cut: false, normal_map: None });
            }
        } else {
            let cps: Vec<[f32; 3]> = pv.raw_control_points().map_err(|e| format!("fbx: {e}"))?.map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect();
            let n = cps.len() as u32;
            positions.extend(cps);
            uvs.resize(positions.len(), [0.0, 0.0]);
            // A negative index ends a polygon and is stored as !index.
            let first = indices.len() as u32;
            let mut poly: Vec<u32> = Vec::new();
            for &raw in pv.raw_polygon_vertices() {
                let (i, last) = if raw < 0 { ((!raw) as u32, true) } else { (raw as u32, false) };
                poly.push(base + i.min(n.saturating_sub(1)));
                if last {
                    for k in 1..poly.len().saturating_sub(1) {
                        indices.extend([poly[0], poly[k], poly[k + 1]]);
                    }
                    poly.clear();
                }
            }
            if want && indices.len() as u32 > first {
                parts.push(Part { first, count: indices.len() as u32 - first, texture: None, color: BASE_COLOR, alpha_cut: false, normal_map: None });
            }
        }
        if indices.len() / 3 > MAX_TRIANGLES {
            return Err(format!("model too large to open (more than {MAX_TRIANGLES} triangles)"));
        }
    }
    uvs.resize(positions.len(), [0.0, 0.0]);
    let mesh = Mesh::build(positions, Vec::new(), indices, true)?;
    Ok(mesh.dressed(if any_uv { uvs } else { Vec::new() }, Vec::new(), set.list, parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binary STL of a unit cube spanning 0..1 (12 triangles).
    pub(crate) fn cube_stl() -> Vec<u8> {
        let v = |x: f32, y: f32, z: f32| [x, y, z];
        let corners = [v(0., 0., 0.), v(1., 0., 0.), v(1., 1., 0.), v(0., 1., 0.), v(0., 0., 1.), v(1., 0., 1.), v(1., 1., 1.), v(0., 1., 1.)];
        let quads = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [2, 3, 7, 6], [1, 2, 6, 5], [0, 4, 7, 3]];
        let mut tris = Vec::new();
        for q in quads {
            tris.push([corners[q[0]], corners[q[1]], corners[q[2]]]);
            tris.push([corners[q[0]], corners[q[2]], corners[q[3]]]);
        }
        let mut out = vec![0u8; 80];
        out.extend((tris.len() as u32).to_le_bytes());
        for t in tris {
            out.extend([0u8; 12]); // normal left zero: must be recomputed
            for p in t {
                for c in p {
                    out.extend(c.to_le_bytes());
                }
            }
            out.extend([0u8; 2]);
        }
        out
    }

    // ---- 3D style: camera, light, shadow -----------------------------------------------------------

    fn project(m: &Mat4, p: [f32; 3]) -> [f32; 4] {
        let mut o = [0.0; 4];
        for j in 0..4 {
            o[j] = p[0] * m[0][j] + p[1] * m[1][j] + p[2] * m[2][j] + m[3][j];
        }
        o
    }

    #[test]
    fn default_style_matches_the_old_headlight() {
        let cube = parse_stl(&cube_stl()).unwrap();
        let cam = camera(&cube, &Orbit::default(), 1.5);
        for k in 0..3 {
            assert!((cam.light_view[k] - normalize(LIGHT_DIR)[k]).abs() < 0.01, "axis {k}: {:?}", cam.light_view);
        }
        assert!(cam.shadow_mvp.is_none(), "no shadow unless asked for");
        // The model's centre is on the view axis: it projects to the middle of the screen.
        let c = project(&cam.mvp, cube.centre());
        assert!(c[0].abs() < 1e-4 && c[1].abs() < 1e-4, "{c:?}");
    }

    #[test]
    fn panning_slides_the_picture_by_the_pointer_distance() {
        let cube = parse_stl(&cube_stl()).unwrap();
        let mut orbit = Orbit::default();
        let before = project(&camera(&cube, &orbit, 1.5).mvp, cube.centre());
        orbit.pan_by(100.0, -50.0, 1000.0, 500.0); // right 100 px, up 50 px in a 1000x500 window
        let after = project(&camera(&cube, &orbit, 1.5).mvp, cube.centre());
        let (dx, dy) = ((after[0] / after[3]) - (before[0] / before[3]), (after[1] / after[3]) - (before[1] / before[3]));
        assert!((dx - 0.2).abs() < 1e-4 && (dy - 0.2).abs() < 1e-4, "{dx} {dy}"); // 100/1000 * 2 = 0.2 NDC in both directions
        assert_eq!(Orbit::default().pan, [0.0, 0.0], "reset puts it back");
    }

    #[test]
    fn quarter_turns_compose_and_cancel() {
        let id = ModelStyle::default().rotation();
        let turned = |t: [u8; 3]| ModelStyle { turns: t, ..ModelStyle::default() }.rotation();
        // A quarter turn about Z sends +X to +Y (row-vector convention).
        let r = turned([0, 0, 1]);
        let x = [1.0 * r[0][0] + r[1][0] * 0.0, r[0][1], r[0][2]];
        assert!((x[0]).abs() < 1e-6 && (x[1] - 1.0).abs() < 1e-6 && x[2].abs() < 1e-6, "{x:?}");
        // Four quarter turns are no turn at all, and 4 is the same as 0.
        assert_eq!(turned([4, 4, 4]), id);
        let twice = mat_mul(&turned([1, 0, 0]), &turned([1, 0, 0]));
        assert_eq!(twice, turned([2, 0, 0]));
    }

    #[test]
    fn turning_the_model_moves_it_on_screen_but_keeps_it_centred() {
        let cube = parse_stl(&cube_stl()).unwrap();
        let plain = camera_styled(&cube, &Orbit::default(), 1.0, &ModelStyle::default());
        let turned = camera_styled(&cube, &Orbit::default(), 1.0, &ModelStyle { turns: [1, 0, 0], ..ModelStyle::default() });
        let corner = [0.0, 0.0, 1.0];
        assert_ne!(project(&plain.mvp, corner), project(&turned.mvp, corner));
        let c = project(&turned.mvp, cube.centre());
        assert!(c[0].abs() < 1e-4 && c[1].abs() < 1e-4);
    }

    #[test]
    fn orthographic_has_no_perspective_divide_and_the_same_size_at_the_centre() {
        let cube = parse_stl(&cube_stl()).unwrap();
        let persp = camera_styled(&cube, &Orbit::default(), 1.0, &ModelStyle::default());
        let ortho = camera_styled(&cube, &Orbit::default(), 1.0, &ModelStyle { ortho: true, ..ModelStyle::default() });
        let p = [0.7, 0.2, 0.6];
        assert!((project(&ortho.mvp, p)[3] - 1.0).abs() < 1e-5, "w stays 1");
        assert!(project(&persp.mvp, p)[3] > 1.5, "perspective divides by distance");
        // A point beside the centre lands at about the same screen x either way (the ortho box is sized to match).
        let (a, b) = (project(&persp.mvp, [0.55, 0.5, 0.5]), project(&ortho.mvp, [0.55, 0.5, 0.5]));
        assert!((a[0] / a[3] - b[0]).abs() < 0.03, "{a:?} {b:?}");
    }

    #[test]
    fn world_light_stays_put_while_the_viewer_light_follows_the_camera() {
        let cube = parse_stl(&cube_stl()).unwrap();
        let spin = |style: ModelStyle, yaw: f32| camera_styled(&cube, &Orbit { yaw, ..Orbit::default() }, 1.0, &style).light_view;
        let viewer = ModelStyle::default();
        let world = ModelStyle { light: LightMode::World, ..ModelStyle::default() };
        let (v0, v1) = (spin(viewer, 0.0), spin(viewer, 1.3));
        let (w0, w1) = (spin(world, 0.0), spin(world, 1.3));
        for k in 0..3 {
            assert!((v0[k] - v1[k]).abs() < 1e-5, "viewer light is fixed in view space");
        }
        assert!((0..3).any(|k| (w0[k] - w1[k]).abs() > 0.1), "scene light moves in view space as the camera orbits");
        let head_on = spin(ModelStyle { light_az: 0.0, light_el: 0.0, ..ModelStyle::default() }, 0.4);
        assert!((head_on[2] - 1.0).abs() < 1e-5, "0/0 from the viewer shines straight at the model: {head_on:?}");
    }

    #[test]
    fn a_shadow_needs_light_above_the_floor_and_lands_on_the_floor() {
        let cube = parse_stl(&cube_stl()).unwrap(); // Z is up for STL; the cube sits on z = 0..1
        let lit = ModelStyle { shadow: true, light_el: 50.0, ..ModelStyle::default() };
        let cam = camera_styled(&cube, &Orbit::default(), 1.0, &lit);
        let sm = cam.shadow_mvp.expect("light 50 degrees up gives a shadow");
        // The same top corner, flattened: it must end up exactly where the floor plane meets the same view ray as a floor point.
        // Floor = the cube's lowest z (0). Flattening the bottom corner changes nothing; the top corner moves.
        let bottom = [0.0, 0.0, 0.0];
        let flat_bottom = project(&sm, bottom);
        let real_bottom = project(&cam.mvp, bottom);
        for k in 0..4 {
            assert!((flat_bottom[k] - real_bottom[k]).abs() < 1e-4, "points on the floor stay where they are");
        }
        let top = [0.0, 0.0, 1.0];
        assert_ne!(project(&sm, top), project(&cam.mvp, top));
        // Light at or below the horizon: nothing to cast.
        // (A light fixed to the viewer is as high as the camera is, so the horizon test needs a light fixed in the scene.)
        let low = ModelStyle { shadow: true, light: LightMode::World, light_el: 5.0, ..ModelStyle::default() };
        assert!(camera_styled(&cube, &Orbit::default(), 1.0, &low).shadow_mvp.is_none());
    }

    #[test]
    fn shading_ids_round_trip_and_cycle() {
        for m in Shading::ALL {
            assert_eq!(Shading::from_id(m.id()), Some(m));
        }
        assert_eq!(Shading::from_id(" Cartoon "), Some(Shading::Cartoon));
        assert_eq!(Shading::from_id("nope"), None);
        let mut m = Shading::Combined;
        for _ in 0..Shading::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, Shading::Combined);
    }

    #[test]
    fn mtl_normal_maps_are_found_under_every_common_key() {
        let mut out = HashMap::new();
        parse_mtl("newmtl a
map_Kd a.png
map_Bump -bm 1.0 a_n.png
newmtl b
norm b_n.png
newmtl c
bump c_n.png
", &mut out);
        assert_eq!(out["a"].map_bump.as_deref(), Some("a_n.png"));
        assert_eq!(out["b"].map_bump.as_deref(), Some("b_n.png"));
        assert_eq!(out["c"].map_bump.as_deref(), Some("c_n.png"));
        assert_eq!(out["a"].map_kd.as_deref(), Some("a.png"));
    }

    #[test]
    fn binary_stl_cube() {
        let m = parse_stl(&cube_stl()).unwrap();
        assert_eq!(m.triangles(), 12);
        assert_eq!((m.min, m.max), ([0.0; 3], [1.0; 3]));
        assert!(!m.y_up);
        assert!(m.normals.iter().all(|n| (dot(*n, *n) - 1.0).abs() < 1e-4), "normals recomputed from the faces");
        assert!((m.radius() - 0.866).abs() < 0.01);
    }

    #[test]
    fn ascii_stl_and_garbage() {
        let s = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        assert_eq!(parse_stl(s.as_bytes()).unwrap().triangles(), 1);
        assert!(parse_stl(b"nothing useful here").is_err());
        assert!(parse_stl(b"solid empty\nendsolid\n").is_err());
    }

    #[test]
    fn obj_with_quads_negative_indices_and_normals() {
        let obj = "# quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1 4//1\nf -4 -3 -2\n";
        let m = parse_obj(obj.as_bytes(), None).unwrap();
        assert_eq!(m.triangles(), 3, "the quad becomes two triangles, plus one triangle");
        assert!(m.y_up);
        assert!(m.normals.iter().all(|n| (n[2] - 1.0).abs() < 1e-4), "file normals used, missing ones computed");
        assert!(parse_obj(b"v 0 0 0\nf 1 2 3\n", None).is_err(), "faces pointing at nothing give no triangles");
    }

    #[test]
    fn ply_ascii_and_binary() {
        let ascii = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";
        assert_eq!(parse_ply(ascii.as_bytes(), None).unwrap().triangles(), 1);
        let mut bin = b"ply\nformat binary_little_endian 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar uint vertex_index\nend_header\n".to_vec();
        for p in [[0f32, 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]] {
            for c in p {
                bin.extend(c.to_le_bytes());
            }
        }
        bin.push(4);
        for i in [0u32, 1, 2, 3] {
            bin.extend(i.to_le_bytes());
        }
        let m = parse_ply(&bin, None).unwrap();
        assert_eq!(m.triangles(), 2, "a quad face is split in two");
        assert!(parse_ply(b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nend_header\n0\n", None).is_err(), "no faces");
    }

    /// A GLB with one triangle, node translated by (10, 0, 0).
    fn triangle_glb() -> Vec<u8> {
        let mut bin = Vec::new();
        for c in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            bin.extend(c.to_le_bytes());
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"translation":[10,0,0]}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}}}}]}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}}],"bufferViews":[{{"buffer":0,"byteLength":{}}}],"buffers":[{{"byteLength":{}}}]}}"#,
            bin.len(),
            bin.len()
        );
        let mut jb = json.into_bytes();
        while jb.len() % 4 != 0 {
            jb.push(b' ');
        }
        let total = 12 + 8 + jb.len() + 8 + bin.len();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend((total as u32).to_le_bytes());
        out.extend((jb.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(&jb);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(&bin);
        out
    }

    #[test]
    fn glb_triangle_with_node_transform() {
        let m = parse_gltf(&triangle_glb(), None).unwrap();
        assert_eq!(m.triangles(), 1);
        assert_eq!(m.min, [10.0, 0.0, 0.0]);
        assert_eq!(m.max, [11.0, 1.0, 0.0]);
        assert!(m.y_up);
        assert!(parse_gltf(b"glTF\x02\x00\x00\x00\x10\x00\x00\x00", None).is_err());
    }

    #[test]
    fn fbx_ascii_is_refused_politely_and_junk_is_an_error() {
        assert!(parse_fbx(b"; FBX 7.3.0 project file\n", None).err().unwrap().contains("binary"));
        assert!(parse_fbx(b"Kaydara FBX Binary  \x00\x1a\x00junk", None).is_err());
    }


    // ---- textures -------------------------------------------------------------------------------

    /// Tests that flip the global "load model textures" switch (or depend on it being on) take this lock.
    static TEXTURE_SWITCH: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 2x2 uncompressed 24-bit TGA: red, green on top; blue, white below.
    fn tga_2x2() -> Vec<u8> {
        let mut v = vec![0u8; 18];
        v[2] = 2;
        v[12] = 2;
        v[14] = 2;
        v[16] = 24;
        v[17] = 0x20; // top-left origin
        v.extend_from_slice(&[0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255]); // BGR: red, green, blue, white
        v
    }

    fn png_2x2() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255]).unwrap();
        }
        out
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nv_model_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A quad facing the viewer (+Z), OBJ style (v up), with texture coordinates covering the whole picture.
    const QUAD_OBJ: &str = "mtllib quad.mtl\nv -1 -1 0\nv 1 -1 0\nv 1 1 0\nv -1 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nvn 0 0 1\nusemtl paint\nf 1/1/1 2/2/1 3/3/1 4/4/1\n";

    #[test]
    fn obj_mtl_texture_is_loaded_and_the_preview_shows_it() {
        let _g = TEXTURE_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("obj");
        std::fs::write(dir.join("quad.obj"), QUAD_OBJ).unwrap();
        std::fs::write(dir.join("quad.mtl"), "newmtl paint\nKd 1 1 1\nmap_Kd -s 1 1 1 colours.tga\n").unwrap();
        std::fs::write(dir.join("colours.tga"), tga_2x2()).unwrap();
        let img = decode(&dir.join("quad.obj"), true).unwrap();
        let mesh = img.mesh.as_ref().unwrap();
        assert_eq!(mesh.textures.len(), 1);
        assert_eq!((mesh.textures[0].width, mesh.textures[0].height), (2, 2));
        assert_eq!(mesh.parts.len(), 1);
        assert_eq!(mesh.parts[0].texture, Some(0));
        assert_eq!(mesh.uvs.len(), mesh.positions.len());
        // OBJ's v axis points up, ours down: the bottom-left corner (-1,-1) has vt (0,0), which is the picture's bottom edge.
        let bl = mesh.positions.iter().position(|p| p[0] < 0.0 && p[1] < 0.0).unwrap();
        assert_eq!(mesh.uvs[bl], [0.0, 1.0]);
        // The thumbnail shows all four colours of the picture.
        let count = |f: &dyn Fn(&[u8]) -> bool| img.rgba.chunks_exact(4).filter(|p| f(p)).count();
        assert!(count(&|p| p[0] > p[1] + 40 && p[0] > p[2] + 40) > 50, "red quadrant");
        assert!(count(&|p| p[1] > p[0] + 40 && p[1] > p[2] + 40) > 50, "green quadrant");
        assert!(count(&|p| p[2] > p[0] + 40 && p[2] > p[1] + 40) > 50, "blue quadrant");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn textures_off_gives_the_plain_surface_and_missing_files_do_not_fail() {
        let _g = TEXTURE_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("off");
        std::fs::write(dir.join("quad.obj"), QUAD_OBJ).unwrap();
        std::fs::write(dir.join("quad.mtl"), "newmtl paint\nmap_Kd nowhere.png\n").unwrap();
        // The picture isn't there: the model still opens, without a texture.
        let mesh = decode(&dir.join("quad.obj"), true).unwrap().mesh.unwrap();
        assert!(mesh.textures.is_empty() && !mesh.is_textured());
        std::fs::write(dir.join("nowhere.png"), png_2x2()).unwrap();
        assert!(decode(&dir.join("quad.obj"), true).unwrap().mesh.unwrap().is_textured());
        crate::set_model_textures(false);
        let off = decode(&dir.join("quad.obj"), true).unwrap().mesh.unwrap();
        crate::set_model_textures(true);
        assert!(off.textures.is_empty() && off.parts.is_empty() && off.uvs.is_empty(), "setting off: nothing but the geometry");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn texture_names_with_odd_paths_and_wrong_extensions_are_found() {
        let dir = temp_dir("find");
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::write(dir.join("textures").join("Skin.PNG"), png_2x2()).unwrap();
        assert!(find_texture_file(&dir, r"C:\Users\artist\Desktop\assets\Skin.PNG").is_some(), "absolute Windows path -> the textures folder");
        assert!(find_texture_file(&dir, "skin.tga").is_some(), "same name, other extension");
        assert!(find_texture_file(&dir, "other.png").is_none());
        assert_eq!(mtl_texture_name("-s 2 2 2 -o 0 0 my skin.png").as_deref(), Some("my skin.png"));
        assert_eq!(percent_decode("my%20skin%2B.png"), "my skin+.png");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn obj_materials_become_separate_parts() {
        let _g = TEXTURE_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("parts");
        let obj = "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\nusemtl a\nf 1 2 3\nusemtl b\nf 2 4 3\nusemtl a\nf 1 2 4\n";
        std::fs::write(dir.join("m.obj"), obj).unwrap();
        std::fs::write(dir.join("m.mtl"), "newmtl a\nKd 1 0 0\nnewmtl b\nKd 0 0 1\n").unwrap();
        let mesh = decode(&dir.join("m.obj"), true).unwrap().mesh.unwrap();
        assert_eq!(mesh.triangles(), 3);
        assert_eq!(mesh.parts.len(), 2, "faces are grouped by material");
        assert_eq!((mesh.parts[0].first, mesh.parts[0].count, mesh.parts[0].color), (0, 6, [1.0, 0.0, 0.0]));
        assert_eq!((mesh.parts[1].first, mesh.parts[1].count, mesh.parts[1].color), (6, 3, [0.0, 0.0, 1.0]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn glb_with_an_embedded_png_texture() {
        let _g = TEXTURE_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
        let png = png_2x2();
        let mut bin = Vec::new();
        for c in [0f32, 0., 0., 1., 0., 0., 0., 1., 0.] {
            bin.extend(c.to_le_bytes());
        }
        for c in [0f32, 0., 1., 0., 0., 1.] {
            bin.extend(c.to_le_bytes());
        }
        let png_at = bin.len();
        bin.extend(&png);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"material":0}}]}}],
"materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}},"baseColorFactor":[1,1,1,1]}}}}],
"textures":[{{"source":0}}],"images":[{{"bufferView":2,"mimeType":"image/png"}}],
"accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}}],
"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},{{"buffer":0,"byteOffset":{png_at},"byteLength":{}}}],
"buffers":[{{"byteLength":{}}}]}}"#,
            png.len(),
            bin.len()
        );
        let mut jb = json.into_bytes();
        while jb.len() % 4 != 0 {
            jb.push(b' ');
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let total = 12 + 8 + jb.len() + 8 + bin.len();
        let mut glb = b"glTF".to_vec();
        glb.extend(2u32.to_le_bytes());
        glb.extend((total as u32).to_le_bytes());
        glb.extend((jb.len() as u32).to_le_bytes());
        glb.extend(b"JSON");
        glb.extend(&jb);
        glb.extend((bin.len() as u32).to_le_bytes());
        glb.extend(b"BIN\0");
        glb.extend(&bin);

        let mesh = parse_gltf(&glb, None).unwrap();
        assert_eq!(mesh.textures.len(), 1, "the PNG inside the GLB was decoded");
        assert_eq!(&mesh.textures[0].rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(mesh.parts.len(), 1);
        assert_eq!(mesh.parts[0].texture, Some(0));
        assert_eq!(mesh.uvs, vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    }

    #[test]
    fn ply_vertex_colours_show_in_the_preview() {
        let ply = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n-1 -1 0 255 0 0\n1 -1 0 255 0 0\n0 1 0 255 0 0\n3 0 1 2\n";
        let mesh = parse_ply(ply.as_bytes(), Some(Path::new("."))).unwrap();
        assert_eq!(mesh.colors.len(), 3);
        let rgba = render_preview(&mesh, &Orbit { yaw: 0.0, pitch: 1.5, ..Orbit::default() }, 120, 90);
        let reds = rgba.chunks_exact(4).filter(|p| p[0] > 120 && p[1] < 60 && p[2] < 60).count();
        assert!(reds > 200, "the red triangle is red, not grey: {reds} red pixels");
    }

    #[test]
    fn ply_texture_file_comment_and_corner_coordinates() {
        let _g = TEXTURE_SWITCH.lock().unwrap_or_else(|e| e.into_inner());
        let dir = temp_dir("ply");
        std::fs::write(dir.join("t.png"), png_2x2()).unwrap();
        let ply = "ply\nformat ascii 1.0\ncomment TextureFile t.png\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nproperty list uchar float texcoord\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2 6 0 0 1 0 0 1\n";
        std::fs::write(dir.join("t.ply"), ply).unwrap();
        let mesh = decode(&dir.join("t.ply"), true).unwrap().mesh.unwrap();
        assert_eq!(mesh.triangles(), 1);
        assert_eq!(mesh.uvs.len(), mesh.positions.len());
        assert!(mesh.is_textured());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dressing_drops_what_does_not_fit() {
        let mesh = parse_stl(&cube_stl()).unwrap();
        let n = mesh.positions.len();
        let ok_part = Part { first: 0, count: 6, texture: Some(3), color: [1.0; 3], alpha_cut: true, normal_map: None };
        let m = mesh.dressed(vec![[0.0, 0.0]; n + 1], vec![[1, 2, 3, 4]; n], Vec::new(), vec![ok_part, Part { first: 6, count: 100_000, texture: None, color: [1.0; 3], alpha_cut: false, normal_map: None }]);
        assert!(m.uvs.is_empty(), "wrong number of coordinates is ignored");
        assert_eq!(m.colors.len(), n);
        assert!(m.parts.is_empty(), "parts that run past the indices are all dropped");
    }

    #[test]
    fn camera_looks_at_the_model_and_the_preview_shows_it() {
        let m = parse_stl(&cube_stl()).unwrap();
        let (w, h) = (160u32, 120u32);
        let img = render_preview(&m, &Orbit::default(), w, h);
        let at = |x: u32, y: u32| &img[((y * w + x) * 4) as usize..][..3];
        assert_eq!(at(0, 0), &BACKDROP, "corner is backdrop");
        let centre = at(w / 2, h / 2);
        assert_ne!(centre, &BACKDROP, "the cube covers the centre");
        // Zooming in makes the model cover more pixels; zooming out fewer.
        let covered = |o: Orbit| render_preview(&m, &o, w, h).chunks_exact(4).filter(|p| p[..3] != BACKDROP).count();
        let (base, near, far) = (covered(Orbit::default()), covered(Orbit { zoom: 2.0, ..Orbit::default() }), covered(Orbit { zoom: 0.5, ..Orbit::default() }));
        assert!(near > base && base > far, "zoom: {far} < {base} < {near}");
        // Turning the model changes the picture.
        assert_ne!(render_preview(&m, &Orbit::default(), w, h), render_preview(&m, &Orbit { yaw: 1.5, ..Orbit::default() }, w, h));
    }

    #[test]
    fn full_decode_carries_the_mesh() {
        let dir = std::env::temp_dir().join(format!("nv_model_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("cube.stl");
        std::fs::write(&f, cube_stl()).unwrap();
        let img = decode(&f, true).unwrap();
        assert_eq!((img.width, img.height), (320, 240));
        assert_eq!(img.mesh.as_ref().map(|m| m.triangles()), Some(12));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
