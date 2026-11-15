//! Colour look-up tables in the Adobe / Resolve `.cube` format (3D and 1D), applied with trilinear interpolation.
//!
//! A LUT file is read once and kept (keyed by path and modification time), so a slider move never touches the disk.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, PartialEq)]
pub struct Lut {
    pub title: String,
    /// Entries per side (3D) or in total (1D).
    pub size: usize,
    pub is_3d: bool,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// 3D: size^3 entries, red varying fastest (the .cube order). 1D: size entries.
    pub data: Vec<[f32; 3]>,
}

fn nums(rest: &str) -> Option<[f32; 3]> {
    let mut it = rest.split_whitespace().map(|t| t.parse::<f32>());
    let v = [it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?];
    Some(v)
}

pub fn parse(text: &str) -> Result<Lut, String> {
    let mut title = String::new();
    let (mut size3, mut size1) = (0usize, 0usize);
    let (mut dmin, mut dmax) = ([0.0f32; 3], [1.0f32; 3]);
    let mut data = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (key, rest) = line.split_once(char::is_whitespace).map(|(k, r)| (k, r.trim())).unwrap_or((line, ""));
        match key {
            "TITLE" => title = rest.trim_matches('"').to_string(),
            "LUT_3D_SIZE" => size3 = rest.parse().map_err(|_| "bad LUT_3D_SIZE".to_string())?,
            "LUT_1D_SIZE" => size1 = rest.parse().map_err(|_| "bad LUT_1D_SIZE".to_string())?,
            "DOMAIN_MIN" => dmin = nums(rest).ok_or("bad DOMAIN_MIN")?,
            "DOMAIN_MAX" => dmax = nums(rest).ok_or("bad DOMAIN_MAX")?,
            "LUT_3D_INPUT_RANGE" | "LUT_1D_INPUT_RANGE" => {}
            _ => {
                if let Some(v) = nums(line) {
                    data.push(v);
                }
            }
        }
    }
    let (size, is_3d) = match (size3, size1) {
        (n, 0) if n >= 2 => (n, true),
        (0, n) if n >= 2 => (n, false),
        _ => return Err("not a .cube file (no LUT_3D_SIZE / LUT_1D_SIZE)".into()),
    };
    if size > 256 {
        return Err("LUT too large".into());
    }
    let want = if is_3d { size * size * size } else { size };
    if data.len() != want {
        return Err(format!("the LUT should have {want} entries, it has {}", data.len()));
    }
    if (0..3).any(|c| dmax[c] <= dmin[c]) {
        return Err("bad DOMAIN".into());
    }
    Ok(Lut { title, size, is_3d, domain_min: dmin, domain_max: dmax, data })
}

impl Lut {
    /// The colour `rgb` (0..1) goes to.
    pub fn lookup(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let mut t = [0.0f32; 3];
        for c in 0..3 {
            t[c] = ((rgb[c] - self.domain_min[c]) / (self.domain_max[c] - self.domain_min[c])).clamp(0.0, 1.0) * (n - 1) as f32;
        }
        if !self.is_3d {
            let mut out = [0.0f32; 3];
            for c in 0..3 {
                let i = (t[c] as usize).min(n - 2);
                let f = t[c] - i as f32;
                out[c] = self.data[i][c] * (1.0 - f) + self.data[i + 1][c] * f;
            }
            return out;
        }
        let i = [(t[0] as usize).min(n - 2), (t[1] as usize).min(n - 2), (t[2] as usize).min(n - 2)];
        let f = [t[0] - i[0] as f32, t[1] - i[1] as f32, t[2] - i[2] as f32];
        let at = |r: usize, g: usize, b: usize| self.data[r + g * n + b * n * n];
        let mut out = [0.0f32; 3];
        for (dr, wr) in [(0, 1.0 - f[0]), (1, f[0])] {
            for (dg, wg) in [(0, 1.0 - f[1]), (1, f[1])] {
                for (db, wb) in [(0, 1.0 - f[2]), (1, f[2])] {
                    let w = wr * wg * wb;
                    if w == 0.0 {
                        continue;
                    }
                    let v = at(i[0] + dr, i[1] + dg, i[2] + db);
                    for c in 0..3 {
                        out[c] += v[c] * w;
                    }
                }
            }
        }
        out
    }
}

type Cache = Mutex<HashMap<String, (std::time::SystemTime, Arc<Lut>)>>;

/// Load (or fetch from the cache) the LUT at `path`.
pub fn load(path: &str) -> Result<Arc<Lut>, String> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).map_err(|e| format!("{path}: {e}"))?;
    if let Some((t, l)) = cache.lock().unwrap().get(path) {
        if *t == mtime {
            return Ok(l.clone());
        }
    }
    let bytes = std::fs::read(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("LUT file too large".into());
    }
    let lut = Arc::new(parse(&String::from_utf8_lossy(&bytes))?);
    cache.lock().unwrap().insert(path.to_string(), (mtime, lut.clone()));
    Ok(lut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity_3d(n: usize) -> String {
        let mut s = format!("TITLE \"id\"\nLUT_3D_SIZE {n}\n");
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let f = |v: usize| v as f32 / (n - 1) as f32;
                    s.push_str(&format!("{} {} {}\n", f(r), f(g), f(b)));
                }
            }
        }
        s
    }

    #[test]
    fn an_identity_cube_changes_nothing() {
        let lut = parse(&identity_3d(5)).unwrap();
        assert_eq!(lut.title, "id");
        for c in [[0.0, 0.0, 0.0], [1.0, 0.5, 0.25], [0.33, 0.77, 0.9]] {
            let o = lut.lookup(c);
            assert!((0..3).all(|k| (o[k] - c[k]).abs() < 1e-5), "{c:?} -> {o:?}");
        }
    }

    #[test]
    fn red_runs_fastest_and_blue_green_can_be_swapped() {
        // A 2-entry cube that swaps red and blue.
        let text = "LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";
        // Entry order is r fastest: (r0 g0 b0) (r1 g0 b0) ... so the first two lines are r=0 and r=1 at g=b=0.
        let lut = parse(text).unwrap();
        let o = lut.lookup([1.0, 0.0, 0.0]);
        assert_eq!(o, [0.0, 0.0, 1.0], "red went to blue");
    }

    #[test]
    fn a_1d_lut_curves_each_channel_and_bad_files_are_refused() {
        let lut = parse("LUT_1D_SIZE 3\n0 0 0\n0.8 0.8 0.8\n1 1 1\n").unwrap();
        let o = lut.lookup([0.5, 0.25, 1.0]);
        assert!((o[0] - 0.8).abs() < 1e-5 && (o[1] - 0.4).abs() < 1e-5 && (o[2] - 1.0).abs() < 1e-5, "{o:?}");
        assert!(parse("hello").is_err());
        assert!(parse("LUT_3D_SIZE 2\n0 0 0\n").is_err());
        assert!(parse("LUT_3D_SIZE 2\nDOMAIN_MIN 1 1 1\nDOMAIN_MAX 0 0 0\n").is_err());
    }
}
