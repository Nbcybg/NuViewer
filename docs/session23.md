# Session 23 (2026-11-22) - the layer panel (work package 1 of `session23-plan.md`)

Done: **one layer panel for PSD, GIMP XCF and OpenRaster files**, with the shared model the Spine / Live2D players will reuse. Not done in this session: Krita, layered TIFF, AI / EPS layers, and everything of work packages 2-4 (Spine, Live2D, UnityFS); see the end.

## What exists

| Piece | Where | Verified |
|---|---|---|
| Layers as stored + the compositor (`Blend` with 22 modes, `Layer`, `LayerDoc`, `Eff`, `compose` in row bands on all cores, `flatten`) | `crates/nv-codecs/src/layerdoc.rs` | 6 unit tests (stacking, offsets, clipping at the canvas, opacity, blend, group inheritance, unread layers, ORA file with groups / offsets / modes) |
| Readers: OpenRaster (`stack.xml` + one PNG per layer, nested stacks, offsets, `composite-op`), PSD (`psd` crate for pixels + **own record scan** for visibility / opacity / blend of layers and group folders), GIMP XCF (`designfmt::xcf_layers`; the old `xcf()` now flattens through the same compositor) | `layerdoc.rs`, `designfmt.rs` | ORA + XCF by unit test; **PSD on one real file** (`Llumisims_SetsukiEyes_PSD.psd`, 2048x4096, 60 layers, 15 groups: read in 360 ms, flattened in 9 ms) with `crates/nv-codecs/examples/layerinfo.rs` |
| Panel state: tree, filter (keeps the parents of a match; a matching group keeps its insides), folded groups, hover highlight (rest dimmed to 18%, a switched-off layer is shown for the preview), solo, group opacity x child opacity, "as saved" / zero for double / triple click, text form for a future `.nulayers` pair file | `crates/nv-core/src/layers.rs` | 7 unit tests |
| Viewer panel on the right edge: eye, name, opacity slider, fold arrow, filter box, *All on / All off / Reset / Save*, scroll, an outline of the highlighted layer's box on the picture | `nuviewer/src/layers_ui.rs`, key `Ctrl+L` (`Action::Layers`), View menu entry | `tools/test/e2e-layers.ps1`, **21 checks, all pass** (messages only; screenshots looked at) |
| Recompose off the UI thread, one job at a time (a change made meanwhile starts the next job when the first ends), result uploaded under the picture's id; leaving the file / closing the panel drops that texture so the file's own picture is read again | `layers_ui.rs` | e2e (eye, slider, double click, hover, reset) |
| **Save** (also `Ctrl+Shift+S` while the panel is open): the picture as the panel has it, with the video-snapshot settings; PNG / WebP / TIFF / TGA keep transparency, JPEG / BMP get the viewer's canvas colour behind the picture | `layers_save` | e2e: PNG next to the file, canvas size, transparent corner stays transparent |

## Things to know

- **PSD visibility is read by our own record scan.** The `psd` crate (0.3.5) reads the "visible" flag the wrong way round (bit 1 *set* = hidden) and takes a group's settings from the closing divider instead of the folder's own record, so on the real file it showed everything hidden and the groups wrong. After the fix only the group "Human" is on, which is what that file is.
- The flattened PSD differs from the picture the file stores: mean 13.7 levels over the pixels with content on the real file (ours is darker by 15-20 levels; alpha is identical). Suspects: layer masks, layer effects and some blend modes, which are not applied (the panel says so under the list). **Not isolated.** Until the user changes something the viewer keeps showing the file's own merged picture, so there is no jump on opening the panel.
- Non-separable blend modes (hue, saturation, colour, luminosity) and dissolve are drawn as normal; clipping masks, layer masks (PSD), adjustment layers and effects are ignored; groups are not isolated buffers. PSD layers are read for 8-bit RGB documents only; PSB and 16-bit give a message and the merged picture stays.
- Memory: layers are cropped to their content (PSD canvas-size buffers are cut down), 1 GB cap on layer pixels.
- The panel is not in the sidebar (its button list is fixed at 16 and tests count it).
- Test hook: `NUVIEWER_TEST=1` puts the panel's geometry and state in the window title.

## Later in the session

Spine and Live2D players were built on top of the layer panel (the same panel lists slots / parts): see `docs/spine.md`, `docs/live2d.md` and the handoff `docs/session24-plan.md`, which is the current status.

## Not done

- **Krita (`.kra`)**: layers are in Krita's own tile format (LZF) and need a real file to check. **Layered TIFF** (tag 37724) and **AI / EPS layers** need real files and the Windows.Data.Pdf `/OFF` test first.
- **Hover highlight only dims**; there is no per-layer opacity "outline only" mode.
- The user's choice is not saved between runs (`LayerSet::to_text` / `apply_text` exist and are tested; nothing writes the pair file yet).
- Work packages 2-4: Spine player + export (`nv-anim`), Live2D + Cubism Core, UnityFS. **Questions that must be answered first:** which Spine version(s) the files use, whether the user has `Live2DCubismCore.dll` (and may bundle it).
