//! The right-click menu and the toolbar's drop-down menus. Menu items are numbered as they are added; the number picked
//! maps back to a `Cmd` through the list built with the menu, so the code that builds a menu and the code that
//! runs its choice can't drift apart.

use crate::app::App;
use crate::chrome::Btn;
use crate::model::{Entry, Show};
use crate::state::ViewMode;
use nv_core::folder::SortKey;
use nv_win::menu::{popup, Entry as Item};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuickOp {
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
}

impl QuickOp {
    pub fn label(self) -> &'static str {
        match self {
            QuickOp::RotateLeft => "Rotate left 90°",
            QuickOp::RotateRight => "Rotate right 90°",
            QuickOp::FlipH => "Flip horizontally",
            QuickOp::FlipV => "Flip vertically",
        }
    }
}

/// What "Quick convert" can do in one click.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Preset {
    JpegHigh,
    JpegSmall,
    Png,
    Webp,
    Bmp,
    Resize(u32),
}

impl Preset {
    pub fn label(self) -> String {
        match self {
            Preset::JpegHigh => "JPEG — high quality (92)".into(),
            Preset::JpegSmall => "JPEG — small file (75)".into(),
            Preset::Png => "PNG".into(),
            Preset::Webp => "WebP (lossless)".into(),
            Preset::Bmp => "BMP".into(),
            Preset::Resize(n) => format!("JPEG, longest side {n} px"),
        }
    }

    pub fn target(self) -> nv_editor::convert::Out {
        use nv_editor::convert::Out;
        match self {
            Preset::JpegHigh => Out::Jpeg(92),
            Preset::JpegSmall | Preset::Resize(_) => Out::Jpeg(if matches!(self, Preset::Resize(_)) { 88 } else { 75 }),
            Preset::Png => Out::Png,
            Preset::Webp => Out::Webp,
            Preset::Bmp => Out::Bmp,
        }
    }

    pub fn max_edge(self) -> Option<u32> {
        if let Preset::Resize(n) = self { Some(n) } else { None }
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Open,
    Slideshow,
    Quick(QuickOp),
    EditWith(usize),
    ChooseProgram,
    SetUpPrograms,
    ShowLocation,
    Refresh,
    Shuffle,
    SelectItem,
    SelectAll,
    SelectNone,
    SelectInvert,
    CopyImage,
    CopyFile,
    CopyPath,
    Cut,
    Paste,
    CopyTo,
    MoveTo,
    Print,
    NewFolder,
    Rename,
    Delete,
    Compress,
    Convert(Preset),
    BatchRename,
    BatchConvert,
    BatchTimes,
    BatchStrip,
    Sort(SortKey),
    Show(Show),
    ToggleFolders,
    ToggleHidden,
    ToggleNames,
    View(ViewMode),
    PinTop,
    PinLeft,
    PinRight,
    Fullscreen,
    Help,
    OpenFolderInExplorer,
    ClearThumbs,
    ViewerSettings,
}

#[derive(Default)]
struct Builder {
    cmds: Vec<Cmd>,
}

impl Builder {
    fn item(&mut self, cmd: Cmd, text: impl Into<String>) -> Item {
        self.cmds.push(cmd);
        Item::item(self.cmds.len() as u32, text)
    }

    fn checked(&mut self, cmd: Cmd, text: impl Into<String>, on: bool) -> Item {
        self.cmds.push(cmd);
        Item::checked(self.cmds.len() as u32, text, on)
    }

    fn sub(&mut self, text: &str, items: Vec<Item>) -> Item {
        Item::Submenu { text: text.into(), items, enabled: true }
    }
}

impl App {
    /// The right-click menu, in the order the gallery's design asks for.
    pub fn context_menu(&mut self, x: i32, y: i32) {
        let picked: Vec<&Entry> = self.sel.entries(&self.listing);
        let n = self.sel.count(&self.listing).max(picked.len().min(1));
        let has = !picked.is_empty();
        let many = self.sel.count(&self.listing) >= 2;
        let any_file = picked.iter().any(|e| !e.is_dir());
        let any_pic = picked.iter().any(|e| e.viewable());
        let all_rewritable = !picked.is_empty() && picked.iter().all(|e| nv_editor::save::Format::of_extension(&e.ext).is_some());
        let paste_ok = nv_win::files::clipboard_files(self.hwnd).is_some();
        let externals: Vec<String> = self.settings.external.iter().map(|e| e.name.clone()).collect();
        let shuffled = self.listing.order.shuffle.is_some();
        let _ = n;
        let mut b = Builder::default();

        let mut menu: Vec<Item> = Vec::new();
        menu.push(b.item(Cmd::Open, "Open\tSpace").enabled(has));
        menu.push(b.item(Cmd::Slideshow, "Slideshow\tF8").enabled(any_pic));
        let quick: Vec<Item> = [QuickOp::RotateLeft, QuickOp::RotateRight, QuickOp::FlipH, QuickOp::FlipV].iter().map(|&q| b.item(Cmd::Quick(q), q.label())).collect();
        menu.push(Item::Submenu { text: "Quick edit".into(), items: quick, enabled: all_rewritable });
        let mut with: Vec<Item> = externals.iter().enumerate().map(|(i, name)| b.item(Cmd::EditWith(i), name.clone())).collect();
        if !with.is_empty() {
            with.push(Item::Separator);
        }
        with.push(b.item(Cmd::ChooseProgram, "Choose a program…"));
        with.push(b.item(Cmd::SetUpPrograms, "Set up the list…"));
        menu.push(Item::Submenu { text: "Edit with…".into(), items: with, enabled: any_file });
        menu.push(b.item(Cmd::ShowLocation, "Show file location\tCtrl+E").enabled(has));
        menu.push(Item::Separator);
        menu.push(b.item(Cmd::Refresh, "Refresh\tF5"));
        menu.push(b.checked(Cmd::Shuffle, "Shuffle\tCtrl+R", shuffled));
        let select = vec![
            b.item(Cmd::SelectItem, "Item (toggle)").enabled(has),
            b.item(Cmd::SelectAll, "All\tCtrl+A"),
            b.item(Cmd::SelectNone, "None\tEsc"),
            b.item(Cmd::SelectInvert, "Invert"),
        ];
        menu.push(b.sub("Select", select));
        menu.push(Item::Separator);
        menu.push(b.item(Cmd::CopyImage, "Copy image").enabled(any_pic));
        menu.push(b.item(Cmd::CopyFile, "Copy file\tCtrl+C").enabled(has));
        menu.push(b.item(Cmd::CopyPath, "Copy path\tCtrl+Shift+C").enabled(has));
        menu.push(b.item(Cmd::Cut, "Cut\tCtrl+X").enabled(has));
        menu.push(b.item(Cmd::Paste, "Paste\tCtrl+V").enabled(paste_ok));
        menu.push(b.item(Cmd::CopyTo, "Copy to…\tF7").enabled(has));
        menu.push(Item::Separator);
        menu.push(b.item(Cmd::MoveTo, "Move to…\tF6").enabled(has));
        menu.push(Item::Separator);
        menu.push(b.item(Cmd::Print, "Print\tCtrl+P").enabled(any_pic));
        menu.push(b.item(Cmd::NewFolder, "New folder\tCtrl+N"));
        menu.push(b.item(Cmd::Rename, "Rename\tF2").enabled(has && !many));
        menu.push(b.item(Cmd::Delete, "Delete\tDel").enabled(has));
        if many {
            menu.push(b.item(Cmd::Compress, "Compress (.zip)"));
        }
        let convert: Vec<Item> = [Preset::JpegHigh, Preset::JpegSmall, Preset::Png, Preset::Webp, Preset::Bmp, Preset::Resize(1920), Preset::Resize(1280), Preset::Resize(800)]
            .iter()
            .map(|&p| b.item(Cmd::Convert(p), p.label()))
            .collect();
        menu.push(Item::Submenu { text: "Quick convert".into(), items: convert, enabled: any_pic });
        if many {
            menu.push(Item::Separator);
            let batch = vec![
                b.item(Cmd::BatchRename, "Rename…"),
                b.item(Cmd::BatchConvert, "Convert…"),
                b.item(Cmd::BatchTimes, "Change time stamp…"),
                b.item(Cmd::BatchStrip, "Remove metadata…"),
            ];
            menu.push(b.sub("Batch", batch));
        }
        if let Some(id) = popup(self.hwnd, x, y, &menu) {
            if let Some(cmd) = b.cmds.get(id as usize - 1).cloned() {
                self.run_cmd(cmd);
            }
        }
    }

    /// Sort / Show / Tools drop-downs under their buttons.
    pub fn toolbar_menu(&mut self, btn: Btn) {
        let Some(rc) = self.chrome.rect_of(btn) else { return };
        let (x, y) = (rc[0] as i32, rc[3] as i32);
        let mut b = Builder::default();
        let mut menu: Vec<Item> = Vec::new();
        match btn {
            Btn::Sort => {
                let o = self.listing.order;
                for k in SortKey::ALL {
                    menu.push(b.checked(Cmd::Sort(k), format!("{}{}", k.label(), if o.shuffle.is_none() && o.key == k { if o.descending { "  (descending — click again for ascending)" } else { "  (ascending — click again to reverse)" } } else { "" }), o.shuffle.is_none() && o.key == k));
                }
                menu.push(Item::Separator);
                menu.push(b.checked(Cmd::Shuffle, "Shuffle\tCtrl+R", o.shuffle.is_some()));
            }
            Btn::Show => {
                for s in Show::ALL_CHOICES {
                    menu.push(b.checked(Cmd::Show(s), s.label(), self.listing.show == s));
                }
                menu.push(Item::Separator);
                menu.push(b.checked(Cmd::ToggleFolders, "Folders", self.listing.folders));
                menu.push(b.checked(Cmd::ToggleHidden, "Hidden files", self.persist.hidden));
            }
            Btn::Tools => {
                let batch = vec![
                    b.item(Cmd::BatchRename, "Rename…"),
                    b.item(Cmd::BatchConvert, "Convert…"),
                    b.item(Cmd::BatchTimes, "Change time stamp…"),
                    b.item(Cmd::BatchStrip, "Remove metadata…"),
                ];
                menu.push(b.sub("Batch (selected files)", batch));
                menu.push(b.item(Cmd::Compress, "Compress selected (.zip)"));
                menu.push(Item::Separator);
                for m in ViewMode::ALL {
                    menu.push(b.checked(Cmd::View(m), m.label(), self.mode == m));
                }
                menu.push(b.checked(Cmd::ToggleNames, "File names under thumbnails", self.persist.names));
                menu.push(Item::Separator);
                menu.push(b.checked(Cmd::PinTop, "Keep the toolbar open\tCtrl+T", self.panels.pin_top));
                menu.push(b.checked(Cmd::PinLeft, "Keep folders open\tCtrl+B", self.panels.pin_left));
                menu.push(b.checked(Cmd::PinRight, "Keep details open\tCtrl+I", self.panels.pin_right));
                menu.push(b.checked(Cmd::Fullscreen, "Fullscreen\tF11", nv_win::is_fullscreen()));
                menu.push(Item::Separator);
                menu.push(b.item(Cmd::OpenFolderInExplorer, "Open this folder in Explorer"));
                menu.push(b.item(Cmd::ClearThumbs, "Rebuild thumbnails of this folder"));
                menu.push(b.item(Cmd::ViewerSettings, "NuViewer settings…"));
                menu.push(b.item(Cmd::Help, "Keys and mouse\tF1"));
            }
            _ => return,
        }
        if let Some(id) = popup(self.hwnd, x, y, &menu) {
            if let Some(cmd) = b.cmds.get(id as usize - 1).cloned() {
                self.run_cmd(cmd);
            }
        }
    }

    pub fn run_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Open => self.open_selected(false),
            Cmd::Slideshow => self.open_selected(true),
            Cmd::Quick(op) => self.quick_edit(op),
            Cmd::EditWith(i) => self.edit_with(Some(i)),
            Cmd::ChooseProgram => self.edit_with(None),
            Cmd::SetUpPrograms => self.launch_viewer_settings(),
            Cmd::ShowLocation => self.show_location(),
            Cmd::Refresh => self.refresh(Vec::new()),
            Cmd::Shuffle => self.toggle_shuffle(),
            Cmd::SelectItem => {
                if let Some(c) = self.sel.cursor {
                    self.sel.toggle(&self.listing, c);
                    self.update_title();
                    nv_win::invalidate(self.hwnd);
                }
            }
            Cmd::SelectAll => {
                self.sel.all(&self.listing);
                self.update_title();
                nv_win::invalidate(self.hwnd);
            }
            Cmd::SelectNone => {
                self.sel.clear();
                self.update_title();
                nv_win::invalidate(self.hwnd);
            }
            Cmd::SelectInvert => {
                self.sel.invert(&self.listing);
                self.update_title();
                nv_win::invalidate(self.hwnd);
            }
            Cmd::CopyImage => self.copy_image(),
            Cmd::CopyFile => self.copy_files(false),
            Cmd::CopyPath => self.copy_path(),
            Cmd::Cut => self.copy_files(true),
            Cmd::Paste => self.paste(),
            Cmd::CopyTo => self.transfer_selected(false),
            Cmd::MoveTo => self.transfer_selected(true),
            Cmd::Print => self.print_selected(),
            Cmd::NewFolder => self.new_folder(),
            Cmd::Rename => self.start_rename(),
            Cmd::Delete => self.delete_selected(false),
            Cmd::Compress => self.compress_selected(),
            Cmd::Convert(p) => self.quick_convert(p),
            Cmd::BatchRename => self.batch_rename(),
            Cmd::BatchConvert => self.batch_convert(),
            Cmd::BatchTimes => self.batch_times(),
            Cmd::BatchStrip => self.batch_strip(),
            Cmd::Sort(k) => self.set_sort(k),
            Cmd::Show(s) => self.set_show(s),
            Cmd::ToggleFolders => {
                self.listing.folders = !self.listing.folders;
                self.rearrange_keeping_focus();
                self.save_state();
            }
            Cmd::ToggleHidden => {
                self.persist.hidden = !self.persist.hidden;
                self.refresh(Vec::new());
                self.save_state();
            }
            Cmd::ToggleNames => {
                self.persist.names = !self.persist.names;
                self.clamp_scroll();
                self.request_thumbs();
                nv_win::invalidate(self.hwnd);
                self.save_state();
            }
            Cmd::View(m) => self.set_mode(m),
            Cmd::PinTop => {
                self.panels.pin_top = !self.panels.pin_top;
                self.panels_changed();
            }
            Cmd::PinLeft => {
                self.panels.pin_left = !self.panels.pin_left;
                self.panels_changed();
            }
            Cmd::PinRight => {
                self.panels.pin_right = !self.panels.pin_right;
                self.panels_changed();
            }
            Cmd::Fullscreen => crate::draw::toggle_fullscreen(self),
            Cmd::Help => {
                self.help = !self.help;
                nv_win::invalidate(self.hwnd);
            }
            Cmd::OpenFolderInExplorer => {
                let _ = std::process::Command::new("explorer.exe").arg(&self.listing.dir).spawn();
            }
            Cmd::ClearThumbs => self.rebuild_thumbs(),
            Cmd::ViewerSettings => self.launch_viewer_settings(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_numbers_map_back_to_commands() {
        let mut b = Builder::default();
        let a = b.item(Cmd::Open, "Open");
        let c = b.checked(Cmd::Shuffle, "Shuffle", true);
        match (a, c) {
            (Item::Item { id: 1, .. }, Item::Item { id: 2, checked: true, .. }) => {}
            _ => panic!("ids must start at 1 and count up (0 means 'dismissed')"),
        }
        assert!(matches!(b.cmds[0], Cmd::Open) && matches!(b.cmds[1], Cmd::Shuffle));
    }

    #[test]
    fn presets_describe_themselves() {
        assert_eq!(Preset::Resize(1920).max_edge(), Some(1920));
        assert_eq!(Preset::Png.max_edge(), None);
        assert!(Preset::JpegHigh.label().contains("92"));
        assert_eq!(Preset::Bmp.target().ext(), "bmp");
    }
}
