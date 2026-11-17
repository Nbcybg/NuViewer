//! Dev tool: how long do the gallery's thumbnails take in a folder, cold (own empty cache)? Prints no file names.
//! `cargo run --release -p nv-core --example thumbbench -- <folder> [count]`
//! Files: `make_thumb` per picture (read + decode + shrink + cache write). Sub-folders: the "cover" the gallery shows
//! (reading the folder, picking the first picture, then its thumbnail).
use std::path::{Path, PathBuf};
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn cover(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(String, PathBuf)> = None;
    for e in rd.flatten().take(4000) {
        let Ok(t) = e.file_type() else { continue };
        if !t.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let ext = Path::new(&name).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !nv_codecs::is_supported_ext(&ext) || nv_codecs::is_raw_ext(&ext) {
            continue;
        }
        if best.as_ref().map(|(b, _)| nv_core::folder::natural_cmp(&name, b) == std::cmp::Ordering::Less).unwrap_or(true) {
            best = Some((name, e.path()));
        }
    }
    best.map(|b| b.1)
}

fn main() {
    // A private, empty thumbnail cache so every thumbnail is made, not read.
    let scratch = std::env::temp_dir().join(format!("nv_thumbbench_{}", std::process::id()));
    std::env::set_var("LOCALAPPDATA", &scratch);
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else { return eprintln!("usage: thumbbench <folder> [count]") };
    let count: usize = args.next().and_then(|c| c.parse().ok()).unwrap_or(24);
    let Ok(rd) = std::fs::read_dir(&dir) else { return eprintln!("can't read the folder") };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let t = Instant::now();
    for e in rd.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => dirs.push(p),
            Ok(_) => {
                let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                if nv_codecs::is_supported_ext(&ext) && nv_codecs::media::media_kind(&ext).is_none() {
                    files.push(p)
                }
            }
            Err(_) => {}
        }
    }
    println!("listed {} files and {} folders in {:.1} ms", files.len(), dirs.len(), ms(t));
    files.sort();
    dirs.sort();
    // The gallery's own worker pool: the same files through `Loader::thumbs` with N threads (THUMBBENCH_THREADS).
    if let Some(n) = std::env::var("THUMBBENCH_THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        let loader = nv_core::Loader::thumbs(n, std::sync::Arc::new(|| {}));
        let skip: usize = std::env::var("THUMBBENCH_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let want: Vec<PathBuf> = files.iter().skip(skip).take(count).cloned().collect();
        let t = Instant::now();
        loader.request(want.clone());
        let mut got = 0;
        while got < want.len() && t.elapsed().as_secs() < 120 {
            match loader.try_recv() {
                Some(_) => got += 1,
                None => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
        println!("pool of {n}: {got} thumbnails in {:.0} ms", ms(t));
        let _ = std::fs::remove_dir_all(&scratch);
        return;
    }
    let mut times: Vec<(f64, u64)> = Vec::new();
    let t_all = Instant::now();
    let verbose = std::env::var_os("THUMBBENCH_VERBOSE").is_some();
    for p in files.iter().take(count) {
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        // The file is read once on its own first (the cold part), then the thumbnail is made from the now cached file.
        if verbose {
            let t = Instant::now();
            let q = nv_core::thumbs::quick_thumb(p);
            println!("  quick path: {} in {:.1} ms", if q.is_some() { "an EXIF picture" } else { "none" }, ms(t));
        }
        let t = Instant::now();
        let read_ms = if verbose && std::env::var_os("THUMBBENCH_WARM").is_some() { let _ = std::fs::read(p); ms(t) } else { 0.0 };
        let t = Instant::now();
        let r = nv_core::thumbs::make_thumb(p);
        if verbose {
            println!("  .{} {:.1} MB: read {:.0} ms, make_thumb (warm file) {:.0} ms", p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(), size as f64 / 1e6, read_ms, ms(t));
        }
        times.push((ms(t) + read_ms, size));
        if r.is_err() {
            println!("  failed: {}", r.err().unwrap_or_default().chars().take(60).collect::<String>());
        }
    }
    if !times.is_empty() {
        let total: f64 = times.iter().map(|t| t.0).sum();
        let bytes: u64 = times.iter().map(|t| t.1).sum();
        let mut v: Vec<f64> = times.iter().map(|t| t.0).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("files: {} thumbnails in {:.0} ms ({:.1} ms each; median {:.1}, worst {:.1}); average file {:.1} MB", times.len(), total, total / times.len() as f64, v[v.len() / 2], v[v.len() - 1], bytes as f64 / times.len() as f64 / 1e6);
    }
    let mut ctimes: Vec<(f64, f64)> = Vec::new();
    for d in dirs.iter().take(count) {
        let t = Instant::now();
        let pick = cover(d);
        let pick_ms = ms(t);
        let t = Instant::now();
        if let Some(p) = pick {
            let _ = nv_core::thumbs::make_thumb(&p);
        }
        ctimes.push((pick_ms, ms(t)));
    }
    if !ctimes.is_empty() {
        let (a, b): (f64, f64) = ctimes.iter().fold((0.0, 0.0), |s, c| (s.0 + c.0, s.1 + c.1));
        println!("folders: {} covers: choosing the picture {:.1} ms each, its thumbnail {:.1} ms each", ctimes.len(), a / ctimes.len() as f64, b / ctimes.len() as f64);
    }
    println!("all in {:.0} ms", ms(t_all));
    let _ = std::fs::remove_dir_all(&scratch);
}
