# Changelog

Notable changes to NuViewer, newest first. Details and measurements for each round of work are in `HANDOFF.md`.

### Added
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
- **Alt+Tab looked like a hang.** Settings and every other dialog were separate top-level windows, so switching back landed
  on the main window while it waited for the dialog. Dialogs are now owned by the viewer window (one Alt+Tab entry, the
  waiting dialog comes forward, the viewer is disabled meanwhile) and a message box raised from Settings belongs to Settings.
- "File types…" in the context menu opens in the same process instead of starting a second one.

### Changed
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
