# Handoff for session 24

Written at the end of session 23 (2026-11-22). Read `docs/session23.md` (layer panel), `docs/spine.md`, `docs/live2d.md` first, then this. Session 23 is committed on `release/0.6.0` (version number not bumped; CHANGELOG has an "Unreleased" section: decide the version with the user).

## 1. State

| Area | Where | Status |
|---|---|---|
| Layer panel (PSD / XCF / OpenRaster), `Ctrl+L` | `nv-codecs/layerdoc.rs`, `nv-core/layers.rs`, `nuviewer/layers_ui.rs`, `tools/test/e2e-layers.ps1` (21) | done; one real PSD checked; flattened PSD ~14 levels darker than the file's own merged picture (masks / effects not applied, cause not isolated) |
| Spine player (CPU-drawn) | `crates/nv-anim` (model, json, binary, pose, raster, atlas), `nv-codecs/spinefmt.rs`, `nuviewer/spine.rs`, `anim.rs`, `tools/test/e2e-spine.ps1` (7) | JSON 3.8 - 4.2; binary **4.1 checked on a real 5 MB game file**, 4.2 on a tiny real sample, 4.0 / 3.8 not checked on real files |
| Live2D player | `nv-anim/cubism.rs` (late-bound Core), `live2d.rs`, `nv-codecs/live2dfmt.rs`, `nuviewer/live2d.rs`, `tools/mock-cubism`, `tools/test/e2e-live2d.ps1` (8) | works with the **mock** Core only; the real `Live2DCubismCore.dll` was never run (licence: the user downloads it, Settings > Archives & tools says why) |
| Settings | `settings_more.rs` "Add-on: Live2D models" section, pref `cubism_core` | compiled, **never looked at in a running Settings window** |

Tests: `cargo test -p nv-anim` (33 + `tests/cubism_mock.rs` 2 + `tests/real_42.rs` + `tests/real_41.rs`, which skips when `T:\Benchmark-folder\Bright_Moon\illust_dating20.skel` is absent), `-p nv-codecs --lib` 255, `-p nv-core --lib` 144. e2e: the three scripts above need the debug build (`cargo build -p nuviewer -p mock-cubism`, target `R:\cargo-target\NuViewer`) and no other NuViewer open; `e2e-live2d.ps1` uses `NUVIEWER_CUBISM_CORE` (the mock DLL sits next to the debug exe).
Example tools: `cargo run --release -p nv-codecs --example riginfo -- <skel|json> [--png out] [--anim n] [--t s] [--all] [--constraints] [--bone i]`, `... --example layerinfo -- <psd|xcf|ora>`.

## 2. The real files and the reference

- `T:\Benchmark-folder\Bright_Moon\` : `illust_dating20.skel` (Spine 4.1.11, Brown Dust 2 style: 2218 bones, 1030 slots, 100 animations) + `.atlas` + 8 page PNGs. **Opens, plays, draws right.** It has black letterbox frames and parts parked at 1e8 (the first fit ignores them: `Rig::bounds_of`). A black area still remains at the left of the first fit.
- `C:\Users\noobc\Documents\Electronic Arts\The Sims 4\Mods\Llumisims_SetsukiEyes_PSD.psd` : the one real layered PSD.
- Reference for 4.2 / 4.3 binary layout: `https://github.com/BastienGimbert/SkelToJson` (MIT; clone it into a scratchpad folder, `src/SkelToJson/converter.py`). 4.3 is **not** read by us.
- No real Live2D model, no Cubism Core, no Spine 3.8 / 4.0 / big 4.2 file, no Krita / layered TIFF / AI / EPS file on this PC.

## 3. Facts learned that are easy to lose

- `psd` crate 0.3.5 has the layer-visible flag inverted and takes group settings from the wrong record: `layerdoc.rs::psd_records` rescans (see `session23.md`).
- Spine 4.1 binary: image-sequence blocks (bool + 4 varints) after the path of regions, after the hull of meshes and after `deform` of linked meshes; a timeline-count varint at the start of each animation; a type byte (0 deform, 1 sequence) before each attachment timeline's key count; draw-order offsets are plain varints read as `u32 as i32`; skin names are plain strings in 4.x (`sref` only in 3.x, unverified).
- 4.x curves: control points are absolute (seconds, value units) and are evaluated in that space (`Curve::BezierAbs`); dividing by the value change blows up where two keys are equal.
- Constraints: after each one only its bones' subtree is recomputed (`update_subtrees`), not the whole skeleton (25 ms -> 1.5 ms on the real rig).
- Game rigs hide parts by moving a bone 1e8 away: never treat huge coordinates as an error; transform constraints following such a bone go far too.
- The rasteriser draws in parallel row bands (`raster.rs`); output is byte-identical to a single pass.
- Bash heredoc python eats backslashes (`\N` is a unicode escape): write patch scripts with the Write tool or use the Edit tool for Windows paths; `tools/dev/patch.py` has `<NL> <T> <BS>` tokens.

## 4. Not done (suggested order)

1. **Look at things by eye that only compile:** Settings > Archives & tools > the Live2D add-on group; the panel's *Anim / Skin* buttons, *Speed*, *Ping-pong*; *PNG frames* and *WebM alpha* export (never run: the frame export also opens Explorer on success); a Spine rig in the gallery (thumbnail is the first frame).
2. **Get real Live2D data:** ask for a model + `Live2DCubismCore.dll` (user downloads it from live2d.com: do not bundle, see `docs/live2d.md`). Then check `cubism.rs` against the real Core (struct sizes, `csmReadCanvasInfo`, mask arrays), add physics (`physics3.json`), pose (`pose3.json`), lip sync, drag-a-parameter UI.
3. **Spine gaps:** path constraints (JSON and binary), 4.2 physics (parsed and skipped in binary; JSON `physics` key ignored), dark / two-colour tint (read, not drawn), mixing between animations, image sequences (4.1+; only the first image name is used), binary 4.0 / 3.8 on real files, 4.3 binary (layout is in SkelToJson), non-uniform two-bone IK, edge anti-aliasing while playing.
4. **Other formats from `session23-plan.md`:** Krita `.kra` (LZF tiles), layered TIFF (tag 37724), AI layers (Optional Content Groups: test first that Windows.Data.Pdf honours `/OFF`), EPS layers, **UnityFS** bundles (the user's game caches in `AppData\LocalLow\Watcher of Realms\...\cacheimages\*.unity3d` are examples; do not try to get round encryption).
5. Panel polish: PSD layer masks / clipping masks / effects (the 14-level difference), the `.nulayers` pair file (`LayerSet::to_text / apply_text` exist, nothing writes it), a sidebar button for the panel (the 16-button list is counted by tests), `tools/test/e2e-features.ps1` is stale (expects 77 actions / 43 menu entries, finds 105 / 48) and hangs in its Settings section: update or retire it.
6. Housekeeping: version number / release of this work (0.7.0?), `nuviewer/tests/formats.rs` and `nv-editor` `extreme` fail since before session 22, translations, code signing.

## 5. Questions for the user at the start

- Version: is this 0.7.0 (CHANGELOG "Unreleased" -> a release), and should the installer / portable be rebuilt (`tools/package.ps1`, ~7.5 min, nothing of session 23 was packaged)?
- Do they have Live2D models and the Cubism Core, and Spine files of other versions (3.8, 4.0, 4.2) to check the readers on?
- Is the left-over black area of the first fit on `illust_dating20` acceptable, or should the fit follow the picture box (the item named `bg`)?
