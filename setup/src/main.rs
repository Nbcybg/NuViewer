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
//!   /uninstall [/S]       remove NuViewer (what Settings > Apps > Uninstall runs)
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

struct Args {
    silent: bool,
    uninstall: bool,
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
    let code = if args.uninstall { run_uninstall(&args, &env) } else { run_install(&args, &env) };
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
            Some(x) => x,
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

fn run_uninstall(args: &Args, env: &Env) -> i32 {
    let own = std::env::current_exe().unwrap_or_default();
    let dir = args.dir.clone().or_else(|| own.parent().map(|p| p.to_path_buf())).unwrap_or_default();
    if !args.silent && !nv_win::shell::confirm(windows::Win32::Foundation::HWND::default(), "Remove NuViewer and its file-type registration?") {
        return 0;
    }
    match install::uninstall(env, &dir) {
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

/// The installer window. Returns the chosen options and whether to launch NuViewer, or None if cancelled.
fn wizard(_args: &Args, env: &Env, dir: PathBuf) -> Option<(Options, bool)> {
    let upgrading = install::existing_install(env).is_some();
    let current = nv_win::registry::registered_exts(&env.layout());
    let picked = if current.is_empty() { assoc::default_picks() } else { assoc::picked_for(&current) };

    let form = Form::new(&format!("NuViewer {} — Setup", install::VERSION), 560, 108 + assoc::checklist_height() + 136).ok()?;
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
    form.button(ID_OK, 372, bottom + 94, 84, 28, if upgrading { "Update" } else { "Install" }, true);
    form.button(ID_CANCEL, 464, bottom + 94, 84, 28, "Cancel", false);

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
                let opts = Options { dir, exts: assoc::exts_of(&assoc::read_picks(form)), start_menu: form.is_checked(ID_START_MENU), desktop: form.is_checked(ID_DESKTOP) };
                result = Some((opts, form.is_checked(ID_LAUNCH)));
                true
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
