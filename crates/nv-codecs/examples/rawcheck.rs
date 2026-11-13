//! Dev tool: does full-size development agree with half-size development? Compares the full picture, area-averaged down to the
//! half-size picture's size, with the half-size picture (RMS difference per channel in 8-bit levels, and the mean colours).
//! A wrong colour phase or a broken demosaic shows up as a large RMS. Prints no file names.
use nv_codecs::RawMode;

fn main() {
    for (n, f) in std::env::args().skip(1).enumerate() {
        let path = std::path::Path::new(&f);
        nv_codecs::set_raw_mode(RawMode::Half);
        let Ok(half) = nv_codecs::decode_file(path) else { println!("#{n} half failed"); continue };
        nv_codecs::set_raw_mode(RawMode::Full);
        let Ok(full) = nv_codecs::decode_file(path) else { println!("#{n} full failed"); continue };
        let edge = half.width.max(half.height);
        let small = nv_codecs::downscale(&full, edge);
        if (small.width, small.height) != (half.width, half.height) {
            println!("#{n} size mismatch full->{}x{} half {}x{}", small.width, small.height, half.width, half.height);
            continue;
        }
        let mut se = [0f64; 3];
        let (mut ma, mut mb) = ([0f64; 3], [0f64; 3]);
        let px = (half.width * half.height) as usize;
        for (a, b) in small.rgba.chunks_exact(4).zip(half.rgba.chunks_exact(4)) {
            for c in 0..3 {
                let d = a[c] as f64 - b[c] as f64;
                se[c] += d * d;
                ma[c] += a[c] as f64;
                mb[c] += b[c] as f64;
            }
        }
        let f = |v: [f64; 3]| format!("({:.1}, {:.1}, {:.1})", v[0] / px as f64, v[1] / px as f64, v[2] / px as f64);
        println!("#{n} {}x{}: RMS diff ({:.2}, {:.2}, {:.2}) levels; mean full {} half {}", half.width, half.height, (se[0] / px as f64).sqrt(), (se[1] / px as f64).sqrt(), (se[2] / px as f64).sqrt(), f(ma), f(mb));
    }
}
