//! Human-readable file and EXIF details for the info panel.

use exif::{In, Tag, Value};
use std::path::Path;
use std::time::SystemTime;

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[u]) }
}

/// Civil date/time from seconds since the Unix epoch (UTC), then shifted by `utc_offset_minutes`.
pub fn format_unix(secs: i64, utc_offset_minutes: i32) -> String {
    let t = secs + utc_offset_minutes as i64 * 60;
    let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn gps_coordinate(exif: &exif::Exif, value_tag: Tag, ref_tag: Tag) -> Option<f64> {
    let f = exif.get_field(value_tag, In::PRIMARY)?;
    let Value::Rational(r) = &f.value else { return None };
    if r.len() < 3 {
        return None;
    }
    let deg = r[0].to_f64() + r[1].to_f64() / 60.0 + r[2].to_f64() / 3600.0;
    let negative = exif
        .get_field(ref_tag, In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .map(|s| s.contains('S') || s.contains('W'))
        .unwrap_or(false);
    Some(if negative { -deg } else { deg })
}

/// Label/value rows describing `path`: file facts first, then whatever EXIF it carries.
/// `utc_offset_minutes` converts the modified time to local time.
pub fn describe(path: &Path, utc_offset_minutes: i32) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    rows.push(("File".to_string(), path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    if let Some(dir) = path.parent() {
        rows.push(("Folder".to_string(), dir.display().to_string()));
    }
    if let Ok(meta) = std::fs::metadata(path) {
        rows.push(("Size".to_string(), human_size(meta.len())));
        if let Some(secs) = meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64) {
            rows.push(("Modified".to_string(), format_unix(secs, utc_offset_minutes)));
        }
    }

    let Ok(file) = std::fs::File::open(path) else { return rows };
    let Ok(exif) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)) else { return rows };
    let get = |tag: Tag| exif.get_field(tag, In::PRIMARY).map(|f| f.display_value().with_unit(&exif).to_string().trim_matches('"').trim().to_string()).filter(|s| !s.is_empty());

    let camera = [get(Tag::Make), get(Tag::Model)].into_iter().flatten().collect::<Vec<_>>();
    if !camera.is_empty() {
        // Many cameras repeat the make inside the model ("Canon" + "Canon EOS R5").
        let joined = if camera.len() == 2 && camera[1].to_lowercase().starts_with(&camera[0].to_lowercase()) { camera[1].clone() } else { camera.join(" ") };
        rows.push(("Camera".to_string(), joined));
    }
    if let Some(v) = get(Tag::LensModel) {
        rows.push(("Lens".to_string(), v));
    }
    if let Some(v) = get(Tag::DateTimeOriginal).or_else(|| get(Tag::DateTime)) {
        rows.push(("Taken".to_string(), v));
    }
    let mut shot = Vec::new();
    if let Some(v) = get(Tag::ExposureTime) {
        shot.push(format!("{v}"));
    }
    if let Some(v) = get(Tag::FNumber) {
        shot.push(v);
    }
    if let Some(v) = get(Tag::PhotographicSensitivity) {
        shot.push(format!("ISO {v}"));
    }
    if let Some(v) = get(Tag::FocalLength) {
        shot.push(v);
    }
    if !shot.is_empty() {
        rows.push(("Exposure".to_string(), shot.join("   ")));
    }
    if let Some(v) = get(Tag::ExposureBiasValue) {
        rows.push(("Exp. bias".to_string(), v));
    }
    if let Some(v) = get(Tag::Flash) {
        rows.push(("Flash".to_string(), v));
    }
    if let Some(v) = get(Tag::Software) {
        rows.push(("Software".to_string(), v));
    }
    if let (Some(lat), Some(lon)) = (gps_coordinate(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef), gps_coordinate(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef)) {
        rows.push(("GPS".to_string(), format!("{lat:.5}, {lon:.5}")));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn unix_time_formatting() {
        assert_eq!(format_unix(0, 0), "1970-01-01 00:00");
        assert_eq!(format_unix(1_000_000_000, 0), "2001-09-09 01:46");
        assert_eq!(format_unix(1_000_000_000, 7 * 60), "2001-09-09 08:46"); // Bangkok
        assert_eq!(format_unix(951_782_400, 0), "2000-02-29 00:00"); // leap day
    }

    #[test]
    fn missing_file_still_describes() {
        let rows = describe(Path::new("Z:/no/such/file.jpg"), 0);
        assert_eq!(rows[0].0, "File");
        assert!(rows.iter().all(|(k, _)| k != "Size"));
    }
}
