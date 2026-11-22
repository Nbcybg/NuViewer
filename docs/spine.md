# Spine skeletons in NuViewer

Open a `.skel` (binary) or a skeleton `.json` (3.8, 4.0, 4.1, 4.2). The `.atlas` and its page pictures must be beside it (same name, or the only `.atlas` in the folder). The skeleton plays at once (animation "idle" if there is one, else the first).

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

| Part | Status |
|---|---|
| JSON 3.8, 4.0, 4.1, 4.2 | bones (5 inherit modes), slots, skins (old and new form), region / mesh / weighted mesh / linked mesh / clipping, IK (1 and 2 bones, softness, stretch), transform constraints, all timelines (bone, slot colour incl. rgb / alpha / two colour, deform, draw order, IK / transform mixes), curve dialects (3.x fractions, 4.x absolute), 3.x short-way rotation. **Tested on skeletons made for the tests; no real Spine file was available.** |
| Binary `.skel` **4.1** | **checked on a real file** (4.1.11 from a game: 2218 bones, 1030 slots, 112 IK, 356 transform constraints, 100 animations, 5 MB): read to the last byte, every animation poses, the picture drawn is right (`crates/nv-anim/tests/real_41.rs`, skipped where the file is absent). Found on the way: 4.1 has image-sequence blocks in region / mesh / linked-mesh attachments, a timeline count at the start of every animation, a type byte in front of attachment timelines, and draw-order offsets written as 32-bit varints. |
| Binary `.skel` **4.2** | follows the layout documented by the MIT-licensed [SkelToJson](https://github.com/BastienGimbert/SkelToJson) (flag bytes for constraints and attachments, reference scale, physics constraints read and skipped); **checked on its one real 4.2.43 sample** (`tests/real_42.rs`: bone, slot, region with image sequence, animation). Anything beyond that sample (4.2 meshes, IK flags, timelines) is built from that document and not yet seen on a real file. 4.3 is refused. |
| Binary `.skel` 4.0, 3.8 | from the format's layout as understood; **not checked against a real file**; strict (a wrong guess is an error, not a wrong picture). 3.8 is tried with and without a string table. |
| Binary with path constraints | refused ("export JSON"); a `.skel` that cannot be read uses the `.json` of the same name beside it. |
| Path constraints, 4.2 physics, events, mixing between animations | not applied (named in the panel). |
| Drawing | CPU rasteriser: normal / additive / multiply / screen, premultiplied pages, clipping, vertex colour, trimmed and rotated atlas regions. Two-colour tint (`dark`) is read but not drawn. No edge anti-aliasing while playing (2x supersampling in exports and the snapshot). |

Fit on opening: the whole skeleton is fitted to the window, **leaving out** items that would shrink the picture to a speck: items far bigger than the rest (game rigs have black letterbox frames 40x the picture), big items whose texels are plain black, and parts parked far away (animators hide a part by moving its bone 100000000 units).
Speed: on the 2218-bone rig a pose with its 468 constraints takes ~1.5 ms and drawing 200 meshes at 1024 px ~12 ms (all cores), so it plays at the full 30 fps on the UI thread.

Tests: `crates/nv-anim` (33 + the two real-file tests), `tools/test/e2e-spine.ps1` (7: opens, plays, pauses, panel in Spine mode, slot off, snapshot with a transparent corner). Not run by a test: the frame export, the WebM export (needs ffmpeg), skin / animation buttons, speed.
