//! The promises in `docs/TRUST.md` that a program can check about itself, checked on every `cargo test`:
//! NuViewer has no network code and no networking library, and it carries a licence.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").canonicalize().unwrap()
}

/// Libraries whose job is talking over a network: none may be in the build.
const NETWORK_CRATES: &[&str] = &[
    "reqwest", "ureq", "hyper", "curl", "isahc", "attohttpc", "surf", "tokio", "native-tls", "rustls", "openssl", "h2", "minreq", "tungstenite", "websocket", "socket2", "mio", "http", "httparse",
];

/// What network code looks like in our own sources and in the Windows APIs we could call.
const NETWORK_WORDS: &[&str] = &["std::net::", "TcpStream", "TcpListener", "UdpSocket", "WinHttp", "WinInet", "InternetOpen", "URLDownloadToFile", "Win32::Networking", "Windows::Web", "Windows::Networking"];

#[test]
fn no_networking_library_is_part_of_the_build() {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    let names: Vec<&str> = lock.lines().filter_map(|l| l.strip_prefix("name = \"")).filter_map(|l| l.strip_suffix('"')).collect();
    assert!(names.len() > 100, "the lock file was read");
    for n in names {
        assert!(!NETWORK_CRATES.contains(&n), "a networking library is in Cargo.lock: {n}. NuViewer promises not to talk to the network (docs/TRUST.md); if this is deliberate, change the promise first.");
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !matches!(name.as_str(), "target" | "dist" | ".git" | "docs") {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_source_file_opens_a_network_connection() {
    let mut files = Vec::new();
    rust_files(&root(), &mut files);
    assert!(files.len() > 200, "the sources were found ({})", files.len());
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    for f in files.iter().filter(|f| f.file_name() != Some(me.as_os_str())) {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for w in NETWORK_WORDS {
            assert!(!text.contains(w), "{} mentions `{w}`: NuViewer promises not to use the network (docs/TRUST.md)", f.display());
        }
    }
}

#[test]
fn the_licence_is_in_the_repository_and_in_every_crate() {
    let r = root();
    let lic = std::fs::read_to_string(r.join("LICENSE")).unwrap();
    assert!(lic.starts_with("MIT License"));
    let ws = std::fs::read_to_string(r.join("Cargo.toml")).unwrap();
    assert!(ws.contains("license = \"MIT\""));
    for member in ["crates/nv-core", "crates/nv-codecs", "nuviewer", "nugallery", "nueditor", "nuquick", "setup"] {
        let t = std::fs::read_to_string(r.join(member).join("Cargo.toml")).unwrap();
        assert!(t.contains("license.workspace = true"), "{member} does not carry the workspace licence");
    }
}
