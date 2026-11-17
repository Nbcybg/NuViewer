//! `arclist <archive> [dest]`: list an archive the way the gallery does (and unpack it into `dest`), with timings.
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else { return eprintln!("usage: arclist <archive> [dest]") };
    let t = Instant::now();
    match nv_core::archive_tools::list_ex(Path::new(path), None, "") {
        Ok((v, damaged)) => {
            println!("listed {} entries in {:.0} ms (damaged: {damaged})", v.len(), t.elapsed().as_secs_f64() * 1000.0);
            for e in v.iter().take(5) {
                println!("  {}  {} bytes", e.name.len(), e.size);
            }
            if let Some(dest) = args.get(1) {
                let wanted: Vec<String> = v.iter().filter(|e| !e.is_dir).map(|e| e.name.clone()).collect();
                let t = Instant::now();
                let r = nv_core::archive_tools::extract(Path::new(path), Path::new(dest), &wanted, None, "", &mut |_, _| true);
                println!("unpacked in {:.0} ms: {:?}", t.elapsed().as_secs_f64() * 1000.0, r);
            }
        }
        Err(e) => println!("error: {e}"),
    }
}
