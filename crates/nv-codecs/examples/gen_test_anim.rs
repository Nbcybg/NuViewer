//! Dev tool: write synthetic animations for manual/automated viewer tests.
//! `cargo run --release -p nv-codecs --example gen_test_anim -- <out_dir>`
//! Produces anim.gif (4 solid frames, 300 ms each) and anim.png (APNG, same colours).

use std::fs::File;
use std::path::Path;

const COLOURS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];
const SIZE: u16 = 320;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: gen_test_anim <out_dir>");
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();

    // GIF
    let palette: Vec<u8> = COLOURS.iter().flatten().copied().collect();
    let mut enc = gif::Encoder::new(File::create(dir.join("anim.gif")).unwrap(), SIZE, SIZE, &palette).unwrap();
    enc.set_repeat(gif::Repeat::Infinite).unwrap();
    for i in 0..COLOURS.len() {
        let frame = gif::Frame { width: SIZE, height: SIZE, delay: 30, buffer: std::borrow::Cow::Owned(vec![i as u8; SIZE as usize * SIZE as usize]), ..gif::Frame::default() };
        enc.write_frame(&frame).unwrap();
    }
    drop(enc);

    // APNG
    let mut e = png::Encoder::new(File::create(dir.join("anim.png")).unwrap(), SIZE as u32, SIZE as u32);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.set_animated(COLOURS.len() as u32, 0).unwrap();
    let mut w = e.write_header().unwrap();
    w.set_frame_delay(3, 10).unwrap();
    for c in COLOURS {
        let data: Vec<u8> = c.iter().copied().cycle().take(SIZE as usize * SIZE as usize * 3).collect();
        w.write_image_data(&data).unwrap();
    }
    w.finish().unwrap();
    println!("wrote {}", dir.display());
}
