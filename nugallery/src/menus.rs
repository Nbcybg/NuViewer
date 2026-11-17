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
    QuickPng,
    QuickJpeg,
    Open,
    Slideshow,
    Quick(QuickOp),
    NuEditor,
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
    Extract,
    MakePdf,
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
    RawLook,
    View(ViewMode),
    PinTop,
    PinLeft,
    PinRight,
    Fullscreen,
    Borderless,
    PickColour,
    Palette,
    Help,
    OpenFolderInExplorer,
    /// Open the file with a program Windows offers for it (`Handler::id`).
    OpenWithApp(String),
    /// Windows' own "choose an app" window.
    OpenWithChoose,
    Properties,
    Settings,
    ClearThumbs,
    ViewerSettings,
    Undo,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleSequences,
    Importer,
    Exit,
}

/// The menu bar's labels (`&` marks the Alt+letter).
pub const BAR_LABELS: [&str; 5] = ["&File", "&Edit", "&View", "&Tools", "&Help"];

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
    /// "Open with": the programs Windows lists for the first selected file, and "Choose another app".
    fn open_with_menu(&self, b: &mut Builder, picked: &[&Entry]) -> Item {
        let first = picked.iter().find(|e| !e.is_dir()).map(|e| e.path.clone());
        let mut items: Vec<Item> = Vec::new();
        if let Some(p) = &first {
            for h in nv_win::openwith::handlers(p).into_iter().take(14) {
                items.push(b.item(Cmd::OpenWithApp(h.id.clone()), h.name));
            }
        }
        if !items.is_empty() {
            items.push(Item::Separator);
        }
        items.push(b.item(Cmd::OpenWithChoose, "Choose another app…"));
        Item::Submenu { text: "Open with".into(), items, enabled: first.is_some() }
    }

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
        let ow = self.open_with_menu(&mut b, &picked);
        menu.push(ow);
        let quick: Vec<Item> = [QuickOp::RotateLeft, QuickOp::RotateRight, QuickOp::FlipH, QuickOp::FlipV].iter().map(|&q| b.item(Cmd::Quick(q), q.label())).collect();
        menu.push(b.item(Cmd::NuEditor, "Edit in NuEditor	F4").enabled(any_pic && !many));
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
        menu.push(b.checked(Cmd::ToggleSequences, "Group image sequences\tCtrl+G", self.settings.sequences));
        let select = vec![
            b.item(Cmd::SelectItem, "Item (toggle)").enabled(has),
            b.item(Cmd::SelectAll, "All\tCtrl+A"),
            b.item(Cmd::SelectNone, "None\tEsc"),
            b.item(Cmd::SelectInvert, "Invert"),
        ];
        menu.push(b.sub("Select", select));
        menu.push(Item::Separator);
        menu.push(b.item(Cmd::CopyImage, "Copy picture").enabled(any_pic));
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
        menu.push(b.item(Cmd::Undo, format!("{}\tCtrl+Z", self.undo_label())).enabled(!self.undo.is_empty()));
        menu.push(b.item(Cmd::Properties, "Properties"));
        let any_archive = picked.iter().any(|e| nv_core::archive_tools::is_archive(&e.path));
        menu.push(b.item(Cmd::Compress, "Compress…").enabled(has));
        menu.push(b.item(Cmd::Extract, "Unpack here").enabled(any_archive));
        menu.push(b.item(Cmd::MakePdf, "Make a PDF").enabled(true));
        let convert: Vec<Item> = [Preset::JpegHigh, Preset::JpegSmall, Preset::Png, Preset::Webp, Preset::Bmp, Preset::Resize(1920), Preset::Resize(1280), Preset::Resize(800)]
            .iter()
            .map(|&p| b.item(Cmd::Convert(p), p.label()))
            .collect();
        menu.push(Item::Submenu { text: "Quick convert".into(), items: convert, enabled: any_pic });
        // One click, no questions: next to each file, same name, nothing lost (PNG is lossless, JPEG is quality 100 at full colour resolution).
        menu.push(b.item(Cmd::QuickPng, nv_core::i18n::t("menu.convert_png")).enabled(any_pic));
        menu.push(b.item(Cmd::QuickJpeg, nv_core::i18n::t("menu.convert_jpeg")).enabled(any_pic));
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
        // How this folder is shown, and the program's settings.
        menu.push(Item::Separator);
        let mut view: Vec<Item> = ViewMode::ALL.iter().map(|&v| b.checked(Cmd::View(v), v.label(), self.mode == v)).collect();
        view.push(Item::Separator);
        view.push(b.item(Cmd::ZoomIn, "Larger thumbnails\tCtrl +"));
        view.push(b.item(Cmd::ZoomOut, "Smaller thumbnails\tCtrl −"));
        view.push(b.item(Cmd::ZoomReset, "Default size\tCtrl 0"));
        view.push(Item::Separator);
        view.push(b.checked(Cmd::ToggleFolders, "Folders", self.listing.folders));
        view.push(b.checked(Cmd::ToggleHidden, "Hidden files", self.persist.hidden));
        view.push(b.checked(Cmd::ToggleNames, "File names under thumbnails", self.persist.names));
        menu.push(b.sub("View", view));
        let sort = self.sort_items(&mut b);
        menu.push(b.sub("Sort by", sort));
        let show: Vec<Item> = Show::ALL_CHOICES.iter().map(|&s| b.checked(Cmd::Show(s), s.label(), self.listing.show == s)).collect();
        menu.push(b.sub("Show only", show));
        menu.push(b.item(Cmd::Settings, "Settings…"));
        if let Some(id) = popup(self.hwnd, x, y, &menu) {
            if let Some(cmd) = b.cmds.get(id as usize - 1).cloned() {
                self.run_cmd(cmd);
            }
        }
    }

    // ---- the menu bar --------------------------------------------------------------------------------

    /// A left press: on a label it opens that menu; elsewhere it lets go of a keyboard-opened bar. True if the press was the bar's.
    pub fn menubar_press(&mut self, x: i32, y: i32) -> bool {
        let bar_h = self.menubar.height(self.dpi);
        if self.menubar.visible() && (y as f32) < bar_h {
            if let Some(n) = self.menubar.nav_hit(x as f32, y as f32, self.dpi) {
                self.menubar.close();
                match n {
                    // "<": the program before this one is the viewer: the picked picture opens there; a folder goes up one level.
                    nv_win::menubar::Nav::Back => {
                        let picked_picture = self.sel.entries(&self.listing).first().is_some_and(|e| !e.is_dir() && e.viewable());
                        if picked_picture {
                            self.open_selected(false);
                        } else {
                            self.go_up();
                        }
                    }
                    nv_win::menubar::Nav::Close => self.exit(),
                }
                nv_win::invalidate(self.hwnd);
                return true;
            }
            match self.menubar.hit(x as f32, y as f32) {
                Some(i) => self.open_menubar_menu(i),
                None => {
                    self.menubar.close();
                    nv_win::invalidate(self.hwnd);
                }
            }
            return true;
        }
        if self.menubar.keyboard() {
            self.menubar.close();
            nv_win::invalidate(self.hwnd);
        }
        false
    }

    /// A key: Alt+letter opens a menu; while the bar has the keyboard its arrows / Enter / Esc are its own. True if used.
    pub fn menubar_key(&mut self, vk: u32) -> bool {
        if self.active.is_some() {
            return false;
        }
        let m = nv_win::modifiers();
        if m.alt && !m.ctrl {
            if let Some(i) = self.menubar.key(vk, true) {
                self.open_menubar_menu(i);
                return true;
            }
            return false; // Alt+arrows, Alt+Enter: the gallery's own
        }
        if self.menubar.keyboard() {
            if let Some(i) = self.menubar.key(vk, false) {
                self.open_menubar_menu(i);
            }
            nv_win::invalidate(self.hwnd);
            return true;
        }
        false
    }

    pub fn open_menubar_menu(&mut self, i: usize) {
        let mut b = Builder::default();
        let menu = self.bar_menu(i, &mut b);
        nv_win::invalidate(self.hwnd);
        let picked = self.menubar.run(self.hwnd, i, &menu);
        nv_win::invalidate(self.hwnd);
        if let Some(id) = picked {
            if let Some(cmd) = b.cmds.get(id as usize - 1).cloned() {
                self.run_cmd(cmd);
            }
        }
    }

    fn sort_items(&self, b: &mut Builder) -> Vec<Item> {
        let o = self.listing.order;
        let mut v: Vec<Item> = SortKey::ALL.iter().map(|&k| b.checked(Cmd::Sort(k), k.label().to_string(), o.shuffle.is_none() && o.key == k)).collect();
        v.push(Item::Separator);
        v.push(b.checked(Cmd::Shuffle, "Shuffle\tCtrl+R", o.shuffle.is_some()));
        v
    }

    /// The entries under menu bar label `i` (File, Edit, View, Tools, Help).
    fn bar_menu(&self, i: usize, b: &mut Builder) -> Vec<Item> {
        let picked: Vec<&Entry> = self.sel.entries(&self.listing);
        let has = !picked.is_empty();
        let many = self.sel.count(&self.listing) >= 2;
        let any_file = picked.iter().any(|e| !e.is_dir());
        let any_pic = picked.iter().any(|e| e.viewable());
        let all_rewritable = !picked.is_empty() && picked.iter().all(|e| nv_editor::save::Format::of_extension(&e.ext).is_some());
        let paste_ok = nv_win::files::clipboard_files(self.hwnd).is_some();
        let externals: Vec<String> = self.settings.external.iter().map(|e| e.name.clone()).collect();
        let mut m: Vec<Item> = Vec::new();
        match i {
            0 => {
                m.push(b.item(Cmd::Open, "Open\tSpace").enabled(has));
                m.push(b.item(Cmd::Slideshow, "Slideshow\tF8").enabled(any_pic));
                let ow = self.open_with_menu(b, &picked);
                m.push(ow);
                m.push(b.item(Cmd::NuEditor, "Edit in NuEditor\tF4").enabled(any_pic && !many));
                let mut with: Vec<Item> = externals.iter().enumerate().map(|(k, name)| b.item(Cmd::EditWith(k), name.clone())).collect();
                if !with.is_empty() {
                    with.push(Item::Separator);
                }
                with.push(b.item(Cmd::ChooseProgram, "Choose a program…"));
                with.push(b.item(Cmd::SetUpPrograms, "Set up the list…"));
                m.push(Item::Submenu { text: "Edit with…".into(), items: with, enabled: any_file });
                m.push(b.item(Cmd::ShowLocation, "Show file location\tCtrl+E").enabled(has));
                m.push(Item::Separator);
                m.push(b.item(Cmd::NewFolder, "New folder\tCtrl+N"));
                m.push(b.item(Cmd::Rename, "Rename\tF2").enabled(has && !many));
                m.push(b.item(Cmd::Delete, "Delete\tDel").enabled(has));
                m.push(b.item(Cmd::Print, "Print\tCtrl+P").enabled(any_pic));
                m.push(b.item(Cmd::Properties, "Properties"));
                m.push(Item::Separator);
                m.push(b.item(Cmd::Settings, "Settings…"));
                m.push(b.item(Cmd::Exit, "Close"));
            }
            1 => {
                m.push(b.item(Cmd::Undo, format!("{}\tCtrl+Z", self.undo_label())).enabled(!self.undo.is_empty()));
                m.push(Item::Separator);
                m.push(b.item(Cmd::Cut, "Cut\tCtrl+X").enabled(has));
                m.push(b.item(Cmd::CopyFile, "Copy file\tCtrl+C").enabled(has));
                m.push(b.item(Cmd::CopyImage, "Copy picture").enabled(any_pic));
                m.push(b.item(Cmd::CopyPath, "Copy path\tCtrl+Shift+C").enabled(has));
                m.push(b.item(Cmd::Paste, "Paste\tCtrl+V").enabled(paste_ok));
                m.push(Item::Separator);
                m.push(b.item(Cmd::CopyTo, "Copy to…\tF7").enabled(has));
                m.push(b.item(Cmd::MoveTo, "Move to…\tF6").enabled(has));
                m.push(Item::Separator);
                let select = vec![b.item(Cmd::SelectItem, "Item (toggle)").enabled(has), b.item(Cmd::SelectAll, "All\tCtrl+A"), b.item(Cmd::SelectNone, "None\tEsc"), b.item(Cmd::SelectInvert, "Invert")];
                m.push(b.sub("Select", select));
            }
            2 => {
                for v in ViewMode::ALL {
                    m.push(b.checked(Cmd::View(v), v.label(), self.mode == v));
                }
                m.push(Item::Separator);
                m.push(b.item(Cmd::ZoomIn, "Larger thumbnails (fewer columns)\tCtrl +"));
                m.push(b.item(Cmd::ZoomOut, "Smaller thumbnails (more columns)\tCtrl −"));
                m.push(b.item(Cmd::ZoomReset, "Default size\tCtrl 0"));
                m.push(Item::Separator);
                let sort = self.sort_items(b);
                m.push(b.sub("Sort by", sort));
                let show: Vec<Item> = Show::ALL_CHOICES.iter().map(|&s| b.checked(Cmd::Show(s), s.label(), self.listing.show == s)).collect();
                m.push(b.sub("Show only", show));
                m.push(b.checked(Cmd::ToggleFolders, "Folders", self.listing.folders));
                m.push(b.checked(Cmd::ToggleHidden, "Hidden files", self.persist.hidden));
                m.push(b.checked(Cmd::ToggleNames, "File names under thumbnails", self.persist.names));
                m.push(b.checked(Cmd::ToggleSequences, "Group image sequences\tCtrl+G", self.settings.sequences));
                m.push(b.checked(Cmd::RawLook, "RAW: camera look (not sensor data)\tShift+R", self.settings.raw_mode.is_camera_look()));
                m.push(Item::Separator);
                m.push(b.checked(Cmd::PinTop, "Keep the toolbar open\tCtrl+T", self.panels.pin_top));
                m.push(b.checked(Cmd::PinLeft, "Keep folders open\tCtrl+B", self.panels.pin_left));
                m.push(b.checked(Cmd::PinRight, "Keep details open\tCtrl+I", self.panels.pin_right));
                m.push(b.checked(Cmd::Fullscreen, "Fullscreen\tF11", nv_win::is_fullscreen()));
                m.push(b.checked(Cmd::Borderless, "Borderless window\tShift+F11", nv_win::is_borderless()));
                m.push(Item::Separator);
                m.push(b.item(Cmd::Refresh, "Refresh\tF5"));
            }
            3 => {
                let batch = vec![b.item(Cmd::BatchRename, "Rename…"), b.item(Cmd::BatchConvert, "Convert…"), b.item(Cmd::BatchTimes, "Change time stamp…"), b.item(Cmd::BatchStrip, "Remove metadata…")];
                m.push(Item::Submenu { text: "Batch (selected files)".into(), items: batch, enabled: has });
                let any_archive = picked.iter().any(|e| nv_core::archive_tools::is_archive(&e.path));
                m.push(b.item(Cmd::Compress, "Compress selected…").enabled(has));
                m.push(b.item(Cmd::Extract, "Unpack the selected archive").enabled(any_archive));
                m.push(b.item(Cmd::MakePdf, "Make a PDF of the selected pictures or folder"));
                let convert: Vec<Item> = [Preset::JpegHigh, Preset::JpegSmall, Preset::Png, Preset::Webp, Preset::Bmp, Preset::Resize(1920), Preset::Resize(1280), Preset::Resize(800)].iter().map(|&p| b.item(Cmd::Convert(p), p.label())).collect();
                m.push(Item::Submenu { text: "Quick convert".into(), items: convert, enabled: any_pic });
                let quick: Vec<Item> = [QuickOp::RotateLeft, QuickOp::RotateRight, QuickOp::FlipH, QuickOp::FlipV].iter().map(|&q| b.item(Cmd::Quick(q), q.label())).collect();
                m.push(Item::Submenu { text: "Quick edit".into(), items: quick, enabled: all_rewritable });
                m.push(Item::Separator);
                m.push(b.item(Cmd::Importer, "Import photos… (NuImporter, mock-up)"));
                m.push(b.checked(Cmd::PickColour, "Pick a colour from the preview\tK", self.pick.on));
                m.push(b.checked(Cmd::Palette, "Palette of the preview picture\tCtrl+K", self.pick.palette_on));
                m.push(Item::Separator);
                m.push(b.item(Cmd::OpenFolderInExplorer, "Open this folder in Explorer"));
                m.push(b.item(Cmd::ClearThumbs, "Rebuild thumbnails of this folder"));
                m.push(b.item(Cmd::ViewerSettings, "NuViewer settings…"));
            }
            _ => m.push(b.item(Cmd::Help, "Keys, mouse and credits\tF1")),
        }
        m
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
                menu.push(b.checked(Cmd::ToggleSequences, "Group image sequences\tCtrl+G", self.settings.sequences));
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
                menu.push(b.checked(Cmd::Borderless, "Borderless window\tShift+F11", nv_win::is_borderless()));
                menu.push(Item::Separator);
                menu.push(b.item(Cmd::OpenFolderInExplorer, "Open this folder in Explorer"));
                menu.push(b.item(Cmd::ClearThumbs, "Rebuild thumbnails of this folder"));
                menu.push(b.item(Cmd::Importer, "Import photos… (NuImporter, mock-up)"));
                menu.push(b.checked(Cmd::PickColour, "Pick a colour from the preview\tK", self.pick.on));
                menu.push(b.checked(Cmd::Palette, "Palette of the preview picture\tCtrl+K", self.pick.palette_on));
                menu.push(b.item(Cmd::ViewerSettings, "NuViewer settings…"));
                menu.push(b.item(Cmd::Help, "Keys, mouse and credits\tF1"));
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
            Cmd::NuEditor => self.launch_editor(),
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
            Cmd::Extract => self.extract_selected(),
            Cmd::MakePdf => self.make_pdf(),
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
            Cmd::RawLook => self.toggle_raw_look(),
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
            Cmd::Borderless => crate::draw::toggle_borderless(self),
            Cmd::PickColour => self.toggle_pick(),
            Cmd::Palette => self.toggle_palette(),
            Cmd::Help => {
                self.help = !self.help;
                nv_win::invalidate(self.hwnd);
            }
            Cmd::OpenWithApp(id) => self.open_with_app(&id),
            Cmd::OpenWithChoose => self.open_with_choose(),
            Cmd::Properties => self.show_properties(),
            Cmd::Settings => self.launch_viewer_settings(),
            Cmd::OpenFolderInExplorer => {
                let _ = std::process::Command::new("explorer.exe").arg(&self.listing.dir).spawn();
            }
            Cmd::ClearThumbs => self.rebuild_thumbs(),
            Cmd::ViewerSettings => self.launch_viewer_settings(),
            Cmd::Undo => self.undo_last(),
            Cmd::ZoomIn => self.zoom_cells(1.0),
            Cmd::ZoomOut => self.zoom_cells(-1.0),
            Cmd::ZoomReset => self.reset_cell(),
            Cmd::ToggleSequences => self.toggle_sequences(),
            Cmd::Importer => self.launch_importer(),
            Cmd::Exit => self.exit(),
            Cmd::QuickPng => self.quick_to(nv_core::quick::Target::Png),
            Cmd::QuickJpeg => self.quick_to(nv_core::quick::Target::Jpeg),
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
