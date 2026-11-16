//! Standard MIDI files (`.mid`, `.midi`, `.rmi`, `.kar`). Windows has no player for them that fits the viewer's sound
//! pipeline, so a file is shown as a piano roll (what is played, when, on which channel) and played from a copy rendered to a
//! WAV by a small built-in synthesizer (a handful of simple timbres, drums, no sound font). The copy lives in the cache folder.

use crate::Image;
use std::path::{Path, PathBuf};

pub const EXTS: &[&str] = &["mid", "midi", "rmi", "kar"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

#[derive(Clone, Debug)]
pub struct Note {
    pub channel: u8,
    pub key: u8,
    pub velocity: u8,
    pub start: f64, // seconds
    pub end: f64,
    pub program: u8,
    pub volume: f32, // channel volume at the start (CC 7 * CC 11)
}

pub struct Song {
    pub notes: Vec<Note>,
    pub seconds: f64,
    pub tracks: usize,
    pub title: String,
    pub tempo_bpm: f64,
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}
fn be32(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn varlen(b: &[u8], at: &mut usize) -> Option<u32> {
    let mut v = 0u32;
    for _ in 0..4 {
        let c = *b.get(*at)?;
        *at += 1;
        v = (v << 7) | (c & 0x7F) as u32;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Ev {
    On(u8, u8, u8),
    Off(u8, u8),
    Program(u8, u8),
    Cc(u8, u8, u8),
}

/// Parse a Standard MIDI File (a RIFF "RMID" wrapper is skipped).
pub fn parse(file: &[u8]) -> Result<Song, String> {
    let mut b = file;
    if b.starts_with(b"RIFF") && b.len() > 20 && &b[8..12] == b"RMID" {
        let at = b.windows(4).position(|w| w == b"MThd").ok_or("midi: no MThd chunk in the RIFF wrapper")?;
        b = &b[at..];
    }
    if !b.starts_with(b"MThd") {
        return Err("midi: not a MIDI file (no MThd header)".into());
    }
    let hlen = be32(b, 4).ok_or("midi: truncated header")? as usize;
    let format = be16(b, 8).ok_or("midi: truncated header")?;
    let ntracks = be16(b, 10).ok_or("midi: truncated header")? as usize;
    let division = be16(b, 12).ok_or("midi: truncated header")?;
    let mut at = 8 + hlen.max(6);
    // Events of all tracks with absolute ticks.
    let mut events: Vec<(u64, Ev)> = Vec::new();
    let mut tempos: Vec<(u64, u32)> = vec![(0, 500_000)];
    let mut title = String::new();
    let mut tracks = 0;
    let mut end_tick = 0u64;
    let mut seq_offset = 0u64;
    while at + 8 <= b.len() && tracks < ntracks.max(1) + 64 {
        let tag = &b[at..at + 4];
        let len = be32(b, at + 4).unwrap_or(0) as usize;
        let body = b.get(at + 8..(at + 8 + len).min(b.len())).unwrap_or(&[]);
        at += 8 + len;
        if tag != b"MTrk" {
            continue;
        }
        tracks += 1;
        if format == 2 {
            // Format 2: independent sequences, played one after the other.
            seq_offset = end_tick;
        }
        let (mut i, mut tick, mut status) = (0usize, seq_offset, 0u8);
        while i < body.len() {
            let Some(delta) = varlen(body, &mut i) else { break };
            tick += delta as u64;
            let Some(&first) = body.get(i) else { break };
            if first & 0x80 != 0 {
                status = first;
                i += 1;
            }
            match status {
                0xFF => {
                    let Some(&kind) = body.get(i) else { break };
                    i += 1;
                    let Some(n) = varlen(body, &mut i) else { break };
                    let data = body.get(i..(i + n as usize).min(body.len())).unwrap_or(&[]);
                    i += n as usize;
                    match kind {
                        0x51 if data.len() == 3 => tempos.push((tick, u32::from_be_bytes([0, data[0], data[1], data[2]]).max(1))),
                        0x03 if title.is_empty() && tracks <= 1 => title = String::from_utf8_lossy(data).trim().to_string(),
                        0x2F => break,
                        _ => {}
                    }
                }
                0xF0 | 0xF7 => {
                    let Some(n) = varlen(body, &mut i) else { break };
                    i += n as usize;
                }
                s if s >= 0x80 => {
                    let ch = s & 0x0F;
                    let two = !matches!(s & 0xF0, 0xC0 | 0xD0);
                    let Some(&d1) = body.get(i) else { break };
                    let d2 = if two { body.get(i + 1).copied().unwrap_or(0) } else { 0 };
                    i += if two { 2 } else { 1 };
                    match s & 0xF0 {
                        0x90 if d2 > 0 => events.push((tick, Ev::On(ch, d1, d2))),
                        0x90 | 0x80 => events.push((tick, Ev::Off(ch, d1))),
                        0xC0 => events.push((tick, Ev::Program(ch, d1))),
                        0xB0 => events.push((tick, Ev::Cc(ch, d1, d2))),
                        _ => {}
                    }
                }
                _ => {
                    i += 1; // data byte without a status: damaged, skip it
                }
            }
        }
        end_tick = end_tick.max(tick);
    }
    if tracks == 0 {
        return Err("midi: the file has no tracks".into());
    }
    tempos.sort_by_key(|t| t.0);
    // Ticks -> seconds through the tempo map.
    let smpte = division & 0x8000 != 0;
    let ticks_to_sec = |t: u64| -> f64 {
        if smpte {
            let fps = match -(((division >> 8) as u8) as i8) {
                24 => 24.0,
                25 => 25.0,
                29 => 29.97,
                30 => 30.0,
                _ => 25.0,
            };
            return t as f64 / (fps * (division & 0xFF).max(1) as f64);
        }
        let ppq = division.max(1) as f64;
        let (mut secs, mut last_tick, mut tempo) = (0.0, 0u64, 500_000u32);
        for &(tt, tp) in &tempos {
            if tt >= t {
                break;
            }
            secs += (tt - last_tick) as f64 * tempo as f64 / 1e6 / ppq;
            last_tick = tt;
            tempo = tp;
        }
        secs + (t - last_tick) as f64 * tempo as f64 / 1e6 / ppq
    };
    events.sort_by_key(|e| e.0);
    let mut program = [0u8; 16];
    let (mut vol, mut expr) = ([100u8; 16], [127u8; 16]);
    let mut open: Vec<Vec<(usize, u64)>> = vec![Vec::new(); 16 * 128]; // (note index, start tick) per channel+key
    let mut notes: Vec<Note> = Vec::new();
    for (tick, ev) in events {
        match ev {
            Ev::Program(c, p) => program[c as usize] = p,
            Ev::Cc(c, 7, v) => vol[c as usize] = v,
            Ev::Cc(c, 11, v) => expr[c as usize] = v,
            Ev::On(c, k, v) => {
                if notes.len() >= 400_000 {
                    continue;
                }
                let t = ticks_to_sec(tick);
                notes.push(Note { channel: c, key: k, velocity: v, start: t, end: t, program: program[c as usize], volume: vol[c as usize] as f32 / 127.0 * expr[c as usize] as f32 / 127.0 });
                open[c as usize * 128 + k as usize & 0x7FF].push((notes.len() - 1, tick));
            }
            Ev::Off(c, k) => {
                let slot = &mut open[c as usize * 128 + k as usize & 0x7FF];
                if !slot.is_empty() {
                    let (idx, _) = slot.remove(0);
                    notes[idx].end = ticks_to_sec(tick).max(notes[idx].start + 0.02);
                }
            }
            Ev::Cc(..) => {}
        }
    }
    let total = ticks_to_sec(end_tick);
    for slot in &open {
        for &(idx, _) in slot {
            notes[idx].end = total.max(notes[idx].start + 0.2); // never released: sounds to the end
        }
    }
    let seconds = notes.iter().map(|n| n.end).fold(total, f64::max);
    let tempo_bpm = tempos.first().map(|t| 60_000_000.0 / t.1 as f64).unwrap_or(120.0);
    Ok(Song { notes, seconds, tracks, title, tempo_bpm })
}

fn channel_colour(c: u8) -> [u8; 3] {
    const P: [[u8; 3]; 16] = [
        [255, 99, 99], [255, 159, 64], [255, 205, 86], [160, 220, 90], [75, 192, 120], [60, 200, 200], [64, 160, 255], [110, 120, 255],
        [180, 120, 255], [255, 120, 200], [235, 235, 235], [255, 175, 150], [200, 200, 100], [120, 220, 170], [140, 190, 255], [210, 160, 230],
    ];
    P[c as usize & 15]
}

/// The piano roll: time across, pitch up; every channel its own colour; brighter = louder.
pub fn piano_roll(song: &Song, w: u32, h: u32) -> Image {
    let (w, h) = (w as usize, h as usize);
    let mut px = vec![0u8; w * h * 4];
    let bg = [16u8, 18, 24];
    for p in px.chunks_exact_mut(4) {
        p[..3].copy_from_slice(&bg);
        p[3] = 255;
    }
    let pitched: Vec<&Note> = song.notes.iter().filter(|n| n.channel != 9).collect();
    let (lo, hi) = pitched.iter().fold((127u8, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
    let (lo, hi) = if pitched.is_empty() { (36, 84) } else { (lo.saturating_sub(2), (hi + 3).min(127)) };
    let span = (hi - lo).max(24) as f64;
    let (margin_x, margin_y) = (w / 40, h / 12);
    let (pw, ph) = (w - 2 * margin_x, h - 2 * margin_y);
    let dur = song.seconds.max(1.0);
    let row_h = (ph as f64 / (span + 1.0)).max(1.0);
    // Octave lines (every C) and a second grid across.
    for k in lo..=hi {
        if k % 12 == 0 {
            let y = margin_y + ph - ((k - lo) as f64 / span * ph as f64) as usize;
            for x in margin_x..margin_x + pw {
                let o = (y.min(h - 1) * w + x) * 4;
                for c in 0..3 {
                    px[o + c] = px[o + c].saturating_add(18);
                }
            }
        }
    }
    let step = if dur > 600.0 { 120.0 } else if dur > 120.0 { 30.0 } else if dur > 30.0 { 10.0 } else { 2.0 };
    let mut t = 0.0;
    while t <= dur {
        let x = margin_x + (t / dur * pw as f64) as usize;
        for y in margin_y..margin_y + ph {
            let o = (y * w + x.min(w - 1)) * 4;
            for c in 0..3 {
                px[o + c] = px[o + c].saturating_add(10);
            }
        }
        t += step;
    }
    let mut draw = |x0: usize, x1: usize, y0: usize, y1: usize, col: [u8; 3]| {
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                let o = (y * w + x) * 4;
                px[o..o + 3].copy_from_slice(&col);
            }
        }
    };
    // Drums (channel 10) along the bottom strip, everything else by pitch.
    for n in &song.notes {
        let x0 = margin_x + (n.start / dur * pw as f64) as usize;
        let x1 = (margin_x + (n.end / dur * pw as f64) as usize).max(x0 + 1);
        let bright = 0.55 + 0.45 * (n.velocity as f32 / 127.0);
        let base = channel_colour(n.channel);
        let col = [(base[0] as f32 * bright) as u8, (base[1] as f32 * bright) as u8, (base[2] as f32 * bright) as u8];
        if n.channel == 9 {
            let y = margin_y + ph + margin_y / 4;
            draw(x0, x0 + 2, y, y + margin_y / 2, col);
        } else {
            let yc = margin_y as f64 + ph as f64 - ((n.key.clamp(lo, hi) - lo) as f64 / span * ph as f64);
            let y0 = (yc - row_h / 2.0).max(0.0) as usize;
            draw(x0, x1, y0, y0 + (row_h as usize).max(1), col);
        }
    }
    let mut img = Image::new(w as u32, h as u32, px);
    img.profile_assumable = true;
    let mins = song.seconds as u64 / 60;
    img.colour_note = Some(format!(
        "MIDI{}: {} track{}, {} notes, {}:{:02}, {:.0} BPM — piano roll; it plays through a built-in synthesizer",
        if song.title.is_empty() { String::new() } else { format!(" \"{}\"", song.title) },
        song.tracks,
        if song.tracks == 1 { "" } else { "s" },
        song.notes.len(),
        mins,
        song.seconds as u64 % 60,
        song.tempo_bpm
    ));
    img
}

pub fn decode(path: &Path, first_only: bool) -> Result<Image, String> {
    let bytes = crate::progress::read_file(path)?;
    let song = parse(&bytes)?;
    Ok(piano_roll(&song, if first_only { 640 } else { 1600 }, if first_only { 360 } else { 900 }))
}

// ---- the synthesizer ----------------------------------------------------------------------------------------------

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
enum Timbre {
    Piano,
    Organ,
    Pluck,
    Bass,
    Strings,
    Brass,
    Flute,
    Lead,
    Pad,
    Bell,
}

fn timbre_of(program: u8) -> Timbre {
    match program {
        0..=7 => Timbre::Piano,
        8..=15 => Timbre::Bell,
        16..=23 => Timbre::Organ,
        24..=31 => Timbre::Pluck,
        32..=39 => Timbre::Bass,
        40..=55 => Timbre::Strings,
        56..=71 => Timbre::Brass,
        72..=79 => Timbre::Flute,
        80..=87 => Timbre::Lead,
        88..=103 => Timbre::Pad,
        104..=111 => Timbre::Pluck,
        112..=119 => Timbre::Bell,
        _ => Timbre::Flute,
    }
}

/// (attack s, decay time constant s or 0 = sustained, release s)
fn envelope_of(t: Timbre) -> (f32, f32, f32) {
    match t {
        Timbre::Piano => (0.004, 1.6, 0.25),
        Timbre::Bell => (0.002, 1.2, 0.4),
        Timbre::Organ => (0.012, 0.0, 0.05),
        Timbre::Pluck => (0.003, 0.7, 0.12),
        Timbre::Bass => (0.006, 0.0, 0.08),
        Timbre::Strings => (0.09, 0.0, 0.25),
        Timbre::Brass => (0.04, 0.0, 0.1),
        Timbre::Flute => (0.05, 0.0, 0.12),
        Timbre::Lead => (0.01, 0.0, 0.08),
        Timbre::Pad => (0.2, 0.0, 0.4),
    }
}

fn wave(t: Timbre, ph: f32) -> f32 {
    use std::f32::consts::TAU;
    let s = |k: f32| (ph * TAU * k).sin();
    match t {
        Timbre::Piano => s(1.0) + 0.5 * s(2.0) + 0.25 * s(3.0) + 0.12 * s(4.0),
        Timbre::Bell => s(1.0) + 0.6 * s(2.76) + 0.3 * s(5.4),
        Timbre::Organ => s(1.0) + 0.7 * s(2.0) + 0.5 * s(3.0) + 0.3 * s(4.0),
        Timbre::Pluck => 1.5 * (ph.fract() * 2.0 - 1.0) * 0.5 + s(1.0) * 0.5,
        Timbre::Bass => 0.8 * (ph.fract() * 2.0 - 1.0) + 0.6 * s(1.0),
        Timbre::Strings => (ph.fract() * 2.0 - 1.0) * 0.6 + 0.3 * s(2.0),
        Timbre::Brass => (if ph.fract() < 0.5 { 1.0 } else { -1.0 }) * 0.5 + 0.5 * s(1.0),
        Timbre::Flute => s(1.0) + 0.15 * s(2.0),
        Timbre::Lead => (if ph.fract() < 0.35 { 1.0 } else { -1.0 }) * 0.55,
        Timbre::Pad => (ph.fract() * 2.0 - 1.0) * 0.4 + 0.5 * s(1.0),
    }
}

/// A tiny deterministic noise source for the drums.
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn render_drum(buf: &mut [f32], start: usize, key: u8, gain: f32, noise: &mut Noise) {
    use std::f32::consts::TAU;
    let n = match key {
        35 | 36 => (RATE as f32 * 0.28) as usize,
        38 | 40 => (RATE as f32 * 0.2) as usize,
        42 | 44 => (RATE as f32 * 0.06) as usize,
        46 => (RATE as f32 * 0.25) as usize,
        49 | 51 | 52 | 55 | 57 => (RATE as f32 * 0.6) as usize,
        _ => (RATE as f32 * 0.18) as usize,
    };
    let mut ph = 0.0f32;
    let mut lp = 0.0f32;
    for i in 0..n {
        let Some(o) = buf.get_mut(start + i) else { break };
        let t = i as f32 / RATE as f32;
        let env = (-t * (n as f32 / RATE as f32).recip() * 5.0).exp();
        let v = match key {
            35 | 36 => {
                ph += TAU * (45.0 + 110.0 * (-t * 30.0).exp()) / RATE as f32;
                ph.sin() * env * 1.2
            }
            38 | 40 => {
                ph += TAU * 190.0 / RATE as f32;
                ph.sin() * env * 0.5 + noise.next() * env * 0.7
            }
            42 | 44 | 46 | 49 | 51 | 52 | 55 | 57 => {
                let w = noise.next();
                let hp = w - lp;
                lp += 0.4 * (w - lp) * 0.0 + (w - lp) * 0.3; // crude high-pass
                hp * env * 0.45
            }
            41 | 43 | 45 | 47 | 48 | 50 => {
                ph += TAU * (90.0 + (key as f32 - 41.0) * 14.0) / RATE as f32;
                ph.sin() * env
            }
            _ => noise.next() * env * 0.4,
        };
        *o += v * gain;
    }
}

/// Render the song to 16-bit mono PCM.
pub fn synthesize(song: &Song) -> Vec<i16> {
    let total = (((song.seconds + 1.5).min(1200.0)) * RATE as f64) as usize;
    let mut buf = vec![0f32; total];
    let mut noise = Noise(0x1234_5678);
    for n in &song.notes {
        let start = (n.start * RATE as f64) as usize;
        if start >= total {
            continue;
        }
        let gain = (n.velocity as f32 / 127.0).powf(1.3) * n.volume.max(0.05) * 0.22;
        if n.channel == 9 {
            render_drum(&mut buf, start, n.key, gain * 2.2, &mut noise);
            continue;
        }
        let timbre = timbre_of(n.program);
        let (attack, decay, release) = envelope_of(timbre);
        let freq = 440.0 * 2f32.powf((n.key as f32 - 69.0) / 12.0);
        let held = ((n.end - n.start).max(0.03) * RATE as f64) as usize;
        let tail = (release * RATE as f32) as usize;
        // High notes decay faster on a piano; very long held notes of a decaying timbre fall silent early: cut them.
        let len = if decay > 0.0 { (held + tail).min((decay * 6.0 * RATE as f32) as usize + tail) } else { held + tail };
        let mut ph = 0.0f32;
        let dph = freq / RATE as f32;
        for i in 0..len {
            let Some(o) = buf.get_mut(start + i) else { break };
            let t = i as f32 / RATE as f32;
            let mut env = if t < attack { t / attack } else { 1.0 };
            if decay > 0.0 {
                env *= (-(t) / decay).exp();
            }
            if i >= held {
                env *= 1.0 - (i - held) as f32 / tail.max(1) as f32;
            }
            *o += wave(timbre, ph) * env * gain;
            ph += dph;
        }
    }
    // Soft limiter.
    buf.iter().map(|&v| ((v / (1.0 + v.abs() * 0.6)).clamp(-1.0, 1.0) * 30000.0) as i16).collect()
}

pub fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + data_len);
    out.extend(b"RIFF");
    out.extend((36 + data_len as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data_len as u32).to_le_bytes());
    for s in samples {
        out.extend(s.to_le_bytes());
    }
    out
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("NuViewer").join("midi")
}

/// A WAV the sound player can play for `path` (rendered once, then kept in the cache folder).
pub fn playable_wav(path: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in path.to_string_lossy().bytes().chain(mtime.to_le_bytes()).chain(meta.len().to_le_bytes()).chain(*b"v1") {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    let dest = cache_dir().join(format!("{h:016x}.wav"));
    if dest.is_file() {
        return Ok(dest);
    }
    let song = parse(&std::fs::read(path).map_err(|e| format!("read: {e}"))?)?;
    let wav = wav_bytes(&synthesize(&song));
    std::fs::create_dir_all(cache_dir()).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("tmp");
    std::fs::write(&tmp, wav).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-track file: tempo 120, division 480, middle C for one beat, then a drum hit.
    fn tiny() -> Vec<u8> {
        let mut t = Vec::new();
        t.extend([0x00, 0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]); // tempo 500000
        t.extend([0x00, 0xC0, 0x00]); // program 0 on channel 0
        t.extend([0x00, 0x90, 60, 100]);
        t.extend([0x83, 0x60, 0x80, 60, 0]); // delta 480 ticks, note off
        t.extend([0x00, 0x99, 36, 100]);
        t.extend([0x60, 0x89, 36, 0]);
        t.extend([0x00, 0xFF, 0x2F, 0x00]);
        let mut f = b"MThd".to_vec();
        f.extend(6u32.to_be_bytes());
        f.extend([0, 0, 0, 1, 0x01, 0xE0]);
        f.extend(b"MTrk");
        f.extend((t.len() as u32).to_be_bytes());
        f.extend(t);
        f
    }

    #[test]
    fn notes_get_their_times_from_the_tempo_map() {
        let s = parse(&tiny()).unwrap();
        assert_eq!(s.notes.len(), 2);
        assert_eq!((s.notes[0].key, s.notes[0].channel), (60, 0));
        assert!((s.notes[0].end - 0.5).abs() < 1e-6, "one beat at 120 BPM is half a second: {}", s.notes[0].end);
        assert_eq!(s.notes[1].channel, 9);
        assert!((s.tempo_bpm - 120.0).abs() < 1e-6);
    }

    #[test]
    fn synthesis_makes_sound_and_a_valid_wav() {
        let s = parse(&tiny()).unwrap();
        let pcm = synthesize(&s);
        assert!(pcm.len() as f64 > s.seconds * RATE as f64);
        assert!(pcm.iter().any(|v| v.abs() > 500), "silent");
        let wav = wav_bytes(&pcm);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize, pcm.len() * 2);
        let img = piano_roll(&s, 320, 180);
        assert_eq!((img.width, img.height), (320, 180));
        assert!(img.rgba.chunks_exact(4).any(|p| p[..3] != [16, 18, 24]), "no notes drawn");
    }

    #[test]
    fn not_midi_is_an_error() {
        assert!(parse(b"hello").is_err());
    }
}
