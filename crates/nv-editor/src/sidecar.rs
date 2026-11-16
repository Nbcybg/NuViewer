//! The pair file: a recipe saved next to the picture without touching the picture ("not burned in").
//!
//! `IMG_0001.jpg` -> `IMG_0001.jpg.nuedit` (the full picture name plus `.nuedit`, so `a.jpg` and `a.png` don't clash, RAW + JPG
//! pairs stay separate, and darktable's / Lightroom's `.xmp` next to the same picture is left alone). Plain text, one line per
//! group, `name key="value" ...`, so it is quick to read, easy to diff and can be edited by hand:
//!
//! ```text
//! nuedit version="1" source="IMG_0001.jpg" width="6240" height="4160" size="16384000" mtime="1731600000"
//! light exposure="0.5" contrast="12"
//! curve channel="master" points="0,0 0.25,0.2 1,1"
//! retouch kind="clone" radius="0.01" source="0.3,0.4" points="0.5,0.5 0.51,0.5"
//! ```
//!
//! Lines starting with `#` are comments. Unknown lines and keys are ignored (so a newer file still opens), missing ones keep
//! their default.

use crate::recipe::*;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

/// Which picture the recipe was made for. When the picture changes afterwards (another size or time), the editor warns
/// instead of applying the recipe blindly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub size: u64,
    pub mtime: i64,
}

impl Source {
    /// Read the file's size and time (width / height come from the caller, who has decoded it).
    pub fn of(path: &Path, width: u32, height: u32) -> Source {
        let meta = std::fs::metadata(path).ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Source { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), width, height, size: meta.map(|m| m.len()).unwrap_or(0), mtime }
    }

    /// Is `now` the same picture this was written for? The pixel size must match; size / time on disk may differ when the
    /// picture was merely copied (a copy has a new time), so they only matter if the size also differs.
    pub fn matches(&self, now: &Source) -> bool {
        (self.width == 0 || self.width == now.width) && (self.height == 0 || self.height == now.height)
    }
}

/// `dir/IMG.jpg` -> `dir/IMG.jpg.nuedit`.
pub fn sidecar_path(picture: &Path) -> PathBuf {
    let mut name = picture.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".nuedit");
    picture.with_file_name(name)
}

pub fn has_sidecar(picture: &Path) -> bool {
    sidecar_path(picture).is_file()
}

// ---------------------------------------------------------------------------------------------------- writing

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The shortest text that reads back as the same f32.
fn num(v: f32) -> String {
    format!("{v}")
}

fn pairs(points: &[(f32, f32)]) -> String {
    points.iter().map(|&(x, y)| format!("{},{}", num(x), num(y))).collect::<Vec<_>>().join(" ")
}

pub fn to_text(r: &Recipe, src: &Source) -> String {
    let mut out = String::with_capacity(1024);
    out.push_str(&format!(
        "nuedit version=\"{VERSION}\" source=\"{}\" width=\"{}\" height=\"{}\" size=\"{}\" mtime=\"{}\" auto=\"{}\"\n",
        esc(&src.name),
        src.width,
        src.height,
        src.size,
        src.mtime,
        r.auto.key()
    ));
    for g in Group::ALL {
        let changed: Vec<usize> = params_of(g).filter(|&i| !r.is_default(i)).collect();
        if changed.is_empty() {
            continue;
        }
        out.push_str(g.key());
        for i in changed {
            out.push_str(&format!(" {}=\"{}\"", PARAMS[i].key, num(r.v[i])));
        }
        out.push('\n');
    }
    if r.invert || r.quarter_turns % 4 != 0 || r.flip_h || r.flip_v {
        out.push_str("flags");
        if r.invert {
            out.push_str(" invert=\"1\"");
        }
        if r.quarter_turns % 4 != 0 {
            out.push_str(&format!(" quarter_turns=\"{}\"", r.quarter_turns % 4));
        }
        if r.flip_h {
            out.push_str(" flip_h=\"1\"");
        }
        if r.flip_v {
            out.push_str(" flip_v=\"1\"");
        }
        out.push('\n');
    }
    if let Some(c) = r.wb_chart {
        out.push_str(&format!("wb_chart rgb=\"{},{},{}\"\n", num(c[0]), num(c[1]), num(c[2])));
    }
    for (name, c) in [("master", &r.curves.master), ("r", &r.curves.r), ("g", &r.curves.g), ("b", &r.curves.b)] {
        if !c.points.is_empty() {
            out.push_str(&format!("curve channel=\"{name}\" points=\"{}\"\n", pairs(&c.points)));
        }
    }
    for (i, b) in r.hue_wheel.iter().enumerate() {
        if b.hue != 0.0 || b.sat != 0.0 || b.lum != 0.0 {
            out.push_str(&format!("band name=\"{}\" hue=\"{}\" sat=\"{}\" lum=\"{}\"\n", BAND_NAMES[i], num(b.hue), num(b.sat), num(b.lum)));
        }
    }
    if let Some(c) = r.crop {
        out.push_str(&format!("crop x=\"{}\" y=\"{}\" w=\"{}\" h=\"{}\"\n", num(c.x), num(c.y), num(c.w), num(c.h)));
    }
    for s in &r.retouch {
        out.push_str(&format!("retouch kind=\"{}\" radius=\"{}\"", s.kind.key(), num(s.radius)));
        if let Some((x, y)) = s.source {
            out.push_str(&format!(" source=\"{},{}\"", num(x), num(y)));
        }
        out.push_str(&format!(" points=\"{}\"\n", pairs(&s.points)));
    }
    if let Some(l) = &r.lut {
        out.push_str(&format!("lut file=\"{}\"\n", esc(l)));
    }
    out
}

// ---------------------------------------------------------------------------------------------------- reading

struct Element {
    name: String,
    attrs: Vec<(String, String)>,
}

impl Element {
    fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
}

/// One element per line: a name, then `key="value"` pairs. Blank lines and `#` comments are skipped.
fn elements(text: &str) -> Vec<Element> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let b = line.as_bytes();
        let mut j = 0;
        while j < b.len() && !b[j].is_ascii_whitespace() {
            j += 1;
        }
        let name = line[..j].to_string();
        let mut attrs = Vec::new();
        loop {
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            let k0 = j;
            while j < b.len() && b[j] != b'=' && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            let key = line[k0..j].to_string();
            if j < b.len() && b[j] == b'=' {
                j += 1;
                if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                    let q = b[j];
                    let v0 = j + 1;
                    j = v0;
                    while j < b.len() && b[j] != q {
                        j += 1;
                    }
                    attrs.push((key, unesc(&line[v0..j.min(b.len())])));
                    j += 1;
                } else {
                    // An unquoted value runs to the next space.
                    let v0 = j;
                    while j < b.len() && !b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    attrs.push((key, line[v0..j].to_string()));
                }
            }
        }
        out.push(Element { name, attrs });
    }
    out
}

fn parse_f(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_pair(s: &str) -> Option<(f32, f32)> {
    let (a, b) = s.split_once(',')?;
    Some((parse_f(a)?, parse_f(b)?))
}

fn parse_pairs(s: &str) -> Vec<(f32, f32)> {
    s.split_whitespace().filter_map(parse_pair).collect()
}

fn flag(e: &Element, k: &str) -> bool {
    matches!(e.get(k), Some("1") | Some("true"))
}

/// Parse a pair file. Err only when the text isn't a NuEditor file at all.
pub fn from_text(text: &str) -> Result<(Recipe, Source), String> {
    let els = elements(text);
    let root = els.iter().find(|e| e.name == "nuedit").ok_or("not a NuEditor file (no \"nuedit\" line)")?;
    let version: u32 = root.get("version").and_then(|v| v.parse().ok()).unwrap_or(1);
    if version > VERSION {
        return Err(format!("written by a newer NuEditor (file format {version}, this one reads up to {VERSION})"));
    }
    let src = Source {
        name: root.get("source").unwrap_or("").to_string(),
        width: root.get("width").and_then(|v| v.parse().ok()).unwrap_or(0),
        height: root.get("height").and_then(|v| v.parse().ok()).unwrap_or(0),
        size: root.get("size").and_then(|v| v.parse().ok()).unwrap_or(0),
        mtime: root.get("mtime").and_then(|v| v.parse().ok()).unwrap_or(0),
    };
    let mut r = Recipe { auto: AutoMode::from_key(root.get("auto").unwrap_or("off")), ..Recipe::default() };
    for e in els.iter().filter(|e| e.name != "nuedit") {
        if let Some(g) = Group::ALL.iter().find(|g| g.key() == e.name) {
            for (k, v) in &e.attrs {
                if let (Some(i), Some(x)) = (param_by_key(k), parse_f(v)) {
                    if PARAMS[i].group == *g {
                        r.set_index(i, x);
                    }
                }
            }
            continue;
        }
        match e.name.as_str() {
            "flags" => {
                r.invert = flag(e, "invert");
                r.quarter_turns = e.get("quarter_turns").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0) % 4;
                r.flip_h = flag(e, "flip_h");
                r.flip_v = flag(e, "flip_v");
            }
            "wb_chart" => {
                let v: Vec<f32> = e.get("rgb").unwrap_or("").split(',').filter_map(parse_f).collect();
                if v.len() == 3 {
                    r.wb_chart = Some([v[0], v[1], v[2]]);
                }
            }
            "curve" => {
                let pts: Vec<(f32, f32)> = parse_pairs(e.get("points").unwrap_or("")).into_iter().map(|(x, y)| (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))).collect();
                let mut c = Curve { points: pts };
                c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
                match e.get("channel").unwrap_or("master") {
                    "r" => r.curves.r = c,
                    "g" => r.curves.g = c,
                    "b" => r.curves.b = c,
                    _ => r.curves.master = c,
                }
            }
            "band" => {
                if let Some(i) = BAND_NAMES.iter().position(|n| Some(*n) == e.get("name")) {
                    let f = |k: &str| e.get(k).and_then(parse_f).unwrap_or(0.0);
                    r.hue_wheel[i] = Band { hue: f("hue"), sat: f("sat"), lum: f("lum") };
                }
            }
            "crop" => {
                let f = |k: &str| e.get(k).and_then(parse_f);
                if let (Some(x), Some(y), Some(w), Some(h)) = (f("x"), f("y"), f("w"), f("h")) {
                    if w > 0.0 && h > 0.0 {
                        r.crop = Some(Crop { x: x.clamp(0.0, 1.0), y: y.clamp(0.0, 1.0), w: w.min(1.0), h: h.min(1.0) });
                    }
                }
            }
            "retouch" => {
                if let Some(kind) = e.get("kind").and_then(RetouchKind::from_key) {
                    r.retouch.push(Retouch {
                        kind,
                        radius: e.get("radius").and_then(parse_f).unwrap_or(0.01),
                        source: e.get("source").and_then(parse_pair),
                        points: parse_pairs(e.get("points").unwrap_or("")),
                    });
                }
            }
            "lut" => r.lut = e.get("file").map(str::to_string).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    Ok((r, src))
}

/// Write the pair file next to `picture` (atomically). An identity recipe removes the file instead, so "revert everything"
/// leaves no stale file behind.
pub fn save(picture: &Path, r: &Recipe, src: &Source) -> std::io::Result<()> {
    let path = sidecar_path(picture);
    if r.is_identity() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    crate::save::write_atomic(&path, to_text(r, src).as_bytes())
}

/// The recipe saved for `picture`, if there is a pair file. `Ok(None)` = no file; `Err` = there is one but it can't be read.
pub fn load(picture: &Path) -> Result<Option<(Recipe, Source)>, String> {
    let path = sidecar_path(picture);
    match std::fs::read_to_string(&path) {
        Ok(text) => from_text(&text).map(Some).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy() -> Recipe {
        let mut r = Recipe::default();
        r.set(P::Exposure, 0.5);
        r.set(P::Contrast, -12.5);
        r.set(P::Temp, 33.0);
        r.set(P::Vignette, -40.0);
        r.set(P::Gamma, 1.25);
        r.invert = true;
        r.quarter_turns = 3;
        r.flip_h = true;
        r.wb_chart = Some([0.4, 0.5, 0.6]);
        r.curves.master.set_point(0.25, 0.2);
        r.curves.master.set_point(0.75, 0.8);
        r.curves.b.set_point(0.5, 0.6);
        r.hue_wheel[3] = Band { hue: 10.0, sat: -20.0, lum: 5.0 };
        r.crop = Some(Crop { x: 0.1, y: 0.2, w: 0.5, h: 0.6 });
        r.retouch.push(Retouch { kind: RetouchKind::Clone, radius: 0.02, source: Some((0.3, 0.4)), points: vec![(0.5, 0.5), (0.51, 0.5)] });
        r.retouch.push(Retouch { kind: RetouchKind::RedEye, radius: 0.005, source: None, points: vec![(0.2, 0.2)] });
        r.lut = Some("C:\\luts\\a & b \"film\".cube".to_string());
        r.auto = AutoMode::Tone;
        r
    }

    #[test]
    fn everything_survives_a_round_trip() {
        let src = Source { name: "IMG <1>.jpg".into(), width: 6240, height: 4160, size: 16_000_000, mtime: 1_731_600_000 };
        let text = to_text(&busy(), &src);
        let (back, s2) = from_text(&text).unwrap();
        assert_eq!(back, busy());
        assert_eq!(s2, src);
    }

    #[test]
    fn only_changed_sliders_are_written() {
        let mut r = Recipe::default();
        r.set(P::Exposure, 1.0);
        let text = to_text(&r, &Source::default());
        assert!(text.contains("\nlight exposure=\"1\"\n"), "{text}");
        assert!(!text.contains("contrast") && !text.contains("color ") && !text.contains("curve"));
    }

    #[test]
    fn tolerant_reader() {
        let text = "# made by hand\n\nnuedit version='1' extra='x'\nlight  exposure=0.5 bogus=\"1\"\nfuture thing=\"1\"\nwb exposure=\"3\" temp=\"10\"\n";
        let (r, _) = from_text(text).unwrap();
        assert_eq!(r.get(P::Exposure), 0.5);
        assert_eq!(r.get(P::Temp), 10.0, "keys of another group are ignored, own keys read");
        assert!(from_text("<html></html>").is_err());
        assert!(from_text("nuedit version=\"99\"\n").is_err());
        // Out-of-range values are clamped, garbage is ignored.
        let (r, _) = from_text("nuedit\nlight exposure=\"99\" contrast=\"abc\"\n").unwrap();
        assert_eq!(r.get(P::Exposure), 99.0, "a value typed beyond the slider is kept in the pair file");
        assert_eq!(r.get(P::Contrast), 0.0);
        // Windows line ends are fine.
        assert!(from_text("nuedit\r\nlight exposure=\"1\"\r\n").is_ok());
    }

    #[test]
    fn path_keeps_the_full_name() {
        assert_eq!(sidecar_path(Path::new("D:\\p\\a.jpg")), PathBuf::from("D:\\p\\a.jpg.nuedit"));
        assert_ne!(sidecar_path(Path::new("a.jpg")), sidecar_path(Path::new("a.raf")));
    }

    #[test]
    fn save_load_and_identity_removes_the_file() {
        let dir = std::env::temp_dir().join(format!("nuedit-sidecar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pic = dir.join("a.jpg");
        std::fs::write(&pic, b"x").unwrap();
        assert_eq!(load(&pic).unwrap(), None);
        let src = Source::of(&pic, 10, 10);
        save(&pic, &busy(), &src).unwrap();
        assert!(has_sidecar(&pic));
        let (r, s) = load(&pic).unwrap().unwrap();
        assert_eq!(r, busy());
        assert!(s.matches(&Source::of(&pic, 10, 10)) && !s.matches(&Source::of(&pic, 11, 10)));
        save(&pic, &Recipe::default(), &src).unwrap();
        assert!(!has_sidecar(&pic), "an unchanged recipe leaves no file");
        std::fs::write(sidecar_path(&pic), "junk").unwrap();
        assert!(load(&pic).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
