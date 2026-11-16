# NuViewer

A very fast, very light image viewer for Windows. Written in Rust on Win32 + Direct3D 11.

- Opens in about 60 ms for typical photos (about 125 ms for a 12 MP phone photo) and flips between images in a frame
  (neighbours are decoded ahead of time). EXIF rotation is done by the GPU, so rotated photos cost nothing extra.
- Shrinking to fit the screen stays sharp (mipmaps + bicubic resampling in linear light), unlike viewers that
  just bilinear-scale.
- A real gallery with cached thumbnails, EXIF panel, slideshow, and basic file management.
- Small (the whole app is one exe), no runtime. Comes with a per-user installer (no administrator rights).

## Formats

- **Photos:** JPEG / JPE, PNG/APNG (also optimised PNGs from OptiPNG and friends: they are ordinary PNGs), GIF (animated), WebP (incl. animated), PSD and **PSB** (Photoshop large documents; the merged picture, 8/16 bit, RGB / CMYK / greyscale / indexed / Lab), **JPEG XL** (`.jxl`, animated too), **JPEG 2000** (`.jp2` `.j2k` `.j2c` `.jpf` `.jpx` `.jpc`), FLIF, PCX, DICOM (`.dcm`: uncompressed / RLE / JPEG / JPEG 2000 data, window centre and width applied), camera RAW (**developed from the sensor data**: black / white level, as-shot white balance and the camera's colour matrix, nothing added; the embedded preview is an option and the fallback), and, through Windows' own codecs: AVIF, HEIC (needs the Microsoft *HEVC Video
  Extensions*; `.hif` too), TIFF, BMP, JPEG XR. **Icons and bitmaps:** ICO, CUR (cursors), ICNS (Apple icons), DIB (Windows bitmaps without a file header). Camera RAW covers CR2 CR3 CRW NEF NRW PEF RAF RWL MRW ORF SRW X3F ARW SR2 SRF RW2 DNG and more. **Limits:** `.ecw` (ERDAS' compression is proprietary: NuViewer lists the files and says to convert them, e.g. `gdal_translate in.ecw out.jp2`; it does open them if you have an ECW codec for Windows installed), playing Flash `.swf` (it is a program: NuViewer shows the largest bitmap stored inside the movie, and says so when there is none). FLIF / JPEG XL / DICOM / PSB files that use rarely seen features are refused with an error code. Colour managed: a picture's embedded ICC profile (e.g. Display P3) is converted to the output profile you choose in
  Settings > Colour (sRGB, this PC's display profile from Windows, or any `.icc` / `.icm` file), and you decide what a
  JPEG / PNG / WebP *without* a profile is (sRGB, Display P3, Adobe RGB or a file). Switchable. **CMYK** pictures (JPEG / YCCK,
  PSD / PSB, TIFF, JPEG 2000) are converted through their own profile, or, when they have none, through the one you choose
  (Coated FOGRA39, SWOP or a file) and the viewer tells you it assumed it; rendering intent and black point compensation are
  settings too (`docs/colour-cmyk.md`). PDF pages are drawn by Windows and do not go through these profiles.
- **Game / render formats:** TGA (also `.icb`, `.vda`, `.vst`; RLE, colour-mapped, 16/24/32 bit), DDS (uncompressed and
  block-compressed BC1-BC7), PNM (PPM, PGM, PBM, PAM), Radiance HDR / RGBE (`.hdr` `.rgbe`) and OpenEXR (tone mapped to a normal picture), QOI.
- **Documents:** PDF, one page at a time (rendered by Windows' own PDF engine, so text and vector graphics stay sharp);
  Adobe Illustrator `.ai` saved with PDF compatibility (the default).
- **Vector art:** SVG / SVGZ (resvg, with system fonts), Windows metafiles EMF / WMF, EPS and old PostScript-style `.ai`
  (with Ghostscript if it is installed, otherwise the preview stored inside the file).
- **3D models:** STL (binary and ASCII), OBJ, PLY, glTF / GLB, binary FBX. They are drawn live on the GPU: drag to
  turn, wheel to zoom, `F` resets the camera, `U` swaps which axis is "up". Gallery thumbnails are rendered too.
  **Textures** (Settings > Viewer > 3D models, on by default): OBJ + MTL (`map_Kd`, `Kd`, one part per material), glTF / GLB
  (base colour texture, embedded or external, vertex colours), PLY (vertex colours, texture coordinates, `comment
  TextureFile`), binary FBX (diffuse texture, embedded or external). Texture files are looked for next to the model
  and in `textures/`; a missing picture just leaves the plain surface. Not supported: animation, point clouds,
  ASCII FBX, normal / roughness maps.

**Loading big pictures.** While the picture you asked for is still loading, the old one is dimmed and a panel shows what is
happening: reading from disk (bytes, MB/s), decoding, or copying to the graphics card, and the drive's overall read /
write speed and busy percentage, so you can tell when the disk is the bottleneck. The same disk readout can stay on
screen (`D`, or Settings > Viewer > Loading). The window title carries the same numbers, and the top bar shows how long
the file took to read. Huge pictures go to the GPU in small bands between window messages, so the window never freezes.

EXIF orientation is applied. Images beyond what the GPU can show (more than 16384 px a side or ~120 megapixels) are
scaled down to fit and say so; anything over 250 megapixels is refused.

**PDF pages.** `←/→`, Space, Backspace, PgUp/PgDn and the mouse wheel turn pages; past the last (or before the first)
page they carry on into the neighbouring file. `Home` / `End` jump to the first / last page, `Ctrl+←/→` skip the
pages and go to the previous / next file. The gallery shows page 1. Pages are rendered at about 1.5 x the screen's
long side, so zooming in far gets soft (re-rendering at the zoom level is not done yet).

## Order, film strip and read mode

**What next / previous walk.** One list per folder, **by name** ("natural": `img2` before `img10`, ignoring case) unless you change it.
`J` steps through name → date modified → size → type, `Shift+J` reverses, `R` shuffles. The gallery, the film strip, the
slideshow and the arrow keys all use that same list, so what the film strip shows is exactly what the arrow keys will visit.
The sort is remembered (Settings > Mouse & keyboard, or `sort =` / `sort_desc =` in `settings.ini`); shuffle never is. RAW and JPG of one
shot are always separate entries.

**Film strip** (pointer on the right edge, or `L`, or the viewer-controls button): thumbnails come from the gallery's cache, only the
rows on screen are made. The current picture is outlined; click a thumbnail to jump, wheel over the strip scrolls it.

**Read mode** (`B`) shows two pages side by side as one picture, so zoom, hold-to-zoom and panning work on the pair.
How pages pair up: from the *start page* on they go two at a time (2+3, 4+5 ...), pages before it (the cover) stand alone, and a
wide scan (more than 1.2 × as wide as tall, i.e. a double-page spread already) stands alone and the pairing carries on after it.
*Setting the start page:* the default is page 2 (cover alone), changed in Settings; if a book comes out wrong, go to the page that
should be the **first of a pair** and press `Shift+B`: pairs now start there (remembered for that folder during the run).
`Ctrl+B` swaps the sides for right-to-left books. Next / previous move a whole spread; the slideshow too.

## Menu bar, zoom keys, credits (all three programs)

Put the pointer on the **top edge** of the window, or tap **Alt** (or **F10**): a menu bar (File, Edit, View, ... Help) appears; **Alt+F**,
**Alt+E**, **Alt+V** ... open a menu, Left / Right / Enter / Esc work inside the bar. **Ctrl +** and **Ctrl -** zoom (in the gallery:
fewer / more columns), **Ctrl+0** resets. Deleting asks first (Settings > Viewer to turn that off) and **Ctrl+Z** brings back a deleted,
moved, renamed or copied file in the viewer and in the gallery. **F1** is the help page of each program; the credits (author and the
open-source libraries NuViewer stands on) are at the bottom of it.

## Video, sound, 360, dim and the canvas

- **Video and sound** play in the viewer (Space / P, Shift+Left / Right seek 5 s). The bar at the bottom edge is the same for video, sound, GIF / APNG / WebP,
  image sequences and slideshows: play, step, a seek track that shows a picture of the spot under the pointer, speed (1x 2x 4x 0.5x), loop (Shift+L),
  sound (Shift+M) and volume. A WAV is shown as its waveform. Video Windows has no decoder for (10-bit 4:2:2 camera files, MXF) is played through a copy that
  ffmpeg makes in the background, if ffmpeg is installed (Settings > Archives && PDF). R3D / MXF / BRAW play through a proxy file with the same name when one exists.
- **360 view:** Shift+P on a picture twice as wide as it is high (drag looks around, wheel zooms).
- **Dim:** Shift+D darkens the screen (how much: Settings > Screen && play). **Canvas:** a colour, the picture itself blurred, or a picture file of your own.
- **Reading style:** Ctrl+B switches Western (left to right) and Japanese (right to left) reading in read mode.

## NuQuick (Space-bar preview in Explorer)

`nuquick.exe` sits in the background (tray icon, ~2 MB, no CPU) and shows the picture selected in Explorer or on the desktop when you press
**Space**, like Quick Look on a Mac. It never takes the focus: the arrow keys keep moving Explorer's selection and the preview follows;
**Space** or **Esc** closes it, **Enter** opens the picture in NuViewer. Space is only taken while Explorer is in front and the file list has the
focus (typing a file name still works), and the keyboard hook exists only during that time. Switch it on and off in **Settings > Space preview** (also Start with Windows); the tray icon's menu has Pause, Start with Windows, Settings and Quit.
Details, measurements and what is not verified yet: `docs/nuquick.md`.

## NuGallery (the gallery)

`nugallery.exe` is the folder browser. Start `nuviewer.exe` without a picture (or `nugallery.exe [folder or file]`) and it
opens; it remembers the last folder, view and pinned panels. It starts fullscreen like the viewer (`--windowed` opts out).

- **Edges** (the pointer rests there for a moment): **left** the folder tree plus folder data and folder size, **right** the
  details of the focused file (preview, name, size, histogram, editable properties, EXIF), **top** the toolbar (previous /
  next file, back, forward, up, address bar, sort, show only…, thumbnails / details / list, search, tools, exit), **bottom**
  the Windows taskbar. Each panel has a *Pin* button (or `Ctrl+B` / `Ctrl+I` / `Ctrl+T`) to keep it open and docked.
- **Mouse:** wheel = previous / next file (Shift+wheel scrolls), Ctrl+wheel = thumbnail size (columns), click selects (Ctrl
  adds, Shift ranges, dragging on empty space draws a selection box), double click or middle click views the file (a folder is
  entered), right click = menu. Alt+F4 exits.
- **Keys:** Space = view in NuViewer, Enter = next file, `1`-`5` = thumbnail size, `` ` `` = reset the size, **a letter jumps to the first name that starts with it**,
  Ctrl+1 / 2 / 3 = thumbnails / details / list, Ctrl+J sort, Ctrl+R shuffle, F8 slideshow, F2 rename, Del Recycle Bin, Shift+Del delete for good,
  Ctrl+N new folder, Ctrl+C / X / V, Ctrl+Shift+C copy path, F6 / F7 move / copy to, F5 refresh, Ctrl+F search, Ctrl+L address
  bar, Backspace or Alt+Up = up, Alt+Left / Right = back / forward, F1 = the whole list.
- **Menu:** open, slideshow, quick edit (rotate, flip), edit with…, show file location, refresh, shuffle, select (item, all,
  none), copy image / file / path, cut, paste, copy to, move to, print, new folder, rename, delete, **compress** (when several
  files are selected), quick convert (JPEG, PNG, WebP, BMP, resized JPEG). With several files selected a **Batch** submenu (also
  under Tools) opens: rename (pattern with `{name}`, `{n:3}`, `{date}`, `{taken}`…, find / replace, case), convert, change
  time stamp, remove metadata. Properties are written through Windows' property system, so JPEG and TIFF keep them in the
  file itself.
- **Folders show a cover**: the first picture inside is on the folder's tile. The items have a **scroll bar** (drag it, or click
  the track). **Drag and drop:** files can be dragged out of the gallery to Explorer or any program that takes files, and files
  dropped on the gallery (from Explorer, or another program) are copied or moved into the folder under the pointer or the
  current folder (moved within a drive, copied between drives; Ctrl forces a copy, Shift a move). Dropping a picture on the
  viewer opens it.
- **Closing:** closing the gallery closes the viewer it opened; closing a viewer that was opened from the gallery returns to the
  gallery; with no gallery running, closing the viewer ends the program.
- The viewer follows the gallery's order (sort is shared; shuffle is passed along), and closing it returns to the gallery with
  the last picture seen selected.

### Archives and PDF

Space or a double click on an archive opens it as a folder (zip / cbz natively; 7z, rar, tar and AES zips through 7-Zip, else Windows' tar); a password is asked
for when needed and the pictures are unpacked into a cache (`%LOCALAPPDATA%/NuViewer/arc`, 1 GB, cleaned by itself). Right-click > Compress (format and level in
Settings), Unpack here (Ctrl+Shift+E), **Make a PDF** (Ctrl+Shift+P): the selected pictures, the selected folders, or this folder; JPEGs go in untouched.

## NuEditor (the editor)

`nueditor.exe` is the third program: **E** in the viewer or **F4** in the gallery opens it on the picture, **Esc** puts you back where you
were. It works on a window-sized copy of the picture, so every slider is instant whatever the picture's size, and renders on its own
thread so the window never waits. Details, speed numbers and status: `docs/nueditor.md`.

* **Panel** (right, Tab hides it): White balance (temperature, tint, four-axis shift, `W` picker), Light, Colour, Detail (sharpen,
  clarity, vignette, bloom), Composition (rotate, Pitch / Yaw / Zoom, `,` `.` turn 45, `H` `V` flip, `R` crop and straighten with
  Ctrl+drag), Heal, Film (LUT, noise reduction, grain, film blur), Curves, Hue wheel, Presets, Export copy. Drag or wheel a slider,
  right-click or double-click puts it back, right-click a header resets the group, `0` resets all, `C` shows the original, `S` splits
  original | edited.
* **Heal tools**: `K` clone and `J` heal (Ctrl+click the place to copy from, then paint), `M` remove, `P` patch (outline the flaw,
  drag it onto a good place), `E` red-eye, `[` `]` brush size.
* **Look at it**: wheel / `1` `2` `4` / `F` zoom and pan (also on turned or cropped pictures), `Y` histogram -> RGB parade -> vector
  scope with the skin line, `X` false colour.
* **Keeping the edits**: Esc asks (pair file / new file / burn in / discard); Settings > NuEditor chooses the default and the format,
  JPEG quality, name suffix and EXIF of copies. The pair file `IMG.jpg.nuedit` is plain text next to the picture; the viewer and the
  gallery show the picture with it applied. `Ctrl+S` saves the default way, `Ctrl+Shift+S` saves a copy with the *Export copy* options
  (resize with a chosen algorithm, format, colours, file size, watermark, plain-background removal). `Ctrl+Z` / `Ctrl+Y` undo and
  redo (200 steps); `Ctrl+Alt+Z` brings back the original of a burned-in picture (the viewer's `Ctrl+Z` does too).
* **Presets**: *Save this look as a preset* in the Presets section; a click applies one. NuGallery's *Convert* can apply a preset or the
  edits in the pair files to many pictures at once.

## Keys and mouse (the viewer)

Press **F1** (or **H**) in the app for this list; these are the defaults and **Settings** changes them.

| Key / mouse | Action |
|---|---|
| ← → · Space · Backspace · PgUp PgDn · Home End | previous / next / first / last (pages first in a document) |
| `Ctrl+←` `Ctrl+→` · `Ctrl+Home` `Ctrl+End` | previous / next / first / last *file* |
| Mouse wheel | previous / next picture, page or frame (in the gallery: scroll; in 3D: zoom) |
| Hold left button · drag while held | zoom to 100% (200% if the picture is smaller than half the screen) and pan; release returns to the fitted view |
| `Ctrl` + drag | zoom into the dragged area (Esc cancels) |
| Click on a zoomed picture | back to the fitted view (dragging pans instead) |
| `Ctrl` + wheel | zoom around the pointer |
| `F` · `A` | fit · 100%. These are the two *base views*: pressing the one that is on switches to the other |
| `1` `2` `3` `4` `5` · `` ` `` | 1x … 5x of the base view (fit or 100%, whichever is on) · reset zoom |
| `]` `[` · `Shift+]` `Shift+[` | rotate +45° / −45° · +90° / −90° |
| `Ctrl+S` | save the rotation (JPEG / PNG are rewritten in place, other formats get a PNG copy; the original is backed up and `Ctrl+Z` restores it) |
| Pointer on the left edge · `V` | viewer controls: a vertical bar (next, previous, fit, 100%, fullscreen, NuEditor, ±45°, slideshow, image sequences, gallery, settings, help, close). It closes when the pointer leaves it; `V` keeps it open |
| `E` | NuEditor (the picture editor, see below; Esc there brings you back to this picture) |
| 3D: drag · `Shift`+drag · wheel | turn the model · slide it (pan) · zoom; `F` resets all three |
| 3D: `T` `N` `W` `Shift+S` | texture · normal map · wireframe · shadow on/off |
| 3D: `Shift+T` `O` `X` `Y` `Z` | next shading · perspective / orthographic · turn the model 90° about X / Y / Z (Shift = back) |
| Right click | context menu (every action with its shortcut, *Open with* your programs, Settings) |
| Pointer on the top edge | info bar: name, size and megapixels, file size, position, speed, zoom |
| Pointer on the bottom screen edge | shows the Windows taskbar while in fullscreen |
| `G` · Tab · Enter | gallery (arrows select, wheel scrolls, Ctrl+wheel changes size) |
| `F11` · `Alt+Enter` | windowed ↔ fullscreen (pictures open fullscreen; the pointer hides when idle) |
| `I` | file info and EXIF |
| `S` · `+` `-` | slideshow · faster / slower |
| `J` · `Shift+J` · `R` | sort by name → date → size → type · ascending / descending · **shuffle** (random order; `R` again restores the sort). Next / previous, the gallery, the film strip and the slideshow all follow this order |
| Pointer on the right edge · `L` | **film strip**: a vertical column of thumbnails of the folder. Click one to jump to it, wheel over it scrolls it, its header sorts (Name / Date / Size / Type, ▲▼) and shuffles. It closes when the pointer leaves; `L` keeps it open |
| `B` · `Shift+B` · `Ctrl+B` | **read mode** (books, comics): two pages side by side with no gap · pairs start at the page on screen · left-to-right / right-to-left (manga) |
| `Q` | fold numbered pictures into image sequences / show every picture (gallery) |
| `P` · `C` | play / pause a sequence or animation · keep the playback bar on screen |
| `F2` | rename (the name part is pre-selected) |
| `Del` · `Shift+Del` | Recycle Bin · delete permanently |
| `F5` · `F6` / `M` | copy to… · move to… |
| `Ctrl+Z` | undo rename / move / copy / delete |
| `Ctrl+C` · `Ctrl+Shift+C` · `Ctrl+E` | copy image + file · copy path · show in Explorer |
| `Ctrl+,` · `F9` | Settings |
| Middle mouse button | close the viewer (not in the gallery) |
| `Esc` | close panel / stop slideshow / back from the gallery / quit |

## Settings

`Ctrl+,` (or right click > Settings, or `nuviewer.exe --settings`) opens the Settings window; OK applies and saves at once.

- **Colour:** colour management on/off, the output profile (sRGB / the Windows display profile / a profile file) and the
  assumed profile of pictures that carry none. Saved rotations and the clipboard always stay sRGB.
- **CMYK & RAW:** what a CMYK picture without a profile is taken to be (Coated FOGRA39 / SWOP / a file), the rendering intent
  (relative colorimetric by default, perceptual, saturation, absolute) and black point compensation; and how camera RAW
  is shown: developed full size (default), developed half size, or the camera's embedded preview, and whether gallery
  thumbnails of RAW files are developed too (slow: whole files are read). See `docs/raw-development.md`.
- **Viewer:** background colour, open fullscreen or windowed, what holding the left button does
  (automatic 100% / 200% for small pictures, always 100 / 200 / 400%, off) and what counts as "small", image sequences
  (fold, minimum length, playback speed), keep the playback bar visible, always show the disk read / write speed, and
  load the textures of 3D models, the left-edge viewer controls on/off, and what happens to a rotated picture when you
  move on (ask every time, or save automatically), and **List documents**: zip / cbz, docx / xlsx / pptx / odt, txt and
  Affinity files appear in folders and the gallery with a preview picture (the first page, the Office thumbnail, the
  first lines of the text) or a card with name and size.
- **File types:** the format check-box list (same as `--associate`), applied with one button.
- **External programs:** a list of programs the current picture can be sent to (*Open with* in the right-click menu).
  The command line uses `{file}` for the path, e.g. `"C:\Program Files\Adobe\Adobe Photoshop\Photoshop.exe" "{file}"`.
- **3D:** shading (combined, diffuse, cartoon, normals, unlit), texture / normal map / wireframe / floor shadow, the light
  (from the viewer, or fixed in the scene; angle and height), perspective or orthographic, which axis is up and a
  90° turn per axis for models that arrive face-down. The T / N / W / Shift+S / O / X / Y / Z keys change the same values.
- **Mouse and keyboard:** wheel action (pages / zoom / nothing), the top info bar,
  the taskbar reveal, and every keyboard shortcut (several keys per action, conflicts are refused, Reset restores
  the default). Also here: the sort order (name / date / size / type, descending), the film strip on the right
  edge, and read mode (the page its first pair starts at, right-to-left).

All dialogs (Settings, the file-type chooser, questions) belong to the viewer window: Alt+Tab shows one NuViewer entry and
switching back lands on the dialog that is waiting, not on a window that looks frozen. Errors carry a code
(`NV-xxxx`, see `docs/error-codes.md`).

Everything is stored in `%APPDATA%\NuViewer\settings.ini`, a plain `key = value` file you can also edit by hand
(unknown or invalid lines are ignored). The environment variable `NUVIEWER_SETTINGS` points to another file.

### Image sequences and playback

A run of files named `name_0001.png, name_0002.png, …` (same name, same extension, numbers one apart, at least the
configured length) can be folded into one gallery tile with a frame count (`Q`, or Settings). Opening it shows the
first frame; **P** plays it at the configured frame rate, `+`/`-` change the rate. RAW and JPG of the same shot are never
folded together (a run only holds files with the same extension), and folding is **off by default** because camera
files (`DSC_0001.jpg, DSC_0002.jpg …`) look exactly like a sequence.

The **playback bar** at the bottom of the viewer (near the bottom edge, or always) controls whatever plays over time:
a slideshow, an animated GIF/APNG/WebP (play/pause, frame stepping, seek, speed) or a sequence (play/pause, frame stepping,
seek, frames per second). It is built so video and music can use the same bar later (play/pause, seek, time, speed,
volume); there is no video or audio decoder yet.

- **0.3.1:** more clipart and legacy formats (PCD, DCX, CLP, CIN, DPX, PDN, WAL, WAD, CPT, PS, HP-GL, CGM, flat DXF); the full list of what opens, what NuEditor can edit and each format's name is in `docs/formats.csv`.

## New in 0.3.0

Details: `docs/session16.md`, `docs/session17.md`, `CHANGELOG.md`.

- **A logo and icons:** every program has its own icon now (the same glyph, one colour each: NuViewer blue, NuGallery teal, NuEditor orange, NuQuick violet, NuImporter green), in
  the exe, the window, the taskbar, shortcuts and the tray. Sources in `assets/` (`tools/winicon.rs` embeds them without rc.exe).
- **Faster first picture:** the first picture is painted on the CPU exactly as the GPU would draw it and stays until you zoom or move on, so nothing flickers when the GPU takes over.
  A damaged 30 MP JPEG opens in ~0.2 s (was 2 s); PSD, PNG, TGA, JPEG open in 0.1-0.2 s. Rapid next / back is 2-3x faster (pictures you have seen stay ready).
- **Formats:** PaintTool SAI, Clip Studio `.clip`, openCanvas `.oci` (size card), Daz `.duf`, 3ds Max `.max` (stored preview), slicer `.3mf`, MIDI (piano roll, plays through a built-in
  synthesizer), HTML pages, Flash `.swf` / `.fla`, damaged JPEGs. PSD / PSB transparency follows Photoshop's rule (fixes lost backgrounds).
- **Also:** ping-pong play, the RAW look toggle (Shift+R), paged documents stop at their last page.

## New in 0.2.0

The details, limits and what was not verified are in `docs/formats-0.2.md`, `docs/importer.md` and `docs/localization.md`.

- **Video:** `.` / `,` (and two bar buttons) step one frame forward / back, exactly. **Ctrl+Shift+S** saves the frame as a picture (Settings > Video snapshot: PNG 8 / 16 bit,
  JPEG, WebP, TIFF, BMP; folder; name). Colours are handled: Rec.709 / Rec.709-A / sRGB-as-shown for SDR; **HDR10 / HLG, 10 or 12 bit,** tone-mapped to sRGB or kept as a
  16-bit PNG tagged BT.2020 + PQ. Needs ffmpeg for the exact frame (otherwise the frame on screen is saved, and it says so).
- **Formats:** video mp4 webm mov mkv avi mpeg mpg flv AVCHD (mts, m2ts) vob ... and sound mp3 ogg opus flac alac aac m4a wma aiff au pcm ape dsd (dsf, dff) ...
  (Windows plays what it can; ffmpeg makes a playable, colour-correct copy of the rest). 3D: **3DS 3MF AMF DAE LWO DXF LAS USDA / USDZ Maya ASCII Unity packages** natively;
  STEP / IGES / Alembic / binary USD / DWG through FreeCAD, Blender, assimp or the ODA converter when installed; 3ds Max, Cinema 4D, Modo, ZBrush, Substance Painter, Marmoset,
  Softimage, Unreal files are closed formats: their stored preview picture is shown, or a message says what to export. **JSON, XML, YAML, TOML, INI, TSV ...** open as a coloured page.
- **Convert with one click:** "Convert to PNG / JPEG" in the right-click menu of NuViewer, NuGallery, NuEditor and **Explorer** (add it in Settings > Language & Explorer);
  next to the file, same name, nothing lost. Keys Ctrl+Alt+P / Ctrl+Alt+J in the viewer.
- **NuImporter:** scanner, screenshot (Ctrl+Print Screen / Ctrl+Alt+Print Screen from anywhere, through NuQuick), clipboard, a camera on a cable or writing into a watched folder
  (tether), pictures inside PowerPoint / Word / zip / ISO files, a folder; then save at once, edit first, or put on the clipboard.
- **NuEditor:** zoom out below the fit; click a value and type a number (beyond the slider's range); BMP / TIFF / TGA / QOI save formats; Ctrl+C.
- **`<` and `x`** at the ends of every menu bar: back to the previous program (the gallery) and close.
- **Settings** has a sidebar of categories with icons; the **credits** (F1) are centred with clickable website and support links; the **text is in files** (`crates/nv-text/lang`)
  so a language is one file (Thai starter included).

## Using it

```
nuviewer.exe <image-or-folder> [--gallery] [--new-window] [--windowed] [--actual-size]
nuviewer.exe --settings       # the Settings window
nuviewer.exe --associate      # tick which formats Windows should open with NuViewer (check-box list)
nuviewer.exe --register       # register all formats at once (current user only)
nuviewer.exe --unregister     # remove the registration again
```

Pictures open borderless fullscreen from the very first frame (no windowed frame first); `--windowed` opens a normal
window instead (`--fullscreen` is still accepted and means the default). `--actual-size` shows the first image at 100%
instead of fitted, handy for timing and pixel peeping.

Flags can go anywhere on the command line. **One window:** if NuViewer is already running, a second launch hands
its file/folder to the existing window (which comes to the front) and exits within a few milliseconds; a launch
with no arguments just brings the window forward. `--new-window` opts out. If the running window can't take the
request (it is stuck in a dialog, say) the new launch opens its own window after about three seconds.

Windows doesn't let a program make itself the default viewer. `--associate` (and the installer) therefore register
the formats you tick and then open Settings > Apps > Default apps > NuViewer, where each type has a "Set default"
button; the message afterwards says how many types already open with NuViewer and how many still wait for you there.
The same list is in the Start menu as *NuViewer file types*.

Thumbnails are cached in `%LOCALAPPDATA%\NuViewer\thumbs` (trimmed to 512 MB, least recently used first).
If something goes wrong the details are written to `%LOCALAPPDATA%\NuViewer\crash.log`.

## Building

Needs a recent stable Rust (MSVC toolchain) on Windows.

```
cargo build --release
cargo test
```

The exes are `target\release\nuviewer.exe` and `target\release\nugallery.exe` (keep them in one folder). If the drive holding the project is short on space, point cargo elsewhere
with `CARGO_TARGET_DIR`.

`tools\package.ps1` builds a release and produces

- `dist\NuViewer-<version>-setup.exe`: installer and uninstaller in one file (see below),
- `dist\NuViewer-<version>-win64.zip`: portable copy (both exes, this file, and `.cmd` helpers for the file types).

### Installer

Double-click `NuViewer-<version>-setup.exe`: choose the folder (default `%LOCALAPPDATA%\Programs\NuViewer`), tick the
image formats and the Start menu / desktop shortcuts, press Install. Everything is per user (HKCU), no UAC prompt.
Removal is Settings > Apps > Installed apps > NuViewer (or `uninstall.exe` in the install folder). Silent use:

```
setup.exe /S [/D=<folder>] [/formats=all|none|default|jpg,png,cr2] [/nostartmenu] [/desktop] [/launch] [/log=<file>]
uninstall.exe /uninstall /S
```

Tests: `cargo test` per crate, plus `tools\test\e2e.ps1`, which drives the real installer, the format chooser and the
middle-click close against a scratch registry key and temp folders (your own file associations are never touched).
`tools\test\e2e-mouse.ps1` checks default fullscreen, wheel navigation, hold-to-zoom, area zoom and the context menu by
sending messages to the viewer window and reading its pixels back, so it never moves your real mouse (add
`-RealInput` to also test the taskbar reveal at the bottom edge; it skips itself if another program holds the focus).

## Layout

```
crates/nv-core     folder listing, decode pool + prefetch, thumbnails, gallery layout, file operations
crates/nv-text     the text of the programs (en.txt, th.txt) and `t()` / `tr()`: one file per language
crates/nv-editor   NuEditor's engine, the formats it saves, quick convert's encoders
crates/nv-media    video and sound playback (Media Foundation + waveOut), frame stepping
crates/nv-codecs   all decoders, EXIF/ICC handling, resizing, metadata
crates/nv-render   Direct3D 11 renderer (textures, mips, bicubic shader, overlays)
crates/nv-win      Win32: window, input, text rasterising, shell integration, registry, form toolkit + file-type chooser, crash handling
nuviewer           the viewer (start-up, one picture at a time, HUD, film strip, editing)
nugallery          the gallery program (grid / details / list, panels, menus, file operations, batch)
nueditor           the editor (recipe engine, panel, retouching, import mode)
nuimporter         scanner, screenshot, clipboard, camera, pictures inside files
nuquick            Space-bar preview for Explorer (resident agent + borderless preview window) and the screenshot hot keys
setup              installer / uninstaller (carries a compressed nuviewer.exe)
```

Dev tools: `cargo run --release -p nv-codecs --example probe -- <files>` decodes files and prints timings and
pixel statistics (it never prints file names); `gen_test_anim` and `gen_big` write synthetic test images.
