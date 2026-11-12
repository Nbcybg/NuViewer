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
