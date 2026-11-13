//! Does CMYK come out right? FOGRA39 -> sRGB checked against
//!  * the table the user got from ColorSync (C100 ~ 0,160,227; M100 ~ 229,13,128; dark rich black ~ 29,33,33), and
//!  * LittleCMS 2 (an independent engine, run through Pillow's ImageCms) for twelve patches and every rendering intent.
//!
//! The profile is the one embedded (as the output intent) in the user's "free CMYK chart" PDF. If that file is at
//! `NV_CMYK_PDF` (default: `P:\Downloads\free-cmyk-chart-printable-download-US-letter.pdf`) the test reads the profile
//! from it and also checks that it is byte for byte the profile built into the viewer; otherwise the built-in copy is used.

use nv_codecs::{ColourSettings, Image, Intent, Profile};
use std::io::Read;
use std::sync::Mutex;

/// The colour settings are process-wide: tests that change them must not overlap.
static SERIAL: Mutex<()> = Mutex::new(());

const BUILT_IN: &[u8] = include_bytes!("../profiles/CoatedFOGRA39.icc");

/// FOGRA39 -> sRGB with LittleCMS 2 (Pillow ImageCms), patches in ink percent. One row per patch: perceptual, relative,
/// relative + black point compensation, saturation, absolute.
const LCMS: [((u8, u8, u8, u8), [[u8; 3]; 5]); 12] = [
    ((0, 0, 0, 0), [[255, 255, 255], [255, 255, 255], [255, 255, 255], [255, 255, 255], [239, 241, 244]]),
    ((100, 0, 0, 0), [[0, 159, 227], [0, 160, 228], [0, 159, 227], [0, 159, 227], [0, 151, 218]]),
    ((0, 100, 0, 0), [[230, 0, 126], [230, 12, 128], [230, 0, 126], [230, 0, 126], [216, 11, 122]]),
    ((0, 0, 100, 0), [[255, 237, 0], [255, 238, 0], [255, 237, 0], [255, 237, 0], [247, 224, 0]]),
    ((0, 0, 0, 100), [[29, 29, 27], [43, 43, 42], [29, 29, 27], [29, 29, 27], [40, 40, 40]]),
    ((0, 100, 100, 0), [[227, 6, 20], [228, 31, 37], [227, 6, 19], [227, 6, 20], [215, 29, 36]]),
    ((100, 0, 100, 0), [[0, 150, 65], [0, 152, 71], [0, 150, 64], [0, 150, 65], [0, 143, 68]]),
    ((100, 100, 0, 0), [[49, 39, 131], [58, 50, 134], [49, 39, 131], [49, 39, 131], [52, 47, 128]]),
    ((100, 100, 100, 0), [[50, 50, 49], [59, 59, 58], [50, 50, 48], [50, 50, 49], [55, 55, 55]]),
    ((60, 40, 40, 100), [[3, 10, 10], [30, 33, 33], [2, 9, 9], [3, 10, 10], [27, 30, 31]]),
    ((100, 100, 100, 100), [[0, 0, 0], [28, 27, 24], [0, 0, 0], [0, 0, 0], [26, 25, 22]]),
    ((0, 0, 0, 50), [[157, 156, 156], [158, 158, 158], [157, 156, 156], [157, 156, 156], [148, 149, 151]]),
];

/// The ICC profile in the chart PDF (the output intent's stream), or None if the PDF isn't on this machine.
fn profile_from_pdf() -> Option<Vec<u8>> {
    let path = std::env::var("NV_CMYK_PDF").unwrap_or_else(|_| r"P:\Downloads\free-cmyk-chart-printable-download-US-letter.pdf".into());
    let data = std::fs::read(path).ok()?;
    let at = |needle: &[u8], from: usize| data[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from);
    // "/DestOutputProfile 14 0 R" -> object 14.
    let k = at(b"/DestOutputProfile", 0)?;
    let rest = &data[k + 18..k + 40];
    let num: String = String::from_utf8_lossy(rest).trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
    let obj = at(format!("\r{num} 0 obj").as_bytes(), 0).or_else(|| at(format!("\n{num} 0 obj").as_bytes(), 0))?;
    let stream = at(b"stream", obj)? + 6;
    let start = stream + if data[stream] == b'\r' { 2 } else { 1 };
    let end = at(b"endstream", start)?;
    let mut icc = Vec::new();
    flate2::read::ZlibDecoder::new(&data[start..end]).read_to_end(&mut icc).ok()?;
    (icc.get(36..40) == Some(b"acsp")).then_some(icc)
}

fn ink(p: u8) -> u8 {
    ((p as u32 * 255 + 50) / 100) as u8
}

/// sRGB (or output) values of CMYK patches given in percent, through `profile` (None = the built-in FOGRA39 assumed).
fn convert(patches: &[(u8, u8, u8, u8)], profile: Option<&[u8]>) -> Vec<[u8; 3]> {
    let mut rgba = Vec::new();
    for &(c, m, y, k) in patches {
        rgba.extend_from_slice(&[ink(c), ink(m), ink(y), ink(k)]);
    }
    let mut img = Image::new(patches.len() as u32, 1, rgba);
    img.cmyk = true;
    img.icc = profile.map(|p| p.to_vec());
    img.apply_icc();
    assert!(!img.cmyk, "converted");
    assert!(img.rgba.chunks_exact(4).all(|p| p[3] == 255), "opaque");
    img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect()
}

fn settings(intent: Intent, bpc: bool) -> ColourSettings {
    ColourSettings { enabled: true, output: None, untagged: None, cmyk: None, intent, bpc }
}

fn worst(a: [u8; 3], b: [u8; 3]) -> i32 {
    (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap()
}

#[test]
fn the_profile_in_the_pdf_is_the_one_built_in() {
    let Some(pdf) = profile_from_pdf() else {
        eprintln!("(chart PDF not found: skipping the comparison with the built-in profile)");
        return;
    };
    assert_eq!(pdf.len(), BUILT_IN.len());
    assert!(pdf == BUILT_IN, "the profile in the PDF differs from crates/nv-codecs/profiles/CoatedFOGRA39.icc");
    let p = Profile::cmyk_from_bytes(&pdf).expect("parses");
    assert!(p.name.contains("FOGRA39"), "{}", p.name);
}

#[test]
fn matches_the_colorsync_table() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let icc = profile_from_pdf().unwrap_or_else(|| BUILT_IN.to_vec());
    // ColorSync: relative colorimetric, no black point compensation.
    nv_codecs::configure_colour(settings(Intent::Relative, false)).unwrap();
    let got = convert(&[(100, 0, 0, 0), (0, 100, 0, 0), (60, 40, 40, 100)], Some(&icc));
    for (name, got, want) in [("C100", got[0], [0, 160, 227]), ("M100", got[1], [229, 13, 128]), ("rich black C60 M40 Y40 K100", got[2], [29, 33, 33])] {
        eprintln!("{name}: got {got:?}, ColorSync {want:?}");
        assert!(worst(got, want) <= 3, "{name}: {got:?} vs ColorSync {want:?}");
    }
}

#[test]
fn matches_littlecms_for_every_intent() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let icc = profile_from_pdf().unwrap_or_else(|| BUILT_IN.to_vec());
    let patches: Vec<_> = LCMS.iter().map(|r| r.0).collect();
    // (column in the table, intent, black point compensation, tolerance). Absolute colorimetric is looser: LittleCMS keeps
    // saturated out-of-gamut values unclipped for longer than moxcms does (yellow's red and blue differ by up to 12).
    let modes = [("perceptual", 0, Intent::Perceptual, false, 4), ("relative", 1, Intent::Relative, false, 4), ("relative + BPC", 2, Intent::Relative, true, 5), ("saturation", 3, Intent::Saturation, false, 4), ("absolute", 4, Intent::Absolute, false, 12)];
    let mut report = String::new();
    let mut failures = Vec::new();
    for (name, col, intent, bpc, tol) in modes {
        nv_codecs::configure_colour(settings(intent, bpc)).unwrap();
        let got = convert(&patches, Some(&icc));
        let mut max = 0;
        for (i, (p, rows)) in LCMS.iter().enumerate() {
            let w = worst(got[i], rows[col]);
            max = max.max(w);
            if w > tol {
                failures.push(format!("{name}: CMYK {p:?} -> {:?}, LittleCMS {:?}", got[i], rows[col]));
            }
        }
        report += &format!("{name}: worst difference {max} (tolerance {tol})\n");
    }
    nv_codecs::configure_colour(ColourSettings::rgb(true, None, None)).unwrap();
    eprintln!("{report}");
    assert!(failures.is_empty(), "{}\n{report}", failures.join("\n"));
}

#[test]
fn an_untagged_cmyk_picture_is_read_as_the_chosen_profile() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    nv_codecs::configure_colour(settings(Intent::Relative, false)).unwrap();
    // No profile in the picture: built-in FOGRA39 is assumed, and the picture says so.
    let mut rgba = vec![ink(100), 0, 0, 0];
    rgba.extend_from_slice(&[0, 0, 0, 0]);
    let mut img = Image::new(2, 1, rgba);
    img.cmyk = true;
    img.apply_icc();
    assert!(img.colour_assumed, "the viewer has to tell the user that a profile was assumed");
    assert!(img.colour_note.as_deref().unwrap_or("").contains("FOGRA39"), "{:?}", img.colour_note);
    assert!(worst([img.rgba[0], img.rgba[1], img.rgba[2]], [0, 160, 228]) <= 3);
    // The same pixels with an embedded profile are not "assumed".
    let mut img = Image::new(1, 1, vec![ink(100), 0, 0, 0]);
    img.cmyk = true;
    img.icc = Some(BUILT_IN.to_vec());
    img.apply_icc();
    assert!(!img.colour_assumed);
    assert!(img.colour_note.as_deref().unwrap_or("").contains("embedded"), "{:?}", img.colour_note);
}

#[test]
fn switching_colour_management_off_still_gives_a_picture() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    nv_codecs::configure_colour(ColourSettings { enabled: false, ..settings(Intent::Relative, false) }).unwrap();
    let got = convert(&[(0, 0, 0, 0), (0, 0, 0, 100), (100, 0, 0, 0)], None);
    assert_eq!(got[0], [255, 255, 255]);
    assert_eq!(got[1], [0, 0, 0]);
    assert_eq!(got[2], [0, 255, 255], "the naive formula, only without colour management");
    nv_codecs::configure_colour(ColourSettings::rgb(true, None, None)).unwrap();
}

#[test]
fn the_cmyk_profile_setting_changes_the_result_and_the_thumbnail_key() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(swop) = Profile::swop() else {
        eprintln!("(Windows' RSWOP.icm not found: skipping)");
        return;
    };
    nv_codecs::configure_colour(settings(Intent::Relative, false)).unwrap();
    let fogra = convert(&[(100, 0, 0, 0)], None)[0];
    let sig_fogra = nv_codecs::colour_signature();
    nv_codecs::configure_colour(ColourSettings { cmyk: Some(swop), ..settings(Intent::Relative, false) }).unwrap();
    let sw = convert(&[(100, 0, 0, 0)], None)[0];
    assert_ne!(fogra, sw, "SWOP cyan is not FOGRA39 cyan");
    assert_ne!(sig_fogra, nv_codecs::colour_signature(), "cached thumbnails must not be reused");
    nv_codecs::configure_colour(ColourSettings::rgb(true, None, None)).unwrap();
}

#[cfg(windows)]
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(name)
}

/// A CMYK TIFF (made with Pillow: paper, 100% cyan, 100% black, 50% black) must reach the colour engine as raw inks, not
/// pass through WIC's own CMYK -> RGB conversion, and come out the same as the profile says.
#[cfg(windows)]
#[test]
fn a_cmyk_tiff_is_read_raw_and_converted_through_the_profile() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    nv_codecs::configure_colour(settings(Intent::Relative, false)).unwrap();
    let path = fixture("cmyk_4x1.tif");
    let raw = nv_codecs::decode_file_first_frame(&path).expect("decodes");
    assert!(raw.cmyk, "still raw CMYK after the decoder");
    assert_eq!(&raw.rgba[..16], &[0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 128], "the inks as they are in the file");
    let img = nv_codecs::decode_file(&path).expect("decodes");
    assert!(!img.cmyk && img.colour_assumed, "a TIFF without a profile: FOGRA39 is assumed, and said so");
    for (i, want) in [[255, 255, 255], [0, 160, 228], [43, 43, 42], [158, 158, 158]].into_iter().enumerate() {
        let got = [img.rgba[i * 4], img.rgba[i * 4 + 1], img.rgba[i * 4 + 2]];
        assert!(worst(got, want) <= 3, "pixel {i}: {got:?} vs {want:?}");
    }
}

/// An RGB TIFF with an embedded profile: WIC's colour context has to reach the colour engine.
#[cfg(windows)]
#[test]
fn a_tiff_hands_over_its_embedded_profile() {
    let img = nv_codecs::decode_file_first_frame(&fixture("rgb_icc_2x1.tif")).expect("decodes");
    assert!(!img.cmyk);
    let icc = img.icc.expect("the TIFF's profile");
    assert_eq!(icc.get(36..40), Some(&b"acsp"[..]));
}
