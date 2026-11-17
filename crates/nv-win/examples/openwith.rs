//! `openwith <file>...`: the programs Windows offers for each file, and how long asking takes.
use std::time::Instant;

fn main() {
    nv_win::com_init();
    for f in std::env::args().skip(1) {
        let p = std::path::PathBuf::from(&f);
        let t = Instant::now();
        let h = nv_win::openwith::handlers(&p);
        let first = t.elapsed();
        let t = Instant::now();
        let _ = nv_win::openwith::handlers(&p);
        println!("{}: {} programs, first {:.0} ms, again {:.0} ms: {}", p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default(), h.len(), first.as_secs_f64() * 1000.0, t.elapsed().as_secs_f64() * 1000.0, h.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(", "));
    }
}
