//! Spine's binary skeleton (`.skel`) for 3.8, 4.0 and 4.1.
//!
//! **Not verified against Esoteric's files.** This follows the documented layout as far as it is public and as the format is understood; no real
//! `.skel` was available when it was written. Two things keep a wrong guess from showing garbage: every read is bounds-checked and every index
//! is range-checked, and the whole file must be used up exactly (a reader that is out of step ends elsewhere than the end). 3.8 is read both with
//! and without a string table (the first that fits wins). Not read, with a message that says to export JSON instead: **4.2** binary files
//! (their constraint and bone encodings changed), skeletons with **path constraints**, and anything with a version this does not know.

use crate::atlas::Atlas;
use crate::model::*;
use std::collections::HashMap;

struct R<'a> {
    b: &'a [u8],
    p: usize,
    strings: Option<Vec<Option<String>>>,
}

type Res<T> = Result<T, String>;

impl<'a> R<'a> {
    fn eof<T>(&self) -> Res<T> {
        Err(format!("skel: the file ends too early (at byte {})", self.p))
    }
    fn byte(&mut self) -> Res<u8> {
        match self.b.get(self.p) {
            Some(v) => {
                self.p += 1;
                Ok(*v)
            }
            None => self.eof(),
        }
    }
    fn sbyte(&mut self) -> Res<i8> {
        Ok(self.byte()? as i8)
    }
    fn bool(&mut self) -> Res<bool> {
        Ok(self.byte()? != 0)
    }
    fn int(&mut self) -> Res<i32> {
        let s = self.b.get(self.p..self.p + 4).ok_or_else(|| format!("skel: the file ends too early (at byte {})", self.p))?;
        self.p += 4;
        Ok(i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn short(&mut self) -> Res<u16> {
        let s = self.b.get(self.p..self.p + 2).ok_or_else(|| format!("skel: the file ends too early (at byte {})", self.p))?;
        self.p += 2;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn float(&mut self) -> Res<f32> {
        Ok(f32::from_bits(self.int()? as u32))
    }
    /// Unsigned varint (Spine's `readInt(true)`), 7 bits at a time, low bits first.
    fn var(&mut self) -> Res<usize> {
        let mut v = 0usize;
        for k in 0..5 {
            let b = self.byte()?;
            v |= ((b & 0x7F) as usize) << (7 * k);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("skel: a number is too long".into())
    }
    /// Signed varint (zig-zag, Spine's `readInt(false)`).
    fn zig(&mut self) -> Res<i32> {
        let v = self.var()? as u32;
        Ok(((v >> 1) as i32) ^ -((v & 1) as i32))
    }
    /// A count that must be sane for what is left of the file (each item takes at least `each` bytes).
    fn count(&mut self, each: usize) -> Res<usize> {
        let n = self.var()?;
        if n.saturating_mul(each.max(1)) > self.b.len() - self.p.min(self.b.len()) + 16 {
            return Err(format!("skel: an implausible count ({n}) at byte {}", self.p));
        }
        Ok(n)
    }
    fn string(&mut self) -> Res<Option<String>> {
        let n = self.var()?;
        match n {
            0 => Ok(None),
            1 => Ok(Some(String::new())),
            n => {
                let s = self.b.get(self.p..self.p + n - 1).ok_or_else(|| "skel: a string runs past the end of the file".to_string())?;
                self.p += n - 1;
                Ok(Some(String::from_utf8_lossy(s).into_owned()))
            }
        }
    }
    fn sref(&mut self) -> Res<Option<String>> {
        if self.strings.is_none() {
            return self.string();
        }
        let i = self.var()?;
        if i == 0 {
            return Ok(None);
        }
        self.strings.as_ref().and_then(|t| t.get(i - 1).cloned()).ok_or_else(|| format!("skel: string reference {i} is outside the string table"))
    }
    fn floats(&mut self, n: usize) -> Res<Vec<f32>> {
        if n * 4 > self.b.len() - self.p.min(self.b.len()) {
            return self.eof();
        }
        (0..n).map(|_| self.float()).collect()
    }
    fn shorts(&mut self) -> Res<Vec<u16>> {
        let n = self.count(2)?;
        (0..n).map(|_| self.short()).collect()
    }
    fn rgba(&mut self) -> Res<[f32; 4]> {
        let v = self.int()? as u32;
        Ok([(v >> 24) as f32 / 255.0, ((v >> 16) & 0xFF) as f32 / 255.0, ((v >> 8) & 0xFF) as f32 / 255.0, (v & 0xFF) as f32 / 255.0])
    }
    fn rgb_int(&mut self) -> Res<[f32; 3]> {
        let v = self.int()? as u32;
        Ok([((v >> 16) & 0xFF) as f32 / 255.0, ((v >> 8) & 0xFF) as f32 / 255.0, (v & 0xFF) as f32 / 255.0])
    }
    fn index(&mut self, n: usize, what: &str) -> Res<usize> {
        let i = self.var()?;
        if i < n {
            Ok(i)
        } else {
            Err(format!("skel: a {what} index ({i}) is out of range ({n}) at byte {}", self.p))
        }
    }
}

#[derive(Clone, Copy)]
enum Raw {
    Linear,
    Stepped,
    Bez([f32; 4]),
}

/// Keys being collected: values per key, and the curve leaving each key per channel.
struct Keys {
    stride: usize,
    times: Vec<f32>,
    vals: Vec<f32>,
    raws: Vec<Raw>,
}

impl Keys {
    fn new(stride: usize) -> Keys {
        Keys { stride, times: Vec::new(), vals: Vec::new(), raws: Vec::new() }
    }
    fn push(&mut self, t: f32, v: &[f32]) {
        self.times.push(t);
        self.vals.extend_from_slice(v);
        self.raws.extend(std::iter::repeat(Raw::Linear).take(self.stride));
    }
    fn set_raw(&mut self, frame: usize, ch: usize, r: Raw) {
        let s = self.stride;
        self.raws[frame * s + ch] = r;
    }
    fn track(&self, absolute: bool) -> Track {
        let s = self.stride;
        let mut t = Track::new(s);
        for i in 0..self.times.len() {
            let mut curves = Vec::with_capacity(s);
            for ch in 0..s {
                let raw = self.raws[i * s + ch];
                curves.push(if i + 1 >= self.times.len() { Curve::Linear } else { norm(raw, absolute, self.times[i], self.times[i + 1], self.vals[i * s + ch], self.vals[(i + 1) * s + ch]) });
            }
            t.push(self.times[i], &self.vals[i * s..i * s + s], &curves);
        }
        t
    }
}

fn norm(raw: Raw, absolute: bool, t0: f32, t1: f32, v0: f32, v1: f32) -> Curve {
    match raw {
        Raw::Linear => Curve::Linear,
        Raw::Stepped => Curve::Stepped,
        Raw::Bez(c) if !absolute => Curve::Bezier(c),
        Raw::Bez(c) => {
            if t1 - t0 <= 1e-9 {
                Curve::Linear
            } else {
                let _ = (v0, v1);
                Curve::BezierAbs(c)
            }
        }
    }
}

struct Ver {
    legacy: bool, // 3.x
    minor41: bool, // 4.1 and later: the split bone timelines, rgb/alpha slot timelines
    v42: bool,     // 4.2: flag bytes for constraints and attachments, a reference scale, physics constraints
}

pub fn parse(bytes: &[u8], atlas: Atlas) -> Result<Rig, String> {
    // Which version? 3.x writes its hash as a string, 4.x as 8 raw bytes; both are followed by the version string.
    let try3 = {
        let mut r = R { b: bytes, p: 0, strings: None };
        let _hash = r.string();
        r.string().ok().flatten().filter(|v| v.starts_with("3."))
    };
    let (version, body_at) = if let Some(v) = try3 {
        let mut r = R { b: bytes, p: 0, strings: None };
        let _ = r.string();
        let _ = r.string();
        (v, r.p)
    } else {
        let mut r = R { b: bytes, p: 8, strings: None };
        let v = r.string().ok().flatten().filter(|v| v.starts_with("4.") || v.starts_with("3.")).ok_or("skel: not a Spine skeleton (no version found)")?;
        (v, r.p)
    };
    let mut parts = version.split('.');
    let (major, minor): (u32, u32) = (parts.next().and_then(|m| m.parse().ok()).unwrap_or(0), parts.next().and_then(|m| m.parse().ok()).unwrap_or(0));
    if !(major == 3 && minor >= 8 || major == 4 && minor <= 2) {
        return Err(format!("skel: Spine {version} is not supported (3.8 - 4.2 binary; this file is newer: use the JSON export)"));
    }
    let ver = Ver { legacy: major == 3, minor41: major == 4 && minor >= 1, v42: major == 4 && minor >= 2 };
    // 3.8 has been seen with and without a string table: try the likelier layout first.
    let mut last_err = String::new();
    for table in [true, false] {
        if !table && !ver.legacy {
            break;
        }
        match body(bytes, body_at, &version, &ver, table, atlas.clone()) {
            Ok(rig) => return Ok(rig),
            Err(e) => last_err = e,
        }
    }
    Err(format!("{last_err} (Spine {version} binary: this reader is unverified; export JSON from Spine if it fails)"))
}

fn body(bytes: &[u8], at: usize, version: &str, ver: &Ver, table: bool, atlas: Atlas) -> Res<Rig> {
    let mut r = R { b: bytes, p: at, strings: None };
    let (x, y, width, height) = (r.float()?, r.float()?, r.float()?, r.float()?);
    if ver.v42 {
        r.float()?; // the reference scale
    }
    let nonessential = r.bool()?;
    let mut fps = 30.0;
    if nonessential {
        fps = r.float()?;
        let _images = r.string()?;
        let _audio = r.string()?;
    }
    if table {
        let n = r.count(1)?;
        let mut t = Vec::with_capacity(n);
        for _ in 0..n {
            t.push(r.string()?);
        }
        r.strings = Some(t);
    }
    // Bones.
    trace(&r, "before bones");
    let nb = r.count(30)?;
    let mut bones: Vec<BoneData> = Vec::with_capacity(nb);
    for i in 0..nb {
        let name = r.string()?.unwrap_or_default();
        let parent = if i == 0 { None } else { Some(r.index(i, "parent bone")?) };
        let (rotation, bx, by, scale_x, scale_y, shear_x, shear_y, length) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
        let inherit = Inherit::from_index(r.var()? as u8);
        let skin_required = r.bool()?;
        if nonessential {
            r.int()?;
            if ver.v42 {
                r.string()?; // icon
                r.bool()?; // visible in the editor
            }
        }
        bones.push(BoneData { name, parent, length, x: bx, y: by, rotation, scale_x, scale_y, shear_x, shear_y, inherit, skin_required });
    }
    if bones.is_empty() {
        return Err("skel: no bones".into());
    }
    trace(&r, &format!("bones {}", bones.len()));
    // Slots.
    let ns = r.count(8)?;
    let mut slots: Vec<SlotData> = Vec::with_capacity(ns);
    for _ in 0..ns {
        let name = r.string()?.unwrap_or_default();
        let bone = r.index(bones.len(), "slot bone")?;
        let color = r.rgba()?;
        let dark_raw = r.int()?;
        let dark = if dark_raw == -1 { None } else { Some([((dark_raw >> 16) & 0xFF) as f32 / 255.0, ((dark_raw >> 8) & 0xFF) as f32 / 255.0, (dark_raw & 0xFF) as f32 / 255.0]) };
        let attachment = r.sref()?;
        let blend = BlendMode::from_index(r.var()? as u8);
        if nonessential && ver.v42 {
            r.bool()?; // visible in the editor
        }
        slots.push(SlotData { name, bone, color, dark, attachment, blend });
    }
    trace(&r, &format!("slots {}", slots.len()));
    // IK constraints.
    let ni = r.count(8)?;
    let mut ik = Vec::with_capacity(ni);
    for _ in 0..ni {
        let name = r.string()?.unwrap_or_default();
        let order = r.var()? as i32;
        if ver.v42 {
            let nbn = r.count(1)?;
            if nbn == 0 || nbn > 2 {
                return Err("skel: an IK constraint with an unusual bone count".into());
            }
            let mut bs = Vec::new();
            for _ in 0..nbn {
                bs.push(r.index(bones.len(), "IK bone")?);
            }
            let target = r.index(bones.len(), "IK target")?;
            let flags = r.byte()?;
            // Three states for the mix: absent (0), exactly 1, or stored.
            let mix = if flags & 32 != 0 { if flags & 64 != 0 { r.float()? } else { 1.0 } } else { 0.0 };
            let softness = if flags & 128 != 0 { r.float()? } else { 0.0 };
            ik.push(IkData { name, order, bones: bs, target, mix, softness, bend_positive: flags & 2 != 0, compress: flags & 4 != 0, stretch: flags & 8 != 0, uniform: flags & 16 != 0, skin_required: flags & 1 != 0 });
            continue;
        }
        let skin_required = r.bool()?;
        let nbn = r.count(1)?;
        if nbn == 0 || nbn > 2 {
            return Err("skel: an IK constraint with an unusual bone count".into());
        }
        let mut bs = Vec::new();
        for _ in 0..nbn {
            bs.push(r.index(bones.len(), "IK bone")?);
        }
        let target = r.index(bones.len(), "IK target")?;
        let (mix, softness) = (r.float()?, r.float()?);
        let bend = r.sbyte()?;
        let (compress, stretch, uniform) = (r.bool()?, r.bool()?, r.bool()?);
        ik.push(IkData { name, order, bones: bs, target, mix, softness, bend_positive: bend >= 0, compress, stretch, uniform, skin_required });
    }
    // Transform constraints.
    let nt = r.count(8)?;
    let mut transform = Vec::with_capacity(nt);
    for _ in 0..nt {
        let name = r.string()?.unwrap_or_default();
        let order = r.var()? as i32;
        if ver.v42 {
            let nbn = r.count(1)?;
            let mut bs = Vec::new();
            for _ in 0..nbn {
                bs.push(r.index(bones.len(), "transform bone")?);
            }
            let target = r.index(bones.len(), "transform target")?;
            let f1 = r.byte()?;
            let opt = |r: &mut R, on: bool| -> Res<f32> { if on { r.float() } else { Ok(0.0) } };
            let (rotation, tx, ty, sx, sy) = (opt(&mut r, f1 & 8 != 0)?, opt(&mut r, f1 & 16 != 0)?, opt(&mut r, f1 & 32 != 0)?, opt(&mut r, f1 & 64 != 0)?, opt(&mut r, f1 & 128 != 0)?);
            let f2 = r.byte()?;
            let shy = opt(&mut r, f2 & 1 != 0)?;
            let (mr, mx, my, msx, msy, msh) = (opt(&mut r, f2 & 2 != 0)?, opt(&mut r, f2 & 4 != 0)?, opt(&mut r, f2 & 8 != 0)?, opt(&mut r, f2 & 16 != 0)?, opt(&mut r, f2 & 32 != 0)?, opt(&mut r, f2 & 64 != 0)?);
            transform.push(TransformData { name, order, bones: bs, target, rotation, x: tx, y: ty, scale_x: sx, scale_y: sy, shear_y: shy, mix_rotate: mr, mix_x: mx, mix_y: my, mix_scale_x: msx, mix_scale_y: msy, mix_shear_y: msh, local: f1 & 2 != 0, relative: f1 & 4 != 0, skin_required: f1 & 1 != 0 });
            continue;
        }
        let skin_required = r.bool()?;
        let nbn = r.count(1)?;
        let mut bs = Vec::new();
        for _ in 0..nbn {
            bs.push(r.index(bones.len(), "transform bone")?);
        }
        let target = r.index(bones.len(), "transform target")?;
        let (local, relative) = (r.bool()?, r.bool()?);
        let (rotation, tx, ty, sx, sy, shy) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
        let (mr, mx, my, msx, msy, msh);
        if ver.minor41 {
            (mr, mx, my, msx, msy, msh) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
        } else {
            let (rm, tm, sm, hm) = (r.float()?, r.float()?, r.float()?, r.float()?);
            (mr, mx, my, msx, msy, msh) = (rm, tm, tm, sm, sm, hm);
        }
        transform.push(TransformData { name, order, bones: bs, target, rotation, x: tx, y: ty, scale_x: sx, scale_y: sy, shear_y: shy, mix_rotate: mr, mix_x: mx, mix_y: my, mix_scale_x: msx, mix_scale_y: msy, mix_shear_y: msh, local, relative, skin_required });
    }
    trace(&r, &format!("ik {} transform {}", ik.len(), transform.len()));
    // Path constraints: only their presence matters.
    let np = r.count(8)?;
    if np > 0 {
        return Err("skel: this skeleton has path constraints, which the binary reader does not read: export JSON".into());
    }
    // Physics constraints (4.2): read to stay in step, not applied.
    let mut unsupported: Vec<String> = Vec::new();
    let mut physics_count = 0usize;
    if ver.v42 {
        physics_count = r.count(8)?;
        for _ in 0..physics_count {
            r.string()?; // name
            r.var()?; // order
            r.index(bones.len(), "physics bone")?;
            let flags = r.byte()?;
            for bit in [2u8, 4, 8, 16, 32, 64] {
                if flags & bit != 0 {
                    r.float()?;
                }
            }
            r.byte()?; // step
            r.float()?; // inertia
            r.float()?; // strength
            r.float()?; // damping
            if flags & 128 != 0 {
                r.float()?; // mass
            }
            r.float()?; // wind
            r.float()?; // gravity
            let flags2 = r.byte()?;
            if flags2 & 128 != 0 {
                r.float()?; // mix
            }
        }
        if physics_count > 0 {
            unsupported.push("physics constraints".into());
        }
    }
    // Skins.
    let mut skins: Vec<Skin> = Vec::new();
    let mut linked: Vec<(usize, usize, String, String, String, bool, String, [f32; 4], f32, f32)> = Vec::new();
    trace(&r, "before skins");
    let default_slots = r.var()?;
    trace(&r, &format!("default skin slots {default_slots}"));
    skins.push(Skin { name: "default".into(), slots: vec![HashMap::new(); slots.len()], ..Default::default() });
    if default_slots > 0 {
        read_skin_slots(&mut r, ver.minor41, ver.v42, &mut skins, 0, default_slots, &bones, &slots, nonessential, &mut linked)?;
    }
    let nsk = r.count(2)?;
    for _ in 0..nsk {
        let name = if ver.legacy { r.sref()? } else { r.string()? }.unwrap_or_default();
        let mut skin = Skin { name, slots: vec![HashMap::new(); slots.len()], ..Default::default() };
        if ver.v42 && nonessential {
            r.int()?; // the skin's colour
        }
        if !ver.legacy {
            let nbn = r.count(1)?;
            for _ in 0..nbn {
                skin.bones.push(r.index(bones.len(), "skin bone")?);
            }
            let nik = r.count(1)?;
            for _ in 0..nik {
                skin.ik.push(r.index(ik.len(), "skin IK")?);
            }
            let ntf = r.count(1)?;
            for _ in 0..ntf {
                skin.transform.push(r.index(transform.len(), "skin transform")?);
            }
            let npa = r.count(1)?;
            for _ in 0..npa {
                r.var()?;
            }
            if ver.v42 {
                let nph = r.count(1)?;
                for _ in 0..nph {
                    r.var()?;
                }
            }
        }
        let count = r.var()?;
        skins.push(skin);
        let si = skins.len() - 1;
        read_skin_slots(&mut r, ver.minor41, ver.v42, &mut skins, si, count, &bones, &slots, nonessential, &mut linked)?;
    }
    resolve_linked(&mut skins, linked);
    // Event definitions (read to stay in step).
    let ne = r.count(3)?;
    let mut event_audio: Vec<bool> = Vec::new();
    for _ in 0..ne {
        let _name = if ver.legacy { r.sref()? } else { r.string()? };
        let _int = r.zig()?;
        let _float = r.float()?;
        let _string = r.string()?;
        let audio = r.string()?;
        if audio.is_some() {
            r.float()?;
            r.float()?;
        }
        event_audio.push(audio.is_some());
    }
    // Animations.
    let na = r.count(2)?;
    let mut animations = Vec::with_capacity(na);
    for _ in 0..na {
        let name = r.string()?.unwrap_or_default();
        animations.push(read_animation(&mut r, name, ver, &bones, &slots, &skins, &ik, &transform, &event_audio)?);
    }
    if r.p != bytes.len() {
        return Err(format!("skel: {} bytes are left over after the animations (the reader is out of step)", bytes.len() - r.p));
    }
    animations.sort_by(|a, b| a.name.cmp(&b.name));
    let _ = physics_count;
    Ok(Rig { version: version.to_string(), name: String::new(), x, y, width, height, fps, bones, slots, skins, ik, transform, animations, atlas, legacy: ver.legacy, unsupported })
}

fn trace(r: &R, what: &str) {
    if std::env::var_os("NV_SKEL_TRACE").is_some() {
        eprintln!("skel@{}/{}: {what}", r.p, r.b.len());
    }
}

/// 4.2's image sequence: four numbers (the flag that says it is there is in the attachment's flag byte).
fn skip_sequence42(r: &mut R) -> Res<()> {
    for _ in 0..4 {
        r.var()?;
    }
    Ok(())
}

/// One attachment in the 4.2 layout: a flag byte (type in the low 3 bits, the rest say which optional parts follow).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn read_attachment42(r: &mut R, key: &str, si: usize, slot: usize, bones: &[BoneData], slots: &[SlotData], nonessential: bool, linked: &mut Vec<(usize, usize, String, String, String, bool, String, [f32; 4], f32, f32)>) -> Res<Option<Attachment>> {
    let flags = r.byte()?;
    let name = if flags & 8 != 0 { r.sref()?.unwrap_or_else(|| key.to_string()) } else { key.to_string() };
    let ty = flags & 7;
    trace(r, &format!("attachment42 slot {slot} key {key} name {name} type {ty}"));
    let weighted_flag = |bit: u8| flags & bit != 0;
    let read_verts = |r: &mut R, weighted: bool| -> Res<(usize, Vertices)> {
        let n = r.count(2)?;
        if !weighted {
            return Ok((n, Vertices::Plain(r.floats(n * 2)?)));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let bc = r.count(12)?;
            let mut inf = Vec::with_capacity(bc);
            for _ in 0..bc {
                inf.push(Influence { bone: r.var()?, x: r.float()?, y: r.float()?, weight: r.float()? });
            }
            out.push(inf);
        }
        Ok((n, Vertices::Weighted(out)))
    };
    Ok(match ty {
        0 => {
            let path = if flags & 16 != 0 { r.sref()?.unwrap_or_else(|| name.clone()) } else { name.clone() };
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            if flags & 64 != 0 {
                skip_sequence42(r)?;
            }
            let rotation = if flags & 128 != 0 { r.float()? } else { 0.0 };
            let (x, y, sx, sy, width, height) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
            Some(Attachment::Region(Region { path, x, y, rotation, scale_x: sx, scale_y: sy, width, height, color }))
        }
        1 => {
            read_verts(r, weighted_flag(16))?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Other)
        }
        2 => {
            let path = if flags & 16 != 0 { r.sref()?.unwrap_or_else(|| name.clone()) } else { name.clone() };
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            if flags & 64 != 0 {
                skip_sequence42(r)?;
            }
            let hull = r.var()?;
            let (vc, vertices) = read_verts(r, weighted_flag(128))?;
            let uvs = r.floats(vc * 2)?;
            let tri_count = (vc * 2).saturating_sub(hull + 2) * 3;
            if tri_count > r.b.len() {
                return Err("skel: a mesh with an implausible triangle count".into());
            }
            let mut triangles = Vec::with_capacity(tri_count);
            for _ in 0..tri_count {
                triangles.push(r.var()? as u16);
            }
            let (mut width, mut height) = (0.0, 0.0);
            if nonessential {
                let ec = r.count(1)?;
                for _ in 0..ec {
                    r.var()?;
                }
                width = r.float()?;
                height = r.float()?;
            }
            if triangles.iter().any(|&t| t as usize >= vc) {
                return Err("skel: a mesh triangle points at a vertex that does not exist".into());
            }
            if let Vertices::Weighted(w) = &vertices {
                if w.iter().flatten().any(|i| i.bone >= bones.len()) {
                    return Err("skel: a mesh is weighted to a bone that does not exist".into());
                }
            }
            Some(Attachment::Mesh(Mesh { path, color, width, height, uvs, triangles, vertices, hull, deform_skin: si, deform_name: key.to_string() }))
        }
        3 => {
            let path = if flags & 16 != 0 { r.sref()?.unwrap_or_else(|| name.clone()) } else { name.clone() };
            let color = if flags & 32 != 0 { r.rgba()? } else { [1.0; 4] };
            if flags & 64 != 0 {
                skip_sequence42(r)?;
            }
            let deform = flags & 128 != 0;
            let skin_index = r.var()?;
            let parent = r.sref()?.unwrap_or_default();
            let (mut w, mut h) = (0.0, 0.0);
            if nonessential {
                w = r.float()?;
                h = r.float()?;
            }
            // The skin is given by number; "" means the skin being read is not known yet, so the default (0) stands in when it is out of range.
            linked.push((si, slot, key.to_string(), parent, format!("#{skin_index}"), deform, path, color, w, h));
            None
        }
        4 => {
            let (vc, _) = read_verts(r, weighted_flag(64))?;
            r.floats(vc * 2 / 6)?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Other)
        }
        5 => {
            r.float()?;
            r.float()?;
            r.float()?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Other)
        }
        6 => {
            let end_slot = r.index(slots.len(), "clipping end slot")?;
            let (vc, vertices) = read_verts(r, weighted_flag(16))?;
            if nonessential {
                r.int()?;
            }
            Some(Attachment::Clip(Clip { end_slot, vertices, vertex_count: vc }))
        }
        t => return Err(format!("skel: unknown attachment type {t}")),
    })
}

/// 4.1's image-sequence block: a flag, then four numbers.
fn skip_sequence(r: &mut R) -> Res<()> {
    if r.bool()? {
        for _ in 0..4 {
            r.var()?;
        }
    }
    Ok(())
}

fn read_vertices(r: &mut R, count: usize) -> Res<Vertices> {
    let weighted = r.bool()?;
    if !weighted {
        return Ok(Vertices::Plain(r.floats(count * 2)?));
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let n = r.count(12)?;
        let mut inf = Vec::with_capacity(n);
        for _ in 0..n {
            inf.push(Influence { bone: r.var()?, x: r.float()?, y: r.float()?, weight: r.float()? });
        }
        out.push(inf);
    }
    Ok(Vertices::Weighted(out))
}

#[allow(clippy::too_many_arguments)]
fn read_skin_slots(r: &mut R, sequences: bool, v42: bool, skins: &mut [Skin], si: usize, slot_count: usize, bones: &[BoneData], slots: &[SlotData], nonessential: bool, linked: &mut Vec<(usize, usize, String, String, String, bool, String, [f32; 4], f32, f32)>) -> Res<()> {
    for _ in 0..slot_count {
        let slot = r.index(slots.len(), "skin slot")?;
        let n = r.count(2)?;
        for _ in 0..n {
            let key = r.sref()?.unwrap_or_default();
            if v42 {
                if let Some(att) = read_attachment42(r, &key, si, slot, bones, slots, nonessential, linked)? {
                    skins[si].slots[slot].insert(key, att);
                }
                continue;
            }
            let name = r.sref()?.unwrap_or_else(|| key.clone());
            let ty = r.byte()?;
            trace(r, &format!("attachment slot {slot} key {key} name {name} type {ty}"));
            let att = match ty {
                0 => {
                    let path = r.sref()?.unwrap_or_else(|| name.clone());
                    if sequences {
                        skip_sequence(r)?;
                    }
                    let (rotation, x, y, sx, sy, width, height) = (r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?);
                    let color = r.rgba()?;
                    Attachment::Region(Region { path, x, y, rotation, scale_x: sx, scale_y: sy, width, height, color })
                }
                1 => {
                    let n = r.var()?;
                    read_vertices(r, n)?;
                    if nonessential {
                        r.int()?;
                    }
                    Attachment::Other
                }
                2 => {
                    let path = r.sref()?.unwrap_or_else(|| name.clone());
                    let color = r.rgba()?;
                    let vc = r.count(4)?;
                    let uvs = r.floats(vc * 2)?;
                    let triangles = r.shorts()?;
                    let vertices = read_vertices(r, vc)?;
                    if let Vertices::Weighted(w) = &vertices {
                        if w.iter().flatten().any(|i| i.bone >= bones.len()) {
                            return Err("skel: a mesh is weighted to a bone that does not exist".into());
                        }
                    }
                    let hull = r.var()?;
                    if sequences {
                        skip_sequence(r)?;
                    }
                    let (mut width, mut height) = (0.0, 0.0);
                    if nonessential {
                        r.shorts()?;
                        width = r.float()?;
                        height = r.float()?;
                    }
                    if triangles.iter().any(|&t| t as usize >= vc) {
                        return Err("skel: a mesh triangle points at a vertex that does not exist".into());
                    }
                    Attachment::Mesh(Mesh { path, color, width, height, uvs, triangles, vertices, hull, deform_skin: si, deform_name: key.clone() })
                }
                3 => {
                    let path = r.sref()?.unwrap_or_else(|| name.clone());
                    let color = r.rgba()?;
                    let skin = r.sref()?.unwrap_or_else(|| "default".into());
                    let parent = r.sref()?.unwrap_or_default();
                    let deform = r.bool()?;
                    if sequences {
                        skip_sequence(r)?;
                    }
                    let (mut w, mut h) = (0.0, 0.0);
                    if nonessential {
                        w = r.float()?;
                        h = r.float()?;
                    }
                    linked.push((si, slot, key.clone(), parent, skin, deform, path, color, w, h));
                    continue;
                }
                4 => {
                    let _closed = r.bool()?;
                    let _constant = r.bool()?;
                    let vc = r.var()?;
                    read_vertices(r, vc)?;
                    r.floats(vc / 3)?;
                    if nonessential {
                        r.int()?;
                    }
                    Attachment::Other
                }
                5 => {
                    r.float()?;
                    r.float()?;
                    r.float()?;
                    if nonessential {
                        r.int()?;
                    }
                    Attachment::Other
                }
                6 => {
                    let end_slot = r.index(slots.len(), "clipping end slot")?;
                    let vc = r.var()?;
                    let vertices = read_vertices(r, vc)?;
                    if nonessential {
                        r.int()?;
                    }
                    Attachment::Clip(Clip { end_slot, vertices, vertex_count: vc })
                }
                t => return Err(format!("skel: unknown attachment type {t}")),
            };
            skins[si].slots[slot].insert(key, att);
        }
    }
    Ok(())
}

#[allow(clippy::type_complexity)]
fn resolve_linked(skins: &mut [Skin], linked: Vec<(usize, usize, String, String, String, bool, String, [f32; 4], f32, f32)>) {
    let mut pending = linked;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for l in pending {
            let (si, slot, key, parent, parent_skin, deform, path, color, w, h) = l;
            let psi = match parent_skin.strip_prefix('#') {
                Some(n) => n.parse::<usize>().unwrap_or(0).min(skins.len().saturating_sub(1)),
                None => skins.iter().position(|s| s.name == parent_skin).unwrap_or(0),
            };
            let p = skins.get(psi).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(&parent)).cloned();
            match p {
                Some(Attachment::Mesh(p)) => {
                    let (deform_skin, deform_name) = if deform { (p.deform_skin, p.deform_name.clone()) } else { (si, key.clone()) };
                    skins[si].slots[slot].insert(key, Attachment::Mesh(Mesh { path, color, width: if w > 0.0 { w } else { p.width }, height: if h > 0.0 { h } else { p.height }, deform_skin, deform_name, ..p }));
                }
                _ => rest.push((si, slot, key, parent, parent_skin, deform, path, color, w, h)),
            }
        }
        if rest.len() == before {
            break;
        }
        pending = rest;
    }
}

/// Read the curve that leaves a key, into `keys` for `frame`, `channels` channels.
fn read_curve(r: &mut R, keys: &mut Keys, frame: usize, channels: usize, first_channel: usize, legacy: bool) -> Res<()> {
    match r.byte()? {
        0 => {}
        1 => {
            for c in 0..channels {
                keys.set_raw(frame, first_channel + c, Raw::Stepped);
            }
        }
        2 => {
            if legacy {
                // 3.x: one curve for the whole key.
                let c = [r.float()?, r.float()?, r.float()?, r.float()?];
                for ch in 0..keys.stride {
                    keys.set_raw(frame, ch, Raw::Bez(c));
                }
            } else {
                for c in 0..channels {
                    let v = [r.float()?, r.float()?, r.float()?, r.float()?];
                    keys.set_raw(frame, first_channel + c, Raw::Bez(v));
                }
            }
        }
        t => return Err(format!("skel: unknown curve type {t}")),
    }
    Ok(())
}

/// A timeline of `stride` floats per key in the 4.x layout: first key, then (next key, curve of the previous) pairs.
fn read_keys4(r: &mut R, frame_count: usize, stride: usize, scale_channels: &dyn Fn(usize) -> bool, read_vals: &mut dyn FnMut(&mut R) -> Res<Vec<f32>>) -> Res<Keys> {
    let _ = scale_channels;
    let mut keys = Keys::new(stride);
    if frame_count == 0 {
        return Ok(keys);
    }
    let mut time = r.float()?;
    let mut vals = read_vals(r)?;
    for frame in 0..frame_count {
        keys.push(time, &vals);
        if frame + 1 == frame_count {
            break;
        }
        let t2 = r.float()?;
        let v2 = read_vals(r)?;
        read_curve(r, &mut keys, frame, stride, 0, false)?;
        time = t2;
        vals = v2;
    }
    Ok(keys)
}

/// The same in the 3.x layout: key, then its curve, key, curve ...
fn read_keys3(r: &mut R, frame_count: usize, stride: usize, read_vals: &mut dyn FnMut(&mut R) -> Res<Vec<f32>>) -> Res<Keys> {
    let mut keys = Keys::new(stride);
    for frame in 0..frame_count {
        let time = r.float()?;
        let vals = read_vals(r)?;
        keys.push(time, &vals);
        if frame + 1 < frame_count {
            read_curve(r, &mut keys, frame, stride, 0, true)?;
        }
    }
    Ok(keys)
}

fn byte_color(r: &mut R, n: usize) -> Res<Vec<f32>> {
    (0..n).map(|_| Ok(r.byte()? as f32 / 255.0)).collect()
}

#[allow(clippy::too_many_arguments)]
fn read_animation(r: &mut R, name: String, ver: &Ver, bones: &[BoneData], slots: &[SlotData], skins: &[Skin], ik: &[IkData], transform: &[TransformData], event_audio: &[bool]) -> Res<Animation> {
    let absolute = !ver.legacy;
    let mut anim = Animation { name, ..Default::default() };
    let mut dur = 0.0f32;
    if !ver.legacy {
        r.var()?; // the number of timelines in all (a size hint)
    }
    trace(r, &format!("animation {}", anim.name));
    // Slots.
    let n = r.count(2)?;
    for _ in 0..n {
        let slot = r.index(slots.len(), "animation slot")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            let tl = if ty == 0 {
                let mut v = Vec::with_capacity(frames);
                for _ in 0..frames {
                    v.push((r.float()?, r.sref()?));
                }
                if let Some(l) = v.last() {
                    dur = dur.max(l.0);
                }
                anim.slots.push((slot, SlotTl::Attachment(v)));
                continue;
            } else if ver.legacy {
                match ty {
                    1 => SlotTl::Color(read_keys3(r, frames, 4, &mut |r| Ok(r.rgba()?.to_vec()))?.track(absolute)),
                    2 => SlotTl::TwoColor(read_keys3(r, frames, 7, &mut |r| {
                        let mut v = r.rgba()?.to_vec();
                        v.extend(r.rgb_int()?);
                        Ok(v)
                    })?.track(absolute)),
                    t => return Err(format!("skel: unknown slot timeline {t}")),
                }
            } else {
                let _bezier = r.var()?;
                let k4 = |keys: Keys| keys.track(true);
                match ty {
                    1 => SlotTl::Color(k4(read_keys4(r, frames, 4, &|_| false, &mut |r| byte_color(r, 4))?)),
                    2 => SlotTl::Rgb(k4(read_keys4(r, frames, 3, &|_| false, &mut |r| byte_color(r, 3))?)),
                    3 => SlotTl::TwoColor(k4(read_keys4(r, frames, 7, &|_| false, &mut |r| byte_color(r, 7))?)),
                    4 => SlotTl::Rgb2(k4(read_keys4(r, frames, 6, &|_| false, &mut |r| byte_color(r, 6))?)),
                    5 => SlotTl::Alpha(k4(read_keys4(r, frames, 1, &|_| false, &mut |r| byte_color(r, 1))?)),
                    t => return Err(format!("skel: unknown slot timeline {t}")),
                }
            };
            if let SlotTl::Color(t) | SlotTl::Rgb(t) | SlotTl::TwoColor(t) | SlotTl::Rgb2(t) | SlotTl::Alpha(t) = &tl {
                dur = dur.max(t.duration());
            }
            anim.slots.push((slot, tl));
        }
    }
    trace(r, "  slots done");
    // Bones.
    let n = r.count(2)?;
    for _ in 0..n {
        let bone = r.index(bones.len(), "animation bone")?;
        let nn = r.count(2)?;
        for _ in 0..nn {
            let ty = r.byte()?;
            let frames = r.count(4)?;
            if ver.v42 && ty == 10 {
                // The inherit mode over time: not played (the setup mode stays).
                for _ in 0..frames {
                    r.float()?;
                    r.byte()?;
                }
                continue;
            }
            let tl = if ver.legacy {
                match ty {
                    0 => BoneTl::Rotate(read_keys3(r, frames, 1, &mut |r| Ok(vec![r.float()?]))?.track(absolute)),
                    1 => BoneTl::Translate(read_keys3(r, frames, 2, &mut |r| Ok(vec![r.float()?, r.float()?]))?.track(absolute)),
                    2 => BoneTl::Scale(read_keys3(r, frames, 2, &mut |r| Ok(vec![r.float()?, r.float()?]))?.track(absolute)),
                    3 => BoneTl::Shear(read_keys3(r, frames, 2, &mut |r| Ok(vec![r.float()?, r.float()?]))?.track(absolute)),
                    t => return Err(format!("skel: unknown bone timeline {t}")),
                }
            } else {
                let _bezier = r.var()?;
                let one = &mut |r: &mut R| Ok(vec![r.float()?]);
                let two = &mut |r: &mut R| Ok(vec![r.float()?, r.float()?]);
                if ver.minor41 {
                    match ty {
                        0 => BoneTl::Rotate(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        1 => BoneTl::Translate(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        2 => BoneTl::TranslateX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        3 => BoneTl::TranslateY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        4 => BoneTl::Scale(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        5 => BoneTl::ScaleX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        6 => BoneTl::ScaleY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        7 => BoneTl::Shear(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        8 => BoneTl::ShearX(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        9 => BoneTl::ShearY(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        t => return Err(format!("skel: unknown bone timeline {t}")),
                    }
                } else {
                    match ty {
                        0 => BoneTl::Rotate(read_keys4(r, frames, 1, &|_| false, one)?.track(true)),
                        1 => BoneTl::Translate(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        2 => BoneTl::Scale(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        3 => BoneTl::Shear(read_keys4(r, frames, 2, &|_| false, two)?.track(true)),
                        t => return Err(format!("skel: unknown bone timeline {t}")),
                    }
                }
            };
            let d = match &tl {
                BoneTl::Rotate(t) | BoneTl::Translate(t) | BoneTl::TranslateX(t) | BoneTl::TranslateY(t) | BoneTl::Scale(t) | BoneTl::ScaleX(t) | BoneTl::ScaleY(t) | BoneTl::Shear(t) | BoneTl::ShearX(t) | BoneTl::ShearY(t) => t.duration(),
            };
            dur = dur.max(d);
            anim.bones.push((bone, tl));
        }
    }
    trace(r, "  bones done");
    // IK constraints.
    let n = r.count(2)?;
    for _ in 0..n {
        let ci = r.index(ik.len(), "animation IK")?;
        let frames = r.count(4)?;
        let t = if ver.legacy {
            read_keys3(r, frames, 6, &mut |r| Ok(vec![r.float()?, r.float()?, r.sbyte()? as f32, r.bool()? as u8 as f32, r.bool()? as u8 as f32, 0.0]))?.track(false)
        } else if ver.v42 {
            let _bezier = r.var()?;
            let mix_of = |r: &mut R, flags: u8| -> Res<f32> {
                if flags & 1 != 0 {
                    if flags & 2 != 0 { r.float() } else { Ok(1.0) }
                } else {
                    Ok(0.0)
                }
            };
            let mut keys = Keys::new(6);
            if frames > 0 {
                let mut flags = r.byte()?;
                let mut time = r.float()?;
                let mut mix = mix_of(r, flags)?;
                let mut soft = if flags & 4 != 0 { r.float()? } else { 0.0 };
                for frame in 0..frames {
                    keys.push(time, &[mix, soft, if flags & 8 != 0 { 1.0 } else { -1.0 }, (flags & 16 != 0) as u8 as f32, (flags & 32 != 0) as u8 as f32, 0.0]);
                    if frame + 1 == frames {
                        break;
                    }
                    let nf = r.byte()?;
                    let t2 = r.float()?;
                    let m2 = mix_of(r, nf)?;
                    let s2 = if nf & 4 != 0 { r.float()? } else { 0.0 };
                    if nf & 64 != 0 {
                        for c in 0..6 {
                            keys.set_raw(frame, c, Raw::Stepped);
                        }
                    } else if nf & 128 != 0 {
                        for c in 0..2 {
                            keys.set_raw(frame, c, Raw::Bez([r.float()?, r.float()?, r.float()?, r.float()?]));
                        }
                    }
                    flags = nf;
                    time = t2;
                    mix = m2;
                    soft = s2;
                }
            }
            keys.track(true)
        } else {
            let _bezier = r.var()?;
            // time, mix, softness first; the bend direction and flags of a key come before the next key's time.
            let mut keys = Keys::new(6);
            if frames > 0 {
                let mut time = r.float()?;
                let (mut mix, mut soft) = (r.float()?, r.float()?);
                for frame in 0..frames {
                    let (bend, compress, stretch) = (r.sbyte()? as f32, r.bool()? as u8 as f32, r.bool()? as u8 as f32);
                    keys.push(time, &[mix, soft, bend, compress, stretch, 0.0]);
                    if frame + 1 == frames {
                        break;
                    }
                    let t2 = r.float()?;
                    let (m2, s2) = (r.float()?, r.float()?);
                    match r.byte()? {
                        0 => {}
                        1 => {
                            for c in 0..6 {
                                keys.set_raw(frame, c, Raw::Stepped);
                            }
                        }
                        2 => {
                            for c in 0..2 {
                                keys.set_raw(frame, c, Raw::Bez([r.float()?, r.float()?, r.float()?, r.float()?]));
                            }
                        }
                        t => return Err(format!("skel: unknown curve type {t}")),
                    }
                    time = t2;
                    mix = m2;
                    soft = s2;
                }
            }
            keys.track(true)
        };
        dur = dur.max(t.duration());
        anim.ik.push((ci, t));
    }
    trace(r, "  ik done");
    // Transform constraints.
    let n = r.count(2)?;
    for _ in 0..n {
        let ci = r.index(transform.len(), "animation transform")?;
        let frames = r.count(4)?;
        let t = if ver.legacy {
            read_keys3(r, frames, 6, &mut |r| {
                let (a, b, c, d) = (r.float()?, r.float()?, r.float()?, r.float()?);
                Ok(vec![a, b, b, c, c, d])
            })?.track(false)
        } else if ver.minor41 {
            let _bezier = r.var()?;
            read_keys4(r, frames, 6, &|_| false, &mut |r| Ok(vec![r.float()?, r.float()?, r.float()?, r.float()?, r.float()?, r.float()?]))?.track(true)
        } else {
            let _bezier = r.var()?;
            // 4.0 mixes: rotate, translate, scale, shear (four channels, curves per channel).
            let mut keys = read_keys4(r, frames, 4, &|_| false, &mut |r| Ok(vec![r.float()?, r.float()?, r.float()?, r.float()?]))?;
            // Expand to the six-channel form: rotate, x, y, scale x, scale y, shear y.
            let mut six = Keys::new(6);
            for i in 0..keys.times.len() {
                let v = &keys.vals[i * 4..i * 4 + 4];
                six.push(keys.times[i], &[v[0], v[1], v[1], v[2], v[2], v[3]]);
                let raws: Vec<Raw> = keys.raws[i * 4..i * 4 + 4].to_vec();
                for (c, rw) in [(0, raws[0]), (1, raws[1]), (2, raws[1]), (3, raws[2]), (4, raws[2]), (5, raws[3])] {
                    six.set_raw(i, c, rw);
                }
            }
            keys = six;
            keys.track(true)
        };
        dur = dur.max(t.duration());
        anim.transform.push((ci, t));
    }
    trace(r, "  transform done");
    // Path timelines: not supported (skeletons with path constraints are refused earlier).
    let n = r.count(2)?;
    if n > 0 {
        return Err("skel: path constraint timelines are not read".into());
    }
    trace(r, "  path done");
    if ver.v42 {
        // Physics constraint timelines: read to stay in step, not played.
        let n = r.count(2)?;
        for _ in 0..n {
            r.var()?; // constraint index + 1 (0 = all)
            let nn = r.count(2)?;
            for _ in 0..nn {
                let ty = r.byte()?;
                let frames = r.count(4)?;
                if ty == 8 {
                    for _ in 0..frames {
                        r.float()?;
                    }
                } else {
                    r.var()?; // bezier count
                    read_keys4(r, frames, 1, &|_| false, &mut |r| Ok(vec![r.float()?]))?;
                }
            }
        }
    }
    // Deform.
    let n = r.count(2)?;
    for _ in 0..n {
        let si = r.index(skins.len(), "deform skin")?;
        let ns = r.count(2)?;
        for _ in 0..ns {
            let slot = r.index(slots.len(), "deform slot")?;
            let na = r.count(2)?;
            for _ in 0..na {
                let att_name = r.sref()?.unwrap_or_default();
                // 4.1 and later: a type byte (0 deform, 1 image sequence) comes before the key count.
                let kind = if ver.minor41 { r.byte()? } else { 0 };
                let mesh_len = skins.get(si).and_then(|s| s.slots.get(slot)).and_then(|m| m.get(&att_name)).and_then(|a| if let Attachment::Mesh(m) = a { Some(m.vertices.deform_len()) } else { None });
                let frames = r.count(4)?;
                if kind == 1 {
                    // A sequence timeline: time, mode and index, delay per key. Not played.
                    for _ in 0..frames {
                        r.float()?;
                        r.int()?;
                        r.float()?;
                    }
                    continue;
                }
                if kind != 0 {
                    return Err(format!("skel: unknown attachment timeline type {kind}"));
                }
                if !ver.legacy {
                    let _bezier = r.var()?;
                }
                let len = mesh_len.ok_or_else(|| format!("skel: a deform timeline for {att_name} that is not a mesh"))?;
                let mut tl = DeformTl { skin: si, slot, attachment: att_name, times: Vec::new(), frames: Vec::new(), curves: Vec::new() };
                let mut time = r.float()?;
                for frame in 0..frames {
                    let end = r.var()?;
                    let mut full = vec![0.0f32; len];
                    if end > 0 {
                        let start = r.var()?;
                        if start + end > len {
                            return Err("skel: a deform key reaches past its mesh".into());
                        }
                        for v in full.iter_mut().skip(start).take(end) {
                            *v = r.float()?;
                        }
                    }
                    tl.times.push(time);
                    tl.frames.push(full);
                    tl.curves.push(Curve::Linear);
                    if frame + 1 < frames {
                        let t2 = r.float()?;
                        let curve = match r.byte()? {
                            0 => Curve::Linear,
                            1 => Curve::Stepped,
                            2 => {
                                let c = [r.float()?, r.float()?, r.float()?, r.float()?];
                                if ver.legacy {
                                    Curve::Bezier(c)
                                } else {
                                    let dt = (t2 - time).max(1e-6);
                                    Curve::Bezier([(c[0] - time) / dt, c[1], (c[2] - time) / dt, c[3]])
                                }
                            }
                            t => return Err(format!("skel: unknown curve type {t}")),
                        };
                        tl.curves[frame] = curve;
                        time = t2;
                    }
                }
                if let Some(&t) = tl.times.last() {
                    dur = dur.max(t);
                }
                anim.deform.push(tl);
            }
        }
    }
    trace(r, "  deform done");
    // Draw order.
    let n = r.count(2)?;
    for _ in 0..n {
        let t = r.float()?;
        let no = r.count(2)?;
        let mut offsets = Vec::with_capacity(no);
        for _ in 0..no {
            let slot = r.index(slots.len(), "draw order slot")?;
            // Spine writes the (possibly negative) offset as a plain varint of the 32-bit number.
            offsets.push((slot, r.var()? as u32 as i32));
        }
        dur = dur.max(t);
        anim.draw_order.push((t, offsets));
    }
    // Events (only read to stay in step).
    let n = r.count(4)?;
    for _ in 0..n {
        let t = r.float()?;
        let e = r.index(event_audio.len().max(1), "event")?;
        r.zig()?;
        r.float()?;
        if r.bool()? {
            r.string()?;
        }
        if event_audio.get(e).copied().unwrap_or(false) {
            r.float()?;
            r.float()?;
        }
        dur = dur.max(t);
    }
    anim.duration = dur;
    Ok(anim)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes what the reader expects (a mirror of it: this proves the reader is self-consistent and strict, not that it matches Esoteric's files).
    #[derive(Default)]
    struct W(Vec<u8>);
    impl W {
        fn var(&mut self, mut v: usize) {
            loop {
                let b = (v & 0x7F) as u8;
                v >>= 7;
                if v == 0 {
                    self.0.push(b);
                    break;
                }
                self.0.push(b | 0x80);
            }
        }
        fn string(&mut self, s: Option<&str>) {
            match s {
                None => self.var(0),
                Some(s) => {
                    self.var(s.len() + 1);
                    self.0.extend(s.as_bytes());
                }
            }
        }
        fn f(&mut self, v: f32) {
            self.0.extend(v.to_bits().to_be_bytes());
        }
        fn i(&mut self, v: i32) {
            self.0.extend(v.to_be_bytes());
        }
        fn b(&mut self, v: u8) {
            self.0.push(v);
        }
    }

    const ATLAS: &str = "t.png\nsize:4,4\nbox\nbounds:0,0,4,4\n";

    /// One bone "root", one slot "s" showing "box", a 40 x 40 region, and an animation "a" rotating the bone 0 -> 90 over a second.
    fn skel(version: &str, table: bool) -> Vec<u8> {
        let v4 = version.starts_with("4.");
        let mut w = W::default();
        if v4 {
            w.0.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        } else {
            w.string(Some("hash"));
        }
        w.string(Some(version));
        for v in [-20.0, -20.0, 40.0, 40.0] {
            w.f(v);
        }
        if version.starts_with("4.2") {
            w.f(100.0); // reference scale
        }
        w.b(0); // no nonessential data
        let names = ["box"];
        if table {
            w.var(names.len());
            for n in names {
                w.string(Some(n));
            }
        }
        let sref = |w: &mut W, n: Option<&str>| {
            if table {
                w.var(n.map(|_| 1).unwrap_or(0));
            } else {
                w.string(n);
            }
        };
        // bones
        w.var(1);
        w.string(Some("root"));
        for v in [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0] {
            w.f(v);
        }
        w.var(0);
        w.b(0);
        // slots
        w.var(1);
        w.string(Some("s"));
        w.var(0);
        w.i(-1); // white
        w.i(-1); // no dark colour
        sref(&mut w, Some("box"));
        w.var(0);
        w.var(0); // ik
        w.var(0); // transform
        w.var(0); // path
        if version.starts_with("4.2") {
            w.var(0); // physics
        }
        // default skin: one slot, one attachment
        w.var(1);
        w.var(0);
        w.var(1);
        sref(&mut w, Some("box"));
        if version.starts_with("4.2") {
            w.b(16 | 128); // a region with its own path and a rotation
            sref(&mut w, Some("box"));
            for v in [0.0, 0.0, 0.0, 1.0, 1.0, 40.0, 40.0] {
                w.f(v);
            }
        } else {
            sref(&mut w, None);
            w.b(0);
            sref(&mut w, Some("box"));
            if version.starts_with("4.1") {
                w.b(0); // no image sequence
            }
            for v in [0.0, 0.0, 0.0, 1.0, 1.0, 40.0, 40.0] {
                w.f(v);
            }
            w.i(-1);
        }
        w.var(0); // other skins
        w.var(0); // events
        // one animation
        w.var(1);
        w.string(Some("a"));
        if v4 {
            w.var(1); // timeline count
        }
        w.var(0); // slot timelines
        w.var(1); // bones with timelines
        w.var(0);
        w.var(1);
        w.b(0); // rotate
        w.var(2); // frames
        if v4 {
            w.var(1); // bezier count
            w.f(0.0);
            w.f(0.0);
            w.f(1.0);
            w.f(90.0);
            w.b(2); // bezier, absolute: (0.25, 0), (0.75, 90)
            for v in [0.25, 0.0, 0.75, 90.0] {
                w.f(v);
            }
        } else {
            w.f(0.0);
            w.f(0.0);
            w.b(2); // bezier, fractions
            for v in [0.25, 0.0, 0.75, 1.0] {
                w.f(v);
            }
            w.f(1.0);
            w.f(90.0);
        }
        w.var(0); // ik
        w.var(0); // transform
        w.var(0); // path
        if v4 && version.starts_with("4.2") {
            w.var(0); // physics
        }
        w.var(0); // deform
        w.var(0); // draw order
        w.var(0); // events
        w.0
    }

    fn check(rig: &Rig) {
        assert_eq!((rig.bones.len(), rig.slots.len(), rig.animations.len()), (1, 1, 1));
        assert_eq!(rig.slots[0].attachment.as_deref(), Some("box"));
        let Some((Attachment::Region(r), _)) = rig.find_attachment(0, 0, "box") else { panic!("no region") };
        assert_eq!((r.width, r.height), (40.0, 40.0));
        assert!((rig.animations[0].duration - 1.0).abs() < 1e-6);
        let a = |t: f32| rig.pose(0, Some(0), t).bones[0].rotation;
        assert!((a(0.5) - 45.0).abs() < 0.2, "an ease-in-out is half way in the middle: {}", a(0.5));
        assert!(a(0.2) < 20.0 && a(1.0) > 89.9);
    }

    #[test]
    fn all_three_binary_versions_read_and_agree() {
        for (v, table) in [("4.2.33", true), ("4.1.24", true), ("4.0.64", true), ("3.8.99", true), ("3.8.99", false)] {
            let rig = parse(&skel(v, table), Atlas::parse(ATLAS).unwrap()).unwrap_or_else(|e| panic!("{v} table={table}: {e}"));
            check(&rig);
            assert_eq!(rig.legacy, v.starts_with("3."));
        }
    }

    #[test]
    fn a_cut_off_or_padded_file_is_refused_not_misread() {
        let good = skel("4.1.24", true);
        for cut in [10, 40, good.len() - 1] {
            assert!(parse(&good[..cut], Atlas::parse(ATLAS).unwrap()).is_err(), "cut at {cut}");
        }
        let mut padded = good.clone();
        padded.extend([0, 0, 0]);
        assert!(parse(&padded, Atlas::parse(ATLAS).unwrap()).is_err());
        assert!(parse(b"not a skeleton at all", Atlas::parse(ATLAS).unwrap()).is_err());
    }

    #[test]
    fn version_4_2_and_unknown_versions_say_what_to_do() {
        let e = parse(&skel("4.3.10", true), Atlas::parse(ATLAS).unwrap()).unwrap_err();
        assert!(e.contains("JSON") && e.contains("4.3"), "{e}");
        let e = parse(&skel("3.4.02", true), Atlas::parse(ATLAS).unwrap()).unwrap_err();
        assert!(e.contains("not supported"), "{e}");
    }

    #[test]
    fn varints_and_zigzag() {
        let mut w = W::default();
        for v in [0usize, 1, 127, 128, 300, 70000] {
            w.var(v);
        }
        let mut r = R { b: &w.0, p: 0, strings: None };
        for v in [0usize, 1, 127, 128, 300, 70000] {
            assert_eq!(r.var().unwrap(), v);
        }
        let mut z = R { b: &[0, 1, 2, 3], p: 0, strings: None };
        assert_eq!((z.zig().unwrap(), z.zig().unwrap(), z.zig().unwrap(), z.zig().unwrap()), (0, -1, 1, -2));
    }
}
