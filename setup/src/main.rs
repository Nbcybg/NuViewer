#![windows_subsystem = "windows"]
//! NuViewer setup / uninstaller (one exe: the installed copy is `uninstall.exe`).
//!
//! Wizard (double-click): pick the folder, the image formats and the shortcuts, press Install.
//! Command line (all switches also work as `--name`):
//!   /S                    silent install, no windows
//!   /D=<folder>           install folder (default %LOCALAPPDATA%\Programs\NuViewer)
//!   /formats=<list>       all | none | default | comma-separated extensions, e.g. jpg,png,cr2   (default: `default`)
//!   /nostartmenu          no Start menu shortcuts          /desktop   add a Desktop shortcut
//!   /launch               start NuViewer afterwards
//!   /portable [/D=<dir>]  portable copy: only unpack the programs into the folder (nothing is registered, settings stay in
//!                         its Data folder); `portable.txt` in it is what makes the programs portable
//!   /uninstall [/S]       remove NuViewer (what Settings > Apps > Uninstall runs); silent keeps settings and the backups of edited
//!                         pictures, `/purge` removes those too, `/keepcache` keeps the thumbnail cache
//!   /log=<file>           write the outcome as text (handy with /S, which shows nothing)
//!   /regprefix=<subkey>, /startmenudir=<dir>, /desktopdir=<dir>   redirect everything to scratch locations (tests)

mod install;

use install::{Env, Options};
use nv_win::assoc;
use nv_win::ui::{Form, ID_CANCEL, ID_OK};
use std::path::PathBuf;

const ID_DIR: u16 = 100;
const ID_BROWSE: u16 = 101;
const ID_START_MENU: u16 = 110;
const ID_DESKTOP: u16 = 111;
const ID_LAUNCH: u16 = 112;
const ID_PORTABLE: u16 = 113;

struct Args {
    silent: bool,
    uninstall: bool,
    portable: bool,
    purge: bool,
    keep_cache: bool,
    launch: bool,
    desktop: bool,
    start_menu: bool,
    dir: Option<PathBuf>,
    formats: String,
    log: Option<PathBuf>,
    reg_prefix: Option<String>,
    start_menu_dir: Option<PathBuf>,
    desktop_dir: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args {
        silent: false,
        uninstall: false,
        portable: false,
        purge: false,
        keep_cache: false,
        launch: false,
        desktop: false,
        start_menu: true,
        dir: None,
        formats: "default".into(),
        log: None,
        reg_prefix: None,
        start_menu_dir: None,
        desktop_dir: None,
    };
    for raw in std::env::args().skip(1) {
        let arg = raw.trim_start_matches(['/', '-']).to_string();
        let (name, value) = match arg.split_once(['=', ':']) {
            Some((n, v)) => (n.to_ascii_lowercase(), Some(v.trim_matches('"').to_string())),
            None => (arg.to_ascii_lowercase(), None),
        };
        match (name.as_str(), value) {
            ("s" | "silent" | "quiet", _) => a.silent = true,
            ("uninstall", _) => a.uninstall = true,
            ("portable", _) => a.portable = true,
            ("purge", _) => a.purge = true,
            ("keepcache", _) => a.keep_cache = true,
            ("launch", _) => a.launch = true,
            ("desktop", _) => a.desktop = true,
            ("nostartmenu", _) => a.start_menu = false,
            ("d" | "dir", Some(v)) => a.dir = Some(PathBuf::from(v)),
            ("formats", Some(v)) => a.formats = v,
            ("log", Some(v)) => a.log = Some(PathBuf::from(v)),
            ("regprefix", Some(v)) => a.reg_prefix = Some(v),
            ("startmenudir", Some(v)) => a.start_menu_dir = Some(PathBuf::from(v)),
            ("desktopdir", Some(v)) => a.desktop_dir = Some(PathBuf::from(v)),
            _ => {}
        }
    }
    a
}

/// Extensions (with dots) for a `/formats=` value.
fn formats_from(spec: &str) -> Vec<String> {
    match spec.to_ascii_lowercase().as_str() {
        "all" => assoc::all_exts(),
        "none" => Vec::new(),
        "default" | "" => assoc::exts_of(&assoc::default_picks()),
        list => list.split(',').map(|e| format!(".{}", e.trim().trim_start_matches('.'))).filter(|e| e.len() > 1).collect(),
    }
}

fn say(args: &Args, text: &str) {
    if let Some(log) = &args.log {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
            let _ = writeln!(f, "{text}");
        }
    }
}

fn make_env(args: &Args) -> Env {
    let mut env = Env::real();
    if let Some(p) = &args.reg_prefix {
        env.reg_prefix = Some(p.clone());
        env.data_dir = None; // a test run never touches the real thumbnail cache
        env.config_dir = None; // nor the real settings
        env.start_menu = args.start_menu_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("NuViewerSetupTest").join("start"));
        env.desktop = args.desktop_dir.clone().unwrap_or_else(|| std::env::temp_dir().join("NuViewerSetupTest").join("desktop"));
    }
    if let Some(d) = &args.start_menu_dir {
        env.start_menu = d.clone();
    }
    if let Some(d) = &args.desktop_dir {
        env.desktop = d.clone();
    }
    env
}

fn main() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let args = parse_args();
    let env = make_env(&args);
    let code = if args.uninstall {
        run_uninstall(&args, &env)
    } else if args.portable {
        run_portable(&args)
    } else {
        run_install(&args, &env)
    };
    std::process::exit(code);
}

fn run_install(args: &Args, env: &Env) -> i32 {
    let payload = match install::payload() {
        Ok(p) => p,
        Err(e) => return fail(args, &e),
    };
    let self_exe = std::env::current_exe().unwrap_or_default();
    let dir = args.dir.clone().or_else(|| install::existing_install(env)).unwrap_or_else(install::default_dir);

    let (opts, launch) = if args.silent {
        (Options { dir, exts: formats_from(&args.formats), start_menu: args.start_menu, desktop: args.desktop }, args.launch)
    } else {
        match wizard(args, env, dir) {
            Some(Wizard::Install(opts, launch)) => (opts, launch),
            Some(Wizard::Portable(dir, launch)) => return finish_portable(args, &payload, &dir, launch),
            None => return 0, // cancelled
        }
    };

    let exe = match install::install(env, &opts, &payload, &self_exe) {
        Ok(exe) => exe,
        Err(e) => return fail(args, &e),
    };
    say(args, &format!("installed {} ({} types)", exe.display(), opts.exts.len()));
    if !args.silent {
        let mut text = format!("NuViewer {} is installed in\n{}", install::VERSION, opts.dir.display());
        let mut open_settings = false;
        if env.reg_prefix.is_none() && !opts.exts.is_empty() {
            let (more, open) = assoc::describe(&opts.exts);
            text += &format!("\n\n{more}");
            open_settings = open;
        }
        nv_win::shell::message(&text);
        if open_settings {
            assoc::open_default_apps_settings();
        }
    }
    if launch {
        let _ = std::process::Command::new(&exe).spawn();
    }
    0
}

fn run_portable(args: &Args) -> i32 {
    let payload = match install::payload() {
        Ok(p) => p,
        Err(e) => return fail(args, &e),
    };
    let dir = args.dir.clone().unwrap_or_else(|| install::default_dir().with_file_name("NuViewer-portable"));
    finish_portable(args, &payload, &dir, args.launch)
}

fn finish_portable(args: &Args, payload: &[(String, Vec<u8>)], dir: &std::path::Path, launch: bool) -> i32 {
    match install::portable(dir, payload) {
        Ok(exe) => {
            say(args, &format!("portable copy in {}", dir.display()));
            if !args.silent {
                nv_win::shell::message(&format!("The portable copy of NuViewer {} is in\n{}\n\nIt keeps its settings in the Data folder there and changes nothing else on this PC. Copy or move the folder anywhere; delete it to remove everything.", install::VERSION, dir.display()));
            }
            if launch {
                let _ = std::process::Command::new(&exe).spawn();
            }
            0
        }
        Err(e) => fail(args, &e),
    }
}

const ID_PURGE_CACHE: u16 = 120;
const ID_PURGE_BACKUPS: u16 = 121;
const ID_PURGE_SETTINGS: u16 = 122;

/// The question of an interactive uninstall: what else goes besides the programs. None = cancelled.
fn uninstall_dialog() -> Option<install::Purge> {
    let form = Form::new("Uninstall NuViewer", 520, 262).ok()?;
    form.label(14, 12, 492, 40, "NuViewer, its shortcuts and its file-type registration will be removed. Choose what else should go:");
    form.check(ID_PURGE_CACHE, 28, 60, 470, 22, "The thumbnail cache and logs (they are made again when needed)", true);
    form.check(ID_PURGE_BACKUPS, 28, 88, 470, 22, "The saved originals of pictures NuEditor burned in (cannot be made again)", false);
    form.check(ID_PURGE_SETTINGS, 28, 116, 470, 22, "My settings, presets and language files", false);
    form.label(14, 152, 492, 36, "Your pictures, and the .nuedit pair files next to them, are never touched.");
    form.button(ID_OK, 322, 204, 92, 28, "Uninstall", true);
    form.button(ID_CANCEL, 422, 204, 84, 28, "Cancel", false);
    let mut result = None;
    form.run(&mut |form, id| match id {
        ID_OK => {
            result = Some(install::Purge { cache: form.is_checked(ID_PURGE_CACHE), backups: form.is_checked(ID_PURGE_BACKUPS), settings: form.is_checked(ID_PURGE_SETTINGS) });
            true
        }
        ID_CANCEL => true,
        _ => false,
    });
    result
}

fn run_uninstall(args: &Args, env: &Env) -> i32 {
    let own = std::env::current_exe().unwrap_or_default();
    let dir = args.dir.clone().or_else(|| own.parent().map(|p| p.to_path_buf())).unwrap_or_default();
    let purge = if args.silent {
        if args.purge { install::Purge::ALL } else { install::Purge { cache: !args.keep_cache, ..install::Purge::DEFAULT } }
    } else {
        match uninstall_dialog() {
            Some(p) => p,
            None => return 0,
        }
    };
    match install::uninstall(env, &dir, purge, &install::packed_names()) {
        Ok(()) => {
            say(args, &format!("uninstalled {}", dir.display()));
            if !args.silent {
                nv_win::shell::message("NuViewer was removed.");
            }
            0
        }
        Err(e) => fail(args, &e),
    }
}

fn fail(args: &Args, msg: &str) -> i32 {
    say(args, &format!("error: {msg}"));
    if !args.silent {
        nv_win::shell::message(&format!("Setup couldn't finish:\n\n{msg}"));
    }
    1
}

/// What the wizard decided.
enum Wizard {
    /// Install for this user, and whether to start NuViewer afterwards.
    Install(Options, bool),
    /// Only unpack into this folder (a portable copy), and whether to start NuViewer afterwards.
    Portable(PathBuf, bool),
}

/// The installer window. Returns the choice, or None if cancelled.
fn wizard(_args: &Args, env: &Env, dir: PathBuf) -> Option<Wizard> {
    let upgrading = install::existing_install(env).is_some();
    let current = nv_win::registry::registered_exts(&env.layout());
    let picked = if current.is_empty() { assoc::default_picks() } else { assoc::picked_for(&current) };

    let form = Form::new(&format!("NuViewer {} — Setup", install::VERSION), 560, 108 + assoc::checklist_height() + 162).ok()?;
    let intro = if upgrading {
        "NuViewer is already installed here; Setup will update it. It installs for your account only (no administrator rights)."
    } else {
        "Fast, lightweight image viewer. Setup installs it for your account only (no administrator rights)."
    };
    form.label(14, 12, 532, 34, intro);
    form.label(14, 52, 80, 20, "Install to:");
    form.edit(ID_DIR, 90, 48, 372, 24, &dir.display().to_string());
    form.button(ID_BROWSE, 470, 47, 78, 26, "Browse…", false);
    form.label(14, 84, 532, 20, "Open these image formats with NuViewer (you can change this later with \"NuViewer file types\"):");
    let bottom = assoc::build_checklist(&form, 12, 108, 536, &picked);
    form.button(assoc::ID_SELECT_ALL, 12, bottom + 2, 96, 26, "Select all", false);
    form.button(assoc::ID_SELECT_NONE, 116, bottom + 2, 96, 26, "Select none", false);
    form.check(ID_START_MENU, 14, bottom + 38, 250, 22, "Create a Start menu shortcut", true);
    form.check(ID_DESKTOP, 280, bottom + 38, 250, 22, "Create a desktop shortcut", false);
    form.check(ID_LAUNCH, 14, bottom + 62, 400, 22, "Start NuViewer when Setup finishes", false);
    form.check(ID_PORTABLE, 14, bottom + 88, 534, 22, "Portable copy: only unpack into this folder (nothing is registered, settings stay beside the programs)", false);
    form.button(ID_OK, 372, bottom + 120, 84, 28, if upgrading { "Update" } else { "Install" }, true);
    form.button(ID_CANCEL, 464, bottom + 120, 84, 28, "Cancel", false);

    let mut result = None;
    form.run(&mut |form, id| {
        if assoc::handle_select(form, id) {
            return false;
        }
        match id {
            ID_BROWSE => {
                let start = PathBuf::from(form.text(ID_DIR));
                if let Some(picked) = nv_win::shell::pick_folder(form.hwnd, "Choose the install folder", Some(&start)) {
                    let named = picked.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("NuViewer")).unwrap_or(false);
                    form.set_text(ID_DIR, &if named { picked } else { picked.join("NuViewer") }.display().to_string());
                }
                false
            }
            ID_OK => {
                let dir = PathBuf::from(form.text(ID_DIR).trim());
                if !dir.is_absolute() || dir.parent().is_none() {
                    nv_win::shell::message("Please enter a full folder path, such as C:\\Users\\you\\AppData\\Local\\Programs\\NuViewer.");
                    return false;
                }
                if form.is_checked(ID_PORTABLE) {
                    result = Some(Wizard::Portable(dir, form.is_checked(ID_LAUNCH)));
                    return true;
                }
                let opts = Options { dir, exts: assoc::exts_of(&assoc::read_picks(form)), start_menu: form.is_checked(ID_START_MENU), desktop: form.is_checked(ID_DESKTOP) };
                result = Some(Wizard::Install(opts, form.is_checked(ID_LAUNCH)));
                true
            }
            ID_PORTABLE => {
                // A portable copy goes to its own folder by default, not over the installed program.
                let (inst, port) = (install::default_dir(), install::default_dir().with_file_name("NuViewer-portable"));
                let cur = PathBuf::from(form.text(ID_DIR).trim());
                if form.is_checked(ID_PORTABLE) && cur == inst {
                    form.set_text(ID_DIR, &port.display().to_string());
                } else if !form.is_checked(ID_PORTABLE) && cur == port {
                    form.set_text(ID_DIR, &inst.display().to_string());
                }
                false
            }
            ID_CANCEL => true,
            _ => false,
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_switch() {
        assert_eq!(formats_from("none"), Vec::<String>::new());
        assert_eq!(formats_from("all").len(), assoc::all_exts().len());
        assert_eq!(formats_from("jpg, .PNG,cr2"), vec![".jpg", ".png", ".cr2"]);
        let d = formats_from("default");
        assert!(d.contains(&".jpg".to_string()) && d.contains(&".raf".to_string()) && !d.contains(&".psd".to_string()));
    }
}
