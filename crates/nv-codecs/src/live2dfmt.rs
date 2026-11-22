//! A Live2D model (`*.model3.json`, or a `.moc3` next to one): the files it lists, loaded for the viewer's player. The `.moc3` itself is
//! evaluated by Live2D's Cubism Core DLL, which the user downloads (`nv_anim::cubism`, `docs/live2d.md`); without it opening a model
//! gives the message that says how to get it.

use crate::Image;
use nv_anim::cubism::{Core, Model};
use nv_anim::live2d::{self, DisplayInfo, Model3, Motion};
use nv_anim::{PageImage, View};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn is_model3(path: &Path) -> bool {
    path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".model3.json")).unwrap_or(false)
}

/// `foo.moc3` -> the `.model3.json` in its folder that names it (or the only one).
pub fn model3_for(path: &Path) -> Option<PathBuf> {
    if is_model3(path) {
        return Some(path.to_path_buf());
    }
    let dir = path.parent()?;
    let moc = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let all: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| is_model3(p)).collect();
    all.iter().find(|p| std::fs::read_to_string(p).map(|t| t.to_ascii_lowercase().contains(&moc)).unwrap_or(false)).cloned().or_else(|| if all.len() == 1 { all.into_iter().next() } else { None })
}

pub struct Loaded {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub name: String,
    pub info: Model3,
    pub display: DisplayInfo,
    pub moc: Vec<u8>,
    pub textures: Vec<Option<Arc<PageImage>>>,
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let path = model3_for(path).ok_or("Live2D: no .model3.json beside this .moc3")?;
    let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let text = String::from_utf8_lossy(&crate::progress::read_file(&path)?).into_owned();
    let info = live2d::parse_model3(&text)?;
    let display = info.display_info.as_ref().and_then(|f| std::fs::read_to_string(dir.join(f)).ok()).and_then(|t| live2d::parse_cdi3(&t).ok()).unwrap_or_default();
    let moc = crate::progress::read_file(&dir.join(&info.moc)).map_err(|e| format!("{}: {e}", info.moc))?;
    let mut textures = Vec::new();
    for t in &info.textures {
        let bytes = crate::progress::read_file(&dir.join(t)).map_err(|e| format!("texture {t}: {e}"))?;
        let ext = Path::new(t).extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
        let img = crate::decode_image_bytes(&bytes, &ext).map_err(|e| format!("texture {t}: {e}"))?;
        textures.push(Some(Arc::new(PageImage { width: img.width, height: img.height, rgba: img.rgba })));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(".model3.json").to_string()).unwrap_or_default();
    Ok(Loaded { path, dir, name, info, display, moc, textures })
}

/// The motion file `file` of a model (relative to its folder).
pub fn load_motion(dir: &Path, file: &str) -> Result<Motion, String> {
    live2d::parse_motion3(&String::from_utf8_lossy(&crate::progress::read_file(&dir.join(file))?))
}

pub fn load_expression(dir: &Path, file: &str) -> Result<Vec<(String, f32, live2d::ExpBlend)>, String> {
    live2d::parse_exp3(&String::from_utf8_lossy(&crate::progress::read_file(&dir.join(file))?))
}

/// The Core, from beside the program or the places `Core::search_places` lists.
pub fn core() -> Result<Arc<Core>, String> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    Core::load(exe_dir.as_deref())
}

/// The model in its default pose: (canvas width, height, view) fitting everything it draws, long side at most `max`.
pub fn canvas_for_model(model: &mut Model, max: usize) -> (usize, usize, View) {
    let items = live2d::items_from_drawables(&model.drawables());
    let b = live2d::bounds_of_items(&items).unwrap_or([-1.0, -1.0, 1.0, 1.0]);
    let (bw, bh) = ((b[2] - b[0]).max(1e-3), (b[3] - b[1]).max(1e-3));
    let k = max as f32 / bw.max(bh);
    let (w, h) = (((bw * k).round() as usize).clamp(16, max), ((bh * k).round() as usize).clamp(16, max));
    (w, h, View::fit(b, w, h, 0.0))
}

/// The first picture: the model with default parameters.
pub fn decode(path: &Path, first_only: bool) -> Result<Image, String> {
    let loaded = load(path)?;
    let core = core()?;
    let mut model = Model::new(core, &loaded.moc)?;
    let (w, h, view) = canvas_for_model(&mut model, if first_only { 512 } else { 1024 });
    let items = live2d::items_from_drawables(&model.drawables());
    let rgba = nv_anim::render_pages(&loaded.textures, &items, w, h, view, 1, None);
    let mut img = Image::new(w as u32, h as u32, rgba);
    img.colour_note = Some(format!("Live2D model {}: {} parameters, {} parts, {} motions, {} expressions", loaded.name, model.params.len(), model.parts.len(), loaded.info.motions.len(), loaded.info.expressions.len()));
    Ok(img)
}
