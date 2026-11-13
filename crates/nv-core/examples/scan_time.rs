//! Dev tool: how long does listing and sorting a folder take? (What a second process would pay to build its own list.)
//! `cargo run --release -p nv-core --example scan_time -- <folder>...`  (prints no folder names)
use std::time::Instant;

fn main() {
    for (n, dir) in std::env::args().skip(1).enumerate() {
        let mut best = f64::MAX;
        let mut count = 0;
        for _ in 0..5 {
            let t = Instant::now();
            let list = nv_core::FolderList::open(std::path::Path::new(&dir)).map(|l| l.files.len());
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            count = list.unwrap_or(0);
        }
        println!("#{n}: {count} files listed and sorted in {best:.2} ms (best of 5, warm)");
    }
}
