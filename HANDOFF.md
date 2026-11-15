# NuViewer — handoff

Written 2026-11-11. Read this first, then `README.md` (user-facing) and the plan referenced in the project memory
(`%USERPROFILE%\.claude\plans\valiant-conjuring-hopper.md`). The project is on GitHub (Nbcybg/NuViewer);
`dist/` and the generated benchmark pictures are not committed.

## 1. State in one paragraph

NuViewer is a fast, light Windows image viewer in Rust + Win32 + Direct3D 11 (workspace crates: `nv-core`, `nv-codecs`,
`nv-render`, `nv-win`, `nuviewer`). Milestones M0–M5 are done (viewer, gallery, many formats, file ops, EXIF, slideshow,
single instance, crash safety, size guards). This session was about **speed on real photos and camera RAW**, compared
head to head with FastStone 8.3. NuViewer is faster in every test except one (see §5, open item 1).

### Session 3 (2026-11-12): installer, file-type chooser, middle click

- **Middle mouse button closes the viewer** (`Event::MiddleClick`, sent on WM_MBUTTONUP so the "up" can't land on a
  window behind ours). Viewer mode only: ignored in the gallery and while the rename box is open; also closes during
  the start-up `Boot` phase. Listed in the help overlay and README.
- **File-type chooser with check boxes**: `nuviewer.exe --associate` (also the Start menu entry "NuViewer file types").
  21 entries (11 pictures + 10 camera-RAW families, table `FORMATS` in `crates/nv-win/src/assoc.rs`; integration test
  `nuviewer/tests/formats.rs` fails if it drifts from `nv_codecs::supported_extensions()`). `registry::apply(all, enabled)`
  registers exactly the ticked ones and removes the rest. **Windows limitation, verified on this PC: every image
  extension has a `UserChoice` owned by Photos (UWP) and that value is hash-protected, so no program can take over the
  default silently.** After applying we read `UserChoice` back (`assoc::describe`), say how many types already open with
  NuViewer, and open `ms-settings:defaultapps?registeredAppUser=NuViewer` for the rest. We never write UserChoice.
- **Installer** = new crate `setup/` (`nuviewer-setup.exe`, ~3.2 MB with a deflated nuviewer.exe inside; the installed
  copy is `uninstall.exe`). Per user, no UAC: `%LOCALAPPDATA%\Programs\NuViewer`, HKCU registration, Start menu (+ "file
  types" shortcut), optional desktop shortcut, Add/Remove Programs entry (`UninstallString`, `QuietUninstallString`),
  closes a running viewer before upgrading, uninstaller removes files/registry/shortcuts/thumbnail cache and the folder
  (via a detached `cmd` after exit; `rmdir` without /s so foreign files survive). Wizard + silent switches are documented in
  `setup/src/main.rs` and README. `setup/build.rs` embeds `$NUVIEWER_EXE` (else `<target>\release\nuviewer.exe`; empty
  payload with a warning if missing). `tools\package.ps1` builds both and writes `dist\NuViewer-<ver>-setup.exe` + zip.
- **UI toolkit** `crates/nv-win/src/ui.rs`: modal `Form` from plain Win32 controls (dip coordinates, system DPI, Tab/Enter/Esc
  via IsDialogMessage). `tools/app.manifest` is embedded into nuviewer.exe and the setup with link args
  (`/MANIFEST:EMBED`, `/MANIFESTINPUT`), no rc.exe: visual-styles controls, PerMonitorV2, longPathAware.
- **Tests**: nv-win 10 (registry apply/registered_exts, assoc), setup 3 (install -> re-install with fewer formats ->
  uninstall in a scratch HKCU key + temp folders), nuviewer 1 (format list). `tools\test\e2e.ps1` (30 checks) drives the
  real installer/wizard/chooser windows and the middle click against a scratch registry key; all pass.
  **Not verified:** a *real* mouse middle click (another always-on-top window covered the click point here, so the
  script fell back to posting WM_MBUTTONUP, which does close the viewer); the chooser's Apply and the Settings page
  with the real registry (deliberately not run, it would register NuViewer on this PC); HiDPI > 100% layout of the forms
  (scaled by `GetDpiForSystem`, only seen at 100%); upgrade over a running viewer in the wizard path.
- Still no exe icon / version resource (shortcuts show the generic icon). Gotcha: the Bash tool rewrites backslashes in
  here-doc python (`\\n` became a real newline, `\N` raised a unicode error): use the Edit tool for any text with `\`.

### Session 4 (2026-11-12, after the user tried the installer): context menu, fullscreen default, new mouse scheme

The user installed the 0.1.0 setup for real (HKCU registration of all 40 extensions, `%LOCALAPPDATA%\Programs\NuViewer`).
Test scripts must therefore never assume a clean machine and never kill every `nuviewer` process (`e2e.ps1` now
compares the real registration before/after and only stops processes it started).

- **Fullscreen by default**: `nv_win::run(.., fullscreen = !--windowed)`; `--fullscreen` is accepted as a no-op.
  **Esc no longer leaves fullscreen first**: it closes panels / stops the slideshow, else quits (gallery: back). F11 toggles.
- **Mouse scheme** (`nuviewer/src/mouse.rs`, state in `View.held` / `View.area`):
  - wheel = previous / next (120 per step, fractions accumulate in `wheel_acc`; ignored while a hold/area gesture is active;
    in the gallery it still scrolls); **Ctrl+wheel** keeps the old zoom-at-cursor.
  - hold left button = zoom to 100% around the pointer (**200% when the picture is at most half the screen in both
    directions**, `View::hold_scale`, screen = monitor resolution), drag pans while held, release restores the previous view
    exactly. The double-click toggle is gone: `WM_LBUTTONDBLCLK` now also captures the mouse and the app treats it as a press.
    `WM_CAPTURECHANGED` is turned into `Event::MouseUp` (idempotent) so a lost capture can't leave the picture zoomed.
  - Ctrl + left drag = rubber-band area zoom (`View::zoom_to_rect`, drag under 8 px is ignored). Ctrl is read from the message
    (`MouseDown/DoubleClick.ctrl`, MK_CONTROL), not GetKeyState, so tests can post it.
  - right click = native popup menu (`nv_win::menu`, 22 entries, 3 separators). Items that are keyboard shortcuts reuse
    them: id = `vk | mods << 16`, executed via `nv_win::call_with_mods` + `handle(Event::Key)`; "File types…" spawns
    `--associate`, "Close" quits. In the gallery the tile under the pointer is selected first.
  - **Taskbar at the bottom edge**: `ITaskbarList2::MarkFullscreenWindow` via `nv_win::set_taskbar_hidden` (hidden when
    entering fullscreen, shown/hidden again from `App::edge_tick`, an 80 ms timer polling `GetCursorPos`: pointer on the
    last screen row for 2 ticks shows it, leaving the taskbar's height + 6 px hides it). Fighting the taskbar's z-order by
    hand was tried first and does not work on this Windows 11 (the shell hides the tray window itself).
- Tests: `tools\test\e2e-mouse.ps1` (29 checks, all pass): default fullscreen geometry, wheel steps and half-notch
  accumulation, hold-zoom 100% / 200% with the pointer point fixed, pan, release, area zoom geometry and rubber band, tiny
  Ctrl-click, menu contents + choosing an item through the menu's keyboard handling + Esc. It posts messages to the
  viewer window and reads the window back with PrintWindow, so it works when another program has the focus.
  `tools\test\e2e.ps1` still 30/30.
- **NOT verified: the taskbar reveal** (and the fact that MarkFullscreenWindow(true) really hides the taskbar on this
  PC). A game (Black Desert, windowed fullscreen) was running and keeps the foreground and clips the cursor, so the
  shell logic and `GetCursorPos` polling could not be exercised. Run `tools\test\e2e-mouse.ps1 -RealInput` with no game
  running; it refuses to send any input unless the viewer is foreground and the pointer actually moves. Also unverified:
  a real (hardware) left-button hold / right click (only messages were sent), DPI > 100%, multi-monitor edge handling.
- **Incident to remember**: the first version of the mouse test sent *real* mouse/keyboard input while the game had the
  focus, so a left click, a right click, Ctrl, Alt and Esc went into the game window (at about 1921,795). The script was
  rewritten to messages only; never send real input unless the target window is verified to be in front.

### Session 5 (2026-11-12): view fixes, new formats, 3D, Settings, sequences, playback bar

**Viewer fixes (all covered by `tools\test\e2e-mouse.ps1`, 54 checks):** zoom anchors at the pointer clamped to the
picture (over the canvas it goes to the picture edge) and `View::clamp_pan` keeps picture edges from ever entering the
window; Ctrl+wheel zooms the same way and stops at "fit"; a plain click on a zoomed picture cancels the zoom (drag pans;
Esc cancels a Ctrl+drag); Alt+Enter toggles fullscreen (`WM_SYSKEYDOWN` handled in nv-win); opening from the gallery no
longer leaves the previous picture up (`Renderer::hide_current` + thumbnail placeholder); top-edge info bar (name,
size + MP, file size, n/N, decode / shown-in ms, zoom or page).

**New formats** (all in `nv-codecs`, unit-tested with generated files): PDF + PDF-compatible `.ai` via
`Windows.Data.Pdf` (`pdf.rs`; pages are addressed by the pseudo path `file.pdf?page=N`, `?` can't be in a Windows file
name; `App.doc_page`, `step()`, `set_page()`, neighbouring pages prefetched), SVG/SVGZ via resvg (`svgdec.rs`), EMF/WMF
via GDI+ (`metafile.rs`), EPS and old `.ai` (`eps.rs`: Ghostscript if installed, else DOS-EPS TIFF/WMF preview or ASCII
preview), 3D: STL/OBJ/PLY/glTF-GLB/binary FBX (`model.rs`; `Image.mesh`; software preview for thumbnails; live D3D11 draw in
`nv-render/src/mesh.rs` + `shaders/mesh.hlsl`, created lazily; drag turns, wheel zooms, F resets, U flips up axis).
Not done: PDF re-render at zoom level, 3D materials/textures/animation/point clouds/ASCII FBX, FBX node transforms.
No video or audio decoding.

**Settings** (`nv-core/src/{settings,keys,sequence}.rs`, `nuviewer/src/{settings_ui,actions}.rs`): `settings.ini` in
`%APPDATA%\NuViewer` (override: env `NUVIEWER_SETTINGS`; **always set it in tests**). Every key now goes through
`Keymap` -> `Action` -> `App::perform` (the old hard-coded key code is gone; the context menu runs actions too, so it
follows remapped keys). Settings window: Viewer / File types / External programs / Mouse & keyboard (`Ctrl+,`, `F9`,
right click, `nuviewer.exe --settings`). External programs appear under *Open with* in the menu (`shell::run_external`).
Wheel action, hold-zoom level, edge-click zones, top bar, taskbar reveal, canvas colour, ICC on/off
(`nv_codecs::set_icc_enabled`) are all honoured.
**Image sequences**: `sequence::find_runs`; the gallery shows `items` (file indices) instead of every file when folding
is on (`gallery.selected` stays a *file* index, tiles are mapped with `file_of`/`tile_of`); default OFF because camera
files look like sequences; RAW/JPG are never merged (a run needs the same extension).
**Playback bar** (`controls.rs`): slideshow, animation (pause/step/seek/speed) and sequence playback share one bar;
`MediaKind::{Video, Audio}` are reserved, nothing decodes them yet. `P` play/pause, `C` pin the bar.
Behaviour change: `+` now means *faster* (shorter slideshow interval); it used to lengthen the interval.
F1 help starts with the version (unit-tested).

**Tests** (all passing at the end of the session): unit nv-core 41, nv-win 11, nv-codecs 63, setup 3, nuviewer 3;
e2e `e2e.ps1` 30 (installer), `e2e-mouse.ps1` 54 (+1 skipped: taskbar reveal needs `-RealInput`),
`e2e-features.ps1` 61 (settings file effects, edge click, keymap, PDF pages, SVG, 3D, sequences + bar, GIF pause,
context menu, Settings window incl. conflict refusal and saving). `tools/test/lib.ps1` holds the shared helpers.
**Still unverified:** taskbar reveal at the bottom edge (a game holds focus here), real hardware mouse input, DPI > 100%,
multi-monitor, Ghostscript path of EPS (not installed here), FBX with real files (only parser error paths tested).
`Dir` and `Move` are PowerShell aliases: never name a test helper that way (it bit twice).

### Session 6 (2026-11-12): big-picture loading, smooth navigation, TGA & co, 3D textures

User complaints: (1) opening / stepping to a 100 MP+ picture looks like a hang (old picture stays up, only the title says
"loading"), and wanted the disk's read / write speed to see bottlenecks; (2) prev / next on big pictures freezes the UI;
(3) TGA (and other formats) don't open; (4) 3D models need a setting that loads their textures.

- **Progress plumbing** (`nv-codecs/src/progress.rs`): a pool worker installs a `JobProgress` (thread-local) per job;
  every decoder reads files through `progress::read_file` (2 MB chunks, counters, cancel flag, "hold" gate). WIC formats
  are now read by us and decoded from memory (files <= 1 GB) so they are measurable too. `Loader` (`nv-core/loader.rs`):
  first path of a request is *urgent* and has a reserved worker; running jobs no longer wanted are cancelled (they stop at
  the next read chunk, result has `cancelled = true` and is not an error; `App.retry` re-requests); prefetch reads wait
  while the urgent one is still reading (HDD seek thrash); prefetch workers run below normal priority
  (`progress::set_background`, also in the resize / icc helper threads); `Loader::state_of(path)` -> `LoadState`;
  `Loaded.snap` carries bytes read + read seconds (top bar: "read 120 MB in 3.2 s (37 MB/s)").
- **Loading panel** (`nuviewer/src/loading.rs`): after 150 ms of loading the old picture is dimmed (55% black) and a
  panel shows stage (waiting / reading n of m MB + MB/s / decoding s / sending to GPU %), a bar (orange when the disk is
  >= 85% busy: "The disk is the bottleneck"), and the drive's read / write speed + busy %. `nv-win/src/diskstat.rs`: PDH
  counters `\LogicalDisk(F:)\Disk Read/Write Bytes/sec` and `% Idle Time`, sampled every 400 ms on a thread that only runs
  while someone called `watch()` in the last 5 s. Setting `disk_monitor` / key `D` keeps the readout up (bottom-right).
  Title shows the same numbers while loading (the e2e test reads them).
- **No UI freeze**: pictures >= 128 MB of pixels (`STAGED_UPLOAD_BYTES`) are copied to the GPU in 6 MB bands
  (`Renderer::begin_upload / step_upload / upload_progress / cancel_upload`, `App::pump_uploads`), ~8 ms per slice, next
  slice from `TIMER_UPLOAD` (a timer, not a posted message, so WM_PAINT and input run in between). The picture on screen
  goes first (a prefetch upload in progress is set aside), the freed CPU copy is dropped on another thread, and
  neighbours are only prepared if they fit the GPU budget (40% of the card's memory, 768 MB .. 2.5 GB; sizes of decoded
  pictures remembered in `App.sizes` so a neighbour that doesn't fit is not decoded again); next to a > 256 MB picture only
  one neighbour is prepared. Smaller pictures still upload in one go (as before, ~11 ms paced next).
- **New formats** (`nv-codecs/src/imgfmt.rs`, `image` crate with only tga/pnm/hdr/exr/qoi + `image_dds` for DDS incl.
  BC1-BC7): TGA (+icb/vda/vst), DDS, PPM/PGM/PBM/PAM/PNM, HDR, EXR, QOI. HDR/EXR are tone mapped when anything exceeds 1.0.
  The file-type chooser has four new entries (the Settings window grew to 704 px high). Not added: JPEG XL, JPEG 2000, PCX,
  SGI, XCF, ...
- **3D textures** (setting `model_textures`, default on; `nv_codecs::set_model_textures`; Settings > Viewer > 3D models):
  `Mesh` now has `uvs`, `colors`, `textures: Vec<ModelTexture>`, `parts: Vec<Part>` (index ranges with texture / colour /
  alpha cut). OBJ+MTL (map_Kd with options, Kd, one part per `usemtl`, texture lookup tolerant of absolute / backslash
  paths, textures/ folder and wrong extensions), glTF/GLB (baseColorTexture or specular-glossiness diffuse, embedded
  bufferView / data URI / file, baseColorFactor, COLOR_0, alphaMode MASK/BLEND), PLY (vertex colours, s/t/u/v,
  per-face `texcoord` lists, `comment TextureFile`), binary FBX (UV layer, material layer, diffuse texture from the video
  clip's embedded content or file name). GPU: `mesh.hlsl` samples R8G8B8A8_UNORM (sRGB numbers, like the grey path),
  36-byte vertices, one draw per part. Thumbnails / first frame are software rendered with textures (perspective-correct
  uv, nearest from a <= 1024 px copy). **FBX is untested on real files** (no sample on this PC; only compile + error paths).
  Changing the setting reloads everything (`App::reload_models`).
- Measured (this PC, `e2e-loading.ps1`, 108 MP / 424 MB noise PNG, warm cache): longest window ping during the load
  9-13 ms with the new build vs 152-163 ms with the 0.1.0 dist build (the staged upload is the difference). The disk
  line of the panel needs ~0.8 s of sampling, so it is absent on loads shorter than that; the *reading* stage was only
  seen for a moment because the file was in the OS cache (a cold HDD shows the MB/s and the orange "disk is the
  bottleneck" bar; not exercised here). Existing suites after the change: e2e-mouse 54/54, e2e-features 61/61 (keyboard
  list now 37 actions: new `D`), unit tests all green (nv-codecs 84, nv-core 44, nv-win 13, nuviewer 3, setup 3).
- Tests: nv-codecs 84 (new: progress 5, imgfmt 8, model textures 9), nv-core loader tests, nv-win diskstat 2;
  `tools\test\e2e-loading.ps1` (panel pixels, ping latency during a 108 MP load with optional `-Baseline` old exe, quick
  navigation, TGA/PPM, textured OBJ on/off).

### Session 7 (2026-11-13): zoom keys, left-edge controls, rotation + save, 3D style, NuEditor mock-up, gallery split prep

The user's 8-point list. Verified with `tools\test\e2e-session7.ps1` (64 checks, messages only) and the older suites.

1. **Zoom keys.** `View` got `base_actual` (kept from picture to picture): F / A choose the base (the one that is on switches
   to the other), 1-5 = n x base around the window centre (`View::set_multiple`), `` ` `` = `reset_to_base`. In 3D the digits set
   the camera zoom. The old "1 = 100%" is now `A`. A click on a zoomed picture returns to the base, not always to fit.
2. **Edge click removed** (setting, hint, `edge_press`); old `edge_click` / `edge_pct` lines in settings.ini are ignored.
3. **Viewer controls** `sidebar.rs`: 14 buttons in the order the user gave, every button is an `Action`. Opens when the pointer
   touches the left edge (6 px), closes when it leaves the bar (+10 px), `V` pins it, setting `sidebar`. Tooltip shows label + key.
   Posted test clicks need the bar opened and the button hovered *before each click* (Windows sends a mouse move at the real
   pointer position after a click, which closes the bar): see `Bar-Click` in the test.
4. **3D style** (`nv_codecs::model::ModelStyle`, saved as `model_*` settings, Settings > 3D page): shadings combined / diffuse /
   cartoon / normals / unlit; light from the viewer or fixed in the scene (angle + height); perspective / orthographic; up axis
   Auto/Y/Z; quarter turns about X, Y, Z. `camera_styled` builds the matrices; `mesh.hlsl` has the looks and four passes
   (normal, floor shadow, wire lines, dimmed solid under the wire).
5. **3D keys** `model3d.rs`: T texture, N normal map, W wireframe, Shift+S shadow, Shift+T next shading, O ortho, X/Y/Z turn.
   Normal maps: OBJ MTL `map_Bump`/`bump`/`norm`, glTF `normalTexture`; the shader builds the tangent frame from screen-space
   derivatives. **Not verified visually** (no model with a normal map on this PC): the green channel flip is a guess for
   y-up maps. Shadow = the model flattened onto the floor along the light (stencil so overlaps don't double-darken); it needs
   the light above ~15 degrees. Gallery thumbnails keep the file's own orientation (not the user's axis fix). Checked by eye
   on an STL cube: combined, shadow and wireframe look right (screenshots were in the session scratchpad).
6. **Rotation** `rotate.rs` + `nv-editor`: the turn is a transform in the quad vertex shader (`rot` in the 96-byte cbuffer),
   `view_size()` gives the box around the turned picture to all view maths. Pending until saved. Save = decode again, turn
   (`nv_editor::raster`, exact for right angles, bilinear with transparent corners otherwise), JPEG q95 4:4:4 with the EXIF
   block carried over (orientation reset to 1, pixel size patched) or PNG; other formats get `name_edited.png`; written
   atomically; the original goes to `%LOCALAPPDATA%\NuViewer\undo` and `UndoOp::Rewritten` brings it back with Ctrl+Z.
   Setting `rotate_save` = ask (default; Yes / No / Cancel) or auto. Settled before: next / previous / first / last, gallery,
   opening another path, Esc / Quit / middle click / the window's close box (`nv_win::veto_close`). Ctrl+Z while a turn is
   pending takes the turn back first. Not rotatable: 3D, animations, document pages. JPEG corners of a 45 degree turn are black.
   Saving runs on the UI thread (a 12 MP JPEG takes a few hundred ms). Backups older than 7 days are pruned by the
   start-up housekeeping thread (`save::prune_backups`).
7. **NuEditor mock-up** `editor_ui.rs` + `nv-editor::tools`: palette (9 tools, only Rotate is `Ready`) + inspector with
   Rotate +-45 / +-90, Save, Revert; other tools show their hint and dummy sliders. `E` opens, `E` / Esc closes.
8. **Gallery split prep** `gallery.rs`, `docs/gallery-split.md`: gallery code moved out of `main.rs`, thumbnail workers start lazily.

New crate `crates/nv-editor` (16 tests): `raster` (rotate / flip / flatten), `ops` (EditStack with undo / redo, merges
rotations), `save` (encode, EXIF patch, backup, atomic write), `tools` (the catalogue). The keyboard list has 59 actions now.
Tests: nv-core 44, nv-codecs 92 (8 new camera / light / shadow / MTL tests), nv-editor 16, nv-win 13.

### Session 8 (2026-11-13): alt-tab fix, error codes, new formats, sort / shuffle / film strip, read mode

The user's 7-point list. Tests: unit tests in every crate (nv-core 57, nv-codecs 139 + 7 integration with real encoder output,
nv-win 14), `tools\test\e2e-session8.ps1` (49 checks, messages only), older e2e scripts updated for the new counts.

1. **Alt+Tab confusion.** Settings and every other form (`nv_win::ui::Form`) were top-level windows with no owner, so Alt+Tab
   landed on the main window, which sat in the form's nested message loop and looked frozen. Now `nv_win::main_window()`
   (set in `nv_win::run`) is the owner of every `Form` and of `shell::message`; the form disables the owner while it runs and
   re-enables it *before* destroying itself (`Form::close`), is centred on the viewer's monitor and has no minimise box. "File
   types…" in the context menu runs in-process (`associate_dialog()`) instead of spawning `--associate`. Message boxes and the
   folder picker already had `self.hwnd` as owner. Verified: `GetWindow(form, GW_OWNER) == viewer`, viewer disabled while open.
2. **Error codes** `nv_codecs::errors` (`NV-xxxx`, 29 codes, table `TABLE`, user page `docs/error-codes.md`, a unit test keeps
   the two in step). `decode_file` / `decode_file_first_frame` tag failures (`errors::coded` classifies the decoder's text), the
   loader tags a panicking decoder 6001, GPU upload failures 5001, file operations / settings / colour use `hud::failure(code, text)`
   (toasts end with `[NV-7002]`), the crash dialog and `crash.log` say NV-6002. A picture that fails now gets an **on-screen panel**
   (`loading.rs: draw_error`: code, title, the decoder's words, advice) and `I` shows "Error" rows.
3. **Formats** (`crates/nv-codecs`): PSB + a PSD fallback reader `psdraw.rs` (merged image, raw / RLE / ZIP, 8 / 16 bit, RGB / CMYK /
   grey / indexed / Lab); JPEG XL `jxldec.rs` (jxl-oxide, animated); JPEG 2000 `jp2.rs` (hayro-jpeg2000: jp2 j2k j2c jpf jpx jpc); FLIF
   `flifdec.rs` (flif crate); `icon.rs` = DIB, ICO fallback, CUR, ICNS (PNG / JP2 payloads, old types via the icns crate); `pcx.rs`;
   `dicom.rs` (first frame; uncompressed, RLE, JPEG baseline, JPEG 2000; window / rescale; palette, RGB, YBR); `swf.rs` (largest
   embedded bitmap only); RGBE = `.rgbe` through the HDR decoder; `.hif` via WIC; X3F joined `RAW_EXTS`. APNG, JPE, SVGZ, DDS, PNM, WMF,
   the other RAW extensions, DNG already worked. **ECW** is proprietary: listed in folders (`TRY_EXTS`), asks WIC, else NV-2004 with
   a "convert with GDAL" hint. OptiPNG outputs are plain PNGs. Not verified on real files: FLIF (no encoder here), DICOM, PSB
   (synthetic tests only), X3F. `MORE_EXTS` / `TRY_EXTS` / the chooser (`assoc.rs`, now 40 formats; Settings > File types buttons moved to the
   OK row) must stay in step: `nuviewer/tests/formats.rs` checks it.
4. **Order** (`nv_core::folder`: `SortKey`, `Order`, `FolderList::set_order`): name (natural, the old and default order), date,
   size, type, descending, and **shuffle** = a seeded hash order (stable, files added later land where the shuffle puts them).
   One list drives next / previous, gallery, film strip, slideshow (the slideshow's own shuffle is gone: `R` shuffles the list).
   Keys `J`, `Shift+J`, `R`; context menu "Sort by"; Settings > Mouse & keyboard; `sort` / `sort_desc` saved, shuffle not.
   `nuviewer/src/order.rs`.
5. **Film strip** `strip.rs`: right edge (6 px) or `L`; header chips = sort keys, direction, shuffle; thumbnails reuse the gallery's worker pool,
   disk cache and `renderer.upload_thumb`; only visible rows ± a few are requested (`strip_request_thumbs`). Hit-testing is shared by
   hover, click and wheel (`strip_hit`). Setting `film_strip`.
6. **Read mode** `read.rs` + `nv_core::spread` (pure pairing logic, tested): `B`, `Shift+B` (pairs start here), `Ctrl+B` (direction),
   settings `read_start` (default 2: cover alone) and `read_rtl`. Wide scans (`peek_size`, header-only, cached) stand alone. The pair is
   drawn as one virtual picture (`SpreadView`; `view_size()` returns it) so zoom / pan / hold-zoom work; the seam is snapped to a whole pixel
   (verified: no gap). New renderer calls `draw_image(id, rect)` and `image_size_of(id)`. Rotation is off in read mode.
7. Answer to "what do next / previous walk": the folder's files in **natural name order**; there was no other order before this session.

New keys: `J`, `Shift+J`, `L`, `B`, `Shift+B`, `Ctrl+B` (R now shuffles the list). Sidebar has 16 buttons (film strip, read mode after Gallery).

### Session 9 (2026-11-14): RAW developed, CMYK through profiles, document thumbnails, decode statistics

The user's 11-point list. Details and numbers: `docs/raw-development.md`, `docs/colour-cmyk.md`, `docs/gallery-split.md`.

1. **RAW.** `develop.rs` rewritten: the main path is development from the sensor data, `RawMode::{Full (default), Half, Preview}`
   (setting `raw_mode`, env `NUVIEWER_RAW`). Only camera metadata is applied (levels, as-shot WB, colour matrix interpolated A..D65
   at the WB's temperature, sRGB curve); **auto exposure and highlight roll-off removed**. Own threaded demosaic (Malvar-He-Cutler for
   Bayer, 3x3 same-colour average for X-Trans, binned half size). Found and fixed on the way: the old code took
   `HashMap::values().next()` as the colour matrix (random illuminant). Time before / after (warm): CR2 67 -> 349 ms, RAF 81 -> 273,
   DNG 169 -> 332 (full), 223 / 123 / 169 (half). RAW thumbnails stay preview-based (`raw_thumbs` develops them; reads whole files).
2. **Gallery / viewer survey and decisions** (`docs/gallery-split.md`): counts of every coupling, film strip stays in the viewer, Esc =
   close viewer and raise NuGallery with the last seen picture selected (viewer sends `seen <path>`), no shared list (both build it:
   0.4-1.9 ms), documents only in the gallery's list. Nothing split yet; `dpi` living in `Gallery` is the first thing to move.
3. **Document thumbnails** `docthumb.rs`: own ZIP reader (central directory + one entry, zip64) for zip / cbz / epub / Office / OpenDocument,
   Affinity preview PNG scan (**not verified on a real Affinity file**), text cards (GDI), fallback cards (badge, name, size). Setting
   `documents` (off) -> `nv_core::folder::set_include_documents`. `decode_for_thumbnail` is new; `decode_file_first_frame` stays the whole
   first frame (the clipboard uses it).
4-7. **CMYK raw out of decoders**, converted in `cmyk.rs` through the embedded profile or the assumed one (FOGRA39 built in from the
   user's PDF / SWOP from Windows / file), notice shown (toast + top bar + info panel row *Processing*); Adobe inversion for JPEG (marker
   presence), YCCK, PSD / PSB (+ ICC resource 1039), TIFF via WIC native CMYK, JP2 (**untested on a real CMYK JP2**). moxcms does
   support CMYK; BPC and absolute are done in `cmyk.rs` in float.
8. PSD / PSB, TIFF, JP2: through profiles. **PDF / AI / EPS: no** (Windows.Data.Pdf converts itself; measured C100 -> 0,158,213).
9. Settings > **CMYK & RAW** tab: CMYK profile, rendering intent, BPC, RAW mode, RAW thumbnails. Viewer tab: List documents.
10. Validation `crates/nv-codecs/tests/cmyk_fogra39.rs`: the user's ColorSync values reproduce exactly with *relative colorimetric, no BPC*
    (default), LittleCMS agreement within 1 level (4 with BPC, 12 absolute).
11. Optimisation + statistics: `stats.rs` (table per format and stage, `Image::stages`, info panel *Decoded in*, saved to
    `%LOCALAPPDATA%\NuViewer\decode-stats.txt` on exit). PNG: checksums off + one-pass RGBA (~7% warm). JPEG thumbnails: DCT-scaled
    decode (15-25%). Measured and **not** worth it: zune-jpeg 0.5 (0.92-1.05x), colour conversion is already fast (12 MP: P3 ~9 ms,
    CMYK ~33 ms, BPC ~105 ms).

Tests: nv-codecs 160 unit + 8 (cmyk_fogra39) + 7, nv-core 58, nv-editor 16, nv-win 14, nuviewer 3; e2e: `e2e-features` 60, `e2e-session7` 64,
`e2e-session8` 55, new `tools/test/e2e-session9.ps1` 16 (CMYK pixels on screen, RAW sizes, documents in the gallery). Dev tools in
`crates/nv-codecs/examples`: `bench_decode`, `bench_colour`, `rawinfo`, `rawcheck`, `doccards`; `crates/nv-core/examples/scan_time`;
`tools/extract-pdf-icc.py`, `tools/lcms-reference.py`.
**Gotchas:** the Bash tool turns `\\n` inside heredoc python into a real newline (again): patch files containing backslashes with
the Write / Edit tools. The repo mixes CRLF and LF files: edit with line-ending-preserving tools only.
**Not done / ideas:** an exposure setting for RAW (the developed picture is darker than the camera JPEG by design); PDF through our
profiles needs our own renderer; CBZ page navigation (`file.cbz?page=N`); the actual gallery split.

### Session 10 (2026-11-15): NuGallery, the gallery as its own program

The user asked for: the gallery split off but seamless and as fast; a right-click menu (open, slideshow, quick edit, edit with,
show location, refresh, shuffle, select item/all/none, copy image/file/path, cut, paste, copy to, move to, print, new folder,
rename, delete, compress (several files), quick convert); mouse edges (left tree + folder data + folder size, bottom taskbar,
right preview / name / size / histogram / editable metadata / EXIF, top toolbar); wheel = previous / next file, left click
selects, middle click views, Ctrl+wheel = columns; Space = view, Enter = next file, 1-5 zoom, ~ reset; batch rename / convert /
time stamp / remove metadata; `nuviewer.exe` alone opens the gallery; time the thumbnails.

- **New crate `nugallery`** (`nugallery.exe`, same folder as `nuviewer.exe`): `app.rs` (state, events, async scan / thumbnails /
  jobs, edge panels), `draw.rs` (grid / details / list, toolbar, overlays), `panels.rs` (left and right panels), `menus.rs`,
  `ops.rs` (file operations, jobs), `batch_ui.rs` (the four batch dialogs, `nv_win::ui::Form`), `model.rs` (entries, show / sort /
  search, selection), `grid.rs`, `chrome.rs` (layout of panels and toolbar buttons, shared by drawing and hit-testing), `tree.rs`,
  `info.rs` (preview, histogram, properties), `input.rs` (text box), `state.rs` (`%APPDATA%\NuViewer\gallery.ini`, env
  `NUGALLERY_STATE`), `gfx.rs` (text cache), `cpu.rs` (first frames, below). `colour.rs` is shared with the viewer by `#[path]`.
  54 unit tests.
- **New library code (all unit-tested):** `nv_core::batch` (rename patterns `{name} {n:3} {date} {time} {taken} {folder}`, find /
  replace, case, two-phase renames for swaps, date parsing, file times), `nv_core::archive` (zip writer, stored for already
  compressed types), `nv_codecs::metastrip` (lossless removal of EXIF / XMP / IPTC / comments from JPEG, PNG, WebP; keeps the
  profile and, optionally, the orientation), `nv_editor::convert` (JPEG / PNG / lossless WebP / BMP encoders),
  `nv_win::files` (shell copy / move / recycle with Explorer's dialogs, file clipboard with cut / copy, print, drives, known
  folders), `nv_win::props` (title, author, tags, comment, copyright, rating through the shell property system; **COM must be
  initialised on the calling thread, `ensure_com` does it**: the first version failed on worker threads), renderer `set_clip`.
- **Hand-over with the viewer:** see `docs/gallery-split.md` "Session 10". `nuviewer.exe` with no picture or with `--gallery` starts
  `nugallery.exe`; viewer flags `--from-gallery`, `--slideshow`, `--shuffle <seed>`; the viewer's `G` / Tab starts the gallery and
  closes. `nuviewer/src/gallery.rs` (the old grid) is unreachable and can be deleted (`enter_builtin_gallery` and friends).
- **Installer / package:** the setup carries both exes (`setup/build.rs` packs `$NUVIEWER_EXE` and `$NUGALLERY_EXE`; blob table in
  `install.rs`, tested), a Start menu shortcut "NuGallery", uninstall removes both, `tools/package.ps1` builds and zips both.
  NOT run for real (it would register things on this PC); the unit test installs and uninstalls in a scratch registry key.
- **Interpretation choices to confirm with the user:** (1) "wheel = next / prev file" in the gallery moves the selection by one
  file per notch (Shift+wheel scrolls); (2) "1 2 3 4 5 = zoom preview, ~ = reset zoom thumbnail" became five thumbnail sizes and a
  reset (the preview panel itself has no zoom); (3) the toolbar / panels slide out when the pointer rests on an edge for ~140 ms
  and leave ~450 ms after it is gone, each with a Pin button; (4) the toolbar's "Show only..." is a list of kinds (pictures, RAW,
  JPEG, PNG, GIF, WebP, TIFF, documents, 3D, all files); (5) folders are shown as tiles (folders first) so Up / Back / Forward
  and the address bar mean something; (6) deleting / copying / moving uses Explorer's own dialogs (so folders work and
  conflicts are asked), which also means there is no Ctrl+Z in the gallery.
- **Thumbnail timing** (`tools/bench/gallery-thumbs.ps1`, release build, windowed 1280x800 = 24 tiles, median of 5, ms since the
  process started; this PC, GTX 1050 Ti). Before the CPU first frame the gallery waited for Direct3D (gpu ~220-330 ms):
  READY 285 / 237 (cold / warm, 200 PNG) and 352 / 274 (40 x 12 MP JPEG). With the first frames painted on the CPU while the GPU
  starts (`cpu.rs`): **READY 92 cold / 52 warm (200 PNG, 1000x700); 278 cold / 54 warm (12 MP JPEG)**, process spawn to the
  title showing it ("wall") 125 / 76 and 304 / 82. The first thumbnail is decoded at 38-74 ms; the cold 12 MP number is decode
  work (24 pictures, 5 workers), not start-up. The old in-viewer gallery measured ~30-40 ms warm / 150-170 ms cold *inside* the
  running viewer (no process start, no GPU start), so these are not the same quantity; opening the viewer from the gallery gives
  the picture in ~26 ms (title "first image 26 ms" for a 500x400 PNG). Fullscreen shows ~60 tiles instead of 24: cold times grow
  with the tile count, warm ones barely.
- **Tests:** `tools/test/e2e-gallery.ps1` (23 checks: rename, new folder, search, copy / paste, delete, compress + zip contents,
  batch convert / rename / time stamp dialogs driven through their controls, metadata dialog, `nuviewer.exe` alone opens the
  gallery) and `tools/test/e2e-gallery-link.ps1` (8: Space opens the viewer, Esc returns on the last picture, G from a standalone
  viewer); both use messages only (WM_NV_INJECT_KEY for Ctrl combinations, WM_CHAR for typing) and scratch settings / state /
  cache. They need no other NuViewer running (single instance). The sections of `e2e.ps1`, `e2e-mouse.ps1`,
  `e2e-features.ps1`, `e2e-session9.ps1` that tested the in-viewer gallery now `Skip`. Test hooks: injected keys 0xFFF1-0xFFF7
  start the batch dialogs / compress / quick convert / quick edit.
- **Not verified:** the right-click menu itself (a native popup; only the command handlers were driven), Print, "Edit with" and
  Copy image on real data, the taskbar reveal at the bottom edge in the gallery (same code as the viewer's, game holds focus), real
  mouse input, DPI > 100%, the details edits on a real camera JPEG (round trip tested on a synthetic JPEG), rubber-band selection
  with real mouse drags, drag and drop of files (not implemented), a thumbnail cache shared with the viewer's film strip (same
  keys, so it should).
- **Ideas left:** delete the old grid from the viewer; Ctrl+Z for gallery renames; drag and drop out of / into the gallery;
  recursive search; folder thumbnails; DirectWrite instead of GDI for names; EXIF thumbnail for faster cold JPEG tiles; tooltips
  on the toolbar glyphs.

### Session 11 (2026-11-15): NuEditor, the third program

Full design, speed rules, status table and open questions: **`docs/nueditor.md`**. Summary of what exists:

- **`nv_editor::recipe`** (one `PARAMS` table -> slider enum `P`; curves, crop, retouch strokes, LUT, hue wheel, flags, `auto`),
  **`nv_editor::sidecar`** (`IMG.jpg.nuedit`, plain text lines, tolerant reader, identity recipe deletes the file),
  **`nv_editor::engine`** (geometry + per-pixel stage: 3 byte tables when every step is per channel, general path otherwise;
  row-banded threads; `Frame` for tiles / vignette; `is_implemented` tells the UI which sliders are live). 39 tests in nv-editor.
- **`nueditor/`** (`nueditor.exe`, `Program::Editor` in `nv_win::instance`): `main.rs` (args `--from viewer|gallery`,
  `--via-gallery`, `--angle`, GPU + decode threads), `app.rs` (proxy preview, accordion of sliders from the table, drag / wheel /
  right-click reset, undo, keys: Tab panel, C original, `,` `.` turn 45, H V flip, I invert, 0 reset, Ctrl+Z/Y/S, Esc),
  `flow.rs` (leave question with pair file / new file / burn in / discard, the three savers, hand-back). 5 tests.
- **Hand-over:** viewer `E` (when `nueditor.exe` is next to it) starts the editor and closes (pending turn -> `--angle`); gallery
  `F4` / right-click "Edit in NuEditor"; Esc in the editor sends the program it came from back to the same picture (gallery
  re-reads its folder on that `Open`).
- **Tests:** `tools/test/e2e-editor.ps1` (26) and `e2e-editor-link.ps1` (8), messages only. Unit: nv-win 18, nugallery 55.
- **Numbers:** 1080p proxy 3.3 / 6.6 / 12.3 ms (tables / general / everything); 12 MP JPEG first image ~115 ms; viewer E -> editor
  window 159 ms; editor Esc -> viewer 52 ms.
- **Not done:** installer packing, zoom / pan / 100% crop render, crop frame, curve editor, colour picker, filters (sharpen,
  clarity, bloom, NR, grain, blur, LUT), heal tools, scopes, resize / convert tools, XML shown in viewer / gallery, shared undo
  journal. Questions for the user are in `docs/nueditor.md` section 6 (shift meaning, pair file name, Enter default).
- **Follow-up, same day (answers to the three questions):** WB "shift" is four axes (shadows / highlights x green-magenta /
  blue-yellow); pair file is plain text `IMG.jpg.nuedit` (not XML, not `.xmp`, which darktable owns); Settings > NuEditor tab (new,
  window widened to 760) holds the default save way (pair file / new file / burn in, factory pair) and "ask when leaving"
  (`editor_save`, `editor_ask`). **E1 done:** `nueditor/src/app/view.rs` (fit / zoom to 16x / pan / double click, detail render
  cut from a pyramid, A/B split `S`, scopes `Y`, false colour `X`) + `nv_editor::scopes` (histogram with clipped %, parade,
  vector scope with skin line, false colour). Zoom needs an uncut picture. Installer + `tools\package.ps1` carry `nueditor.exe`.
  e2e-editor.ps1 is 35 checks now. Settings tab not driven by a test (only compiled).
- **E2 done (same day):** crop frame with ratio lock + straighten line (`R`, `A`, Ctrl+drag), WB picker (`W`), curve editor and hue
  wheel (two extra panel sections after the seven groups; hue wheel engine in `engine.rs`: `band_weights` shares a hue linearly between
  two bands), `nv_editor::cropmath`, `nueditor/src/app/tools.rs`. A cropped picture uses a re-made proxy (`ensure_geo`). Tests: nv-editor
  54, `tools	est\e2e-editor-tools.ps1` (14), e2e-editor.ps1 (42), link (8), settings (9). Not verified: real mouse, DPI > 100%.
- **E3-E7 done (2026-11-15, one run; the user said "keep going until E7"):**
  - E3: render on a **worker thread** with stale-drop (`app.rs` `spawn_renderer`, `Msg::Rendered`), `Arc` proxies; save options
    (Settings > NuEditor: format of a new file, JPEG quality, name suffix, EXIF, watermark text, "show pair files" - fields
    `editor_format/quality/suffix/exif/watermark/show_pair` in `nv_core::settings`); presets (`nv_editor::preset`, `.nupreset`
    in `%APPDATA%\NuViewer\presets`, panel section).
  - E4: `nv_editor::{blur, spatial, lut}`: sharpen, clarity, noise reduction (luma / colour), skin smooth, bloom, film blur, grain,
    LUT (.cube 3D + 1D). Radii are fractions of the picture's long side. `engine::apply_pixels` now includes them.
  - E5: `nv_editor::retouch` (clone, heal, remove (push-pull), patch, red-eye), `nueditor/src/app/heal.rs` (mouse, overlay),
    `engine::{display_to_source, source_to_display}` (mapping through the geometry, tested), `engine::RetouchCache`.
  - E6: `nv_editor::{resample (7 algorithms), quantize, watermark, bgremove, export}`; panel sections "Presets" and "Export copy"
    (`app/panel.rs`: `Item` rows = `Num` sliders / `Pick` cycles / `Btn` / presets / tool strip, panel scrolls); Ctrl+Shift+S;
    NuGallery's Convert dialog got the same tools (`ConvertSpec`, `ops::convert_one` goes through `export::run`).
  - E7: `nv_core::pair` (viewer decodes with the pair file applied; thumbnails too, pair-file time in the cache key; gallery drops
    the edited picture's thumbnail on hand-back), `nv_editor::journal` (burn-ins noted in `burn-in-journal.tsv`; viewer Ctrl+Z
    and editor Ctrl+Alt+Z restore), zoom / picker / heal on turned, flipped, cropped pictures (`view.rs` `sync_base`),
    Pitch / Yaw / Zoom (`raster::perspective`).
  - Tests: nv-editor 101, nv-core 71, nueditor 6, nugallery 55, nv-win 18; e2e: `e2e-editor.ps1` 42, `-tools` 14, `-link` 8,
    `-settings` 9, **`-more` 20** (presets, clone + undo, remove, zoom on turned, Pitch, Export), **`-pair` 10** (viewer shows the pair
    file, setting off, other size, journal Ctrl+Z). `NUEDITOR_TEST=1` puts a state line in the editor's title for the scripts.
  - Not verified: the modal system dialogs (Save copy / preset / LUT / delete preset), real mouse, DPI > 100%, RAW / CMYK in the new
    tools, heal quality on real portraits. See `docs/nueditor.md` sections 7 and 10.
- Nothing committed (the user did not ask).

### Session 12 (2026-11-15): the user's 17-point list (menu bars, editor discoverability, gallery fixes, importer mock-up)

The user's list, and what was done (details of every choice are in the code comments):

1. **NuEditor right-click + window mode:** right-click on the picture = context menu (`chrome.rs`, same commands as the menus);
   F11 / Alt+Enter / View > Fullscreen switch window <-> fullscreen (`toggle_window`, resizes the swapchain by hand like the gallery).
2. **NuEditor compose / pan / crop / pitch-yaw:** the features existed but only behind keys, so nothing showed them. Now: a tool strip
   on the picture (Crop, White picker, Retouch, Fit, 100%, - / +, Split, Graph, Panel), a **navigator** (drag it to pan) when zoomed,
   arrow keys pan, Composition section has "Crop..." and "Crop to the picture (no empty corners)" buttons, and while Composition is open
   (or the crop tool is on) the **whole picture is shown with its outline (yellow) and the crop frame (white)** on a neutral sheet
   (`geo_overlay`, `engine::extent_quad`, `raster::perspective_forward`, `cropmath::inscribed`; all unit-tested).
3. **Viewer:** right-click now has "Edit in NuEditor" and "Edit with..." (programs from Settings + "Choose a program..." + "Set up the list...").
4. **Graphs in NuEditor:** histogram / RGB parade / vector scope (skin line) sit in a dock at the top of the panel with four tabs and are
   **on by default**; Y still cycles; with the panel hidden the graph floats bottom-left.
5. **Gallery edge panels leave at once:** `EDGE_LEAVE` 350 -> 60 ms, timer 40 -> 16 ms, slack 12 -> 3 px.
6. **Gallery folder tree scroll bar** (`Tree::scrollbar`, drag + click on the track).
7. **Gallery wheel scrolls** (3 rows a notch); Ctrl+wheel still sets the columns. (Reverses session 10's choice 1.)
8. **Gallery preview controls** (`preview.rs`): wheel zooms about the pointer, drag pans, double click fit <-> 3x, a sharper 3072 px decode
   is fetched the first time you zoom; animated GIF / WebP / APNG and folded image sequences play with a bar (prev, play / pause, next,
   scrub, n/N); 3D models turn when dragged (software `render_preview`, no GPU mesh pipeline in the gallery) and zoom with the wheel.
   **NOT done: video and audio.** No program of the family decodes or plays them yet (the viewer's playback bar has reserved slots only);
   it needs Media Foundation (`IMFMediaEngine`) and is a job of its own. The control bar is generic enough to take them.
9. **Gallery image-sequence toggle:** Ctrl+G / View > "Group image sequences" / Tools; runs fold into one tile with a "play N" badge
   (`model::fold_sequences`, only while sorted by name ascending; RAW + JPG never fold); delete / copy / move of a tile applies to all frames.
10. **Delete confirmation + undo:** new setting `confirm_delete` (default on; Settings > Viewer) asks before Recycle Bin deletes in the
    viewer and the gallery. The viewer already had Ctrl+Z for delete / move / copy; the gallery now has it too (`undo.rs`: delete, move,
    rename, copy; the shell does the work, so afterwards we look at what is on disk). Edit menu names the step ("Undo delete (3 items)").
11. "Copy image" vs "Copy picture": both programs say **Copy picture** now (gallery also has Copy file, Copy path).
12. **Drive names in the tree:** "Photos (D:)" from the volume label, read on one thread per drive (`files::volume_label`), so a sleeping
    disk can't delay the window.
13. **Cold start** (measured with `tools/bench/coldstart.ps1`, -FreshCopy = a copy nobody has seen; spawn -> first visible window of the process, `--windowed --new-window`):
    a program that has been started before opens its window in **~25 ms**; the **first start of an executable Windows has not seen**
    (a fresh build, a fresh copy, an update) takes **0.4-0.75 s**, for all three 15 MB programs alike and *also in `--associate` mode,
    which does no GPU and no decoding*, so it is not our start-up code: it is Windows Defender (real-time protection is on here)
    scanning the file. A 300 KB exe costs ~+35 ms the same way. Reading a fresh exe once takes ~330 ms by itself (the on-access
    scan), and a start after that read takes ~445 ms instead of ~790 ms. Things tried / done:
    - size-optimised dependencies (`opt-level="s"` for everything but the hot decoders): 15.0 -> 13.0 MB, **no measurable gain** -> not adopted;
    - `nv_win::prewarm_siblings()` (all three programs): 2.5 s after start a low-priority thread reads the other programs' exes once, so
      the scan of NuGallery / NuEditor is done before the user presses G / E / F4 (first start after an update ~45% sooner);
    - the folder tree (`Tree::new`: known folders + `is_dir` checks that can stall on OneDrive / network folders) is built on a worker
      thread (`Msg::TreeReady`), drive labels on one thread per drive: the gallery window no longer waits for either.
    What cannot be fixed from inside the program: the scan itself (a Defender exclusion for the install folder would remove it; that
    is the user's decision, nothing was changed). Real cold-boot disk reads were not measured (the file cache can't be dropped without
    admin rights).
14. **Credits** (`nv_core::about`, author line `AUTHOR`, libraries) at the bottom of the help of the viewer, the gallery and the editor
    (the editor gained an F1 help page).
15. **Menu bars** in all three programs (`nv_win::menubar`): top edge or Alt / F10; Alt+F / E / V / G / T / H open a menu; Left / Right /
    Enter / Esc inside. New `Event::AltTap`; Alt+letter reaches the programs as keys with Alt held (`WM_SYSKEYDOWN`).
16. **Ctrl + / Ctrl - / Ctrl+0:** viewer zoom (`Action::ZoomIn/ZoomOut`, `ZoomReset` also on Ctrl+0), gallery columns, editor zoom.
17. **NuImporter mock-up** (`nuimporter/`, `nuimporter.exe`; started from the gallery's Tools menu): source choice (removable drives, a
    folder, demo data), scan, sub-folder layout + naming + event name, options, "Import..." shows a dry run and copies nothing.
    RAW and JPG stay separate files and share one shot number when numbering (`plan.rs`, tested). Packed into the installer.

## 2. Build, test, run

```powershell
$env:CARGO_TARGET_DIR = 'T:\cargo-target\NuViewer'   # P: is nearly full; never create target/ under P:
$env:CARGO_BUILD_JOBS = '1'                          # the machine is often RAM-starved (a game runs on it)
cd P:\Project_Code\NuViewer
Get-Process nuviewer -ErrorAction SilentlyContinue | Stop-Process -Force   # a running exe blocks the link step
cargo build --release                                # -> T:\cargo-target\NuViewer\release\nuviewer.exe
cargo test -p nv-codecs --release -j 1               # 41 tests, all passing (session 2)
cargo build --release -p nv-codecs --example probe   # dev tool: prints size, average colour, read/decode ms per file
```

Only `nv-codecs` tests were run this session; the other crates were only built. Run `cargo test --release -j 1`
per crate before relying on them.

New command-line flags (not yet in `README.md`):

```
nuviewer.exe <file> --fullscreen      # borderless fullscreen from the first frame, no windowed frame
nuviewer.exe <file> --actual-size     # first image at 100% (skips the fitted CPU preview)
```

## 3. What changed this session

| Area | Change | Where |
|---|---|---|
| First-frame jump | The start-up CPU preview used `floor((cw-w)/2)`, the GPU path `round(cx-w/2)`, so images with an odd margin (default client 1264x761) jumped 1 px when the GPU took over (~245 ms). `Preview` now carries `x,y` computed with the GPU formula; `paint_bgra` takes them. | `nuviewer/src/main.rs` (`make_preview`, `Preview`), `crates/nv-win/src/lib.rs` (`paint_bgra`) |
| Start fullscreen / 100% | `nv_win::run` has a new `fullscreen` argument (calls `toggle_fullscreen` before `ShowWindow`); `Boot.actual_size`, `App::new(.., actual_size)`. | `crates/nv-win/src/lib.rs`, `nuviewer/src/main.rs` |
| RAF partial read | Fujifilm RAF header (160 bytes) holds the embedded JPEG offset (byte 84) and length (byte 88) as big-endian u32. Only that range is read (~6 MB of 56 MB). Any oddity falls back to the whole-file path. | `raw.rs`: `raf_preview_range`, `read_raf_preview` |
| TIFF RAW stage 1 | Walk IFD0, SubIFDs and the IFD chain with tiny reads; candidates are JPEGInterchangeFormat/Length (513/514) or a single compression 6/7 strip. Accepted only if the best JPEG is >= 1 MP (`MIN_PREVIEW_PX`) **and** >= 25% of the largest IFD image. | `raw.rs`: `walk`, `parse_ifd`, `tiff_preview`, `read_tiff_preview` |
| TIFF RAW stage 2 | If stage 1 refuses (Sony ARW keeps its preview in the MakerNote; DNGs with a small preview): find the sensor block (IFD with the most pixels pointing at strips/tiles) and read only the bytes before and after it, then run the normal "largest embedded JPEG" scan on those. Same answer as a whole-file scan without touching the sensor data. Guards: block >= 4 MB and >= 40% of the file, head <= 24 MB, tail <= 4 MB, block must not itself open with a SOF0-2 JPEG. | `raw.rs`: `sensor_skip_scan`, `decode_tiff_skipping_sensor` |
| Dispatch | `decode_file_opts`: ext `raf` -> RAF read; ext in `TIFF_RAW_EXTS` -> stage 1 -> stage 2; then the old whole-file read -> `raw::decode_preview` -> WIC fallback. CR3, CRW, MRW still read whole files. | `crates/nv-codecs/src/lib.rs` |

`decode_preview` now delegates to `decode_preview_with(bytes, container_orientation)` so partial buffers can be given the
container's (IFD0) orientation explicitly.

## 4. Rules and context from the user

- **RAW and JPG of the same shot must stay two separate entries** (`DSCF8252.RAF` and `DSCF8252.JPG`). Never hide, merge
  or filter pairs: the user edits photos and skipping one causes errors in their workflow. At most an optional indicator.
- Sidecars: darktable `.xmp` files (`DSCF8252.RAF.xmp`, ~1.5 KB) hold `xmp:Rating` and an (empty in the samples) edit
  history. They are not listed as images. Nothing reads them yet.
- Don't screenshot or print contents of the user's photo folders; numeric pixel comparison is fine.
- Do not edit FastStone's settings files (`%LOCALAPPDATA%\FastStone\FSIV\*.db`); the user changes its settings in the app.
- Sample data (all the user's own, on this PC):
  - `F:\Photo\Kyoto012025\C1\DCIM\118FUJIF` — Fujifilm X-T4 RAF (56–59 MB, embedded JPEG 4416x2944) + camera JPG
    (6240x4160) + `.xmp`, 498 shots + 9 MOV.
  - `F:\Photo\DCIM\100CANON` — Canon EOS 60D CR2 (164 files, 21–28 MB; IFD0 lists the full 5184x3456 JPEG) + JPG.
  - `P:\Downloads\DSC02765.dng` — Sony ILCE-7M3 DNG (Adobe converter): 6048x4024 tiled lossless-JPEG sensor data,
    previews 1024x683 JPEG plus JPEG-XL ones (JPEG-XL is not supported).
  - `F:\PIC\2025-06` — 161 PNG (0.1–1.5 MP).
  - No Sony ARW, Nikon NEF, Olympus ORF, Pentax PEF or Panasonic RW2 files exist on the machine.
- Drive **F: reads only ~37 MB/s** and had ~3.7 GB free; cold-cache timings there are dominated by I/O.

## 5. Measurements (this PC, GTX 1050 Ti, games often running, so use ranges)

Method: spawn or key press -> the picture area on screen stops changing (BitBlt of a 640x360 region, 24x14 sample grid,
~10 ms resolution). The scripts were in per-session scratch folders and are **not in the repo** (see §7).

| Test | NuViewer | FastStone 8.3 |
|---|---|---|
| PNG folder, open random image | ~55 ms | ~685 ms |
| PNG, next/back paced / rapid | 11–16 ms / ~10 ms | 26–33 ms / ~60 ms |
| PNG, close app | ~39 ms | ~305 ms |
| Synthetic 12 MP JPEG, first image | ~91 ms | ~305 ms |
| Synthetic 12 MP JPEG, next, paced | ~41 ms (first 3–4 steps ~45, then 6–28) | ~31 ms |
| Fuji RAF fullscreen 100%, cold, before the fix | 1.73 s | 3.9–4.0 s (user settings) |
| Fuji RAF fullscreen 100%, cold, after the fix | 0.42–0.55 s | — |
| Fuji RAF, warm | 0.27–0.29 s | 2.3–2.4 s (earlier run: 4.9–5.9 s) |
| Canon CR2, whole-file read before / new decode, cold | 0.59–1.29 s / 75–210 ms (median ~177) | — |
| Sony DNG decode before / after | 19 ms / 3 ms; process read 0.86 MB instead of 20.7 MB | — |

Caveats: NuViewer shows the camera's embedded preview (13 MP for the X-T4); FastStone's RAW mode under the user's
setting is unknown (it prints `par.111 is a Fujifilm X-T4 image.` many times and takes seconds, which looks like a full
develop), so RAW comparisons are not like for like. FastStone's own numbers drifted between runs. Each cell is 3 files.

## 6. Open items (suggested order)

1. ~~**Paced "next" on 12 MP JPEGs is slower than FastStone**~~ **FIXED 2026-11-11 (session 2).** Cause: the driver
   held back the texture copy + `GenerateMips` until the texture was first drawn, so a prefetched 12 MP image still cost
   tens of ms on the key press. Fix: `self.ctx.Flush()` at the end of `Renderer::upload` (`crates/nv-render/src/lib.rs`).
   Measured with `tools/bench/race2.ps1 -Kind nv -Filter '*.jpg' -PaceMs 600` on 40 synthetic 12 MP JPEGs (4 runs each):
   paced next median 36–40 -> 11 ms, paced back 51 -> 11 ms (FastStone ~31 ms). Rapid presses unchanged (the first 3–4
   steps after a direction change still wait for decode, 50–100 ms). The harness itself sometimes times out whole
   runs (focus theft, seen with old and new builds): rerun before drawing conclusions.
2. **Slow first open after the drive has been idle (~1 s)** on F:, seen several times (also for CR2 that otherwise
   open in ~170 ms). Cause unknown (drive wake-up or antivirus suspected). The loader also prefetches neighbouring
   camera JPGs in full (6–16 MB each) on the same slow disk; that may compete with the file being opened.
3. ~~**RAW with no usable preview**~~ **DONE 2026-11-11 (session 2), first version.** New `crates/nv-codecs/src/develop.rs`
   (`develop_half`): `rawler` 0.8 decodes the sensor data (new dependency, ~6 min cold build with `-j 1`, exe 7.5 MB),
   every 2x2 Bayer block becomes one pixel (no demosaic), black/white level per CFA position, camera WB, camera->sRGB
   matrix (dcraw recipe from rawler's `develop_params()` matrix), auto exposure (log-average luminance -> 0.22, gain
   clamped 1..6), extended-Reinhard highlight roll-off, sRGB gamma LUT; threaded over rows; EXIF orientation from
   `raw_metadata`. Used in `decode_file_opts` (a) when a TIFF RAW's stage-2 preview is < `DEVELOP_BELOW_PX` (2 MP) and
   the developed picture is >= 1.5x larger, (b) when there is no usable preview at all (before the WIC fallback). Never
   for thumbnails (`first_only`). Env `NUVIEWER_NO_DEVELOP=1` turns it off. Panics/errors fall back to the preview.
   Sony DNG: 1024x683 preview -> 3000x2000 developed (shown 2000x3000, orientation 6), decode ~175 ms warm
   (642 ms first, cold), first image in the app 183 ms warm. Colour check is numeric only: channel ratios match the
   camera preview (130:135:129 vs 111:113:110), brightness is darker (mean 111 vs 133) because there is no camera tone
   curve. Limits / next steps: Bayer 2x2 only (X-Trans, 4-colour and float DNGs return an error -> preview/WIC);
   half-size only (no full-res demosaic / GPU demosaic yet); the first colour matrix of the file is used (no
   illuminant interpolation); no Sony ARW/Nikon/etc. files on the machine, so only the DNG path is verified; Canon
   CR2 and Fuji RAF still take the fast full-size preview path (probe re-run: unchanged, 60–77 ms). 3 unit tests added
   (41 in nv-codecs). User decision (speed first): `DEVELOP_BELOW_PX` = 1 MP, so Sony ARW (1.7 MP preview, ~20 ms)
   is NOT developed; only previews under 1 MP (the Adobe DNG's 0.7 MP) and files with no preview are. Not verified on
   a real ARW (none on the machine).
4. **Read `xmp:Rating` (and maybe labels) from the `.xmp` sidecars** and show it in the info panel / gallery. Keep the
   RAW/JPG pair rule (§4).
5. CR3 (ISO-BMFF), CRW, MRW still read whole files.
6. Verify the TIFF RAW paths on real ARW/NEF/ORF/PEF/RW2 files if any appear. The design falls back safely, so the
   worst case is "not faster", never a wrong picture; the synthetic tests cover both byte orders and the refusal cases.
7. ~~Housekeeping~~ done 2026-11-11: `--fullscreen` / `--actual-size` are in `README.md`; tests of nv-core (25), nv-win (8)
   and nv-codecs (38) pass (nv-render and nuviewer have none); benchmark scripts are in `tools/bench/` (`race2.ps1` now
   takes `-Filter`; `img12/` = 8 synthetic 12 MP JPEGs — copy them to ~40 files for the next/back test).
8. Older backlog (from the plan): two-stage preview for huge JPEGs (scaled decode first), thumbnails that still decode
   fully, filenames in the gallery (needs DirectWrite), exe icon/version resource (installer itself is done, session 3), true tiling for images
   over 16k px, pure-Rust AVIF/HEIC, platform abstraction (M6).
9. Untested: DPI changes between monitors, the native folder picker dialog itself, Recycle Bin restore outside `C:`,
   ICC cost on 12 MP Display-P3 photos.

## 7. Gotchas

- `Get-Process nuviewer | Stop-Process` before every build or the link fails with "Access is denied".
- Never re-save source files with PowerShell `Get-Content | Set-Content` (PS 5.1 mangles UTF-8); use an editor tool.
- When writing PowerShell helpers, don't name a function `Diff` (it is an alias for `Compare-Object`).
- The viewer window cascades to a new position on every launch; screen-based timing must probe relative to the
  window's current centre. Fullscreen and `--actual-size` make the probe position fixed.
- FastStone (`C:\Program Files (x86)\FastStone Image Viewer\FSViewer.exe`) writes `par.111 ...` lines to the console.
- The benchmark helpers used this session: `race2.ps1` (key/zoom/close scenarios on a folder), `race3.ps1`
  (record-then-analyse open timing, cold and warm, `-ExtraArgs '--fullscreen --actual-size'`), `race4.ps1` (unused
  windowed-then-fullscreen variant), `tiffdump.py` (TIFF/DNG/CR2 structure dump) and `rafhdr.py` (RAF header). They
  lived under `%LOCALAPPDATA%\Temp\claude\<session>\...\scratchpad` and may be cleaned up.
