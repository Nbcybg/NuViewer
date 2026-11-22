# Spine skeletons in NuViewer

Open a `.skel` (binary) or a skeleton `.json` (3.8, 4.0, 4.1, 4.2, 4.3). The `.atlas` and its page pictures must be beside it (same name, or the only `.atlas` in the folder). The skeleton plays at once (animation "idle" if there is one, else the first).

## Controls

| | |
|---|---|
| `P` / Space | play / pause |
| wheel | zoom (always, on a skeleton); drag pans; double click fits |
| `Shift+.` / panel *Speed* | speed 0.25x ... 4x |
| `Shift+L` / *Loop*, `Shift+K` / *Ping-pong* | loop, ping-pong |
| `Ctrl+L` | the panel: *Anim* / *Anim* (previous, next; the setup pose is one more stop), *Skin*, *Play*, speed, loop, ping-pong, the slots as layers (eye, name, opacity, filter, hover highlight, solo), *PNG frames*, *WebM alpha*, *Save* |
| `Ctrl+Shift+S` / *Save* | the pose on screen as a picture (video-snapshot settings; PNG keeps transparency, JPEG gets the canvas colour) |
| `Ctrl+Shift+F` / *PNG frames* | every frame as a numbered PNG with transparency in a new folder next to the skeleton |
| *WebM alpha* | VP9 with transparency through ffmpeg (needs libvpx-vp9; Settings > Archives & tools says where ffmpeg is looked for) |

## What is read, and how far it was checked

Real files of every version from 3.8 to 4.3 came from the example assets of [playcanvas-spine](https://github.com/playcanvas/playcanvas-spine) (Esoteric's own skeletons: 3.8 spineboy, 4.0 owl and raptor, 4.1 raptor, 4.2 spineboy and celestial-circus, 4.3 celestial-circus / diamond / snowglobe / tank, the 4.3 ones in both `.skel` and `.json`). They belong to Esoteric Software and are **not in this repository**: `crates/nv-anim/tests/real_samples.rs` and `tools/test/e2e-spine43.ps1` read them from `NUVIEWER_SPINE_SAMPLES` (default `T:\Benchmark-folder\spine-samples`, layout `spine-4.3/tank/tank-pro.json`) and skip themselves when the folder is absent.

| Part | Status |
|---|---|
| JSON 3.8 - 4.3 | bones (5 inherit modes), slots, skins (old and new form), region / mesh / weighted mesh / linked mesh (also 4.3's `source`) / clipping / path, IK (1 and 2 bones, softness, stretch), transform constraints (old and 4.3's property mappings), path constraints, 4.2+ physics, 4.3 sliders, all timelines (bone, slot colour incl. rgb / alpha / two colour, deform on meshes, clipping and paths, image sequences, draw order, IK / transform / path / physics / slider), curve dialects (3.x fractions, 4.x absolute), 3.x short-way rotation. **All ten real skeletons read and every animation of them poses without a NaN.** |
| Binary `.skel` **4.3** | `crates/nv-anim/src/binary43.rs`, written from the 4.3 runtime's reader. **Checked on four real files: read to the last byte, and every animation poses the same as the JSON export beside it** (`binary_43_matches_json_43`). One list of constraints in the order they work; transform constraints as property mappings; weighted vertices that say how many numbers they take; image sequences; draw order folders (skipped). |
| Binary `.skel` **4.1** | **checked on a real file** (4.1.11 from a game: 2218 bones, 1030 slots, 112 IK, 356 transform constraints, 100 animations, 5 MB): read to the last byte, every animation poses, the picture drawn is right (`tests/real_41.rs`, skipped where the file is absent). Found on the way: 4.1 has image-sequence blocks in region / mesh / linked-mesh attachments, a timeline count at the start of every animation, a type byte in front of attachment timelines, and draw-order offsets written as 32-bit varints. |
| Binary `.skel` **4.2** | follows the layout documented by the MIT-licensed [SkelToJson](https://github.com/BastienGimbert/SkelToJson) (flag bytes for constraints and attachments, reference scale, path and physics constraints and their timelines); **checked on its one real 4.2.43 sample** (`tests/real_42.rs`). Path and physics in the 4.2 binary were written from that document, not yet seen on a real file. |
| Binary `.skel` 4.0, 3.8 | from the format's layout as understood; **not checked against a real file** (no binary of these versions was found; their JSON is checked); strict (a wrong guess is an error, not a wrong picture). 3.8 is tried with and without a string table. |
| Constraints | IK (1 and 2 bones), transform (old and 4.3), **path** (constant speed or not, all spacing / rotate modes), **physics** (springs: x, y, rotate, scale, shear; needs the player's clock, see below), **sliders** (a bone property drives the time of an animation; the animation's bone *and slot* timelines apply). Checked on the tank (path + transform), snowglobe and celestial circus (physics), diamond (slider). |
| Animations | one at a time, with a quarter-second ease from the one before (bones ease, slots switch); the setup pose is one more stop. Not played: 4.3 bone `inherit` timelines, physics `reset` timelines. |
| Drawing | CPU rasteriser: normal / additive / multiply / screen, premultiplied pages, clipping (also deformed), vertex colour, **two-colour tint (`dark`)**, trimmed atlas regions packed at **0, 90, 180 or 270 degrees** (a region packed 90 degrees was drawn mirrored before: it is checked now with a four-colour test picture for regions and meshes), image sequences with their timelines. No edge anti-aliasing while playing (2x supersampling in exports and the snapshot). |
| Physics and time | `PhysicsSim` (in `nv-anim`) is the springs' memory: the player advances it by the real time (times the speed) every frame and renders from it; PNG frames and WebM start a fresh one and step it by 1/fps, so an export is repeatable; the snapshot uses the springs as they are on screen. A new animation restarts the springs. Without a `PhysicsSim` (thumbnails, the gallery, the first fit) physics stays at rest. |

Checked against the reference runtime's live page (playcanvas-spine, `spine-4.3.html`, runs in the in-app browser): circus, tank, snowglobe and the diamond now look the same. (The diamond was white until the *slider's* animation also applied its slot timelines: it recolours the facets, fades the gradient layers and plays the shine sequences.)

Fit on opening: the whole skeleton is fitted to the window, **leaving out** items that would shrink the picture to a speck: items far bigger than the rest (game rigs have black letterbox frames 40x the picture), big items whose texels are plain black, and parts parked far away (animators hide a part by moving its bone 100000000 units).
Speed: on the 2218-bone rig a pose with its 468 constraints takes ~1.5 ms and drawing 200 meshes at 1024 px ~12 ms (all cores), so it plays at the full 30 fps on the UI thread.

Tests: `crates/nv-anim` (39 unit tests, incl. regions at every quarter turn, two-colour tint, easing, 4.3 transform, slider, physics; `real_samples.rs` 6, `real_41`, `real_42`), `tools/test/e2e-spine.ps1` (7: opens, plays, pauses, panel in Spine mode, slot off, snapshot with a transparent corner) and `tools/test/e2e-spine43.ps1` (8: four real 4.3 `.skel` open, draw and move). Not run by a test: the frame export, the WebM export (needs ffmpeg), skin / animation buttons, speed.

The Spine runtimes are under Esoteric's own licence: the algorithms here (IK, constraints, physics, atlas rotation) follow the published runtimes' behaviour. If NuViewer is ever distributed beyond personal use, check that licence.
