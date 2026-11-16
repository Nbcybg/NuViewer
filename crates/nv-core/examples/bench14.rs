//! Dev tool: timings of the session 14 pieces (waveform of a long WAV, PDF of many pictures, archive list + unpack, Blender
//! preview). Writes its scratch files under the temp folder and removes them.   cargo run --release -p nv-core --example bench14
use std::io::Write;
use std::path::Path;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn wav(path: &Path, seconds: u32) {
    let rate = 48_000u32;
    let frames = rate as u64 * seconds as u64;
    let bytes = frames * 4; // 16-bit stereo
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    f.write_all(b"RIFF").unwrap();
    f.write_all(&((36 + bytes) as u32).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    for v in [16u32.to_le(), 0x0002_0001, rate, rate * 4, 0x0010_0004] {
        f.write_all(&v.to_le_bytes()).unwrap();
    }
    f.write_all(b"data").unwrap();
    f.write_all(&(bytes as u32).to_le_bytes()).unwrap();
    let block: Vec<u8> = (0..rate * 4 / 4).flat_map(|i| (((i as f32 * 0.03).sin() * 12000.0) as i16).to_le_bytes().into_iter().chain((((i as f32 * 0.05).sin() * 9000.0) as i16).to_le_bytes())).collect();
    for _ in 0..seconds {
        f.write_all(&block[..(rate * 4) as usize]).unwrap();
    }
}

fn main() {
    let dir = std::env::temp_dir().join("nv_bench14");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 1. Waveform: 10 minutes (110 MB, read through) and 90 minutes (1 GB, sampled).
    for (label, secs) in [("10 min WAV (110 MB)", 600u32), ("90 min WAV (1 GB)", 5400)] {
        let p = dir.join("long.wav");
        wav(&p, secs);
        let t = Instant::now();
        let img = nv_codecs::wave::waveform(&p).unwrap();
        println!("waveform of {label}: {:.0} ms ({}x{})", ms(t), img.width, img.height);
        let _ = std::fs::remove_file(&p);
    }

    // 2. PDF of 40 JPEGs (12 MP-ish synthetic) and of 20 PNGs.
    let rgb: Vec<u8> = (0..4000 * 3000).flat_map(|i| [(i % 251) as u8, (i / 4000 % 253) as u8, 90]).collect();
    let jpg = nv_codecs::encode_jpeg_rgb(&rgb, 4000, 3000, 85).unwrap();
    let mut jpgs = Vec::new();
    for i in 0..40 {
        let p = dir.join(format!("p{i:02}.jpg"));
        std::fs::write(&p, &jpg).unwrap();
        jpgs.push(p);
    }
    let t = Instant::now();
    let n = nv_core::pdf_make::make(&dir.join("jpgs.pdf"), &jpgs, nv_core::prefs::PdfPage::Fit, 88, &mut |_, _| true).unwrap();
    println!("PDF of {n} JPEGs ({} MB in, {} MB out): {:.0} ms", 40 * jpg.len() / 1_000_000, std::fs::metadata(dir.join("jpgs.pdf")).unwrap().len() / 1_000_000, ms(t));
    let mut pngs = Vec::new();
    for i in 0..20 {
        let p = dir.join(format!("q{i:02}.png"));
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 1600, 1200);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&rgb[..1600 * 1200 * 3]).unwrap();
        }
        std::fs::write(&p, out).unwrap();
        pngs.push(p);
    }
    let t = Instant::now();
    let n = nv_core::pdf_make::make(&dir.join("pngs.pdf"), &pngs, nv_core::prefs::PdfPage::A4, 88, &mut |_, _| true).unwrap();
    println!("PDF of {n} PNGs (1600x1200, A4 pages): {:.0} ms", ms(t));

    // 3. zip: pack 40 JPEGs, list, unpack all.
    let zip = dir.join("pack.zip");
    let entries = nv_core::archive::collect(&jpgs);
    let t = Instant::now();
    nv_core::archive::write_zip(&zip, &entries, 0, &mut |_, _| true).unwrap();
    println!("zip of 40 JPEGs: {:.0} ms", ms(t));
    let t = Instant::now();
    let list = nv_core::archive_tools::list(&zip, None, "").unwrap();
    println!("list {} entries (native): {:.1} ms", list.len(), ms(t));
    let t = Instant::now();
    nv_core::archive_tools::extract(&zip, &dir.join("out"), &[], None, "", &mut |_, _| true).unwrap();
    println!("unpack {} MB (native): {:.0} ms", 40 * jpg.len() / 1_000_000, ms(t));
    if let Some(z) = nv_core::archive_tools::seven_zip("") {
        let sz = dir.join("pack.7z");
        let t = Instant::now();
        let st = std::process::Command::new(&z).args(["a", "-t7z", "-mx=3", "-y", "-bd"]).arg(&sz).arg(dir.join("out").join("p00.jpg")).arg(dir.join("p01.jpg")).stdout(std::process::Stdio::null()).status().unwrap();
        println!("7z create (2 JPEGs, exit {st}): {:.0} ms", ms(t));
        let t = Instant::now();
        let l = nv_core::archive_tools::list(&sz, None, "").unwrap();
        println!("7z list {} entries: {:.0} ms", l.len(), ms(t));
        let t = Instant::now();
        nv_core::archive_tools::extract(&sz, &dir.join("out7"), &[], None, "", &mut |_, _| true).unwrap();
        println!("7z unpack: {:.0} ms", ms(t));
    }
    let _ = std::fs::remove_dir_all(&dir);
}
