# Plug-ins (interface version 1)

A plug-in adds **a picture format** NuViewer cannot read, or **a command** to the right-click menu. It is a folder:

```
%APPDATA%\NuViewer\plugins\
    my-plugin\
        plugin.ini        <- the description (below)
        decode.cmd        <- the programs it uses: .exe, .cmd or .bat, inside this folder
```

Open the folder from the right-click menu (**Plug-ins > Open the plug-ins folder**), copy a plug-in folder in, then choose
**Plug-ins > Reload plug-ins**. The menu says how many were loaded and why any were skipped. Two examples are in
`docs/plugin-examples/`.

## The safety rules (why a plug-in cannot hurt the viewer)

* Nothing is loaded into NuViewer: a plug-in is **a separate program**, started when it is used. A crash or a hang stays in that program;
  a decoder that runs longer than its limit is stopped.
* A plug-in can only start **programs inside its own folder** (no `C:\...`, no `..`, no link that leads out; `.exe` `.cmd` `.bat` only).
* Programs are started **without a shell** and every file name is **one argument**, so a file called `a & del x.jpg` is just a long name.
* A decoder can only **add** formats: a format NuViewer reads itself is never handed to a plug-in.
* NuViewer never downloads or updates plug-ins. A plug-in runs with your user rights: **install only plug-ins you trust.**
* `plugins = off` in `%APPDATA%\NuViewer\settings.ini` loads nothing. `NUVIEWER_PLUGINS` points to another folder (tests, a portable copy).

## plugin.ini

`key = value` lines; `#` starts a comment. Unknown keys are ignored.

| key | meaning |
|---|---|
| `api = 1` | **Required.** The interface this plug-in was written for. A plug-in that asks for a newer one is skipped ("update NuViewer"). |
| `name`, `version`, `author`, `description` | Shown in the menu and in messages. `name` defaults to the folder name. |
| `decoder.extensions = xyz, abc` | The formats it reads (no dots). Extensions NuViewer already reads are dropped. |
| `decoder.exe = decode.cmd` | The program that converts one file to a PNG. |
| `decoder.args = "{input}" "{output}"` | Its arguments (default as shown). `{output}` is required: the `.png` it must write. |
| `decoder.timeout = 30` | Seconds before it is stopped (1 to 600, default 30). |
| `command.N.name = Edit in Paint` | N = 0, 1, 2 ... up to 31. The text in the menu (with the plug-in's name in front when it has several). |
| `command.N.exe = open.cmd` | The program, inside the folder. |
| `command.N.args = "{file}"` | Arguments, see below (default `{file}`). |
| `command.N.for = pictures` | Which files it is offered for: `pictures` (anything NuViewer can show), `raw` or `any`. |
| `command.N.multi = false` | `true`: one start with all selected files as `{files}`. `false`: one start per file (at most 50). |
| `command.N.window = normal` | `hidden` starts a console program without its window. |

Arguments are split like a Windows command line (spaces separate, `"` keeps a piece together), **then** these are filled in, each inside
its own argument: `{file}` the (first) file, `{files}` (as a whole argument) every file, `{dir}` its folder, `{name}` file name,
`{stem}` without extension, `{ext}` extension, `{plugin}` the plug-in's folder.

## A decoder, step by step

When NuViewer meets `shot.xyz` and `xyz` is a plug-in format, it runs `decode.cmd "C:\pics\shot.xyz" "C:\Users\you\AppData\Local\Temp\nuviewer-plugin-1234-0.png"`,
waits for it to end with exit code 0, reads the PNG (the **first frame**; RGBA or RGB, any size NuViewer accepts) and deletes it. The picture then
behaves like any other: gallery thumbnails, fit and zoom, next / previous, the info bar. A non-zero exit code, no PNG, or the time limit
become an error message that names the plug-in ("the plug-in "X" could not read this file (exit code 3)").

```
:: decode.cmd: needs ImageMagick (magick.exe on the PATH)
@magick "%~1[0]" "%~2"
```

## What it cannot do (yet)

Interface 1 has formats and menu commands, nothing else: no panels, no new keys, no hooks into editing, no access to NuViewer's memory.
That is deliberate: it is the part that can be made safe and kept stable. More is added as interface 2, 3 ... and `api = 1` plug-ins keep working.
Ideas and requests go to the support and feedback channel (`docs/TRUST.md` has the link).
