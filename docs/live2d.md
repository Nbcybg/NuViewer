# Live2D (`.moc3`) and the Cubism Core DLL

## Why `Live2DCubismCore.dll` is not in NuViewer

`.moc3` files can only be evaluated by Live2D's **Cubism Core**, which is proprietary software under the *Live2D Proprietary Software License Agreement*. Live2D publishes it only as part of the Cubism SDK (download after accepting the agreement), does not allow modifying it, and a program that is released with Cubism (Core) needs the SDK *Release / Publication License* (free below a revenue threshold, paid above it; read the agreement). Passing the DLL on inside an installer is governed by that agreement, which a bundled copy would have to be accepted under by whoever ships it, not by us on the user's behalf.
So NuViewer **does not ship, download or accept the licence for the DLL**. It loads it by hand if the user has put it in place.

## Where to put it

Download the *Cubism SDK for Native* from live2d.com, accept its licence, and copy `Core\dll\windows\x86_64\Live2DCubismCore.dll` to one of (searched in this order):

1. the file named by the environment variable `NUVIEWER_CUBISM_CORE`
2. next to `nuviewer.exe`
3. `%APPDATA%\NuViewer\live2d\Live2DCubismCore.dll`

NuViewer is 64-bit: use the x86_64 DLL. Without it a `.moc3` model says exactly this (`Core::missing_message`).

## The player (viewer)

Open `*.model3.json` (or a `.moc3` beside one). With the Core in place the model plays: the first "Idle" motion (else the first), `P` / Space plays and pauses, wheel zooms, drag pans, `Ctrl+L` opens the same panel as for Spine: *Anim* = motions, *Skin* = expressions, speed / loop / ping-pong, **the model's parts as rows** (eye, name from `cdi3.json`, opacity 0-1 = the part opacity of the model, filter, hover highlight), *Save* / `Ctrl+Shift+S` (snapshot), *PNG frames* and *WebM alpha* (ffmpeg with libvpx-vp9). Without the Core opening the model shows the message that says where to put the DLL.
Verified with a **mock Core** (`tools/mock-cubism`, builds `Live2DCubismCore.dll` with the same C functions and a made-up model): `crates/nv-anim/tests/cubism_mock.rs` (2) and `tools/test/e2e-live2d.ps1` (8: draws, plays, pauses, parts as layers, snapshot with a transparent corner, no-Core message). The mock follows the documented API; **it proves the plumbing, not Live2D's real Core**. The eyes blink by themselves (the model3 "EyeBlink" group, every 4.2 s, when no motion drives them) and `ParamBreath` breathes. Not applied: physics (`physics3.json`), pose (`pose3.json`), lip sync. Frame / WebM export, motion / expression buttons: not run by a test.

## What exists (`crates/nv-anim`)

| Piece | File | Tested |
|---|---|---|
| Late-bound Core: revive moc (64-byte aligned), model (16-byte aligned), parameters, parts, drawables, update | `cubism.rs` | only the "missing / not a Core / alignment" paths; **the calls into a real Core were never run** (no DLL on the development PC) |
| `model3.json`, `cdi3.json` (readable names), `motion3.json` (linear / bezier / stepped / inverse stepped, loop), `exp3.json` | `live2d.rs` | 8 unit tests |
| Drawables -> draw list (render order, blend flags, clipping masks as silhouettes, flipped v) and the CPU rasteriser | `live2d.rs`, `raster.rs` | masks, inversion, order, opacity |

## Not done

- Physics (`physics3.json`), pose (`pose3.json`), eye blink / breathing / lip sync, `.cmo3 .can3` editor projects.
- Texture loading beside the model (the textures named in `model3.json` must be decoded by the caller and passed to `render_pages`).

## In Settings

**Settings > Archives & tools > "Add-on: Live2D models (.moc3)"** says why the DLL is a download of your own (proprietary, licence accepted by you), shows whether it was found (`Found: <path>` / `Not installed`), and has *Browse...* (pick the DLL anywhere; saved as `cubism_core` in the settings), *Download page...* (live2d.com) and *Open the folder for it* (`%APPDATA%\NuViewer\live2d`). The same page already holds the other add-ons: 7-Zip and ffmpeg (each with its own reason text). The page was compiled and its logic follows the 7-Zip / ffmpeg rows, but it was **not looked at in a running Settings window**.
