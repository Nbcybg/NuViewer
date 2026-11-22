//! Mock Cubism Core: two parameters (ParamAngleX moves the head sideways, ParamOpen is unused), two parts (PartBody, PartHead: the head's
//! opacity), four drawables: Body, Head, a hidden Mask covering the left half, and Eye clipped by that mask.
use std::ffi::{c_char, c_void, CString};
use std::sync::Mutex;

struct State {
    param_values: Vec<f32>,
    part_opacities: Vec<f32>,
    opacities: Vec<f32>,
    verts: Vec<Vec<[f32; 2]>>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u16>,
    masks: Vec<i32>,
    // pointer tables (stable: filled once)
    vert_ptrs: Vec<*const [f32; 2]>,
    uv_ptrs: Vec<*const [f32; 2]>,
    idx_ptrs: Vec<*const u16>,
    mask_ptrs: Vec<*const i32>,
}
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static CONSTANT: [u8; 4] = [0, 0, 0, 0];
static DYNAMIC: [u8; 4] = [1, 1, 1, 1]; // all visible
static TEXTURES: [i32; 4] = [0, 0, 0, 0];
static ORDERS: [i32; 4] = [1, 2, 0, 3];
static MASK_COUNTS: [i32; 4] = [0, 0, 0, 1];
static VERT_COUNTS: [i32; 4] = [4, 4, 4, 4];
static INDEX_COUNTS: [i32; 4] = [6, 6, 6, 6];
static PARAM_MIN: [f32; 2] = [-30.0, 0.0];
static PARAM_MAX: [f32; 2] = [30.0, 1.0];
static PARAM_DEF: [f32; 2] = [0.0, 0.0];
static CANVAS: [f32; 2] = [100.0, 100.0];

struct Ids(Vec<*const c_char>);
unsafe impl Sync for Ids {}
unsafe impl Send for Ids {}
fn ids(names: &[&str]) -> Ids {
    Ids(names.iter().map(|n| CString::new(*n).unwrap().into_raw() as *const c_char).collect())
}
static PARAM_IDS: std::sync::OnceLock<Ids> = std::sync::OnceLock::new();
static PART_IDS: std::sync::OnceLock<Ids> = std::sync::OnceLock::new();
static DRAW_IDS: std::sync::OnceLock<Ids> = std::sync::OnceLock::new();

fn quad(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<[f32; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn recompute(s: &mut State) {
    let dx = s.param_values[0] * 0.01;
    s.verts[0] = quad(-0.5, -0.5, 0.5, 0.5);
    s.verts[1] = quad(-0.2 + dx, 0.2, 0.2 + dx, 0.6);
    s.verts[2] = quad(-0.5, -0.5, 0.0, 0.5);
    s.verts[3] = quad(-0.3, -0.1, 0.3, 0.1);
    s.opacities = vec![1.0, s.part_opacities[1], 1.0, 1.0];
    for i in 0..4 {
        s.vert_ptrs[i] = s.verts[i].as_ptr();
    }
}

#[no_mangle]
pub extern "C" fn csmGetVersion() -> u32 {
    (5 << 24) | (0 << 16) | 0
}
#[no_mangle]
pub unsafe extern "C" fn csmGetMocVersion(a: *const c_void, size: u32) -> u32 {
    if size >= 4 && std::slice::from_raw_parts(a as *const u8, 4) == b"MOC3" { 1 } else { 0 }
}
#[no_mangle]
pub unsafe extern "C" fn csmReviveMocInPlace(a: *mut c_void, size: u32) -> *mut c_void {
    if (a as usize) % 64 != 0 || csmGetMocVersion(a, size) == 0 { std::ptr::null_mut() } else { a }
}
#[no_mangle]
pub extern "C" fn csmGetSizeofModel(_moc: *const c_void) -> u32 {
    64
}
#[no_mangle]
pub unsafe extern "C" fn csmInitializeModelInPlace(_moc: *const c_void, a: *mut c_void, size: u32) -> *mut c_void {
    if (a as usize) % 16 != 0 || size < 64 {
        return std::ptr::null_mut();
    }
    PARAM_IDS.get_or_init(|| ids(&["ParamAngleX", "ParamOpen"]));
    PART_IDS.get_or_init(|| ids(&["PartBody", "PartHead"]));
    DRAW_IDS.get_or_init(|| ids(&["Body", "Head", "Mask", "Eye"]));
    let mut s = State {
        param_values: vec![0.0; 2],
        part_opacities: vec![1.0; 2],
        opacities: vec![1.0; 4],
        verts: vec![Vec::new(); 4],
        uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        indices: vec![0, 1, 2, 0, 2, 3],
        masks: vec![2],
        vert_ptrs: vec![std::ptr::null(); 4],
        uv_ptrs: Vec::new(),
        idx_ptrs: Vec::new(),
        mask_ptrs: vec![std::ptr::null(); 4],
    };
    s.uv_ptrs = vec![s.uvs.as_ptr(); 4];
    s.idx_ptrs = vec![s.indices.as_ptr(); 4];
    s.mask_ptrs[3] = s.masks.as_ptr();
    recompute(&mut s);
    *STATE.lock().unwrap() = Some(s);
    a
}
#[no_mangle]
pub extern "C" fn csmUpdateModel(_m: *mut c_void) {
    if let Some(s) = STATE.lock().unwrap().as_mut() {
        recompute(s);
    }
}
#[no_mangle]
pub unsafe extern "C" fn csmReadCanvasInfo(_m: *const c_void, size: *mut [f32; 2], origin: *mut [f32; 2], ppu: *mut f32) {
    *size = CANVAS;
    *origin = [50.0, 50.0];
    *ppu = 100.0;
}
#[no_mangle]
pub extern "C" fn csmGetParameterCount(_m: *const c_void) -> i32 { 2 }
#[no_mangle]
pub extern "C" fn csmGetParameterIds(_m: *const c_void) -> *const *const c_char { PARAM_IDS.get().unwrap().0.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetParameterMinimumValues(_m: *const c_void) -> *const f32 { PARAM_MIN.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetParameterMaximumValues(_m: *const c_void) -> *const f32 { PARAM_MAX.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetParameterDefaultValues(_m: *const c_void) -> *const f32 { PARAM_DEF.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetParameterValues(_m: *mut c_void) -> *mut f32 { STATE.lock().unwrap().as_mut().unwrap().param_values.as_mut_ptr() }
#[no_mangle]
pub extern "C" fn csmGetPartCount(_m: *const c_void) -> i32 { 2 }
#[no_mangle]
pub extern "C" fn csmGetPartIds(_m: *const c_void) -> *const *const c_char { PART_IDS.get().unwrap().0.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetPartOpacities(_m: *mut c_void) -> *mut f32 { STATE.lock().unwrap().as_mut().unwrap().part_opacities.as_mut_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableCount(_m: *const c_void) -> i32 { 4 }
#[no_mangle]
pub extern "C" fn csmGetDrawableIds(_m: *const c_void) -> *const *const c_char { DRAW_IDS.get().unwrap().0.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableConstantFlags(_m: *const c_void) -> *const u8 { CONSTANT.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableDynamicFlags(_m: *const c_void) -> *const u8 {
    // the Mask drawable (index 2) is not visible
    static HIDDEN_MASK: [u8; 4] = [1, 1, 0, 1];
    let _ = &DYNAMIC;
    HIDDEN_MASK.as_ptr()
}
#[no_mangle]
pub extern "C" fn csmGetDrawableTextureIndices(_m: *const c_void) -> *const i32 { TEXTURES.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableRenderOrders(_m: *const c_void) -> *const i32 { ORDERS.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableOpacities(_m: *const c_void) -> *const f32 { STATE.lock().unwrap().as_ref().unwrap().opacities.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableMaskCounts(_m: *const c_void) -> *const i32 { MASK_COUNTS.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableMasks(_m: *const c_void) -> *const *const i32 { STATE.lock().unwrap().as_ref().unwrap().mask_ptrs.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableVertexCounts(_m: *const c_void) -> *const i32 { VERT_COUNTS.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableVertexPositions(_m: *const c_void) -> *const *const [f32; 2] { STATE.lock().unwrap().as_ref().unwrap().vert_ptrs.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableVertexUvs(_m: *const c_void) -> *const *const [f32; 2] { STATE.lock().unwrap().as_ref().unwrap().uv_ptrs.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableIndexCounts(_m: *const c_void) -> *const i32 { INDEX_COUNTS.as_ptr() }
#[no_mangle]
pub extern "C" fn csmGetDrawableIndices(_m: *const c_void) -> *const *const u16 { STATE.lock().unwrap().as_ref().unwrap().idx_ptrs.as_ptr() }
#[no_mangle]
pub extern "C" fn csmResetDrawableDynamicFlags(_m: *mut c_void) {}
