# Session 24 (2026-11-22) - Spine 4.3, path / physics / slider constraints, and what real files showed

Started from `docs/session24-plan.md`. The user pointed at `playcanvas/playcanvas-spine`, which turned out to hold real Spine skeletons of **every version from 3.8 to 4.3** (the 4.3 ones as `.skel` *and* `.json`), a **complete 4.3 spine-core** (`build/playcanvas-spine.4.3.js`, the reference for everything below) and a live page that runs them in WebGL (`https://playcanvas.github.io/playcanvas-spine/examples/spine-4.3.html`, works in the in-app browser: use it to compare pictures).

The sample files are kept in `T:\Benchmark-folder\spine-samples\` (not in the repository: Esoteric's assets). Download list: `examples/assets/spine-<ver>/<name>/` of that repo, files `*-pro.json|skel`, `*-pma.atlas`, `*-pma*.png`. Tests read them from `NUVIEWER_SPINE_SAMPLES`.

## What was done

| Area | Where | Checked how |
|---|---|---|
| 4.3 JSON: one `constraints` list (ik / transform / path / physics / slider), new transform constraint (`source`, `properties: from -> to`), linked meshes as `source` / `slot` / `timelines`, deform under the animation's `attachments` | `nv-anim/src/json.rs` | all 10 real skeletons read, every animation poses without NaN (`tests/real_samples.rs`) |
| 4.3 binary | `nv-anim/src/binary43.rs` | the four real `.skel`: read to the last byte, every animation poses like the JSON beside it (`binary_43_matches_json_43`) |
| Path constraints (3.8 - 4.3), 4.2 / 4.3 physics, 4.3 sliders, 4.3 transform | `nv-anim/src/constraints.rs`, loop in `pose.rs::pose_mix`, readers in `json.rs`, `binary.rs`, `binary43.rs` | tank treads follow the path, physics lags and settles (synthetic + celestial circus), slider drives a bone (synthetic + diamond), transform copies a mapped property (synthetic) |
| Physics clock | `PhysicsSim` (`constraints.rs`); the player advances it, exports step it by 1/fps, the snapshot clones the on-screen state | e2e: snowglobe / circus move by themselves |
| Image sequences + their timelines | `model.rs` (`Seq`, `SeqTl`, `sequence_frame`), readers, `pose.rs` | diamond draws (shine frames) |
| Atlas regions packed at 180 / 270 degrees; 90 fixed | `atlas.rs`, `pose.rs::region_item / mesh_item` | `regions_/meshes_packed_at_every_quarter_turn_come_out_upright` (four-colour picture); circus wings are right now |
| Two-colour tint | `DrawItem.dark`, `raster.rs` | unit test (black texels become the dark colour) |
| Crossfade between animations (0.25 s) | `Rig::pose_mix`, `Player.fade` | unit test; not seen by eye |
| Deform of clipping polygons and paths; deform timelines of bounding boxes are read and dropped | `model.rs::Attachment::deform_len` | tank (its clipping is deformed) reads |
| Settings > Archives & tools text clipped | `nuviewer/src/settings_more.rs` (`elide`) | looked at in a screenshot |

## Facts that are easy to lose

- **The 4.3 runtime in `playcanvas-spine.4.3.js` is the spec.** SkelToJson's 4.3 reader is for an *older* 4.3 beta: IK flag bits differ, slider data is cut short. Layout notes: bones `inherit` is a byte *before* `length`; non-essential bone data = colour, icon, icon size, icon rotation, visible; constraint block = name, type byte (0 ik, 1 path, 2 transform, 3 physics, 4 slider), no `order` (the list order is the order of work); weighted vertices have a *total number count* varint before the per-vertex bone data (`binary43::read_vertices43` checks it); a linked mesh has a source *slot* index before its skin index; meshes have a `timelineSlots` list after the triangles; animation constraint timelines use the unified index (physics: index + 1, 0 = skeleton-wide); the slider's animation numbers come after all animations; draw order folders follow the draw order.
- **Atlas `rotate: N` means "packed turned N degrees counter-clockwise"** (libgdx). Region corners (bottom-left, top-left, top-right, bottom-right): 0 `(u,v2) (u,v) (u2,v) (u2,v2)`; 90 `(u2,v2) (u,v2) (u,v) (u2,v)`; 180 `(u2,v) (u2,v2) (u,v2) (u,v)`; 90 / 270 swap the page rectangle (`u2 = (x + h) / pw`). The old code had 90 mirrored.
- The 4.3 JSON export drops tiny numbers (a bone x of -0.0015 is missing); compare binary with JSON with a tolerance.
- 4.1 / 4.2 JSON files keep `deform` timelines under the animation's `attachments` key (`skin -> slot -> attachment -> { deform | sequence }`); only <= 4.0 uses a top-level `deform`.
- Physics needs a clock: `PhysicsSim::advance` clamps a step to 0.1 s so a stall does not blow the springs up. Without a sim (thumbnails, gallery, first fit) physics stays at rest.
- World-space constraints (path, physics, 4.3 transform with world target) modify the bone's world matrix, then `set_local_from_world` and `update_below` (the bone itself is not recomputed from its local values); local-target ones use `update_subtrees`.
- `riginfo --hide a,b` leaves slots out of the picture, `riginfo --slotcolors` draws every slot alone on black and prints coverage and mean colour: the way to find which layer tints a picture wrong. `NV_SKEL_NO_JSON=1` stops `.skel` falling back to the `.json` beside it (without it riginfo "reads" 4.3 binaries from their JSON and proves nothing).
- A shell `cd` in the Bash tool changes the session's working directory for later calls: use absolute paths.

## Not done / not verified

1. ~~The 4.3 diamond is white~~ **fixed**: a slider's animation must apply its *slot* timelines (colours, attachments, sequences) as well as its bone timelines. Found by hiding layers (`riginfo --hide gradient`, `--slotcolors`) and reading the slider animation's JSON; compare against the reference page for the *same animation* (the first look compared different animations).
2. 4.2 binary path / physics were written from the SkelToJson document and never seen on a real file; binary 3.8 / 4.0 still have no real file. A `.skel` written by Spine 3.8 / 4.0 / 4.2 from the user would settle it.
3. Not played: 4.3 bone `inherit` timelines, physics `reset` timelines (the sim just keeps going), draw order folders (read and skipped), non-uniform two-bone IK, edge anti-aliasing while playing.
4. The crossfade, the Anim / Skin / Speed / Ping-pong buttons, PNG frames and WebM export were not clicked through (compiled and unit-tested only). The panel and the Settings group were looked at in screenshots.
5. Live2D: still only the mock Core. Everything in `docs/session24-plan.md` items 2, 4, 5, 6 stands (Krita, layered TIFF, AI / EPS layers, UnityFS, PSD masks, `.nulayers`, version number / release, `e2e-features.ps1` stale).
6. **Licence**: the Spine runtimes are under Esoteric's licence and `nv-anim` follows their algorithms closely (IK, constraints, physics, atlas rotation). Fine for personal use; check before NuViewer ships to others.

## Questions for the user

- Version for this work (0.7.0?) and rebuild installer / portable (`tools/package.ps1`, ~7.5 min; nothing of sessions 23 and 24 is packaged)?
- A Live2D model and the Cubism Core? Spine 3.8 / 4.0 / 4.2 binary files from real games?
