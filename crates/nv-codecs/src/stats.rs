//! Decode statistics: how long each kind of picture takes, and where the time goes.
//!
//! Every `decode_file` / thumbnail decode adds a row per format (count, total, min, max, megapixels, bytes) and the stages
//! inside it (read + decode, shrink to fit the GPU, colour conversion, RAW sensor decode, RAW develop ...). The viewer
//! writes the table to `%LOCALAPPDATA%\NuViewer\decode-stats.txt` when it quits; the `bench_decode` example prints it; the
//! info panel shows the stage times of the picture on screen (`Image::stages`).

use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub count: u64,
    pub total_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    /// Megapixels decoded (rows for formats only; stages leave it 0).
    pub megapixels: f64,
    pub bytes: u64,
}

impl Row {
    pub fn avg_ms(&self) -> f64 {
        if self.count == 0 { 0.0 } else { self.total_ms / self.count as f64 }
    }

    /// Megapixels per second over everything recorded (0 if unknown).
    pub fn mp_per_s(&self) -> f64 {
        if self.total_ms <= 0.0 { 0.0 } else { self.megapixels / (self.total_ms / 1000.0) }
    }
}

fn table() -> &'static Mutex<BTreeMap<String, Row>> {
    static T: std::sync::OnceLock<Mutex<BTreeMap<String, Row>>> = std::sync::OnceLock::new();
    T.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn add(key: String, ms: f64, megapixels: f64, bytes: u64) {
    let Ok(mut t) = table().lock() else { return };
    let r = t.entry(key).or_default();
    if r.count == 0 {
        r.min_ms = ms;
        r.max_ms = ms;
    }
    r.count += 1;
    r.total_ms += ms;
    r.min_ms = r.min_ms.min(ms);
    r.max_ms = r.max_ms.max(ms);
    r.megapixels += megapixels;
    r.bytes += bytes;
}

/// One finished decode of a `.ext` picture (`kind` = "view" or "thumb").
pub fn record_decode(kind: &str, ext: &str, ms: f64, width: u32, height: u32, bytes: u64) {
    add(format!("{kind} .{}", if ext.is_empty() { "?" } else { ext }), ms, width as f64 * height as f64 / 1e6, bytes);
}

/// A stage inside decoding (not a whole picture).
pub fn record_stage(name: &str, ms: f64) {
    add(format!("stage: {name}"), ms, 0.0, 0);
}

pub fn snapshot() -> Vec<(String, Row)> {
    table().lock().map(|t| t.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut t) = table().lock() {
        t.clear();
    }
}

/// The table as text.
pub fn report() -> String {
    let rows = snapshot();
    if rows.is_empty() {
        return "no pictures decoded yet\n".into();
    }
    let mut out = format!("{:<34} {:>6} {:>10} {:>9} {:>9} {:>10} {:>9}\n", "what", "count", "avg ms", "min ms", "max ms", "MP/s", "MB read");
    for (name, r) in rows {
        let mps = if r.megapixels > 0.0 { format!("{:.1}", r.mp_per_s()) } else { "-".into() };
        let mb = if r.bytes > 0 { format!("{:.1}", r.bytes as f64 / 1e6) } else { "-".into() };
        out += &format!("{:<34} {:>6} {:>10.1} {:>9.1} {:>9.1} {:>10} {:>9}\n", name, r.count, r.avg_ms(), r.min_ms, r.max_ms, mps, mb);
    }
    out
}

/// Where the viewer keeps the table.
pub fn default_path() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("NuViewer").join("decode-stats.txt")
}

/// Write `report()` to `path` (best effort, for the viewer to call on exit).
pub fn save(path: &std::path::Path) {
    if snapshot().is_empty() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, report());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_aggregate() {
        record_decode("test", "xyz", 10.0, 1000, 1000, 2_000_000);
        record_decode("test", "xyz", 30.0, 1000, 1000, 2_000_000);
        record_stage("test stage", 5.0);
        let snap: std::collections::HashMap<_, _> = snapshot().into_iter().collect();
        let r = &snap["test .xyz"];
        assert_eq!((r.count, r.min_ms, r.max_ms), (2, 10.0, 30.0));
        assert!((r.avg_ms() - 20.0).abs() < 1e-9);
        assert!((r.mp_per_s() - 50.0).abs() < 1e-6, "2 MP in 40 ms");
        assert_eq!(snap["stage: test stage"].count, 1);
        assert!(report().contains("test .xyz"));
    }
}
