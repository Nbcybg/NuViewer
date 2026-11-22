//! XMP star ratings and colour labels for culling: the same two fields Adobe Bridge, Lightroom, Camera Raw, darktable and XnView
//! read and write (`xmp:Rating` -1 .. 5, `xmp:Label` "Red" / "Yellow" / "Green" / "Blue" / "Purple").
//!
//! **The picture file is never touched.** A rating lives in a sidecar next to it; everything else in that sidecar (develop settings,
//! keywords, whatever another program put there) is carried over byte for byte, only the two values change. Without a sidecar a
//! rating is read from the XMP packet inside the picture (JPEG, PNG, WebP, TIFF ... when it is in the first 512 KB) and a new
//! sidecar is made on the first change.
//!
//! Sidecar names (the convention each program uses): an existing `IMG_0001.CR2.xmp` (darktable) or, for camera RAW, `IMG_0001.xmp`
//! (Adobe, XnView, RawTherapee) is read and kept up to date; a new sidecar is `IMG_0001.xmp` for a RAW file and `IMG_0001.jpg.xmp`
//! for everything else, so the JPG of a RAW + JPG pair never shares a rating by accident.

use std::path::{Path, PathBuf};

/// A colour label, named as Adobe names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Label {
    Red,
    Yellow,
    Green,
    Blue,
    Purple,
}

impl Label {
    pub const ALL: [Label; 5] = [Label::Red, Label::Yellow, Label::Green, Label::Blue, Label::Purple];

    pub fn name(self) -> &'static str {
        match self {
            Label::Red => "Red",
            Label::Yellow => "Yellow",
            Label::Green => "Green",
            Label::Blue => "Blue",
            Label::Purple => "Purple",
        }
    }

    pub fn from_name(s: &str) -> Option<Label> {
        // Bridge's own words for the five (Select / Second / Approved / Review / To Do) are accepted too.
        match s.trim().to_ascii_lowercase().as_str() {
            "red" | "select" => Some(Label::Red),
            "yellow" | "second" => Some(Label::Yellow),
            "green" | "approved" => Some(Label::Green),
            "blue" | "review" => Some(Label::Blue),
            "purple" | "to do" | "todo" => Some(Label::Purple),
            _ => None,
        }
    }

    /// Colour for a dot or a stripe, 0xRRGGBB.
    pub fn rgb(self) -> u32 {
        match self {
            Label::Red => 0xE5484D,
            Label::Yellow => 0xF5C518,
            Label::Green => 0x3FB950,
            Label::Blue => 0x4C8DFF,
            Label::Purple => 0xA371F7,
        }
    }
}

/// What is known about a picture's rating.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Meta {
    /// 0 = not rated, 1..=5 stars.
    pub stars: u8,
    /// Marked "rejected" (`xmp:Rating` -1).
    pub rejected: bool,
    pub label: Option<Label>,
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        *self == Meta::default()
    }

    /// "★★★☆☆", "rejected" or "" for the status line.
    pub fn stars_text(&self) -> String {
        if self.rejected {
            return "\u{2715} rejected".into();
        }
        if self.stars == 0 {
            return String::new();
        }
        (1..=5).map(|i| if i <= self.stars { '\u{2605}' } else { '\u{2606}' }).collect()
    }

    /// "★★★ · Red" for a message.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        let s = self.stars_text();
        if !s.is_empty() {
            parts.push(s);
        }
        if let Some(l) = self.label {
            parts.push(l.name().to_string());
        }
        if parts.is_empty() { "no rating".to_string() } else { parts.join("  ·  ") }
    }
}

/// One keystroke's worth of change to a rating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateChange {
    /// 0..=5 stars; the number the picture already has clears it again (press 3 twice = no stars).
    Stars(u8),
    /// Rejected on / off.
    ToggleReject,
    /// A colour label; the one the picture already has removes it. `None` clears the label.
    Label(Option<Label>),
}

impl Meta {
    pub fn changed(self, c: RateChange) -> Meta {
        let mut m = self;
        match c {
            RateChange::Stars(n) => {
                let n = n.min(5);
                m.rejected = false;
                m.stars = if n == self.stars && !self.rejected { 0 } else { n };
            }
            RateChange::ToggleReject => {
                m.rejected = !self.rejected;
                if m.rejected {
                    m.stars = 0;
                }
            }
            RateChange::Label(l) => m.label = if l.is_some() && l == self.label { None } else { l },
        }
        m
    }
}

impl Meta {
    /// Another picture of a selection takes over what `c` changed on the focused one (`new` is the focused picture's result): its stars,
    /// reject mark or label become the same, its other fields stay.
    pub fn follow(self, new: Meta, c: RateChange) -> Meta {
        let mut m = self;
        match c {
            RateChange::Stars(_) => {
                m.stars = new.stars;
                m.rejected = new.rejected;
            }
            RateChange::ToggleReject => {
                m.rejected = new.rejected;
                if new.rejected {
                    m.stars = 0;
                }
            }
            RateChange::Label(_) => m.label = new.label,
        }
        m
    }
}

/// What a filter lets through (the gallery's "Rating" choices).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Filter {
    /// Everything.
    #[default]
    All,
    /// At least this many stars (not rejected).
    AtLeast(u8),
    Unrated,
    Rejected,
    /// Everything except rejected pictures.
    HideRejected,
    Label(Label),
}

impl Filter {
    pub fn accepts(self, m: &Meta) -> bool {
        match self {
            Filter::All => true,
            Filter::AtLeast(n) => !m.rejected && m.stars >= n,
            Filter::Unrated => !m.rejected && m.stars == 0,
            Filter::Rejected => m.rejected,
            Filter::HideRejected => !m.rejected,
            Filter::Label(l) => m.label == Some(l),
        }
    }

    pub fn id(self) -> String {
        match self {
            Filter::All => "all".into(),
            Filter::AtLeast(n) => format!("stars{n}"),
            Filter::Unrated => "unrated".into(),
            Filter::Rejected => "rejected".into(),
            Filter::HideRejected => "hide_rejected".into(),
            Filter::Label(l) => format!("label_{}", l.name().to_ascii_lowercase()),
        }
    }

    pub fn from_id(s: &str) -> Option<Filter> {
        let s = s.trim().to_ascii_lowercase();
        Some(match s.as_str() {
            "all" => Filter::All,
            "unrated" => Filter::Unrated,
            "rejected" => Filter::Rejected,
            "hide_rejected" => Filter::HideRejected,
            _ => {
                if let Some(n) = s.strip_prefix("stars").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=5).contains(n)) {
                    Filter::AtLeast(n)
                } else {
                    Filter::Label(Label::from_name(s.strip_prefix("label_")?)?)
                }
            }
        })
    }

    pub fn label(self) -> String {
        match self {
            Filter::All => "Any rating".into(),
            Filter::AtLeast(n) => format!("{} and up", "\u{2605}".repeat(n as usize)),
            Filter::Unrated => "Not rated".into(),
            Filter::Rejected => "Rejected".into(),
            Filter::HideRejected => "Hide rejected".into(),
            Filter::Label(l) => format!("Label: {}", l.name()),
        }
    }
}

// ---- where the sidecar is ----------------------------------------------------------------------------------------------

fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Sidecars to look for, best first: `IMG.CR2.xmp`, and for a camera RAW `IMG.xmp`.
pub fn candidates(picture: &Path) -> Vec<PathBuf> {
    let mut with_ext = picture.as_os_str().to_os_string();
    with_ext.push(".xmp");
    let mut v = vec![PathBuf::from(with_ext)];
    if nv_codecs::is_raw_ext(&ext_lower(picture)) {
        v.push(picture.with_extension("xmp"));
    }
    v
}

/// The sidecar that exists, else the one a first rating would create.
pub fn sidecar_for(picture: &Path) -> PathBuf {
    let c = candidates(picture);
    c.iter().find(|p| p.is_file()).cloned().unwrap_or_else(|| if nv_codecs::is_raw_ext(&ext_lower(picture)) { c[1].clone() } else { c[0].clone() })
}

// ---- reading -----------------------------------------------------------------------------------------------------------

/// The value of `name` (like `xmp:Rating`) in an XMP text: as an attribute (`xmp:Rating="3"`) or as an element
/// (`<xmp:Rating>3</xmp:Rating>`).
fn value_of(xml: &str, name: &str) -> Option<String> {
    let at = find_attr(xml, name)?;
    if let Some((vs, ve)) = at {
        return Some(xml[vs..ve].to_string());
    }
    None
}

/// Locate `name`: `Some(Some((start, end)))` of the value text for an attribute or element, `None` if it is not there.
fn find_attr(xml: &str, name: &str) -> Option<Option<(usize, usize)>> {
    // attribute: name="..."  or name='...'
    let mut from = 0;
    while let Some(rel) = xml[from..].find(name) {
        let p = from + rel;
        let after = p + name.len();
        let before_ok = p == 0 || !xml.as_bytes()[p - 1].is_ascii_alphanumeric() && xml.as_bytes()[p - 1] != b':';
        let rest = &xml[after..];
        let trimmed = rest.trim_start();
        if before_ok && trimmed.starts_with('=') {
            let q = trimmed[1..].trim_start();
            if let Some(quote) = q.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let vs = xml.len() - q.len() + 1;
                if let Some(len) = xml[vs..].find(quote) {
                    return Some(Some((vs, vs + len)));
                }
            }
        }
        // element: <name>value</name>  (the tag must not have attributes: Rating and Label are plain)
        if before_ok && p > 0 && xml.as_bytes()[p - 1] == b'<' && rest.starts_with('>') {
            let vs = after + 1;
            if let Some(len) = xml[vs..].find("</") {
                return Some(Some((vs, vs + len)));
            }
        }
        from = after;
    }
    None
}

/// Read a rating out of XMP text.
pub fn parse(xml: &str) -> Meta {
    let mut m = Meta::default();
    if let Some(v) = value_of(xml, "xmp:Rating") {
        match v.trim().parse::<i32>() {
            Ok(-1) => m.rejected = true,
            Ok(n) if (1..=5).contains(&n) => m.stars = n as u8,
            _ => {}
        }
    }
    if let Some(v) = value_of(xml, "xmp:Label") {
        m.label = Label::from_name(&v);
    }
    m
}

/// The XMP packet inside a picture file, if it is in the first `limit` bytes.
fn embedded_packet(picture: &Path, limit: u64) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(picture).ok()?.take(limit).read_to_end(&mut buf).ok()?;
    let start = buf.windows(11).position(|w| w == b"<x:xmpmeta ").or_else(|| buf.windows(10).position(|w| w == b"<x:xmpmeta"))?;
    let end = buf[start..].windows(12).position(|w| w == b"</x:xmpmeta>").map(|e| start + e + 12)?;
    Some(String::from_utf8_lossy(&buf[start..end]).into_owned())
}

/// The rating in the picture's sidecar; `None` when it has no sidecar. Cheap (a file or two), unlike `read`.
pub fn read_sidecar(picture: &Path) -> Option<Meta> {
    candidates(picture).iter().find_map(|c| std::fs::read_to_string(c).ok()).map(|t| parse(&t))
}

/// The rating of a picture: its sidecar, else the XMP inside it (looked for in the first 512 KB). A picture without either has
/// `Meta::default()`.
pub fn read(picture: &Path) -> Meta {
    read_within(picture, 512 * 1024)
}

/// Like `read`, looking inside the picture only as far as `limit` bytes (a gallery reading a whole folder uses a small one).
pub fn read_within(picture: &Path, limit: u64) -> Meta {
    if let Some(m) = read_sidecar(picture) {
        return m;
    }
    // Camera RAW packs its XMP somewhere else (and `xmp:Rating` is not in the part we scan); only ordinary pictures are searched.
    if nv_codecs::is_raw_ext(&ext_lower(picture)) {
        return Meta::default();
    }
    embedded_packet(picture, limit).map(|t| parse(&t)).unwrap_or_default()
}

// ---- writing -----------------------------------------------------------------------------------------------------------

const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";

fn new_sidecar(meta: &Meta) -> String {
    let mut attrs = format!("xmp:Rating=\"{}\"", rating_value(meta));
    if let Some(l) = meta.label {
        attrs += &format!(" xmp:Label=\"{}\"", l.name());
    }
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"NuViewer\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\" xmlns:xmp=\"{XMP_NS}\" {attrs}/>\n </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>\n"
    )
}

fn rating_value(m: &Meta) -> i32 {
    if m.rejected { -1 } else { m.stars as i32 }
}

/// Set (or remove, with `None`) the value of `name` in XMP text; every byte outside that value stays as it was.
fn set_value(xml: &mut String, name: &str, value: Option<&str>) {
    match find_attr(xml, name) {
        Some(Some((vs, ve))) => match value {
            Some(v) => xml.replace_range(vs..ve, v),
            None => {
                // Remove the whole attribute (or element) with its leading space.
                let is_element = xml[..vs].ends_with('>');
                if is_element {
                    let open = xml[..vs].rfind('<').unwrap_or(0);
                    let close = xml[ve..].find('>').map(|c| ve + c + 1).unwrap_or(ve);
                    xml.replace_range(open..close, "");
                } else {
                    let attr_start = xml[..vs].rfind(name).unwrap_or(vs);
                    let mut from = attr_start;
                    while from > 0 && xml.as_bytes()[from - 1].is_ascii_whitespace() {
                        from -= 1;
                    }
                    xml.replace_range(from..ve + 1, "");
                }
            }
        },
        _ => {
            let Some(v) = value else { return };
            // Add it to the first rdf:Description (as an attribute), declaring the xmp namespace if nothing did.
            let Some(d) = xml.find("<rdf:Description") else { return };
            let Some(gt) = xml[d..].find('>') else { return };
            let mut end = d + gt;
            if xml.as_bytes()[end - 1] == b'/' {
                end -= 1;
            }
            let tag = &xml[d..end];
            let mut insert = String::new();
            if !xml.contains("xmlns:xmp=") {
                insert += &format!(" xmlns:xmp=\"{XMP_NS}\"");
            }
            insert += &format!(" {name}=\"{v}\"");
            let keep_space = tag.ends_with(|c: char| c.is_ascii_whitespace());
            if keep_space {
                insert = insert.trim_start().to_string() + " ";
            }
            xml.insert_str(end, &insert);
        }
    }
}

/// The text of a sidecar after the rating changed to `meta`.
pub fn updated(existing: Option<&str>, meta: &Meta) -> String {
    let Some(old) = existing.filter(|t| t.contains("<rdf:Description")) else { return new_sidecar(meta) };
    let mut xml = old.to_string();
    set_value(&mut xml, "xmp:Rating", Some(&rating_value(meta).to_string()));
    set_value(&mut xml, "xmp:Label", meta.label.map(|l| l.name()));
    xml
}

/// Save a rating for `picture` in its sidecar (made when there is none). The picture is not touched. Written to a temporary
/// file and renamed over the sidecar, so a crash can leave the old sidecar or the new one, never half of one.
pub fn write(picture: &Path, meta: &Meta) -> Result<PathBuf, String> {
    let path = sidecar_for(picture);
    let existing = std::fs::read_to_string(&path).ok();
    let text = updated(existing.as_deref(), meta);
    if existing.as_deref() == Some(text.as_str()) {
        return Ok(path);
    }
    let tmp = {
        let mut n = path.as_os_str().to_os_string();
        n.push(".nutmp");
        PathBuf::from(n)
    };
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARKTABLE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"XMP Core 4.4.0-Exiv2\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  <rdf:Description rdf:about=\"\"\n    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n    xmlns:darktable=\"http://darktable.sf.net/\"\n   xmp:Rating=\"1\"\n   darktable:history_end=\"3\">\n   <darktable:history><rdf:Seq><rdf:li darktable:operation=\"exposure\"/></rdf:Seq></darktable:history>\n  </rdf:Description>\n </rdf:RDF>\n</x:xmpmeta>\n";

    fn m(stars: u8, label: Option<Label>) -> Meta {
        Meta { stars, rejected: false, label }
    }

    #[test]
    fn reads_attributes_and_elements() {
        assert_eq!(parse(DARKTABLE), m(1, None));
        assert_eq!(parse("<x:xmpmeta><rdf:Description><xmp:Rating>4</xmp:Rating><xmp:Label>Green</xmp:Label></rdf:Description></x:xmpmeta>"), m(4, Some(Label::Green)));
        assert_eq!(parse("<rdf:Description xmp:Rating='-1' xmp:Label='Select'/>"), Meta { stars: 0, rejected: true, label: Some(Label::Red) });
        assert_eq!(parse("<rdf:Description xmp:Rating=\"9\" xmp:Label=\"Mauve\"/>"), Meta::default(), "nonsense is ignored");
        assert_eq!(parse(""), Meta::default());
        // A longer name must not be mistaken for xmp:Rating.
        assert_eq!(parse("<rdf:Description xmpDM:xmp:RatingX=\"5\" my:xmp:Rating=\"4\"/>"), Meta::default());
    }

    #[test]
    fn changing_a_rating_touches_only_the_value() {
        let out = updated(Some(DARKTABLE), &m(4, None));
        assert_eq!(out, DARKTABLE.replace("xmp:Rating=\"1\"", "xmp:Rating=\"4\""), "everything else is identical");
        assert_eq!(parse(&out), m(4, None));
    }

    #[test]
    fn a_label_is_added_and_removed_without_disturbing_the_rest() {
        let with = updated(Some(DARKTABLE), &m(1, Some(Label::Blue)));
        assert_eq!(parse(&with), m(1, Some(Label::Blue)));
        assert!(with.contains("darktable:history_end=\"3\"") && with.contains("exposure"));
        let without = updated(Some(&with), &m(1, None));
        assert_eq!(without, DARKTABLE, "removing it again restores the file exactly");
    }

    #[test]
    fn a_rating_is_added_to_a_sidecar_that_has_none() {
        let plain = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"r\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"/></rdf:RDF></x:xmpmeta>";
        let out = updated(Some(plain), &m(3, Some(Label::Red)));
        assert_eq!(parse(&out), m(3, Some(Label::Red)));
        assert!(out.contains("xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\""), "the namespace is declared");
        assert!(out.contains("xmlns:dc="), "the rest stays");
        assert!(roxmltree::Document::parse(&out).is_ok(), "still well-formed XML");
    }

    #[test]
    fn element_form_is_updated_in_place_and_rejects_use_minus_one() {
        let src = "<rdf:Description><xmp:Rating>2</xmp:Rating><xmp:Label>Red</xmp:Label></rdf:Description>";
        let out = updated(Some(src), &Meta { stars: 0, rejected: true, label: None });
        assert_eq!(out, "<rdf:Description><xmp:Rating>-1</xmp:Rating></rdf:Description>");
        assert_eq!(parse(&out), Meta { stars: 0, rejected: true, label: None });
    }

    #[test]
    fn a_new_sidecar_is_valid_xml() {
        let t = updated(None, &m(5, Some(Label::Purple)));
        assert!(roxmltree::Document::parse(t.trim_start_matches("<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>").split("<?xpacket end").next().unwrap()).is_ok());
        assert_eq!(parse(&t), m(5, Some(Label::Purple)));
    }

    #[test]
    fn sidecar_names_follow_each_programs_convention() {
        let raw = Path::new("d/IMG_1.CR2");
        let jpg = Path::new("d/IMG_1.JPG");
        assert_eq!(candidates(raw), vec![PathBuf::from("d/IMG_1.CR2.xmp"), PathBuf::from("d/IMG_1.xmp")]);
        assert_eq!(candidates(jpg), vec![PathBuf::from("d/IMG_1.JPG.xmp")]);
        assert_eq!(sidecar_for(raw), PathBuf::from("d/IMG_1.xmp"), "a new RAW sidecar uses the Adobe name");
        assert_eq!(sidecar_for(jpg), PathBuf::from("d/IMG_1.JPG.xmp"));
    }

    #[test]
    fn write_read_round_trip_leaves_the_picture_alone() {
        let d = std::env::temp_dir().join(format!("nv_xmp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (raw, jpg) = (d.join("A.RAF"), d.join("A.JPG"));
        std::fs::write(&raw, b"raw bytes").unwrap();
        std::fs::write(&jpg, b"jpg bytes").unwrap();
        std::fs::write(d.join("A.RAF.xmp"), DARKTABLE).unwrap(); // darktable's sidecar of the RAW

        assert_eq!(read(&raw), m(1, None));
        assert_eq!(read(&jpg), Meta::default(), "the JPG of the pair does not share the RAW's rating");

        write(&raw, &m(5, Some(Label::Green))).unwrap();
        write(&jpg, &m(2, None)).unwrap();
        assert_eq!(read(&raw), m(5, Some(Label::Green)));
        assert_eq!(read(&jpg), m(2, None));
        assert!(std::fs::read_to_string(d.join("A.RAF.xmp")).unwrap().contains("darktable:history_end"), "darktable's data survived");
        assert!(!d.join("A.xmp").exists(), "the existing sidecar was used, not a second one");
        assert_eq!(std::fs::read(&raw).unwrap(), b"raw bytes");
        assert_eq!(std::fs::read(&jpg).unwrap(), b"jpg bytes");
        assert!(!d.join("A.RAF.xmp.nutmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_xmp_inside_a_jpeg_is_read_when_there_is_no_sidecar() {
        let d = std::env::temp_dir().join(format!("nv_xmp_emb_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE1, 0x01, 0x00];
        bytes.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF><rdf:Description xmp:Rating=\"3\" xmp:Label=\"Yellow\"/></rdf:RDF></x:xmpmeta>");
        std::fs::write(d.join("e.jpg"), &bytes).unwrap();
        assert_eq!(read(&d.join("e.jpg")), m(3, Some(Label::Yellow)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keystrokes_change_a_rating_the_way_lightroom_does() {
        let none = Meta::default();
        let three = none.changed(RateChange::Stars(3));
        assert_eq!(three, m(3, None));
        assert_eq!(three.changed(RateChange::Stars(3)), none, "the same number again clears it");
        assert_eq!(three.changed(RateChange::Stars(5)), m(5, None));
        assert_eq!(three.changed(RateChange::Stars(0)), none);
        let red = three.changed(RateChange::Label(Some(Label::Red)));
        assert_eq!(red, m(3, Some(Label::Red)));
        assert_eq!(red.changed(RateChange::Label(Some(Label::Red))), three, "the same label again removes it");
        assert_eq!(red.changed(RateChange::Label(Some(Label::Blue))), m(3, Some(Label::Blue)));
        assert_eq!(red.changed(RateChange::Label(None)), three);
        let rej = red.changed(RateChange::ToggleReject);
        assert_eq!(rej, Meta { stars: 0, rejected: true, label: Some(Label::Red) });
        assert_eq!(rej.changed(RateChange::ToggleReject), Meta { stars: 0, rejected: false, label: Some(Label::Red) });
        assert_eq!(rej.changed(RateChange::Stars(2)), m(2, Some(Label::Red)), "rating a rejected picture un-rejects it");
    }

    #[test]
    fn a_selection_follows_the_focused_picture() {
        let c = RateChange::Stars(4);
        let focused = m(2, Some(Label::Red)).changed(c); // 4 stars, red
        let other = m(5, Some(Label::Blue));
        assert_eq!(other.follow(focused, c), m(4, Some(Label::Blue)), "stars follow, the label stays");
        let c = RateChange::Label(Some(Label::Green));
        let focused = m(2, None).changed(c);
        assert_eq!(m(1, Some(Label::Red)).follow(focused, c), m(1, Some(Label::Green)));
        let c = RateChange::ToggleReject;
        let focused = m(3, None).changed(c);
        assert_eq!(m(5, None).follow(focused, c), Meta { stars: 0, rejected: true, label: None });
    }

    #[test]
    fn filters() {
        let five = m(5, Some(Label::Red));
        let none = Meta::default();
        let rej = Meta { stars: 3, rejected: true, label: None };
        assert!(Filter::All.accepts(&rej));
        assert!(Filter::AtLeast(4).accepts(&five) && !Filter::AtLeast(4).accepts(&none) && !Filter::AtLeast(1).accepts(&rej));
        assert!(Filter::Unrated.accepts(&none) && !Filter::Unrated.accepts(&five));
        assert!(Filter::Rejected.accepts(&rej) && !Filter::Rejected.accepts(&five));
        assert!(Filter::HideRejected.accepts(&none) && !Filter::HideRejected.accepts(&rej));
        assert!(Filter::Label(Label::Red).accepts(&five) && !Filter::Label(Label::Blue).accepts(&five));
        for f in [Filter::All, Filter::AtLeast(3), Filter::Unrated, Filter::Rejected, Filter::HideRejected, Filter::Label(Label::Purple)] {
            assert_eq!(Filter::from_id(&f.id()), Some(f));
        }
        assert_eq!(Filter::from_id("stars9"), None);
    }
}
