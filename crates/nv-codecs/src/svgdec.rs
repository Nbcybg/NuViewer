//! SVG / SVGZ, rendered with resvg (pure Rust). Vector art is rasterised at screen size (see `render_long_side`),
//! on white paper, so a logo with a transparent background is visible on the dark viewer background.

use crate::Image;
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

/// System fonts are slow to enumerate (hundreds of ms), so they are loaded once, and only when a file has text.
fn system_fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

fn has_text(bytes: &[u8]) -> bool {
    // gzip-compressed (.svgz) files can't be scanned: assume they might contain text.
    bytes.starts_with(&[0x1F, 0x8B]) || bytes.windows(5).any(|w| w == b"<text")
}

pub fn decode(bytes: &[u8], thumb: bool) -> Result<Image, String> {
    let mut opt = usvg::Options::default();
    if has_text(bytes) {
        opt.fontdb = system_fonts();
    }
    let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| format!("svg: {e}"))?;
    let size = tree.size();
    let (sw, sh) = (size.width().max(1.0), size.height().max(1.0));
    let long = if thumb { 640.0 } else { crate::render_long_side() as f32 };
    let s = long / sw.max(sh);
    let (w, h) = (((sw * s).ceil() as u32).max(1), ((sh * s).ceil() as u32).max(1));
    crate::check_size(w as u64, h as u64)?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("svg: image too large")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::from_scale(w as f32 / sw, h as f32 / sh), &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha; after filling with white everything is opaque, so the data is plain RGBA.
    Ok(Image::new(w, h, pixmap.take()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_ON_BLUE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
        <rect width="40" height="20" fill="#0000ff"/><rect x="10" y="5" width="20" height="10" fill="#ff0000"/></svg>"##;

    fn px(img: &Image, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
    }

    #[test]
    fn renders_shapes_at_screen_size_keeping_the_aspect() {
        let img = decode(RED_ON_BLUE.as_bytes(), true).unwrap();
        assert_eq!((img.width, img.height), (640, 320), "40x20 scaled so the long side is 640");
        assert_eq!(px(&img, 320, 160), [255, 0, 0, 255], "centre is the red rectangle");
        assert_eq!(px(&img, 10, 10), [0, 0, 255, 255], "corner is the blue background");
        let full = decode(RED_ON_BLUE.as_bytes(), false).unwrap();
        assert!(full.width >= 2048 && full.width == full.height * 2);
    }

    #[test]
    fn transparent_art_sits_on_white() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><circle cx="5" cy="5" r="2" fill="#000"/></svg>"##;
        let img = decode(svg.as_bytes(), true).unwrap();
        assert_eq!(px(&img, 2, 2), [255, 255, 255, 255]);
        assert_eq!(px(&img, img.width / 2, img.height / 2)[..3], [0, 0, 0]);
    }

    #[test]
    fn gzip_compressed_svgz_works() {
        // gzip "stored" (uncompressed) block wrapper around the same SVG: valid gzip without a compressor.
        let data = RED_ON_BLUE.as_bytes();
        let crc = {
            let mut table = [0u32; 256];
            for i in 0..256u32 {
                let mut c = i;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                table[i as usize] = c;
            }
            !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
        };
        let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 255, 1];
        gz.extend((data.len() as u16).to_le_bytes());
        gz.extend((!(data.len() as u16)).to_le_bytes());
        gz.extend(data);
        gz.extend(crc.to_le_bytes());
        gz.extend((data.len() as u32).to_le_bytes());
        let img = decode(&gz, true).unwrap();
        assert_eq!(px(&img, 320, 160), [255, 0, 0, 255]);
    }

    #[test]
    fn text_renders_with_system_fonts_and_junk_is_an_error() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60"><text x="5" y="45" font-size="48" font-family="Arial" fill="#000">Hi</text></svg>"##;
        let img = decode(svg.as_bytes(), true).unwrap();
        assert!(img.rgba.chunks_exact(4).any(|p| p[0] < 100), "some dark text pixels");
        assert!(decode(b"definitely not svg", true).is_err());
    }
}
