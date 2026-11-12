//! Compile HLSL to bytecode at build time so startup doesn't load d3dcompiler or compile shaders.
use std::{env, ffi::c_void, fs, path::PathBuf};
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    // (source, entry, target, output)
    let jobs = [
        ("shaders/quad.hlsl", "vs\0", "vs_4_0\0", "quad_vs.cso"),
        ("shaders/quad.hlsl", "ps\0", "ps_4_0\0", "quad_ps.cso"),
        ("shaders/mesh.hlsl", "vs\0", "vs_4_0\0", "mesh_vs.cso"),
        ("shaders/mesh.hlsl", "ps\0", "ps_4_0\0", "mesh_ps.cso"),
    ];
    for (file, entry, target, name) in jobs {
        println!("cargo:rerun-if-changed={file}");
        let src = fs::read_to_string(file).unwrap();
        unsafe {
            let mut code = None;
            let mut errs = None;
            let r = D3DCompile(
                src.as_ptr() as *const c_void, src.len(), PCSTR::null(), None, None,
                PCSTR(entry.as_ptr()), PCSTR(target.as_ptr()), D3DCOMPILE_OPTIMIZATION_LEVEL3, 0,
                &mut code, Some(&mut errs),
            );
            if let Err(e) = r {
                let msg = errs.map(|b| String::from_utf8_lossy(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).into_owned()).unwrap_or_default();
                panic!("HLSL compile failed ({file}): {e} {msg}");
            }
            let b = code.unwrap();
            fs::write(out.join(name), std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize())).unwrap();
        }
    }
}
