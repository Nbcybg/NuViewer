//! Dev tool: write oversized or hostile images to exercise the size guards.
//! `cargo run --release -p nv-codecs --example gen_big -- <kind> <out_file>`
//! kinds: png-wide (20000x1000), png-large (12000x9000, ~108 MP), png-bomb (header claims 60000x60000),
//!        jpeg-wide (20000x500), png-noise (12000x9000 RGB noise: ~330 MB, slow to read and decode, for loading tests)

use std::fs::File;
use std::io::Write;

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}

fn gray_png(path: &str, w: u32, h: u32) {
    let mut enc = png::Encoder::new(File::create(path).unwrap(), w, h);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut writer = enc.write_header().unwrap();
    // A smooth diagonal gradient with a coarse grid, written row by row so we never hold the whole image.
    let mut stream = writer.stream_writer().unwrap();
    let mut row = vec![0u8; w as usize];
    for y in 0..h {
        for (x, p) in row.iter_mut().enumerate() {
            let grid = (x as u32 % 500 < 3) || (y % 500 < 3);
            *p = if grid { 255 } else { ((x as u64 * 200 / w as u64) as u8).wrapping_add((y as u64 * 55 / h as u64) as u8) };
        }
        stream.write_all(&row).unwrap();
    }
    stream.finish().unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (kind, path) = (args.next().expect("kind"), args.next().expect("out file"));
    match kind.as_str() {
        "png-wide" => gray_png(&path, 20_000, 1_000),
        "png-large" => gray_png(&path, 12_000, 9_000),
        "png-noise" => {
            let (w, h) = (12_000u32, 9_000u32);
            let mut enc = png::Encoder::new(File::create(&path).unwrap(), w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fastest);
            let mut writer = enc.write_header().unwrap();
            let mut stream = writer.stream_writer().unwrap();
            let mut state = 0x2545_F491_4F6C_DD1Du64;
            let mut row = vec![0u8; w as usize * 3];
            for _ in 0..h {
                for b in row.iter_mut() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    *b = (state >> 24) as u8;
                }
                stream.write_all(&row).unwrap();
            }
            stream.finish().unwrap();
        }
        "png-bomb" => {
            // Valid signature + IHDR for a 60000x60000 1-bit image, then nothing: any decoder that trusts the header
            // and allocates would try to reserve gigabytes.
            let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut ihdr = Vec::new();
            ihdr.extend(60_000u32.to_be_bytes());
            ihdr.extend(60_000u32.to_be_bytes());
            ihdr.extend([1, 0, 0, 0, 0]); // 1-bit grayscale, deflate, no filter, no interlace
            chunk(&mut out, b"IHDR", &ihdr);
            chunk(&mut out, b"IDAT", &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
            chunk(&mut out, b"IEND", &[]);
            std::fs::write(&path, out).unwrap();
        }
        "jpeg-wide" => {
            let (w, h) = (20_000u16, 500u16);
            let rgb: Vec<u8> = (0..w as usize * h as usize * 3).map(|i| ((i / 3 % w as usize) * 255 / w as usize) as u8).collect();
            jpeg_encoder::Encoder::new_file(&path, 80).unwrap().encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb).unwrap();
        }
        other => panic!("unknown kind {other}"),
    }
    println!("wrote {path}");
}
