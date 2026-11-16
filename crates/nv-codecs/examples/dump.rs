//! Dev tool: decode a file and save it as a PNG (transparency shown over mid grey, long side capped at 1600).
//! `cargo run --release -p nv-codecs --example dump -- <in> <out.png>`
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let img = nv_codecs::decode_file(std::path::Path::new(&a[0])).expect("decode");
    println!("{}x{} note={:?}", img.width, img.height, img.colour_note);
    let (w, h) = (img.width as usize, img.height as usize);
    let scale = (w.max(h) as f64 / 1600.0).max(1.0);
    let (ow, oh) = ((w as f64 / scale) as usize, (h as f64 / scale) as usize);
    let mut out = Vec::with_capacity(ow * oh * 3);
    for y in 0..oh {
        for x in 0..ow {
            let p = &img.rgba[((y as f64 * scale) as usize * w + (x as f64 * scale) as usize) * 4..][..4];
            let al = p[3] as u32;
            for c in 0..3 {
                out.push(((p[c] as u32 * al + 128 * (255 - al)) / 255) as u8);
            }
        }
    }
    let f = std::fs::File::create(&a[1]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), ow as u32, oh as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&out).unwrap();
}
