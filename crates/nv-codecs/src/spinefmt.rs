//! A Spine skeleton (`.skel`, or a skeleton `.json`) with its atlas and page pictures, as a rig the viewer can play. The picture this gives
//! is the first frame: the skeleton in its setup pose (or the first animation at time 0), fitted to a canvas, with real transparency.

use crate::Image;
use nv_anim::{PageImage, Rig, View};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A `.json` is only taken for Spine when its first kilobytes have the `"skeleton"` header with a `"spine"` version.
pub fn looks_like_spine_json(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4096];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let t = String::from_utf8_lossy(&head[..n]);
    t.contains("\"skeleton\"") && t.contains("\"spine\"")
}

pub fn is_candidate(path: &Path, ext: &str) -> bool {
    ext == "skel" || (ext == "json" && looks_like_spine_json(path))
}

/// The atlas next to a skeleton: same name, else `name.atlas.txt`, else the only (or best matching) `.atlas` in the folder.
pub fn find_atlas(skel: &Path) -> Option<PathBuf> {
    let dir = skel.parent()?;
    let stem = skel.file_stem()?.to_string_lossy().into_owned();
    for name in [format!("{stem}.atlas"), format!("{stem}.atlas.txt"), format!("{stem}.txt")] {
        let p = dir.join(&name);
        if p.is_file() && name.ends_with(".atlas") || p.is_file() && name.ends_with(".atlas.txt") {
            return Some(p);
        }
    }
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("atlas")).unwrap_or(false)).collect();
    all.sort();
    all.iter().find(|p| p.file_stem().map(|s| s.to_string_lossy().starts_with(&stem) || stem.starts_with(&*s.to_string_lossy())).unwrap_or(false)).cloned().or_else(|| if all.len() == 1 { all.pop() } else { None })
}

/// Read the skeleton, its atlas and the page pictures.
pub fn load(path: &Path) -> Result<Arc<Rig>, String> {
    let atlas_path = find_atlas(path).ok_or_else(|| format!("{}: no .atlas file beside this skeleton (Spine exports one with the skeleton: keep them together)", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))?;
    let atlas_text = String::from_utf8_lossy(&crate::progress::read_file(&atlas_path)?).into_owned();
    let mut atlas = nv_anim::Atlas::parse(&atlas_text)?;
    let dir = atlas_path.parent().unwrap_or(Path::new("."));
    for page in &mut atlas.pages {
        let mut file = dir.join(&page.name);
        if !file.is_file() {
            // Case differences and exports that renamed the page: a file of that name in any case.
            let want = page.name.to_ascii_lowercase();
            if let Some(f) = std::fs::read_dir(dir).ok().and_then(|d| d.flatten().find(|e| e.file_name().to_string_lossy().to_ascii_lowercase() == want)) {
                file = f.path();
            }
        }
        let bytes = crate::progress::read_file(&file).map_err(|e| format!("atlas page {}: {e}", page.name))?;
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
        let img = crate::decode_image_bytes(&bytes, &ext).map_err(|e| format!("atlas page {}: {e}", page.name))?;
        page.image = Some(Arc::new(PageImage { width: img.width, height: img.height, rgba: img.rgba }));
    }
    let bytes = crate::progress::read_file(path)?;
    let is_skel = path.extension().map(|e| e.eq_ignore_ascii_case("skel")).unwrap_or(false);
    let mut rig = if is_skel {
        match nv_anim::binary::parse(&bytes, atlas.clone()) {
            Ok(r) => r,
            Err(e) => {
                // A binary this reader cannot read (4.2, path constraints, a layout it guessed wrong): the JSON export beside it, if there is one.
                if std::env::var_os("NV_SKEL_NO_JSON").is_some() {
                    return Err(e); // (tests of the binary reader itself: no help from the JSON beside it)
                }
                let sibling = ["json", "JSON"].iter().map(|x| path.with_extension(x)).find(|p| p.is_file() && looks_like_spine_json(p));
                match sibling {
                    Some(j) => nv_anim::json::parse(&String::from_utf8_lossy(&crate::progress::read_file(&j)?), atlas)?,
                    None => return Err(e),
                }
            }
        }
    } else {
        nv_anim::json::parse(&String::from_utf8_lossy(&bytes), atlas)?
    };
    rig.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Arc::new(rig))
}

/// The canvas size and view that show the whole of `anim` of `rig`: the long side is at most `max` pixels.
pub fn canvas_for(rig: &Rig, skin: usize, anim: Option<usize>, max: usize) -> (usize, usize, View) {
    let b = bounds_for(rig, skin, anim);
    let (bw, bh) = ((b[2] - b[0]).max(1.0), (b[3] - b[1]).max(1.0));
    let k = max as f32 / bw.max(bh);
    let (w, h) = (((bw * k).round() as usize).clamp(16, max), ((bh * k).round() as usize).clamp(16, max));
    (w, h, View::fit(b, w, h, 0.0))
}

/// The box (x0, y0, x1, y1 in skeleton units) that holds the whole of an animation's motion (the skeleton's own box when it has no
/// geometry): what a view fits to the window.
pub fn bounds_for(rig: &Rig, skin: usize, anim: Option<usize>) -> [f32; 4] {
    // The camera of the BD2 viewer first (the setup pose, background slots left out when they would dwarf the picture); a skeleton that
    // shows nothing in its setup pose is measured over its animation instead.
    rig.camera_bounds(skin).or_else(|| rig.bounds(skin, anim, 16)).or_else(|| (rig.width > 0.0 && rig.height > 0.0).then(|| [rig.x, rig.y, rig.x + rig.width, rig.y + rig.height])).unwrap_or([-100.0, -100.0, 100.0, 100.0])
}

pub fn decode(path: &Path, first_only: bool) -> Result<Image, String> {
    let rig = load(path)?;
    let anim = if rig.animations.is_empty() { None } else { Some(0) };
    let (w, h, view) = canvas_for(&rig, 0, anim, if first_only { 512 } else { 1024 });
    let pose = rig.pose(0, None, 0.0);
    let rgba = rig.draw(&pose, w, h, view, 1, None);
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.profile_assumable = false;
    img.colour_note = Some(format!("Spine {} skeleton: {} bones, {} slots, {} animations, {} skins", if rig.version.is_empty() { "?" } else { &rig.version }, rig.bones.len(), rig.slots.len(), rig.animations.len(), rig.skins.len()));
    img.rig = Some(rig);
    Ok(img)
}
