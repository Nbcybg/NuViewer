//! Dev tool: compare a full JPEG decode with a DCT-scaled one sized for a 1264x761 window.
//! `cargo run --release -p nv-codecs --example bench_preview -- <jpeg>...`  (prints no file names)

use std::io::Cursor;
use std::time::Instant;

fn main() {
    for (n, arg) in std::env::args().skip(1).enumerate() {
        let bytes = std::fs::read(&arg).unwrap();
        // Min of several runs: the machine is shared and single timings swing by 2-4x.
        let runs = if bytes.len() > 8_000_000 { 3 } else { 7 };
        let mut full_ms = f64::MAX;
        let (mut fw, mut fh) = (0, 0);
        for _ in 0..runs {
            let t = Instant::now();
            let full = nv_codecs::decode_jpeg(&bytes);
            full_ms = full_ms.min(t.elapsed().as_secs_f64() * 1000.0);
            (fw, fh) = full.as_ref().map(|i| (i.width, i.height)).unwrap_or((0, 0));
        }

        // Best of 3 for the scaled decode to dodge one-off noise (the machine is shared).
        let mut best = f64::MAX;
        let mut shape = String::new();
        for _ in 0..runs {
            let t = Instant::now();
            let mut dec = jpeg_decoder::Decoder::new(Cursor::new(&bytes));
            dec.read_info().unwrap();
            let info = dec.info().unwrap();
            let s = (1264.0 / info.width as f64).min(761.0 / info.height as f64).min(1.0);
            let want = (((info.width as f64 * s).ceil() as u16).max(1), ((info.height as f64 * s).ceil() as u16).max(1));
            let (sw, sh) = dec.scale(want.0, want.1).unwrap();
            let px = dec.decode().unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if ms < best {
                best = ms;
                shape = format!("{}x{} -> scaled {sw}x{sh} ({:?}, {} bytes, icc={})", info.width, info.height, info.pixel_format, px.len(), dec.icc_profile().map(|p| p.len()).unwrap_or(0));
            }
        }
        println!("#{n} full(zune+icc+orient) {fw}x{fh}: min {full_ms:6.1} ms | jpeg-decoder scaled: {best:6.1} ms | {shape}");
    }
}
