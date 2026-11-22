# NuEditor: design and status

NuEditor is the third program of the family: **NuViewer** (look), **NuGallery** (find / manage), **NuEditor** (change). The aims, in
the order they win a conflict: **fast, stable, minimal UI, easy**. Compared against FastStone Viewer and Windows Photos on this PC
(macOS Preview not available here).

## 1. One idea per program, one hand-over between them

```
NuGallery --F4 / menu--> NuEditor --Esc--> NuGallery   (same window place, same picture selected, folder re-read)
NuViewer  --E----------> NuEditor --Esc--> NuViewer    (viewer closes while editing, comes back on the same picture)
NuGallery --Space--> NuViewer --E--> NuEditor --Esc--> NuViewer --Esc--> NuGallery   (the chain is kept: --via-gallery)
```

* Separate processes, like NuGallery, so the viewer stays small and its first image stays at ~50 ms. Nothing of the editor is loaded
  until you press E / F4.
* Hand-over = `nv_win::instance` (named mutex + window class + `WM_COPYDATA`), `Program::Editor` is the third entry. The editor
  takes `--from viewer|gallery`, `--via-gallery`, `--angle <deg>`; on leaving it tells the program it came from (or starts it).
* Window place / fullscreen are shared through `winstate`, so the three look like one program.
* A turn that is pending in the viewer is **not** saved or asked about: it travels to the editor as the starting angle
  (`--angle`), so nothing is written twice and nothing is lost.
* Colour: files stay **sRGB** (embedded profile -> sRGB on load, like the clipboard and rotate-save). Only what is *shown* goes
  through the output profile chosen in Settings > Colour (`srgb_to_output`), so "ICC from the setup" holds without ever baking the
  monitor profile into a saved file.

## 2. What makes it fast

| Rule | How |
|---|---|
| Work is bounded by the **window**, not the picture | The preview is the *proxy*: the picture shrunk to the size it is shown at (`ensure_proxy`). A slider costs the same for 2 MP and 100 MP. Full size is touched only by saving. |
| First image like the viewer | Window, GPU device (thread) and decode (thread) start together; first frame on the CPU (GDI) when the picture is small enough, GPU after. |
| Table-driven per-pixel engine | Everything that works on one channel (white balance, exposure, levels, brightness, mid-tones, gamma, contrast, curves, invert) is folded into **three 256-entry byte tables**; a move costs one table read per byte. Highlights / shadows, saturation, hue, vibrance, mid-tone shift and the vignette use the general path (table reads + a few multiplies per pixel, no allocation). |
| All cores | Rows are split over `available_parallelism` threads (not for tiny buffers). |
| Unchanged = copy | `Recipe::is_identity` short-circuits the engine. |
| One engine for everything | Preview, burn-in, batch (NuGallery) use `nv_editor::engine`, so what you see is what is written and there is one thing to keep correct (no GPU/CPU parity problem). A GPU path is only worth adding for neighbourhood filters (blur, clarity, NR) on big proxies. |
| Recipes are tiny | Undo = a list of recipes (a few hundred bytes each), not pixel copies. |

Measured (release, this PC, GTX 1050 Ti, machine busy): engine on a 1920x1080 proxy 3.3 ms (tables) / 6.6 ms (general) / 12.3 ms
(everything on); first image of a 12 MP JPEG 113-119 ms (decode ~100 ms, the same decoder as the viewer); viewer E -> editor
window with the picture 159 ms; editor Esc -> viewer with the picture 52 ms. The editor shows "edit N ms" in its title and panel.

The preview runs on a **worker thread** (E3): every change sends the newest recipe to it, an older request that is still waiting
when the thread gets to it is dropped, so a slider never queues work and the window never waits for the engine. The 100% view
renders only the visible part of the picture (a piece of the pyramid, with a margin for the neighbourhood filters).
Not done: SIMD for the general path.

## 3. The recipe (what the user did)

`nv_editor::recipe`: one table `PARAMS` (group, key, label, min, max, default) generates the slider enum `P`; the UI, reset, identity
test and the pair-file writer all walk that table. Not-a-slider data are fields: `invert`, `wb_chart` (picked neutral), `curves`
(master, r, g, b), `hue_wheel` (8 bands), `quarter_turns`, `flip_h/v`, `crop`, `retouch` (clone / heal / remove / patch / red-eye
strokes, in 0..1 of the uncropped, unturned picture), `lut`, `auto`.

Pipeline order (`engine::render`): heal strokes (on the picture's own pixels, in 0..1 of the original) -> geometry (turn, flip,
keystone / zoom, crop) -> white balance + exposure in linear light -> highlights / shadows (luma mask) -> per-channel tone tables
-> mid-tone colour shift -> saturation + hue matrix -> vibrance -> hue wheel -> invert -> vignette (relative to the cropped frame)
-> LUT -> noise reduction (luma, colour) -> clarity -> sharpen -> skin smoothing -> bloom -> film blur -> grain.
Every neighbourhood radius is a fraction of the picture's long side (`spatial::unit`), so the preview, the 100% view and the saved
picture look the same.

## 4. Keeping edits: the three ways, and getting back

Leaving with edits (Esc, close box, opening another picture) asks once. The button Enter presses is your choice in
**Settings > NuEditor** (factory: the pair file, the one that cannot lose anything); the same page can turn the question off, so the
chosen way is used without asking. Ctrl+S inside the editor follows the same choice.

| Choice | Result | Undo |
|---|---|---|
| **Pair file** | `IMG.jpg.nuedit` next to the picture (plain text, see below), picture untouched | delete the file, or Reset all and leave |
| **New file** | `IMG_edit.jpg` (`_edit (2)` ...), same format when we can write it (JPEG q95 4:4:4 + EXIF, PNG), else PNG | delete it |
| **Burn in** | the picture is replaced; the original goes to `%LOCALAPPDATA%\NuViewer\undo` for 7 days; the pair file is removed (the edit lives in the pixels now). Formats we can't rewrite (RAW, GIF, WebP ...) get a PNG copy instead. | restore from the backup folder |
| Discard / Keep editing | nothing written / back to the picture | |

Inside a session: Ctrl+Z / Ctrl+Y (200 steps), right-click a slider = default, double click = as it was before the last save, triple click = zero, right-click a group header = reset
group, `0` / Reset all, `C` = show the original. Opening a picture that has a pair file applies it (with a note when the
picture's size differs). The pair file is one line per group (`light exposure="0.5" contrast="12"`), hand-editable, forward-tolerant (unknown lines and
keys ignored, newer versions refused). It is **not** `.xmp`: darktable / Lightroom keep their own `IMG.RAF.xmp` next to the same
pictures, and NuEditor never reads or overwrites those.

## 5. Your list against what exists today

Legend: **done** (engine + UI), **engine** (recipe + engine, no UI yet), **data** (recipe / pair file only), **todo**.

| Area | Item | State |
|---|---|---|
| White balance | temp, tint | done |
| | colour chart (neutral picker) | done: key `W`, click something grey / white (11x11 average in linear light); the White balance group reset clears it |
| | shift | done: four axes, green / magenta and blue / yellow, each for the **shadows** and for the **highlights** (luma masks), for correcting a camera cast |
| Light | exposure, brightness, contrast, highlights, midtones, shadows, white / black point, gamma | done |
| | curve | done: panel section "Curves" (RGB / R / G / B tabs; click adds, drag moves, right-click removes a point; monotone cubic) |
| Colour | saturation, vibrance, hue | done |
| | invert | done (key `I`) |
| | hue wheel (8 bands) | done: panel section "Hue wheel" (click the ring to choose red / orange / yellow / green / aqua / blue / purple / magenta; hue shift up to 30 degrees, saturation, brightness; neighbouring hues are shared linearly between two bands) |
| Detail | vignette | done |
| | sharpen (with radius), clarity (both ways), bloom (threshold, radius) | done (E4, `nv_editor::spatial` on `nv_editor::blur`) |
| Composition | free rotate, +-45 (keys `,` `.`), flip H / V (keys `H` `V`) | done |
| | crop + straighten | done: key `R`; drag the frame (corners, edges, inside), `A` cycles ratio (free, as the picture, 1:1, 3:2, 4:3, 5:4, 16:9), Ctrl+drag a line along something that should be level, Enter applies, Esc cancels; Ctrl+Z undoes |
| | pitch, yaw, zoom | done (E7): a tipped sheet seen by a camera (`raster::perspective`), +-100 = 40 degrees, corners the tilt empties are transparent; crop afterwards |
| Heal | clone, heal, remove, patch, red-eye | done (E5): keys K J M P E or the strip in the Heal group; `nv_editor::retouch` |
| | skin smooth | done (E5): skin-tone mask, edge-preserving smoothing |
| Film | LUT (.cube, 3D and 1D) with strength | done (E4): buttons at the end of the Film group |
| | noise reduction (luma, colour), grain (amount, size, roughness, colour), film blur | done (E4) |
| Auto | option | stored in the recipe (`auto`), nothing computes it (as asked) |
| Tools | resize with chosen algorithm (Lanczos, bicubic, Mitchell, Catmull-Rom, bilinear, area, nearest), convert (JPEG / PNG / WebP lossless / BMP), colour reduce (median cut, dither, palette PNG), file reduce (target KB), watermark (text, 6 places), background remove (plain backgrounds) | done (E6): panel section "Export copy" (Ctrl+Shift+S saves the copy), and in NuGallery's Convert dialog for many pictures; `nv_editor::{resample, quantize, watermark, bgremove, export}` |
| Presets | save a look under a name, apply it to any picture | done (E3): panel section "Presets", `.nupreset` files in `%APPDATA%\NuViewer\presets` |
| Analysis | histogram (with clipped %), RGB parade, vector scope with the skin-tone line, false colour | done (`Y` cycles the scopes, `X` false colour; `nv_editor::scopes`) |
| View | fit, zoom to 16x, pan, 100% detail | done (wheel / `1` `2` `4` / `F`; double click; drag to pan); works on turned, flipped, cropped and healed pictures too (E7) |
| Compare | A / B | `C` toggles, `S` splits (drag the divider), both at fit and zoomed in: done |
| Save | overwrite / new file / pair file, with choices | done: the three ways, default in Settings; Settings > NuEditor also holds the format of a new file, JPEG quality, name suffix, EXIF on / off (E3) |
| Revert | pair file, backup, undo | done |
| Leave | Esc asks (burn in / new file / pair file) | done |

## 6. Decisions (your answers in bold)

1. **"shift" = four axes** (shadows and highlights, each green / magenta and blue / yellow), better than a two-axis split-tone for
   repairing a camera's colour. Done.
2. **Pair file like Lightroom / darktable, but not XML**: `IMG.jpg.nuedit`, plain text lines. `.xmp` is left to darktable.
3. **Default save is a setting, factory = pair file** (Settings > NuEditor). Done.
4. Editor is a separate program (`nueditor.exe`); the viewer's old mock-up is only used when it is not next to the viewer.
5. Proxy preview on the CPU instead of a GPU shader: one engine, 1-12 ms per change at 1080p; the GPU shows, zooms and later runs the heavy filters.
6. The gallery re-reads the folder when the editor hands back (a copy may have appeared, a picture may have changed).
7. Zoom / pan work on the uncut picture; a turn, flip or crop shows the fitted proxy only (a cut-out of a turned picture would need
   the turned picture at full size). Detail views cut from a pyramid (full, 1/2, 1/4 ...) so a zoom step costs about the same as a slider move.

## 7. Not done / not verified

* The installer and `tools\package.ps1` carry `nueditor.exe` now (unit test in a scratch registry; no real install was run); no Start-menu shortcut for it (it needs a picture, and opens from the viewer / gallery).
* The viewer's "copy image" and the gallery's file operations use the file as it is on disk, not the pair-file version.
* The gallery has no command to take back a burn-in; the viewer (Ctrl+Z) and the editor (Ctrl+Alt+Z) do, through the journal.
* Real mouse input, HiDPI above 100%, RAW pictures (they open as developed pixels), CMYK pictures and the F4 / menu entry of the
  gallery were not tried; the viewer -> editor -> viewer chain and all of the editor's own flows are covered by
  `tools\test\e2e-editor.ps1` (26 checks) and `tools\test\e2e-editor-link.ps1` (8 checks), messages only.

## 8. Milestones

| | What | State |
|---|---|---|
| E1 | Zoom / pan + detail render, A/B split, histogram + parade + vector scope + false colour | done |
| E2 | Crop frame, straighten line, colour-chart picker, curve editor, hue wheel UI | done |
| E3 | Worker-thread render with stale-drop, save options (quality / format / metadata / name), presets | done |
| E4 | Sharpen, clarity, bloom, noise reduction, grain, film blur, LUT (.cube) | done (`blur`, `spatial`, `lut`) |
| E5 | Heal / clone / remove / patch / red-eye / skin smooth | done (`retouch`, `app/heal.rs`) |
| E6 | Resize (nearest, bilinear, bicubic, Mitchell, Catmull-Rom, Lanczos, area) + convert / colour reduce / file-size target / watermark / background remove; batch from NuGallery | done (`resample`, `quantize`, `watermark`, `bgremove`, `export`) |
| E7 | Pair file shown in viewer / gallery, shared undo journal, zoom on turned pictures, pitch / yaw / zoom | done |

## 9. E2 notes

* `nv_editor::cropmath` (frame hit-testing, drag with ratio lock that keeps the opposite corner / the centre, `fit_ratio`, `level_angle`)
  and the hue-wheel engine are unit-tested; the mouse side is `nueditor/src/app/tools.rs`, covered by `tools	est\e2e-editor-tools.ps1`.
* A cropped picture is shown from a *re-made* screen-sized proxy (`App::ensure_geo`: turn + flip + crop applied to the full picture,
  then shrunk), so it stays sharp. Cost: ~100-200 ms when the crop or the angle changes, none per slider move. Dragging the
  Rotate slider while a crop exists therefore feels slower than without one.
* The white balance picker and zoom need the uncut picture (a turn / flip / crop changes which pixel is where).
* Crop frames are relative to the *turned* picture; retouch strokes (later) are relative to the unturned one.

## 10. E3 - E7 notes

**Presets.** `nv_editor::preset`: the look of a recipe (everything except turn, flips, crop, keystone and heal strokes, which belong
to one picture) as a `.nupreset` text file in `%APPDATA%\NuViewer\presets` (`NUEDITOR_PRESET_DIR` overrides). The panel section lists
them; a click applies one (Ctrl+Z undoes), right-click deletes (asks), "Save this look as a preset" opens the system Save dialog
(only the name you type is used). NuGallery's Convert dialog can apply a preset to many pictures.

**Save options** (Settings > NuEditor): format of a new file (same as the picture / JPEG / PNG / WebP lossless), JPEG quality,
name suffix, EXIF on / off, watermark text. They apply to "new file", to a burned-in picture (it keeps its format) and are the
starting values of the Export section. A new file is always checked for a free name (`_edit`, `_edit (2)` ...).

**Export copy** (panel section, Ctrl+Shift+S): format, JPEG quality, resize (percent / long edge / width / height) with the
algorithm, colour reduction (2-256, dithered or not; a PNG then becomes a palette PNG), a file-size target in KB (JPEG: quality goes
down first, then the size; PNG: fewer colours, then the size), background removal (tolerance), watermark (text from Settings, place,
size, opacity), keep EXIF. A click on a choice cycles it (right click goes back), the wheel works on sliders and choices.
`nv_editor::export::run` is the one pipeline: background removal -> resize -> watermark -> colour reduction -> encode.

**Heal tools** (keys K clone, J heal, M remove, P patch, E red-eye, `[` `]` brush size, Esc leaves): clone / heal: Ctrl+click the
place to copy from, then paint (the distance is fixed by the first stroke, like an "aligned" clone stamp); remove: paint over it;
patch: draw around the flaw, then drag the outlined area onto a good place and let go; red-eye: click each eye. Each stroke is one
undo step and is stored in the pair file (0..1 of the original picture, so it stays put when the crop or angle changes). The
picture is updated when a stroke is finished; the brush ring, the path, the clone source and the patch outlines show while it is made.
Heal needs Pitch / Yaw / Zoom at zero (the keystone has no inverse in the mapping).

**Background remove** works for plain or nearly plain backgrounds (studio backdrop, product on white, flat sky): the colours along
the border are the background, whatever of them touches the border becomes transparent with a soft edge. It is not a subject
detector; a busy background needs a mask drawn by hand.

**Pair file in the viewer and gallery.** `nv_core::pair`: when a picture has a `.nuedit` file (and Settings > NuEditor allows it) the
viewer decodes the picture, applies the recipe with the same engine and shows that; gallery thumbnails do the same on the thumbnail
(the pair file's time is part of the thumbnail cache key). A pair file for a picture of another size is ignored. Cost for a picture
without one: a single `metadata` call.

**Journal.** Every burn-in appends a line to `burn-in-journal.tsv` in the backup folder (time, picture, backup). The viewer's Ctrl+Z
(when its own undo list is empty) and the editor's Ctrl+Alt+Z bring the original back from it; backups still go after 7 days.

**Zoom on turned pictures.** The 100% view is cut from a pyramid made of the picture *as shown* (heal strokes, turn, flips, keystone,
crop applied once at full size, rebuilt when one of them changes). The white-balance picker and the heal tools map a click back to
the original through `engine::display_to_source` (tested against the real geometry).

**NuGallery Convert** now has the editor's tools: resample algorithm, "apply the edits made in NuEditor" (each picture's pair file),
a preset, colour reduction, file-size target, watermark text, background removal.

### Numbers (release, 1920x1080 proxy, this PC, machine busy)

Per-pixel stage 3.4 ms (tables) / 8.3 ms (general) / 13.5 ms (everything on). Neighbourhood filters: grain 7 ms, clarity 17 ms,
sharpen 27 ms, bloom 30 ms, film blur 32 ms, skin smooth 33 ms, noise reduction (luma + colour) 53 ms, **all of them at once
209 ms**. They run on the worker thread, so the window stays live and only the newest value is rendered; a few effects together
(the normal case) stay around 50-100 ms. Measured with `cargo test --release -p nv-editor -- --ignored --nocapture timing`.

### Tests

`cargo test -p nv-editor` (101 tests: engine, blur, spatial filters, LUT, retouch, resample, quantize, watermark, export, presets,
journal, mapping), `-p nv-core` (pair file display), `-p nueditor` (save options and the export pipeline on files).
End to end (messages only): `tools\test\e2e-editor.ps1`, `e2e-editor-link.ps1`, `e2e-editor-settings.ps1`,
`e2e-editor-tools.ps1`, `e2e-editor-more.ps1` (presets, clone + undo, remove, zoom on a turned picture, Pitch, Export section),
`e2e-editor-pair.ps1` (pair file in the viewer, setting off, other size, journal Ctrl+Z).

### Not done / not verified (E3 - E7)

* The system Save / Open dialogs (Save copy, Save preset, Choose LUT) and the delete-preset question are modal; they are covered by
  the code paths below them (`flow::save_copy`, `preset::save` ...) but not clicked through by a script.
* Heal and patch quality was judged on synthetic pictures and by the unit tests, not on real portraits; the remove tool fills smoothly
  with the surrounding grain added back and is not a content-aware fill.
* Real mouse, high-DPI, RAW and CMYK pictures were not tried in the new tools.
* Panel scrolling (wheel over the panel outside a slider) was checked only through the Export section.
