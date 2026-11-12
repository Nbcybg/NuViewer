//! What installing and uninstalling actually do (no UI). Per-user only: files under %LOCALAPPDATA%\Programs,
//! registry under HKEY_CURRENT_USER, shortcuts in the user's Start menu / Desktop. No administrator rights.

use nv_win::registry::{self, Layout};
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, SHGetKnownFolderPath, ShellLink, FOLDERID_Desktop, FOLDERID_Programs, KF_FLAG_DEFAULT};

pub const APP_EXE: &str = "nuviewer.exe";
pub const UNINSTALLER: &str = "uninstall.exe";
const SHORTCUT: &str = "NuViewer.lnk";
const SHORTCUT_TYPES: &str = "NuViewer file types.lnk";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));
const PAYLOAD_LEN: &str = include_str!(concat!(env!("OUT_DIR"), "/payload.len"));

/// The compressed-in nuviewer.exe, unpacked.
pub fn payload() -> Result<Vec<u8>, String> {
    if PAYLOAD.is_empty() {
        return Err("this setup was built without nuviewer.exe inside it".into());
    }
    let want: usize = PAYLOAD_LEN.trim().parse().unwrap_or(0);
    let data = miniz_oxide::inflate::decompress_to_vec(PAYLOAD).map_err(|e| format!("the setup data is damaged ({e:?})"))?;
    if data.len() != want {
        return Err("the setup data is damaged (size mismatch)".into());
    }
    Ok(data)
}

/// Where things go. `real()` is the user's actual profile; tests and the `/regprefix` switch redirect everything.
pub struct Env {
    /// None = the real registry; Some(prefix) = a scratch HKCU subkey holding Classes, NuViewer, RegisteredApplications, Uninstall.
    pub reg_prefix: Option<String>,
    pub start_menu: PathBuf,
    pub desktop: PathBuf,
    /// NuViewer's own data (thumbnail cache, crash log), removed on uninstall. None leaves it alone (tests).
    pub data_dir: Option<PathBuf>,
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

/// Ask a running NuViewer to close (an upgrade can't replace a running exe) and wait briefly.
fn close_running_viewer() {
    use windows::core::w;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
    for _ in 0..30 {
        match unsafe { FindWindowW(w!("NuViewerWindow"), None) } {
            Ok(h) if !h.is_invalid() => unsafe {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => return,
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
/// `payload` is the unpacked nuviewer.exe; `self_exe` is copied next to it as the uninstaller.
/// Returns the installed nuviewer.exe.
pub fn install(env: &Env, opts: &Options, payload: &[u8], self_exe: &Path) -> Result<PathBuf, String> {
    if opts.dir.parent().is_none() || !opts.dir.is_absolute() {
        return Err(format!("\"{}\" isn't a usable install folder (pick a folder, not a whole drive).", opts.dir.display()));
    }
    close_running_viewer();
    std::fs::create_dir_all(&opts.dir).map_err(|e| format!("couldn't create {}: {e}", opts.dir.display()))?;
    let exe = opts.dir.join(APP_EXE);
    std::fs::write(&exe, payload).map_err(|e| format!("couldn't write {} ({e}). Is NuViewer still running?", exe.display()))?;
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
    }
    if opts.desktop {
        create_shortcut(&env.desktop.join(SHORTCUT), &exe, "", "Fast, lightweight image viewer")?;
    }

    let key = env.uninstall_key();
    let q = |p: &Path| format!("\"{}\"", p.display());
    registry::set_string(&key, Some("DisplayName"), "NuViewer")?;
    registry::set_string(&key, Some("DisplayVersion"), VERSION)?;
    registry::set_string(&key, Some("Publisher"), "NuViewer")?;
    registry::set_string(&key, Some("InstallLocation"), &opts.dir.display().to_string())?;
    registry::set_string(&key, Some("DisplayIcon"), &q(&exe))?;
    registry::set_string(&key, Some("UninstallString"), &format!("{} /uninstall", q(&uninstaller)))?;
    registry::set_string(&key, Some("QuietUninstallString"), &format!("{} /uninstall /S", q(&uninstaller)))?;
    registry::set_dword(&key, "NoModify", 1)?;
    registry::set_dword(&key, "NoRepair", 1)?;
    registry::set_dword(&key, "EstimatedSize", ((payload.len() * 2) / 1024) as u32)?;
    Ok(exe)
}

/// Undo `install`. The running uninstaller can't delete itself, so a detached `cmd` removes it (and the folder,
/// but only if nothing else is in it) a moment after we exit.
pub fn uninstall(env: &Env, dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    close_running_viewer();
    let layout = env.layout();
    registry::unregister(&layout, APP_EXE, &nv_win::assoc::all_exts());
    registry::delete_tree(&env.uninstall_key());
    for lnk in [env.start_menu.join(SHORTCUT), env.start_menu.join(SHORTCUT_TYPES), env.desktop.join(SHORTCUT)] {
        let _ = std::fs::remove_file(lnk);
    }
    let exe = dir.join(APP_EXE);
    if exe.exists() {
        std::fs::remove_file(&exe).map_err(|e| format!("couldn't remove {} ({e}). Is NuViewer still running?", exe.display()))?;
    }
    if let Some(data) = &env.data_dir {
        let _ = std::fs::remove_dir_all(data);
    }
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
        let env = Env { reg_prefix: Some(prefix.clone()), start_menu: root.join("start"), desktop: root.join("desk"), data_dir: None };
        let fake_self = root.join("setup.exe");
        std::fs::write(&fake_self, b"setup").unwrap();
        let dir = root.join("app");
        let opts = Options { dir: dir.clone(), exts: vec![".jpg".into(), ".png".into()], start_menu: true, desktop: true };

        let exe = install(&env, &opts, b"fake exe", &fake_self).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"fake exe");
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
        install(&env, &opts2, b"fake exe 2", &fake_self).unwrap();
        assert_eq!(registry::registered_exts(&layout), vec![".gif"]);
        assert_eq!(std::fs::read(&exe).unwrap(), b"fake exe 2");

        uninstall(&env, &dir).unwrap();
        assert!(!exe.exists());
        assert!(!registry::is_registered(&layout));
        assert!(registry::registered_exts(&layout).is_empty());
        assert!(registry::read_string(&env.uninstall_key(), Some("DisplayName")).is_none());
        assert!(!env.start_menu.join(SHORTCUT).exists() && !env.desktop.join(SHORTCUT).exists());
        registry::delete_tree(&prefix);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_a_drive_root() {
        let env = Env { reg_prefix: Some("Software\\NuViewerSetupTest_root".into()), start_menu: PathBuf::new(), desktop: PathBuf::new(), data_dir: None };
        let opts = Options { dir: PathBuf::from("C:\\"), exts: vec![], start_menu: false, desktop: false };
        assert!(install(&env, &opts, b"x", Path::new("x.exe")).is_err());
    }
}
