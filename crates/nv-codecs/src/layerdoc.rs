//! Pictures made of layers (PSD, GIMP XCF, OpenRaster ...): the layers as they are stored, and the one function that flattens
//! them again for whatever visibility / opacity the user chose in the layer panel.
//!
//! Pure code, no window. The state the panel edits (names, tree, filter, hover) lives in `nv_core::layers`; it hands this module
//! one `Eff` per layer (on / off and the opacity already multiplied through the groups above it).
//!
//! What is *not* done, and said so in the UI: non-separable blend modes (hue, saturation, colour, luminosity) and "dissolve" are
//! composited as normal, clipping masks are ignored, layer effects (drop shadows ...) are not drawn, and a group is not an
//! isolated buffer (its opacity multiplies its children one by one, which differs from Photoshop only when children overlap
//! inside a semi-transparent group).

use crate::Image;

/// How a layer is mixed with what is below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    LinearDodge,
    LinearBurn,
    HardLight,
    SoftLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    GrainExtract,
    GrainMerge,
}

impl Blend {
    /// The name the panel shows.
    pub fn name(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Multiply => "Multiply",
            Blend::Screen => "Screen",
            Blend::Overlay => "Overlay",
            Blend::Darken => "Darken",
            Blend::Lighten => "Lighten",
            Blend::ColorDodge => "Colour dodge",
            Blend::ColorBurn => "Colour burn",
            Blend::LinearDodge => "Add",
            Blend::LinearBurn => "Linear burn",
            Blend::HardLight => "Hard light",
            Blend::SoftLight => "Soft light",
            Blend::VividLight => "Vivid light",
            Blend::LinearLight => "Linear light",
            Blend::PinLight => "Pin light",
            Blend::HardMix => "Hard mix",
            Blend::Difference => "Difference",
            Blend::Exclusion => "Exclusion",
            Blend::Subtract => "Subtract",
            Blend::Divide => "Divide",
            Blend::GrainExtract => "Grain extract",
            Blend::GrainMerge => "Grain merge",
        }
    }

    /// One colour channel: `d` below, `s` above, both 0..1.
    pub fn channel(self, d: f32, s: f32) -> f32 {
        match self {
            Blend::Normal => s,
            Blend::Multiply => d * s,
            Blend::Screen => 1.0 - (1.0 - d) * (1.0 - s),
            Blend::Overlay => {
                if d < 0.5 {
                    2.0 * d * s
                } else {
                    1.0 - 2.0 * (1.0 - d) * (1.0 - s)
                }
            }
            Blend::Darken => d.min(s),
            Blend::Lighten => d.max(s),
            Blend::ColorDodge => {
                if s >= 1.0 {
                    1.0
                } else {
                    (d / (1.0 - s)).min(1.0)
                }
            }
            Blend::ColorBurn => {
                if s <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - d) / s).min(1.0)
                }
            }
            Blend::LinearDodge => (d + s).min(1.0),
            Blend::LinearBurn => (d + s - 1.0).max(0.0),
            Blend::HardLight => {
                if s < 0.5 {
                    2.0 * d * s
                } else {
                    1.0 - 2.0 * (1.0 - d) * (1.0 - s)
                }
            }
            Blend::SoftLight => {
                let r = 1.0 - (1.0 - d) * (1.0 - s);
                (1.0 - d) * d * s + d * r
            }
            Blend::VividLight => {
                if s < 0.5 {
                    Blend::ColorBurn.channel(d, 2.0 * s)
                } else {
                    Blend::ColorDodge.channel(d, 2.0 * (s - 0.5))
                }
            }
            Blend::LinearLight => (d + 2.0 * s - 1.0).clamp(0.0, 1.0),
            Blend::PinLight => {
                if s < 0.5 {
                    d.min(2.0 * s)
                } else {
                    d.max(2.0 * (s - 0.5))
                }
            }
            Blend::HardMix => {
                if Blend::VividLight.channel(d, s) < 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            Blend::Difference => (d - s).abs(),
            Blend::Exclusion => d + s - 2.0 * d * s,
            Blend::Subtract => (d - s).max(0.0),
            Blend::Divide => {
                if s <= 0.0 {
                    1.0
                } else {
                    (d / s).min(1.0)
                }
            }
            Blend::GrainExtract => (d - s + 0.5).clamp(0.0, 1.0),
            Blend::GrainMerge => (d + s - 0.5).clamp(0.0, 1.0),
        }
    }

    /// GIMP's layer mode numbers (XCF property 7).
    pub fn from_xcf(mode: u32) -> Blend {
        match mode {
            3 => Blend::Multiply,
            4 => Blend::Screen,
            5 | 23 => Blend::Overlay,
            6 => Blend::Difference,
            7 => Blend::LinearDodge,
            8 => Blend::Subtract,
            9 => Blend::Darken,
            10 => Blend::Lighten,
            15 => Blend::Divide,
            16 => Blend::ColorDodge,
            17 => Blend::ColorBurn,
            18 => Blend::HardLight,
            19 => Blend::SoftLight,
            20 => Blend::GrainExtract,
            21 => Blend::GrainMerge,
            _ => Blend::Normal,
        }
    }

    /// OpenRaster's `composite-op` ("svg:multiply", "svg:src-over" ...).
    pub fn from_ora(op: &str) -> Blend {
        match op.trim().trim_start_matches("svg:") {
            "multiply" => Blend::Multiply,
            "screen" => Blend::Screen,
            "overlay" => Blend::Overlay,
            "darken" => Blend::Darken,
            "lighten" => Blend::Lighten,
            "color-dodge" => Blend::ColorDodge,
            "color-burn" => Blend::ColorBurn,
            "hard-light" => Blend::HardLight,
            "soft-light" => Blend::SoftLight,
            "difference" => Blend::Difference,
            "exclusion" => Blend::Exclusion,
            "plus" | "add" => Blend::LinearDodge,
            _ => Blend::Normal,
        }
    }

    /// Photoshop's four-letter blend keys ("mul ", "scrn", "over" ...).
    pub fn from_psd(key: &[u8; 4]) -> Blend {
        match key {
            b"mul " => Blend::Multiply,
            b"scrn" => Blend::Screen,
            b"over" => Blend::Overlay,
            b"dark" => Blend::Darken,
            b"lite" => Blend::Lighten,
            b"div " => Blend::ColorDodge,
            b"idiv" => Blend::ColorBurn,
            b"lddg" => Blend::LinearDodge,
            b"lbrn" => Blend::LinearBurn,
            b"hLit" => Blend::HardLight,
            b"sLit" => Blend::SoftLight,
            b"vLit" => Blend::VividLight,
            b"lLit" => Blend::LinearLight,
            b"pLit" => Blend::PinLight,
            b"hMix" => Blend::HardMix,
            b"diff" => Blend::Difference,
            b"smud" => Blend::Exclusion,
            b"fsub" => Blend::Subtract,
            b"fdiv" => Blend::Divide,
            _ => Blend::Normal,
        }
    }

    /// Blend modes of the `psd` crate (it does not export the type: its Debug name is the variant name).
    fn from_psd_crate(m: impl std::fmt::Debug) -> Blend {
        match format!("{m:?}").as_str() {
            "Multiply" => Blend::Multiply,
            "Screen" => Blend::Screen,
            "Overlay" => Blend::Overlay,
            "Darken" => Blend::Darken,
            "Lighten" => Blend::Lighten,
            "ColorDodge" => Blend::ColorDodge,
            "ColorBurn" => Blend::ColorBurn,
            "LinearDodge" => Blend::LinearDodge,
            "LinearBurn" => Blend::LinearBurn,
            "HardLight" => Blend::HardLight,
            "SoftLight" => Blend::SoftLight,
            "VividLight" => Blend::VividLight,
            "LinearLight" => Blend::LinearLight,
            "PinLight" => Blend::PinLight,
            "HardMix" => Blend::HardMix,
            "Difference" => Blend::Difference,
            "Exclusion" => Blend::Exclusion,
            "Subtract" => Blend::Subtract,
            "Divide" => Blend::Divide,
            _ => Blend::Normal,
        }
    }
}

/// One layer or group, as stored.
#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    /// The group this one sits in (an index into `LayerDoc::layers`).
    pub parent: Option<usize>,
    pub group: bool,
    /// As saved in the file.
    pub visible: bool,
    pub opacity: f32,
    pub mode: Blend,
    /// Where the layer's top-left corner is on the canvas.
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// `w * h` straight (not premultiplied) RGBA. Empty for a group, and for a layer that was left unread (see `loaded`).
    pub rgba: Vec<u8>,
    /// False when the reader skipped this layer's pixels (a hidden layer of a file read for plain viewing).
    pub loaded: bool,
}

impl Layer {
    pub fn bounds(&self) -> [i32; 4] {
        [self.x, self.y, self.x + self.w as i32, self.y + self.h as i32]
    }
}

/// A whole layered picture. `layers` run from the top of the stack to the bottom (the order of a layer panel); a group is
/// listed before its children.
#[derive(Clone, Debug)]
pub struct LayerDoc {
    pub width: u32,
    pub height: u32,
    /// "PSD", "XCF", "OpenRaster".
    pub format: &'static str,
    /// Shown under the panel: what this reader does not do.
    pub note: String,
    /// The profile of the file (PSD), applied when the layers are flattened for display.
    pub icc: Option<Vec<u8>>,
    pub layers: Vec<Layer>,
}

/// What one layer contributes to the picture right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Eff {
    pub on: bool,
    pub opacity: f32,
}

impl LayerDoc {
    /// The layers as the file has them: hidden ones off, opacity multiplied through the groups above.
    pub fn default_eff(&self) -> Vec<Eff> {
        let mut out: Vec<Eff> = Vec::with_capacity(self.layers.len());
        for (i, l) in self.layers.iter().enumerate() {
            let up = l.parent.filter(|&p| p < i).map(|p| out[p]).unwrap_or(Eff { on: true, opacity: 1.0 });
            out.push(Eff { on: up.on && l.visible, opacity: up.opacity * l.opacity });
        }
        out
    }

    /// Bytes of layer pixels held.
    pub fn bytes(&self) -> usize {
        self.layers.iter().map(|l| l.rgba.len()).sum()
    }
}

/// Flatten the layers that are on, bottom to top, into straight RGBA of the canvas size. `eff` has one entry per layer; a layer
/// whose pixels were not read (`loaded == false`) is skipped. Row bands are done on all cores.
pub fn compose(doc: &LayerDoc, eff: &[Eff]) -> Vec<u8> {
    let (w, h) = (doc.width as usize, doc.height as usize);
    let mut out = vec![0u8; w * h * 4];
    if w == 0 || h == 0 {
        return out;
    }
    // Bottom to top, with everything that cannot show left out.
    let order: Vec<usize> = (0..doc.layers.len()).rev().filter(|&i| {
        let l = &doc.layers[i];
        let e = eff.get(i).copied().unwrap_or(Eff { on: true, opacity: 1.0 });
        e.on && e.opacity > 0.0 && !l.group && l.loaded && l.w > 0 && l.h > 0 && l.rgba.len() >= l.w as usize * l.h as usize * 4
    }).collect();
    if order.is_empty() {
        return out;
    }
    let band = 32usize;
    let bands = h.div_ceil(band);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 8).min(bands);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let chunks: Vec<std::sync::Mutex<&mut [u8]>> = out.chunks_mut(band * w * 4).map(std::sync::Mutex::new).collect();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut canvas = vec![0f32; band * w * 4];
                loop {
                    let b = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if b >= bands {
                        break;
                    }
                    let (y0, y1) = (b * band, ((b + 1) * band).min(h));
                    let rows = y1 - y0;
                    let cv = &mut canvas[..rows * w * 4];
                    cv.fill(0.0);
                    for &i in &order {
                        paint_band(&doc.layers[i], eff.get(i).map(|e| e.opacity).unwrap_or(1.0), cv, w, y0, y1);
                    }
                    let mut dst = chunks[b].lock().unwrap();
                    for (o, c) in dst.chunks_exact_mut(4).zip(cv.chunks_exact(4)) {
                        for k in 0..4 {
                            o[k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                }
            });
        }
    });
    drop(chunks);
    out
}

/// One layer onto rows `y0..y1` of the float canvas (straight colour + alpha per pixel).
fn paint_band(l: &Layer, opacity: f32, cv: &mut [f32], w: usize, y0: usize, y1: usize) {
    let (ly0, ly1) = (l.y.max(y0 as i32), (l.y + l.h as i32).min(y1 as i32));
    let (lx0, lx1) = (l.x.max(0), (l.x + l.w as i32).min(w as i32));
    if ly0 >= ly1 || lx0 >= lx1 {
        return;
    }
    let lw = l.w as usize;
    for cy in ly0..ly1 {
        let sy = (cy - l.y) as usize;
        for cx in lx0..lx1 {
            let sx = (cx - l.x) as usize;
            let s = &l.rgba[(sy * lw + sx) * 4..(sy * lw + sx) * 4 + 4];
            if s[3] == 0 {
                continue;
            }
            let sa = s[3] as f32 / 255.0 * opacity;
            let at = ((cy as usize - y0) * w + cx as usize) * 4;
            let d = &mut cv[at..at + 4];
            let da = d[3];
            if l.mode == Blend::Normal && (sa >= 0.999 || da == 0.0) {
                if sa >= 0.999 {
                    d[0] = s[0] as f32 / 255.0;
                    d[1] = s[1] as f32 / 255.0;
                    d[2] = s[2] as f32 / 255.0;
                    d[3] = 1.0;
                    continue;
                }
                d[0] = s[0] as f32 / 255.0;
                d[1] = s[1] as f32 / 255.0;
                d[2] = s[2] as f32 / 255.0;
                d[3] = sa;
                continue;
            }
            let oa = sa + da * (1.0 - sa);
            for c in 0..3 {
                let sc = s[c] as f32 / 255.0;
                let bl = if da > 0.0 && l.mode != Blend::Normal { l.mode.channel(d[c], sc) } else { sc };
                d[c] = ((1.0 - sa) * da * d[c] + sa * ((1.0 - da) * sc + da * bl)) / oa;
            }
            d[3] = oa;
        }
    }
}

/// The picture as `Image` (straight RGBA, no profile: the readers' pixels are sRGB numbers).
pub fn flatten(doc: &LayerDoc, eff: &[Eff]) -> Image {
    let mut img = Image::new(doc.width, doc.height, compose(doc, eff));
    img.profile_assumable = true;
    img.icc = doc.icc.clone();
    img
}

// ---- which files have layers ---------------------------------------------------------------------------------------------------

/// Extensions `read` understands.
pub const LAYERED_EXTS: &[&str] = &["psd", "xcf", "ora"];

pub fn is_layered_ext(ext: &str) -> bool {
    LAYERED_EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Read every layer of a file (for the layer panel). `Err` says why this file has none we can show.
pub fn read(path: &std::path::Path) -> Result<LayerDoc, String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "xcf" => crate::designfmt::xcf_layers(&crate::progress::read_file(path)?, false),
        "ora" => ora(path),
        "psd" => psd_layers(&crate::progress::read_file(path)?),
        _ => Err("this file type has no layers we can read".into()),
    }
}

// ---- OpenRaster ----------------------------------------------------------------------------------------------------------------

fn ora(path: &std::path::Path) -> Result<LayerDoc, String> {
    let mut zip = crate::docthumb::Zip::open(path)?;
    let entries = zip.entries.clone();
    let find = |name: &str| entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)).cloned();
    let stack = find("stack.xml").ok_or("ora: no stack.xml in this file")?;
    let xml = String::from_utf8_lossy(&zip.read(&stack, 16 << 20)?).into_owned();
    let doc = roxmltree::Document::parse(&xml).map_err(|e| format!("ora: stack.xml: {e}"))?;
    let root = doc.root_element();
    let attr_f = |n: roxmltree::Node, k: &str, d: f32| n.attribute(k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
    let (w, h) = (attr_f(root, "w", 0.0) as u32, attr_f(root, "h", 0.0) as u32);
    crate::check_size(w as u64, h as u64)?;
    let top = root.children().find(|c| c.has_tag_name("stack")).ok_or("ora: no stack in stack.xml")?;
    let mut layers: Vec<Layer> = Vec::new();
    let mut budget = 1usize << 30;
    fn walk(node: roxmltree::Node, parent: Option<usize>, ox: i32, oy: i32, zip: &mut crate::docthumb::Zip, entries: &[crate::docthumb::Entry], layers: &mut Vec<Layer>, budget: &mut usize, depth: u32) -> Result<(), String> {
        if depth > 32 {
            return Ok(());
        }
        let num = |n: roxmltree::Node, k: &str| n.attribute(k).and_then(|v| v.trim().parse::<f32>().ok());
        for c in node.children().filter(|c| c.is_element()) {
            let visible = c.attribute("visibility").map(|v| !v.eq_ignore_ascii_case("hidden")).unwrap_or(true);
            let opacity = num(c, "opacity").unwrap_or(1.0).clamp(0.0, 1.0);
            let mode = Blend::from_ora(c.attribute("composite-op").unwrap_or(""));
            let (x, y) = (num(c, "x").unwrap_or(0.0) as i32 + ox, num(c, "y").unwrap_or(0.0) as i32 + oy);
            let name = c.attribute("name").unwrap_or("").to_string();
            if c.has_tag_name("stack") {
                let idx = layers.len();
                layers.push(Layer { name: if name.is_empty() { "Group".into() } else { name }, parent, group: true, visible, opacity, mode, x, y, w: 0, h: 0, rgba: Vec::new(), loaded: true });
                walk(c, Some(idx), x, y, zip, entries, layers, budget, depth + 1)?;
            } else if c.has_tag_name("layer") {
                let src = c.attribute("src").unwrap_or("");
                let entry = entries.iter().find(|e| e.name.eq_ignore_ascii_case(src));
                let name = if name.is_empty() { src.to_string() } else { name };
                let mut layer = Layer { name, parent, group: false, visible, opacity, mode, x, y, w: 0, h: 0, rgba: Vec::new(), loaded: true };
                if let Some(e) = entry {
                    let bytes = zip.read(e, 256 << 20)?;
                    if let Ok(img) = crate::decode_image_bytes(&bytes, &crate::docthumb::ext_of(&e.name)) {
                        *budget = budget.checked_sub(img.rgba.len()).ok_or("ora: the layers need more than 1 GB")?;
                        layer.w = img.width;
                        layer.h = img.height;
                        layer.rgba = img.rgba;
                    }
                }
                layers.push(layer);
            }
        }
        Ok(())
    }
    walk(top, None, 0, 0, &mut zip, &entries, &mut layers, &mut budget, 0)?;
    if layers.is_empty() {
        return Err("ora: no layers".into());
    }
    Ok(LayerDoc { width: w, height: h, format: "OpenRaster", note: "Blend modes other than the common ones are drawn as normal.".into(), icc: None, layers })
}

// ---- Photoshop -----------------------------------------------------------------------------------------------------------------

/// The box around the pixels that are not fully transparent, and a copy of just that box. Empty layer: 0 x 0.
fn crop_to_content(rgba: &[u8], w: usize, h: usize) -> (i32, i32, u32, u32, Vec<u8>) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0usize, 0usize);
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        let first = row.chunks_exact(4).position(|p| p[3] != 0);
        if let Some(f) = first {
            let last = row.chunks_exact(4).rposition(|p| p[3] != 0).unwrap_or(f);
            x0 = x0.min(f);
            x1 = x1.max(last + 1);
            y0 = y0.min(y);
            y1 = y1.max(y + 1);
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return (0, 0, 0, 0, Vec::new());
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity(cw * ch * 4);
    for y in y0..y1 {
        out.extend_from_slice(&rgba[(y * w + x0) * 4..(y * w + x1) * 4]);
    }
    (x0 as i32, y0 as i32, cw as u32, ch as u32, out)
}

/// What a layer record of a PSD says that the `psd` crate gets wrong or drops: its visibility (bit 1 of the flags is set when the
/// layer is HIDDEN; the crate reads it the other way round), and everything about a group, whose settings sit in the folder's own
/// record while the crate takes the closing divider's.
#[derive(Clone, Debug)]
struct PsdRec {
    /// Section divider type: 1 open folder, 2 closed folder, 3 end of a group.
    divider: Option<u32>,
    visible: bool,
    opacity: u8,
    key: [u8; 4],
}

fn psd_records(b: &[u8]) -> Option<Vec<PsdRec>> {
    let u32at = |i: usize| b.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize);
    let u16at = |i: usize| b.get(i..i + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as usize);
    if b.get(0..4)? != b"8BPS" || u16at(4)? != 1 {
        return None;
    }
    let mut p = 26usize;
    p += 4 + u32at(p)?; // colour mode data
    p += 4 + u32at(p)?; // image resources
    p += 4; // layer and mask information length
    p += 4; // layer info length
    let count = i16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]).unsigned_abs() as usize;
    p += 2;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        p += 16; // top, left, bottom, right
        let channels = u16at(p)?;
        p += 2 + channels * 6;
        p += 4; // blend signature
        let key: [u8; 4] = b.get(p..p + 4)?.try_into().ok()?;
        p += 4;
        let (opacity, flags) = (*b.get(p)?, *b.get(p + 2)?);
        p += 4;
        let extra_len = u32at(p)?;
        p += 4;
        let extra = b.get(p..p + extra_len)?;
        p += extra_len;
        // extra: layer mask data, blending ranges, the name, then "8BIM key length data" blocks
        let eu32 = |i: usize| extra.get(i..i + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]) as usize);
        let mut q = 4 + eu32(0)?;
        q += 4 + eu32(q)?;
        let name_len = *extra.get(q)? as usize;
        q += (1 + name_len).div_ceil(4) * 4;
        let mut divider = None;
        while let Some(sig) = extra.get(q..q + 4) {
            if sig != b"8BIM" && sig != b"8B64" {
                break;
            }
            let k = extra.get(q + 4..q + 8)?;
            let len = eu32(q + 8)?;
            if k == b"lsct" {
                divider = eu32(q + 12).map(|v| v as u32);
            }
            q += 12 + len;
        }
        out.push(PsdRec { divider, visible: flags & 2 == 0, opacity, key });
    }
    Some(out)
}

fn psd_layers(bytes: &[u8]) -> Result<LayerDoc, String> {
    if bytes.get(4..6) == Some(&[0, 2]) {
        return Err("psb: the layers of a large document (PSB) are not read; the merged picture is shown".into());
    }
    let r = std::panic::catch_unwind(|| -> Result<LayerDoc, String> {
        let psd = psd::Psd::from_bytes(bytes).map_err(|e| format!("psd: {e}"))?;
        if !matches!(psd.depth(), psd::PsdDepth::Eight) {
            return Err("psd: layers are read for 8-bit documents only (the merged picture is shown)".into());
        }
        if !matches!(psd.color_mode(), psd::ColorMode::Rgb) {
            return Err("psd: layers are read for RGB documents only (the merged picture is shown)".into());
        }
        let (w, h) = (psd.width(), psd.height());
        crate::check_size(w as u64, h as u64)?;
        let src = psd.layers();
        if src.is_empty() {
            return Err("psd: this file has a single flat picture".into());
        }
        // The records, top to bottom (the file stores them bottom to top): pixel layers and group folders, each in their own order.
        let recs: Option<Vec<PsdRec>> = psd_records(bytes).map(|mut v| {
            v.reverse();
            v
        });
        let (layer_recs, folder_recs): (Vec<PsdRec>, Vec<PsdRec>) = match &recs {
            Some(v) => (v.iter().filter(|r| r.divider.is_none() || r.divider == Some(0)).cloned().collect(), v.iter().filter(|r| matches!(r.divider, Some(1) | Some(2))).cloned().collect()),
            None => (Vec::new(), Vec::new()),
        };
        let use_recs = recs.is_some() && layer_recs.len() == src.len();
        // The crate lists the pixel layers top to bottom and keeps groups apart: put each group in front of its first layer.
        let groups = psd.groups();
        let mut out: Vec<Layer> = Vec::new();
        let mut group_at: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        fn ensure_group(id: u32, groups: &std::collections::HashMap<u32, psd::PsdGroup>, folders: &[PsdRec], out: &mut Vec<Layer>, at: &mut std::collections::HashMap<u32, usize>, depth: u32) -> Option<usize> {
            if let Some(&i) = at.get(&id) {
                return Some(i);
            }
            if depth > 32 {
                return None;
            }
            let g = groups.get(&id)?;
            let parent = g.parent_id().and_then(|p| ensure_group(p, groups, folders, out, at, depth + 1));
            // Group ids count the folders in top-to-bottom order, from 1.
            let (visible, opacity, mode) = match folders.get(id as usize - 1) {
                Some(r) => (r.visible, r.opacity as f32 / 255.0, Blend::from_psd(&r.key)),
                None => (true, 1.0, Blend::Normal),
            };
            out.push(Layer { name: g.name().to_string(), parent, group: true, visible, opacity, mode, x: 0, y: 0, w: 0, h: 0, rgba: Vec::new(), loaded: true });
            at.insert(id, out.len() - 1);
            Some(out.len() - 1)
        }
        let mut budget = 1usize << 30;
        for (n, l) in src.iter().enumerate() {
            let parent = l.parent_id().and_then(|p| ensure_group(p, groups, &folder_recs, &mut out, &mut group_at, 0));
            let (visible, opacity, mode) = if use_recs { (layer_recs[n].visible, layer_recs[n].opacity as f32 / 255.0, Blend::from_psd(&layer_recs[n].key)) } else { (!l.visible(), l.opacity() as f32 / 255.0, Blend::from_psd_crate(l.blend_mode())) };
            let (lw, lh) = (l.width() as u32, l.height() as u32);
            let mut rgba = l.rgba();
            // The crate hands over the layer laid out on the whole canvas (or on its own box): keep only what has content.
            let (mut x, mut y, mut pw, mut ph) = (0i32, 0i32, 0u32, 0u32);
            if rgba.len() == w as usize * h as usize * 4 {
                (x, y, pw, ph, rgba) = crop_to_content(&rgba, w as usize, h as usize);
            } else if rgba.len() == lw as usize * lh as usize * 4 {
                let (cx, cy, cw, ch, c) = crop_to_content(&rgba, lw as usize, lh as usize);
                (x, y, pw, ph, rgba) = (cx + l.layer_left(), cy + l.layer_top(), cw, ch, c);
            } else {
                rgba.clear();
            }
            budget = budget.checked_sub(rgba.len()).ok_or("psd: the layers need more than 1 GB")?;
            out.push(Layer { name: l.name().to_string(), parent, group: false, visible, opacity, mode, x, y, w: pw, h: ph, rgba, loaded: true });
        }
        Ok(LayerDoc { width: w, height: h, format: "PSD", note: "Layer masks, clipping masks, adjustment layers and layer effects are not applied; some blend modes are approximated.".into(), icc: crate::psdraw::icc_profile(bytes), layers: out })
    });
    match r {
        Ok(v) => v,
        Err(_) => Err("psd: the layer reader failed on this file".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        (0..w * h).flat_map(|_| rgba).collect()
    }

    fn layer(name: &str, x: i32, y: i32, w: u32, h: u32, rgba: [u8; 4]) -> Layer {
        Layer { name: name.into(), parent: None, group: false, visible: true, opacity: 1.0, mode: Blend::Normal, x, y, w, h, rgba: solid(w, h, rgba), loaded: true }
    }

    fn doc(layers: Vec<Layer>) -> LayerDoc {
        LayerDoc { width: 4, height: 4, format: "test", note: String::new(), icc: None, layers }
    }

    fn px(v: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * 4 + x) * 4;
        [v[i], v[i + 1], v[i + 2], v[i + 3]]
    }

    #[test]
    fn layers_stack_from_the_bottom_and_leave_the_rest_transparent() {
        // top first: a red 2x2 at (1,1) over a blue 4x4
        let d = doc(vec![layer("red", 1, 1, 2, 2, [255, 0, 0, 255]), layer("blue", 0, 0, 4, 4, [0, 0, 255, 255])]);
        let out = compose(&d, &d.default_eff());
        assert_eq!(px(&out, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&out, 1, 1), [255, 0, 0, 255]);
        assert_eq!(px(&out, 3, 3), [0, 0, 255, 255]);
        let only_red = compose(&d, &[Eff { on: true, opacity: 1.0 }, Eff { on: false, opacity: 1.0 }]);
        assert_eq!(px(&only_red, 0, 0), [0, 0, 0, 0]);
        assert_eq!(px(&only_red, 2, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn opacity_mixes_and_a_layer_outside_the_canvas_is_clipped() {
        let d = doc(vec![layer("white", -2, -2, 4, 4, [255, 255, 255, 255]), layer("black", 0, 0, 4, 4, [0, 0, 0, 255])]);
        let out = compose(&d, &[Eff { on: true, opacity: 0.5 }, Eff { on: true, opacity: 1.0 }]);
        let p = px(&out, 0, 0);
        assert!((p[0] as i32 - 128).abs() <= 1 && p[3] == 255, "{p:?}");
        assert_eq!(px(&out, 3, 3), [0, 0, 0, 255]); // the white layer ends at (2,2)
    }

    #[test]
    fn blend_modes_and_group_opacity() {
        let mut mult = layer("mult", 0, 0, 4, 4, [128, 128, 128, 255]);
        mult.mode = Blend::Multiply;
        let d = doc(vec![mult, layer("base", 0, 0, 4, 4, [200, 100, 50, 255])]);
        let out = compose(&d, &d.default_eff());
        let p = px(&out, 2, 2);
        assert!((p[0] as i32 - 100).abs() <= 1 && (p[1] as i32 - 50).abs() <= 1 && (p[2] as i32 - 25).abs() <= 1, "{p:?}");
        // a group at 50% over a layer: the child inherits it
        let g = Layer { name: "g".into(), parent: None, group: true, visible: true, opacity: 0.5, mode: Blend::Normal, x: 0, y: 0, w: 0, h: 0, rgba: vec![], loaded: true };
        let mut child = layer("child", 0, 0, 4, 4, [255, 255, 255, 255]);
        child.parent = Some(0);
        let d = doc(vec![g, child]);
        let eff = d.default_eff();
        assert_eq!(eff[1], Eff { on: true, opacity: 0.5 });
        let mut hidden = d.clone();
        hidden.layers[0].visible = false;
        assert!(!hidden.default_eff()[1].on);
    }

    #[test]
    fn unread_layers_are_skipped_not_drawn_as_garbage() {
        let mut a = layer("a", 0, 0, 4, 4, [9, 9, 9, 255]);
        a.loaded = false;
        a.rgba.clear();
        let d = doc(vec![a]);
        assert!(compose(&d, &d.default_eff()).iter().all(|&b| b == 0));
    }

    /// A zip with stored (not packed) entries.
    fn stored_zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
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
            out.extend(data);
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

    fn png_rgba(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().unwrap();
        wr.write_image_data(&solid(w, h, rgba)).unwrap();
        drop(wr);
        out
    }

    #[test]
    fn an_openraster_file_gives_layers_groups_offsets_and_modes() {
        let xml = r#"<?xml version="1.0"?><image w="6" h="4"><stack>
            <layer name="Top" src="data/top.png" x="1" y="1" opacity="0.5" composite-op="svg:multiply"/>
            <stack name="Group" x="2" y="0" visibility="hidden"><layer name="Inner" src="data/inner.png" x="1" y="0"/></stack>
            <layer name="Back" src="data/back.png"/></stack></image>"#;
        let zip = stored_zip(&[("mimetype", b"image/openraster".to_vec()), ("stack.xml", xml.as_bytes().to_vec()), ("data/top.png", png_rgba(2, 2, [255, 0, 0, 255])), ("data/inner.png", png_rgba(1, 1, [0, 255, 0, 255])), ("data/back.png", png_rgba(6, 4, [0, 0, 255, 255]))]);
        let path = std::env::temp_dir().join(format!("nv_layers_{}.ora", std::process::id()));
        std::fs::write(&path, zip).unwrap();
        let d = read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((d.width, d.height, d.layers.len()), (6, 4, 4));
        assert_eq!(d.layers[0].name, "Top");
        assert_eq!(d.layers[0].mode, Blend::Multiply);
        assert!((d.layers[0].opacity - 0.5).abs() < 1e-6);
        assert!(d.layers[1].group && !d.layers[1].visible);
        assert_eq!(d.layers[2].parent, Some(1));
        assert_eq!((d.layers[2].x, d.layers[2].y), (3, 0)); // group offset + its own
        assert_eq!(d.layers[3].parent, None);
        let eff = d.default_eff();
        assert!(!eff[2].on); // inside a hidden group
        let out = flatten(&d, &eff);
        assert_eq!(out.width, 6);
    }

    #[test]
    fn blend_names_and_mode_tables() {
        assert_eq!(Blend::from_ora("svg:multiply"), Blend::Multiply);
        assert_eq!(Blend::from_ora("svg:src-over"), Blend::Normal);
        assert_eq!(Blend::from_xcf(3), Blend::Multiply);
        assert_eq!(Blend::from_xcf(0), Blend::Normal);
        assert_eq!(Blend::from_psd(b"scrn"), Blend::Screen);
        assert_eq!(Blend::Screen.name(), "Screen");
        assert!((Blend::Difference.channel(0.2, 0.7) - 0.5).abs() < 1e-6);
    }
}
