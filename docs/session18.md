# Session 18 (2026-11-17): the 16 gallery bugs, 2 viewer bugs and the video check

The user's list, with the cause found for each, what changed, and how it was checked. Scripts: `tools/test/e2e-fixes.ps1` (gallery; it runs with
`NUGALLERY_TEST=1`, which adds what is only on screen to the window title), `tools/test/e2e-video.ps1` (viewer, 10 clips made with ffmpeg).
Popup menus make the gallery script timing-sensitive: a failure of one popup check on a loaded PC is worth one rerun.

## Video (the "frozen frame" report)

Cause: `display.rs::proxy_rect`. The first picture of a viewer is painted on the CPU while Direct3D starts (a "proxy" texture). It kept being drawn instead of the
real texture whenever both covered the same pixels. A video that was the FIRST picture (opened from Explorer, or from the gallery) therefore showed frame 0 for
ever while the clock and the bar ran. Opening the same video by arrowing to it worked. Animated GIF / WebP as first picture had the same fault, and so did a first
picture changed by NuEditor. Now the real texture is drawn as soon as it exists and the proxy is dropped then.
Checked: `e2e-video.ps1`: h264 mp4 / mov / mkv, h264 with B-frames, hevc, vp9 webm, mpeg4 avi, wmv, flv, silent mp4 all move within 1.7 s (before: none).
`crates/nv-media/examples/play.rs` shows the player itself was never at fault (115 frames in 4 s).
Not verified: sound (this PC's test session), 4K HEVC from a phone (Windows needs the HEVC extension; the ffmpeg proxy path then makes a playable copy first, which
takes a while for a long clip and shows the poster meanwhile).

## Gallery

| # | Cause | Change |
|---|---|---|
| 1 two selected after the viewer | a read that asked to select added to the old selection | a read that selects replaces it |
| 2 thumbnail of something not in the zip | not reproduced with zips that have a table of contents (tiles show the right pictures); the broken zip had no thumbnail path at all | damaged zips are read from the front (`zipscan`) for thumbnails and for opening |
| 3 Back / Forward to the first picture | history kept only paths and selected the folder came from | history entries carry scroll, focused item and selection (`NavPoint`) |
| 4 zip opens / does not open, slow | no table of contents -> "not a zip"; everything was unpacked before anything showed | `nv_codecs::zipscan` walks local headers (data descriptors, ZIP64, cut-off last entry); folder shown at once and filled while unpacking; every file type, not only pictures |
| 5 no zip mark | none | yellow ext tag on archive tiles |
| 6 no Open with | none | `nv_win::openwith` (SHAssocEnumHandlers), gallery and viewer menus |
| 7, 8 tree and files do not work together, no right click | drops on the tree went to "the folder on screen"; no menu | `treeui.rs`: drag from items onto tree rows / folder tiles (own drag state, OLE only after leaving the window), drops from Explorer resolve tree rows, full menu, keyboard when the tree has focus |
| 9 tree scrolls by itself | `reveal_tree` used half the window height as the tree's height | real height, and a click never moves the tree |
| 10 long names | `fit()` with an ellipsis | hint with the whole name; the Details panel wraps it |
| 11 no settings in the right click menu | none | Settings, View, Sort by, Show only |
| 12 no sideways scroll | none | sideways scroll bar, Shift + wheel (and a sideways wheel) |
| 13 no controller for video | the preview showed the poster only | player + bar in the preview (`preview.rs`) |
| 14 slow thumbnails | measured, see below | EXIF quick path + background warming |
| 15 address bar | caret only by keys, no drag, no menu | mouse select, double click, right click menu |
| 16 no hints | none | `hints.rs` |

### Thumbnails: what the slowness is

`crates/nv-core/examples/thumbbench.rs`, release build, empty cache:
- 161 PNG of ~1 MB on F: (a hard disk): 60 ms each one after the other, 24 visible in 1.27 s in the gallery. The same files already in the OS cache: 316 ms for 24 with
  one worker, 96 ms with five: the decoding scales perfectly, the time is the disk (many small reads at ~20 MB/s effective).
- Fuji JPEG 6240 x 4160 (17 MB) + RAF (59 MB): RAF 90 ms (partial read), JPEG 230 ms decode + the whole 17 MB read. 24 visible took 5.7 s, all disk.
- Folder covers: choosing the picture 2 ms, its thumbnail 65 ms each.
Nothing is wrong in the code path; the cost is reading whole files from a slow disk. Changes: big JPEGs give their EXIF picture first (128 KB read), the real
thumbnail replaces it; one low-priority worker fills the disk cache for the rest of the folder when nothing else is waiting. Not measured after the change on F: (cold
cache needs the disk to be cold, see the numbers in `HANDOFF.md` "Session 18").

## Viewer

- Fit enlarges (`fit_scale_for`, at most 8x; `Settings.fit_enlarge`); the CPU first frame enlarges the same way (`fit_like_gpu` works for scales above 1).
- Fit and 100% are one toggle: one sidebar button that shows the view a press switches to, one menu entry, F and A both toggle.
- The hold-the-button zoom never zooms out from an enlarged small picture.
