//! Live2D Cubism Core (`Live2DCubismCore.dll`), loaded by hand the first time a `.moc3` is opened.
//!
//! **The DLL is not part of NuViewer.** It is Live2D's proprietary software: it is downloaded from Live2D (Cubism SDK for Native) after
//! accepting their licence, and the licence governs passing it on (see `docs/live2d.md`). NuViewer therefore never ships it; the user puts
//! it where `Core::search_places` says, and when it is missing the answer says exactly that.
//!
//! The C API used (csmapi): `csmReviveMocInPlace` (64-byte aligned), `csmInitializeModelInPlace` (16-byte aligned), `csmUpdateModel`,
//! the parameter / part / drawable getters. Everything is read through plain arrays the Core owns; this module copies what it needs.
//! **Not run here:** no Core DLL was available on the machine this was written on, so every call below follows the published header and
//! has not been exercised; `live2d.rs` (everything around it) is tested without it.

use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    pub unsafe fn load(path: &std::path::Path) -> Option<HMODULE> {
        let w: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        LoadLibraryW(PCWSTR(w.as_ptr())).ok()
    }

    pub unsafe fn sym(m: HMODULE, name: &str) -> Option<*const c_void> {
        let c = format!("{name}\0");
        GetProcAddress(m, PCSTR(c.as_ptr())).map(|f| f as *const c_void)
    }

    pub type Module = HMODULE;
}

#[cfg(not(windows))]
mod win {
    use std::ffi::c_void;
    pub unsafe fn load(_: &std::path::Path) -> Option<Module> {
        None
    }
    pub unsafe fn sym(_: Module, _: &str) -> Option<*const c_void> {
        None
    }
    #[derive(Clone, Copy)]
    pub struct Module;
}

type Fv = unsafe extern "C" fn() -> u32;
type FMocVersion = unsafe extern "C" fn(*const c_void, u32) -> u32;
type FRevive = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type FSizeofModel = unsafe extern "C" fn(*const c_void) -> u32;
type FInitModel = unsafe extern "C" fn(*const c_void, *mut c_void, u32) -> *mut c_void;
type FUpdate = unsafe extern "C" fn(*mut c_void);
type FCanvas = unsafe extern "C" fn(*const c_void, *mut [f32; 2], *mut [f32; 2], *mut f32);
type FCount = unsafe extern "C" fn(*const c_void) -> i32;
type FIds = unsafe extern "C" fn(*const c_void) -> *const *const c_char;
type FFloatsC = unsafe extern "C" fn(*const c_void) -> *const f32;
type FFloatsM = unsafe extern "C" fn(*mut c_void) -> *mut f32;
type FIntsC = unsafe extern "C" fn(*const c_void) -> *const i32;
type FBytesC = unsafe extern "C" fn(*const c_void) -> *const u8;
type FPtrsC<T> = unsafe extern "C" fn(*const c_void) -> *const *const T;
type FReset = unsafe extern "C" fn(*mut c_void);

/// The entry points, looked up once.
pub struct Core {
    _module: win::Module,
    pub path: PathBuf,
    version: Fv,
    moc_version: FMocVersion,
    revive: FRevive,
    sizeof_model: FSizeofModel,
    init_model: FInitModel,
    update: FUpdate,
    canvas: FCanvas,
    param_count: FCount,
    param_ids: FIds,
    param_min: FFloatsC,
    param_max: FFloatsC,
    param_default: FFloatsC,
    param_values: FFloatsM,
    part_count: FCount,
    part_ids: FIds,
    part_opacities: FFloatsM,
    drawable_count: FCount,
    drawable_ids: FIds,
    drawable_constant_flags: FBytesC,
    drawable_dynamic_flags: FBytesC,
    drawable_textures: FIntsC,
    drawable_render_orders: FIntsC,
    drawable_opacities: FFloatsC,
    drawable_mask_counts: FIntsC,
    drawable_masks: FPtrsC<i32>,
    drawable_vertex_counts: FIntsC,
    drawable_vertex_positions: FPtrsC<[f32; 2]>,
    drawable_vertex_uvs: FPtrsC<[f32; 2]>,
    drawable_index_counts: FIntsC,
    drawable_indices: FPtrsC<u16>,
    reset_flags: FReset,
}

unsafe impl Send for Core {}
unsafe impl Sync for Core {}

pub const DLL_NAME: &str = "Live2DCubismCore.dll";

/// Where Live2D hands the Cubism SDK out (the licence is accepted there).
pub const DOWNLOAD_PAGE: &str = "https://www.live2d.com/en/sdk/download/native/";

static CONFIGURED: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// The path chosen in Settings (may be empty): set at start and whenever the settings change.
pub fn configure(path: &str) {
    *CONFIGURED.lock().unwrap() = path.trim().trim_matches('"').to_string();
}

/// The folder `%APPDATA%\NuViewer\live2d`, where the DLL can simply be copied to.
pub fn user_folder() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("NuViewer").join("live2d"))
}

impl Core {
    /// Where the DLL is looked for, best first: the file named by `NUVIEWER_CUBISM_CORE`, next to the program, and
    /// `%APPDATA%\NuViewer\live2d\`.
    pub fn search_places(exe_dir: Option<&Path>) -> Vec<PathBuf> {
        let mut v = Vec::new();
        let chosen = CONFIGURED.lock().unwrap().clone();
        if !chosen.is_empty() {
            v.push(PathBuf::from(chosen));
        }
        if let Some(p) = std::env::var_os("NUVIEWER_CUBISM_CORE") {
            v.push(PathBuf::from(p));
        }
        if let Some(d) = exe_dir {
            v.push(d.join(DLL_NAME));
        }
        if let Some(a) = std::env::var_os("APPDATA") {
            v.push(PathBuf::from(a).join("NuViewer").join("live2d").join(DLL_NAME));
        }
        v
    }

    /// What to tell the user when no DLL is found.
    pub fn missing_message(exe_dir: Option<&Path>) -> String {
        let places: Vec<String> = Self::search_places(exe_dir).iter().map(|p| p.display().to_string()).collect();
        format!("Live2D Cubism Core ({DLL_NAME}) was not found. NuViewer cannot ship it (Live2D's licence): download the Cubism SDK for Native from live2d.com, accept its licence, and copy Core\\dll\\windows\\x86_64\\{DLL_NAME} to one of: {}", places.join("  |  "))
    }

    /// The DLL that would be used, if any (just a file check: nothing is loaded).
    pub fn find(exe_dir: Option<&Path>) -> Option<PathBuf> {
        Self::search_places(exe_dir).into_iter().find(|p| p.is_file())
    }

    /// Load the first DLL that exists in `search_places` and has every entry point.
    pub fn load(exe_dir: Option<&Path>) -> Result<Arc<Core>, String> {
        for p in Self::search_places(exe_dir) {
            if p.is_file() {
                return unsafe { Self::load_from(&p) }.map(Arc::new);
            }
        }
        Err(Self::missing_message(exe_dir))
    }

    /// # Safety
    /// Loads and calls into a native library.
    pub unsafe fn load_from(path: &Path) -> Result<Core, String> {
        let m = win::load(path).ok_or_else(|| format!("{}: could not be loaded (wrong bitness? NuViewer is 64-bit: use the x86_64 DLL)", path.display()))?;
        macro_rules! f {
            ($name:literal) => {
                match win::sym(m, $name) {
                    Some(p) => std::mem::transmute_copy(&p),
                    None => return Err(format!("{}: has no {} (not a Cubism Core?)", path.display(), $name)),
                }
            };
        }
        Ok(Core {
            _module: m,
            path: path.to_path_buf(),
            version: f!("csmGetVersion"),
            moc_version: f!("csmGetMocVersion"),
            revive: f!("csmReviveMocInPlace"),
            sizeof_model: f!("csmGetSizeofModel"),
            init_model: f!("csmInitializeModelInPlace"),
            update: f!("csmUpdateModel"),
            canvas: f!("csmReadCanvasInfo"),
            param_count: f!("csmGetParameterCount"),
            param_ids: f!("csmGetParameterIds"),
            param_min: f!("csmGetParameterMinimumValues"),
            param_max: f!("csmGetParameterMaximumValues"),
            param_default: f!("csmGetParameterDefaultValues"),
            param_values: f!("csmGetParameterValues"),
            part_count: f!("csmGetPartCount"),
            part_ids: f!("csmGetPartIds"),
            part_opacities: f!("csmGetPartOpacities"),
            drawable_count: f!("csmGetDrawableCount"),
            drawable_ids: f!("csmGetDrawableIds"),
            drawable_constant_flags: f!("csmGetDrawableConstantFlags"),
            drawable_dynamic_flags: f!("csmGetDrawableDynamicFlags"),
            drawable_textures: f!("csmGetDrawableTextureIndices"),
            drawable_render_orders: f!("csmGetDrawableRenderOrders"),
            drawable_opacities: f!("csmGetDrawableOpacities"),
            drawable_mask_counts: f!("csmGetDrawableMaskCounts"),
            drawable_masks: f!("csmGetDrawableMasks"),
            drawable_vertex_counts: f!("csmGetDrawableVertexCounts"),
            drawable_vertex_positions: f!("csmGetDrawableVertexPositions"),
            drawable_vertex_uvs: f!("csmGetDrawableVertexUvs"),
            drawable_index_counts: f!("csmGetDrawableIndexCounts"),
            drawable_indices: f!("csmGetDrawableIndices"),
            reset_flags: f!("csmResetDrawableDynamicFlags"),
        })
    }

    /// "5.1.0" style: the Core's version (major.minor.patch packed in a u32: major << 24 | minor << 16 | patch).
    pub fn version_text(&self) -> String {
        let v = unsafe { (self.version)() };
        format!("{}.{}.{}", v >> 24, (v >> 16) & 0xFF, v & 0xFFFF)
    }
}

fn aligned(len: usize, align: usize) -> (Vec<u8>, usize) {
    let buf = vec![0u8; len + align];
    let off = (align - (buf.as_ptr() as usize % align)) % align;
    (buf, off)
}

/// One parameter of the model (the knobs motions turn).
#[derive(Clone, Debug)]
pub struct Param {
    pub id: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

/// One drawable (a textured mesh) as the Core has computed it for the current parameter values.
#[derive(Clone, Debug)]
pub struct Drawable {
    pub id: String,
    pub additive: bool,
    pub multiply: bool,
    pub double_sided: bool,
    pub inverted_mask: bool,
    pub visible: bool,
    pub texture: usize,
    pub render_order: i32,
    pub opacity: f32,
    pub masks: Vec<usize>,
    pub verts: Vec<[f32; 2]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

/// A revived `.moc3` and its model instance.
pub struct Model {
    core: Arc<Core>,
    _moc_mem: Vec<u8>,
    _model_mem: Vec<u8>,
    model: *mut c_void,
    pub params: Vec<Param>,
    pub parts: Vec<String>,
    /// Canvas in Cubism units: size, origin, pixels per unit.
    pub canvas_size: [f32; 2],
    pub canvas_origin: [f32; 2],
    pub pixels_per_unit: f32,
}

unsafe impl Send for Model {}

unsafe fn ids(f: FIds, model: *const c_void, n: i32) -> Vec<String> {
    let p = f(model);
    if p.is_null() {
        return Vec::new();
    }
    (0..n.max(0) as usize).map(|i| CStr::from_ptr(*p.add(i)).to_string_lossy().into_owned()).collect()
}

impl Model {
    /// Revive the `.moc3` bytes and make a model from them.
    pub fn new(core: Arc<Core>, moc: &[u8]) -> Result<Model, String> {
        unsafe {
            if moc.len() < 64 || &moc[..4] != b"MOC3" {
                return Err("moc3: not a .moc3 file (no MOC3 header)".into());
            }
            let file_version = (core.moc_version)(moc.as_ptr() as *const c_void, moc.len() as u32);
            if file_version == 0 {
                return Err("moc3: this Core cannot read this .moc3 (a newer Cubism Editor version than the Core: update Live2DCubismCore.dll)".into());
            }
            let (mut mem, off) = aligned(moc.len(), 64);
            mem[off..off + moc.len()].copy_from_slice(moc);
            let revived = (core.revive)(mem.as_mut_ptr().add(off) as *mut c_void, moc.len() as u32);
            if revived.is_null() {
                return Err("moc3: the Core refused this file (damaged?)".into());
            }
            let size = (core.sizeof_model)(revived) as usize;
            let (mut mmem, moff) = aligned(size, 16);
            let model = (core.init_model)(revived, mmem.as_mut_ptr().add(moff) as *mut c_void, size as u32);
            if model.is_null() {
                return Err("moc3: the Core could not make a model from this file".into());
            }
            let (mut sz, mut origin, mut ppu) = ([0f32; 2], [0f32; 2], 1f32);
            (core.canvas)(model, &mut sz, &mut origin, &mut ppu);
            let n = (core.param_count)(model);
            let pids = ids(core.param_ids, model, n);
            let (mins, maxs, defs) = ((core.param_min)(model), (core.param_max)(model), (core.param_default)(model));
            let params = pids.into_iter().enumerate().map(|(i, id)| Param { id, min: *mins.add(i), max: *maxs.add(i), default: *defs.add(i) }).collect();
            let parts = ids(core.part_ids, model, (core.part_count)(model));
            let mut m = Model { core, _moc_mem: mem, _model_mem: mmem, model, params, parts, canvas_size: sz, canvas_origin: origin, pixels_per_unit: if ppu > 0.0 { ppu } else { 1.0 } };
            m.reset_to_defaults();
            m.update();
            Ok(m)
        }
    }

    pub fn reset_to_defaults(&mut self) {
        for i in 0..self.params.len() {
            self.set_param(i, self.params[i].default);
        }
    }

    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.params.iter().position(|p| p.id == id)
    }

    pub fn part_index(&self, id: &str) -> Option<usize> {
        self.parts.iter().position(|p| p == id)
    }

    pub fn set_param(&mut self, i: usize, v: f32) {
        if let Some(p) = self.params.get(i) {
            let v = v.clamp(p.min, p.max);
            unsafe { *(self.core.param_values)(self.model).add(i) = v };
        }
    }

    pub fn param_value(&self, i: usize) -> f32 {
        if i < self.params.len() {
            unsafe { *(self.core.param_values)(self.model).add(i) }
        } else {
            0.0
        }
    }

    pub fn set_part_opacity(&mut self, i: usize, v: f32) {
        if i < self.parts.len() {
            unsafe { *(self.core.part_opacities)(self.model).add(i) = v.clamp(0.0, 1.0) };
        }
    }

    pub fn update(&mut self) {
        unsafe {
            (self.core.update)(self.model);
        }
    }

    /// Every drawable as computed by the last `update`.
    pub fn drawables(&mut self) -> Vec<Drawable> {
        unsafe {
            let c = &*self.core;
            let m = self.model;
            let n = (c.drawable_count)(m).max(0) as usize;
            let dids = ids(c.drawable_ids, m, n as i32);
            let (cf, df, tex, ro, op, mc, mk, vc, vp, vu, ic, ix) = ((c.drawable_constant_flags)(m), (c.drawable_dynamic_flags)(m), (c.drawable_textures)(m), (c.drawable_render_orders)(m), (c.drawable_opacities)(m), (c.drawable_mask_counts)(m), (c.drawable_masks)(m), (c.drawable_vertex_counts)(m), (c.drawable_vertex_positions)(m), (c.drawable_vertex_uvs)(m), (c.drawable_index_counts)(m), (c.drawable_indices)(m));
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let (nv, ni, nm) = ((*vc.add(i)).max(0) as usize, (*ic.add(i)).max(0) as usize, (*mc.add(i)).max(0) as usize);
                let (pos, uv, idx, mask) = (*vp.add(i), *vu.add(i), *ix.add(i), *mk.add(i));
                out.push(Drawable {
                    id: dids.get(i).cloned().unwrap_or_default(),
                    additive: *cf.add(i) & 1 != 0,
                    multiply: *cf.add(i) & 2 != 0,
                    double_sided: *cf.add(i) & 4 != 0,
                    inverted_mask: *cf.add(i) & 8 != 0,
                    visible: *df.add(i) & 1 != 0,
                    texture: (*tex.add(i)).max(0) as usize,
                    render_order: *ro.add(i),
                    opacity: *op.add(i),
                    masks: if mask.is_null() { Vec::new() } else { (0..nm).map(|k| *mask.add(k) as usize).collect() },
                    verts: if pos.is_null() { Vec::new() } else { (0..nv).map(|k| *pos.add(k)).collect() },
                    uvs: if uv.is_null() { Vec::new() } else { (0..nv).map(|k| *uv.add(k)).collect() },
                    indices: if idx.is_null() { Vec::new() } else { (0..ni).map(|k| *idx.add(k)).collect() },
                });
            }
            (c.reset_flags)(m);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_core_is_explained_not_a_crash() {
        let dir = std::env::temp_dir().join("nv_no_core_here");
        let e = match Core::load(Some(&dir)) {
            Ok(_) => panic!("there is no DLL in that folder"),
            Err(e) => e,
        };
        assert!(e.contains("Live2DCubismCore.dll") && e.contains("licence") && e.contains("live2d"), "{e}");
        assert!(Core::search_places(Some(&dir)).iter().any(|p| p.starts_with(&dir)));
    }

    #[test]
    fn a_file_that_is_not_a_core_is_refused() {
        let p = std::env::temp_dir().join(format!("nv_fake_core_{}.dll", std::process::id()));
        std::fs::write(&p, b"MZ not really").unwrap();
        let r = unsafe { Core::load_from(&p) };
        let _ = std::fs::remove_file(&p);
        assert!(r.is_err());
    }

    #[test]
    fn alignment_helper_aligns() {
        let (buf, off) = aligned(100, 64);
        assert_eq!((buf.as_ptr() as usize + off) % 64, 0);
        assert!(buf.len() >= off + 100);
    }
}
