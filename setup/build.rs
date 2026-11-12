// Compresses the release nuviewer.exe into OUT_DIR/payload.bin so the installer is one self-contained file.
// The exe comes from $NUVIEWER_EXE, else <target>/release/nuviewer.exe. If it isn't there yet the payload is empty
// (the crate still builds and its tests run; tools/package.ps1 builds nuviewer first, so a real setup always has it).
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let exe = std::env::var_os("NUVIEWER_EXE").map(PathBuf::from).or_else(|| {
        // OUT_DIR = <target>/release/build/<pkg>-<hash>/out
        out.ancestors().nth(3).map(|release| release.join("nuviewer.exe"))
    });
    println!("cargo:rerun-if-env-changed=NUVIEWER_EXE");
    let data = match exe.as_ref().and_then(|p| std::fs::read(p).ok()) {
        Some(bytes) => {
            println!("cargo:rerun-if-changed={}", exe.unwrap().display());
            bytes
        }
        None => {
            println!("cargo:warning=nuviewer.exe not found: building the setup WITHOUT a payload (build nuviewer first, or set NUVIEWER_EXE)");
            Vec::new()
        }
    };
    let packed = if data.is_empty() { Vec::new() } else { miniz_oxide::deflate::compress_to_vec(&data, 9) };
    std::fs::write(out.join("payload.bin"), &packed).unwrap();
    std::fs::write(out.join("payload.len"), data.len().to_string()).unwrap();

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
