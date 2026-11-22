//! 2D animation programs' own files. None of them is a picture format, so each shows the best picture the file (or the folder
//! around it) holds, and otherwise a card with what can be read from it:
//!  * Spine (`atlas` `skel` `spine`): the atlas page (the sprite sheet the skeleton is drawn from) of the `.atlas` / the `.skel`'s
//!    sibling atlas; a `.spine` project is a card (or the biggest picture inside, when it is a zip),
//!  * Toon Boom Harmony / Storyboard Pro (`xstage` `sbpz`): the scene's facts from the XML (size, frame rate, drawing and peg nodes),
//!    or the thumbnail / biggest picture in a `sbpz` package,
//!  * Moho / Anime Studio (`moho` `anme`): the preview or biggest picture of the zip kind, the project's size and frame rate of the
//!    text kind, else a card,
//!  * Adobe Animate (`xfl`): the uncompressed project: `DOMDocument.xml` beside the stub (size, frame rate, layers) and the biggest
//!    picture of its `LIBRARY` folder (`fla`, the zipped kind, is read by the Flash module),
//!  * Pencil2D (`pclx`): the first frame, composited from the bitmap layers in the package,
//!  * Synfig Studio (`sif` `sifz` `sfg`): a card with the canvas (size, frame rate, length, layers),
//!  * OpenToonz (`tnz`): a card with the scene (resolution, frame rate, range, levels),
//!  * TVPaint (`tvpp`): a card (a closed format).
//!
//! What was checked against real files and what only against files made to the documented shape is in `docs/formats-0.6.md`.

use crate::docthumb::{accent_for, ext_of, fmt_bytes, is_image_name, render_card, Card, Zip};
use crate::Image;
use std::path::{Path, PathBuf};

pub const EXTS: &[&str] = &["atlas", "skel", "spine", "xstage", "sbpz", "moho", "anme", "xfl", "pclx", "sif", "sifz", "sfg", "tnz", "tvpp"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    if !is_ext(ext) {
        return None;
    }
    Some(match ext {
        "atlas" => atlas(path, first_only),
        "skel" => skel(path, first_only),
        "spine" => spine_project(path, first_only),
        "xstage" => xstage(path, first_only),
        "sbpz" => package(path, "sbpz", "Toon Boom Storyboard Pro package", first_only),
        "moho" | "anme" => moho(path, ext, first_only),
        "xfl" => xfl(path, first_only),
        "pclx" => pencil2d(path, first_only),
        "sif" | "sifz" | "sfg" => synfig(path, ext, first_only),
        "tnz" => tnz(path, first_only),
        _ => Ok(card(path, ext, "TVPaint Animation project", vec!["a closed format: open it in TVPaint".into()], first_only, "a card: TVPaint's format is closed")),
    })
}

// ---- helpers ---------------------------------------------------------------------------------------------------------

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn card(path: &Path, ext: &str, kind: &str, lines: Vec<String>, first_only: bool, note: &str) -> Image {
    let mut all = vec![kind.to_string()];
    all.extend(lines);
    let all: Vec<String> = all.into_iter().map(|l| crate::fontview::clip(&l, 46)).collect();
    let mut img = render_card(&Card { badge: ext.to_ascii_uppercase(), accent: accent_for(ext), title: name_of(path), subtitle: fmt_bytes(size_of(path)), lines: all }, if first_only { 1 } else { 2 });
    img.colour_note = Some(note.to_string());
    img
}

/// The text of a file, at most `limit` bytes (a project file can be many megabytes; its facts are near the top).
fn read_text(path: &Path, limit: u64) -> Result<String, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut buf = Vec::new();
    f.take(limit).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    // UTF-16 with a byte order mark (some Windows tools write that).
    if buf.starts_with(&[0xFF, 0xFE]) || buf.starts_with(&[0xFE, 0xFF]) {
        let le = buf[0] == 0xFF;
        let units: Vec<u16> = buf[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    Ok(String::from_utf8_lossy(buf.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&buf)).into_owned())
}

fn is_zip(path: &Path) -> bool {
    use std::io::Read;
    let mut h = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut h)).is_ok() && &h[..2] == b"PK"
}

/// The biggest picture in a zip, named for the note. `prefer`: names (lower case, as in the zip) tried first, in order, by suffix.
fn zip_picture(zip: &mut Zip, prefer: &[&str], limit: u64, what: &str) -> Option<Image> {
    let entries = zip.entries.clone();
    let mut order: Vec<_> = Vec::new();
    for p in prefer {
        if let Some(e) = entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(p)) {
            order.push(e.clone());
        }
    }
    let mut pics: Vec<_> = entries.iter().filter(|e| is_image_name(&e.name)).cloned().collect();
    pics.sort_by(|a, b| b.size.cmp(&a.size));
    order.extend(pics.into_iter().take(3));
    for e in order {
        if let Ok(bytes) = zip.read(&e, limit) {
            if let Ok(mut img) = crate::decode_image_bytes(&bytes, &ext_of(&e.name)) {
                img.profile_assumable = true;
                img.colour_note = Some(format!("{what}: {}", e.name));
                return Some(img);
            }
        }
    }
    None
}

fn zip_limit(first_only: bool) -> u64 {
    if first_only { 64 << 20 } else { 256 << 20 }
}

/// A zip package whose best picture is shown, or a card.
fn package(path: &Path, ext: &str, kind: &str, first_only: bool) -> Result<Image, String> {
    let mut zip = Zip::open(path)?;
    if let Some(img) = zip_picture(&mut zip, &["thumbnail.png", "preview.png", "thumbnail.jpg", "preview.jpg"], zip_limit(first_only), &format!("{kind} picture")) {
        return Ok(img);
    }
    let n = zip.entries.len();
    Ok(card(path, ext, kind, vec![format!("{n} files inside, no picture")], first_only, "a card: no picture in this package"))
}

// ---- XML ---------------------------------------------------------------------------------------------------------------

/// The attribute `name` (case-insensitive) of the first element called `tag` (case-insensitive, ignoring a namespace prefix).
fn attr<'a>(doc: &'a roxmltree::Document, tag: &str, name: &str) -> Option<&'a str> {
    doc.descendants().filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case(tag)).find_map(|n| n.attributes().find(|a| a.name().eq_ignore_ascii_case(name)).map(|a| a.value()))
}

fn count_tag(doc: &roxmltree::Document, tag: &str) -> usize {
    doc.descendants().filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case(tag)).count()
}

fn text_of(doc: &roxmltree::Document, tag: &str) -> Option<String> {
    doc.descendants().find(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case(tag)).and_then(|n| n.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

fn parse_xml(text: &str) -> Option<roxmltree::Document<'_>> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    roxmltree::Document::parse_with_options(text, opts).ok()
}

// ---- Spine -----------------------------------------------------------------------------------------------------------------

/// What a Spine / libGDX texture atlas says: its pages (image file, size) and how many regions it holds.
#[derive(Debug, PartialEq)]
pub(crate) struct AtlasInfo {
    pub pages: Vec<(String, Option<(u32, u32)>)>,
    pub regions: usize,
}

/// Read an atlas text. A page starts after a blank line (or at the top) with the image's file name; its attributes (`size:`...)
/// follow, then one unindented line per region, each with indented attributes below it.
pub(crate) fn parse_atlas(text: &str) -> AtlasInfo {
    let mut pages: Vec<(String, Option<(u32, u32)>)> = Vec::new();
    let mut regions = 0;
    let mut expect_page = true;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            expect_page = true;
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let t = line.trim();
        if expect_page && !indented && !t.contains(':') {
            pages.push((t.to_string(), None));
            expect_page = false;
            continue;
        }
        expect_page = false;
        if let Some(rest) = t.strip_prefix("size:") {
            let n: Vec<u32> = rest.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if let (Some(p), [w, h]) = (pages.last_mut(), n.as_slice()) {
                if p.1.is_none() {
                    p.1 = Some((*w, *h));
                }
            }
            continue;
        }
        if !indented && !t.contains(':') && !pages.is_empty() {
            regions += 1;
        }
    }
    AtlasInfo { pages, regions }
}

fn atlas(path: &Path, first_only: bool) -> Result<Image, String> {
    let text = read_text(path, 8 << 20)?;
    let info = parse_atlas(&text);
    let dir = path.parent().unwrap_or(Path::new("."));
    for (file, _) in &info.pages {
        let p = dir.join(file);
        if let Ok(bytes) = std::fs::read(&p) {
            if let Ok(mut img) = crate::decode_image_bytes(&bytes, &ext_of(file)) {
                img.profile_assumable = true;
                img.colour_note = Some(format!("Spine atlas page {file} ({} regions on {} page{})", info.regions, info.pages.len(), if info.pages.len() == 1 { "" } else { "s" }));
                return Ok(img);
            }
        }
    }
    let mut lines = vec![format!("{} region(s) on {} page(s)", info.regions, info.pages.len())];
    for (f, s) in info.pages.iter().take(4) {
        lines.push(match s {
            Some((w, h)) => format!("{f}  {w} x {h}"),
            None => f.clone(),
        });
    }
    lines.push("the page picture is not next to the atlas".into());
    Ok(card(path, "atlas", "Spine / libGDX texture atlas", lines, first_only, "a card: the atlas page picture was not found beside the file"))
}

/// The version string of a binary skeleton (`4.1.24`, `3.8.99`): the first `d.d` / `d.d.d` in its header.
pub(crate) fn skel_version(head: &[u8]) -> Option<String> {
    let text: Vec<u8> = head.iter().take(96).map(|&b| if b.is_ascii_graphic() { b } else { b' ' }).collect();
    let s = String::from_utf8_lossy(&text).into_owned();
    let bytes = s.as_bytes();
    for i in 0..bytes.len() {
        if (bytes[i] == b'3' || bytes[i] == b'4') && bytes.get(i + 1) == Some(&b'.') && bytes.get(i + 2).is_some_and(|c| c.is_ascii_digit()) {
            let end = s[i..].find(|c: char| !(c.is_ascii_digit() || c == '.')).map(|e| i + e).unwrap_or(s.len());
            return Some(s[i..end].trim_end_matches('.').to_string());
        }
    }
    None
}

fn sibling_atlas(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let dir = path.parent()?;
    for name in [format!("{stem}.atlas"), format!("{stem}.atlas.txt")] {
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
    }
    // Any atlas whose name starts with the skeleton's (hero.skel, hero-pma.atlas).
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("atlas")) && p.file_stem().is_some_and(|s| s.to_string_lossy().starts_with(&stem)))
}

fn skel(path: &Path, first_only: bool) -> Result<Image, String> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path).map_err(|e| format!("open: {e}"))?.take(128).read_to_end(&mut head).map_err(|e| e.to_string())?;
    let version = skel_version(&head);
    if let Some(a) = sibling_atlas(path) {
        if let Ok(mut img) = atlas(&a, first_only) {
            // A card means the atlas had no picture: say it is the skeleton's.
            let prior = img.colour_note.take().unwrap_or_default();
            img.colour_note = Some(format!("Spine skeleton{}: {prior}", version.as_ref().map(|v| format!(" {v}")).unwrap_or_default()));
            if !prior.starts_with("a card") {
                return Ok(img);
            }
        }
    }
    let mut lines = vec![match &version {
        Some(v) => format!("Spine version {v}"),
        None => "Spine version not recognised".into(),
    }];
    lines.push("no .atlas beside it: put the atlas and its picture in the same folder".into());
    Ok(card(path, "skel", "Spine skeleton (binary)", lines, first_only, "a card: no atlas picture found beside the skeleton"))
}

fn spine_project(path: &Path, first_only: bool) -> Result<Image, String> {
    if is_zip(path) {
        if let Ok(mut zip) = Zip::open(path) {
            if let Some(img) = zip_picture(&mut zip, &["preview.png", "thumbnail.png"], zip_limit(first_only), "Spine project picture") {
                return Ok(img);
            }
        }
    }
    Ok(card(path, "spine", "Spine editor project", vec!["open it in Spine; export the skeleton (.json / .skel) and the atlas to see its sprite sheet here".into()], first_only, "a card: a Spine project holds no preview picture"))
}

// ---- Toon Boom ---------------------------------------------------------------------------------------------------------------

fn xstage(path: &Path, first_only: bool) -> Result<Image, String> {
    let text = read_text(path, 24 << 20)?;
    let Some(doc) = parse_xml(&text) else {
        return Ok(card(path, "xstage", "Toon Boom Harmony scene", vec!["the scene's XML could not be read".into()], first_only, "a card: unreadable scene"));
    };
    let mut lines = Vec::new();
    // Harmony writes the frame rate and the resolution in several places across versions: the first that is there wins.
    let fps = attr(&doc, "scene", "fps").or_else(|| attr(&doc, "scene", "framerate")).map(str::to_string).or_else(|| text_of(&doc, "framerate")).or_else(|| text_of(&doc, "fps"));
    let res = attr(&doc, "resolution", "w").zip(attr(&doc, "resolution", "h")).map(|(w, h)| format!("{w} x {h}")).or_else(|| attr(&doc, "scene", "width").zip(attr(&doc, "scene", "height")).map(|(w, h)| format!("{w} x {h}")));
    if let Some(r) = res {
        lines.push(format!("resolution {r}"));
    }
    if let Some(f) = fps {
        lines.push(format!("{f} frames per second"));
    }
    let by_type = |t: &str| doc.descendants().filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("node") && n.attribute("type").is_some_and(|v| v.eq_ignore_ascii_case(t))).count();
    let (drawings, pegs, comps) = (by_type("READ"), by_type("PEG"), by_type("COMPOSITE"));
    lines.push(format!("{drawings} drawing layer(s), {pegs} peg(s), {comps} composite(s)"));
    let nodes = count_tag(&doc, "node");
    lines.push(format!("{nodes} nodes in the node view"));
    Ok(card(path, "xstage", "Toon Boom Harmony scene", lines, first_only, "a card: Harmony's drawings are vector files beside the scene"))
}

// ---- Moho ---------------------------------------------------------------------------------------------------------------------

/// Numbers named like a project setting somewhere in a JSON text: width, height, frame rate and end frame.
pub(crate) fn moho_facts(json: &serde_json::Value) -> Vec<String> {
    fn walk(v: &serde_json::Value, found: &mut std::collections::BTreeMap<&'static str, f64>, depth: usize) {
        if depth > 6 {
            return;
        }
        match v {
            serde_json::Value::Object(m) => {
                for (k, val) in m {
                    let key = k.to_ascii_lowercase();
                    let slot = match key.as_str() {
                        "width" | "canvaswidth" | "stagewidth" => Some("width"),
                        "height" | "canvasheight" | "stageheight" => Some("height"),
                        "fps" | "framerate" | "framespersecond" => Some("fps"),
                        "endframe" | "lastframe" | "totalframes" => Some("end"),
                        _ => None,
                    };
                    if let (Some(slot), Some(n)) = (slot, val.as_f64()) {
                        found.entry(slot).or_insert(n);
                    }
                    walk(val, found, depth + 1);
                }
            }
            serde_json::Value::Array(a) => a.iter().take(64).for_each(|x| walk(x, found, depth + 1)),
            _ => {}
        }
    }
    let mut found = std::collections::BTreeMap::new();
    walk(json, &mut found, 0);
    let mut lines = Vec::new();
    if let (Some(w), Some(h)) = (found.get("width"), found.get("height")) {
        lines.push(format!("canvas {w:.0} x {h:.0}"));
    }
    if let Some(f) = found.get("fps") {
        lines.push(format!("{f:.0} frames per second"));
    }
    if let Some(e) = found.get("end") {
        lines.push(format!("ends at frame {e:.0}"));
    }
    lines
}

fn moho(path: &Path, ext: &str, first_only: bool) -> Result<Image, String> {
    let kind = if ext == "anme" { "Anime Studio project" } else { "Moho project" };
    if is_zip(path) {
        let mut zip = Zip::open(path)?;
        if let Some(img) = zip_picture(&mut zip, &["preview.png", "thumbnail.png", "preview.jpg", "thumbnail.jpg"], zip_limit(first_only), &format!("{kind} picture")) {
            return Ok(img);
        }
        // The settings of a zipped project are in its JSON.
        let json = zip.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".json")).cloned();
        let mut lines = vec![format!("{} files inside, no picture", zip.entries.len())];
        if let Some(e) = json {
            if let Ok(b) = zip.read(&e, 16 << 20) {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
                    lines.extend(moho_facts(&v));
                }
            }
        }
        return Ok(card(path, ext, kind, lines, first_only, "a card: no picture in this project"));
    }
    let text = read_text(path, 8 << 20)?;
    let mut lines = Vec::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        lines.extend(moho_facts(&v));
    }
    if lines.is_empty() {
        lines.push("open it in Moho to see it".into());
    }
    Ok(card(path, ext, kind, lines, first_only, "a card: a Moho project holds no preview picture"))
}

// ---- Adobe Animate (uncompressed project) -------------------------------------------------------------------------------------

fn xfl(path: &Path, first_only: bool) -> Result<Image, String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let dom = dir.join("DOMDocument.xml");
    // The biggest picture of the library, if there is one.
    let mut best: Option<(u64, PathBuf)> = None;
    if let Ok(rd) = std::fs::read_dir(dir.join("LIBRARY")) {
        for e in rd.flatten() {
            let p = e.path();
            if is_image_name(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()) {
                let s = size_of(&p);
                if best.as_ref().map_or(true, |b| s > b.0) {
                    best = Some((s, p));
                }
            }
        }
    }
    if let Some((_, p)) = &best {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(mut img) = crate::decode_image_bytes(&bytes, &ext_of(&p.to_string_lossy().replace('\\', "/"))) {
                img.profile_assumable = true;
                img.colour_note = Some(format!("Animate library picture: {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                return Ok(img);
            }
        }
    }
    let mut lines = Vec::new();
    if let Ok(text) = read_text(&dom, 16 << 20) {
        if let Some(doc) = parse_xml(&text) {
            if let (Some(w), Some(h)) = (attr(&doc, "DOMDocument", "width"), attr(&doc, "DOMDocument", "height")) {
                lines.push(format!("stage {w} x {h}"));
            }
            if let Some(f) = attr(&doc, "DOMDocument", "frameRate") {
                lines.push(format!("{f} frames per second"));
            }
            lines.push(format!("{} layer(s), {} keyframe(s)", count_tag(&doc, "DOMLayer"), count_tag(&doc, "DOMFrame")));
        }
    }
    if lines.is_empty() {
        lines.push("DOMDocument.xml not found beside the project file".into());
    }
    Ok(card(path, "xfl", "Adobe Animate project (uncompressed)", lines, first_only, "a card: no library picture in this project"))
}

// ---- Pencil2D -----------------------------------------------------------------------------------------------------------------

/// A frame picture of a Pencil2D package: `data/<layer>.<frame>.png`; (layer, frame) from the name.
pub(crate) fn pencil_name(name: &str) -> Option<(u32, u32)> {
    let file = name.rsplit('/').next()?;
    let stem = file.strip_suffix(".png").or_else(|| file.strip_suffix(".PNG"))?;
    let (l, f) = stem.split_once('.')?;
    Some((l.parse().ok()?, f.parse().ok()?))
}

fn pencil2d(path: &Path, first_only: bool) -> Result<Image, String> {
    let mut zip = Zip::open(path)?;
    let entries = zip.entries.clone();
    let mut frames: Vec<(u32, u32, crate::docthumb::Entry)> = entries.iter().filter_map(|e| pencil_name(&e.name).map(|(l, f)| (l, f, e.clone()))).collect();
    // The facts from main.xml.
    let mut lines = Vec::new();
    if let Some(e) = entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with("main.xml")).cloned() {
        if let Ok(b) = zip.read(&e, 16 << 20) {
            let text = String::from_utf8_lossy(&b).into_owned();
            if let Some(doc) = parse_xml(&text) {
                if let Some(f) = attr(&doc, "fps", "value").map(str::to_string).or_else(|| text_of(&doc, "fps")) {
                    lines.push(format!("{f} frames per second"));
                }
                lines.push(format!("{} layer(s)", count_tag(&doc, "layer")));
            }
        }
    }
    if frames.is_empty() {
        return Ok(card(path, "pclx", "Pencil2D animation", [vec!["no bitmap frame in the package (vector layers only)".to_string()], lines].concat(), first_only, "a card: Pencil2D vector layers are not drawn"));
    }
    // The first frame of the animation: the lowest frame number, every bitmap layer that has a picture there, bottom layer first.
    frames.sort_by_key(|(l, f, _)| (*f, *l));
    let first = frames[0].1;
    let mut pics: Vec<Image> = Vec::new();
    for (_, f, e) in frames.iter().filter(|(_, f, _)| *f == first) {
        let _ = f;
        if let Ok(b) = zip.read(e, zip_limit(first_only)) {
            if let Ok(img) = crate::decode_image_bytes(&b, "png") {
                pics.push(img);
            }
        }
    }
    if pics.is_empty() {
        return Err("the first frame's picture could not be read".into());
    }
    let layers = pics.len();
    let mut base = pics.remove(0);
    for over in &pics {
        base = overlay(&base, over);
    }
    base.profile_assumable = true;
    base.colour_note = Some(format!("Pencil2D frame {first} ({layers} layer picture(s) composited; {} frame pictures in the package)", frames.len()));
    Ok(base)
}

/// `over` on `under`, both at their own size, aligned at the top left (Pencil2D bitmap layers are saved at the camera size),
/// on a canvas the size of the bigger one.
fn overlay(under: &Image, over: &Image) -> Image {
    let (w, h) = (under.width.max(over.width), under.height.max(over.height));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for (img, first) in [(under, true), (over, false)] {
        for y in 0..img.height {
            for x in 0..img.width {
                let s = ((y * img.width + x) * 4) as usize;
                let d = ((y * w + x) * 4) as usize;
                if first {
                    out[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
                } else {
                    let a = img.rgba[s + 3] as u32;
                    let da = out[d + 3] as u32;
                    // straight alpha "over"
                    let oa = a + da * (255 - a) / 255;
                    for c in 0..3 {
                        let top = img.rgba[s + c] as u32 * a;
                        let bottom = out[d + c] as u32 * da * (255 - a) / 255;
                        out[d + c] = if oa == 0 { 0 } else { ((top + bottom) / oa).min(255) as u8 };
                    }
                    out[d + 3] = oa.min(255) as u8;
                }
            }
        }
    }
    Image::new(w, h, out)
}

// ---- Synfig ---------------------------------------------------------------------------------------------------------------------

fn synfig(path: &Path, ext: &str, first_only: bool) -> Result<Image, String> {
    let text = match ext {
        "sif" => read_text(path, 24 << 20)?,
        "sifz" => {
            use std::io::Read;
            let f = std::fs::File::open(path).map_err(|e| format!("open: {e}"))?;
            let mut s = String::new();
            flate2::read::GzDecoder::new(f).take(48 << 20).read_to_string(&mut s).map_err(|e| format!("not a gzip file: {e}"))?;
            s
        }
        _ => {
            let mut zip = Zip::open(path)?;
            if let Some(img) = zip_picture(&mut zip, &["thumbnail.png", "preview.png"], zip_limit(first_only), "Synfig package picture") {
                return Ok(img);
            }
            let e = zip.entries.iter().find(|e| e.name.to_ascii_lowercase().ends_with(".sif")).cloned().ok_or("no canvas (.sif) in the package")?;
            String::from_utf8_lossy(&zip.read(&e, 24 << 20)?).into_owned()
        }
    };
    let Some(doc) = parse_xml(&text) else {
        return Ok(card(path, ext, "Synfig Studio canvas", vec!["the canvas XML could not be read".into()], first_only, "a card: unreadable canvas"));
    };
    let mut lines = Vec::new();
    if let (Some(w), Some(h)) = (attr(&doc, "canvas", "width"), attr(&doc, "canvas", "height")) {
        lines.push(format!("canvas {w} x {h}"));
    }
    if let Some(f) = attr(&doc, "canvas", "fps") {
        lines.push(format!("{f} frames per second"));
    }
    if let (Some(b), Some(e)) = (attr(&doc, "canvas", "begin-time"), attr(&doc, "canvas", "end-time")) {
        lines.push(format!("time {b} to {e}"));
    }
    let layers = count_tag(&doc, "layer");
    lines.push(format!("{layers} layer(s)"));
    if let Some(name) = text_of(&doc, "name") {
        lines.push(format!("name: {name}"));
    }
    Ok(card(path, ext, "Synfig Studio canvas", lines, first_only, "a card: Synfig renders its canvas itself"))
}

// ---- OpenToonz ------------------------------------------------------------------------------------------------------------------

fn tnz(path: &Path, first_only: bool) -> Result<Image, String> {
    let text = read_text(path, 24 << 20)?;
    let Some(doc) = parse_xml(&text) else {
        return Ok(card(path, "tnz", "OpenToonz scene", vec!["the scene's XML could not be read".into()], first_only, "a card: unreadable scene"));
    };
    let mut lines = Vec::new();
    if let Some(r) = text_of(&doc, "resolution") {
        lines.push(format!("resolution {}", r.replace(' ', " x ")));
    }
    if let Some(f) = text_of(&doc, "frameRate").or_else(|| text_of(&doc, "frameRate ")) {
        lines.push(format!("{f} frames per second"));
    }
    if let Some(r) = text_of(&doc, "range") {
        lines.push(format!("output range {r} (-1 = to the last frame)"));
    }
    let levels = doc.descendants().filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("level")).count();
    lines.push(format!("{levels} level(s)"));
    Ok(card(path, "tnz", "OpenToonz scene", lines, first_only, "a card: the levels are separate files (.pli .tlv .png)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_anim_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let data: Vec<u8> = (0..w * h).flat_map(|_| rgba).collect();
            enc.write_header().unwrap().write_image_data(&data).unwrap();
        }
        out
    }

    /// A zip with stored entries.
    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            out.extend(b"PK\x03\x04");
            out.extend([20u8, 0, 0, 0, 0, 0]);
            out.extend([0u8; 4]);
            out.extend(0u32.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(*data);
            central.extend(b"PK\x01\x02");
            central.extend([20u8, 0, 20, 0, 0, 0, 0, 0]);
            central.extend([0u8; 4]);
            central.extend(0u32.to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0u8; 8]);
            central.extend(0u32.to_le_bytes());
            central.extend(offset.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let cd = out.len() as u32;
        out.extend(&central);
        out.extend(b"PK\x05\x06");
        out.extend([0u8; 4]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(cd.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    const ATLAS: &str = "hero.png\nsize: 64,32\nformat: RGBA8888\nfilter: Linear,Linear\nrepeat: none\nhead\n  rotate: false\n  xy: 0, 0\n  size: 16, 16\n  orig: 16, 16\n  offset: 0, 0\n  index: -1\ntorso\n  rotate: false\n  xy: 16, 0\n  size: 16, 16\n\nhero2.png\nsize: 32,32\narm\n  bounds: 0,0,8,8\n";

    #[test]
    fn an_atlas_lists_pages_sizes_and_regions() {
        let a = parse_atlas(ATLAS);
        assert_eq!(a.pages, vec![("hero.png".to_string(), Some((64, 32))), ("hero2.png".to_string(), Some((32, 32)))]);
        assert_eq!(a.regions, 3);
        assert_eq!(parse_atlas(""), AtlasInfo { pages: vec![], regions: 0 });
    }

    #[test]
    fn an_atlas_shows_its_first_page_picture() {
        let d = scratch("atlas");
        std::fs::write(d.join("hero.atlas"), ATLAS).unwrap();
        std::fs::write(d.join("hero.png"), png(64, 32, [200, 30, 30, 255])).unwrap();
        let img = decode(&d.join("hero.atlas"), "atlas", true).unwrap().unwrap();
        assert_eq!((img.width, img.height), (64, 32));
        assert!(img.colour_note.unwrap().contains("3 regions on 2 pages"));
        // A skeleton beside it borrows the atlas.
        std::fs::write(d.join("hero.skel"), b"\x00\x00\x00\x00\x00\x00\x00\x01\x054.1.24\x00").unwrap();
        let s = decode(&d.join("hero.skel"), "skel", true).unwrap().unwrap();
        assert_eq!((s.width, s.height), (64, 32));
        assert!(s.colour_note.unwrap().contains("4.1.24"));
        // Without the picture the atlas is a card, and says so.
        std::fs::remove_file(d.join("hero.png")).unwrap();
        let c = decode(&d.join("hero.atlas"), "atlas", true).unwrap().unwrap();
        assert!(c.colour_note.unwrap().starts_with("a card"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_spine_version_is_found_in_the_header() {
        assert_eq!(skel_version(b"\x00\x12abcdef\x053.8.99\x00more").as_deref(), Some("3.8.99"));
        assert_eq!(skel_version(b"\x01\x02\x03\x04\x05\x064.2.33").as_deref(), Some("4.2.33"));
        assert_eq!(skel_version(b"no version here at all"), None);
    }

    #[test]
    fn a_harmony_scene_gives_its_facts() {
        let d = scratch("xstage");
        let xml = r#"<?xml version="1.0"?><project><scene fps="24" width="1920" height="1080"><nodeslist><node type="READ" name="Drawing"/><node type="READ" name="Drawing2"/><node type="PEG" name="Peg"/><node type="COMPOSITE" name="Composite"/></nodeslist></scene></project>"#;
        std::fs::write(d.join("a.xstage"), xml).unwrap();
        let img = decode(&d.join("a.xstage"), "xstage", true).unwrap().unwrap();
        assert!(img.width > 0 && img.colour_note.unwrap().starts_with("a card"));
        // The reading of the facts themselves.
        let doc = parse_xml(xml).unwrap();
        assert_eq!(attr(&doc, "scene", "fps"), Some("24"));
        assert_eq!(count_tag(&doc, "node"), 4);
        std::fs::write(d.join("bad.xstage"), "<project><scene").unwrap();
        assert!(decode(&d.join("bad.xstage"), "xstage", true).unwrap().is_ok(), "an unreadable scene is a card, not an error");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn moho_settings_are_found_in_text_and_in_zip_json() {
        let v: serde_json::Value = serde_json::from_str(r#"{"project":{"canvas":{"width":1280,"height":720},"fps":30,"endFrame":240}}"#).unwrap();
        assert_eq!(moho_facts(&v), vec!["canvas 1280 x 720", "30 frames per second", "ends at frame 240"]);
        let d = scratch("moho");
        std::fs::write(d.join("a.moho"), r#"{"width":640,"height":360,"fps":24}"#).unwrap();
        assert!(decode(&d.join("a.moho"), "moho", true).unwrap().is_ok());
        // Zipped, with a preview picture.
        std::fs::write(d.join("b.moho"), make_zip(&[("Project.json", b"{}"), ("Preview.png", &png(30, 20, [9, 9, 9, 255]))])).unwrap();
        let img = decode(&d.join("b.moho"), "moho", true).unwrap().unwrap();
        assert_eq!((img.width, img.height), (30, 20));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_uncompressed_animate_project_shows_its_library_picture_or_facts() {
        let d = scratch("xfl");
        std::fs::write(d.join("a.xfl"), "PROXY-CS5").unwrap();
        std::fs::write(d.join("DOMDocument.xml"), r#"<DOMDocument width="800" height="600" frameRate="30"><timelines><DOMTimeline><layers><DOMLayer><frames><DOMFrame index="0"/></frames></DOMLayer></layers></DOMTimeline></timelines></DOMDocument>"#).unwrap();
        let card = decode(&d.join("a.xfl"), "xfl", true).unwrap().unwrap();
        assert!(card.colour_note.unwrap().starts_with("a card"));
        std::fs::create_dir_all(d.join("LIBRARY")).unwrap();
        std::fs::write(d.join("LIBRARY").join("hero.png"), png(40, 30, [1, 2, 3, 255])).unwrap();
        let img = decode(&d.join("a.xfl"), "xfl", true).unwrap().unwrap();
        assert_eq!((img.width, img.height), (40, 30));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pencil2d_frame_names_and_the_composited_first_frame() {
        assert_eq!(pencil_name("data/001.003.png"), Some((1, 3)));
        assert_eq!(pencil_name("data/x.png"), None);
        assert_eq!(pencil_name("main.xml"), None);
        let d = scratch("pencil");
        let main = br#"<document><projectdata><fps>12</fps></projectdata><object><layer id="1" name="a"/><layer id="2" name="b"/></object></document>"#;
        let zip = make_zip(&[("main.xml", main), ("data/001.002.png", &png(8, 8, [255, 0, 0, 255])), ("data/002.002.png", &png(8, 8, [0, 0, 255, 128])), ("data/001.005.png", &png(8, 8, [0, 255, 0, 255]))]);
        std::fs::write(d.join("a.pclx"), zip).unwrap();
        let img = decode(&d.join("a.pclx"), "pclx", true).unwrap().unwrap();
        // Frame 2 is the first: a red layer with a half-transparent blue one over it -> purple-ish, opaque.
        assert_eq!((img.width, img.height), (8, 8));
        let p = &img.rgba[0..4];
        assert_eq!(p[3], 255);
        assert!(p[0] > 100 && p[2] > 100 && p[1] < 10, "{p:?}");
        assert!(img.colour_note.unwrap().contains("frame 2"));
        // Vector-only: a card.
        std::fs::write(d.join("v.pclx"), make_zip(&[("main.xml", main)])).unwrap();
        assert!(decode(&d.join("v.pclx"), "pclx", true).unwrap().unwrap().colour_note.unwrap().starts_with("a card"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn synfig_canvases_plain_gzipped_and_packaged() {
        let d = scratch("synfig");
        let sif = r#"<?xml version="1.0"?><canvas version="1.4" width="480" height="270" fps="24" begin-time="0" end-time="5s"><name>Walk</name><layer type="rectangle"/><layer type="circle"/></canvas>"#;
        std::fs::write(d.join("a.sif"), sif).unwrap();
        assert!(decode(&d.join("a.sif"), "sif", true).unwrap().is_ok());
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(sif.as_bytes()).unwrap();
        std::fs::write(d.join("a.sifz"), enc.finish().unwrap()).unwrap();
        assert!(decode(&d.join("a.sifz"), "sifz", true).unwrap().is_ok());
        std::fs::write(d.join("a.sfg"), make_zip(&[("canvas.sif", sif.as_bytes())])).unwrap();
        assert!(decode(&d.join("a.sfg"), "sfg", true).unwrap().is_ok());
        let doc = parse_xml(sif).unwrap();
        assert_eq!(attr(&doc, "canvas", "width"), Some("480"));
        assert_eq!(count_tag(&doc, "layer"), 2);
        std::fs::write(d.join("bad.sifz"), b"not gzip").unwrap();
        assert!(decode(&d.join("bad.sifz"), "sifz", true).unwrap().is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn opentoonz_scene_and_the_closed_formats_are_cards() {
        let d = scratch("tnz");
        std::fs::write(d.join("a.tnz"), r#"<tnz><properties><outputs><output name="main"><range>0 -1</range><frameRate>24</frameRate><resolution>1920 1080</resolution></output></outputs></properties><levelSet><level name="A"/><level name="B"/></levelSet></tnz>"#).unwrap();
        assert!(decode(&d.join("a.tnz"), "tnz", true).unwrap().is_ok());
        std::fs::write(d.join("a.tvpp"), b"TVPP....").unwrap();
        assert!(decode(&d.join("a.tvpp"), "tvpp", true).unwrap().unwrap().colour_note.unwrap().starts_with("a card"));
        std::fs::write(d.join("a.spine"), b"\x01\x02\x03").unwrap();
        assert!(decode(&d.join("a.spine"), "spine", true).unwrap().unwrap().colour_note.unwrap().starts_with("a card"));
        std::fs::write(d.join("a.sbpz"), make_zip(&[("thumbnails/thumbnail.png", &png(12, 9, [5, 5, 5, 255]))])).unwrap();
        let s = decode(&d.join("a.sbpz"), "sbpz", true).unwrap().unwrap();
        assert_eq!((s.width, s.height), (12, 9));
        assert!(decode(&d.join("a.png"), "png", true).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn utf16_project_text_is_read() {
        let d = scratch("u16");
        let mut b = vec![0xFFu8, 0xFE];
        for u in "<project><scene fps=\"25\"/></project>".encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        std::fs::write(d.join("a.xstage"), b).unwrap();
        let t = read_text(&d.join("a.xstage"), 1 << 20).unwrap();
        assert!(t.starts_with("<project>") && t.contains("fps"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
