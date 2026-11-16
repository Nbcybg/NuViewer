//! WAV sound as a picture: the waveform. Streams the file (a long recording is never loaded whole): a small file is read
//! through, a big one is sampled in short windows spread over its length, so a 2 GB recording draws in about the time a
//! photo does.

use crate::Image;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const WIDTH: usize = 1600;
const LANE_H: usize = 200;
/// Bigger than this and the file is sampled instead of read through.
const SCAN_ALL_BELOW: u64 = 48 * 1024 * 1024;
/// Frames looked at around each column when sampling.
const WINDOW_FRAMES: usize = 24_576;

#[derive(Clone, Debug, PartialEq)]
pub struct WaveInfo {
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
    pub float: bool,
    pub data_offset: u64,
    pub data_len: u64,
}

impl WaveInfo {
    pub fn frame_bytes(&self) -> u64 {
        (self.channels as u64 * (self.bits as u64 / 8)).max(1)
    }
    pub fn frames(&self) -> u64 {
        self.data_len / self.frame_bytes()
    }
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }
}

/// Read the RIFF header of a WAV file.
pub fn read_info<R: Read + Seek>(f: &mut R, file_len: u64) -> Result<WaveInfo, String> {
    let mut head = [0u8; 12];
    f.read_exact(&mut head).map_err(|e| e.to_string())?;
    if &head[0..4] != b"RIFF" && &head[0..4] != b"RF64" || &head[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let (mut fmt, mut pos) = (None, 12u64);
    loop {
        let mut ch = [0u8; 8];
        if f.read_exact(&mut ch).is_err() {
            return Err("no sound data in this WAV".into());
        }
        let size = u32::from_le_bytes([ch[4], ch[5], ch[6], ch[7]]) as u64;
        pos += 8;
        if &ch[0..4] == b"fmt " {
            let mut b = vec![0u8; size.min(64) as usize];
            f.read_exact(&mut b).map_err(|e| e.to_string())?;
            if b.len() < 16 {
                return Err("damaged WAV header".into());
            }
            let mut tag = u16::from_le_bytes([b[0], b[1]]);
            if tag == 0xFFFE && b.len() >= 26 {
                tag = u16::from_le_bytes([b[24], b[25]]); // WAVE_FORMAT_EXTENSIBLE: the real format is in the sub-format
            }
            fmt = Some((tag, u16::from_le_bytes([b[2], b[3]]), u32::from_le_bytes([b[4], b[5], b[6], b[7]]), u16::from_le_bytes([b[14], b[15]])));
            pos += b.len() as u64;
            let rest = size - b.len() as u64 + (size & 1);
            f.seek(SeekFrom::Current(rest as i64)).map_err(|e| e.to_string())?;
            pos += rest;
        } else if &ch[0..4] == b"data" {
            let (tag, channels, rate, bits) = fmt.ok_or("the WAV has data before its format")?;
            if !(tag == 1 || tag == 3) || channels == 0 || !matches!(bits, 8 | 16 | 24 | 32 | 64) {
                return Err(format!("this WAV is compressed or unusual (format {tag}, {bits} bits): play it instead"));
            }
            // A streamed recording can claim 0 or 0xFFFFFFFF bytes: take what is really there.
            let avail = file_len.saturating_sub(pos);
            let len = if size == 0 || size == 0xFFFF_FFFF || size > avail { avail } else { size };
            return Ok(WaveInfo { channels, rate, bits, float: tag == 3, data_offset: pos, data_len: len });
        } else {
            let skip = size + (size & 1);
            f.seek(SeekFrom::Current(skip as i64)).map_err(|e| e.to_string())?;
            pos += skip;
        }
    }
}

/// One sample as -1..1.
fn sample(info: &WaveInfo, b: &[u8]) -> f32 {
    match (info.bits, info.float) {
        (8, _) => (b[0] as f32 - 128.0) / 128.0,
        (16, _) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
        (24, _) => (((b[0] as i32) << 8 | (b[1] as i32) << 16 | (b[2] as i32) << 24) >> 8) as f32 / 8_388_608.0,
        (32, true) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        (32, false) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        (64, true) => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        _ => 0.0,
    }
}

/// Lowest and highest sample of each channel in each of `cols` columns: `peaks[channel][col] = (min, max)`.
pub fn peaks<R: Read + Seek>(f: &mut R, info: &WaveInfo, cols: usize) -> Result<Vec<Vec<(f32, f32)>>, String> {
    let ch = info.channels as usize;
    let fb = info.frame_bytes() as usize;
    let bps = info.bits as usize / 8;
    let frames = info.frames();
    let mut out = vec![vec![(0.0f32, 0.0f32); cols]; ch];
    if frames == 0 {
        return Ok(out);
    }
    let mut buf = vec![0u8; WINDOW_FRAMES.max(64 * 1024 / fb) * fb];
    let scan_all = info.data_len <= SCAN_ALL_BELOW;
    let feed = |col: usize, bytes: &[u8], out: &mut Vec<Vec<(f32, f32)>>| {
        for fr in bytes.chunks_exact(fb) {
            for c in 0..ch {
                let v = sample(info, &fr[c * bps..]);
                let p = &mut out[c][col];
                p.0 = p.0.min(v);
                p.1 = p.1.max(v);
            }
        }
    };
    if scan_all {
        f.seek(SeekFrom::Start(info.data_offset)).map_err(|e| e.to_string())?;
        let mut done = 0u64;
        while done < frames {
            let want = ((frames - done) as usize).min(buf.len() / fb);
            f.read_exact(&mut buf[..want * fb]).map_err(|e| e.to_string())?;
            // A block can straddle columns: split it where the column changes.
            let mut i = 0usize;
            while i < want {
                let col = (((done + i as u64) as u128 * cols as u128) / frames as u128) as usize;
                let col = col.min(cols - 1);
                let end_frame = ((col as u128 + 1) * frames as u128 / cols as u128) as u64;
                let n = ((end_frame.saturating_sub(done + i as u64)) as usize).clamp(1, want - i);
                feed(col, &buf[i * fb..(i + n) * fb], &mut out);
                i += n;
            }
            done += want as u64;
        }
    } else {
        for col in 0..cols {
            let start = (col as u128 * frames as u128 / cols as u128) as u64;
            let n = ((frames - start) as usize).min(WINDOW_FRAMES);
            f.seek(SeekFrom::Start(info.data_offset + start * fb as u64)).map_err(|e| e.to_string())?;
            f.read_exact(&mut buf[..n * fb]).map_err(|e| e.to_string())?;
            feed(col, &buf[..n * fb], &mut out);
        }
    }
    Ok(out)
}

/// Draw the peaks: one lane per channel (up to two; more are mixed into the first two), dark background, centre line.
pub fn draw(peaks: &[Vec<(f32, f32)>]) -> Image {
    let lanes = peaks.len().clamp(1, 2);
    let (w, h) = (WIDTH, LANE_H * lanes);
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[22, 24, 28, 255]);
    }
    let set = |rgba: &mut Vec<u8>, x: usize, y: usize, c: [u8; 3]| {
        let i = (y * w + x) * 4;
        rgba[i..i + 3].copy_from_slice(&c);
    };
    for lane in 0..lanes {
        let mid = lane * LANE_H + LANE_H / 2;
        for x in 0..w {
            set(&mut rgba, x, mid, [60, 66, 76]);
        }
        let src: Vec<(f32, f32)> = if peaks.len() <= 2 {
            peaks[lane.min(peaks.len() - 1)].clone()
        } else {
            (0..w).map(|x| peaks.iter().skip(lane).step_by(2).fold((0.0f32, 0.0f32), |a, p| (a.0.min(p[x].0), a.1.max(p[x].1)))).collect()
        };
        for (x, (lo, hi)) in src.iter().enumerate().take(w) {
            let half = LANE_H as f32 / 2.0 - 4.0;
            let y0 = (mid as f32 - hi.clamp(-1.0, 1.0) * half).round() as usize;
            let y1 = (mid as f32 - lo.clamp(-1.0, 1.0) * half).round() as usize;
            let loud = hi.abs().max(lo.abs());
            let c = if loud > 0.98 { [255, 96, 80] } else { [61, 139, 255] }; // red where it clips
            for y in y0.min(y1)..=y0.max(y1).min(h - 1) {
                set(&mut rgba, x, y, c);
            }
        }
    }
    Image::from_frames(w as u32, h as u32, vec![(rgba, 0)]).expect("one frame")
}

pub fn waveform(path: &Path) -> Result<Image, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let info = read_info(&mut f, len)?;
    let p = peaks(&mut f, &info, WIDTH)?;
    Ok(draw(&p))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    pub fn make_wav(frames: usize, ch: u16) -> Vec<u8> {
        let data_len = (frames * ch as usize * 2) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&ch.to_le_bytes());
        v.extend_from_slice(&48000u32.to_le_bytes());
        v.extend_from_slice(&(48000 * 2 * ch as u32).to_le_bytes());
        v.extend_from_slice(&(2 * ch).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            // Louder in the second half.
            let amp = if i < frames / 2 { 8000.0 } else { 30000.0 };
            let s = ((i as f32 * 0.05).sin() * amp) as i16;
            for _ in 0..ch {
                v.extend_from_slice(&s.to_le_bytes());
            }
        }
        v
    }

    #[test]
    fn header_and_peaks() {
        let bytes = make_wav(96_000, 2);
        let mut c = Cursor::new(bytes.clone());
        let info = read_info(&mut c, bytes.len() as u64).unwrap();
        assert_eq!((info.channels, info.rate, info.bits, info.frames()), (2, 48000, 16, 96_000));
        let p = peaks(&mut c, &info, 100).unwrap();
        assert!(p[0][10].1 < 0.3 && p[0][90].1 > 0.8, "quiet first half, loud second half");
        let img = draw(&p);
        assert_eq!((img.width as usize, img.height as usize), (WIDTH, LANE_H * 2));
    }

    #[test]
    fn refuses_compressed_wav_and_junk() {
        let mut bad = make_wav(100, 1);
        bad[20] = 0x55; // format tag 0x55 = MP3 inside RIFF
        let n = bad.len() as u64;
        assert!(read_info(&mut Cursor::new(bad), n).is_err());
        assert!(read_info(&mut Cursor::new(vec![0u8; 64]), 64).is_err());
    }
}
