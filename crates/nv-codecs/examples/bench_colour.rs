//! Dev tool: how fast is colour conversion? 12-megapixel pictures through P3 -> sRGB, CMYK -> sRGB (every intent, with and
//! without black point compensation), and the CMYK transform build time.
//! `cargo run --release -p nv-codecs --example bench_colour`
use nv_codecs::{ColourSettings, Image, Intent, Profile};
use std::time::Instant;

fn noise(n: usize, seed: u64) -> Vec<u8> {
    let mut s = seed;
    (0..n * 4)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 56) as u8
        })
        .collect()
}

fn time<T>(label: &str, runs: usize, mut f: impl FnMut() -> T) {
    let mut best = f64::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        let _ = f();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("{label:<58} {best:8.1} ms  ({:.0} MP/s)", 12.0 / (best / 1000.0));
}

fn main() {
    let (w, h) = (4000u32, 3000u32);
    let px = (w * h) as usize;
    let p3 = moxcms::ColorProfile::new_display_p3().encode().unwrap();

    println!("12 MP ({w}x{h}), min of 3 runs, all cores");
    nv_codecs::configure_colour(ColourSettings::rgb(true, None, None)).unwrap();
    let base = noise(px, 1);
    time("RGB: Display P3 (embedded) -> sRGB", 3, || {
        let mut img = Image::new(w, h, base.clone());
        img.icc = Some(p3.clone());
        img.apply_display_colour();
        img
    });
    time("(for reference) just cloning the 48 MB buffer", 3, || Image::new(w, h, base.clone()));
    let fogra = Profile::fogra39();
    let _ = fogra;
    for (label, intent, bpc) in [("CMYK FOGRA39 relative", Intent::Relative, false), ("CMYK FOGRA39 perceptual", Intent::Perceptual, false), ("CMYK FOGRA39 relative + BPC (float)", Intent::Relative, true), ("CMYK FOGRA39 absolute (float)", Intent::Absolute, false)] {
        nv_codecs::configure_colour(ColourSettings { enabled: true, output: None, untagged: None, cmyk: None, intent, bpc }).unwrap();
        // First conversion includes building the transform from the 650 KB profile.
        let t = Instant::now();
        let mut img = Image::new(256, 256, noise(256 * 256, 2));
        img.cmyk = true;
        img.apply_display_colour();
        let build = t.elapsed().as_secs_f64() * 1000.0;
        let ink = noise(px, 3);
        time(&format!("{label} (transform build + 64 kpx: {build:.0} ms)"), 3, || {
            let mut img = Image::new(w, h, ink.clone());
            img.cmyk = true;
            img.apply_display_colour();
            img
        });
    }
    nv_codecs::configure_colour(ColourSettings::rgb(true, None, None)).unwrap();
}
