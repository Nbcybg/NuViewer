# NuViewer

A very fast, very light image viewer for Windows. Written in Rust on Win32 + Direct3D 11.

- Opens in about 60 ms for typical photos (about 125 ms for a 12 MP phone photo) and flips between images in a frame
  (neighbours are decoded ahead of time). EXIF rotation is done by the GPU, so rotated photos cost nothing extra.
- Shrinking to fit the screen stays sharp (mipmaps + bicubic resampling in linear light), unlike viewers that
  just bilinear-scale.
- A real gallery with cached thumbnails, EXIF panel, slideshow, and basic file management.
- Small (the whole app is one exe), no runtime. Comes with a per-user installer (no administrator rights).

## Formats

- **Photos:** JPEG, PNG/APNG, GIF (animated), WebP (incl. animated), PSD, camera RAW (embedded preview, developed from the
  sensor data when there is none), and, through Windows' own codecs: AVIF, HEIC (needs the Microsoft *HEVC Video
  Extensions*), TIFF, BMP, ICO, JPEG XR. Colour managed: a picture's embedded ICC profile (e.g. Display P3) is converted to the output profile you choose in
  Settings > Colour (sRGB, this PC's display profile from Windows, or any `.icc` / `.icm` file), and you decide what a
  JPEG / PNG / WebP *without* a profile is (sRGB, Display P3, Adobe RGB or a file). Switchable.
- **Game / render formats:** TGA (also `.icb`, `.vda`, `.vst`; RLE, colour-mapped, 16/24/32 bit), DDS (uncompressed and
  block-compressed BC1-BC7), PNM (PPM, PGM, PBM, PAM), Radiance HDR and OpenEXR (tone mapped to a normal picture), QOI.
- **Documents:** PDF, one page at a time (rendered by Windows' own PDF engine, so text and vector graphics stay sharp);
  Adobe Illustrator `.ai` saved with PDF compatibility (the default).
- **Vector art:** SVG / SVGZ (resvg, with system fonts), Windows metafiles EMF / WMF, EPS and old PostScript-style `.ai`
  (with Ghostscript if it is installed, otherwise the preview stored inside the file).
- **3D models:** STL (binary and ASCII), OBJ, PLY, glTF / GLB, binary FBX. They are drawn live on the GPU: drag to
  turn, wheel to zoom, `F` resets the camera, `U` swaps which axis is "up". Gallery thumbnails are rendered too.
  **Textures** (Settings > Viewer > 3D models, on by default): OBJ + MTL (`map_Kd`, `Kd`, one part per material), glTF / GLB
  (base colour texture, embedded or external, vertex colours), PLY (vertex colours, texture coordinates, `comment
  TextureFile`), binary FBX (diffuse texture, embedded or external). Texture files are looked for next to the model
  and in `textures/`; a missing picture just leaves the plain surface. Not supported: animation, point clouds,
  ASCII FBX, normal / roughness maps.

**Loading big pictures.** While the picture you asked for is still loading, the old one is dimmed and a panel shows what is
happening: reading from disk (bytes, MB/s), decoding, or copying to the graphics card, and the drive's overall read /
write speed and busy percentage, so you can tell when the disk is the bottleneck. The same disk readout can stay on
screen (`D`, or Settings > Viewer > Loading). The window title carries the same numbers, and the top bar shows how long
the file took to read. Huge pictures go to the GPU in small bands between window messages, so the window never freezes.

EXIF orientation is applied. Images beyond what the GPU can show (more than 16384 px a side or ~120 megapixels) are
scaled down to fit and say so; anything over 250 megapixels is refused.

**PDF pages.** `←/→`, Space, Backspace, PgUp/PgDn and the mouse wheel turn pages; past the last (or before the first)
page they carry on into the neighbouring file. `Home` / `End` jump to the first / last page, `Ctrl+←/→` skip the
pages and go to the previous / next file. The gallery shows page 1. Pages are rendered at about 1.5 x the screen's
long side, so zooming in far gets soft (re-rendering at the zoom level is not done yet).

## Keys and mouse

Press **F1** (or **H**) in the app for this list; these are the defaults and **Settings** changes them.

| Key / mouse | Action |
|---|---|
| ← → · Space · Backspace · PgUp PgDn · Home End | previous / next / first / last (pages first in a document) |
| `Ctrl+←` `Ctrl+→` · `Ctrl+Home` `Ctrl+End` | previous / next / first / last *file* |
| Mouse wheel | previous / next picture, page or frame (in the gallery: scroll; in 3D: zoom) |
| Hold left button · drag while held | zoom to 100% (200% if the picture is smaller than half the screen) and pan; release returns to the fitted view |
| `Ctrl` + drag | zoom into the dragged area (Esc cancels) |
| Click on a zoomed picture | back to the fitted view (dragging pans instead) |
| `Ctrl` + wheel | zoom around the pointer |
| `F` · `A` | fit · 100%. These are the two *base views*: pressing the one that is on switches to the other |
| `1` `2` `3` `4` `5` · `` ` `` | 1x … 5x of the base view (fit or 100%, whichever is on) · reset zoom |
| `]` `[` · `Shift+]` `Shift+[` | rotate +45° / −45° · +90° / −90° |
| `Ctrl+S` | save the rotation (JPEG / PNG are rewritten in place, other formats get a PNG copy; the original is backed up and `Ctrl+Z` restores it) |
| Pointer on the left edge · `V` | viewer controls: a vertical bar (next, previous, fit, 100%, fullscreen, NuEditor, ±45°, slideshow, image sequences, gallery, settings, help, close). It closes when the pointer leaves it; `V` keeps it open |
| `E` | NuEditor (mock-up: palette and inspector; only Rotate works yet) |
| 3D: drag · `Shift`+drag · wheel | turn the model · slide it (pan) · zoom; `F` resets all three |
| 3D: `T` `N` `W` `Shift+S` | texture · normal map · wireframe · shadow on/off |
| 3D: `Shift+T` `O` `X` `Y` `Z` | next shading · perspective / orthographic · turn the model 90° about X / Y / Z (Shift = back) |
| Right click | context menu (every action with its shortcut, *Open with* your programs, Settings) |
| Pointer on the top edge | info bar: name, size and megapixels, file size, position, speed, zoom |
| Pointer on the bottom screen edge | shows the Windows taskbar while in fullscreen |
| `G` · Tab · Enter | gallery (arrows select, wheel scrolls, Ctrl+wheel changes size) |
| `F11` · `Alt+Enter` | windowed ↔ fullscreen (pictures open fullscreen; the pointer hides when idle) |
| `I` | file info and EXIF |
| `S` · `+` `-` · `R` | slideshow · faster / slower · shuffle |
| `Q` | fold numbered pictures into image sequences / show every picture (gallery) |
| `P` · `C` | play / pause a sequence or animation · keep the playback bar on screen |
| `F2` | rename (the name part is pre-selected) |
| `Del` · `Shift+Del` | Recycle Bin · delete permanently |
| `F5` · `F6` / `M` | copy to… · move to… |
| `Ctrl+Z` | undo rename / move / copy / delete |
| `Ctrl+C` · `Ctrl+Shift+C` · `Ctrl+E` | copy image + file · copy path · show in Explorer |
| `Ctrl+,` · `F9` | Settings |
| Middle mouse button | close the viewer (not in the gallery) |
| `Esc` | close panel / stop slideshow / back from the gallery / quit |

## Settings

`Ctrl+,` (or right click > Settings, or `nuviewer.exe --settings`) opens the Settings window; OK applies and saves at once.

- **Colour:** colour management on/off, the output profile (sRGB / the Windows display profile / a profile file) and the
  assumed profile of pictures that carry none. Saved rotations and the clipboard always stay sRGB.
- **Viewer:** background colour, open fullscreen or windowed, what holding the left button does
  (automatic 100% / 200% for small pictures, always 100 / 200 / 400%, off) and what counts as "small", image sequences
  (fold, minimum length, playback speed), keep the playback bar visible, always show the disk read / write speed, and
  load the textures of 3D models, the left-edge viewer controls on/off, and what happens to a rotated picture when you
  move on (ask every time, or save automatically).
- **File types:** the format check-box list (same as `--associate`), applied with one button.
- **External programs:** a list of programs the current picture can be sent to (*Open with* in the right-click menu).
  The command line uses `{file}` for the path, e.g. `"C:\Program Files\Adobe\Adobe Photoshop\Photoshop.exe" "{file}"`.
- **3D:** shading (combined, diffuse, cartoon, normals, unlit), texture / normal map / wireframe / floor shadow, the light
  (from the viewer, or fixed in the scene; angle and height), perspective or orthographic, which axis is up and a
  90° turn per axis for models that arrive face-down. The T / N / W / Shift+S / O / X / Y / Z keys change the same values.
- **Mouse and keyboard:** wheel action (pages / zoom / nothing), the top info bar,
  the taskbar reveal, and every keyboard shortcut (several keys per action, conflicts are refused, Reset restores
  the default).

Everything is stored in `%APPDATA%\NuViewer\settings.ini`, a plain `key = value` file you can also edit by hand
(unknown or invalid lines are ignored). The environment variable `NUVIEWER_SETTINGS` points to another file.

### Image sequences and playback

A run of files named `name_0001.png, name_0002.png, …` (same name, same extension, numbers one apart, at least the
configured length) can be folded into one gallery tile with a frame count (`Q`, or Settings). Opening it shows the
first frame; **P** plays it at the configured frame rate, `+`/`-` change the rate. RAW and JPG of the same shot are never
folded together (a run only holds files with the same extension), and folding is **off by default** because camera
files (`DSC_0001.jpg, DSC_0002.jpg …`) look exactly like a sequence.

The **playback bar** at the bottom of the viewer (near the bottom edge, or always) controls whatever plays over time:
a slideshow, an animated GIF/APNG/WebP (play/pause, frame stepping, seek, speed) or a sequence (play/pause, frame stepping,
seek, frames per second). It is built so video and music can use the same bar later (play/pause, seek, time, speed,
volume); there is no video or audio decoder yet.

## Using it

```
nuviewer.exe <image-or-folder> [--gallery] [--new-window] [--windowed] [--actual-size]
nuviewer.exe --settings       # the Settings window
nuviewer.exe --associate      # tick which formats Windows should open with NuViewer (check-box list)
nuviewer.exe --register       # register all formats at once (current user only)
nuviewer.exe --unregister     # remove the registration again
```

Pictures open borderless fullscreen from the very first frame (no windowed frame first); `--windowed` opens a normal
window instead (`--fullscreen` is still accepted and means the default). `--actual-size` shows the first image at 100%
instead of fitted, handy for timing and pixel peeping.

Flags can go anywhere on the command line. **One window:** if NuViewer is already running, a second launch hands
its file/folder to the existing window (which comes to the front) and exits within a few milliseconds; a launch
with no arguments just brings the window forward. `--new-window` opts out. If the running window can't take the
request (it is stuck in a dialog, say) the new launch opens its own window after about three seconds.

Windows doesn't let a program make itself the default viewer. `--associate` (and the installer) therefore register
the formats you tick and then open Settings > Apps > Default apps > NuViewer, where each type has a "Set default"
button; the message afterwards says how many types already open with NuViewer and how many still wait for you there.
The same list is in the Start menu as *NuViewer file types*.

Thumbnails are cached in `%LOCALAPPDATA%\NuViewer\thumbs` (trimmed to 512 MB, least recently used first).
If something goes wrong the details are written to `%LOCALAPPDATA%\NuViewer\crash.log`.

## Building

Needs a recent stable Rust (MSVC toolchain) on Windows.

```
cargo build --release
cargo test
```

The exe is `target\release\nuviewer.exe`. If the drive holding the project is short on space, point cargo elsewhere
with `CARGO_TARGET_DIR`.

`tools\package.ps1` builds a release and produces

- `dist\NuViewer-<version>-setup.exe`: installer and uninstaller in one file (see below),
- `dist\NuViewer-<version>-win64.zip`: portable copy (exe, this file, and `.cmd` helpers for the file types).

### Installer

Double-click `NuViewer-<version>-setup.exe`: choose the folder (default `%LOCALAPPDATA%\Programs\NuViewer`), tick the
image formats and the Start menu / desktop shortcuts, press Install. Everything is per user (HKCU), no UAC prompt.
Removal is Settings > Apps > Installed apps > NuViewer (or `uninstall.exe` in the install folder). Silent use:

```
setup.exe /S [/D=<folder>] [/formats=all|none|default|jpg,png,cr2] [/nostartmenu] [/desktop] [/launch] [/log=<file>]
uninstall.exe /uninstall /S
```

Tests: `cargo test` per crate, plus `tools\test\e2e.ps1`, which drives the real installer, the format chooser and the
middle-click close against a scratch registry key and temp folders (your own file associations are never touched).
`tools\test\e2e-mouse.ps1` checks default fullscreen, wheel navigation, hold-to-zoom, area zoom and the context menu by
sending messages to the viewer window and reading its pixels back, so it never moves your real mouse (add
`-RealInput` to also test the taskbar reveal at the bottom edge; it skips itself if another program holds the focus).

## Layout

```
crates/nv-core     folder listing, decode pool + prefetch, thumbnails, gallery layout, file operations
crates/nv-codecs   all decoders, EXIF/ICC handling, resizing, metadata
crates/nv-render   Direct3D 11 renderer (textures, mips, bicubic shader, overlays)
crates/nv-win      Win32: window, input, text rasterising, shell integration, registry, form toolkit + file-type chooser, crash handling
nuviewer           the app (start-up, viewer, gallery, HUD, file actions)
setup              installer / uninstaller (carries a compressed nuviewer.exe)
```

Dev tools: `cargo run --release -p nv-codecs --example probe -- <files>` decodes files and prints timings and
pixel statistics (it never prints file names); `gen_test_anim` and `gen_big` write synthetic test images.
