//! Dev tool: time the quick first-paint decode of big JPEGs. `cargo run --release -p nv-codecs --example scaledprobe -- <file>... [--win WxH]`
fn main() {
    let mut win = (2560u32, 1440u32);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--win" {
            let (w, h) = args[i + 1].split_once('x').unwrap();
            win = (w.parse().unwrap(), h.parse().unwrap());
            i += 2;
        } else {
            files.push(args[i].clone());
            i += 1;
        }
    }
    for f in files {
        let p = std::path::Path::new(&f);
        for round in 0..2 {
            let t = std::time::Instant::now();
            let r = nv_codecs::decode_jpeg_for_window(p, win);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match r {
                Some(img) => println!("round {round}: {}x{} orientation {} in {ms:.0} ms", img.width, img.height, img.orientation),
                None => println!("round {round}: None in {ms:.0} ms"),
            }
        }
        let t = std::time::Instant::now();
        let full = nv_codecs::decode_file(p).map(|i| (i.width, i.height)).ok();
        println!("full decode: {full:?} in {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}
