//! A decoder plug-in end to end: a real `plugin.ini`, a real program (a .cmd) started by NuViewer, a PNG read back. Its own test
//! program because the plug-in registry is global.
#![cfg(windows)]

use nv_core::plugins;
use std::path::PathBuf;

fn write_png(path: &std::path::Path, w: u32, h: u32) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().unwrap();
    wr.write_image_data(&vec![200u8; (w * h * 4) as usize]).unwrap();
}

#[test]
fn a_plugin_adds_a_format_the_viewer_cannot_read_and_cannot_override_one_it_can() {
    let root: PathBuf = std::env::temp_dir().join(format!("nv_plugin_e2e_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plug = root.join("plugins").join("fakefmt");
    std::fs::create_dir_all(&plug).unwrap();
    write_png(&plug.join("fixture.png"), 3, 2);
    std::fs::write(plug.join("decode.cmd"), "@copy /Y \"%~dp0fixture.png\" \"%~2\" >nul\r\n").unwrap();
    std::fs::write(plug.join("fail.cmd"), "@exit /b 3\r\n").unwrap();
    std::fs::write(plug.join("slow.cmd"), "@ping -n 8 127.0.0.1 >nul\r\n").unwrap();
    std::fs::write(
        plug.join("plugin.ini"),
        "name = Fake format\napi = 1\ndecoder.extensions = zzz, jpg  # jpg is read by NuViewer itself: refused\ndecoder.exe = decode.cmd\n",
    )
    .unwrap();
    // two more plug-ins for the failure paths
    for (id, exe, ext, extra) in [("failing", "fail.cmd", "zzf", ""), ("slow", "slow.cmd", "zzs", "decoder.timeout = 1\n")] {
        let d = root.join("plugins").join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(plug.join(exe), d.join(exe)).unwrap();
        std::fs::write(d.join("plugin.ini"), format!("api = 1\ndecoder.extensions = {ext}\ndecoder.exe = {exe}\n{extra}")).unwrap();
    }
    std::env::set_var("NUVIEWER_PLUGINS", root.join("plugins"));

    assert!(!nv_codecs::is_supported_ext("zzz"), "before loading, the format is unknown");
    let loaded = plugins::load(true);
    assert_eq!(loaded.plugins.len(), 3, "{:?}", loaded.skipped);
    assert!(nv_codecs::is_supported_ext("zzz"));
    assert!(!nv_codecs::plugin_decode::handles("jpg"), "a plug-in cannot take over a format NuViewer reads itself");

    // The picture is listed and decoded through the plug-in.
    let pics = root.join("pics");
    std::fs::create_dir_all(&pics).unwrap();
    std::fs::write(pics.join("a.zzz"), b"not a real picture").unwrap();
    std::fs::write(pics.join("b.zzf"), b"x").unwrap();
    std::fs::write(pics.join("c.zzs"), b"x").unwrap();
    let list = nv_core::FolderList::open(&pics).unwrap();
    assert_eq!(list.files.len(), 3);
    let img = nv_codecs::decode_file(&pics.join("a.zzz")).unwrap();
    assert_eq!((img.width, img.height), (3, 2));
    let thumb = nv_codecs::decode_for_thumbnail(&pics.join("a.zzz")).unwrap();
    assert_eq!((thumb.width, thumb.height), (3, 2), "thumbnails go through it too");
    assert!(std::fs::read_dir(std::env::temp_dir()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(&format!("nuviewer-plugin-{}-", std::process::id()))), "scratch files are removed");

    let e = nv_codecs::decode_file(&pics.join("b.zzf")).err().unwrap();
    assert!(e.contains("could not read this file") && e.contains("exit code 3"), "{e}");
    let t = std::time::Instant::now();
    let e = nv_codecs::decode_file(&pics.join("c.zzs")).err().unwrap();
    assert!(e.contains("took longer than 1 s"), "{e}");
    assert!(t.elapsed().as_secs() < 6, "a plug-in that hangs is stopped at its limit");

    // The master switch: nothing is loaded.
    let off = plugins::load(false);
    assert!(off.plugins.is_empty());
    assert!(!nv_codecs::is_supported_ext("zzz"));
    let _ = std::fs::remove_dir_all(&root);
}
