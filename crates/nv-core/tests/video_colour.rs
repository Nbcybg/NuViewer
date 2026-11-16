//! Snapshots with real clips made by ffmpeg: the matrix and range of the clip are honoured, HDR10 is tone-mapped or kept with its
//! tag. Skipped when ffmpeg isn't installed.

use nv_core::prefs::Prefs;
use nv_core::snapshot;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ffmpeg() -> Option<PathBuf> {
    nv_core::ffmpeg::find().filter(|p| p.is_file())
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nv_vc_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make(ff: &Path, out: &Path, source: &str, vf: &str, extra: &[&str]) {
    let st = Command::new(ff)
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", source, "-vf", vf, "-c:v", "libx264", "-frames:v", "3"])
        .args(extra)
        .arg(out)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success(), "ffmpeg failed to make {}", out.display());
}

fn read_png(path: &Path) -> (u32, u32, png::BitDepth, Vec<u8>, Vec<u8>) {
    let bytes = std::fs::read(path).unwrap();
    let mut dec = png::Decoder::new(std::io::Cursor::new(&bytes));
    dec.set_transformations(png::Transformations::IDENTITY);
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    (info.width, info.height, info.bit_depth, buf[..info.buffer_size()].to_vec(), bytes)
}

fn near(a: u8, b: u8, tol: i32) -> bool {
    (a as i32 - b as i32).abs() <= tol
}

#[test]
fn the_matrix_of_the_clip_is_honoured_for_709_and_601() {
    let Some(ff) = ffmpeg() else { return eprintln!("ffmpeg not installed: skipped") };
    let dir = temp("sdr");
    // A strongly coloured picture (not grey), so a wrong matrix shows.
    for (name, matrix, tag) in [("m709", "bt709", "bt709"), ("m601", "bt601", "smpte170m")] {
        let clip = dir.join(format!("{name}.mp4"));
        make(&ff, &clip, "color=c=0xC86432:s=64x48:r=10:d=1", &format!("scale=out_color_matrix={matrix}:out_range=tv,format=yuv420p"), &["-pix_fmt", "yuv420p", "-colorspace", tag, "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"]);
        let mut prefs = Prefs::default();
        prefs.snapshot_dir = dir.display().to_string();
        prefs.snapshot_name = name.into();
        let saved = snapshot::take(&clip, 0, 0, &prefs, None).unwrap();
        let (w, h, depth, px, _) = read_png(&saved.path);
        assert_eq!((w, h, depth), (64, 48, png::BitDepth::Eight));
        let mid = ((24 * 64 + 32) * 3) as usize;
        let (r, g, b) = (px[mid], px[mid + 1], px[mid + 2]);
        assert!(near(r, 0xC8, 8) && near(g, 0x64, 8) && near(b, 0x32, 8), "{name}: got {r},{g},{b} instead of 200,100,50");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hdr10_is_tone_mapped_into_srgb_or_kept_as_16_bit_with_its_tag() {
    let Some(ff) = ffmpeg() else { return eprintln!("ffmpeg not installed: skipped") };
    let dir = temp("hdr");
    let clip = dir.join("hdr.mp4");
    // 10-bit mid grey tagged BT.2020 + PQ (the numbers are what matter, not how the clip was graded).
    make(&ff, &clip, "color=c=gray:s=64x48:r=10:d=1", "format=yuv420p10le", &["-pix_fmt", "yuv420p10le", "-x264-params", "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:range=tv"]);
    let mut prefs = Prefs::default();
    prefs.snapshot_dir = dir.display().to_string();
    prefs.snapshot_name = "tone".into();
    let tone = snapshot::take(&clip, 0, 0, &prefs, None).unwrap();
    assert!(tone.note.contains("tone-mapped"), "{}", tone.note);
    let (_, _, depth, px, _) = read_png(&tone.path);
    assert_eq!(depth, png::BitDepth::Eight);
    let mid = ((24 * 64 + 32) * 3) as usize;
    let (r, g, b) = (px[mid] as i32, px[mid + 1] as i32, px[mid + 2] as i32);
    assert!((r - g).abs() <= 3 && (g - b).abs() <= 3, "grey stays grey: {r},{g},{b}");
    assert!((60..230).contains(&r), "mid PQ grey lands in the SDR range: {r}");

    prefs.snapshot_keep_hdr = true;
    prefs.snapshot_name = "keep".into();
    let keep = snapshot::take(&clip, 0, 0, &prefs, None).unwrap();
    let (_, _, depth16, px16, bytes) = read_png(&keep.path);
    assert_eq!(depth16, png::BitDepth::Sixteen);
    assert!(bytes.windows(4).any(|w| w == b"cICP"), "tagged BT.2020 / PQ");
    let (r16, g16) = (u16::from_be_bytes([px16[mid * 2], px16[mid * 2 + 1]]), u16::from_be_bytes([px16[mid * 2 + 2], px16[mid * 2 + 3]]));
    assert!((r16 as i32 - g16 as i32).abs() < 700, "still grey in 16 bits: {r16} {g16}");
    let _ = std::fs::remove_dir_all(&dir);
}
