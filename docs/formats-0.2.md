# New formats, video colour, snapshots and quick convert (version 0.2.0)

## Video and sound

Video: mp4 m4v mov mkv avi wmv webm mpg mpeg mpe ts mts m2ts m2t 3gp 3g2 flv f4v ogv ogm vob asf dv rm rmvb divx m2v mod tod wtv (AVCHD = `.mts` / `.m2ts`).
Sound: wav mp3 flac aac m4a m4b ogg oga opus wma aif aiff aifc au snd pcm alac ape dsf dff dsd wv tta mka mpc mp2 amr ac3 dts caf weba.

* What Windows can play natively (Media Foundation) plays natively. The rest (`HELPER_EXTS` in `nv-codecs/src/media.rs`: ogg, opus, ape, dsd, flac-in-odd-containers, flv, vob, rm ...) and anything Windows fails on goes through **ffmpeg** (if installed): a playable copy (720p H.264 + AAC; a sound becomes AAC 256 k) is made in the background and kept in `%LOCALAPPDATA%\NuViewer\proxy` (cap 2 GB). A `.pcm` file has no header: it is read as 16-bit stereo 44.1 kHz.
* **Colour of the playable copy:** HDR10 / HLG video is tone-mapped to SDR (`zscale` + `tonemap=hable`; needs an ffmpeg with libzimg, which the "full" builds have), everything else is converted to BT.709 limited range with the source's own matrix and tagged BT.709. Without that the proxy of an HDR clip would look flat grey.
* **Step by frame:** `.` and `,` (and two buttons on the playback bar) show the next / previous frame *exactly* (the back step decodes forward from the key frame before it and shows the frame ahead of the one on screen; at the first frame it stays). A GIF / APNG / WebP steps one frame with the same keys.

## Snapshot (Ctrl+Shift+S, right-click > Save this frame as a picture)

Settings > Video snapshot: format (PNG 8 / PNG 16 / JPEG / WebP lossless / TIFF / BMP), quality, folder (empty = next to the video), name pattern (`{name} {time}`).

With ffmpeg the exact frame is decoded at **16 bits** (`scale=in_color_matrix=…:in_range=…` with the clip's own matrix and range, `rgb48`), and the colour work is done in `nv-core/src/videocolour.rs`:

| Source | What happens |
|---|---|
| SDR, "as it looks in the player" (default) | The numbers are kept (only matrix, range and primaries are handled: BT.601 / PAL / P3 / 2020 primaries are moved to sRGB's). |
| SDR, **Rec.709 display** (BT.1886, gamma 2.4) / **Rec.709-A** (QuickTime, gamma 1.961) / plain 2.2 | The numbers are turned into light with that curve, then encoded as sRGB. |
| **HDR10** (PQ) / **HLG**, 10 or 12 bit | Light is rebuilt in nits (PQ ST 2084; HLG with system gamma 1.2 at 1000 nits), scaled so 203 nits = SDR white, tone-mapped (Reinhard on luminance, or Hable), BT.2020 → BT.709, sRGB. Peak brightness of the master is a setting (1000 nits is the usual). |
| HDR **kept** (setting) | 16-bit PNG with the numbers as they are and a `cICP` chunk (BT.2020, PQ or HLG), for HDR-aware programs. |
| PNG 16 for SDR | 16-bit sRGB: a 10 / 12-bit video keeps its fine steps. |

Without ffmpeg the frame on screen is saved as the player shows it (8-bit; HDR video then looks flat) and the toast says so. "Rec.709-A" is taken to be QuickTime's tag (gamma 1.961); if you meant something else it is one number in `videocolour.rs`.
Unit-tested: PQ / HLG curves, the three SDR assumptions, HDR white and black, probe parsing, 16-bit PNG with its tag. `crates/nv-core/tests/video_colour.rs` (runs when ffmpeg is installed) makes real BT.601 / BT.709 clips and an HDR10 clip and checks the saved pixels.

## 3D

| Format | How |
|---|---|
| glTF / GLB, VRM, OBJ (+MTL), PLY, STL, FBX | as before |
| **3DS**, **3MF** (with build transforms and components), **AMF** (also zipped), **Collada DAE** (triangles, polylist, polygons; up axis; node transforms are not applied), **LWO** (LightWave 6+ and 5), **DXF** (3DFACE, SOLID, POLYLINE polyface and mesh, MESH entity; ASCII only), **USDA** (ASCII USD) and **USDZ** holding USDA, **Maya ASCII (.ma)** (polygon meshes; no transforms), **LAS** point clouds (up to 250 000 points as small solids, RGB or height colours; LAZ is refused) | read natively (`nv-codecs/src/model_more.rs`), unit-tested on hand-built files |
| **Unity package** | the biggest model inside (fbx, obj, dae, glTF, stl, ply, 3ds, lwo), else its biggest `preview.png` |
| **STEP / STP, IGES / IGS** | through FreeCAD (`FreeCADCmd.exe`) or assimp when one is installed; the result is cached in `%LOCALAPPDATA%\NuViewer\model-cache` |
| **Alembic (abc), binary USD / USDC / USDZ** | through Blender (`blender.exe -b`) or assimp |
| **DWG** | through the free ODA File Converter (DWG → DXF) |
| **3ds Max (max), Cinema 4D (c4d), Modo (lxo), ZBrush (ztl, zpr), Substance 3D Painter (spp), Marmoset (ms), Softimage (hrc), Unreal (uasset), Maya binary (mb), Fusion 360 (f3d)** | closed formats with no public specification: NuViewer shows the **preview picture stored inside the file** when there is one (found by its PNG / JPEG signature) and otherwise says plainly which program's export to use (glTF / FBX / OBJ). They are not 3D-viewable here. |

The converter paths (FreeCAD, assimp, Blender, ODA) and the real-file behaviour of Collada / 3MF / LWO / DXF / MA / USDA beyond the unit tests were **not exercised on real files from those programs** here.

## JSON and data files

json jsonc json5 geojson ndjson jsonl xml xsd xaml plist yaml yml toml ini cfg conf tsv nfo properties env: a dark page, line numbers, coloured keys / strings / numbers / words (JSON is re-indented when it has no comments; a JSON file is checked and the subtitle says "valid JSON" or the line and column of the error). Only the first 512 KB are read. They are always listed in folders.

## Quick convert

`Ctrl+Alt+P` / `Ctrl+Alt+J` in the viewer, "Convert to PNG / JPEG" in the right-click menus of NuViewer, NuGallery (any number of selected pictures) and NuEditor (the picture *with its edits*), and in **Explorer's own menu** (Settings > Language & Explorer adds / removes it; per user, no admin; `nuviewer.exe --to-png|--to-jpg "%1"`).
The copy goes next to the original with the same name (`name (2).png` if that exists, never an overwrite), keeps the file's date, and loses nothing the format can keep: PNG is lossless; JPEG is quality 100, 4:4:4, with the camera's EXIF (orientation set to upright). Colours: the embedded profile is converted to sRGB (the display profile is never baked in).
Explorer starts one process per selected file: the first one collects the others' names for 350 ms and converts the batch (`nv_core::quick::run_cli`). On Windows 11 the entries are in "Show more options" (a top-level entry needs a packaged COM extension). Explorer's classic verbs are limited to 15 selected files at a time; NuGallery has no limit.
