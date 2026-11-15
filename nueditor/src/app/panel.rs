//! The parts of the right-hand panel that are not a plain slider of the recipe: the heal tool strip and brush size (in the Heal
//! group), the LUT buttons (in Film), the Presets section and the "Export copy" section (resize, colours, file size, watermark,
//! background removal). Every one of them is an [`Item`]: a labelled row that is a slider (`Num`), a value that a click cycles
//! (`Pick`), a button (`Btn`), a preset name or the tool strip. The panel lays them out between the slider rows, draws them in
//! the same style and sends the mouse to [`App::item_press`] and friends.

use super::*;
use nv_editor::convert::Out;
use nv_editor::export::{Export, Mark, SizeMode};
use nv_editor::preset::{self, Preset};
use nv_editor::recipe::RetouchKind;
use nv_editor::resample::Algo;
use nv_editor::watermark::Place;

pub const SECTION_PRESETS: usize = 9;
pub const SECTION_EXPORT: usize = 10;
/// Panel sections after the seven slider groups: Curves, Hue wheel, Presets, Export.
pub const SECTION_NAMES: [&str; 4] = ["Curves", "Hue wheel", "Presets", "Export copy"];
pub const GROUP_COMPOSITION: usize = 4;
pub const GROUP_HEAL: usize = 5;
pub const GROUP_FILM: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Num {
    Brush,
    Quality,
    SizeValue,
    Colours,
    TargetKb,
    RemoveBg,
    MarkSize,
    MarkOpacity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Format,
    SizeMode,
    Algo,
    Dither,
    KeepExif,
    MarkOn,
    MarkPlace,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Btn {
    SaveCopy,
    ResetExport,
    SavePreset,
    LutChoose,
    LutClear,
    ClearStrokes,
    /// Composition: start the crop frame, cut away the empty corners, take the crop off.
    CropTool,
    AutoCrop,
    ClearCrop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum What {
    Num(Num),
    Pick(Pick),
    Btn(Btn),
    Preset(usize),
    Tools,
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub what: What,
    pub rect: [f32; 4],
    /// Where the slider bar is (`Num` only).
    pub track: [f32; 4],
}

/// The retouch tools of the strip, in order.
pub const TOOL_KINDS: [(RetouchKind, &str, &str); 5] = [
    (RetouchKind::Clone, "Clone", "K"),
    (RetouchKind::Heal, "Heal", "J"),
    (RetouchKind::Remove, "Remove", "M"),
    (RetouchKind::Patch, "Patch", "P"),
    (RetouchKind::RedEye, "Red-eye", "E"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fmt {
    Same,
    Jpeg,
    Png,
    Webp,
    Bmp,
}

impl Fmt {
    const ALL: [Fmt; 5] = [Fmt::Same, Fmt::Jpeg, Fmt::Png, Fmt::Webp, Fmt::Bmp];

    fn label(self) -> &'static str {
        match self {
            Fmt::Same => "Same as the picture",
            Fmt::Jpeg => "JPEG",
            Fmt::Png => "PNG",
            Fmt::Webp => "WebP (lossless)",
            Fmt::Bmp => "BMP",
        }
    }

    pub fn from_path(p: &Path) -> Option<Fmt> {
        match p.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Fmt::Jpeg),
            "png" => Some(Fmt::Png),
            "webp" => Some(Fmt::Webp),
            "bmp" => Some(Fmt::Bmp),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeKind {
    Original,
    Percent,
    LongEdge,
    Width,
    Height,
}

impl SizeKind {
    const ALL: [SizeKind; 5] = [SizeKind::Original, SizeKind::Percent, SizeKind::LongEdge, SizeKind::Width, SizeKind::Height];
}

/// What the Export section holds. Settings gives the starting values; they are for this session's copies only.
#[derive(Clone, PartialEq, Debug)]
pub struct ExportUi {
    pub format: Fmt,
    pub quality: f32,
    pub size_kind: SizeKind,
    pub size_value: f32,
    pub algo: Algo,
    /// 0 / 1 = off.
    pub colours: f32,
    pub dither: bool,
    /// 0 = off.
    pub target_kb: f32,
    /// 0 = off, else the tolerance.
    pub remove_bg: f32,
    pub mark_on: bool,
    pub mark_place: Place,
    pub mark_size: f32,
    pub mark_opacity: f32,
    pub keep_exif: bool,
}

impl ExportUi {
    pub fn from_settings(s: &Settings) -> ExportUi {
        ExportUi {
            format: match s.editor_format {
                nv_core::settings::EditorFormat::Same => Fmt::Same,
                nv_core::settings::EditorFormat::Jpeg => Fmt::Jpeg,
                nv_core::settings::EditorFormat::Png => Fmt::Png,
                nv_core::settings::EditorFormat::Webp => Fmt::Webp,
            },
            quality: s.editor_quality as f32,
            size_kind: SizeKind::Original,
            size_value: 50.0,
            algo: Algo::Lanczos,
            colours: 0.0,
            dither: true,
            target_kb: 0.0,
            remove_bg: 0.0,
            mark_on: false,
            mark_place: Place::BottomRight,
            mark_size: 25.0,
            mark_opacity: 60.0,
            keep_exif: s.editor_exif,
        }
    }

    /// The same, as what the export pipeline takes. `Err` says what is missing.
    pub fn to_export(&self, original: &Path, watermark_text: &str) -> Result<Export, String> {
        let q = self.quality.round().clamp(1.0, 100.0) as u8;
        let format = match self.format {
            Fmt::Jpeg => Out::Jpeg(q),
            Fmt::Png => Out::Png,
            Fmt::Webp => Out::Webp,
            Fmt::Bmp => Out::Bmp,
            Fmt::Same => match nv_editor::save::Format::of_path(original) {
                Some(nv_editor::save::Format::Jpeg) => Out::Jpeg(q),
                _ => Out::Png,
            },
        };
        let mut e = Export::new(format);
        e.size = match self.size_kind {
            SizeKind::Original => SizeMode::Original,
            SizeKind::Percent => SizeMode::Percent(self.size_value),
            SizeKind::LongEdge => SizeMode::LongEdge(self.size_value.round() as u32),
            SizeKind::Width => SizeMode::Width(self.size_value.round() as u32),
            SizeKind::Height => SizeMode::Height(self.size_value.round() as u32),
        };
        e.algo = self.algo;
        e.colours = (self.colours >= 2.0).then(|| self.colours.round() as u32);
        e.dither = self.dither;
        e.target_kb = (self.target_kb >= 1.0).then(|| self.target_kb.round() as u32);
        e.remove_bg = (self.remove_bg >= 1.0).then_some(self.remove_bg);
        e.keep_exif = self.keep_exif;
        if self.mark_on {
            let Some(image) = flow::text_mark(watermark_text) else {
                return Err("Type the watermark text in Settings > NuEditor first".into());
            };
            e.mark = Some(Mark { image, place: self.mark_place, width_frac: self.mark_size / 100.0, opacity: self.mark_opacity / 100.0 });
        }
        Ok(e)
    }

}

fn bar_row() -> f32 {
    44.0
}

impl App {
    // ------------------------------------------------------------------------------------------ what is shown where

    /// Items that come before the sliders of a group.
    pub(super) fn lead_whats(&self, group: usize) -> Vec<What> {
        match group {
            GROUP_HEAL => vec![What::Tools, What::Num(Num::Brush)],
            GROUP_COMPOSITION => vec![What::Btn(Btn::CropTool), What::Btn(Btn::AutoCrop)],
            _ => Vec::new(),
        }
    }

    /// Items that come after the sliders of a group.
    pub(super) fn tail_whats(&self, group: usize) -> Vec<What> {
        match group {
            GROUP_COMPOSITION if self.recipe.crop.is_some() => vec![What::Btn(Btn::ClearCrop)],
            GROUP_HEAL if !self.recipe.retouch.is_empty() => vec![What::Btn(Btn::ClearStrokes)],
            GROUP_FILM => {
                let mut v = vec![What::Btn(Btn::LutChoose)];
                if self.recipe.lut.is_some() {
                    v.push(What::Btn(Btn::LutClear));
                }
                v
            }
            _ => Vec::new(),
        }
    }

    /// The whole content of a Presets / Export section.
    pub(super) fn section_whats(&self, section: usize) -> Vec<What> {
        if section == SECTION_PRESETS {
            let mut v: Vec<What> = (0..self.presets.len()).map(What::Preset).collect();
            v.push(What::Btn(Btn::SavePreset));
            return v;
        }
        let e = &self.export_ui;
        let mut v = vec![What::Pick(Pick::Format), What::Num(Num::Quality), What::Pick(Pick::SizeMode)];
        if e.size_kind != SizeKind::Original {
            v.push(What::Num(Num::SizeValue));
            v.push(What::Pick(Pick::Algo));
        }
        v.push(What::Num(Num::Colours));
        if e.colours >= 2.0 {
            v.push(What::Pick(Pick::Dither));
        }
        v.push(What::Num(Num::TargetKb));
        v.push(What::Num(Num::RemoveBg));
        v.push(What::Pick(Pick::MarkOn));
        if e.mark_on {
            v.push(What::Pick(Pick::MarkPlace));
            v.push(What::Num(Num::MarkSize));
            v.push(What::Num(Num::MarkOpacity));
        }
        v.push(What::Pick(Pick::KeepExif));
        v.push(What::Btn(Btn::SaveCopy));
        v.push(What::Btn(Btn::ResetExport));
        v
    }

    /// Put `whats` one under the other starting at `*y`.
    pub(super) fn place_items(&self, whats: &[What], panel: [f32; 4], y: &mut f32, out: &mut Vec<Item>) {
        let d = self.dpi;
        let pad = 14.0 * d;
        for &what in whats {
            let h = match what {
                What::Num(_) => bar_row(),
                What::Pick(_) | What::Preset(_) => 28.0,
                What::Btn(_) => 34.0,
                What::Tools => 36.0,
            } * d;
            let rect = match what {
                What::Btn(_) | What::Tools => [panel[0] + pad, *y + 3.0 * d, panel[2] - pad, *y + h - 3.0 * d],
                _ => [panel[0], *y, panel[2], *y + h],
            };
            let track = [panel[0] + pad, *y + 28.0 * d, panel[2] - pad, *y + 32.0 * d];
            out.push(Item { what, rect, track });
            *y += h;
        }
    }

    // ------------------------------------------------------------------------------------------ numbers

    fn num_label(&self, n: Num) -> String {
        match n {
            Num::Brush => "Brush size".into(),
            Num::Quality => "JPEG quality".into(),
            Num::SizeValue => match self.export_ui.size_kind {
                SizeKind::Percent => "Size (%)".into(),
                SizeKind::LongEdge => "Long edge (px)".into(),
                SizeKind::Width => "Width (px)".into(),
                _ => "Height (px)".into(),
            },
            Num::Colours => "Reduce colours".into(),
            Num::TargetKb => "File size at most (KB)".into(),
            Num::RemoveBg => "Remove plain background".into(),
            Num::MarkSize => "Watermark size (% of width)".into(),
            Num::MarkOpacity => "Watermark opacity".into(),
        }
    }

    fn num_range(&self, n: Num) -> (f32, f32) {
        match n {
            Num::Brush => (0.2, 12.0),
            Num::Quality => (1.0, 100.0),
            Num::SizeValue => match self.export_ui.size_kind {
                SizeKind::Percent => (5.0, 400.0),
                _ => (100.0, 8000.0),
            },
            Num::Colours => (0.0, 256.0),
            Num::TargetKb => (0.0, 2000.0),
            Num::RemoveBg => (0.0, 100.0),
            Num::MarkSize => (5.0, 100.0),
            Num::MarkOpacity => (5.0, 100.0),
        }
    }

    fn num_default(&self, n: Num) -> f32 {
        let base = ExportUi::from_settings(&self.settings);
        match n {
            Num::Brush => 2.0,
            Num::Quality => base.quality,
            Num::SizeValue => match self.export_ui.size_kind {
                SizeKind::Percent => 50.0,
                _ => 2000.0,
            },
            Num::Colours => 0.0,
            Num::TargetKb => 0.0,
            Num::RemoveBg => 0.0,
            Num::MarkSize => base.mark_size,
            Num::MarkOpacity => base.mark_opacity,
        }
    }

    pub(super) fn num_get(&self, n: Num) -> f32 {
        let e = &self.export_ui;
        match n {
            Num::Brush => self.tools.brush,
            Num::Quality => e.quality,
            Num::SizeValue => e.size_value,
            Num::Colours => e.colours,
            Num::TargetKb => e.target_kb,
            Num::RemoveBg => e.remove_bg,
            Num::MarkSize => e.mark_size,
            Num::MarkOpacity => e.mark_opacity,
        }
    }

    pub(super) fn num_set(&mut self, n: Num, v: f32) {
        let (lo, hi) = self.num_range(n);
        let v = v.clamp(lo, hi);
        let v = if hi - lo <= 20.0 { (v * 10.0).round() / 10.0 } else { v.round() };
        let e = &mut self.export_ui;
        match n {
            Num::Brush => self.tools.brush = v,
            Num::Quality => e.quality = v,
            Num::SizeValue => e.size_value = v,
            Num::Colours => e.colours = if v < 2.0 { 0.0 } else { v },
            Num::TargetKb => e.target_kb = v,
            Num::RemoveBg => e.remove_bg = v,
            Num::MarkSize => e.mark_size = v,
            Num::MarkOpacity => e.mark_opacity = v,
        }
        self.update_title();
        invalidate(self.hwnd);
    }

    fn num_text(&self, n: Num) -> String {
        let v = self.num_get(n);
        match n {
            Num::Brush => format!("{v:.1}%"),
            Num::Colours => if v < 2.0 { "off".into() } else { format!("{v:.0}") },
            Num::TargetKb | Num::RemoveBg => if v < 1.0 { "off".into() } else { format!("{v:.0}") },
            Num::MarkOpacity | Num::MarkSize => format!("{v:.0}%"),
            _ => format!("{v:.0}"),
        }
    }

    fn pick_label(&self, p: Pick) -> &'static str {
        match p {
            Pick::Format => "Format",
            Pick::SizeMode => "Resize",
            Pick::Algo => "Resample with",
            Pick::Dither => "Dither",
            Pick::KeepExif => "Keep EXIF (JPEG)",
            Pick::MarkOn => "Watermark",
            Pick::MarkPlace => "Watermark place",
        }
    }

    fn pick_text(&self, p: Pick) -> String {
        let e = &self.export_ui;
        let onoff = |b: bool| if b { "on" } else { "off" }.to_string();
        match p {
            Pick::Format => e.format.label().to_string(),
            Pick::SizeMode => match e.size_kind {
                SizeKind::Original => "off (original size)",
                SizeKind::Percent => "by percent",
                SizeKind::LongEdge => "to a long edge",
                SizeKind::Width => "to a width",
                SizeKind::Height => "to a height",
            }
            .to_string(),
            Pick::Algo => e.algo.label().split(" (").next().unwrap_or("").to_string(),
            Pick::Dither => onoff(e.dither),
            Pick::KeepExif => onoff(e.keep_exif),
            Pick::MarkOn => onoff(e.mark_on),
            Pick::MarkPlace => e.mark_place.label().to_string(),
        }
    }

    fn pick_cycle(&mut self, p: Pick, step: i32) {
        let e = &mut self.export_ui;
        let rot = |len: usize, i: usize| ((i as i32 + step).rem_euclid(len as i32)) as usize;
        match p {
            Pick::Format => e.format = Fmt::ALL[rot(Fmt::ALL.len(), Fmt::ALL.iter().position(|f| *f == e.format).unwrap_or(0))],
            Pick::SizeMode => {
                e.size_kind = SizeKind::ALL[rot(SizeKind::ALL.len(), SizeKind::ALL.iter().position(|f| *f == e.size_kind).unwrap_or(0))];
                // Each kind has its own sensible number.
                e.size_value = if e.size_kind == SizeKind::Percent { 50.0 } else { 2000.0 };
            }
            Pick::Algo => e.algo = Algo::ALL[rot(Algo::ALL.len(), Algo::ALL.iter().position(|a| *a == e.algo).unwrap_or(0))],
            Pick::Dither => e.dither = !e.dither,
            Pick::KeepExif => e.keep_exif = !e.keep_exif,
            Pick::MarkOn => e.mark_on = !e.mark_on,
            Pick::MarkPlace => e.mark_place = Place::ALL[rot(Place::ALL.len(), Place::ALL.iter().position(|a| *a == e.mark_place).unwrap_or(0))],
        }
        self.update_title();
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ mouse

    pub(super) fn item_value_from_x(&mut self, i: usize, x: f32) {
        let l = self.layout();
        let Some(it) = l.items.get(i).copied() else { return };
        let What::Num(n) = it.what else { return };
        let t = ((x - it.track[0]) / (it.track[2] - it.track[0]).max(1.0)).clamp(0.0, 1.0);
        let (lo, hi) = self.num_range(n);
        let mut v = lo + (hi - lo) * t;
        let def = self.num_default(n);
        if (v - def).abs() < (hi - lo) * 0.012 {
            v = def;
        }
        self.num_set(n, v);
    }

    /// A left press on an item. Returns the item to keep dragging, if it is a slider.
    pub(super) fn item_press(&mut self, i: usize, x: f32, y: f32) -> Option<usize> {
        let l = self.layout();
        let Some(it) = l.items.get(i).copied() else { return None };
        match it.what {
            What::Num(_) => {
                self.item_value_from_x(i, x);
                return Some(i);
            }
            What::Pick(p) => self.pick_cycle(p, 1),
            What::Btn(b) => self.do_button(b),
            What::Preset(k) => self.apply_preset(k),
            What::Tools => {
                let w = (it.rect[2] - it.rect[0]) / TOOL_KINDS.len() as f32;
                let k = (((x - it.rect[0]) / w) as usize).min(TOOL_KINDS.len() - 1);
                let _ = y;
                self.set_heal_tool(TOOL_KINDS[k].0);
            }
        }
        invalidate(self.hwnd);
        None
    }

    /// Wheel over an item: a slider moves, a choice cycles. True if used.
    pub(super) fn item_wheel(&mut self, i: usize, delta: i32) -> bool {
        let l = self.layout();
        let Some(it) = l.items.get(i).copied() else { return false };
        match it.what {
            What::Num(n) => {
                let (lo, hi) = self.num_range(n);
                let step = ((hi - lo) / 100.0).max(if hi - lo <= 20.0 { 0.1 } else { 1.0 });
                self.num_set(n, self.num_get(n) + if delta > 0 { step } else { -step });
                true
            }
            What::Pick(p) => {
                self.pick_cycle(p, if delta > 0 { -1 } else { 1 });
                true
            }
            _ => false,
        }
    }

    /// Right click: a slider goes back to its default, a preset is deleted, a choice cycles backwards.
    pub(super) fn item_right(&mut self, i: usize) {
        let l = self.layout();
        let Some(it) = l.items.get(i).copied() else { return };
        match it.what {
            What::Num(n) => self.num_set(n, self.num_default(n)),
            What::Pick(p) => self.pick_cycle(p, -1),
            What::Preset(k) => self.delete_preset(k),
            _ => {}
        }
        invalidate(self.hwnd);
    }

    // ------------------------------------------------------------------------------------------ actions

    fn do_button(&mut self, b: Btn) {
        match b {
            Btn::SaveCopy => self.save_copy_dialog(),
            Btn::ResetExport => {
                self.export_ui = ExportUi::from_settings(&self.settings);
                self.say("Export options back to the settings");
            }
            Btn::SavePreset => self.save_preset_dialog(),
            Btn::LutChoose => self.choose_lut(),
            Btn::LutClear => {
                if self.recipe.lut.is_some() {
                    self.snapshot();
                    self.recipe.lut = None;
                    self.changed();
                }
            }
            Btn::CropTool => self.command(chrome::C_CROP),
            Btn::AutoCrop => self.auto_crop(),
            Btn::ClearCrop => self.command(chrome::C_CLEARCROP),
            Btn::ClearStrokes => {
                if !self.recipe.retouch.is_empty() {
                    self.snapshot();
                    self.recipe.retouch.clear();
                    self.changed();
                    self.say("Heal strokes cleared (Ctrl+Z brings them back)");
                }
            }
        }
    }

    pub(super) fn reload_presets(&mut self) {
        self.presets = preset::list();
        self.update_title();
    }

    fn apply_preset(&mut self, k: usize) {
        let Some(p) = self.presets.get(k).cloned() else { return };
        match preset::load(&p.path) {
            Ok(r) => {
                self.snapshot();
                self.recipe = preset::apply(&self.recipe, &r);
                self.changed();
                self.say(&format!("Preset \"{}\" applied (Ctrl+Z undoes it)", p.name));
            }
            Err(e) => self.say(&format!("Can't read the preset: {e}")),
        }
    }

    fn delete_preset(&mut self, k: usize) {
        let Some(p): Option<Preset> = self.presets.get(k).cloned() else { return };
        if nv_win::shell::confirm(self.hwnd, &format!("Delete the preset \"{}\"?", p.name)) {
            let _ = std::fs::remove_file(&p.path);
            self.reload_presets();
            self.say("Preset deleted");
        }
    }

    fn save_preset_dialog(&mut self) {
        if self.recipe.is_identity() {
            self.say("Nothing to save: adjust the picture first");
            return;
        }
        let dir = preset::dir();
        let _ = std::fs::create_dir_all(&dir);
        let Some(path) = nv_win::shell::save_dialog(self.hwnd, "Save the look as a preset", "NuEditor presets (*.nupreset)\0*.nupreset\0\0", "My look", Some(&dir), "nupreset") else { return };
        // A preset always lives in the presets folder, whatever folder the dialog was in: the name is what the user chose.
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "My look".into());
        match preset::save(&name, &self.recipe) {
            Ok(_) => {
                self.reload_presets();
                self.say(&format!("Preset \"{name}\" saved"));
            }
            Err(e) => self.say(&format!("Couldn't save the preset: {e}")),
        }
    }

    fn choose_lut(&mut self) {
        let start = self.recipe.lut.as_deref().map(PathBuf::from).and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let Some(path) = nv_win::shell::open_dialog(self.hwnd, "Choose a LUT (.cube)", "LUT files (*.cube)\0*.cube\0\0", start.as_deref()) else { return };
        let text = path.display().to_string();
        match nv_editor::lut::load(&text) {
            Ok(l) => {
                self.snapshot();
                self.recipe.lut = Some(text);
                self.changed();
                let name = if l.title.is_empty() { flow::file_name(&path) } else { l.title.clone() };
                self.say(&format!("LUT: {name} ({0}{1})", l.size, if l.is_3d { "³" } else { " entries" }));
            }
            Err(e) => self.say(&format!("Not a usable LUT: {e}")),
        }
    }

    /// "Save copy…": the Export section's options through the export pipeline, to a file the user names.
    pub(super) fn save_copy_dialog(&mut self) {
        let Some(full) = &self.full else { return };
        let export = match self.export_ui.to_export(&self.path, &self.settings.editor_watermark) {
            Ok(e) => e,
            Err(msg) => {
                self.say(&msg);
                return;
            }
        };
        let stem = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
        let suffix = if self.settings.editor_suffix.trim().is_empty() { "_edit" } else { &self.settings.editor_suffix };
        let ext = export.format.ext();
        let default = format!("{stem}{suffix}.{ext}");
        let filter = "JPEG (*.jpg)\0*.jpg\0PNG (*.png)\0*.png\0WebP lossless (*.webp)\0*.webp\0BMP (*.bmp)\0*.bmp\0\0";
        let Some(dest) = nv_win::shell::save_dialog(self.hwnd, "Save a copy", filter, &default, self.path.parent(), ext) else { return };
        // The extension the user typed decides the format.
        let mut export = export;
        if let Some(f) = Fmt::from_path(&dest) {
            let mut ui = self.export_ui.clone();
            ui.format = f;
            if let Ok(e) = ui.to_export(&self.path, &self.settings.editor_watermark) {
                export = e;
            }
        }
        let f = Full { path: &self.path, pixels: full };
        match flow::save_copy(&f, &self.recipe, &export, &dest) {
            Ok(s) => self.say(&s.message),
            Err(e) => flow::tell(self.hwnd, &format!("Couldn't save the copy: {e}")),
        }
    }

    // ------------------------------------------------------------------------------------------ drawing

    /// Everything the drawing of the items needs, worked out before the renderer is borrowed.
    pub(super) fn item_draws(&mut self, l: &Layout) -> Vec<ItemDraw> {
        let d = self.dpi;
        let px = |n: f32| (n * d).round() as i32;
        let mut out = Vec::with_capacity(l.items.len());
        for it in &l.items {
            let rect = it.rect;
            let kind = match it.what {
                What::Num(n) => {
                    let (lo, hi) = self.num_range(n);
                    let (v, def) = (self.num_get(n), self.num_default(n));
                    DrawKind::Num { label: self.num_label(n), text: self.num_text(n), t: (v - lo) / (hi - lo).max(1e-6), dt: (def - lo) / (hi - lo).max(1e-6), changed: (v - def).abs() > 1e-4, track: it.track }
                }
                What::Pick(p) => DrawKind::Pick { label: self.pick_label(p), value: self.pick_text(p) },
                What::Btn(b) => DrawKind::Btn {
                    label: match b {
                        Btn::SaveCopy => "Save copy…".to_string(),
                        Btn::ResetExport => "Reset these options".into(),
                        Btn::SavePreset => "Save this look as a preset…".into(),
                        Btn::LutChoose => match self.recipe.lut.clone() {
                            Some(p) => format!("LUT: {}", self.text.fit(&flow::file_name(Path::new(&p)), px(12.0), (rect[2] - rect[0] - 60.0 * d) as i32)),
                            None => "Choose a LUT (.cube)…".into(),
                        },
                        Btn::LutClear => "Remove the LUT".into(),
                        Btn::CropTool => if self.tools.mode == Mode::Crop { "Crop: drag a frame · Enter applies".into() } else { "Crop…  (drag a frame on the picture)".into() },
                        Btn::AutoCrop => "Crop to the picture (no empty corners)".into(),
                        Btn::ClearCrop => "Remove the crop".into(),
                        Btn::ClearStrokes => format!("Clear {} heal stroke{}", self.recipe.retouch.len(), if self.recipe.retouch.len() == 1 { "" } else { "s" }),
                    },
                },
                What::Preset(k) => {
                    let name = self.presets.get(k).map(|p| p.name.clone()).unwrap_or_default();
                    DrawKind::Preset { name: self.text.fit(&name, px(12.0), (rect[2] - rect[0] - 150.0 * d) as i32) }
                }
                What::Tools => DrawKind::Tools { active: TOOL_KINDS.iter().position(|(k, _, _)| self.tools.mode == Mode::Heal(*k)) },
            };
            out.push(ItemDraw { rect, kind });
        }
        out
    }
}

pub enum DrawKind {
    Num { label: String, text: String, t: f32, dt: f32, changed: bool, track: [f32; 4] },
    Pick { label: &'static str, value: String },
    Btn { label: String },
    Preset { name: String },
    Tools { active: Option<usize> },
}

pub struct ItemDraw {
    pub rect: [f32; 4],
    pub kind: DrawKind,
}

/// Draw the items in the style of the slider rows.
pub fn draw_item_list(r: &mut Renderer, text: &mut Text, items: &[ItemDraw], hot: Option<usize>, d: f32) {
    let px = |n: f32| (n * d).round() as i32;
    let (text_c, dim_c, accent) = (rgb(pal::TEXT, 1.0), rgb(pal::DIM, 1.0), rgb(pal::ACCENT, 1.0));
    for (i, it) in items.iter().enumerate() {
        let hot = hot == Some(i);
        let rect = it.rect;
        match &it.kind {
            DrawKind::Num { label, text: value, t, dt, changed, track } => {
                text.draw(r, label, px(12.0), rect[0] + 14.0 * d, rect[1] + 5.0 * d, text_c);
                text.draw_right(r, value, px(12.0), rect[2] - 14.0 * d, rect[1] + 5.0 * d, if *changed { accent } else { dim_c });
                let _ = r.fill_rect(*track, rgb(0x2E3238, 1.0));
                let (a, b) = (track[0] + (track[2] - track[0]) * dt.min(*t), track[0] + (track[2] - track[0]) * dt.max(*t));
                let _ = r.fill_rect([a, track[1], b, track[3]], accent);
                let kx = track[0] + (track[2] - track[0]) * t;
                let _ = r.fill_rect([kx - 5.0 * d, track[1] - 6.0 * d, kx + 5.0 * d, track[3] + 6.0 * d], rgb(0xE8E8E8, 1.0));
            }
            DrawKind::Pick { label, value } => {
                if hot {
                    let _ = r.fill_rect(rect, rgb(pal::TILE_HOVER, 1.0));
                }
                text.draw(r, label, px(12.0), rect[0] + 14.0 * d, rect[1] + 6.0 * d, text_c);
                text.draw_right(r, value, px(12.0), rect[2] - 14.0 * d, rect[1] + 6.0 * d, accent);
            }
            DrawKind::Btn { label } => {
                let fill = if hot { rgb(pal::TILE_HOVER, 1.0) } else { rgb(pal::TILE, 1.0) };
                let _ = r.fill_rect(rect, fill);
                text.draw_centered(r, label, px(12.0), (rect[0] + rect[2]) / 2.0, rect[1] + 8.0 * d, text_c);
            }
            DrawKind::Preset { name } => {
                if hot {
                    let _ = r.fill_rect(rect, rgb(pal::TILE_HOVER, 1.0));
                }
                text.draw(r, name, px(12.0), rect[0] + 14.0 * d, rect[1] + 6.0 * d, text_c);
                text.draw_right(r, "click applies · right-click deletes", px(10.0), rect[2] - 14.0 * d, rect[1] + 8.0 * d, dim_c);
            }
            DrawKind::Tools { active } => {
                let w = (rect[2] - rect[0]) / TOOL_KINDS.len() as f32;
                for (k, (_, name, key)) in TOOL_KINDS.iter().enumerate() {
                    let cell = [rect[0] + w * k as f32 + 1.0, rect[1], rect[0] + w * (k + 1) as f32 - 1.0, rect[3]];
                    let is_active = *active == Some(k);
                    let fill = if is_active { accent } else { rgb(pal::TILE, 1.0) };
                    let _ = r.fill_rect(cell, fill);
                    text.draw_centered(r, name, px(10.0), (cell[0] + cell[2]) / 2.0, cell[1] + 4.0 * d, text_c);
                    text.draw_centered(r, key, px(9.0), (cell[0] + cell[2]) / 2.0, cell[1] + 17.0 * d, if is_active { text_c } else { dim_c });
                }
            }
        }
    }
}
