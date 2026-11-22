# Handoff for session 25

Written at the end of session 24 (2026-11-23). Read `docs/session24.md` (what was done, facts that are easy to lose), `docs/spine.md`, `docs/live2d.md`, then this. Sessions 23 and 24 are committed on `release/0.6.0`; the version number is not bumped (CHANGELOG "Unreleased": decide with the user, 0.7.0?) and nothing of them is packaged.

## 1. State

| Area | Where | Status |
|---|---|---|
| Spine 3.8 - 4.3 player | `crates/nv-anim` (`json`, `binary`, `binary43`, `constraints`, `pose`, `raster`, `atlas`), `nv-codecs/spinefmt.rs`, `nuviewer/spine.rs`, `anim.rs`, `layers_ui.rs` | JSON of every version and binary 4.1 / 4.3 checked on real files; 4.2 binary on one tiny real sample; binary 3.8 / 4.0 **no real file**. Constraints: IK, transform (old + 4.3), path, physics, slider. Two-colour tint, image sequences, crossfade (0.25 s), atlas rotation 0 / 90 / 180 / 270. Circus, tank, snowglobe and diamond match the reference page |
| Live2D player | `nv-anim/cubism.rs`, `live2d.rs`, `nuviewer/live2d.rs`, `tools/mock-cubism` | works with the **mock** Core only (the real `Live2DCubismCore.dll` was never run) |
| Layer panel (PSD / XCF / ORA) | `nv-codecs/layerdoc.rs`, `nv-core/layers.rs`, `nuviewer/layers_ui.rs` | done; flattened PSD ~14 levels darker than the file's merged picture (masks / effects not applied) |
| Settings | `settings_more.rs` | Archives & tools page looked at in a screenshot (long paths are shortened with `elide`) |

Tests (target dir `R:\cargo-target\NuViewer`, set `CARGO_TARGET_DIR`):
- `cargo test -p nv-anim` (39 unit + `tests/real_samples.rs` 7 + `real_41` + `real_42` + `cubism_mock` 2), `cargo test -p nv-codecs --lib` (255), `-p nv-core --lib` (144).
- e2e (debug build `cargo build -p nuviewer -p mock-cubism`, no other NuViewer open): `tools/test/e2e-spine.ps1` (7), `e2e-spine43.ps1` (8), `e2e-live2d.ps1` (8), `e2e-layers.ps1` (21). `e2e-layers` failed once when run straight after another script and passed alone: run them one at a time with `Get-Process nuviewer | Stop-Process` between.
- Real Spine samples are **not in the repo**: `T:\Benchmark-folder\spine-samples\` (or `NUVIEWER_SPINE_SAMPLES`), taken from `examples/assets/` of `github.com/playcanvas/playcanvas-spine`. If `T:` is gone, re-download (list in `docs/session24.md`); tests skip themselves.
- Tools: `cargo run --release -p nv-codecs --example riginfo -- <skel|json> [--png out] [--anim n] [--t s] [--hide a,b] [--slotcolors] [--all] [--constraints] [--bone i]`; `NV_SKEL_NO_JSON=1` stops a `.skel` falling back to its `.json`. `--settings` opens the Settings window (screenshot with `PrintWindow`).

## 2. Reference material that saves hours

- `https://playcanvas.github.io/playcanvas-spine/examples/spine-4.3.html` runs the official 4.3 runtime in WebGL and works in the in-app browser (resize the viewport to 1800x1100 for a larger picture; compare the **same animation**).
- `build/playcanvas-spine.4.3.js` in that repo is a full spine-core 4.3 (readers, constraints, physics, slider, atlas). It is the spec; SkelToJson's 4.3 part is an older beta.
- `github.com/kiletw/SpineViewerWPF` vendors the official C# runtimes of every version 2.1 - 4.3 under `runtimes/SpineRuntime.V*` (4.2: `V42`, 4.3: `V43`); useful to read how an older version's binary reader works (3.8: `V38_95`, 4.0: `V40` / `V40_31`) when a real 3.8 / 4.0 / 4.2 file turns up. Its renderer's blend maths equals ours.

## 3. Not done (suggested order)

1. **Look at things by eye that are only compiled / unit-tested:** the crossfade between animations, the panel's *Anim / Skin / Speed / Ping-pong* buttons, *PNG frames* and *WebM alpha* export (the frame export opens Explorer on success), a Spine rig in the gallery (thumbnail is the first frame, physics at rest).
2. **Spine gaps:** 4.3 bone `inherit` timelines, physics `reset` timelines, draw order folders (read and skipped), non-uniform two-bone IK, edge anti-aliasing while playing (2x SSAA only in exports), additive slider animations (treated like normal ones), binary 3.8 / 4.0 / 4.2 on real files (path and physics of 4.2 binary were written from the SkelToJson document).
3. **Get real Live2D data:** a model + `Live2DCubismCore.dll` (the user downloads it; do not bundle). Then check `cubism.rs` against the real Core (struct sizes, `csmReadCanvasInfo`, mask arrays), add physics (`physics3.json`), pose (`pose3.json`), lip sync, a drag-a-parameter UI.
4. **Other formats** (from `session23-plan.md`): Krita `.kra` (LZF tiles), layered TIFF (tag 37724), AI layers (Optional Content Groups: test first that Windows.Data.Pdf honours `/OFF`), EPS layers, **UnityFS** bundles (the user's game caches in `AppData\LocalLow\Watcher of Realms\...\cacheimages\*.unity3d`; do not get round encryption).
5. **Panel polish:** PSD layer masks / clipping masks / effects, the `.nulayers` pair file (`LayerSet::to_text / apply_text` exist, nothing writes it), a sidebar button for the panel (the 16-button list is counted by tests).
6. **Housekeeping:** version number / release (0.7.0?) and `tools/package.ps1` (~7.5 min); `tools/test/e2e-features.ps1` is stale (expects 77 actions / 43 menu entries, finds 105 / 48) and hangs in its Settings section: update or retire; `nuviewer/tests/formats.rs` and `nv-editor` `extreme` fail since before session 22; translations; code signing.
7. **Licence:** `nv-anim` follows the Spine runtimes' algorithms closely (IK, constraints, physics, atlas rotation). Esoteric's runtime licence applies to anything distributed beyond personal use: decide before a public release.

## 4. Facts easy to lose (full list in `docs/session24.md`)

- Atlas `rotate: N` = packed N degrees counter-clockwise; a slider's animation must apply **slot** timelines (colour, attachment, sequence) as well as bone timelines (the diamond was white without it); 4.1 / 4.2 JSON files keep deform under the animation's `attachments`; the 4.3 JSON export drops tiny numbers; physics needs a clock (`PhysicsSim`, steps clamped to 0.1 s; exports step 1/fps).
- The Bash tool's `cd` persists between calls (use absolute paths); a Bash heredoc with python eats `\u` / `\n` in strings (use the Edit / Write tools for such text); PowerShell-written source files get mangled, use the editor tools.

## 5. Questions for the user at the start

- Version: is this 0.7.0, and should installer / portable be rebuilt?
- Do they have Live2D models and the Cubism Core, and Spine 3.8 / 4.0 / 4.2 binary files from real games?
