# Changelog

Notable changes to NuViewer, newest first. Details and measurements for each round of work are in `HANDOFF.md`.

### Added
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
