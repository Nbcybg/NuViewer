# Session 19 - fonts, archives, design files (version 0.4.0)

The user's list (2026-11-17): fonts with a preview sentence set in Settings (100 characters, default the lazy dog), more compressed formats
(cab tar gzip dmg tgz ...: check which exist), CD5, AFPhoto, KRA, PSP, XCF, AFDesign, CDR, XPS, U3D, MA, MB, VRML, XVL, DWF, DWG, DXF, USD, USDZ, GLTF, CSV, PAK;
and a question: use the paint-matcher app's colour tools in NuEditor?

## What already existed (no work needed)
tgz tar gz bz2 xz zst 7z rar (cards, opened as folders in NuGallery), afphoto afdesign (stored preview), ma mb (model / stored preview), dwg dxf (converter / built-in),
usd usdz gltf (built-in or converter), csv (text card).

## New, and how each is read
| Files | Reader | Source file |
|---|---|---|
| ttf otf ttc otc woff woff2 eot | skrifa reads the font, resvg's text engine (harfrust shaping) draws the specimen with a font list holding only that font; WOFF / WOFF2 / EOT are rebuilt to sfnt first | `fontview.rs`, `webfont.rs` |
| pfb pfa fon | card with what the header says (Windows no longer draws Type 1) | `fontview.rs` |
| cab dmg iso wim esd swm cpio arj lzh lha z taz lz lzma lz4 tbz tbz2 tb2 txz tlz tzst rpm deb ar (+ tar tgz gz xz zst 7z rar) | table of contents read natively, found by content not by extension; viewer = page of up to 70 entries, gallery = card | `archives.rs` |
| jar war ear apk aab xpi vsix nupkg whl ipa crx appx msix kmz pk3 pk4 zipx | zip reader, first picture inside | `docthumb.rs` lists |
| pak | Quake / Sin / Daikatana PACK, Chromium resource pack (largest PNG), Unreal pak footer | `archives.rs` |
| kra ora | zip: mergedimage.png | `designfmt.rs` |
| xcf | layers composited (u8 RLE / raw / zlib, 16 bit, float, masks, groups' visibility, 13 blend modes) | `designfmt.rs` |
| psp pspimage | the JPEG composite stored in the composite image bank (else any JPEG in the file) | `designfmt.rs` |
| cdr cdt | zip thumbnail (X4+) or RIFF DISP bitmap (older) | `designfmt.rs` |
| xps oxps dwfx | thumbnail / biggest picture, else page 1 drawn by Windows' XPS rasterizer (CLSID 503E79BF-..., `XpsRasterService.dll`) | `designfmt.rs`, `xpsdraw.rs` |
| dwf | picture inside the zip part, else card | `designfmt.rs` |
| u3d | card (meshes, faces, nodes); geometry is arithmetic-coded and not drawn | `designfmt.rs` |
| xvl xv3 xv0 cd5 | cards (closed formats), or a PNG / JPEG found stored inside | `designfmt.rs` |
| wrl vrml wrz | VRML 97 and 1.0 parser -> mesh | `vrml.rs` |

## The sentence
`Prefs.font_sample` (settings.ini `font_sample`), cleaned by `fontview::clean_sample` (single spaces, 100 characters, default when empty). `Settings::load()` hands it to
`fontview::set_sample`, so every program that loads the settings has it; the Settings window sets it again on OK. A font thumbnail's cache key includes `fontview::signature()`.

## Checked on real files
Fonts: Arial, Segoe UI, Thai fonts from F:\FONT (JS Mookda: Thai shown), WOFF and WOFF2 made with fontTools from them (glyf-transformed WOFF2 draws the same).
Archives: a 934 MB CAB with 1598 files (header read only), a Joliet ISO with a Japanese disc name, a 132 MB DMG (UDIF v4, zlib, 8 partitions), Chromium resource packs
(af.pak list, resources.pak -> its largest PNG), tar / tar.gz / tar.bz2 / tar.xz / 7z / wim made with 7-Zip.
Made for the tests (no such files on this PC): xcf (raw + RLE), kra, ora, cdr (zip and RIFF DISP), psp, oxps / xps (rasterized by Windows), wrl, u3d, cab, cpio, ar, PACK pak.
NOT verified against real files: xcf from GIMP itself (version 11 pointers, 16-bit / float tiles), psp (block layout from the format description), arj, lzh, rar listing, rpm, deb control,
Unreal pak, DWF.

## The paint-matcher question (colour tools for NuEditor)
The app (`paint-matcher (1).zip`: index.html, app.js, 5 small paint JSON files) has two things for colour work, both done in HSL on the sRGB numbers:
1. **Hue vs Hue / Hue vs Sat / Hue vs Lum curves**: free points on a 360 degree circle, Catmull-Rom through them, baked to one 360-entry table each.
2. **Color Warper**: a 12 spoke x 5 ring mesh on the hue / saturation wheel; dragging a node moves the colours around it (bilinear), like DaVinci Resolve's warper.
NuEditor already has RGB curves and an 8-band hue wheel (shift up to 30 degrees, saturation, brightness per band, neighbours shared linearly).
Recommendation: take both, as one new section "Hue curves & warper": the curves are strictly more precise than the 8 bands (arbitrary points, any amount), the warper is the
faster way to move one colour to another (a sky from teal to blue). Cost is small: the engine already has a per-pixel hue path, a 360-entry table + a 12x5 offset mesh are
two more stages, the pair file gets one more text line each. Do it in OKLCH rather than HSL (the JS version shifts brightness unevenly between hues). The rest of the app
(matching a picked colour to model paints by Delta E in Lab, palette extraction, mosaic, recipe PDF) is a different tool: it would fit NuGallery's Tools menu or NuImporter, not the editor.

## Part 2 (version 0.4.1): the user's next list
Palette and a colour picker that copies the value (without slowing the programs), the paint-matcher colour tools in NuEditor, right-click / text-box completeness, no close button on the
gallery toolbar, a borderless toggle in every program, and "optimize and benchmark that nothing got slower".

- **Picker / palette**: `nv_core::colours` (formats, OKLab k-means palette, `PickData::load`); viewer `nuviewer/src/pick.rs`, gallery `nugallery/src/pick.rs` (preview picture), editor
  `nueditor/src/app/colourtool.rs` (the edited proxy, kept only while a tool is on). Zero cost off: one bool test per frame; a worker thread reads the picture on demand.
- **Hue curves + warper**: `nv_editor::huecurves` (OKLCH, engine stage after the hue wheel, pair-file lines `huecurve` / `warp`), UI = section 9 "Hue curves" of the panel (`tools.rs`).
  Near the grey axis the effect fades out (smoothstep over chroma 0.03) so the result is continuous; a 41^3 lattice makes it fast.
- **Text boxes**: `nv_core::textinput` (was the gallery's `input.rs`) now also drives the viewer's rename box; Form edit boxes get Ctrl+A; editor number entry got clipboard + menu;
  gallery rename / property boxes got the right-click menu. Audit: address / search (already), tree and item menus (already), viewer menu, editor menu and panel rows (already).
- **Borderless**: `nv_win::toggle_borderless` (style without WS_CAPTION | WS_THICKFRAME, same client rectangle; `WM_NCHITTEST` gives edges for resizing and Alt + drag for moving).
- **Benchmarks**: `tools/bench/viewer-speed.ps1` (old vs new exe, first picture, next picture), `crates/nv-editor/examples/bench_render.rs`, the older `coldstart.ps1` / `gallery-thumbs.ps1`.
  Results are in CHANGELOG 0.4.1. e2e: `tools/test/e2e-session19.ps1` (14 checks).
- Not done: the importer window has no borderless toggle (it is a dialog); Alt + drag is the only way to move a borderless window; the palette in the gallery uses the preview picture's
  panel, so it needs the right panel open; the viewer picker is for still pictures (not models, 360 view, turned pictures, spreads, video).
