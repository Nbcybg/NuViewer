//! Turning a finished picture into a file: the output tools of NuEditor in one pipeline.
//!
//! background removal -> resize (chosen algorithm) -> watermark -> colour reduction -> encode (format, quality, or a target
//! file size). The same function writes a copy from the editor and every file of a batch from NuGallery.

use crate::bgremove;
use crate::convert::{self, Out};
use crate::quantize;
use crate::raster::Pixels;
use crate::resample::{self, Algo};
use crate::save;
use crate::watermark::{self, Place};

/// How big the output is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SizeMode {
    Original,
    Percent(f32),
    LongEdge(u32),
    Width(u32),
    Height(u32),
}

impl SizeMode {
    pub fn target(self, w: u32, h: u32) -> (u32, u32) {
        let by = |k: f64| (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
        match self {
            SizeMode::Original => (w, h),
            SizeMode::Percent(p) => by(p.clamp(1.0, 800.0) as f64 / 100.0),
            SizeMode::LongEdge(l) => by(l.max(1) as f64 / w.max(h) as f64),
            SizeMode::Width(x) => by(x.max(1) as f64 / w as f64),
            SizeMode::Height(y) => by(y.max(1) as f64 / h as f64),
        }
    }
}

#[derive(Clone)]
pub struct Mark {
    /// The stamp itself (rendered text or a logo), RGBA.
    pub image: Pixels,
    pub place: Place,
    /// Width of the stamp as a fraction of the picture's width.
    pub width_frac: f32,
    pub opacity: f32,
}

#[derive(Clone)]
pub struct Export {
    pub format: Out,
    pub size: SizeMode,
    pub algo: Algo,
    /// Reduce to this many colours (2..=256), dithered or not.
    pub colours: Option<u32>,
    pub dither: bool,
    /// Make the file at most this many kilobytes: quality (JPEG) or the number of colours (PNG) goes down first, then the size.
    pub target_kb: Option<u32>,
    /// Background removal tolerance (0..100); None = off. Needs a format with transparency to show (PNG / WebP).
    pub remove_bg: Option<f32>,
    pub mark: Option<Mark>,
    /// Carry the EXIF block of the source JPEG into a JPEG.
    pub keep_exif: bool,
    /// What transparency becomes in a format without it.
    pub background: [u8; 3],
}

impl Export {
    pub fn new(format: Out) -> Export {
        Export { format, size: SizeMode::Original, algo: Algo::Lanczos, colours: None, dither: true, target_kb: None, remove_bg: None, mark: None, keep_exif: true, background: [0, 0, 0] }
    }
}

pub struct Exported {
    pub bytes: Vec<u8>,
    pub ext: &'static str,
    pub w: u32,
    pub h: u32,
    /// What was done to meet the target size, for the message to the user.
    pub note: String,
}

fn encode_once(p: &Pixels, format: Out, o: &Export, exif_source: Option<&[u8]>, indexed: bool) -> Result<Vec<u8>, String> {
    match format {
        Out::Jpeg(_) => {
            let exif = if o.keep_exif { exif_source.and_then(|b| save::exif_of_jpeg(b, (p.w, p.h))) } else { None };
            convert::encode(p, format, o.background, exif.as_deref())
        }
        Out::Png if indexed => match quantize::indexed_png(p) {
            Some(b) => Ok(b),
            None => convert::encode(p, format, o.background, None),
        },
        _ => convert::encode(p, format, o.background, None),
    }
}

/// Smallest quality in `lo..=hi` whose JPEG is at most `limit` bytes, found by bisection (larger quality = larger file).
fn jpeg_for_size(p: &Pixels, o: &Export, exif: Option<&[u8]>, hi: u8, lo: u8, limit: usize) -> Result<Option<(u8, Vec<u8>)>, String> {
    let (mut lo, mut hi) = (lo, hi);
    let mut best = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        let bytes = encode_once(p, Out::Jpeg(mid), o, exif, false)?;
        if bytes.len() <= limit {
            best = Some((mid, bytes));
            if mid == 255 {
                break;
            }
            lo = mid + 1;
        } else {
            if mid == 0 {
                break;
            }
            hi = mid - 1;
        }
    }
    Ok(best)
}

/// Run the pipeline on `rendered` (the edited picture at full size). `exif_source` is the original JPEG's bytes, if there is one.
pub fn run(mut rendered: Pixels, o: &Export, exif_source: Option<&[u8]>) -> Result<Exported, String> {
    let mut notes: Vec<String> = Vec::new();
    if let Some(t) = o.remove_bg {
        let share = bgremove::remove_background(&mut rendered, t);
        notes.push(format!("background removed ({:.0}% of the picture)", share * 100.0));
        if !o.format.has_alpha() {
            notes.push("this format has no transparency: the removed area is filled".into());
        }
    }
    let (tw, th) = o.size.target(rendered.w, rendered.h);
    if (tw, th) != (rendered.w, rendered.h) {
        rendered = resample::resize(&rendered, tw, th, o.algo);
    }
    if let Some(m) = &o.mark {
        watermark::stamp(&mut rendered, &m.image, m.place, m.width_frac, m.opacity, 0.02);
    }
    let base = rendered;
    let ext = o.format.ext();
    let reduce = |p: &Pixels, n: u32| quantize::reduce(p, n as usize, o.dither);
    let start = match o.colours {
        Some(n) => {
            notes.push(format!("{n} colours"));
            reduce(&base, n)
        }
        None => base.clone(),
    };
    let indexed = o.colours.is_some() && o.format == Out::Png;
    let Some(kb) = o.target_kb else {
        let bytes = encode_once(&start, o.format, o, exif_source, indexed)?;
        return Ok(Exported { bytes, ext, w: start.w, h: start.h, note: notes.join(", ") });
    };
    let limit = kb as usize * 1024;
    // Step 1: what the format can give up without changing the size.
    let mut scale = 1.0f32;
    for _ in 0..14 {
        let cur = if scale < 1.0 { resample::resize(&base, ((base.w as f32 * scale).round() as u32).max(1), ((base.h as f32 * scale).round() as u32).max(1), o.algo) } else { start.clone() };
        let cur = if scale < 1.0 && o.colours.is_some() { reduce(&cur, o.colours.unwrap_or(256)) } else { cur };
        match o.format {
            Out::Jpeg(q) => {
                if let Some((q, bytes)) = jpeg_for_size(&cur, o, exif_source, q.max(30), 30, limit)? {
                    notes.push(if scale < 1.0 { format!("quality {q}, size x{scale:.2} to fit {kb} KB") } else { format!("quality {q} to fit {kb} KB") });
                    return Ok(Exported { bytes, ext, w: cur.w, h: cur.h, note: notes.join(", ") });
                }
            }
            Out::Png => {
                let first = o.colours.unwrap_or(256).min(256);
                for n in [first, 128, 64, 32, 16].into_iter().filter(|&n| n <= first) {
                    let r = if n == o.colours.unwrap_or(0) && scale == 1.0 { cur.clone() } else { reduce(&cur, n) };
                    let bytes = encode_once(&r, Out::Png, o, None, true)?;
                    if bytes.len() <= limit {
                        notes.push(format!("{n} colours{} to fit {kb} KB", if scale < 1.0 { format!(", size x{scale:.2}") } else { String::new() }));
                        return Ok(Exported { bytes, ext, w: r.w, h: r.h, note: notes.join(", ") });
                    }
                }
            }
            _ => {
                let bytes = encode_once(&cur, o.format, o, None, false)?;
                if bytes.len() <= limit {
                    notes.push(format!("size x{scale:.2} to fit {kb} KB"));
                    return Ok(Exported { bytes, ext, w: cur.w, h: cur.h, note: notes.join(", ") });
                }
            }
        }
        scale *= 0.85;
    }
    Err(format!("couldn't make it {kb} KB or less"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(w: u32, h: u32) -> Pixels {
        // Smooth shapes plus noise: compresses, but not to nothing.
        let mut seed = 5u32;
        Pixels::new(
            w,
            h,
            (0..w * h)
                .flat_map(|i| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let (x, y) = ((i % w) as f32, (i / w) as f32);
                    let n = ((seed >> 26) as i32) - 32;
                    let v = |f: f32| ((128.0 + 100.0 * ((x * f).sin() * (y * f * 0.7).cos())) as i32 + n).clamp(0, 255) as u8;
                    [v(0.05), v(0.08), v(0.03), 255]
                })
                .collect(),
        )
    }

    #[test]
    fn size_modes() {
        assert_eq!(SizeMode::Original.target(300, 200), (300, 200));
        assert_eq!(SizeMode::Percent(50.0).target(300, 200), (150, 100));
        assert_eq!(SizeMode::LongEdge(150).target(300, 200), (150, 100));
        assert_eq!(SizeMode::Width(60).target(300, 200), (60, 40));
        assert_eq!(SizeMode::Height(100).target(300, 200), (150, 100));
    }

    #[test]
    fn resize_then_encode_in_the_chosen_format() {
        let p = photo(200, 120);
        let mut o = Export::new(Out::Png);
        o.size = SizeMode::Width(100);
        let r = run(p.clone(), &o, None).unwrap();
        assert_eq!((r.w, r.h, r.ext), (100, 60, "png"));
        assert_eq!(&r.bytes[..8], b"\x89PNG\r\n\x1a\n");
        let mut o = Export::new(Out::Jpeg(80));
        o.size = SizeMode::Percent(25.0);
        let r = run(p, &o, None).unwrap();
        assert_eq!((r.w, r.h, r.ext), (50, 30, "jpg"));
        assert_eq!(&r.bytes[..3], &[0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn a_target_size_is_met_by_lowering_quality_first() {
        let p = photo(240, 160);
        let big = run(p.clone(), &Export::new(Out::Jpeg(95)), None).unwrap();
        let mut o = Export::new(Out::Jpeg(95));
        o.target_kb = Some((big.bytes.len() / 1024 / 2).max(1) as u32);
        let r = run(p, &o, None).unwrap();
        assert!(r.bytes.len() <= o.target_kb.unwrap() as usize * 1024, "{} vs {} KB", r.bytes.len(), o.target_kb.unwrap());
        assert!(r.note.contains("quality"), "{}", r.note);
        assert_eq!((r.w, r.h), (240, 160), "size kept when quality was enough");
    }

    #[test]
    fn a_hard_target_shrinks_the_picture() {
        let p = photo(240, 160);
        let mut o = Export::new(Out::Jpeg(95));
        o.target_kb = Some(2);
        let r = run(p, &o, None).unwrap();
        assert!(r.bytes.len() <= 2 * 1024);
        assert!(r.w < 240, "{}x{} {}", r.w, r.h, r.note);
    }

    #[test]
    fn colour_reduction_and_watermark_and_background_removal_are_in_the_pipeline() {
        let mut o = Export::new(Out::Png);
        o.colours = Some(8);
        let r = run(photo(64, 64), &o, None).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(r.bytes));
        dec.set_transformations(png::Transformations::EXPAND);
        let mut rd = dec.read_info().unwrap();
        let mut buf = vec![0; rd.output_buffer_size().unwrap()];
        let info = rd.next_frame(&mut buf).unwrap();
        let px = buf.chunks_exact(info.color_type.samples()).map(|c| c[..3].to_vec()).collect::<std::collections::HashSet<_>>();
        assert!(px.len() <= 8, "{}", px.len());

        let mut o = Export::new(Out::Png);
        o.mark = Some(Mark { image: Pixels::new(4, 2, vec![255; 4 * 2 * 4]), place: Place::TopLeft, width_frac: 0.25, opacity: 1.0 });
        let dark = Pixels::new(80, 40, (0..80 * 40).flat_map(|_| [0, 0, 0, 255]).collect());
        let r = run(dark, &o, None).unwrap();
        let dec = png::Decoder::new(std::io::Cursor::new(r.bytes));
        let mut rd = dec.read_info().unwrap();
        let mut buf = vec![0; rd.output_buffer_size().unwrap()];
        rd.next_frame(&mut buf).unwrap();
        assert!(buf.iter().any(|&b| b == 255), "the mark is in the file");

        let mut o = Export::new(Out::Png);
        o.remove_bg = Some(40.0);
        let flat = Pixels::new(30, 30, (0..900).flat_map(|i| if (10..20).contains(&(i % 30)) && (10..20).contains(&(i / 30)) { [200, 20, 20, 255] } else { [20, 20, 200, 255] }).collect());
        let r = run(flat, &o, None).unwrap();
        assert!(r.note.contains("background removed"));
    }
}
