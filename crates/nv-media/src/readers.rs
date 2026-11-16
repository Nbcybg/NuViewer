//! Source readers: one that hands out video frames as BGRA, one that hands out PCM sound.

use crate::load::{mf, Mf};
use std::path::Path;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

const VIDEO: u32 = 0xFFFF_FFFC; // MF_SOURCE_READER_FIRST_VIDEO_STREAM
const AUDIO: u32 = 0xFFFF_FFFD; // MF_SOURCE_READER_FIRST_AUDIO_STREAM
const ALL: u32 = 0xFFFF_FFFE; // MF_SOURCE_READER_ALL_STREAMS
const MEDIA_SOURCE: u32 = 0xFFFF_FFFF;
const TICKS_PER_MS: i64 = 10_000;
/// Output sound format: what the sound card gets.
pub const RATE: u32 = 48_000;
pub const CHANNELS: u32 = 2;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Top-down, 4 bytes a pixel (B, G, R, 255), `width * 4` bytes a row.
    pub bgra: Vec<u8>,
    pub ms: u64,
}

pub enum Read<T> {
    Item(T),
    End,
    Error(String),
}

fn unpack(v: u64) -> (u32, u32) {
    ((v >> 32) as u32, v as u32)
}

pub struct Probed {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub has_video: bool,
    pub has_audio: bool,
}

/// What is in the file, without decoding any of it.
pub fn probe(path: &Path) -> Result<Probed, String> {
    crate::load::init_thread();
    let api = mf()?;
    let r = api.reader(path)?;
    unsafe {
        let duration_ms = r
            .GetPresentationAttribute(MEDIA_SOURCE, &MF_PD_DURATION)
            .ok()
            .and_then(|v: PROPVARIANT| u64::try_from(&v).ok())
            .map(|t| t / TICKS_PER_MS as u64)
            .unwrap_or(0);
        let v = r.GetNativeMediaType(VIDEO, 0).ok();
        let has_audio = r.GetNativeMediaType(AUDIO, 0).is_ok();
        let (width, height) = v.as_ref().and_then(|t| t.GetUINT64(&MF_MT_FRAME_SIZE).ok()).map(unpack).unwrap_or((0, 0));
        let fps = v.as_ref().and_then(|t| t.GetUINT64(&MF_MT_FRAME_RATE).ok()).map(unpack).filter(|(_, d)| *d > 0).map(|(n, d)| n as f32 / d as f32).unwrap_or(0.0);
        Ok(Probed { duration_ms, width, height, fps, has_video: v.is_some() && width > 0, has_audio })
    }
}

fn select_only(r: &IMFSourceReader, stream: u32) -> Result<(), String> {
    unsafe {
        r.SetStreamSelection(ALL, false).map_err(|e| e.to_string())?;
        r.SetStreamSelection(stream, true).map_err(|e| e.to_string())
    }
}

fn set_type(api: &Mf, r: &IMFSourceReader, stream: u32, build: impl Fn(&IMFMediaType) -> windows::core::Result<()>) -> Result<(), String> {
    let t = api.media_type()?;
    unsafe {
        build(&t).map_err(|e| e.to_string())?;
        r.SetCurrentMediaType(stream, None, &t).map_err(|e| {
            if e.code().0 as u32 == 0xC00D_5212 {
                "Windows has no decoder for this video's format (10-bit 4:2:2 or HEVC camera files need ffmpeg or the HEVC Video Extensions)".to_string()
            } else {
                format!("no decoder for this stream ({e})")
            }
        })
    }
}

fn seek_reader(r: &IMFSourceReader, ms: u64) {
    unsafe {
        let _ = r.Flush(ALL);
        let pos = PROPVARIANT::from((ms as i64).saturating_mul(TICKS_PER_MS));
        let none = GUID::zeroed();
        let _ = r.SetCurrentPosition(&none, &pos);
    }
}

pub struct VideoReader {
    r: IMFSourceReader,
    pub width: u32,
    pub height: u32,
    stride: i32,
}

impl VideoReader {
    pub fn open(path: &Path) -> Result<VideoReader, String> {
        let api = mf()?;
        let r = api.reader(path)?;
        select_only(&r, VIDEO)?;
        set_type(api, &r, VIDEO, |t| unsafe {
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
        })?;
        let cur = unsafe { r.GetCurrentMediaType(VIDEO).map_err(|e| e.to_string())? };
        let (width, height) = unsafe { cur.GetUINT64(&MF_MT_FRAME_SIZE).map(unpack).map_err(|e| e.to_string())? };
        let stride = unsafe { cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or(width as i32 * 4) };
        Ok(VideoReader { r, width, height, stride })
    }

    pub fn seek(&self, ms: u64) {
        seek_reader(&self.r, ms);
    }

    /// The next frame. Stream-tick gaps and format changes are skipped over.
    pub fn read(&self) -> Read<Frame> {
        loop {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
            if let Err(e) = unsafe { self.r.ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample)) } {
                return Read::Error(e.to_string());
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Read::End;
            }
            let Some(sample) = sample else { continue };
            return match self.copy(&sample, (ts.max(0) / TICKS_PER_MS) as u64) {
                Ok(f) => Read::Item(f),
                Err(e) => Read::Error(e),
            };
        }
    }

    fn copy(&self, sample: &IMFSample, ms: u64) -> Result<Frame, String> {
        unsafe {
            let buf = sample.ConvertToContiguousBuffer().map_err(|e| e.to_string())?;
            let (mut p, mut max, mut cur) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
            buf.Lock(&mut p, Some(&mut max), Some(&mut cur)).map_err(|e| e.to_string())?;
            let (w, h) = (self.width as usize, self.height as usize);
            let stride = self.stride.unsigned_abs().max(w as u32 * 4) as usize;
            let mut out = vec![0u8; w * h * 4];
            if (cur as usize) >= stride * h - (stride - w * 4) {
                for y in 0..h {
                    let src_row = if self.stride < 0 { h - 1 - y } else { y };
                    let src = std::slice::from_raw_parts(p.add(src_row * stride), w * 4);
                    out[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
                }
                for px in out.chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }
            let _ = buf.Unlock();
            Ok(Frame { width: self.width, height: self.height, bgra: out, ms })
        }
    }
}

pub struct AudioReader {
    r: IMFSourceReader,
}

impl AudioReader {
    pub fn open(path: &Path) -> Result<AudioReader, String> {
        let api = mf()?;
        let r = api.reader(path)?;
        select_only(&r, AUDIO)?;
        set_type(api, &r, AUDIO, |t| unsafe {
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, CHANNELS)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, RATE)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, CHANNELS * 2)?;
            t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, RATE * CHANNELS * 2)
        })?;
        Ok(AudioReader { r })
    }

    pub fn seek(&self, ms: u64) {
        seek_reader(&self.r, ms);
    }

    /// The next block of 16-bit stereo samples.
    pub fn read(&self) -> Read<Vec<i16>> {
        loop {
            let (mut flags, mut sample) = (0u32, None);
            if let Err(e) = unsafe { self.r.ReadSample(AUDIO, 0, None, Some(&mut flags), None, Some(&mut sample)) } {
                return Read::Error(e.to_string());
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Read::End;
            }
            let Some(sample) = sample else { continue };
            unsafe {
                let Ok(buf) = sample.ConvertToContiguousBuffer() else { continue };
                let (mut p, mut cur) = (std::ptr::null_mut::<u8>(), 0u32);
                if buf.Lock(&mut p, None, Some(&mut cur)).is_err() {
                    continue;
                }
                let n = cur as usize / 2;
                let mut out = vec![0i16; n];
                std::ptr::copy_nonoverlapping(p as *const i16, out.as_mut_ptr(), n);
                let _ = buf.Unlock();
                return Read::Item(out);
            }
        }
    }
}

/// A still picture of a video: a frame a little way in (the very first one is often black). Straight from a seek, so it costs
/// one key frame of decoding.
pub fn poster_frame(path: &Path) -> Result<Frame, String> {
    crate::load::init_thread();
    let p = probe(path)?;
    if !p.has_video {
        return Err("no picture in this file".into());
    }
    let vr = VideoReader::open(path)?;
    let at = (p.duration_ms / 10).min(3000);
    if at > 0 {
        vr.seek(at);
    }
    for _ in 0..4 {
        match vr.read() {
            Read::Item(f) => return Ok(f),
            Read::End => break,
            Read::Error(e) => return Err(e),
        }
    }
    vr.seek(0);
    match vr.read() {
        Read::Item(f) => Ok(f),
        _ => Err("could not decode a picture from this video".into()),
    }
}

/// Lowest / highest sample per channel in `cols` columns across a whole sound file: `peaks[channel][col] = (min, max)`.
/// Up to 15 minutes are decoded through; longer files are sampled in short windows so a long recording still draws in
/// a few seconds.
pub fn audio_peaks(path: &Path, cols: usize) -> Result<Vec<Vec<(f32, f32)>>, String> {
    crate::load::init_thread();
    let p = probe(path)?;
    if !p.has_audio {
        return Err("no sound in this file".into());
    }
    let ar = AudioReader::open(path)?;
    let ch = CHANNELS as usize;
    let mut out = vec![vec![(0.0f32, 0.0f32); cols]; ch];
    let dur = p.duration_ms.max(1);
    let mut feed = |col: usize, block: &[i16]| {
        for fr in block.chunks_exact(ch) {
            for c in 0..ch {
                let v = fr[c] as f32 / 32768.0;
                let o = &mut out[c][col.min(cols - 1)];
                o.0 = o.0.min(v);
                o.1 = o.1.max(v);
            }
        }
    };
    if dur <= 15 * 60 * 1000 {
        let mut pos_frames = 0u64;
        let total = dur * RATE as u64 / 1000;
        loop {
            match ar.read() {
                Read::Item(b) => {
                    // Blocks are short (about 20 ms): each goes to the column its first frame falls in.
                    let col = (pos_frames as u128 * cols as u128 / total.max(1) as u128) as usize;
                    feed(col, &b);
                    pos_frames += (b.len() / ch) as u64;
                }
                _ => break,
            }
        }
    } else {
        for col in 0..cols {
            ar.seek(dur * col as u64 / cols as u64);
            for _ in 0..3 {
                if let Read::Item(b) = ar.read() {
                    feed(col, &b);
                }
            }
        }
    }
    Ok(out)
}
