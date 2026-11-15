//! What the right panel shows about a file: a bigger preview, the histogram, the editable properties and the EXIF
//! rows. All of it is produced off the UI thread and only for the picture the pointer has rested on.

use nv_codecs::Image;
use nv_win::props::Field;
use std::path::Path;

/// Longest edge of the preview picture.
pub const PREVIEW_EDGE: u32 = 720;
/// The preview texture of picture `id` lives in the renderer's thumbnail map under this key.
pub fn preview_key(id: u64) -> u64 {
    id ^ 0x5052_4556_4945_5721
}

/// A picture for the preview: the first frame (a JPEG is decoded with its DCT scaled, a RAW gives its embedded preview, so
/// this is fast even for big files), at most `PREVIEW_EDGE` long, upright, in the output colours.
pub fn preview_decode(path: &Path) -> Result<Image, String> {
    let full = nv_codecs::decode_for_thumbnail(path)?;
    let mut small = nv_codecs::downscale(&full, PREVIEW_EDGE).finish_for_thumb();
    drop(full);
    small.srgb_to_output();
    Ok(small)
}

#[derive(Clone)]
pub struct Histogram {
    pub r: [u32; 256],
    pub g: [u32; 256],
    pub b: [u32; 256],
    pub l: [u32; 256],
    pub peak: u32,
}

/// Counts of each brightness level of the picture's pixels (transparent pixels are ignored).
pub fn histogram(img: &Image) -> Histogram {
    let mut h = Histogram { r: [0; 256], g: [0; 256], b: [0; 256], l: [0; 256], peak: 1 };
    for p in img.rgba.chunks_exact(4) {
        if p[3] < 8 {
            continue;
        }
        h.r[p[0] as usize] += 1;
        h.g[p[1] as usize] += 1;
        h.b[p[2] as usize] += 1;
        h.l[((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8) as usize] += 1;
    }
    // The extreme bins (pure black / white clipping) would flatten everything else: scale to the tallest of the rest.
    let peak = (1..255).flat_map(|i| [h.r[i], h.g[i], h.b[i], h.l[i]]).max().unwrap_or(1);
    h.peak = peak.max(1);
    h
}

/// The facts and properties of a file.
pub struct InfoData {
    pub id: u64,
    pub exif: Vec<(String, String)>,
    /// Editable properties (None: this kind of file has none to show).
    pub props: Option<Vec<(Field, String)>>,
    pub dims: Option<(u32, u32)>,
}

pub fn collect(path: &Path, id: u64) -> InfoData {
    InfoData {
        id,
        exif: nv_codecs::metadata::describe(path, nv_win::utc_offset_minutes()).into_iter().filter(|(k, _)| !matches!(k.as_str(), "File" | "Folder" | "Size" | "Modified")).collect(),
        props: nv_win::props::read(path),
        dims: nv_codecs::peek_size(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_counts_levels_and_ignores_transparency() {
        let mut rgba = Vec::new();
        for i in 0..100u32 {
            rgba.extend([100, 150, 200, 255]);
            if i < 10 {
                rgba.extend([0, 0, 0, 255]);
            }
        }
        rgba.extend([50, 50, 50, 0]); // transparent: not counted
        let n = rgba.len() as u32 / 4;
        let img = Image::new(n, 1, rgba);
        let h = histogram(&img);
        assert_eq!((h.r[100], h.g[150], h.b[200]), (100, 100, 100));
        assert_eq!(h.r[50], 0);
        assert_eq!(h.l.iter().sum::<u32>(), 110);
        assert_eq!(h.l[0], 10);
        assert_eq!(h.peak, 100, "black (bin 0) doesn't set the scale");
    }

    /// The property system (the same one Explorer's Details tab uses) really stores what the panel writes.
    #[test]
    fn properties_are_written_into_a_jpeg_and_read_back() {
        use nv_editor::{convert, Pixels};
        let dir = std::env::temp_dir().join(format!("nv_gal_props_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("p.jpg");
        let px = Pixels::new(16, 16, vec![120; 16 * 16 * 4]);
        std::fs::write(&f, convert::encode(&px, convert::Out::Jpeg(90), [255; 3], None).unwrap()).unwrap();
        let get = |field: Field| nv_win::props::read(&f).and_then(|v| v.into_iter().find(|(k, _)| *k == field).map(|(_, t)| t)).unwrap_or_default();
        assert_eq!(get(Field::Title), "");
        let attempt = |field: Field, text: &str| nv_win::props::write(&f, field, text);
        for (field, text) in [(Field::Title, "Sunset at the pier"), (Field::Comment, "taken from the boat"), (Field::Copyright, "(c) test"), (Field::Author, "Somchai; Mali"), (Field::Keywords, "sea; evening"), (Field::Rating, "4")] {
            attempt(field, text).unwrap_or_else(|e| panic!("{field:?}: {e}"));
        }
        assert_eq!(get(Field::Title), "Sunset at the pier");
        assert_eq!(get(Field::Comment), "taken from the boat");
        assert_eq!(get(Field::Copyright), "(c) test");
        assert_eq!(get(Field::Rating), "4");
        assert!(get(Field::Author).contains("Somchai") && get(Field::Author).contains("Mali"), "{}", get(Field::Author));
        assert!(get(Field::Keywords).contains("sea"), "{}", get(Field::Keywords));
        // Clearing a field empties it; the picture itself still decodes.
        attempt(Field::Title, "").unwrap();
        assert_eq!(get(Field::Title), "");
        assert!(nv_codecs::decode_file(&f).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_keys_differ_from_thumbnail_ids() {
        assert_ne!(preview_key(5), 5);
        assert_eq!(preview_key(preview_key(5)), 5);
    }
}
