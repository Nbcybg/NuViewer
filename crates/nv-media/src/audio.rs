//! Sound output through waveOut: a ring of buffers, `write` blocks (politely) while all of them are queued.

use crate::load::*;
use crate::readers::{CHANNELS, RATE};
use std::time::Duration;

const BUFFERS: usize = 6;
const BUFFER_BYTES: usize = 32 * 1024;

pub struct AudioOut {
    mm: &'static WinMm,
    h: isize,
    hdrs: Vec<Box<WaveHdr>>,
    data: Vec<Vec<u8>>,
    next: usize,
    paused: bool,
}

impl AudioOut {
    pub fn open() -> Result<AudioOut, String> {
        let mm = winmm().ok_or("the sound system (winmm) is not available")?;
        let fmt = WaveFormat { tag: 1, channels: CHANNELS as u16, rate: RATE, bytes_per_sec: RATE * CHANNELS * 2, block_align: (CHANNELS * 2) as u16, bits: 16, extra: 0 };
        let mut h = 0isize;
        let r = unsafe { (mm.open)(&mut h, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) };
        if r != 0 {
            return Err(format!("no sound device (error {r})"));
        }
        let mut data: Vec<Vec<u8>> = (0..BUFFERS).map(|_| vec![0u8; BUFFER_BYTES]).collect();
        let mut hdrs = Vec::new();
        for d in data.iter_mut() {
            let mut hdr = Box::new(WaveHdr { data: d.as_mut_ptr(), len: 0, recorded: 0, user: 0, flags: 0, loops: 0, next: std::ptr::null_mut(), reserved: 0 });
            unsafe { (mm.prepare)(h, &mut *hdr, std::mem::size_of::<WaveHdr>() as u32) };
            hdr.flags |= WHDR_DONE; // free
            hdrs.push(hdr);
        }
        Ok(AudioOut { mm, h, hdrs, data, next: 0, paused: false })
    }

    /// Queue `samples` (interleaved 16-bit stereo); waits while every buffer is still playing, as long as `keep_going()`.
    pub fn write(&mut self, samples: &[i16], keep_going: &dyn Fn() -> bool) {
        let bytes: &[u8] = unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2) };
        for chunk in bytes.chunks(BUFFER_BYTES) {
            loop {
                if self.hdrs[self.next].flags & WHDR_DONE != 0 {
                    break;
                }
                if !keep_going() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(4));
            }
            let i = self.next;
            self.data[i][..chunk.len()].copy_from_slice(chunk);
            let hdr = &mut self.hdrs[i];
            hdr.len = chunk.len() as u32;
            hdr.flags &= !WHDR_DONE;
            unsafe { (self.mm.write)(self.h, &mut **hdr, std::mem::size_of::<WaveHdr>() as u32) };
            self.next = (i + 1) % BUFFERS;
        }
    }

    /// Throw away everything queued (a seek, a speed change) and start counting played samples from zero.
    pub fn reset(&mut self) {
        unsafe { (self.mm.reset)(self.h) };
        self.next = 0;
        for h in self.hdrs.iter_mut() {
            h.flags |= WHDR_DONE;
        }
    }

    pub fn set_paused(&mut self, p: bool) {
        if p != self.paused {
            self.paused = p;
            unsafe { if p { (self.mm.pause)(self.h) } else { (self.mm.restart)(self.h) } };
        }
    }

    /// Frames played since the last `reset`.
    pub fn played_frames(&self) -> u64 {
        let mut t = [2u32, 0, 0, 0]; // TIME_SAMPLES
        unsafe { (self.mm.get_position)(self.h, &mut t, 16) };
        if t[0] == 2 { t[1] as u64 } else { 0 }
    }
}

impl Drop for AudioOut {
    fn drop(&mut self) {
        unsafe {
            (self.mm.reset)(self.h);
            for h in self.hdrs.iter_mut() {
                (self.mm.unprepare)(self.h, &mut **h, std::mem::size_of::<WaveHdr>() as u32);
            }
            (self.mm.close)(self.h);
        }
    }
}
