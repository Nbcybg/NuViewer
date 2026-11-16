# Session 17 (2026-11-17): the four points from the five-viewer benchmark

Source: `docs/benchmark-compare-2026-11-17.md`. Harness: `tools/bench/compare/` (set `NV_EXE` to time another build than the installed
one; NuViewer must be told to open fullscreen through `%APPDATA%\NuViewer\window.ini` - `fullscreen = 1` - the `--fullscreen` flag is not
enough on this PC; restore the file afterwards, a backup is made by hand).

## 1. File formats that did not open

| format | what is shown now | how |
|---|---|---|
| `.sai` (PaintTool SAI 1) | the whole canvas rebuilt from the raster layers (opacity, visibility, folders, clipping, multiply / screen / overlay / add / subtract); the stored thumbnail when the layers give nothing; thumbnails in the gallery use the stored thumbnail | `sai.rs`: the encrypted page file system (cipher + key table from the MIT-licensed libsai, `docs/third-party/libsai-LICENSE.txt`), `sai_keys.rs` |
| `.clip` | the canvas preview PNG stored in the file | `artfmt.rs`: `CSFCHUNK` chunk walk, then a small SQLite reader (b-tree + overflow pages) for the `CanvasPreview` table |
| `.oci` (openCanvas) | a card with the picture's size | the pixel data is a closed compressed format (no public description exists); the `OPIM` header gives width and height |
| `.duf` (Daz) | the `name.duf.png` thumbnail next to it, else a card from the (gzipped) JSON: author, date, library counts, scene nodes | `artfmt.rs` |
| `.max` (3ds Max) | the preview stored in the OLE summary information (128 x 48 px, enlarged smoothly) | `ole.rs` (compound-file reader), `gpufit::upscale_smooth` |
| `.3mf` | the model: slicer files (Bambu Studio / Orca / Prusa) keep the meshes in other model files (`p:path`), which are loaded now; the plate picture when the meshes cannot be drawn | `model_more.rs::parse_3mf` |
| `.mid .midi .rmi .kar` | a piano roll (every channel its own colour, drums along the bottom); **plays** through a small built-in synthesizer that renders a WAV once into `%LOCALAPPDATA%\NuViewer\midi` | `midi.rs`, `nuviewer/src/media.rs::media_source` |
| `.lip` | the biggest PNG / JPEG found inside, else a card | `artfmt.rs::lip` - **I do not know what `.lip` is** (see below) |

Also fixed on the way: `render_card` crashed (access violation) when a card line was empty.

Not verified: a real `.clip` file (none on this PC: a synthetic one with a 760 KB preview spread over SQLite overflow pages decodes), audible MIDI
(the tool session has no sound device), `.sai` files with masks / linework layers (not drawn), SAI2 (`.sai2`, another format).

## 2. Damaged JPEG and first open

- The 2081 ms in the benchmark was **not decoding**: the picture was on screen at ~365 ms, but the installed build then spent ~1.7 s
  uploading the 30-megapixel texture and the GPU frame that finally replaced the CPU first frame looked slightly different, so the screen
  recorder counted that as "not complete yet".
- The first frame is now made the way the GPU makes it (`nv-codecs/src/gpufit.rs`: mip chain of 2x2 averages in linear light, then
  Catmull-Rom, turned while sampling) and the GPU draws that very bitmap with point sampling (`Renderer::draw_thumb_exact`, `display.rs`
  `Proxy`) until the view changes. Measured by comparing full-resolution screenshots with and without the proxy
  (`NUVIEWER_NO_PROXY=1`): identical except for the disk read-out badge. So nothing changes on screen when the GPU takes over, and going away
  and coming back shows the same picture.
- Pictures of 24 megapixels or more (JPEG): the first frame comes from a DCT-scaled decode (`jpeg::decode_scaled_for_window`, 136 ms against
  293 ms for the damaged 30 MP file), in parallel with the full decode, and a file without end-of-image marker gets one for that decode.
- The disk read-out badge (setting `disk_monitor`) is not drawn while the first picture is the proxy: its first sample arrives after
  ~1.7 s and was the only change left on screen.

## 3. PSD

- Photoshop's rule for transparency: the first extra channel of the merged picture is transparency **only when the layer count in the layer
  section is negative**. The decoder treated every 4th channel as alpha, so a PSD whose 4th channel is a saved selection (the 293 MB
  `Demon01.psd`) came out with a transparent background and lost the castle behind the figure. ImageGlass and IrfanView showed it right.
- The merged picture is read straight from the file with the layers skipped by seeking (`psdraw::decode_path`), RLE rows
  and the RGB interleave run on all cores, and the `psd` crate (which panicked on these files, then fell back) is only used for a file whose
  merged picture is blank (saved without "Maximize Compatibility").
- Decode 203 -> 42 ms (293 MB), 34 -> 11 ms (32 MB).

## 4. Rapid next / back

- Pictures that were shown stay on the GPU while they take less than half of the budget (`App::seen`, newest first), so going back is a texture
  swap; the prefetch window is 4 ahead / 3 behind instead of 3 / 2.
- Rapid back x5 149 -> 41 ms, rapid next x5 84 -> 50 ms (ImageGlass 134 / 102 in the same run).

## Measured (this PC, same harness, new build vs the numbers in the benchmark document)

| | before | now | IrfanView |
|---|---|---|---|
| JPG pano 6 MB | 322 | ~200 | 299-341 |
| JPG 18 MP | 319 | ~200 | 398-424 |
| JPG damaged | 2081 | ~220 | 425-445 |
| PNG small / 14 MB | 117 / 346 | 105 / 180 | 221 / 421 |
| TGA | 298 | 97-233 | 231-235 |
| PSD small / 32 MB / 293 MB | 109 / 317 / 344 | 88 / 100 / 277 | 192 / 207 / 282 |
| mix first open | 304 | ~190 | 294 |
| paced next / back | 38 / 53 | 38 / 43 | |
| rapid next x5 / back x5 | 84 / 149 | 50 / 41 | |

Slightly slower than before: PDF and AI first open (+30-90 ms; the first frame is 17-25 ms dearer to make and the document decode itself varies).

## Open / to confirm

- `.lip`: the request said ".clip .lip". If `.lip` is a different program's file (Live2D? something else) tell me which one and send a sample.
- `.oci`: needs an exported PNG from openCanvas, or the format's description.
- The installed build is still the old one: run `tools\package.ps1` / the setup to get this.
