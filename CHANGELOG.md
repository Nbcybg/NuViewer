# Changelog

Notable changes to NuViewer, newest first. Details and measurements for each round of work are in `HANDOFF.md`.

## 0.3.0 - 2026-11-17

Details, measurements and what was not verified: `docs/session17.md`.

### Fixed
- **PSD:** a fourth channel is transparency only when the layer count is negative (Photoshop's rule). A 4th channel that is a saved selection made the
  background transparent (the castle behind the figure in a 293 MB file). The merged picture is read without the layers (4-5x faster) and the `psd` crate is only a fallback.
- **First open:** the first picture is made on the CPU exactly the way the GPU makes it and is kept until the view changes, so the GPU taking over (for a
  30-megapixel JPEG that took ~1.7 s) changes nothing on screen: damaged 30 MP JPEG 2081 -> ~220 ms, 6-18 MP JPEG ~320 -> ~200 ms, PSD / TGA / PNG ~100-180 ms.
  JPEGs of 24 MP or more are painted from a DCT-scaled decode. A card line that was empty crashed the card renderer.
- **Rapid next / back:** pictures already seen stay on the GPU, the prefetch window is wider: rapid back x5 149 -> 41 ms.

### Added
- **Logo and icons** for every program (`assets/`, embedded by `tools/winicon.rs`; window, taskbar, tray, shortcuts, file types).
- **Formats:** PaintTool SAI (canvas from the raster layers), Clip Studio Paint `.clip` (canvas preview), openCanvas `.oci` (card), Daz `.duf` (thumbnail or card),
  3ds Max `.max` (stored preview), slicer `.3mf` (meshes in other model files), MIDI `.mid .midi .rmi .kar` (piano roll, plays through a built-in synthesizer), `.lip` (picture inside, else a card).

### Also in 0.3.0 (work of 2026-11-16, `docs/session16.md`)

Details, what was tested and what was not: `docs/session16.md`.

### Fixed
- **A JPEG that other programs open could be refused** ("jpeg: Exhausted data in the image", NV-3000): the decoder ran in its *strict* mode by default. It is lenient now
  (progressive files cut off in their last scans, extra bytes between headers ...), then a repaired copy is tried (metadata blocks with a wrong length cut out, the missing
  end marker added), then Windows' own JPEG codec.
- **Paged documents (PDF ...) no longer lose you to the next file:** the wheel, the arrows and Space stop at the first / last page (a toast says so); only Ctrl+Left / Ctrl+Right leave the document.

### Added
- **Formats:** web pages (`.html .htm .xhtml .mhtml .mht`: drawn by Microsoft Edge / Chrome in the background without network access, kept as a picture in
  `%LOCALAPPDATA%\NuViewer\pages`; a text card when no browser is found), Flash movies `.swf` (the largest bitmap inside, or a card with version / stage size / frame rate / frames)
  and Flash projects `.fla` (a library picture, or a card with the stage and library facts). `.flv`, `.f4v` and `.glb` were already supported; a GLB with Draco or meshopt
  compression now says so instead of showing nothing. All three new formats are in Settings > file types.
- **Ping-pong play** (button on the playback bar for animations and sequences, `Shift+K`, Settings > Screen & play): forward, then backward; with loop off it makes one trip.
- **RAW look toggle** (`Shift+R`, View menus, right-click, NuQuick's tray menu, NuImporter's check box): the camera's own look (its embedded JPEG, exactly as the camera
  processed it) or the sensor data without camera processing. One shared setting (`raw_mode`). NuImporter converts RAW files by the same choice.
- **NuEditor and RAW:** the picture follows the same toggle; the panel shows what the camera recorded (as-shot white balance with its colour temperature, ISO, exposure
  compensation), read from the file. Edits are kept in the pair file (the RAW is never rewritten) and keep applying when you flip the look; saving a JPEG / PNG burns them in.

## 0.2.0 - 2026-11-16

Everything in this version is described, with what was and was not verified, in `docs/formats-0.2.md`, `docs/importer.md` and
`docs/localization.md`.

### Added
- **Video:** one frame forward / back (`.` and `,` and two new buttons on the playback bar; exact, also stepping back; GIF / APNG / WebP step too).
  **Save this frame** (Ctrl+Shift+S, right-click): Settings > Video snapshot picks format (PNG 8 / 16 bit, JPEG, WebP, TIFF, BMP), folder and name, and how colours
  are treated: SDR as shown / Rec.709 (BT.1886) / QuickTime Rec.709-A / 2.2; **HDR10 and HLG** (10 / 12 bit) tone-mapped to sRGB (Reinhard or Hable) or **kept as a
  16-bit PNG tagged BT.2020 + PQ**. The frame is decoded by ffmpeg at 16 bits with the clip's own matrix and range.
- **Formats:** video mp4 m4v mov mkv avi wmv webm mpg mpeg ts mts m2ts 3gp flv f4v ogv vob asf dv rm rmvb ... (anything Windows can't play is converted by ffmpeg, HDR
  tone-mapped, colours tagged BT.709); sound mp3 ogg opus flac alac aac m4a wma aiff aifc au pcm ape dsf dff wv tta mka ...; 3D **3DS 3MF AMF DAE LWO DXF LAS USDA/USDZ Maya ASCII
  Unity packages**, STEP / IGES / Alembic / binary USD / DWG through FreeCAD, Blender, assimp or the ODA converter when installed, and the picture stored inside closed scene
  files (max, c4d, lxo, ztl, spp, ms, hrc, uasset, ...); **JSON, XML, YAML, TOML, INI, TSV ...** as a coloured page (JSON is re-indented and validated).
- **Quick convert, one click, no questions:** "Convert to PNG / JPEG" next to the file (PNG lossless; JPEG quality 100, 4:4:4, EXIF kept) in the right-click menus of NuViewer,
  NuGallery (many files) and NuEditor (with the edits), keys Ctrl+Alt+P / Ctrl+Alt+J, and **in Explorer's own right-click menu** (Settings > Language & Explorer).
- **NuImporter is real:** scanner (Windows' scan window), screenshot (region or whole screen; NuQuick's Ctrl+Print Screen / Ctrl+Alt+Print Screen start it from anywhere), clipboard,
  camera (take a picture over a cable, or watch the folder a wired / Wi-Fi camera writes into), the pictures inside a PowerPoint / Word / zip / epub / ISO, a folder. After import:
  save at once, open in NuEditor first (save or drop when it is closed), or put on the clipboard; PNG, JPEG, WebP, BMP, TIFF, QOI, TGA or "as it was".
- **NuEditor:** zooms out below the fit (down to 5%); click the value of any slider (and of the export numbers) and **type a number**, beyond the slider's range
  (up to +-1 000 000; every slider survives that: tested); save formats BMP, TIFF, TGA, QOI added; Ctrl+C copies the edited picture.
- **Back (<) and close (x)** at the ends of the menu bar of NuViewer, NuGallery and NuEditor (`<` goes to the previous program / the gallery, `x` closes everything).
- **Settings** is a sidebar with four categories and two-tone icons (Keyline Icons, MIT, `docs/third-party`) instead of one crowded row of tabs; new pages: Video snapshot, Import,
  Language & Explorer.
- **Credits** (F1): centred, made by Noobcyborg studio, website and support links you can click.
- **Languages:** the text of the programs is in `crates/nv-text/lang/en.txt` (keys + 700 phrases found in the code); a translation is a file `lang/<code>.txt`
  (`th.txt` Thai starter included); Settings > Language & Explorer. See `docs/localization.md`.

### Changed
- Version 0.2.0. The NuImporter mock-up (nothing was copied) is gone: its card / folder window became the real importer.
- Data files (JSON ...) are always listed in folders.
- The file-type chooser (`--associate`, installer) lists the new video, sound and 3D formats (all off by default; JSON and other data files are never registered).
- Tests: `assoc` group ranges and the editor's clamping tests follow the new behaviour (typed values beyond a slider are kept, also in pair files).

## 0.1.0 and what was added before 0.2.0 (sessions 10-14)

### Added
- **Viewer:** Western / Japanese reading is one switch (Ctrl+B, View menu, right-click menu); in Japanese the Left arrow turns forward. Double click opens
  the gallery. **Dim** (Shift+D, how much in Settings). **Canvas:** plain colour (presets, grey slider, hex), the picture itself blurred, or a picture file.
  **360 view** (Shift+P): drag looks around, wheel zooms; a 2:1 picture offers it.
- **One playback bar** for video, sound, animation, image sequence and slideshow: play, step, seek track with a picture of the spot under the pointer,
  speed 1x / 2x / 4x / 0.5x, loop, volume. **Video and sound play** (Media Foundation + waveOut, loaded only when a media file is opened); a WAV is shown
  as its waveform; R3D / MXF / BRAW are recognised (proxy file when there is one); video Windows can't decode (10-bit camera files) gets a picture and a
  cached playable copy from ffmpeg if it is installed. See `docs/media.md`.
- **Formats:** VRM avatars (as glTF), Blender `.blend` (its stored preview picture), video, sound.
- **NuQuick:** resize the preview from its edges and corners; videos and sounds preview too.
- **NuGallery:** make a **PDF** from the selected pictures / folders; **compress** (zip, 7z with 7-Zip, tar.gz) and **unpack**; an **archive opens as a folder**
  (zip / cbz natively, 7z / rar / tar with 7-Zip or Windows' tar), password asked for when needed. Settings: "Screen && play" and "Archives && PDF" tabs.
- **NuQuick:** the preview can be dragged and remembers where it was left; folders and other files get an info card (icon / thumbnail, kind, size,
  dates, where) instead of nothing.
- **NuGallery:** "Group image sequences" is in the right-click menu too; the menu bar now pushes the toolbar and the side panels down while it shows
  (it used to cover their buttons). **NuViewer:** the file-name bar answers a strip of 1% of the window height along the top; the menu bar only the very edge.
- **NuQuick** (`nuquick.exe`): Space-bar preview for Explorer and the desktop. Background agent (~2 MB, 0 CPU idle; hooks exist only while
  Explorer is in front), picture pre-decoded when selected, ~9 ms from Space to pixels, never takes the focus (arrows keep moving the
  selection), Space / Esc closes, Enter opens NuViewer, tray menu (Pause / Start with Windows / Settings / Quit). **Settings > Space preview** switches it on / off and sets Start with Windows. See `docs/nuquick.md`.
- **Menu bars** in NuViewer, NuGallery and NuEditor: the pointer on the top edge, or Alt / F10, shows File / Edit / View / ... ; Alt+F, Alt+E,
  Alt+V ... open a menu (Left / Right / Enter / Esc work inside the bar). Same entries as the right-click menus.
- **Ctrl + / Ctrl - / Ctrl+0** zoom the picture in the viewer and the editor and change the number of columns in the gallery.
- **NuEditor you can find your way around:** right-click menu, F11 / Alt+Enter window mode, a tool strip (Crop, White picker, Retouch,
  Fit, 100%, zoom, Split, Graph, Panel), a navigator to pan a zoomed picture, **histogram / RGB parade / vector scope (skin line) on by
  default** at the top of the panel, "Crop..." and "Crop to the picture" in Composition, and a view of the whole picture with its outline
  and your crop frame while you turn it or change Pitch / Yaw. F1 help page.
- **NuViewer:** right-click > Edit in NuEditor, Edit with... (your programs, any program, set up the list); delete asks first
  (Settings > Viewer to turn off) and the Edit menu names what Ctrl+Z would undo.
- **NuGallery:** Ctrl+Z undoes delete / move / rename / copy; delete asks first; the wheel scrolls (not next file); the folder tree has a
  scroll bar and shows drive names ("Photos (D:)"); panels leave the moment the pointer does; **preview controls** (wheel zoom, drag pan,
  double click, sharper decode, GIF / WebP / APNG and image-sequence playback with a bar, drag-to-turn 3D); **image sequences can be grouped**
  (Ctrl+G, View menu) into one tile with a badge.
- **Credits** at the bottom of every help page.
- **Faster first start of NuGallery / NuEditor after an update:** Windows Defender scans a never-seen exe for 0.4-0.75 s; each program now reads its
  siblings once in the background, the gallery builds its folder tree and drive labels off the UI thread, and the graph NuEditor shows is remembered
  (`editor_graph`). New setting `confirm_delete`.
- **NuImporter (mock-up)** from the gallery's Tools menu: pick a card / folder, see what would be imported where, nothing is copied yet.

- "Copy image" (gallery) and "Copy picture" (viewer) were the same thing under two names: both say **Copy picture** now.
- **Window state follows between the viewer and the gallery** (`window.ini` next to `settings.ini`): a window in one means a window in the other, in the same place; fullscreen likewise. Keys pressed while the viewer is still starting (Alt+Enter, Enter, arrows) are kept and replayed instead of lost. Letting go of Alt no longer enters menu mode and swallows the next key. Switching between a window and fullscreen no longer shows the old frame stretched for a moment (`DXGI_SCALING_NONE`, and a frame of the new size is drawn at once).
- **NuGallery follow-ups:** folder tiles show the first picture inside as a cover; a scroll bar; type a letter to jump to the
  first name starting with it (the old letter shortcuts moved to Ctrl+R shuffle, Ctrl+J sort, Ctrl+1/2/3 view, F8 slideshow);
  drag files out to Explorer / other programs and drop files in (WM_DROPFILES; the viewer opens a dropped picture); panels
  slide out the moment the pointer touches the edge; closing the gallery closes its viewer, and closing a viewer whose gallery is
  gone ends the program instead of starting the gallery again. Windows now open centred on the screen and Alt+Enter returns to
  that window (it used to come back at Windows' cascading position).
- **NuGallery, the gallery as a program of its own** (`nugallery.exe`, next to `nuviewer.exe`). Starting `nuviewer.exe` with no
  picture opens it; Space / double click / middle click opens a picture in NuViewer, Esc there comes back to the gallery on the
  last picture seen, `G` in the viewer goes the other way. Folders and files as thumbnails (with names), a details list or a
  list; the folder tree, folder data and size on the left edge, preview / name / size / histogram / editable properties
  (title, author, tags, comment, copyright, star rating) / EXIF on the right edge, the toolbar (next / previous, back /
  forward / up, address bar, sort, show only, view, search, tools, exit) on the top edge, the Windows taskbar on the bottom
  edge. Right-click menu: open, slideshow, quick edit, edit with…, show file location, refresh, shuffle, select, copy image /
  file / path, cut, paste, copy to…, move to…, print, new folder, rename, delete, compress (several files), quick convert.
  **Batch** for several files: rename by pattern, convert (format, quality, size, folder), change time stamps (set, shift or
  from the EXIF date), remove metadata (lossless for JPEG / PNG / WebP). Wheel = previous / next file, Ctrl+wheel = thumbnail
  size, `1`-`5` size levels, `` ` `` resets, Space = view, Enter = next file. `tools\bench\gallery-thumbs.ps1` times how fast
  the thumbnails of a folder appear (cold and warm cache).
- **CMYK done properly.** Decoders (JPEG incl. YCCK, PSD / PSB, TIFF, JPEG 2000) hand over the raw inks; one colour engine
  converts them through the embedded profile, or through the profile chosen in Settings > CMYK & RAW (Coated FOGRA39, built
  in, by default; SWOP; any `.icc`) and the viewer says when it assumed one. Adobe's inverted CMYK / YCCK is handled; PSD /
  TIFF profiles are read (RGB TIFFs now use theirs too). Verified against ColorSync's FOGRA39 values and LittleCMS
  (`docs/colour-cmyk.md`). **Rendering intent** (relative colorimetric default, perceptual, saturation, absolute) and
  **black point compensation** are settings. PDF pages still go through Windows' own CMYK conversion.
- **RAW is developed from the sensor data** (full size by default, half size or the embedded preview as options): black /
  white level, as-shot white balance and the camera's colour matrix only; the automatic exposure and highlight roll-off are
  gone. Own multi-threaded demosaic (Bayer and X-Trans), ~270-350 ms for 18-26 MP (`docs/raw-development.md`).
- **Thumbnails of documents:** zip / cbz / epub (first page / cover), docx / xlsx / pptx / odt / ods / odp (their saved
  thumbnail, else the biggest picture), Affinity (the embedded preview), txt / md / log / csv (a card of the first lines), and
  a card with the extension, name and size for anything without a picture. One ZIP reader, reading only the table of contents
  and the one entry needed. Setting "List documents" (off by default).
- **Decode statistics:** `nv_codecs::stats` (per format and per stage: count, average, min, max, MP/s), written to
  `%LOCALAPPDATA%\NuViewer\decode-stats.txt`, the stage times of the picture on screen in the info panel (`I`), and the
  `bench_decode` / `bench_colour` / `rawinfo` / `rawcheck` / `doccards` dev tools.
- **Order of pictures.** One list per folder, shared by next / previous, the gallery, the film strip and the slideshow.
  Sort by name (natural, the default), date modified, size or type, ascending or descending (`J`, `Shift+J`, right-click >
  Sort by, Settings). **Shuffle** (`R`) puts the whole folder in a random order that stays until `R` is pressed again.
  The sort is remembered; shuffle is not.
- **Film strip** on the right edge (`L` keeps it open): thumbnails of the folder, click to jump, wheel to scroll, header to
  sort and shuffle. Uses the gallery's thumbnail workers and cache.
- **Read mode** (`B`) for books and comics: two pages side by side with no gap, zoomable as one picture. Pairs start at a
  configurable page (default 2, cover alone; `Shift+B` sets it to the page on screen), wide double-page scans stand alone,
  `Ctrl+B` switches to right-to-left.
- **Error codes** `NV-xxxx` (29 of them, listed in `docs/error-codes.md`): an on-screen panel for a picture that cannot be
  opened, codes on failed file operations, GPU errors, settings errors and the crash dialog.
- **Formats:** Photoshop PSB, JPEG XL (including animations), JPEG 2000 (`.jp2 .j2k .j2c .jpf .jpx .jpc`), FLIF, PCX,
  DICOM (`.dcm`), Apple ICNS, cursors (`.cur`), bare DIB bitmaps, Radiance `.rgbe`, `.hif` (through Windows), Sigma X3F,
  and Flash `.swf` (shows the largest bitmap inside the movie). `.ecw` files are listed and explain how to convert them.
  Viewer-controls bar gained film strip and read mode buttons; the file-type chooser lists the new formats.

### Fixed
- **Alt+Enter:** DXGI was also handling the key and fought the program's own fullscreen toggle (the window jumped, or came back as a captioned window the size of the screen); it is told to leave the key alone. Leaving fullscreen also lost the window's visible style bit on the first trip. Covered by `tools	est\e2e-gallery.ps1` with real `WM_SYSKEYDOWN` messages, four trips.
- **Alt+Tab looked like a hang.** Settings and every other dialog were separate top-level windows, so switching back landed
  on the main window while it waited for the dialog. Dialogs are now owned by the viewer window (one Alt+Tab entry, the
  waiting dialog comes forward, the viewer is disabled meanwhile) and a message box raised from Settings belongs to Settings.
- "File types…" in the context menu opens in the same process instead of starting a second one.

### Changed
- RAW thumbnails (gallery, film strip) still come from the embedded preview (setting `raw_thumbs` develops them too): development
  reads the whole file, which is slow on slow disks.
- PNG decoding skips the CRC / Adler checks and builds RGBA in one pass (~7% faster on 60 real PNGs); JPEG thumbnails of big
  pictures are decoded with the DCT scaled down (15-25% faster).
- `decode_file_first_frame` is the whole first frame (clipboard); the gallery uses `decode_for_thumbnail`.
- `R` now shuffles the folder's order (it used to shuffle only the slideshow).
- Settings > File types: the Select all / Select none / Apply buttons moved onto the OK / Cancel row to make room for the
  longer format list.
- A file with a known extension but unrecognisable contents is reported as "not a valid picture" instead of "unsupported
  format".

### Not verified on real files
FLIF, DICOM, PSB and X3F decoding (no sample files or encoders on the development PC; synthetic unit tests only).

## Earlier

Earlier work (colour management, CMYK / greyscale JPEG fixes, zoom keys, left-edge controls, rotation with save, 3D styling,
installer, file-type chooser, image sequences, PDF / SVG / EPS / 3D formats, loading panel, RAW previews and development,
single instance, crash safety) is described session by session in `HANDOFF.md`.
