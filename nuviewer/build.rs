// Embeds tools/app.manifest (visual-styles controls, per-monitor DPI, Windows 10/11 compatibility) without needing rc.exe.
#[path = "../tools/winicon.rs"]
mod winicon;

fn main() {
    winicon::embed(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/icons/nuviewer.ico"));
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
