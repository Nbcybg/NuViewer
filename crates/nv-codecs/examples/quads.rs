//! Dev tool: decode files and print the colour at the centre of each quadrant (top-left, top-right, bottom-left,
//! bottom-right). For test pictures made of four coloured blocks, this shows at a glance whether a decoder gets the
//! orientation and the colour order right. `cargo run -p nv-codecs --example quads -- <file>...`

use std::path::Path;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match nv_codecs::decode_file(path) {
            Ok(img) => {
                let (w, h) = (img.width as usize, img.height as usize);
                let at = |fx: f32, fy: f32| {
                    let (x, y) = (((w as f32 * fx) as usize).min(w - 1), ((h as f32 * fy) as usize).min(h - 1));
                    let p = &img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    format!("({:3},{:3},{:3},{:3})", p[0], p[1], p[2], p[3])
                };
                println!("{name}: {w}x{h} frames={}  TL{} TR{} BL{} BR{}", img.frame_count(), at(0.25, 0.25), at(0.75, 0.25), at(0.25, 0.75), at(0.75, 0.75));
            }
            Err(e) => println!("{name}: ERROR {e}"),
        }
    }
}
