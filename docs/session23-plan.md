# Handoff for session 23: layers everywhere, and a Spine / Live2D viewer like BD2 Viewer

Written at the end of session 22 (2026-11-22, version 0.6.0). Read `docs/session22.md` (what exists and what was verified) first, then this. Nothing of session 22 is committed yet (about 144 changed / new files on `main`): ask the user before committing.

## 0. What the user asked for (their words, summarised)

References (looked at by the user, **not** opened by me; I do not know what they are built with, "probably spine-webgl / pixi-live2d-display" is a guess):
`https://jelosus2.github.io/BD2-L2D-Viewer/` (Spine + Live2D character viewer) and `https://l2d.su/` (Live2D viewer). Not to pull assets from them: only the *features* are wanted, applied to files the user has.

1. **One layer panel for every file that has layers**: turn each layer on / off, **filter** layers by name, **hover a layer to highlight it** on the picture (l2d.su), and an **opacity 0-1 per layer** (l2d.su uses transparency instead of off).
2. **Files with layers**: PSD, XCF, Krita, ORA ... and the user added: **TIFF, AI and EPS also have layers - do not forget them** (see section 3 for what is realistic).
3. **Spine and Live2D must be viewable the way BD2 Viewer does**: animation list, skin, **speed, loop on / off, ping-pong, mouse wheel zoom, drag to pan**, layer toggle + filter, and **export**: **WebM with alpha**, **frame sequence with alpha**, **snapshot**.
4. **Snapshot** as PNG with alpha, or JPG / any format without alpha **on the chosen background** (both sites do this; ours = the background the viewer shows).
5. File types to accept (all of them, as far as possible): Unity `.bundle .ab .unity3d .unityfs`; Live2D `.model3.json .cmo3 .cmp3 .moc3 .physics3.json .cdi3.json .motionsync3.json .userdata3.json .can3 .ctmp3 .exp3.json .pose3.json .motion3.json .paramctrl3.json`; Spine `.skel .json .atlas .spine`.

My proposed order (the user saw it and then asked for this handoff, so treat it as agreed unless they say otherwise):
**(1) layer panel on layered pictures -> (2) Spine player + export -> (3) Live2D JSON files, Cubism Core for `.moc3` -> (4) UnityFS reader.**

## 1. The shared design (build once, use everywhere)

A **layer model** that every source fills in, and one panel that edits it:

```text
LayerNode { id, name, kind (pixel / group / drawable / slot / part ...), parent, visible, opacity 0..1, blend, bounds? }
LayerSet  { nodes, hover: Option<id>, solo: Option<id>, filter: String }
```

- Pure code in a new module (suggest `crates/nv-core/src/layers.rs`; tests there): tree, filter (case-insensitive substring, keeps the parents of a match), solo, "effective visibility / opacity" (group x child), serialisation of the user's choice per file (a `.nulayers` pair file like `.nuedit`, optional).
- Panel UI: viewer (left edge bar `V` exists: `nuviewer/src/sidebar.rs`) and gallery preview. Rows = eye / name / opacity slider (double click = as opened, triple click = 0: `Event::TripleClick` and the Form-less slider code in NuEditor `panel.rs` already do that). Hover a row -> highlight that layer on the picture (draw it at full strength and dim the rest, or outline it).
- Rendering a layered picture with choices: PSD / XCF / ORA / Krita = composite the pixel layers on the CPU (XCF already does: `designfmt.rs::xcf` composites layers with blend modes, opacity, masks, groups; reuse its compositor), cache each layer's pixels, re-composite on change (only the changed rectangle if it is slow).
- **Snapshot / export**: reuse `nv-core/src/frames.rs` (`Sink`: format from the video snapshot settings, **alpha rule: a run is PNG when the format cannot carry transparency**, never black) and `nv-win/src/progress.rs` (modal progress + Cancel). Snapshot with a chosen background = composite on the viewer's canvas colour (`Settings.more.canvas_*`, `clear_color()` in `nuviewer/src/main.rs`) for formats without alpha; PNG / WebP / TIFF keep alpha.

## 2. Spine and Live2D player (the big piece)

What exists today: only cards / the sprite sheet (`nv-codecs/src/animfmt.rs`: `parse_atlas`, `skel_version`, sibling-atlas lookup). **No animation, no rig.** Everything below is new.

### 2.1 Architecture (suggested)
- New crate `crates/nv-anim` (pure, no window, heavily unit-tested): `Skeleton` (bones, slots, skins, attachments region / mesh / linked mesh / clipping / path, constraints IK / transform / path, animations), sampling (`pose_at(animation, t)`), **geometry output** = a list of draw batches `{texture page, blend mode, premultiplied?, vertices (x, y, u, v, rgba), indices}` in draw order. The same output type for Live2D (`Drawable`s), so one renderer draws both.
- Renderer: new pipeline in `nv-render` (dynamic vertex / index buffers, textured triangles, blend modes normal / additive / multiply / screen, premultiplied alpha, clipping via stencil or a second pass). The 3D `mesh.rs` is a good template (dynamic buffer `overlay_vb`, `RefCell` buffer reuse, depth state without depth).
- Viewer integration (`nuviewer`): a new mode next to model view (`is_model()`); reuse the playback bar: `controls.rs::MediaKind` has `Turntable` as the example of adding a kind (play / pause, step, seek track = animation time, speed button, loop, **ping-pong** exists as `settings.more.play_pingpong`, `pp_back`). **Mouse wheel zoom + drag pan** like the image view (`mouse.rs`, `View::zoom_at`, pointer-anchored). Animation and skin choosers: a small list in the side panel (not hidden in menus).
- Offscreen rendering for export: a D3D11 render target + staging texture readback in `nv-render` (does not exist yet; the viewer only draws to the swap chain). Frames -> `frames::Sink` for a sequence; for **WebM with alpha** pipe raw RGBA to ffmpeg: `ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r FPS -i - -c:v libvpx-vp9 -pix_fmt yuva420p -auto-alt-ref 0 -b:v 0 -crf 24 out.webm`. **Check `ffmpeg -encoders` has libvpx-vp9 first** (`nv_core::ffmpeg::find`, `has_filter` shows the pattern for asking ffmpeg); say so in the UI when it is missing. ffmpeg is installed on the user's PC (WinGet) but Windows builds differ.

### 2.2 Spine
- Spec is public (JSON format docs, binary format docs). Versions differ: 3.8, 4.0, 4.1, 4.2 - **start with JSON of one version the user's files use (ask!)**, then `.skel` binary (varint strings, per-version layout) as a separate step, then the others.
- Files: `.json` is ambiguous (detect by the `"skeleton"` object with `"spine"` version and `"bones"`; do not claim every `.json`), `.skel`, `.atlas` (+ page PNGs; premultiplied-alpha flag `pma: true` in 4.x atlases), `.spine` project = closed (card).
- Layers = slots (and, inside a slot, attachments): toggle / opacity per slot maps to the slot colour alpha; filter by slot name; hover = highlight the slot's triangles. Skins: list from the file.
- Licence note for the user: write our own runtime from the documented format; do not copy Esoteric's runtime code.

### 2.3 Live2D
- **Easy, do first:** `.model3.json` (lists moc3, textures, physics, pose, expressions, motions), `.cdi3.json` (display names of parameters and parts: **this is where readable layer / part names come from**), `.motion3.json` (curves: linear / stepped / bezier segments; sample into parameters), `.exp3.json`, `.pose3.json`, `.physics3.json`, `.userdata3.json`, `.motionsync3.json`. All JSON (`serde_json` is a dependency).
- **`.moc3`**: binary and read by Live2D's **Cubism Core** (closed, licensed). Plan: load `Live2DCubismCore.dll` dynamically with `LoadLibraryW` (the way `nv-media` binds Media Foundation late, so start-up costs nothing), call its C API (revive moc in place, initialise model, set parameter values, update, read drawables: vertex positions / uvs / indices / draw order / opacity / constant flags for blend + double sided + clipping masks, part opacities), and say plainly where to put the DLL when it is missing (the user supplies it; check Live2D's licence before ever shipping it in the installer). Parts' opacity are writable: that is the "opacity 0-1 instead of off" the l2d.su viewer does; hover highlight = hit-test the drawable triangles under the pointer.
- Own parser of `.moc3` from community documentation is a **much** bigger and riskier job: do not start it unless the user asks and accepts that.
- Physics (`.physics3.json` pendulums) and breathing / eye blink / lip sync are optional extras after the basic motion playback works.
- `.cmo3 .cmp3 .can3 .ctmp3 .paramctrl3.json` are editor project files: closed. Realistic goal: recognise them, show a card, pull textures / names out only if a real file shows how (a `.cmo3` is believed to be a CAFF archive with XML + PNG: **unverified**). Do not promise more.

### 2.4 Unity bundles (last, large)
`.bundle .ab .unity3d .unityfs` = UnityFS container: header, block table (LZ4 / LZMA / none), then serialized files with a type tree; objects of interest: `TextAsset` (Spine skel / atlas, sometimes `.moc3` bytes), `Texture2D` (formats ASTC / ETC / DXT / crunch to decode), `Sprite`, `MonoBehaviour`. New dependencies likely: an LZ4 and an LZMA decoder (check licences, `docs/third-party`). **Encrypted bundles: do not try to get round the encryption or any DRM; say it is encrypted.** Local files the user already has only; no downloading from sites.

## 3. Layers in TIFF, AI, EPS (the user's reminder) - what is realistic

| Format | Where its layers are | Plan | Honest risk |
|---|---|---|---|
| **TIFF** | Photoshop-style layered TIFF: the layers sit in tag 37724 (ImageSourceData, same layer records as PSD) next to the flattened image; also **multi-page** TIFF (pages are not layers but should be listed the same way) | read the tag with the PSD layer parser; pages as a list. `crates/nv-codecs/src/psdraw.rs` and the `psd` crate show how layer records are read | tag layout needs a real file to check |
| **AI** (Illustrator) | PDF-compatible: layers are **Optional Content Groups** (`/OCProperties`, `/OCGs`, default config `/D` with `/ON` `/OFF`) | list the OCG names from the PDF catalogue. To show / hide: rewrite the file's default config in a temporary copy (an appended incremental update) and render it again with Windows.Data.Pdf (`nv-codecs/src/pdf.rs::render_with`). **Test first that Windows.Data.Pdf honours `/OFF`** - if it does not, there is no cheap way (a PDF content-stream interpreter is a project of its own). Opacity per layer is not possible this way (on / off only) | the whole plan stands or falls with that one test |
| **EPS** | Illustrator EPS: DSC comments (`%AI5_BeginLayer` ... `%AI5_EndLayer`) around each layer; plain EPS has none | list layers from the comments; hiding one needs the PostScript filtered before Ghostscript renders it (`nv-codecs/src/eps.rs` shells out to Ghostscript, not verified on real EPS in earlier sessions) | research item; if too fragile, show the layer *names* and say hiding is not supported |

Say this in the UI, not in silence: a layer panel that lists names but cannot hide them must be marked "view only".

## 4. Suggested work packages (one session each at most)

1. `layers.rs` + panel + PSD layer reading (real layers) + XCF + ORA (`stack.xml` + a PNG per layer: easy) + Krita (`maindoc.xml` + tile data: LZF-compressed tiles, harder; ORA-like export first) + TIFF tag 37724. Snapshot with background. **Look at it in a running window** (PrintWindow script, see section 6).
2. `nv-anim` Spine JSON (one version) + atlas + draw + the playback bar + wheel zoom / pan + skin / animation lists + slot layers. Then offscreen render + frame sequence + WebM alpha + snapshot. Then `.skel`.
3. Live2D JSON family + Cubism Core loader + motions + parts opacity + hover highlight; exports share work package 2's code.
4. AI / EPS layers (after the OCG test), UnityFS.

## 5. What exists that you will reuse (file pointers)

- Frames / alpha / progress: `crates/nv-core/src/frames.rs`, `snapshot.rs` (`take_shown_labeled`), `ffmpeg.rs` (`frame_codec_args`, `extract_frames`), `crates/nv-win/src/progress.rs`.
- Playback bar and turntable as the pattern for a new kind: `nuviewer/src/controls.rs`, `turntable.rs`; 3D draw pipeline: `crates/nv-render/src/mesh.rs`, `shaders/mesh.hlsl`; viewer mouse (wheel / pan / pointer-anchored zoom): `nuviewer/src/mouse.rs`, `display.rs`.
- Layers today: XCF compositor `nv-codecs/src/designfmt.rs`; PSD merged only `psddec.rs`, raw parts `psdraw.rs`; Spine atlas `animfmt.rs`; model skeleton (glTF bones) `nv-codecs/src/model.rs` (`Bone`, `gltf_bones`).
- New settings pattern: `crates/nv-core/src/prefs.rs` (`frames_every` shows the three places: struct, `apply`, `serialize`), settings UI `nuviewer/src/settings_more.rs`.
- Keys: `crates/nv-core/src/keys.rs` (**every letter key is taken; Shift / Ctrl combos left: see the list printed by the `a!` table**), a test fails on duplicates.
- Tests: `tools/test/*.ps1` e2e scripts; screenshot helper `tools/test/shot.ps1` (starts a program on a file, captures its window with PrintWindow, no real input).

## 6. Working notes and gotchas (read before touching the shell)

- **Patching files:** the Bash tool turns `\n \t \u` inside heredoc Python into real characters. Write a script file with the Write tool and run it; `tools/dev/patch.py` is the helper (`patch(path, [(old, new)])`, exact match must be unique, CRLF-tolerant, tokens `<NL>` `<T>` `<BS>`; it exits before writing if anything fails). Mixed line endings in the repo: edit tools that keep them.
- **Build:** `CARGO_TARGET_DIR=R:\cargo-target\NuViewer`. A full release build is about **7.5 minutes** (`CARGO_BUILD_JOBS=3`); `tools\package.ps1` builds all six programs + the setup + the portable zip and checks the zip. **R: filled up once** (0 bytes free) because debug and release both live there: delete `R:\cargo-target\NuViewer\debug\incremental` (use PowerShell `Remove-Item` in a background process; `du` / `rm` in Git Bash take minutes) when "not enough space" appears. P: has ~10 GB.
- `cargo ... &` in the Bash tool returns at once (the completion note is for the launcher): wait with `until ! tasklist | grep -qi "cargo.exe\|rustc.exe"; do sleep 10; done`.
- **Do not kill the user's installed programs** (NuQuick runs from `C:\Users\noobc\AppData\Local\Programs\NuViewer`); the e2e / screenshot runs need no other NuGallery / NuViewer open (single-instance forwarding).
- No real mouse / keyboard input into the machine (a game may be in front): post messages or use PrintWindow ([[feedback-no-real-input-into-games]]).
- **Known failing tests that were failing before session 22:** `nuviewer/tests/formats.rs` (file-type chooser vs decoders: the chooser lists archive formats the decoder list does not, and lacks sai / oci / clip ...: decide which list is right), `nv-editor` `extreme` (debug overflow in `blur.rs`), and `nugallery tree::expanding_and_collapsing...` fails now and then and passes on a rerun.
- `cargo check` runs the HLSL build script (shader errors show up there).
- Windows.Data.Pdf paints white paper unless the background colour is set transparent (done in `pdf::render_with(paper = false)`); a GIF pixel that is transparent shows the previous frame unless that frame's disposal is "restore to background".
- RichEdit (NuNote): formatting must suspend undo (`ITextDocument::Undo(tomSuspend)`) and keep the modified flag, or the colouring pollutes Ctrl+Z and the title shows `*`.

## 7. Open items carried over from session 22 (do not lose)

- Never run by hand: the share sheet, the progress window, NuNote's find / replace dialogs, the slider double / triple click, extracting frames of a **video** (needs ffmpeg), the group view's keyboard (arrows across groups) in a real window, the new 3D modes on a rigged model.
- Real files wanted for: Spine / Toon Boom / Moho / Animate / Pencil2D / Synfig / OpenToonz readers (`animfmt.rs`, tested on made-up files only); a rigged glTF / VRM for bones; a Live2D model + Cubism Core; layered TIFF, AI and EPS files.
- Translations (the seven language slots show English); code signing; the gallery's CPU first frame draws no group headers; HDR video frames are extracted without tone mapping.
- Questions to ask the user at the start: which **Spine version(s)** do their files use; do they have `Live2DCubismCore.dll` (and may it be bundled); which of "layer panel first" or "Spine first" they want; whether to commit session 22 as 0.6.0 (not asked answered yet).
