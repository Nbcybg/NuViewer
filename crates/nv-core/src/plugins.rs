//! Plug-ins, API version 1: extra picture formats and extra menu commands, added by dropping a folder into
//! `%APPDATA%\NuViewer\plugins\` (the environment variable `NUVIEWER_PLUGINS` points somewhere else). Written up in `docs/plugins.md`.
//!
//! The design is about trust before power:
//! * A plug-in is a folder with a `plugin.ini` and the programs it names. **Nothing is loaded into NuViewer's process** (no DLLs):
//!   a plug-in is started as its own program, so one that crashes, hangs or is slow cannot take the viewer down, and a decoder is
//!   stopped after its time limit.
//! * A plug-in can only start programs **inside its own folder** (no absolute paths, no `..`, no links out of it) and the programs
//!   are started without a shell, with each argument kept whole, so a file name with spaces or `&` in it cannot become a command.
//! * A decoder can only **add** formats; an extension NuViewer reads itself is never handed over.
//! * Nothing is downloaded or installed by NuViewer, and a plug-in runs only when you use its format or choose its command.
//!   Settings has a master switch (`plugins = off`) that loads nothing at all.
//!
//! `plugin.ini` (`key = value`, `#` starts a comment):
//! ```text
//! name = Open in Paint          # shown in the menu
//! version = 1.0
//! author = Someone
//! description = One line about it
//! api = 1
//! decoder.extensions = xyz, abc # formats it reads
//! decoder.exe = decode.cmd      # inside this folder; gets {input} and {output} (a .png it must write)
//! decoder.args = "{input}" "{output}"
//! decoder.timeout = 30          # seconds
//! command.0.name = Edit in Paint
//! command.0.exe = run.cmd
//! command.0.args = "{file}"     # {file} {files} {dir} {name} {stem} {ext} {plugin}
//! command.0.for = pictures      # pictures | raw | any
//! command.0.multi = false       # true: one start with {files}; false: one start per file
//! command.0.window = normal     # hidden | normal
//! ```

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// The plug-in interface this build understands. A plug-in that asks for a newer one is listed as skipped.
pub const API: u32 = 1;

/// How many programs one command may start at once (a click on 5000 selected files must not start 5000 programs).
const MAX_STARTS: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppliesTo {
    Pictures,
    Raw,
    Any,
}

impl AppliesTo {
    pub fn accepts(self, path: &Path) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match self {
            AppliesTo::Any => true,
            AppliesTo::Raw => nv_codecs::is_raw_ext(&ext),
            AppliesTo::Pictures => nv_codecs::is_supported_ext(&ext),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: String,
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub applies: AppliesTo,
    pub multi: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// The folder name.
    pub id: String,
    pub dir: PathBuf,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub decoder: Option<nv_codecs::plugin_decode::Decoder>,
    pub commands: Vec<Command>,
}

/// What a scan of the plug-ins folder found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    pub dir: PathBuf,
    pub plugins: Vec<Plugin>,
    /// Folders that were not used, and why: (folder name, reason).
    pub skipped: Vec<(String, String)>,
}

impl Loaded {
    /// Every command of every plug-in that applies to `path`: (plug-in index, command index, "Plug-in: command").
    pub fn commands_for(&self, path: &Path) -> Vec<(usize, usize, String)> {
        let mut v = Vec::new();
        for (pi, p) in self.plugins.iter().enumerate() {
            for (ci, c) in p.commands.iter().enumerate() {
                if c.applies.accepts(path) {
                    v.push((pi, ci, if p.commands.len() == 1 && c.name == p.name { c.name.clone() } else { format!("{}: {}", p.name, c.name) }));
                }
            }
        }
        v
    }
}

// ---- where ---------------------------------------------------------------------------------------------------------------

pub fn plugins_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("NUVIEWER_PLUGINS") {
        return PathBuf::from(p);
    }
    crate::settings::Settings::path().parent().map(|p| p.join("plugins")).unwrap_or_else(|| PathBuf::from("plugins"))
}

// ---- reading a manifest --------------------------------------------------------------------------------------------------

/// Split a command line the Windows way: spaces separate, double quotes keep a piece together.
pub fn split_args(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// A program of the plug-in: a relative path inside its folder, a real file there, and one Windows can start by itself.
fn resolve_exe(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err("no program named".into());
    }
    let p = Path::new(rel);
    if p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir)) {
        return Err(format!("\"{rel}\" is outside the plug-in's folder (only a path inside it is allowed)"));
    }
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "exe" | "cmd" | "bat" | "com") {
        return Err(format!("\"{rel}\" is not a program (.exe, .cmd, .bat)"));
    }
    let full = dir.join(p);
    let real = full.canonicalize().map_err(|_| format!("\"{rel}\" is not in the plug-in's folder"))?;
    let base = dir.canonicalize().map_err(|e| e.to_string())?;
    if !real.starts_with(&base) || !real.is_file() {
        return Err(format!("\"{rel}\" is not a file inside the plug-in's folder"));
    }
    Ok(full)
}

/// Read one plug-in folder. `Err` says why it is not usable (shown in the plug-in list).
pub fn parse_plugin(id: &str, dir: &Path, text: &str) -> Result<Plugin, String> {
    let mut kv = std::collections::BTreeMap::<String, String>::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{FEFF}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            // A comment after the value only counts when there is a space before the '#'.
            let v = match v.find(" #") {
                Some(i) => &v[..i],
                None => v,
            };
            kv.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let get = |k: &str| kv.get(k).cloned().unwrap_or_default();
    let api: u32 = get("api").parse().map_err(|_| "plugin.ini has no `api = 1` line".to_string())?;
    if api > API {
        return Err(format!("it needs plug-in interface {api}, this NuViewer has {API}: update NuViewer"));
    }
    if api == 0 {
        return Err("`api` must be 1 or more".into());
    }
    let name = Some(get("name")).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string());

    let decoder = match kv.get("decoder.extensions") {
        None => None,
        Some(list) => {
            let exts: Vec<String> = list.split([',', ' ']).map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric())).collect();
            if exts.is_empty() {
                return Err("decoder.extensions names no extension".into());
            }
            let exe = resolve_exe(dir, &get("decoder.exe")).map_err(|e| format!("decoder: {e}"))?;
            let args = Some(get("decoder.args")).filter(|a| !a.is_empty()).map(|a| split_args(&a)).unwrap_or_else(|| vec!["{input}".into(), "{output}".into()]);
            if !args.iter().any(|a| a.contains("{output}")) {
                return Err("decoder.args must contain {output}, where the PNG is written".into());
            }
            let secs = get("decoder.timeout").parse::<u64>().ok().filter(|s| (1..=600).contains(s)).unwrap_or(30);
            Some(nv_codecs::plugin_decode::Decoder { plugin: name.clone(), exts, exe, args, timeout: Duration::from_secs(secs) })
        }
    };

    let mut commands = Vec::new();
    for i in 0..32 {
        let key = |f: &str| format!("command.{i}.{f}");
        let Some(cname) = kv.get(&key("name")).cloned().filter(|n| !n.is_empty()) else { continue };
        let exe = resolve_exe(dir, &get(&key("exe"))).map_err(|e| format!("command \"{cname}\": {e}"))?;
        let applies = match get(&key("for")).to_ascii_lowercase().as_str() {
            "" | "pictures" => AppliesTo::Pictures,
            "raw" => AppliesTo::Raw,
            "any" => AppliesTo::Any,
            other => return Err(format!("command \"{cname}\": `for = {other}` is not pictures, raw or any")),
        };
        let args = Some(get(&key("args"))).filter(|a| !a.is_empty()).map(|a| split_args(&a)).unwrap_or_else(|| vec!["{file}".into()]);
        commands.push(Command { name: cname, exe, args, applies, multi: matches!(get(&key("multi")).to_ascii_lowercase().as_str(), "true" | "yes" | "1" | "on"), hidden: get(&key("window")).eq_ignore_ascii_case("hidden") });
    }
    if decoder.is_none() && commands.is_empty() {
        return Err("it has neither a decoder nor a command".into());
    }
    Ok(Plugin { id: id.to_string(), dir: dir.to_path_buf(), name, version: get("version"), author: get("author"), description: get("description"), decoder, commands })
}

/// Scan `root` (one folder per plug-in). A bad plug-in is listed as skipped and never stops the others.
pub fn scan(root: &Path) -> Loaded {
    let mut out = Loaded { dir: root.to_path_buf(), ..Default::default() };
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    let mut dirs: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.path()).collect();
    dirs.sort_by_key(|d| d.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    for d in dirs {
        let id = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let manifest = d.join("plugin.ini");
        match std::fs::read_to_string(&manifest) {
            Err(_) => out.skipped.push((id, "no plugin.ini in it".into())),
            Ok(text) => match parse_plugin(&id, &d, &text) {
                Ok(p) => out.plugins.push(p),
                Err(why) => out.skipped.push((id, why)),
            },
        }
    }
    out
}

// ---- the loaded set -----------------------------------------------------------------------------------------------------

static CURRENT: RwLock<Option<Arc<Loaded>>> = RwLock::new(None);

/// Load the plug-ins (nothing when `enabled` is false) and hand their decoders to the picture reader. Call at start-up and for
/// "Reload plug-ins".
pub fn load(enabled: bool) -> Arc<Loaded> {
    let loaded = Arc::new(if enabled { scan(&plugins_dir()) } else { Loaded { dir: plugins_dir(), ..Default::default() } });
    nv_codecs::plugin_decode::set_decoders(loaded.plugins.iter().filter_map(|p| p.decoder.clone()).collect());
    *CURRENT.write().unwrap() = Some(loaded.clone());
    loaded
}

/// The plug-ins loaded by the last `load` (none before it).
pub fn current() -> Arc<Loaded> {
    CURRENT.read().unwrap().clone().unwrap_or_default()
}

// ---- running a command --------------------------------------------------------------------------------------------------

/// The arguments of a command for these files: `{files}` as a whole argument becomes every file; the other fields are filled in
/// inside their argument, which stays one argument whatever the file name contains.
pub fn expand_args(args: &[String], plugin_dir: &Path, files: &[PathBuf]) -> Vec<String> {
    let first = files.first().cloned().unwrap_or_default();
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let (dir, name) = (first.parent().map(s).unwrap_or_default(), first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let stem = first.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = first.extension().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Vec::new();
    for a in args {
        if a == "{files}" {
            out.extend(files.iter().map(|f| s(f)));
        } else {
            out.push(a.replace("{file}", &s(&first)).replace("{dir}", &dir).replace("{name}", &name).replace("{stem}", &stem).replace("{ext}", &ext).replace("{plugin}", &s(plugin_dir)));
        }
    }
    out
}

/// Start a command of a plug-in on `files` and return at once (the program runs on its own). Files the command does not apply to
/// are skipped; `Err` when nothing could be started.
pub fn run_command(plugin: &Plugin, cmd: &Command, files: &[PathBuf]) -> Result<usize, String> {
    let files: Vec<PathBuf> = files.iter().filter(|f| cmd.applies.accepts(f)).cloned().collect();
    if files.is_empty() {
        return Err(format!("\"{}\" does not apply to this file", cmd.name));
    }
    let batches: Vec<Vec<PathBuf>> = if cmd.multi { vec![files] } else { files.into_iter().take(MAX_STARTS).map(|f| vec![f]).collect() };
    let mut started = 0;
    let mut last_err = None;
    for b in &batches {
        let mut c = std::process::Command::new(&cmd.exe);
        c.args(expand_args(&cmd.args, &plugin.dir, b)).current_dir(&plugin.dir);
        #[cfg(windows)]
        if cmd.hidden {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        match c.spawn() {
            Ok(mut child) => {
                started += 1;
                std::thread::spawn(move || {
                    let _ = child.wait(); // reaped, not waited for
                });
            }
            Err(e) => last_err = Some(format!("\"{}\" could not start: {e}", cmd.name)),
        }
    }
    if started == 0 { Err(last_err.unwrap_or_else(|| "nothing was started".into())) } else { Ok(started) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nv_plugins_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn plugin_dir(root: &Path, id: &str, ini: &str, files: &[&str]) -> PathBuf {
        let d = root.join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.ini"), ini).unwrap();
        for f in files {
            std::fs::write(d.join(f), b"@echo off\r\n").unwrap();
        }
        d
    }

    #[test]
    fn arguments_split_like_windows_and_keep_quoted_pieces() {
        assert_eq!(split_args("\"{input}\" \"{output}\""), ["{input}", "{output}"]);
        assert_eq!(split_args("a  \"b c\" d"), ["a", "b c", "d"]);
        assert_eq!(split_args("--size=\"1 2\""), ["--size=1 2"]);
        assert_eq!(split_args("\"\" x"), ["", "x"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn a_good_plugin_is_read_with_decoder_and_commands() {
        let root = scratch("good");
        let d = plugin_dir(
            &root,
            "paint",
            "name = Paint tools\nversion = 1.2\nauthor = Me\napi = 1\ndecoder.extensions = .XYZ, abc  # formats\ndecoder.exe = dec.cmd\ndecoder.timeout = 5\ncommand.0.name = Edit\ncommand.0.exe = run.cmd\ncommand.0.args = \"{file}\" --x\ncommand.1.name = All\ncommand.1.exe = run.cmd\ncommand.1.for = any\ncommand.1.multi = true\ncommand.1.window = hidden\n",
            &["dec.cmd", "run.cmd"],
        );
        let p = parse_plugin("paint", &d, &std::fs::read_to_string(d.join("plugin.ini")).unwrap()).unwrap();
        assert_eq!((p.name.as_str(), p.version.as_str(), p.author.as_str()), ("Paint tools", "1.2", "Me"));
        let dec = p.decoder.as_ref().unwrap();
        assert_eq!(dec.exts, ["xyz", "abc"]);
        assert_eq!(dec.args, ["{input}", "{output}"], "the default arguments");
        assert_eq!(dec.timeout, Duration::from_secs(5));
        assert_eq!(p.commands.len(), 2);
        assert_eq!(p.commands[0].args, ["{file}", "--x"]);
        assert_eq!(p.commands[0].applies, AppliesTo::Pictures);
        assert!(p.commands[1].multi && p.commands[1].hidden && p.commands[1].applies == AppliesTo::Any);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_plugin_cannot_name_a_program_outside_its_folder() {
        let root = scratch("escape");
        std::fs::write(root.join("outside.cmd"), b"x").unwrap();
        let d = plugin_dir(&root, "evil", "api = 1\ncommand.0.name = X\ncommand.0.exe = ..\\outside.cmd\n", &[]);
        let e = parse_plugin("evil", &d, &std::fs::read_to_string(d.join("plugin.ini")).unwrap()).unwrap_err();
        assert!(e.contains("outside"), "{e}");
        for bad in ["C:\\Windows\\System32\\cmd.exe", "\\\\server\\share\\x.exe", "sub/../../outside.cmd", "tool.ps1", "", "missing.exe"] {
            assert!(resolve_exe(&d, bad).is_err(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_and_newer_plugins_are_skipped_with_a_reason_and_the_rest_load() {
        let root = scratch("scan");
        plugin_dir(&root, "a_good", "api = 1\ncommand.0.name = Go\ncommand.0.exe = go.cmd\n", &["go.cmd"]);
        plugin_dir(&root, "b_newer", "api = 2\ncommand.0.name = Go\ncommand.0.exe = go.cmd\n", &["go.cmd"]);
        plugin_dir(&root, "c_empty", "api = 1\nname = Nothing\n", &[]);
        plugin_dir(&root, "d_noapi", "name = Old\n", &[]);
        plugin_dir(&root, "e_noout", "api = 1\ndecoder.extensions = q\ndecoder.exe = go.cmd\ndecoder.args = \"{input}\"\n", &["go.cmd"]);
        std::fs::create_dir_all(root.join("f_no_manifest")).unwrap();
        let l = scan(&root);
        assert_eq!(l.plugins.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a_good"]);
        let why = |id: &str| l.skipped.iter().find(|(i, _)| i == id).map(|(_, w)| w.clone()).unwrap_or_default();
        assert!(why("b_newer").contains("update NuViewer"));
        assert!(why("c_empty").contains("neither"));
        assert!(why("d_noapi").contains("api"));
        assert!(why("e_noout").contains("{output}"));
        assert!(why("f_no_manifest").contains("no plugin.ini"));
        assert_eq!(scan(&root.join("nope")), Loaded { dir: root.join("nope"), ..Default::default() }, "a missing folder is just no plug-ins");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn file_names_fill_their_own_argument_and_never_split() {
        let files = vec![PathBuf::from("C:\\p\\a b & c.jpg"), PathBuf::from("C:\\p\\d.jpg")];
        let args: Vec<String> = ["--in", "{file}", "--name={stem}.{ext}", "{files}", "{dir}", "{plugin}"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            expand_args(&args, Path::new("C:\\plug"), &files),
            ["--in", "C:\\p\\a b & c.jpg", "--name=a b & c.jpg", "C:\\p\\a b & c.jpg", "C:\\p\\d.jpg", "C:\\p", "C:\\plug"]
        );
    }

    #[test]
    fn commands_are_offered_only_for_files_they_apply_to() {
        let root = scratch("menu");
        let d = plugin_dir(&root, "p", "api = 1\nname = P\ncommand.0.name = Pic\ncommand.0.exe = r.cmd\ncommand.1.name = Raw\ncommand.1.exe = r.cmd\ncommand.1.for = raw\ncommand.2.name = Any\ncommand.2.exe = r.cmd\ncommand.2.for = any\n", &["r.cmd"]);
        let p = parse_plugin("p", &d, &std::fs::read_to_string(d.join("plugin.ini")).unwrap()).unwrap();
        let l = Loaded { dir: root.clone(), plugins: vec![p], skipped: vec![] };
        let names = |f: &str| l.commands_for(Path::new(f)).into_iter().map(|c| c.2).collect::<Vec<_>>();
        assert_eq!(names("x.jpg"), ["P: Pic", "P: Any"]);
        assert_eq!(names("x.CR2"), ["P: Pic", "P: Raw", "P: Any"]);
        assert_eq!(names("x.txt.zzz"), ["P: Any"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
