//! What installing and uninstalling actually do (no UI). Per-user only: files under %LOCALAPPDATA%\Programs,
//! registry under HKEY_CURRENT_USER, shortcuts in the user's Start menu / Desktop. No administrator rights.

use nv_win::registry::{self, Layout};
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, SHGetKnownFolderPath, ShellLink, FOLDERID_Desktop, FOLDERID_Programs, KF_FLAG_DEFAULT};

pub const APP_EXE: &str = "nuviewer.exe";
pub const GALLERY_EXE: &str = "nugallery.exe";
pub const EDITOR_EXE: &str = "nueditor.exe";
pub const IMPORTER_EXE: &str = "nuimporter.exe";
pub const QUICK_EXE: &str = "nuquick.exe";
pub const NOTE_EXE: &str = "nunote.exe";
pub const UNINSTALLER: &str = "uninstall.exe";
const SHORTCUT: &str = "NuViewer.lnk";
const SHORTCUT_TYPES: &str = "NuViewer file types.lnk";
const SHORTCUT_GALLERY: &str = "NuGallery.lnk";
const SHORTCUT_EDITOR: &str = "NuEditor.lnk";
const SHORTCUT_IMPORTER: &str = "NuImporter.lnk";
const SHORTCUT_NOTE: &str = "NuNote.lnk";
const SHORTCUT_UNINSTALL: &str = "Uninstall NuViewer.lnk";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));
const PAYLOAD_LEN: &str = include_str!(concat!(env!("OUT_DIR"), "/payload.len"));

/// The files packed into this setup (nuviewer.exe first, then nugallery.exe and nueditor.exe when they were built), unpacked: (name, bytes).
pub fn payload() -> Result<Vec<(String, Vec<u8>)>, String> {
    if PAYLOAD.is_empty() {
        return Err("this setup was built without nuviewer.exe inside it".into());
    }
    let want: usize = PAYLOAD_LEN.trim().parse().unwrap_or(0);
    let data = miniz_oxide::inflate::decompress_to_vec(PAYLOAD).map_err(|e| format!("the setup data is damaged ({e:?})"))?;
    if data.len() != want {
        return Err("the setup data is damaged (size mismatch)".into());
    }
    unpack(&data)
}

/// Split the blob made by build.rs: per file a name length (u8), the name, a data length (u64 LE) and the data.
fn unpack(mut data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let damaged = || "the setup data is damaged (file table)".to_string();
    let mut out = Vec::new();
    while !data.is_empty() {
        let n = *data.first().ok_or_else(damaged)? as usize;
        let name = data.get(1..1 + n).ok_or_else(damaged)?;
        let len = u64::from_le_bytes(data.get(1 + n..9 + n).ok_or_else(damaged)?.try_into().map_err(|_| damaged())?) as usize;
        let body = data.get(9 + n..9 + n + len).ok_or_else(damaged)?;
        out.push((String::from_utf8_lossy(name).into_owned(), body.to_vec()));
        data = &data[9 + n + len..];
    }
    Ok(out)
}

/// Where things go. `real()` is the user's actual profile; tests and the `/regprefix` switch redirect everything.
pub struct Env {
    /// None = the real registry; Some(prefix) = a scratch HKCU subkey holding Classes, NuViewer, RegisteredApplications, Uninstall.
    pub reg_prefix: Option<String>,
    pub start_menu: PathBuf,
    pub desktop: PathBuf,
    /// NuViewer's own data (thumbnail cache, crash log, backups of edited pictures), removed on uninstall as `Purge` says. None leaves it alone (tests).
    pub data_dir: Option<PathBuf>,
    /// The settings (`%APPDATA%\NuViewer`: settings, presets, languages), removed on uninstall only when asked. None leaves it alone (tests).
    pub config_dir: Option<PathBuf>,
}

fn known_folder(id: &windows::core::GUID, fallback: &str) -> PathBuf {
    unsafe {
        match SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
                PathBuf::from(s)
            }
            Err(_) => PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(fallback),
        }
    }
}

impl Env {
    pub fn real() -> Env {
        Env {
            reg_prefix: None,
            start_menu: known_folder(&FOLDERID_Programs, "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs"),
            desktop: known_folder(&FOLDERID_Desktop, "Desktop"),
            data_dir: std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("NuViewer")),
            config_dir: std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("NuViewer")),
        }
    }

    pub fn layout(&self) -> Layout {
        match &self.reg_prefix {
            Some(p) => Layout::sandbox(p),
            None => Layout::real(),
        }
    }

    fn uninstall_key(&self) -> String {
        match &self.reg_prefix {
            Some(p) => format!("{p}\\Uninstall\\NuViewer"),
            None => "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\NuViewer".into(),
        }
    }
}

pub fn default_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
    base.join("Programs").join("NuViewer")
}

/// The folder of an existing installation (from its Add/Remove Programs entry), if any.
pub fn existing_install(env: &Env) -> Option<PathBuf> {
    registry::read_string(&env.uninstall_key(), Some("InstallLocation")).map(PathBuf::from).filter(|p| p.join(APP_EXE).is_file())
}

pub struct Options {
    pub dir: PathBuf,
    /// Extensions with dots to register for.
    pub exts: Vec<String>,
    pub start_menu: bool,
    pub desktop: bool,
}

/// Ask a running NuViewer, NuGallery and NuEditor to close (an upgrade can't replace a running exe) and wait briefly.
fn close_running_viewer() {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    for _ in 0..30 {
        let mut any = false;
        for class in [w!("NuViewerWindow"), w!("NuGalleryWindow"), w!("NuEditorWindow")] {
            if let Ok(h) = unsafe { FindWindowW(class, None) } {
                if !h.is_invalid() {
                    any = true;
                    unsafe {
                        let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
        if !any {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn create_shortcut(lnk: &Path, target: &Path, args: &str, desc: &str) -> Result<(), String> {
    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { std::os::windows::ffi::OsStrExt::encode_wide(s).chain(std::iter::once(0)).collect() };
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| format!("shortcut: {e}"))?;
        let t = wide(target.as_os_str());
        link.SetPath(PCWSTR(t.as_ptr())).map_err(|e| e.to_string())?;
        link.SetIconLocation(PCWSTR(t.as_ptr()), 0).map_err(|e| e.to_string())?;
        let a = wide(std::ffi::OsStr::new(args));
        link.SetArguments(PCWSTR(a.as_ptr())).map_err(|e| e.to_string())?;
        let d = wide(std::ffi::OsStr::new(desc));
        link.SetDescription(PCWSTR(d.as_ptr())).map_err(|e| e.to_string())?;
        if let Some(dir) = target.parent() {
            let wd = wide(dir.as_os_str());
            link.SetWorkingDirectory(PCWSTR(wd.as_ptr())).map_err(|e| e.to_string())?;
        }
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        if let Some(parent) = lnk.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let l = wide(lnk.as_os_str());
        file.Save(PCWSTR(l.as_ptr()), true).map_err(|e| format!("couldn't save {}: {e}", lnk.display()))
    }
}

/// Copy the files, register the file types, create shortcuts and the Add/Remove Programs entry.
/// `files` are the unpacked program files (nuviewer.exe first); `self_exe` is copied next to them as the uninstaller.
/// Returns the installed nuviewer.exe.
pub fn install(env: &Env, opts: &Options, files: &[(String, Vec<u8>)], self_exe: &Path) -> Result<PathBuf, String> {
    if opts.dir.parent().is_none() || !opts.dir.is_absolute() {
        return Err(format!("\"{}\" isn't a usable install folder (pick a folder, not a whole drive).", opts.dir.display()));
    }
    close_running_viewer();
    std::fs::create_dir_all(&opts.dir).map_err(|e| format!("couldn't create {}: {e}", opts.dir.display()))?;
    let exe = opts.dir.join(APP_EXE);
    write_files(&opts.dir, files)?;
    let has_gallery = files.iter().any(|(n, _)| n == GALLERY_EXE);
    let uninstaller = opts.dir.join(UNINSTALLER);
    if self_exe != uninstaller {
        std::fs::copy(self_exe, &uninstaller).map_err(|e| format!("couldn't write {}: {e}", uninstaller.display()))?;
    }

    let layout = env.layout();
    let all = nv_win::assoc::all_exts();
    registry::apply(&layout, &exe, &all, &opts.exts)?;

    // Shortcuts: a Start menu entry is always useful for "Open with"/search; the file-types one reopens the chooser.
    if opts.start_menu {
        create_shortcut(&env.start_menu.join(SHORTCUT), &exe, "", "Fast, lightweight image viewer")?;
        create_shortcut(&env.start_menu.join(SHORTCUT_TYPES), &exe, "--associate", "Choose which image formats open with NuViewer")?;
        let has = |name: &str| files.iter().any(|(n, _)| n == name);
        if has_gallery {
            create_shortcut(&env.start_menu.join(SHORTCUT_GALLERY), &opts.dir.join(GALLERY_EXE), "", "Browse folders as thumbnails")?;
        }
        if has(EDITOR_EXE) {
            create_shortcut(&env.start_menu.join(SHORTCUT_EDITOR), &opts.dir.join(EDITOR_EXE), "", "Edit a picture: light, colour, detail, crop, heal")?;
        }
        if has(IMPORTER_EXE) {
            create_shortcut(&env.start_menu.join(SHORTCUT_IMPORTER), &opts.dir.join(IMPORTER_EXE), "", "Import photos from a card, a phone or the screen")?;
        }
        if has(NOTE_EXE) {
            create_shortcut(&env.start_menu.join(SHORTCUT_NOTE), &opts.dir.join(NOTE_EXE), "", "Notepad: text, Markdown, JSON, HTML, source files")?;
        }
        create_shortcut(&env.start_menu.join(SHORTCUT_UNINSTALL), &uninstaller, "/uninstall", "Remove NuViewer from this PC")?;
    }
    if opts.desktop {
        create_shortcut(&env.desktop.join(SHORTCUT), &exe, "", "Fast, lightweight image viewer")?;
    }

    let key = env.uninstall_key();
    let q = |p: &Path| format!("\"{}\"", p.display());
    registry::set_string(&key, Some("DisplayName"), "NuViewer")?;
    registry::set_string(&key, Some("DisplayVersion"), VERSION)?;
    registry::set_string(&key, Some("Publisher"), PUBLISHER)?;
    registry::set_string(&key, Some("URLInfoAbout"), WEBSITE)?;
    registry::set_string(&key, Some("HelpLink"), SUPPORT)?;
    registry::set_string(&key, Some("Comments"), "Fast, lightweight viewer, gallery and editor for pictures, RAW files, documents and 3D models")?;
    registry::set_string(&key, Some("InstallDate"), &today_yyyymmdd())?;
    registry::set_string(&key, Some("InstallLocation"), &opts.dir.display().to_string())?;
    registry::set_string(&key, Some("DisplayIcon"), &q(&exe))?;
    registry::set_string(&key, Some("UninstallString"), &format!("{} /uninstall", q(&uninstaller)))?;
    registry::set_string(&key, Some("QuietUninstallString"), &format!("{} /uninstall /S", q(&uninstaller)))?;
    registry::set_dword(&key, "NoModify", 1)?;
    registry::set_dword(&key, "NoRepair", 1)?;
    registry::set_dword(&key, "EstimatedSize", ((files.iter().map(|(_, b)| b.len()).sum::<usize>() * 2) / 1024) as u32)?;
    Ok(exe)
}

/// The names of the files packed into this setup (what an uninstall removes besides the programs it knows by name).
pub fn packed_names() -> Vec<String> {
    payload().map(|p| p.into_iter().map(|(n, _)| n).collect()).unwrap_or_default()
}

/// Undo `install`. The running uninstaller can't delete itself, so a detached `cmd` removes it (and the folder,
/// but only if nothing else is in it) a moment after we exit.
pub fn uninstall(env: &Env, dir: &Path, purge: Purge, packed: &[String]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    close_running_viewer();
    let layout = env.layout();
    let all = nv_win::assoc::all_exts();
    registry::unregister(&layout, APP_EXE, &all);
    nv_win::explorer_menu::remove(&layout, &all); // the "Convert to PNG / JPG" entries of the Explorer menu
    registry::delete_tree(&env.uninstall_key());
    for lnk in [SHORTCUT, SHORTCUT_TYPES, SHORTCUT_GALLERY, SHORTCUT_EDITOR, SHORTCUT_IMPORTER, SHORTCUT_NOTE, SHORTCUT_UNINSTALL] {
        let _ = std::fs::remove_file(env.start_menu.join(lnk));
    }
    let _ = std::fs::remove_file(env.desktop.join(SHORTCUT));
    let exe = dir.join(APP_EXE);
    if exe.exists() {
        std::fs::remove_file(&exe).map_err(|e| format!("couldn't remove {} ({e}). Is NuViewer still running?", exe.display()))?;
    }
    let _ = std::fs::remove_file(dir.join(GALLERY_EXE));
    let _ = std::fs::remove_file(dir.join(EDITOR_EXE));
    let _ = std::fs::remove_file(dir.join(IMPORTER_EXE));
    let _ = std::fs::remove_file(dir.join(NOTE_EXE));
    // NuQuick sits in the background: stop it so its file can go, and forget its "start with Windows" entry.
    let _ = std::process::Command::new(dir.join(QUICK_EXE)).arg("--quit").status();
    std::thread::sleep(std::time::Duration::from_millis(300));
    registry::delete_value(r"Software\Microsoft\Windows\CurrentVersion\Run", "NuQuick");
    let _ = std::fs::remove_file(dir.join(QUICK_EXE));
    // The documents and language files Setup put there (a language file the user added under another name stays).
    for name in packed {
        if !name.eq_ignore_ascii_case(APP_EXE) && !name.eq_ignore_ascii_case(UNINSTALLER) {
            let _ = std::fs::remove_file(dir.join(name.replace('/', "\\")));
        }
    }
    let _ = std::fs::remove_dir(dir.join("lang")); // only when empty
    purge_data(env, purge);
    let own = dir.join(UNINSTALLER);
    if own.exists() {
        // `rmdir` without /s only removes the folder when it is empty, so anything the user put there survives.
        let cmd = format!("ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"", own.display(), dir.display());
        let _ = std::process::Command::new("cmd.exe").raw_arg(format!("/c {cmd}")).creation_flags(0x0800_0000).spawn(); // CREATE_NO_WINDOW
    } else {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(())
}

const WEBSITE: &str = "https://nuviewer.noobcyborg.com";
const SUPPORT: &str = "https://discord.com/channels/1572474506872750212/1572474507363491842";

const PUBLISHER: &str = "Noobcyborg studio";

/// Today as YYYYMMDD (the format of the Add/Remove Programs "InstallDate" value), UTC.
fn today_yyyymmdd() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

/// Write the packed files below `dir` (names may carry a sub-folder: `lang/th.txt`).
fn write_files(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    for (name, bytes) in files {
        let target = dir.join(name.replace('/', "\\"));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, bytes).map_err(|e| format!("couldn't write {} ({e}). Is NuViewer still running?", target.display()))?;
    }
    Ok(())
}

/// A portable copy: unpack the programs (and `portable.txt`, which makes them keep their data in `Data\` beside them) into
/// `dir`. Nothing else on the PC is touched: no registry, no shortcuts, no uninstaller. Returns the nuviewer.exe there.
pub fn portable(dir: &Path, files: &[(String, Vec<u8>)]) -> Result<PathBuf, String> {
    if dir.parent().is_none() || !dir.is_absolute() {
        return Err(format!("\"{}\" isn't a usable folder (pick a folder, not a whole drive).", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    write_files(dir, files)?;
    std::fs::write(dir.join("portable.txt"), nv_win::portable::MARKER_TEXT).map_err(|e| format!("couldn't write portable.txt: {e}"))?;
    let _ = std::fs::create_dir_all(nv_win::portable::data_dirs(dir).0);
    let _ = std::fs::create_dir_all(nv_win::portable::data_dirs(dir).1);
    Ok(dir.join(APP_EXE))
}

/// What an uninstall deletes besides the programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Purge {
    /// The thumbnail cache, the crash log and the other things that are made again when needed.
    pub cache: bool,
    /// The saved originals of pictures that were burned in by NuEditor (`undo`): the only copy of such a picture's earlier state.
    pub backups: bool,
    /// The settings, presets and language files in `%APPDATA%\NuViewer`.
    pub settings: bool,
}

impl Purge {
    /// What a silent uninstall does without being told: the cache goes, the user's own things stay.
    pub const DEFAULT: Purge = Purge { cache: true, backups: false, settings: false };
    /// `/purge`: everything.
    pub const ALL: Purge = Purge { cache: true, backups: true, settings: true };
}

fn purge_data(env: &Env, purge: Purge) {
    if let Some(data) = &env.data_dir {
        if purge.cache {
            if let Ok(rd) = std::fs::read_dir(data) {
                for e in rd.flatten() {
                    let is_backup = e.file_name().to_string_lossy().eq_ignore_ascii_case("undo");
                    if is_backup && !purge.backups {
                        continue;
                    }
                    let p = e.path();
                    let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                }
            }
        } else if purge.backups {
            let _ = std::fs::remove_dir_all(data.join("undo"));
        }
        let _ = std::fs::remove_dir(data); // only when nothing is left
    }
    if let Some(cfg) = &env.config_dir {
        if purge.settings {
            let _ = std::fs::remove_dir_all(cfg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nvsetup_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn install_then_uninstall_in_a_scratch_profile() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let root = scratch("full");
        let prefix = format!("Software\\NuViewerSetupTest_{}", std::process::id());
        let env = Env { reg_prefix: Some(prefix.clone()), start_menu: root.join("start"), desktop: root.join("desk"), data_dir: None, config_dir: None };
        let fake_self = root.join("setup.exe");
        std::fs::write(&fake_self, b"setup").unwrap();
        let dir = root.join("app");
        let opts = Options { dir: dir.clone(), exts: vec![".jpg".into(), ".png".into()], start_menu: true, desktop: true };

        let files = vec![
            (APP_EXE.to_string(), b"fake exe".to_vec()),
            (GALLERY_EXE.to_string(), b"fake gallery".to_vec()),
            (EDITOR_EXE.to_string(), b"fake editor".to_vec()),
            (IMPORTER_EXE.to_string(), b"fake importer".to_vec()),
            (NOTE_EXE.to_string(), b"fake notepad".to_vec()),
            ("lang/th.txt".to_string(), b"lang.name = x".to_vec()),
            ("LICENSE".to_string(), b"MIT".to_vec()),
        ];
        let exe = install(&env, &opts, &files, &fake_self).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"fake exe");
        assert_eq!(std::fs::read(dir.join(GALLERY_EXE)).unwrap(), b"fake gallery");
        assert_eq!(std::fs::read(dir.join(EDITOR_EXE)).unwrap(), b"fake editor");
        assert!(env.start_menu.join(SHORTCUT_GALLERY).is_file());
        assert!(env.start_menu.join(SHORTCUT_EDITOR).is_file() && env.start_menu.join(SHORTCUT_IMPORTER).is_file() && env.start_menu.join(SHORTCUT_UNINSTALL).is_file());
        assert!(env.start_menu.join(SHORTCUT_NOTE).is_file() && dir.join(NOTE_EXE).is_file());
        assert_eq!(std::fs::read(dir.join("lang").join("th.txt")).unwrap(), b"lang.name = x", "files in a sub-folder are written there");
        assert!(dir.join("LICENSE").is_file());
        let key = env.uninstall_key();
        assert_eq!(registry::read_string(&key, Some("URLInfoAbout")).as_deref(), Some(WEBSITE));
        assert_eq!(registry::read_string(&key, Some("InstallDate")).map(|d| d.len()), Some(8));
        assert!(dir.join(UNINSTALLER).is_file());
        assert!(env.start_menu.join(SHORTCUT).is_file() && env.start_menu.join(SHORTCUT_TYPES).is_file() && env.desktop.join(SHORTCUT).is_file());
        assert_eq!(existing_install(&env), Some(dir.clone()));
        let layout = env.layout();
        assert!(registry::is_registered(&layout));
        let mut got = registry::registered_exts(&layout);
        got.sort();
        assert_eq!(got, vec![".jpg", ".png"]);
        let un = registry::read_string(&env.uninstall_key(), Some("UninstallString")).unwrap();
        assert!(un.ends_with("uninstall.exe\" /uninstall"), "{un}");

        // Re-running with fewer formats must drop the ones that are no longer ticked.
        let opts2 = Options { exts: vec![".gif".into()], start_menu: false, desktop: false, dir: dir.clone() };
        install(&env, &opts2, &[(APP_EXE.to_string(), b"fake exe 2".to_vec())], &fake_self).unwrap();
        assert_eq!(registry::registered_exts(&layout), vec![".gif"]);
        assert_eq!(std::fs::read(&exe).unwrap(), b"fake exe 2");

        uninstall(&env, &dir, Purge::DEFAULT, &["lang/th.txt".to_string(), "LICENSE".to_string()]).unwrap();
        assert!(!dir.join("lang").join("th.txt").exists() && !dir.join("LICENSE").exists());
        assert!(!exe.exists() && !dir.join(GALLERY_EXE).exists() && !dir.join(EDITOR_EXE).exists());
        assert!(!registry::is_registered(&layout));
        assert!(registry::registered_exts(&layout).is_empty());
        assert!(registry::read_string(&env.uninstall_key(), Some("DisplayName")).is_none());
        assert!(!env.start_menu.join(SHORTCUT).exists() && !env.desktop.join(SHORTCUT).exists());
        registry::delete_tree(&prefix);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_portable_copy_changes_nothing_but_its_own_folder() {
        let root = scratch("portable");
        let prefix = format!("Software\\NuViewerSetupTest_portable_{}", std::process::id());
        let dir = root.join("stick").join("NuViewer");
        let files = vec![(APP_EXE.to_string(), b"fake exe".to_vec()), ("lang/th.txt".to_string(), b"x".to_vec())];
        let exe = portable(&dir, &files).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"fake exe");
        assert!(dir.join("lang").join("th.txt").is_file());
        assert!(nv_win::portable::is_portable_dir(&dir), "portable.txt is what turns portable mode on");
        assert!(dir.join("Data").join("Roaming").is_dir() && dir.join("Data").join("Local").is_dir());
        // No installer artefacts: no uninstaller, no registry entry.
        assert!(!dir.join(UNINSTALLER).exists());
        let env = Env { reg_prefix: Some(prefix), start_menu: root.join("start"), desktop: root.join("desk"), data_dir: None, config_dir: None };
        assert_eq!(existing_install(&env), None);
        assert!(!root.join("start").exists());
        assert!(portable(Path::new("C:\\"), &files).is_err(), "a drive root is no folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstall_keeps_what_the_user_did_not_give_up() {
        let root = scratch("purge");
        let env = |data: &Path, cfg: &Path| Env { reg_prefix: Some(format!("Software\\NuViewerSetupTest_purge_{}", std::process::id())), start_menu: root.join("s"), desktop: root.join("d"), data_dir: Some(data.to_path_buf()), config_dir: Some(cfg.to_path_buf()) };
        let make = |tag: &str| {
            let (data, cfg) = (root.join(tag).join("local"), root.join(tag).join("roaming"));
            std::fs::create_dir_all(data.join("thumbs")).unwrap();
            std::fs::create_dir_all(data.join("undo")).unwrap();
            std::fs::create_dir_all(&cfg).unwrap();
            std::fs::write(data.join("thumbs").join("a.jpg"), b"t").unwrap();
            std::fs::write(data.join("crash.log"), b"c").unwrap();
            std::fs::write(data.join("undo").join("orig.jpg"), b"o").unwrap();
            std::fs::write(cfg.join("settings.ini"), b"s").unwrap();
            (data, cfg)
        };
        // The silent default: the cache goes, the saved originals and the settings stay.
        let (data, cfg) = make("default");
        purge_data(&env(&data, &cfg), Purge::DEFAULT);
        assert!(!data.join("thumbs").exists() && !data.join("crash.log").exists());
        assert!(data.join("undo").join("orig.jpg").is_file() && cfg.join("settings.ini").is_file());
        // Backups only on request, settings only on request.
        purge_data(&env(&data, &cfg), Purge { cache: false, backups: true, settings: false });
        assert!(!data.exists() || !data.join("undo").exists());
        assert!(cfg.join("settings.ini").is_file());
        // Everything.
        let (data, cfg) = make("all");
        purge_data(&env(&data, &cfg), Purge::ALL);
        assert!(!data.exists() && !cfg.exists());
        // Nothing asked: nothing deleted.
        let (data, cfg) = make("none");
        purge_data(&env(&data, &cfg), Purge { cache: false, backups: false, settings: false });
        assert!(data.join("thumbs").join("a.jpg").is_file() && data.join("undo").join("orig.jpg").is_file() && cfg.join("settings.ini").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dates_are_written_the_way_windows_wants_them() {
        let d = today_yyyymmdd();
        assert_eq!(d.len(), 8);
        assert!(d.starts_with("20"), "{d}");
        let (m, day): (u32, u32) = (d[4..6].parse().unwrap(), d[6..8].parse().unwrap());
        assert!((1..=12).contains(&m) && (1..=31).contains(&day));
    }

    #[test]
    fn the_payload_table_round_trips_and_rejects_damage() {
        let mut blob = Vec::new();
        for (n, d) in [("a.exe", &b"123"[..]), ("b.exe", &b""[..])] {
            blob.push(n.len() as u8);
            blob.extend(n.as_bytes());
            blob.extend((d.len() as u64).to_le_bytes());
            blob.extend(d);
        }
        assert_eq!(unpack(&blob).unwrap(), vec![("a.exe".to_string(), b"123".to_vec()), ("b.exe".to_string(), Vec::new())]);
        assert!(unpack(&blob[..blob.len() - 3]).is_err());
        assert!(unpack(&[5, 1]).is_err());
        assert_eq!(unpack(&[]).unwrap(), vec![]);
    }

    #[test]
    fn refuses_a_drive_root() {
        let env = Env { reg_prefix: Some("Software\\NuViewerSetupTest_root".into()), start_menu: PathBuf::new(), desktop: PathBuf::new(), data_dir: None, config_dir: None };
        let opts = Options { dir: PathBuf::from("C:\\"), exts: vec![], start_menu: false, desktop: false };
        assert!(install(&env, &opts, &[(APP_EXE.to_string(), b"x".to_vec())], Path::new("x.exe")).is_err());
    }
}
