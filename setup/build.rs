// Compresses the release nuviewer.exe, nugallery.exe, nueditor.exe and nuimporter.exe into OUT_DIR/payload.bin so the installer is one self-contained
// file. They come from $NUVIEWER_EXE / $NUGALLERY_EXE / $NUEDITOR_EXE, else <target>/release/. If nuviewer.exe isn't there yet the payload is
// empty (the crate still builds and its tests run; tools/package.ps1 builds both first, so a real setup always has them).
// A missing nugallery.exe or nueditor.exe is only a warning: the setup then installs what it has.
use std::path::PathBuf;

fn find(var: &str, name: &str, out: &std::path::Path) -> Option<(String, Vec<u8>)> {
    println!("cargo:rerun-if-env-changed={var}");
    // OUT_DIR = <target>/release/build/<pkg>-<hash>/out
    let path = std::env::var_os(var).map(PathBuf::from).or_else(|| out.ancestors().nth(3).map(|release| release.join(name)))?;
    let bytes = std::fs::read(&path).ok()?;
    println!("cargo:rerun-if-changed={}", path.display());
    Some((name.to_string(), bytes))
}

#[path = "../tools/winicon.rs"]
mod winicon;

fn main() {
    winicon::embed(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/icons/nuviewer.ico"));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let viewer = find("NUVIEWER_EXE", "nuviewer.exe", &out);
    let gallery = find("NUGALLERY_EXE", "nugallery.exe", &out);
    let editor = find("NUEDITOR_EXE", "nueditor.exe", &out);
    let importer = find("NUIMPORTER_EXE", "nuimporter.exe", &out);
    let quick = find("NUQUICK_EXE", "nuquick.exe", &out);
    let note = find("NUNOTE_EXE", "nunote.exe", &out);
    if viewer.is_none() {
        println!("cargo:warning=nuviewer.exe not found: building the setup WITHOUT a payload (build nuviewer first, or set NUVIEWER_EXE)");
    } else if editor.is_none() {
        println!("cargo:warning=nueditor.exe not found: the setup will install without NuEditor (build nueditor first, or set NUEDITOR_EXE)");
    }
    if viewer.is_some() && gallery.is_none() {
        println!("cargo:warning=nugallery.exe not found: the setup will install NuViewer without NuGallery (build nugallery first, or set NUGALLERY_EXE)");
    }
    // Besides the programs: the language files (one per language, see crates/nv-text/lang), the licence and the readme. Their names
    // carry the sub-folder with a '/' (`lang/th.txt`); Setup creates it, the uninstaller removes exactly these.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut extras: Vec<(String, Vec<u8>)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root.join("crates/nv-text/lang")) {
        let mut names: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
        names.sort();
        for p in names {
            println!("cargo:rerun-if-changed={}", p.display());
            if let (Some(n), Ok(b)) = (p.file_name(), std::fs::read(&p)) {
                extras.push((format!("lang/{}", n.to_string_lossy()), b));
            }
        }
    }
    for f in ["LICENSE", "README.md", "CHANGELOG.md"] {
        let p = root.join(f);
        println!("cargo:rerun-if-changed={}", p.display());
        if let Ok(b) = std::fs::read(&p) {
            extras.push((f.to_string(), b));
        }
    }
    // Layout of the blob: for every file: name length (u8), name, data length (u64 LE), data.
    let mut blob = Vec::new();
    if let Some(v) = viewer {
        for (name, data) in std::iter::once(v).chain(gallery).chain(editor).chain(importer).chain(quick).chain(note).chain(extras) {
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
