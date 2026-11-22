//! Animated GIF writer, for editing a GIF as a whole: every frame gets the same recipe (rendered by the engine), then the frames are
//! written back as one animation. GIF holds 256 colours per frame, so each frame gets its own palette (median cut, optionally dithered);
//! a frame with transparency keeps it (one palette entry is the transparent colour, pixels under half opaque go there).

use crate::quantize;
use crate::raster::Pixels;
use std::collections::HashMap;

/// One frame and how long it stays (milliseconds; GIF counts hundredths of a second, at least 20 ms is what browsers honour).
pub struct Frame {
    pub pixels: Pixels,
    pub delay_ms: u32,
}

/// The frames as an endlessly repeating GIF. All frames must have the size of the first.
pub fn encode(frames: &[Frame], dither: bool) -> Result<Vec<u8>, String> {
    let first = frames.first().ok_or("no frames to write")?;
    let (w, h) = (first.pixels.w, first.pixels.h);
    if w == 0 || h == 0 || w > 65_535 || h > 65_535 {
        return Err(format!("{w} x {h} is not a size a GIF can have (1 to 65535 pixels a side)"));
    }
    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
        enc.set_repeat(gif::Repeat::Infinite).map_err(|e| e.to_string())?;
        for f in frames {
            if f.pixels.w != w || f.pixels.h != h {
                return Err("every frame of a GIF has to be the same size".into());
            }
            enc.write_frame(&gif_frame(&f.pixels, f.delay_ms, dither)).map_err(|e| e.to_string())?;
        }
    }
    Ok(out)
}

fn gif_frame(p: &Pixels, delay_ms: u32, dither: bool) -> gif::Frame<'static> {
    let transparent = p.rgba.chunks_exact(4).any(|px| px[3] < 128);
    // Opaque copy for the palette: transparent pixels do not take part in choosing the colours.
    let reduced = quantize::reduce(p, if transparent { 255 } else { 256 }, dither);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut index_of: HashMap<[u8; 3], u8> = HashMap::new();
    let mut indices = Vec::with_capacity((p.w * p.h) as usize);
    let none = 255u8; // the transparent entry's number (the palette has at most 255 colours then)
    for (px, rd) in p.rgba.chunks_exact(4).zip(reduced.rgba.chunks_exact(4)) {
        if transparent && px[3] < 128 {
            indices.push(none);
            continue;
        }
        let c = [rd[0], rd[1], rd[2]];
        let i = *index_of.entry(c).or_insert_with(|| {
            palette.push(c);
            (palette.len() - 1) as u8
        });
        indices.push(i);
    }
    if transparent {
        palette.resize(256, [0, 0, 0]);
    }
    let mut table: Vec<u8> = palette.iter().flatten().copied().collect();
    if table.len() < 6 {
        table.resize(6, 0); // the format wants at least two entries
    }
    gif::Frame {
        width: p.w as u16,
        height: p.h as u16,
        buffer: std::borrow::Cow::Owned(indices),
        palette: Some(table),
        transparent: transparent.then_some(none),
        delay: (delay_ms / 10).clamp(2, 65_535) as u16,
        // Each frame is whole: the one before is cleared first, so a transparent pixel shows the background, not the last frame.
        dispose: gif::DisposalMethod::Background,
        ..gif::Frame::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Pixels {
        Pixels::new(w, h, (0..w * h).flat_map(|_| rgba).collect())
    }

    fn decode(bytes: &[u8]) -> Vec<(u32, Vec<u8>)> {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let mut dec = opts.read_info(std::io::Cursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        while let Some(f) = dec.read_next_frame().unwrap() {
            out.push((f.delay as u32 * 10, f.buffer.to_vec()));
        }
        out
    }

    #[test]
    fn frames_delays_and_colours_survive_a_round_trip() {
        let frames = vec![Frame { pixels: solid(6, 4, [200, 30, 30, 255]), delay_ms: 100 }, Frame { pixels: solid(6, 4, [20, 40, 220, 255]), delay_ms: 40 }, Frame { pixels: solid(6, 4, [250, 250, 250, 255]), delay_ms: 5 }];
        let bytes = encode(&frames, false).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        let back = decode(&bytes);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].0, 100);
        assert_eq!(back[1].0, 40);
        assert_eq!(back[2].0, 20, "a delay under 20 ms is raised to what players honour");
        // The colours are close (a GIF palette is exact for so few colours).
        let near = |a: &[u8], b: [u8; 3]| (0..3).all(|c| (a[c] as i32 - b[c] as i32).abs() <= 8);
        assert!(near(&back[0].1[..4], [200, 30, 30]));
        assert!(near(&back[1].1[..4], [20, 40, 220]));
        assert!(back.iter().all(|f| f.1.chunks_exact(4).all(|p| p[3] == 255)));
    }

    #[test]
    fn transparency_is_kept() {
        let mut half = solid(4, 2, [10, 200, 10, 255]);
        for x in 0..2 {
            for y in 0..2 {
                half.rgba[((y * 4 + x) * 4 + 3) as usize] = 0;
            }
        }
        let back = decode(&encode(&[Frame { pixels: half, delay_ms: 50 }], false).unwrap());
        let f = &back[0].1;
        assert_eq!(f[3], 0, "left half transparent");
        assert_eq!(f[2 * 4 + 3], 255, "right half opaque");
        assert!(f[2 * 4 + 1] > 150, "and still green");
    }

    #[test]
    fn a_photo_like_frame_gets_a_256_colour_palette_that_looks_like_it() {
        let mut p = Pixels::new(32, 32, vec![255; 32 * 32 * 4]);
        for y in 0..32u32 {
            for x in 0..32u32 {
                let i = ((y * 32 + x) * 4) as usize;
                p.rgba[i] = (x * 8) as u8;
                p.rgba[i + 1] = (y * 8) as u8;
                p.rgba[i + 2] = ((x + y) * 4) as u8;
            }
        }
        let back = decode(&encode(&[Frame { pixels: p.clone(), delay_ms: 100 }], true).unwrap());
        let diff: f32 = p.rgba.chunks_exact(4).zip(back[0].1.chunks_exact(4)).map(|(a, b)| (0..3).map(|c| (a[c] as f32 - b[c] as f32).abs()).sum::<f32>() / 3.0).sum::<f32>() / 1024.0;
        assert!(diff < 14.0, "average error {diff}");
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(encode(&[], false).is_err());
        let frames = vec![Frame { pixels: solid(4, 4, [1, 2, 3, 255]), delay_ms: 20 }, Frame { pixels: solid(5, 4, [1, 2, 3, 255]), delay_ms: 20 }];
        assert!(encode(&frames, false).unwrap_err().contains("same size"));
    }
}
