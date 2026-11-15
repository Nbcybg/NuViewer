// Compresses the release nuviewer.exe and nugallery.exe into OUT_DIR/payload.bin so the installer is one self-contained
// file. They come from $NUVIEWER_EXE / $NUGALLERY_EXE, else <target>/release/. If nuviewer.exe isn't there yet the payload is
// empty (the crate still builds and its tests run; tools/package.ps1 builds both first, so a real setup always has them).
// A missing nugallery.exe is only a warning: the setup then installs the viewer alone.
use std::path::PathBuf;

fn find(var: &str, name: &str, out: &std::path::Path) -> Option<(String, Vec<u8>)> {
    println!("cargo:rerun-if-env-changed={var}");
    // OUT_DIR = <target>/release/build/<pkg>-<hash>/out
    let path = std::env::var_os(var).map(PathBuf::from).or_else(|| out.ancestors().nth(3).map(|release| release.join(name)))?;
    let bytes = std::fs::read(&path).ok()?;
    println!("cargo:rerun-if-changed={}", path.display());
    Some((name.to_string(), bytes))
}

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let viewer = find("NUVIEWER_EXE", "nuviewer.exe", &out);
    let gallery = find("NUGALLERY_EXE", "nugallery.exe", &out);
    if viewer.is_none() {
        println!("cargo:warning=nuviewer.exe not found: building the setup WITHOUT a payload (build nuviewer first, or set NUVIEWER_EXE)");
    } else if gallery.is_none() {
        println!("cargo:warning=nugallery.exe not found: the setup will install NuViewer without NuGallery (build nugallery first, or set NUGALLERY_EXE)");
    }
    // Layout of the blob: for every file: name length (u8), name, data length (u64 LE), data.
    let mut blob = Vec::new();
    if let Some(v) = viewer {
        for (name, data) in std::iter::once(v).chain(gallery) {
            blob.push(name.len() as u8);
            blob.extend(name.as_bytes());
            blob.extend((data.len() as u64).to_le_bytes());
            blob.extend(data);
        }
    }
    let packed = if blob.is_empty() { Vec::new() } else { miniz_oxide::deflate::compress_to_vec(&blob, 9) };
    std::fs::write(out.join("payload.bin"), &packed).unwrap();
    std::fs::write(out.join("payload.len"), blob.len().to_string()).unwrap();

    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
