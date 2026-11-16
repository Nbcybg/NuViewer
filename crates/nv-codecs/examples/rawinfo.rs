//! Dev tool: what the camera wrote into a RAW file that development uses (levels, white balance, colour matrices).
//! `cargo run --release -p nv-codecs --example rawinfo -- <file>...` (prints no file names; model names are printed)
use rawler::decoders::RawDecodeParams;
use rawler::rawimage::RawImageData;
use rawler::rawsource::RawSource;

fn main() {
    let camera = std::env::args().any(|a| a == "--camera");
    for (n, f) in std::env::args().skip(1).filter(|a| a != "--camera").enumerate() {
        if camera {
            println!("#{n} {:?}", nv_codecs::camera_data(std::path::Path::new(&f)).map(|c| c.summary()));
            continue;
        }
        let Ok(src) = RawSource::new(std::path::Path::new(&f)) else { println!("#{n} cannot open"); continue };
        let Ok(dec) = rawler::get_decoder(&src) else { println!("#{n} no decoder"); continue };
        let raw = match dec.raw_image(&src, &RawDecodeParams::default(), false) {
            Ok(r) => r,
            Err(e) => {
                println!("#{n} decode error {e}");
                continue;
            }
        };
        let (mn, mx, mean) = match &raw.data {
            RawImageData::Integer(d) => {
                let mut mn = u16::MAX;
                let mut mx = 0;
                let mut sum = 0u64;
                for &v in d.iter().step_by(97) {
                    mn = mn.min(v);
                    mx = mx.max(v);
                    sum += v as u64;
                }
                (mn, mx, sum as f64 / (d.len() / 97).max(1) as f64)
            }
            _ => (0, 0, 0.0),
        };
        println!("#{n} {} {} cfa={} {}x{} cpp={} bps={}", raw.clean_make, raw.clean_model, raw.camera.cfa.name, raw.width, raw.height, raw.cpp, raw.bps);
        println!("   black {:?}\n   white {:?}\n   wb {:?}\n   crop {:?} active {:?}", raw.blacklevel, raw.whitelevel, raw.wb_coeffs, raw.crop_area, raw.active_area);
        let mut keys: Vec<_> = raw.color_matrix.keys().map(|k| format!("{k:?}")).collect();
        keys.sort();
        println!("   matrices {keys:?}");
        println!("   data min {mn} max {mx} mean {mean:.0}");
    }
}

// (appended) what NuEditor shows for a RAW file: `cargo run -p nv-codecs --example rawinfo -- --camera <file>`
