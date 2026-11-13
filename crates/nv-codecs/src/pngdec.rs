//! PNG and APNG. Animated frames are composited onto a full canvas (blend/dispose ops honoured).

use crate::{icc, Image, MAX_ANIM_BYTES};
use png::{BlendOp, ColorType, DisposeOp};
use std::io::Cursor;

fn to_rgba(data: &[u8], color: ColorType) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(data.len() / color.samples() * 4);
    match color {
        ColorType::Rgba => out.extend_from_slice(data),
        ColorType::Rgb => data.chunks_exact(3).for_each(|c| out.extend_from_slice(&[c[0], c[1], c[2], 255])),
        ColorType::GrayscaleAlpha => data.chunks_exact(2).for_each(|c| out.extend_from_slice(&[c[0], c[0], c[0], c[1]])),
        ColorType::Grayscale => data.iter().for_each(|&g| out.extend_from_slice(&[g, g, g, 255])),
        ColorType::Indexed => return Err("png: indexed not expanded".into()),
    }
    Ok(out)
}

pub fn decode(bytes: &[u8], first_only: bool) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    // Default is 64 MiB of decoder memory, far too small for big scans/panoramas. The real guard is `check_size`.
    decoder.set_limits(png::Limits { bytes: 3 << 30 });
    let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    crate::check_size(reader.info().width as u64, reader.info().height as u64)?;
    let icc_profile = reader.info().icc_profile.as_ref().map(|c| c.to_vec());
    let anim_frames = reader.info().animation_control().map(|a| a.num_frames).unwrap_or(0);
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or("png: size overflow")?];

    if anim_frames <= 1 || first_only {
        // Static image, or the first frame of an animation. (If the default image is hidden from the
        // animation it comes first; either way the first decoded frame is what we want.)
        let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
        let mut rgba = to_rgba(&buf[..info.buffer_size()], info.color_type)?;
        if info.width as usize != w || info.height as usize != h {
            // First animation frame smaller than the canvas: place it on a transparent canvas.
            let fc = reader.info().frame_control.clone();
            let mut canvas = vec![0u8; w * h * 4];
            let (xo, yo) = fc.map(|f| (f.x_offset as usize, f.y_offset as usize)).unwrap_or((0, 0));
            blit(&mut canvas, w, h, &rgba, info.width as usize, info.height as usize, xo, yo, BlendOp::Source);
            rgba = canvas;
        }
        let mut img = Image::new(w as u32, h as u32, rgba);
        img.icc = icc_profile; // converted later, after any downscaling
        img.profile_assumable = true;
        return Ok(img);
    }

    let mut canvas = vec![0u8; w * h * 4];
    let mut frames: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut seen = 0u32;
    let mut guard = 0u32;
    while seen < anim_frames && guard <= anim_frames {
        guard += 1;
        let info = reader.next_frame(&mut buf).map_err(|e| format!("apng: {e}"))?;
        let Some(fc) = reader.info().frame_control.clone() else { continue }; // hidden default image: skip
        seen += 1;
        let frame = to_rgba(&buf[..info.buffer_size()], info.color_type)?;
        let saved = (fc.dispose_op == DisposeOp::Previous).then(|| canvas.clone());
        blit(&mut canvas, w, h, &frame, fc.width as usize, fc.height as usize, fc.x_offset as usize, fc.y_offset as usize, fc.blend_op);

        let den = if fc.delay_den == 0 { 100 } else { fc.delay_den as u32 };
        let mut ms = fc.delay_num as u32 * 1000 / den;
        if ms <= 10 {
            ms = 100; // same convention browsers use for degenerate delays
        }
        let mut shown = canvas.clone();
        if let Some(p) = &icc_profile {
            icc::to_srgb_in_place(&mut shown, p);
        }
        frames.push((shown, ms));

        match fc.dispose_op {
            DisposeOp::None => {}
            DisposeOp::Background => clear_rect(&mut canvas, w, h, fc.x_offset as usize, fc.y_offset as usize, fc.width as usize, fc.height as usize),
            DisposeOp::Previous => {
                if let Some(s) = saved {
                    canvas = s;
                }
            }
        }
        if (frames.len() + 1) * w * h * 4 > MAX_ANIM_BYTES {
            break;
        }
    }
    Image::from_frames(w as u32, h as u32, frames).ok_or_else(|| "apng: no frames".to_string())
}

fn clear_rect(canvas: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, fw: usize, fh: usize) {
    for y in y0..(y0 + fh).min(h) {
        let (a, b) = ((y * w + x0.min(w)) * 4, (y * w + (x0 + fw).min(w)) * 4);
        canvas[a..b].fill(0);
    }
}

/// Draw a `fw`x`fh` RGBA sub-image onto the canvas at (x0, y0).
#[allow(clippy::too_many_arguments)]
fn blit(canvas: &mut [u8], w: usize, h: usize, frame: &[u8], fw: usize, fh: usize, x0: usize, y0: usize, op: BlendOp) {
    for y in 0..fh {
        if y0 + y >= h {
            break;
        }
        for x in 0..fw {
            if x0 + x >= w {
                break;
            }
            let (s, d) = ((y * fw + x) * 4, ((y0 + y) * w + x0 + x) * 4);
            let src = &frame[s..s + 4];
            match op {
                BlendOp::Source => canvas[d..d + 4].copy_from_slice(src),
                BlendOp::Over => {
                    let sa = src[3] as f32 / 255.0;
                    if sa >= 1.0 {
                        canvas[d..d + 4].copy_from_slice(src);
                    } else if sa > 0.0 {
                        let da = canvas[d + 3] as f32 / 255.0;
                        let oa = sa + da * (1.0 - sa);
                        for c in 0..3 {
                            let v = (src[c] as f32 * sa + canvas[d + c] as f32 * da * (1.0 - sa)) / oa;
                            canvas[d + c] = v.round().clamp(0.0, 255.0) as u8;
                        }
                        canvas[d + 3] = (oa * 255.0).round() as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Vec<u8> {
        px.iter().copied().cycle().take((w * h * 4) as usize).collect()
    }

    /// 4x4 canvas: frame 0 full red; frame 1 a 2x2 blue patch at (1,1) with Background dispose; frame 2 full green.
    fn make_apng() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 4, 4);
            enc.set_color(ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_animated(3, 0).unwrap();
            let mut w = enc.write_header().unwrap();
            w.set_frame_delay(5, 100).unwrap(); // 50 ms
            w.write_image_data(&solid(4, 4, [255, 0, 0, 255])).unwrap();
            w.set_frame_dimension(2, 2).unwrap();
            w.set_frame_position(1, 1).unwrap();
            w.set_dispose_op(DisposeOp::Background).unwrap();
            w.set_frame_delay(10, 100).unwrap(); // 100 ms
            w.write_image_data(&solid(2, 2, [0, 0, 255, 255])).unwrap();
            w.set_frame_position(0, 0).unwrap();
            w.set_frame_dimension(4, 4).unwrap();
            w.set_dispose_op(DisposeOp::None).unwrap();
            w.set_frame_delay(20, 100).unwrap(); // 200 ms
            w.write_image_data(&solid(4, 4, [0, 255, 0, 255])).unwrap();
            w.finish().unwrap();
        }
        out
    }

    #[test]
    fn apng_frames_are_composited() {
        let img = decode(&make_apng(), false).unwrap();
        assert_eq!((img.width, img.height, img.frame_count()), (4, 4, 3));
        assert_eq!(img.delay_ms, 50);
        assert_eq!(img.extra_frames[0].delay_ms, 100);
        assert_eq!(img.extra_frames[1].delay_ms, 200);
        let px = |f: &[u8], x: usize, y: usize| f[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].to_vec();
        assert_eq!(px(&img.rgba, 0, 0), [255, 0, 0, 255]);
        // Frame 1: red canvas with a blue patch at (1,1)-(2,2); corners stay red.
        assert_eq!(px(&img.extra_frames[0].rgba, 1, 1), [0, 0, 255, 255]);
        assert_eq!(px(&img.extra_frames[0].rgba, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&img.extra_frames[1].rgba, 3, 3), [0, 255, 0, 255]);
    }

    #[test]
    fn first_only_gives_single_frame() {
        let img = decode(&make_apng(), true).unwrap();
        assert_eq!(img.frame_count(), 1);
        assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn plain_png_still_works() {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, 2, 1);
            enc.set_color(ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&[10, 20, 30, 40, 50, 60]).unwrap();
        }
        let img = decode(&out, false).unwrap();
        assert_eq!(img.rgba, [10, 20, 30, 255, 40, 50, 60, 255]);
        assert_eq!(img.frame_count(), 1);
    }
}
