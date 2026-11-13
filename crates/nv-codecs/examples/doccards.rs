//! Dev tool: write the picture `decode_file` makes for each given file as a PNG (`<out_dir>/<n>.png`), at thumbnail size or,
//! with `--big`, at the viewer's size. Used to look at the document cards.
//! `cargo run --release -p nv-codecs --example doccards -- <out_dir> [--big] <file>...`
fn main() {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    let mut big = false;
    for (n, f) in args.enumerate() {
        if f == "--big" {
            big = true;
            continue;
        }
        let path = std::path::Path::new(&f);
        let img = if big { nv_codecs::decode_file(path) } else { nv_codecs::decode_file_first_frame(path) };
        match img {
            Ok(img) => {
                let file = std::fs::File::create(out.join(format!("{n}.png"))).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&img.rgba).unwrap();
                println!("#{n}: {}x{}  {}", img.width, img.height, img.colour_note.clone().unwrap_or_default());
            }
            Err(e) => println!("#{n}: ERROR {e}"),
        }
    }
}
