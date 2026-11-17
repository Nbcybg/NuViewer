//! `exifthumb <jpeg>...`: does the file have a usable EXIF picture, and how big / how fast (prints no names).
use std::time::Instant;

fn main() {
    for f in std::env::args().skip(1) {
        let t = Instant::now();
        let r = nv_codecs::exif_thumbnail(std::path::Path::new(&f), 120);
        match r {
            Some(img) => println!("EXIF picture {}x{} orientation {} in {:.1} ms", img.width, img.height, img.orientation, t.elapsed().as_secs_f64() * 1000.0),
            None => println!("no usable EXIF picture ({:.1} ms)", t.elapsed().as_secs_f64() * 1000.0),
        }
    }
}
