# Add-ons: what a format needs besides NuViewer, and where to get it

NuViewer reads almost everything by itself. A few formats need a program or a codec that is **not part of NuViewer** (it has no network code, so it never downloads anything: you install the add-on yourself, from its maker). The same information is a column of the format list, [`formats.csv`](formats.csv): *Add-on needed (and where to get it)*. An empty cell there means the format needs nothing.

**Optional** means the file still opens without the add-on (as an information card, or the picture stored inside it); the add-on makes it open fully.

| Add-on | Get it from | Needed for | Where NuViewer looks |
|---|---|---|---|
| **ffmpeg** | <https://ffmpeg.org/download.html> (a Windows build, for example the ones linked there from gyan.dev or BtbN) | Required: FLV, F4V, OGV, OGM, VOB, DV, RM / RMVB, DivX, M2V, MOD / TOD, WTV and audio such as OGG, OPUS, APE, WV, DSD, AC3, DTS, AMR, CAF, MKA (the "ffmpeg copy" rows). Optional: MP4, MOV, MKV, WebM, MXF and 10-bit camera files that Windows cannot play itself; the exact frame for *Snapshot*; WebM-with-alpha export (needs libvpx-vp9) | `ffmpeg.exe` on `PATH`, or chosen in **Settings > Archives & tools** |
| **7-Zip** | <https://www.7-zip.org> | 7Z, RAR, CB7, CBR and AES-encrypted zip opened as a folder in NuGallery (their information card needs nothing); also used for several disc-image types | `Program Files\7-Zip\7z.exe`, `PeaZip`, or `PATH` (Settings > Archives & tools) |
| **AV1 Video Extension** | Microsoft Store (free) | AVIF on a PC that does not have the codec yet | Windows (WIC) |
| **HEVC Video Extensions** | Microsoft Store | HEIC / HEIF / HIF on a PC that does not have the codec yet | Windows (WIC) |
| **Live2D Cubism Core** (`Live2DCubismCore.dll`, 64-bit) | Live2D, *Cubism SDK for Native*: <https://www.live2d.com/en/sdk/download/native/> (you accept its licence; NuViewer may not ship it) | Live2D models (`.model3.json` / `.moc3`) | `NUVIEWER_CUBISM_CORE`, next to `nuviewer.exe`, or `%APPDATA%\NuViewer\live2d\`; **Settings > Archives & tools > Add-on: Live2D models** shows whether it was found. See [`live2d.md`](live2d.md) |
| **Ghostscript** | <https://ghostscript.com/releases/gsdnld.html> | Optional: EPS and PS drawn in full (without it the preview picture stored inside the file is shown) | `PATH` / `Program Files\gs` |
| **FreeCAD**, **Blender** or **assimp** (any one) | <https://www.freecad.org/downloads.php> · <https://www.blender.org/download/> · <https://github.com/assimp/assimp/releases> | STEP / STP, IGES / IGS, Alembic (ABC), USD, binary USDC / USDZ: converted to a model NuViewer can show. Without one the file is listed and the message names the tools | `PATH` or `Program Files` |
| **ODA File Converter** (or Blender / FreeCAD) | <https://www.opendesign.com/guestfiles/oda_file_converter> | AutoCAD DWG | `PATH` or `Program Files` |
| **ECW codec** | its maker (Hexagon / ERDAS ECW plug-in for Windows) | ECW satellite and map images | Windows (WIC) |
| **Microsoft Edge** or Google Chrome | comes with Windows (Edge) | HTML / HTM / XHTML / MHTML pages drawn as a picture (headless, offline); without a browser a text card is shown | installed browser |
| a **proxy file** with the same name | made by the camera maker's tool (RED, Blackmagic, ...) | R3D, BRAW (and MXF) play through the proxy | next to the video |

Everything else, including Spine, **Unity asset bundles**, fonts, archives (zip, tar, cab, iso ...), RAW, PSD / XCF / ORA layers and the formats of the drawing programs, is read by NuViewer itself.

Plug-ins ([`plugins.md`](plugins.md)) are a different thing: they add formats and commands and are small folders you drop in; they are not needed for any format above.
