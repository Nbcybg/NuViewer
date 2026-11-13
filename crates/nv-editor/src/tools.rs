//! The tools NuEditor is meant to have. The viewer draws this list in the editor mock-up; a tool becomes `Ready` when
//! it is wired to real code. Keeping the catalogue here (not in the UI) lets tests and the UI agree on it.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolState {
    /// Works today.
    Ready,
    /// In the mock-up only.
    Planned,
}

#[derive(Clone, Copy, Debug)]
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    /// A symbol from a font every Windows install has.
    pub glyph: &'static str,
    pub hint: &'static str,
    pub state: ToolState,
}

pub const TOOLS: &[Tool] = &[
    Tool { id: "rotate", name: "Rotate", glyph: "\u{21BB}", hint: "Turn by 45 or 90 degrees, save in place", state: ToolState::Ready },
    Tool { id: "crop", name: "Crop", glyph: "\u{2702}", hint: "Drag a frame, fixed ratios, straighten", state: ToolState::Planned },
    Tool { id: "resize", name: "Resize", glyph: "\u{2922}", hint: "Scale to pixels or percent, resampling choice", state: ToolState::Planned },
    Tool { id: "adjust", name: "Adjust", glyph: "\u{25D0}", hint: "Exposure, contrast, highlights, shadows, colour", state: ToolState::Planned },
    Tool { id: "filters", name: "Filters", glyph: "\u{2726}", hint: "Sharpen, denoise, presets", state: ToolState::Planned },
    Tool { id: "retouch", name: "Retouch", glyph: "\u{2736}", hint: "Heal and clone spots, red eye", state: ToolState::Planned },
    Tool { id: "draw", name: "Draw", glyph: "\u{270E}", hint: "Pen, shapes, arrows, blur / pixelate regions", state: ToolState::Planned },
    Tool { id: "text", name: "Text", glyph: "T", hint: "Captions and watermarks", state: ToolState::Planned },
    Tool { id: "export", name: "Export", glyph: "\u{21E9}", hint: "Save as, format and quality, strip metadata", state: ToolState::Planned },
];

pub fn find(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_rotate_is_ready() {
        for (i, a) in TOOLS.iter().enumerate() {
            assert!(TOOLS[i + 1..].iter().all(|b| b.id != a.id), "{} twice", a.id);
        }
        assert_eq!(find("rotate").map(|t| t.state), Some(ToolState::Ready));
        assert_eq!(find("crop").map(|t| t.state), Some(ToolState::Planned));
        assert!(find("nope").is_none());
    }
}
