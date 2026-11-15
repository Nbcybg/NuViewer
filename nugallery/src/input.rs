//! A single-line text box (path, search, rename, metadata fields): the text, the caret and a selection. Drawing and the
//! clipboard live in the app; this is only the editing rules, so they can be tested.

pub const VK_BACK: u32 = 0x08;
pub const VK_TAB: u32 = 0x09;
pub const VK_RETURN: u32 = 0x0D;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_END: u32 = 0x23;
pub const VK_HOME: u32 = 0x24;
pub const VK_LEFT: u32 = 0x25;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_DELETE: u32 = 0x2E;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    Copy,
    Cut,
    Paste,
}

/// What a key did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not a key the box uses.
    Ignored,
    /// Handled (moved the caret or changed the text).
    Handled,
    Enter,
    Escape,
    Tab,
    /// The app does it (it has the clipboard).
    Clipboard(Clip),
}

#[derive(Clone, Debug, Default)]
pub struct TextInput {
    pub chars: Vec<char>,
    pub cursor: usize,
    /// The other end of the selection (the caret is one end).
    pub anchor: Option<usize>,
}

impl TextInput {
    /// A box holding `text`, caret at the end.
    pub fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        TextInput { cursor: chars.len(), chars, anchor: None }
    }

    /// A box holding `text` with everything selected (typing replaces it, like focusing a field in Explorer).
    pub fn selected(text: &str) -> Self {
        let mut t = Self::new(text);
        t.select_all();
        t
    }

    /// A rename box: only the name without its extension is selected.
    pub fn name_selected(text: &str) -> Self {
        let mut t = Self::new(text);
        let stem_len = match text.rfind('.') {
            Some(dot) if dot > 0 => text[..dot].chars().count(),
            _ => t.chars.len(),
        };
        t.anchor = Some(0);
        t.cursor = stem_len;
        t
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.chars.len();
    }

    /// The selected range as (start, end) in chars.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.filter(|&a| a != self.cursor).map(|a| (a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> String {
        self.selection().map(|(a, b)| self.chars[a..b].iter().collect()).unwrap_or_default()
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.chars.drain(a..b);
            self.cursor = a;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        self.delete_selection();
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Insert pasted text (line breaks become spaces).
    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        for c in s.chars() {
            let c = if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c };
            if !c.is_control() {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    /// Delete the selection (for Cut).
    pub fn cut(&mut self) -> String {
        let t = self.selected_text();
        self.delete_selection();
        t
    }

    /// Put the caret at char `index` (a click); with `extend` the selection grows from where it was.
    pub fn click_at(&mut self, index: usize, extend: bool) {
        let index = index.min(self.chars.len());
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = index;
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && !self.chars[i - 1].is_alphanumeric() {
            i -= 1;
        }
        while i > 0 && self.chars[i - 1].is_alphanumeric() {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        let n = self.chars.len();
        while i < n && !self.chars[i].is_alphanumeric() {
            i += 1;
        }
        while i < n && self.chars[i].is_alphanumeric() {
            i += 1;
        }
        i
    }

    fn move_to(&mut self, to: usize, shift: bool) {
        if shift {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to;
    }

    pub fn key(&mut self, vk: u32, ctrl: bool, shift: bool) -> Outcome {
        match vk {
            VK_RETURN => Outcome::Enter,
            VK_ESCAPE => Outcome::Escape,
            VK_TAB => Outcome::Tab,
            VK_BACK => {
                if !self.delete_selection() && self.cursor > 0 {
                    let to = if ctrl { self.word_left() } else { self.cursor - 1 };
                    self.chars.drain(to..self.cursor);
                    self.cursor = to;
                }
                Outcome::Handled
            }
            VK_DELETE => {
                if !self.delete_selection() && self.cursor < self.chars.len() {
                    let to = if ctrl { self.word_right() } else { self.cursor + 1 };
                    self.chars.drain(self.cursor..to);
                }
                Outcome::Handled
            }
            VK_LEFT => {
                match (self.selection(), shift) {
                    (Some((a, _)), false) => {
                        self.cursor = a;
                        self.anchor = None;
                    }
                    _ => {
                        let to = if ctrl { self.word_left() } else { self.cursor.saturating_sub(1) };
                        self.move_to(to, shift);
                    }
                }
                Outcome::Handled
            }
            VK_RIGHT => {
                match (self.selection(), shift) {
                    (Some((_, b)), false) => {
                        self.cursor = b;
                        self.anchor = None;
                    }
                    _ => {
                        let to = if ctrl { self.word_right() } else { (self.cursor + 1).min(self.chars.len()) };
                        self.move_to(to, shift);
                    }
                }
                Outcome::Handled
            }
            VK_HOME => {
                self.move_to(0, shift);
                Outcome::Handled
            }
            VK_END => {
                self.move_to(self.chars.len(), shift);
                Outcome::Handled
            }
            0x41 if ctrl => {
                self.select_all();
                Outcome::Handled
            }
            0x43 if ctrl => Outcome::Clipboard(Clip::Copy),
            0x58 if ctrl => Outcome::Clipboard(Clip::Cut),
            0x56 if ctrl => Outcome::Clipboard(Clip::Paste),
            _ => Outcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> TextInput {
        let mut t = TextInput::default();
        for c in s.chars() {
            t.insert(c);
        }
        t
    }

    #[test]
    fn typing_and_deleting() {
        let mut t = typed("abc");
        assert_eq!(t.text(), "abc");
        t.key(VK_LEFT, false, false);
        t.key(VK_BACK, false, false);
        assert_eq!(t.text(), "ac");
        t.key(VK_DELETE, false, false);
        assert_eq!(t.text(), "a");
        t.key(VK_HOME, false, false);
        t.insert('X');
        assert_eq!(t.text(), "Xa");
        assert_eq!(t.key(VK_RETURN, false, false), Outcome::Enter);
        assert_eq!(t.key(VK_ESCAPE, false, false), Outcome::Escape);
        assert_eq!(t.key(0x70, false, false), Outcome::Ignored);
    }

    #[test]
    fn selection_is_replaced_by_typing() {
        let mut t = TextInput::selected("hello");
        assert_eq!(t.selected_text(), "hello");
        t.insert('H');
        assert_eq!(t.text(), "H");
        let mut t = TextInput::new("hello world");
        t.key(VK_LEFT, false, true);
        t.key(VK_LEFT, false, true);
        assert_eq!(t.selected_text(), "ld");
        assert_eq!(t.cut(), "ld");
        assert_eq!(t.text(), "hello wor");
        t.key(VK_LEFT, false, false);
        assert_eq!(t.cursor, 8);
    }

    #[test]
    fn rename_boxes_select_the_name_without_the_extension() {
        let t = TextInput::name_selected("holiday.photo.jpg");
        assert_eq!(t.selected_text(), "holiday.photo");
        let t = TextInput::name_selected("README");
        assert_eq!(t.selected_text(), "README");
        let t = TextInput::name_selected(".hidden");
        assert_eq!(t.selected_text(), ".hidden", "a leading dot is not an extension");
    }

    #[test]
    fn word_jumps_and_pasting() {
        let mut t = TextInput::new("one two  three");
        t.key(VK_LEFT, true, false);
        assert_eq!(t.cursor, 9);
        t.key(VK_BACK, true, false);
        assert_eq!(t.text(), "one three", "Ctrl+Backspace takes the word and the spaces before it");
        t.insert_str("a\r\nb\tc");
        assert_eq!(t.text(), "one a  b cthree", "each line break and tab becomes a space");
        t.key(VK_HOME, false, false);
        t.key(VK_RIGHT, true, false);
        assert_eq!(t.cursor, 3);
        assert_eq!(t.key(0x56, true, false), Outcome::Clipboard(Clip::Paste));
    }

    #[test]
    fn clicks_move_and_extend() {
        let mut t = TextInput::new("abcdef");
        t.click_at(2, false);
        t.click_at(5, true);
        assert_eq!(t.selected_text(), "cde");
        t.click_at(99, false);
        assert_eq!((t.cursor, t.selection()), (6, None));
        t.insert('\u{7}'); // control characters are ignored
        assert_eq!(t.text(), "abcdef");
    }

    #[test]
    fn thai_text_is_edited_by_characters() {
        let mut t = typed("รูปภาพ");
        t.key(VK_BACK, false, false);
        assert_eq!(t.chars.len(), 5);
    }
}
