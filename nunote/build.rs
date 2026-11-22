// Embeds the program icon and tools/app.manifest (per-monitor DPI, visual styles for the common controls) without needing rc.exe.
#[path = "../tools/winicon.rs"]
mod winicon;

fn main() {
    winicon::embed(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/icons/nunote.ico"));
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/app.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
