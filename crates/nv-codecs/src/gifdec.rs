//! GIF: frames composited onto the logical screen honouring disposal methods and transparency.

use crate::{Image, MAX_ANIM_BYTES};
use gif::DisposalMethod;
use std::io::Cursor;

pub fn decode(bytes: &[u8], first_only: bool) -> Result<Image, String> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts.read_info(Cursor::new(bytes)).map_err(|e| format!("gif: {e}"))?;
    let (w, h) = (dec.width() as usize, dec.height() as usize);
    crate::check_size(w as u64, h as u64)?;
    let mut canvas = vec![0u8; w * h * 4];
    let mut frames: Vec<(Vec<u8>, u32)> = Vec::new();

    while let Some(frame) = dec.read_next_frame().map_err(|e| format!("gif: {e}"))? {
        let (left, top, fw, fh) = (frame.left as usize, frame.top as usize, frame.width as usize, frame.height as usize);
        let dispose = frame.dispose;
        let saved = (dispose == DisposalMethod::Previous).then(|| canvas.clone());
        for y in 0..fh {
            if top + y >= h {
                break;
            }
            for x in 0..fw {
                if left + x >= w {
                    break;
                }
                let s = (y * fw + x) * 4;
                if frame.buffer[s + 3] != 0 {
                    let d = ((top + y) * w + left + x) * 4;
                    canvas[d..d + 4].copy_from_slice(&frame.buffer[s..s + 4]);
                }
            }
        }
        let mut ms = frame.delay as u32 * 10;
        if ms <= 10 {
            ms = 100; // browsers treat 0/10 ms as 100 ms
        }
        frames.push((canvas.clone(), ms));
        if first_only || (frames.len() + 1) * w * h * 4 > MAX_ANIM_BYTES {
            break;
        }
        match dispose {
            DisposalMethod::Background => {
                for y in top..(top + fh).min(h) {
                    let (a, b) = ((y * w + left.min(w)) * 4, (y * w + (left + fw).min(w)) * 4);
                    canvas[a..b].fill(0);
                }
            }
            DisposalMethod::Previous => {
                if let Some(s) = saved {
                    canvas = s;
                }
            }
            _ => {}
        }
    }
    Image::from_frames(w as u32, h as u32, frames).ok_or_else(|| "gif: no frames".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4x4 GIF, 3 frames: red full; blue 2x2 patch at (1,1) with Background dispose; green full.
    fn make_gif() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let palette = [255, 0, 0, 0, 0, 255, 0, 255, 0];
            let mut enc = gif::Encoder::new(&mut out, 4, 4, &palette).unwrap();
            enc.set_repeat(gif::Repeat::Infinite).unwrap();
            let mk = |w: u16, h: u16, idx: u8, left: u16, top: u16, delay: u16, dispose: DisposalMethod| gif::Frame {
                width: w,
                height: h,
                left,
                top,
                delay,
                dispose,
                buffer: std::borrow::Cow::Owned(vec![idx; w as usize * h as usize]),
                ..gif::Frame::default()
            };
            enc.write_frame(&mk(4, 4, 0, 0, 0, 5, DisposalMethod::Keep)).unwrap(); // 50 ms
            enc.write_frame(&mk(2, 2, 1, 1, 1, 20, DisposalMethod::Background)).unwrap(); // 200 ms
            enc.write_frame(&mk(4, 4, 2, 0, 0, 0, DisposalMethod::Keep)).unwrap(); // 0 -> 100 ms
        }
        out
    }

    #[test]
    fn frames_delays_and_compositing() {
        let img = decode(&make_gif(), false).unwrap();
        assert_eq!((img.width, img.height, img.frame_count()), (4, 4, 3));
        assert_eq!((img.delay_ms, img.extra_frames[0].delay_ms, img.extra_frames[1].delay_ms), (50, 200, 100));
        let px = |f: &[u8], x: usize, y: usize| f[(y * 4 + x) * 4..(y * 4 + x) * 4 + 3].to_vec();
        assert_eq!(px(&img.rgba, 0, 0), [255, 0, 0]);
        assert_eq!(px(&img.extra_frames[0].rgba, 1, 1), [0, 0, 255]);
        assert_eq!(px(&img.extra_frames[0].rgba, 3, 3), [255, 0, 0]);
        assert_eq!(px(&img.extra_frames[1].rgba, 0, 0), [0, 255, 0]);
    }

    #[test]
    fn first_only() {
        assert_eq!(decode(&make_gif(), true).unwrap().frame_count(), 1);
    }
}
