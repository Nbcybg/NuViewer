//! EXIF orientation: reading and baking into pixels.

use crate::Image;

/// Orientation tag (1..=8) if `bytes` is (or contains, for JPEG/TIFF/HEIF) an EXIF block that has one.
pub fn exif_orientation(bytes: &[u8]) -> Option<u32> {
    exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()
        .and_then(|e| e.get_field(exif::Tag::Orientation, exif::In::PRIMARY).and_then(|f| f.value.get_uint(0)))
}

/// Bakes EXIF orientation (1..=8) into the pixels. Static images only.
/// TODO: move to the GPU (UV transform) to avoid this CPU pass.
pub fn apply(img: Image, o: u32) -> Image {
    if !(2..=8).contains(&o) {
        return img;
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let (dw, dh) = if o >= 5 { (h, w) } else { (w, h) };
    let src: Vec<u32> = img.rgba.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
    let mut out = vec![0u8; w * h * 4];
    for y in 0..dh {
        for x in 0..dw {
            let (sx, sy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            let d = (y * dw + x) * 4;
            out[d..d + 4].copy_from_slice(&src[sy * w + sx].to_ne_bytes());
        }
    }
    Image::new(dw as u32, dh as u32, out)
}
