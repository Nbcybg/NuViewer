# Session 16 (2026-11-16): the user's five points

## 1. A JPEG that every other program opens: "jpeg: Exhausted data in the image" (NV-3000)

**Cause.** `zune-jpeg`'s `DecoderOptions::default()` is *strict* mode. Strict mode refuses a progressive file whose last scans
are missing, extra bytes between headers, a metadata block that runs past the end ... all of which libjpeg, Windows, browsers
and FastStone shrug off. (The file in the report, `20231230_101344.jpg`, was not on this PC, so the exact flaw is not known;
a progressive JPEG cut off half way gives exactly that message with the old code.)

**Fix** (`crates/nv-codecs/src/jpeg.rs`):
1. the decoder runs with `set_strict_mode(false)` (also for CMYK),
2. if that still fails: `repair()` makes a copy (APPn / COM segments whose length does not land on a marker are cut out, the
   end-of-image marker is added when the file stops inside the picture data) and decodes that,
3. then Windows' WIC JPEG codec (`wic::decode_bytes`), orientation from our own EXIF reader,
4. only then the first error is reported.

Tests: truncated baseline / progressive files, a bogus APP1 length (`cut_off_files_still_open`,
`metadata_length_past_the_end_is_cut_out`) and, on the command line, a PIL-made progressive file cut at 50% (was NV-3000, now
decodes), a bogus APP1 in a 136 KB file (via `repair`).
Not verified: the user's actual file.

## 2. New formats: SWF, FLA, FLV, F4V, HTML, GLB

| format | state before | now |
|---|---|---|
| FLV, F4V | already listed as video (ffmpeg helper, `HELPER_EXTS`) | unchanged; ffmpeg made both test files, the classification is covered by `media.rs` tests. The playing itself was **not** re-tested here |
| GLB | already opened (`model.rs`) | unchanged; a hand-made GLB decodes. Draco / meshopt geometry now gives a clear error (`gltf-transform copy` re-exports it) instead of an empty model |
| SWF | only the largest bitmap, an *error* when there is none | the bitmap, else a **card** with Flash version, stage size, frame rate, frame count (`swf::info`). Flash is a program: nothing here plays or draws vector Flash |
| FLA (CS5+ = zip) | unknown | a picture of the library if there is one, else a card (stage size, frame rate, bitmap / library counts from `DOMDocument.xml`); the old binary FLA is recognised only |
| HTML (`html htm xhtml mhtml mht`) | unknown | `webpage.rs`: **headless Edge (or Chrome)** writes a screenshot (1280 x 960, first screen) to `%LOCALAPPDATA%\NuViewer\pages\<hash>.png`; then it is a PNG. About 1.5 s the first time. The browser runs with `--host-resolver-rules=MAP * ~NOTFOUND` so a page cannot reach the network just because it was previewed. One browser at a time. Without a browser: a card with the title and text. `NUVIEWER_BROWSER` names another browser |

All three new ones are listed in folders, in the file-type chooser (`assoc.rs`, unticked by default) and excluded from "convert".
A thumbnail of an HTML page therefore starts a browser: with a folder of hundreds of pages the first visit is slow (once).

## 3. Ping-pong

Button (U+21C4) between the speed and loop buttons on the bar of **animations and sequences** (not video / sound: Media
Foundation cannot play backwards), key `Shift+K` (`Action::MediaPingPong`), Settings > Screen & play, setting `play_pingpong`.
Forward to the last frame, then back to the first, again. With loop off: one trip, ends on the first frame. `App::pp_back` holds the
direction; it resets on every navigation and when playback restarts on frame 1. Applies to `on_timer` (animations) and
`seq_tick` (sequences).

## 4. Paged documents and the wheel

`App::step` (wheel, arrows, Space, PgUp / PgDn) used to carry on into the neighbouring file past the last page. In a document with
two or more pages it now stops and says "This is the last page · Ctrl+Right goes to the next file". `Ctrl+Left / Ctrl+Right`
(`NextFile` / `PrevFile`) were already there and skip the pages. Read mode (books made of many files) is unchanged.

## 5. RAW: camera look or sensor data

What the camera "post-processes" (picture style / film simulation, tone curve, sharpening, white balance) is exactly what it
wrote into the **embedded JPEG** of the RAW file. That is real data, nothing is estimated. So the toggle is:

* **camera look** = `RawMode::Preview`: the camera's own JPEG (fast; a preview below 1 MP, e.g. an Adobe DNG's thumbnail, is
  replaced by a half-size development, as before),
* **sensor data** = `RawMode::Full` (or `Half`): developed from the sensor with only what the camera wrote (docs/raw-development.md).

`Shift+R` in NuViewer (also View menu, right-click) and NuGallery (View menu; the preview panel is made again, thumbnails only
when they are developed too), NuQuick's tray menu (sensor data there is the half-size development, to stay quick),
NuImporter (check box; RAW files it converts follow it: `decode_image_bytes` now accepts RAW through a temporary file). One
setting for all programs (`raw_mode` in settings.ini; `Settings::flip_raw_look_on_disk` for programs that keep no settings in memory).

**NuEditor.** It opens the picture by the same setting. `nv_codecs::camera_data(path)` reads, without decoding the sensor,
the as-shot white-balance multipliers, their colour temperature (solved from the camera's own two colour matrices, as DNG
specifies; only when the file has two illuminants), ISO and exposure compensation, and shows them in the panel header and as a
toast. The WB sliders are relative to the as-shot balance (0 = what the camera chose), so no value is invented. Edits go to the
pair file `IMG.raf.nuedit` (the RAW is never rewritten; darktable keeps its own `.xmp`); "burn in" of a RAW has always made a PNG
copy. A pair file made while the picture was shown as the camera JPEG (4416 x 2944) is accepted when it is shown as sensor data
(6240 x 4160): for RAW files the aspect ratio (within 2%) is compared, not the pixel size (`Source::matches`).

Verified: `camera_data` on the Sony A7 III DNG ("Sony ILCE-7M3 · as shot 5442 K (red x2.46, blue x1.53) · ISO 100").

NOT done / limits:
* the camera look is the embedded JPEG at *its* resolution (Fuji X-T4: 4416 x 2944 against 6240 x 4160). Carrying the camera's look
  onto the full-size development (a fitted LUT) would be new work and an approximation.
* NuEditor does not map picture-style / film-simulation maker notes onto its sliders: no slider means the same as "Classic Chrome",
  so doing it would be guessing. The panel shows the data it can use (white balance, ISO, exposure compensation).
* nothing was tried on the user's Fujifilm / Canon files (not on this PC now); only the Sony DNG.

## Pre-existing failure seen

`cargo test -p nv-editor --test extreme` (debug build): `nr_chroma = 1000000` overflows in `blur.rs:18`; it fails the same way on
the commit before this session.
