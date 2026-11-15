//! Presets: a recipe kept under a name, to apply to any picture. Stored as plain text files (the pair-file format) in
//! `%APPDATA%\NuViewer\presets`, one `.nupreset` per preset, so they can be copied between machines or edited by hand.
//!
//! A preset holds the *look* (white balance, light, colour, detail, film, curves, hue wheel, LUT). What belongs to one picture
//! (turn, flips, crop, perspective, heal strokes) is not stored and not applied.

use crate::recipe::{Recipe, P};
use crate::sidecar::{self, Source};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "nupreset";

/// The folder of the presets (`NUEDITOR_PRESET_DIR` overrides it, for tests).
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NUEDITOR_PRESET_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("NuViewer").join("presets")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: String,
    pub path: PathBuf,
}

/// The recipe without what is specific to one picture.
pub fn look_of(r: &Recipe) -> Recipe {
    let mut p = r.clone();
    keep_picture_parts(&mut p, &Recipe::default());
    p
}

/// Copy the picture-specific parts of `from` into `r` (turn, flips, crop, perspective, strokes).
fn keep_picture_parts(r: &mut Recipe, from: &Recipe) {
    for param in [P::Angle, P::Pitch, P::Yaw, P::Zoom] {
        r.set(param, from.get(param));
    }
    r.quarter_turns = from.quarter_turns;
    r.flip_h = from.flip_h;
    r.flip_v = from.flip_v;
    r.crop = from.crop;
    r.retouch = from.retouch.clone();
}

/// `target` takes the look of `preset`; its own turn, crop, perspective and strokes stay.
pub fn apply(target: &Recipe, preset: &Recipe) -> Recipe {
    let mut out = look_of(preset);
    keep_picture_parts(&mut out, target);
    out
}

/// A file name for a preset: letters, digits, spaces and a few marks; never empty.
pub fn clean_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "preset".to_string() } else { s.chars().take(60).collect() }
}

pub fn path_for(name: &str) -> PathBuf {
    dir().join(format!("{}.{EXTENSION}", clean_name(name)))
}

/// The presets, by name.
pub fn list() -> Vec<Preset> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut v: Vec<Preset> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case(EXTENSION)).unwrap_or(false))
        .filter_map(|p| p.file_stem().map(|s| Preset { name: s.to_string_lossy().into_owned(), path: p.clone() }))
        .collect();
    v.sort_by_key(|p| p.name.to_lowercase());
    v
}

/// Save the look of `r` at `path` (made into a `.nupreset` file, in the presets folder if only a name was given).
pub fn save_at(path: &Path, r: &Recipe) -> std::io::Result<PathBuf> {
    let mut path = path.to_path_buf();
    if path.extension().map(|e| !e.eq_ignore_ascii_case(EXTENSION)).unwrap_or(true) {
        path.set_extension(EXTENSION);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&path, sidecar::to_text(&look_of(r), &Source::default()))?;
    Ok(path)
}

pub fn save(name: &str, r: &Recipe) -> std::io::Result<PathBuf> {
    save_at(&path_for(name), r)
}

pub fn load(path: &Path) -> Result<Recipe, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    sidecar::from_text(&text).map(|(r, _)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{Crop, Retouch, RetouchKind};

    #[test]
    fn a_preset_keeps_the_look_and_leaves_the_picture_alone() {
        let mut r = Recipe::default();
        r.set(P::Exposure, 0.7);
        r.set(P::Contrast, 20.0);
        r.set(P::Grain, 40.0);
        r.curves.master.set_point(0.5, 0.6);
        r.invert = true;
        // Picture-specific: must not travel.
        r.set(P::Angle, 12.0);
        r.flip_h = true;
        r.crop = Some(Crop { x: 0.1, y: 0.1, w: 0.5, h: 0.5 });
        r.retouch.push(Retouch { kind: RetouchKind::RedEye, radius: 0.01, source: None, points: vec![(0.5, 0.5)] });
        let look = look_of(&r);
        assert_eq!(look.get(P::Exposure), 0.7);
        assert!(look.invert && !look.curves.is_identity());
        assert!(look.geometry_identity() && look.retouch.is_empty());

        // Applied to another picture: its own turn / crop / strokes stay, its own look is replaced.
        let mut other = Recipe::default();
        other.set(P::Saturation, 50.0);
        other.set(P::Angle, -3.0);
        other.crop = Some(Crop { x: 0.0, y: 0.0, w: 0.9, h: 0.9 });
        let out = apply(&other, &look);
        assert_eq!(out.get(P::Exposure), 0.7);
        assert_eq!(out.get(P::Saturation), 0.0, "the old look is replaced, not mixed");
        assert_eq!(out.get(P::Angle), -3.0);
        assert!(out.crop.is_some());
    }

    #[test]
    fn presets_are_files_that_list_load_and_survive_odd_names() {
        let d = std::env::temp_dir().join(format!("nv-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::env::set_var("NUEDITOR_PRESET_DIR", &d);
        assert!(list().is_empty());
        let mut r = Recipe::default();
        r.set(P::Temp, 30.0);
        let p = save("Warm: evening/light", &r).unwrap();
        assert!(p.starts_with(&d) && p.file_name().unwrap().to_string_lossy().ends_with(".nupreset"), "{p:?}");
        save("Cool", &Recipe::default()).unwrap();
        let names: Vec<String> = list().into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Cool".to_string(), "Warm_ evening_light".to_string()]);
        assert_eq!(load(&p).unwrap().get(P::Temp), 30.0);
        assert_eq!(clean_name("..."), "preset");
        assert!(load(&d.join("nope.nupreset")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
