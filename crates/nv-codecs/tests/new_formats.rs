//! Files made by real encoders (Pillow, ffmpeg's libjxl) in the formats added in session 8, each a picture of four blocks:
//! red top-left, green top-right, blue bottom-left, white bottom-right. Decoding them proves the orientation and the colour
//! order, which the synthetic unit tests inside the crate cannot.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

fn quadrants(name: &str) -> [[u8; 3]; 4] {
    let img = nv_codecs::decode_file(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let (w, h) = (img.width as usize, img.height as usize);
    assert!(w >= 8 && h >= 8, "{name}: {w}x{h}");
    let at = |fx: f32, fy: f32| {
        let (x, y) = (((w as f32 * fx) as usize).min(w - 1), ((h as f32 * fy) as usize).min(h - 1));
        let p = &img.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
        assert_eq!(p[3], 255, "{name}: opaque");
        [p[0], p[1], p[2]]
    };
    [at(0.25, 0.25), at(0.75, 0.25), at(0.25, 0.75), at(0.75, 0.75)]
}

fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (*x as i32 - *y as i32).abs() <= tol)
}

const WANT: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];

fn check(name: &str, tol: i32) {
    let got = quadrants(name);
    for (i, (g, w)) in got.iter().zip(WANT.iter()).enumerate() {
        assert!(close(*g, *w, tol), "{name}: quadrant {i} is {g:?}, expected {w:?}");
    }
}

#[test]
fn jpeg_2000_container_and_codestream() {
    check("t.jp2", 2);
    check("t.j2k", 2);
}

#[test]
fn jpeg_xl() {
    check("t.jxl", 12); // lossy
}

#[test]
fn pcx_24_bit_and_palette() {
    check("t.pcx", 0);
    check("p.pcx", 0);
}

#[test]
fn icons_cursors_and_bare_dibs() {
    check("t.ico", 0);
    check("t.cur", 0);
    check("t.dib", 0);
}

#[test]
fn apple_icon_with_png_payload() {
    check("t.icns", 0);
}

#[test]
fn error_codes_reach_the_caller() {
    let dir = std::env::temp_dir().join(format!("nv_codes_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.jp2");
    std::fs::write(&junk, b"this is not a picture at all").unwrap();
    let e = nv_codecs::decode_file(&junk).err().expect("junk must fail");
    assert!(nv_codecs::errors::code_of(&e).is_some(), "the message carries a code: {e}");
    let missing = nv_codecs::decode_file(&dir.join("nothing.png")).err().expect("missing must fail");
    assert_eq!(nv_codecs::errors::code_of(&missing), Some(1001), "{missing}");
    let empty = dir.join("empty.png");
    std::fs::write(&empty, b"").unwrap();
    let e = nv_codecs::decode_file(&empty).err().expect("empty must fail");
    assert_eq!(nv_codecs::errors::code_of(&e), Some(1004), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ecw_says_what_to_do_instead_of_failing_silently() {
    let dir = std::env::temp_dir().join(format!("nv_ecw_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("map.ecw");
    std::fs::write(&f, b"ECW junk").unwrap();
    assert!(nv_codecs::is_supported_ext("ecw"), "listed in folders");
    assert!(!nv_codecs::supported_extensions().contains(&"ecw"), "but not registered as a file type");
    let e = nv_codecs::decode_file(&f).err().expect("no decoder");
    assert!(e.contains("gdal_translate") && nv_codecs::errors::code_of(&e) == Some(2004), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
