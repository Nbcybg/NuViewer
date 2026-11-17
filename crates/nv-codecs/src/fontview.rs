//! Font files as a specimen sheet: `ttf` `otf` `ttc` `otc` `woff` `woff2` `eot` are read (the web kinds are first turned back into
//! plain sfnt by `webfont`), and drawn with the font itself, shaped (so Thai marks, Arabic joins and ligatures come out right) by
//! the same text engine as SVG. The sentence is the user's (Settings, up to 100 characters, default "The quick brown fox ...");
//! the sheet adds the alphabet, digits, a line for every script the font covers, and what the font says about itself (name,
//! version, designer, licence). Thumbnails are a smaller card: the big letters, the sentence and the name.
//! Type 1 fonts (`pfb` `pfa`) and Windows bitmap fonts (`fon`) have no outlines we can draw: they get a card with what the file says.

use crate::docthumb::{accent_for, fmt_bytes, render_card, Card};
use crate::Image;
use resvg::{tiny_skia, usvg};
use skrifa::{
    instance::{LocationRef, Size},
    string::StringId,
    FontRef, MetadataProvider,
};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

pub const EXTS: &[&str] = &["ttf", "otf", "ttc", "otc", "woff", "woff2", "eot", "pfb", "pfa", "fon"];

pub fn is_ext(ext: &str) -> bool {
    EXTS.contains(&ext.to_ascii_lowercase().as_str())
}

pub const DEFAULT_SAMPLE: &str = "The quick brown fox jumps over the lazy dog";
pub const MAX_SAMPLE: usize = 100;

static SAMPLE: Mutex<String> = Mutex::new(String::new());

/// The sentence a font is shown with: whitespace made single spaces, at most 100 characters, the default when empty.
pub fn clean_sample(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one.chars().take(MAX_SAMPLE).collect();
    if cut.trim().is_empty() {
        DEFAULT_SAMPLE.to_string()
    } else {
        cut
    }
}

/// Set the sentence (a setting). Fonts decoded afterwards use it; the thumbnail cache key follows `signature()`.
pub fn set_sample(s: &str) {
    *SAMPLE.lock().unwrap_or_else(|e| e.into_inner()) = clean_sample(s);
}

pub fn sample() -> String {
    let g = SAMPLE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_empty() {
        DEFAULT_SAMPLE.to_string()
    } else {
        g.clone()
    }
}

/// Part of a font thumbnail's cache key: changes with the sentence.
pub fn signature() -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in sample().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h ^ 0x1F
}

/// `None` when `ext` is not one of ours.
pub fn decode(path: &Path, ext: &str, first_only: bool) -> Option<Result<Image, String>> {
    if !is_ext(ext) {
        return None;
    }
    Some(crate::progress::read_file(path).and_then(|b| decode_bytes(&b, ext, path, first_only)))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn decode_bytes(b: &[u8], ext: &str, path: &Path, first_only: bool) -> Result<Image, String> {
    let size = b.len() as u64;
    if b.starts_with(b"%!PS-AdobeFont") || b.starts_with(b"%!FontType1") || (b.first() == Some(&0x80) && b.get(1) == Some(&1) && matches!(ext, "pfb" | "pfa")) {
        return Ok(type1_card(b, path, first_only));
    }
    if ext == "fon" && !(b.starts_with(&[0, 1, 0, 0]) || b.starts_with(b"OTTO")) {
        return Ok(fon_card(b, path, first_only));
    }
    let (sfnt, container): (std::borrow::Cow<[u8]>, &str) = if crate::webfont::is_woff2(b) {
        (crate::webfont::woff2_to_sfnt(b)?.into(), "WOFF2 web font")
    } else if crate::webfont::is_woff(b) {
        (crate::webfont::woff_to_sfnt(b)?.into(), "WOFF web font")
    } else if crate::webfont::is_eot(b) && !b.starts_with(&[0, 1, 0, 0]) {
        (crate::webfont::eot_to_sfnt(b)?.into(), "EOT embedded font")
    } else {
        (b.into(), "")
    };
    specimen(sfnt.into_owned(), container, &file_name(path), size, first_only)
}

// ---- the sheet ----------------------------------------------------------------------------------------------------------

const PAPER: u32 = 0xF6F5F0;
const INK: &str = "#1B1B1B";
const GREY: &str = "#6B6B6B";
const ACCENT: u32 = 0x2F7D6D;

/// Scripts shown on the sheet when the font covers them: (label, sample line, the characters that must be there).
const SCRIPTS: &[(&str, &str, &str)] = &[
    ("Thai", "สวัสดีครับ ภาษาไทยสวยงาม กขคงจฉชซญดตถทธนบปผพฟภมยรลวศษสหฬอฮ ก่ ก้ ก๊ ก๋ กิ กี กึ กื กุ กู", "กขคงจฉชซญดตถทธนบปผพฟภมยรลวศษสหฬอฮ"),
    ("Cyrillic", "Съешь же ещё этих мягких французских булок, да выпей чаю", "абвгдежзийклмнопрстуфхцчшщъыьэюя"),
    ("Greek", "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία · αβγδεζηθικλμνξοπρστυφχψω", "αβγδεζηθικλμνξοπρστυφχψω"),
    ("Hebrew", "דג סקרן שט בים מאוכזב ולפתע מצא לו חברה", "אבגדהוזחטיכלמנסעפצקרשת"),
    ("Arabic", "نص حكيم له سر قاطع وذو شأن عظيم مكتوب على ثوب أخضر ومغلف بجلد أزرق", "ابتثجحخدذرزسشصضطظعغفقكلمنهوي"),
    ("Devanagari", "सभी मनुष्यों को गौरव और अधिकारों के मामले में जन्मजात स्वतन्त्रता और समानता प्राप्त है", "कखगघचछजझटठडढतथदधनपफबभमयरलवशषसह"),
    ("Hiragana / Katakana", "いろはにほへと ちりぬるを わかよたれそ アイウエオ カキクケコ", "あいうえおかきくけこ"),
    ("Hangul", "다람쥐 헌 쳇바퀴에 타고파 가나다라마바사", "가나다라마바사"),
    ("CJK ideographs", "天地玄黄 宇宙洪荒 日月盈昃 辰宿列张 永字八法", "天地玄黄宇宙洪荒"),
];

pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c if (c as u32) < 32 && c != '\t' => {}
            c => o.push(c),
        }
    }
    o
}

/// The fonts the labels are set in: Segoe UI and the Windows fonts that have the scripts it lacks. Not every installed font
/// (that is slow), only these few; whatever is not found falls back to the system list.
pub(crate) fn ui_fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        for f in ["segoeui.ttf", "segoeuib.ttf", "leelawui.ttf", "leelauib.ttf", "tahoma.ttf", "arial.ttf", "msgothic.ttc", "consola.ttf", "consolab.ttf"] {
            let _ = db.load_font_file(windir.join("Fonts").join(f));
        }
        if db.is_empty() {
            db.load_system_fonts();
        }
        Arc::new(db)
    })
    .clone()
}

struct Measure<'a> {
    cm: skrifa::charmap::Charmap<'a>,
    gm: skrifa::metrics::GlyphMetrics<'a>,
}

impl<'a> Measure<'a> {
    fn new(font: &FontRef<'a>) -> Self {
        Measure { cm: font.charmap(), gm: font.glyph_metrics(Size::new(1000.0), LocationRef::default()) }
    }
    fn width(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| {
                let g = self.cm.map(c).unwrap_or_default();
                self.gm.advance_width(g).unwrap_or(0.0)
            })
            .sum::<f32>()
            * px
            / 1000.0
    }
    fn has(&self, c: char) -> bool {
        self.cm.map(c).is_some()
    }
    /// The share of the visible characters of `text` the font has a glyph for.
    fn coverage(&self, text: &str) -> f32 {
        let (mut all, mut hit) = (0u32, 0u32);
        for c in text.chars().filter(|c| !c.is_whitespace()) {
            all += 1;
            hit += self.has(c) as u32;
        }
        if all == 0 {
            0.0
        } else {
            hit as f32 / all as f32
        }
    }
    /// `text` cut into lines no wider than `max_w`: at spaces, or anywhere inside a word that does not fit (Thai has no spaces).
    fn wrap(&self, text: &str, px: f32, max_w: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut cur_w = 0.0f32;
        let space = self.width(" ", px);
        for word in text.split(' ').filter(|w| !w.is_empty()) {
            let ww = self.width(word, px);
            if !cur.is_empty() && cur_w + space + ww <= max_w {
                cur.push(' ');
                cur.push_str(word);
                cur_w += space + ww;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
            }
            if ww <= max_w {
                cur = word.to_string();
                cur_w = ww;
            } else {
                for ch in word.chars() {
                    let cw = self.width(&ch.to_string(), px);
                    if cur_w + cw > max_w && !cur.is_empty() {
                        lines.push(std::mem::take(&mut cur));
                        cur_w = 0.0;
                    }
                    cur.push(ch);
                    cur_w += cw;
                }
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            if let Some(l) = lines.last_mut() {
                l.push('…');
            }
        }
        lines
    }
}

fn name_of(font: &FontRef, id: StringId) -> Option<String> {
    let s = font.localized_strings(id).english_or_first()?.to_string();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// What the font says about itself and how it is built.
struct Info {
    family: String,
    style: String,
    full: Option<String>,
    kind: String,
    glyphs: u32,
    chars: usize,
    notes: Vec<String>,
}

fn read_info(font: &FontRef, container: &str, faces: u32, family_fallback: &str) -> Info {
    let family = name_of(font, StringId::TYPOGRAPHIC_FAMILY_NAME).or_else(|| name_of(font, StringId::FAMILY_NAME)).unwrap_or_else(|| family_fallback.to_string());
    let style = name_of(font, StringId::TYPOGRAPHIC_SUBFAMILY_NAME).or_else(|| name_of(font, StringId::SUBFAMILY_NAME)).unwrap_or_default();
    let tags: Vec<[u8; 4]> = font.table_directory.table_records().iter().map(|r| r.tag().to_be_bytes()).collect();
    let has = |t: &[u8; 4]| tags.contains(t);
    let outlines = if has(b"glyf") {
        "TrueType outlines"
    } else if has(b"CFF2") {
        "OpenType CFF2 outlines"
    } else if has(b"CFF ") {
        "OpenType CFF outlines"
    } else if has(b"CBDT") || has(b"sbix") || has(b"EBDT") {
        "bitmap glyphs"
    } else {
        "no outlines"
    };
    let kind = if container.is_empty() { outlines.to_string() } else { format!("{container}, {outlines}") };
    let m = font.metrics(Size::unscaled(), LocationRef::default());
    let mut notes = Vec::new();
    let axes: Vec<String> = font.axes().iter().map(|a| format!("{} {}–{}", a.tag(), a.min_value(), a.max_value())).collect();
    if !axes.is_empty() {
        notes.push(format!("Variable font: {}", axes.join(", ")));
    }
    if has(b"COLR") || has(b"CBDT") || has(b"sbix") || has(b"SVG ") {
        notes.push("Colour glyphs (emoji style): shown here as plain outlines where the engine cannot draw them".into());
    }
    if faces > 1 {
        notes.push(format!("A collection of {faces} fonts; this sheet shows the first"));
    }
    if m.is_monospace {
        notes.push("Monospaced".into());
    }
    let mut feats = Vec::new();
    for (t, n) in [(b"GSUB", "substitutions (GSUB)"), (b"GPOS", "positioning (GPOS)"), (b"kern", "kerning table"), (b"MATH", "maths")] {
        if has(t) {
            feats.push(n);
        }
    }
    if !feats.is_empty() {
        notes.push(format!("OpenType layout: {}", feats.join(", ")));
    }
    Info { family, style, full: name_of(font, StringId::FULL_NAME), kind, glyphs: m.glyph_count, chars: font.charmap().mappings().count(), notes }
}

fn sample_for(m: &Measure, sample: &str) -> String {
    if m.coverage(sample) >= 0.6 {
        return sample.to_string();
    }
    // A font that has none of the sentence (a Thai font and an English sentence): show it in what it does have.
    for (_, line, need) in SCRIPTS {
        if m.coverage(need) >= 0.9 {
            return line.chars().take(MAX_SAMPLE).collect();
        }
    }
    sample.to_string()
}

fn specimen(bytes: Vec<u8>, container: &str, file: &str, file_size: u64, first_only: bool) -> Result<Image, String> {
    let faces = match skrifa::raw::FileRef::new(&bytes) {
        Ok(skrifa::raw::FileRef::Collection(c)) => c.len(),
        _ => 1,
    };
    let data = Arc::new(bytes);
    // The specimen's own font list: only this font, so a missing glyph is a visible gap and a system font of the same name can't stand in.
    let mut db = usvg::fontdb::Database::new();
    let ids = db.load_font_source(usvg::fontdb::Source::Binary(data.clone()));
    let Some(&id) = ids.first() else { return Err("font: this file could not be read as a font".into()) };
    for extra in ids.iter().skip(1) {
        db.remove_face(*extra);
    }
    let family_css = db.face(id).and_then(|f| f.families.first().map(|(n, _)| n.clone())).unwrap_or_default();
    let font = FontRef::from_index(data.as_slice(), 0).map_err(|e| format!("font: {e}"))?;
    let info = read_info(&font, container, faces, &family_css);
    let m = Measure::new(&font);
    let sample = sample_for(&m, &sample());
    let met = font.metrics(Size::unscaled(), LocationRef::default());
    let upem = (met.units_per_em.max(1)) as f32;
    let (asc, desc, lead) = ((met.ascent / upem).clamp(0.6, 1.5), (-met.descent / upem).clamp(0.1, 0.8), (met.leading / upem).clamp(0.0, 0.5));
    let line_h = (asc + desc + lead).clamp(1.05, 1.9);
    let top_pad = (line_h - asc - desc) / 2.0;
    let baseline = |px: f32| (top_pad + asc) * px;

    let fam_attr = format!("&apos;{}&apos;", esc(&family_css));
    let mut spec = String::new(); // elements drawn with the font
    let mut ui = String::new(); // elements drawn with the label fonts
    let (w, h);

    if first_only {
        w = 640u32;
        h = 480u32;
        let (pw, ph) = (w as f32, h as f32);
        ui.push_str(&format!("<rect width=\"{pw}\" height=\"12\" fill=\"#{ACCENT:06X}\"/>"));
        let big = ["Aa", "Ag", "กา", "あア", "漢字", "가나", "Αα", "Аа", "אב", "أب"].iter().find(|s| s.chars().all(|c| m.has(c))).map(|s| s.to_string()).or_else(|| {
            let v: String = font.charmap().mappings().filter_map(|(c, _)| char::from_u32(c)).filter(|c| !c.is_control() && !c.is_whitespace()).take(2).collect();
            (!v.is_empty()).then_some(v)
        });
        let mut y = 40.0f32;
        if let Some(big) = big {
            let px = 200.0f32;
            let wd = m.width(&big, px);
            let px = if wd > pw - 80.0 { px * (pw - 80.0) / wd } else { px };
            spec.push_str(&format!("<text x=\"36\" y=\"{:.1}\" font-family=\"{fam_attr}\" font-size=\"{px:.1}\" fill=\"{INK}\" xml:space=\"preserve\">{}</text>", y + baseline(px), esc(&big)));
            y += line_h * px + 4.0;
        }
        let px = 30.0f32;
        let lines = m.wrap(&sample, px, pw - 72.0, 3);
        for l in &lines {
            spec.push_str(&format!("<text x=\"36\" y=\"{:.1}\" font-family=\"{fam_attr}\" font-size=\"{px}\" fill=\"{INK}\" xml:space=\"preserve\">{}</text>", y + baseline(px), esc(l)));
            y += line_h * px;
        }
        ui.push_str(&format!("<text x=\"36\" y=\"{}\" font-family=\"Segoe UI, Leelawadee UI, Tahoma\" font-weight=\"700\" font-size=\"30\" fill=\"{INK}\">{}</text>", ph - 52.0, esc(&clip(&info.family, 34))));
        ui.push_str(&format!("<text x=\"36\" y=\"{}\" font-family=\"Segoe UI, Leelawadee UI, Tahoma\" font-size=\"20\" fill=\"{GREY}\">{}</text>", ph - 20.0, esc(&clip(&format!("{}  ·  {}", if info.style.is_empty() { "Regular" } else { &info.style }, info.kind), 56))));
    } else {
        w = 1280u32;
        let mw = w as f32 - 96.0;
        let mut y = 56.0f32;
        let ui_font = "font-family=\"Segoe UI, Leelawadee UI, Tahoma\"";
        ui.push_str(&format!("<rect width=\"{w}\" height=\"12\" fill=\"#{ACCENT:06X}\"/>"));
        ui.push_str(&format!("<text x=\"48\" y=\"{}\" {ui_font} font-weight=\"700\" font-size=\"42\" fill=\"{INK}\">{}</text>", y + 20.0, esc(&clip(&info.family, 60))));
        y += 34.0;
        let sub = format!("{}  ·  {}  ·  {}", if info.style.is_empty() { "Regular" } else { &info.style }, info.kind, fmt_bytes(file_size));
        ui.push_str(&format!("<text x=\"48\" y=\"{}\" {ui_font} font-size=\"18\" fill=\"{GREY}\">{}</text>", y + 18.0, esc(&sub)));
        y += 26.0;
        let counts = format!("{} glyphs  ·  {} characters mapped  ·  {}", info.glyphs, info.chars, esc_plain(file));
        ui.push_str(&format!("<text x=\"48\" y=\"{}\" {ui_font} font-size=\"15\" fill=\"{GREY}\">{}</text>", y + 15.0, esc(&counts)));
        y += 38.0;
        ui.push_str(&format!("<rect x=\"48\" y=\"{y}\" width=\"{mw}\" height=\"1\" fill=\"#D6D3C8\"/>"));
        y += 22.0;
        let put = |spec: &mut String, ui: &mut String, y: &mut f32, px: f32, text: &str, tag: &str, max_lines: usize| {
            ui.push_str(&format!("<text x=\"48\" y=\"{}\" {ui_font} font-size=\"12\" fill=\"{GREY}\">{}</text>", *y + 11.0, esc(tag)));
            *y += 18.0;
            for l in m.wrap(text, px, mw, max_lines) {
                spec.push_str(&format!("<text x=\"48\" y=\"{:.1}\" font-family=\"{fam_attr}\" font-size=\"{px}\" fill=\"{INK}\" xml:space=\"preserve\">{}</text>", *y + baseline(px), esc(&l)));
                *y += line_h * px;
            }
            *y += 14.0;
        };
        for (px, lines) in [(80.0f32, 3usize), (56.0, 3), (36.0, 3), (24.0, 3), (16.0, 3), (12.0, 3)] {
            put(&mut spec, &mut ui, &mut y, px, &sample, &format!("{px} px"), lines);
        }
        y += 6.0;
        ui.push_str(&format!("<rect x=\"48\" y=\"{y}\" width=\"{mw}\" height=\"1\" fill=\"#D6D3C8\"/>"));
        y += 22.0;
        let sets: [(&str, &str); 3] = [("Capitals", "ABCDEFGHIJKLMNOPQRSTUVWXYZ"), ("Small letters", "abcdefghijklmnopqrstuvwxyz"), ("Figures and signs", "0123456789  .,:;!?'\"&@#%$€£¥()[]{}<>+−×÷=/\\*_-")];
        for (tag, text) in sets {
            if m.coverage(text) >= 0.5 {
                put(&mut spec, &mut ui, &mut y, 34.0, text, tag, 3);
            }
        }
        for (name, line, need) in SCRIPTS {
            if m.coverage(need) >= 0.85 {
                put(&mut spec, &mut ui, &mut y, 34.0, line, name, 3);
            }
        }
        y += 4.0;
        ui.push_str(&format!("<rect x=\"48\" y=\"{y}\" width=\"{mw}\" height=\"1\" fill=\"#D6D3C8\"/>"));
        y += 26.0;
        let mut facts: Vec<String> = Vec::new();
        if let Some(f) = &info.full {
            facts.push(format!("Full name: {f}"));
        }
        for (id, label) in [(StringId::VERSION_STRING, "Version"), (StringId::DESIGNER, "Designer"), (StringId::MANUFACTURER, "Made by"), (StringId::COPYRIGHT_NOTICE, "Copyright"), (StringId::LICENSE_DESCRIPTION, "Licence"), (StringId::LICENSE_URL, "Licence URL"), (StringId::DESCRIPTION, "About")] {
            if let Some(v) = name_of(&font, id) {
                facts.push(format!("{label}: {v}"));
            }
        }
        facts.extend(info.notes.iter().cloned());
        for f in facts {
            for (i, line) in hard_wrap(&f, 150).into_iter().take(3).enumerate() {
                ui.push_str(&format!("<text x=\"{}\" y=\"{}\" {ui_font} font-size=\"14\" fill=\"{GREY}\" xml:space=\"preserve\">{}</text>", if i == 0 { 48 } else { 64 }, y + 14.0, esc(&line)));
                y += 21.0;
            }
            y += 3.0;
        }
        h = (y + 40.0).ceil().min(6000.0) as u32;
    }

    let render = |svg: &str, fonts: Arc<usvg::fontdb::Database>, family: &str, pixmap: &mut tiny_skia::Pixmap| -> Result<(), String> {
        let mut opt = usvg::Options::default();
        opt.fontdb = fonts;
        opt.font_family = family.to_string();
        let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("font: {e}"))?;
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
        Ok(())
    };
    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("font: sheet too large")?;
    pixmap.fill(tiny_skia::Color::from_rgba8((PAPER >> 16) as u8, (PAPER >> 8) as u8, PAPER as u8, 255));
    let frame = |inner: &str| format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">{inner}</svg>");
    render(&frame(&ui), ui_fonts(), "Segoe UI", &mut pixmap)?;
    render(&frame(&spec), Arc::new(db), &family_css, &mut pixmap)?;
    let mut img = Image::new(w, h, pixmap.take());
    img.colour_note = Some(format!("{} specimen, drawn with the font itself ({})", info.family, info.kind));
    Ok(img)
}

fn esc_plain(s: &str) -> String {
    s.to_string()
}

pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn hard_wrap(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > cols {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ---- Type 1 and Windows bitmap fonts: a card -------------------------------------------------------------------------

/// The value of `/Key` in a Type 1 header: a (string) or a /name.
fn t1_value(head: &str, key: &str) -> Option<String> {
    let at = head.find(&format!("/{key}"))?;
    let rest = head[at + key.len() + 1..].trim_start();
    if let Some(r) = rest.strip_prefix('(') {
        let end = r.find(')')?;
        return Some(r[..end].to_string());
    }
    if let Some(r) = rest.strip_prefix('/') {
        return Some(r.split(|c: char| c.is_whitespace()).next()?.to_string());
    }
    Some(rest.split_whitespace().next()?.to_string())
}

fn type1_card(b: &[u8], path: &Path, first_only: bool) -> Image {
    // A PFB wraps the text in segments (0x80, type, length); the readable header is in the first one. A PFA is the text itself.
    let text_bytes: Vec<u8> = if b.first() == Some(&0x80) { b.get(6..).unwrap_or(&[]).iter().take(8192).copied().collect() } else { b.iter().take(8192).copied().collect() };
    let head: String = text_bytes.iter().map(|&c| if c == b'\r' { '\n' } else { c as char }).collect();
    let name = t1_value(&head, "FullName").or_else(|| t1_value(&head, "FontName")).unwrap_or_else(|| file_name(path));
    let mut lines = vec![name];
    lines.push("Type 1 font (PostScript)".into());
    for (k, label) in [("FamilyName", "Family"), ("Weight", "Weight"), ("version", "Version"), ("Notice", "Notice"), ("ItalicAngle", "Italic angle"), ("isFixedPitch", "Fixed pitch")] {
        if let Some(v) = t1_value(&head, k) {
            lines.push(format!("{label}: {v}"));
        }
    }
    lines.push("Windows no longer draws Type 1 fonts: convert to OpenType to see a specimen".into());
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("pfb").to_ascii_lowercase();
    let mut img = render_card(&Card { badge: ext.to_uppercase(), accent: accent_for(&ext), title: file_name(path), subtitle: fmt_bytes(b.len() as u64), lines }, if first_only { 1 } else { 2 });
    img.colour_note = Some("Type 1 font: what the header says".into());
    img
}

/// A Windows 16-bit font resource file (NE with FNT resources): the face name, size and the like of the first font in it.
fn fon_info(b: &[u8]) -> Option<(String, u16, u16)> {
    let u16le = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
    let u32le = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
    if b.get(..2) != Some(b"MZ") {
        return None;
    }
    let ne = u32le(0x3C)? as usize;
    if b.get(ne..ne + 2) != Some(b"NE") {
        return None;
    }
    let table = ne + u16le(ne + 0x24)? as usize;
    let shift = u16le(table)?;
    let mut at = table + 2;
    loop {
        let ty = u16le(at)?;
        if ty == 0 {
            return None;
        }
        let count = u16le(at + 2)? as usize;
        if ty == 0x8008 && count > 0 {
            let (off, len) = (u16le(at + 8)? as usize, u16le(at + 10)? as usize);
            let start = off << shift;
            let fnt = b.get(start..start + (len << shift))?;
            let face_off = u32::from_le_bytes(fnt.get(105..109)?.try_into().ok()?) as usize;
            let face: String = fnt.get(face_off..)?.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
            let points = u16::from_le_bytes([fnt[68], fnt[69]]);
            let height = u16::from_le_bytes([fnt[88], fnt[89]]);
            return Some((face, points, height));
        }
        at += 8 + count * 12;
    }
}

fn fon_card(b: &[u8], path: &Path, first_only: bool) -> Image {
    let mut lines = Vec::new();
    match fon_info(b) {
        Some((face, points, height)) => {
            lines.push(face);
            lines.push("Windows bitmap font (.fon)".into());
            lines.push(format!("{points} points, {height} pixels high"));
        }
        None => {
            lines.push(file_name(path));
            lines.push("Windows bitmap font (.fon)".into());
        }
    }
    lines.push("Bitmap fonts are not drawn as a specimen".into());
    let mut img = render_card(&Card { badge: "FON".into(), accent: accent_for("fon"), title: file_name(path), subtitle: fmt_bytes(b.len() as u64), lines }, if first_only { 1 } else { 2 });
    img.colour_note = Some("Windows bitmap font: what the file says".into());
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_font(name: &str) -> Option<Vec<u8>> {
        let p = std::path::Path::new(&std::env::var_os("WINDIR")?).join("Fonts").join(name);
        std::fs::read(p).ok()
    }

    fn dark_pixels(img: &Image) -> usize {
        img.rgba.chunks_exact(4).filter(|p| p[0] < 100 && p[1] < 100 && p[2] < 100).count()
    }

    #[test]
    fn sentence_is_cleaned_and_capped() {
        assert_eq!(clean_sample("   "), DEFAULT_SAMPLE);
        assert_eq!(clean_sample("a \n  b\tc"), "a b c");
        assert_eq!(clean_sample(&"x".repeat(300)).chars().count(), 100);
        assert_eq!(clean_sample(&"ก".repeat(300)).chars().count(), 100);
        set_sample("Hello  world");
        assert_eq!(sample(), "Hello world");
        let a = signature();
        set_sample("Other");
        assert_ne!(a, signature());
        set_sample("");
        assert_eq!(sample(), DEFAULT_SAMPLE);
    }

    #[test]
    fn a_ttf_makes_a_sheet_and_a_card() {
        let Some(b) = windows_font("arial.ttf") else { return };
        let full = decode_bytes(&b, "ttf", Path::new("arial.ttf"), false).unwrap();
        assert_eq!(full.width, 1280);
        assert!(full.height > 600, "{}", full.height);
        assert!(dark_pixels(&full) > 5000, "the sheet has text");
        let card = decode_bytes(&b, "ttf", Path::new("arial.ttf"), true).unwrap();
        assert_eq!((card.width, card.height), (640, 480));
        assert!(dark_pixels(&card) > 2000);
    }

    #[test]
    fn a_collection_and_a_non_font_are_handled() {
        if let Some(b) = windows_font("msgothic.ttc") {
            let img = decode_bytes(&b, "ttc", Path::new("msgothic.ttc"), true).unwrap();
            assert!(dark_pixels(&img) > 1000);
        }
        assert!(decode_bytes(b"this is not a font at all, just text", "ttf", Path::new("x.ttf"), true).is_err());
    }

    #[test]
    fn wrap_breaks_long_words_and_caps_lines() {
        let Some(b) = windows_font("arial.ttf") else { return };
        let font = FontRef::from_index(&b, 0).unwrap();
        let m = Measure::new(&font);
        let lines = m.wrap("the quick brown fox jumps over the lazy dog", 40.0, 300.0, 10);
        assert!(lines.len() >= 3 && lines.iter().all(|l| m.width(l, 40.0) <= 301.0), "{lines:?}");
        let one = m.wrap(&"w".repeat(80), 40.0, 300.0, 100);
        assert!(one.len() > 5);
        assert_eq!(m.wrap(&"word ".repeat(100), 40.0, 300.0, 2).len(), 2);
    }

    #[test]
    fn type1_header_is_read() {
        let pfa = b"%!PS-AdobeFont-1.0: Foo 001.000\n/FullName (Foo Bold Oblique) readonly def\n/FamilyName (Foo) readonly def\n/Weight (Bold) readonly def\n/ItalicAngle -12 def\n";
        let img = decode_bytes(pfa, "pfa", Path::new("foo.pfa"), true).unwrap();
        assert!(img.width > 0 && img.colour_note.unwrap().contains("Type 1"));
        assert_eq!(t1_value(std::str::from_utf8(pfa).unwrap(), "FamilyName").as_deref(), Some("Foo"));
    }
}
