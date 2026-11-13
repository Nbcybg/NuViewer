//! Thumbnail generation with an on-disk JPEG cache.
//!
//! Cache key = FNV-1a(path, mtime, size, version) so edited/replaced files get fresh thumbnails.
//! Files live in `%LOCALAPPDATA%\NuViewer\thumbs\<key>.jpg`.

use nv_codecs::Image;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The on-disk cache is trimmed (least recently used first) when it grows beyond this.
pub const CACHE_CAP_BYTES: u64 = 512 * 1024 * 1024;
/// After trimming, leave this much headroom so we don't trim again on the very next start.
const CACHE_TARGET_FRACTION: f64 = 0.8;

/// Longest edge of a stored thumbnail, in pixels.
pub const THUMB_MAX: u32 = 256;
/// Bump to invalidate every cached thumbnail (e.g. after changing size or filter).
const CACHE_VERSION: u64 = 1;
/// Alpha is flattened over the gallery background colour (sRGB).
const BG: [u8; 3] = [0x1A, 0x1A, 0x1A];

fn fnv1a(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

fn cache_dir() -> &'static Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("NuViewer").join("thumbs");
        std::fs::create_dir_all(&dir).ok().map(|_| dir)
    })
}

fn cache_key(path: &Path, meta: &std::fs::Metadata) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    fnv1a(&mut h, path.to_string_lossy().to_lowercase().as_bytes());
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    fnv1a(&mut h, &mtime.to_le_bytes());
    fnv1a(&mut h, &meta.len().to_le_bytes());
    fnv1a(&mut h, &CACHE_VERSION.to_le_bytes());
    fnv1a(&mut h, &THUMB_MAX.to_le_bytes());
    // Cached thumbnails are sRGB; what an untagged picture's numbers mean (a setting) changes them.
    fnv1a(&mut h, &nv_codecs::colour_signature().to_le_bytes());
    h
}

fn touch(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(SystemTime::now());
    }
}

/// Delete least-recently-used `.jpg` thumbnails in `dir` until the folder is under `cap` bytes (to 80% of it,
/// so there's headroom), and sweep stale `.tmp` leftovers. Returns (files removed, bytes freed).
pub fn prune_dir(dir: &Path, cap: u64) -> (usize, u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return (0, 0) };
    let now = SystemTime::now();
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;
    let (mut removed, mut freed) = (0usize, 0u64);
    for e in rd.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let path = e.path();
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        match path.extension().and_then(|x| x.to_str()) {
            Some("tmp") => {
                // A writer that died mid-file; anything older than an hour is garbage.
                if now.duration_since(modified).unwrap_or_default() > Duration::from_secs(3600) && std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                    freed += meta.len();
                }
            }
            Some("jpg") => {
                total += meta.len();
                files.push((modified, meta.len(), path));
            }
            _ => {}
        }
    }
    if total > cap {
        let target = (cap as f64 * CACHE_TARGET_FRACTION) as u64;
        files.sort_by_key(|f| f.0); // oldest first
        for (_, len, path) in files {
            if total <= target {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
                removed += 1;
                freed += len;
            }
        }
    }
    (removed, freed)
}

/// Trim the real thumbnail cache to `CACHE_CAP_BYTES`.
pub fn prune_cache() -> (usize, u64) {
    match cache_dir() {
        Some(dir) => prune_dir(dir, CACHE_CAP_BYTES),
        None => (0, 0),
    }
}

/// Thumbnail for `path`: from the disk cache if present, otherwise decoded, downscaled and cached.
pub fn make_thumb(path: &Path) -> Result<Image, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("stat: {e}"))?;
    let cache_file = cache_dir().as_ref().map(|d| d.join(format!("{:016x}.jpg", cache_key(path, &meta))));

    if let Some(cf) = &cache_file {
        if let Ok(bytes) = std::fs::read(cf) {
            if let Ok(mut img) = nv_codecs::decode_jpeg(&bytes) {
                touch(cf); // "recently used": keeps it away from the trimmer
                img.srgb_to_output();
                return Ok(img);
            }
        }
    }

    let full = nv_codecs::decode_file_first_frame(path)?;
    // Shrink first: colour conversion and rotation are then done on ~256 px instead of the full image.
    let small = nv_codecs::downscale(&full, THUMB_MAX).finish_for_thumb();
    drop(full);

    if let Some(cf) = &cache_file {
        // Best effort: write to a temp name then rename so readers never see a partial file.
        if let Ok(jpg) = nv_codecs::encode_thumb_jpeg(&small, BG) {
            let tmp = cf.with_extension("tmp");
            if std::fs::write(&tmp, jpg).is_ok() && std::fs::rename(&tmp, cf).is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    let mut small = small;
    small.srgb_to_output(); // the cache holds sRGB; what is shown is in the output profile
    Ok(small)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_thumbs_{tag}_{}_{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(dir: &Path, name: &str, len: usize, age_secs: u64) {
        let p = dir.join(name);
        fs::write(&p, vec![0u8; len]).unwrap();
        fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(SystemTime::now() - Duration::from_secs(age_secs)).unwrap();
    }

    #[test]
    fn under_the_cap_nothing_is_removed() {
        let d = temp_dir("under");
        write(&d, "a.jpg", 100, 10);
        write(&d, "b.jpg", 100, 5);
        assert_eq!(prune_dir(&d, 1000), (0, 0));
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn oldest_files_go_first_and_headroom_is_left() {
        let d = temp_dir("lru");
        for (i, age) in [500u64, 400, 300, 200, 100].iter().enumerate() {
            write(&d, &format!("f{i}.jpg"), 100, *age); // f0 is the oldest
        }
        // 500 bytes, cap 400 -> target 320: must drop f0, f1 (oldest) leaving 300.
        let (n, freed) = prune_dir(&d, 400);
        assert_eq!((n, freed), (2, 200));
        let mut left: Vec<String> = fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["f2.jpg", "f3.jpg", "f4.jpg"]);
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn stale_tmp_files_are_swept_but_fresh_ones_kept() {
        let d = temp_dir("tmp");
        write(&d, "old.tmp", 10, 7200);
        write(&d, "new.tmp", 10, 5);
        write(&d, "keep.txt", 10, 99999); // not ours: never touched
        let (n, _) = prune_dir(&d, 1_000_000);
        assert_eq!(n, 1);
        assert!(!d.join("old.tmp").exists() && d.join("new.tmp").exists() && d.join("keep.txt").exists());
        fs::remove_dir_all(&d).ok();
    }
}
