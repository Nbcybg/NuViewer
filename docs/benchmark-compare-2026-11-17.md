# Benchmark: NuViewer vs IrfanView vs ImageGlass vs FastStone vs Image Simple Viewer (2026-11-17)

Machine: Windows 11, 2560x1440, GTX 1050 Ti, 16 GB RAM, files on T:. Versions: NuViewer 0.2.0 (the installed build, user's own settings),
IrfanView 4.76 64-bit (no plug-ins installed), ImageGlass 10.0.906 (Store, first-run wizard skipped), FastStone 8.3, Image Simple Viewer 2.4.0.0 (Store).
Test folder `T:\Benchmark-folder` (351 files, 6 GB). Harness: `tools/bench/compare/` (BH.cs + PowerShell scripts, raw CSV/log/screenshots in `T:\bench-results`).

## How it was measured (same for all five, nothing special-cased for NuViewer)
- Time = real screen. A recorder grabs the whole 2560x1440 screen every ~21 ms (1/4 size, nearest sample) behind a black backdrop window.
  "Picture complete" = the first frame after which the screen no longer differs from the final frame (more than 0.2 % of sampled pixels, 10 grey levels).
  Small live overlays (NuViewer's disk meter) fall below that threshold. Resolution of every number is about +-21 ms.
- Start = the moment just before the process is started (open) or before the key / click is sent (navigation). Real keystrokes through SendInput
  (nothing posted to the window, no NuViewer test hooks, no title-bar timings). The window must be the foreground one before a key is sent, otherwise the run is skipped.
- Every file is read once before the five viewers open it (same file-cache state). The order of the viewers rotates from file to file / pass to pass.
- Everyone opens big: NuViewer fullscreen (window.ini), IrfanView `/fs`, FastStone's own full-screen viewer, ImageGlass and Image Simple Viewer maximised. Gallery windows are each program's own size.
- Correctness is checked, not assumed: after next x5 / back x5 the last picture must equal the picture seen at that position in the slow pass.
  Each format-open run was also looked at by eye (contact sheets `T:\bench-results\sheetA-*.jpg`).
- Limits: format test = 1 run per cell (re-checked by hand where a run looked cut short: ImageGlass TGA/HDR/EXR, FastStone CR2);
  navigation = 3 passes x 5 apps; gallery = 2 cold + 3 warm per app. Audio could not be heard (no sound device in the tool session), so audio "plays" means a player/waveform appeared.

## Test A - open one file of every format (ms until the picture is complete; x = could not open)
| file | NuViewer | IrfanView | ImageGlass | FastStone | Simple Viewer |
|---|---|---|---|---|---|
| JPG pano 6 MB | 322 | 299 | 1231 | 383 | 898 |
| JPG 18 MP | 319 | 398 | 1435 | 453 | 1120 |
| JPG damaged (no EOI) | 2081 | 425 | 1545 | 434 | 927 |
| PNG small | 117 | 224 | 920 | 905 | 767 |
| PNG 14 MB | 346 | 395 | 1072 | 1328 | 835 |
| PNG 56 MB | 892 | 1103 | 2347 | 1814 | 1521 |
| WebP | 117 | x | 917 | 841 | 817 |
| AVIF | 466 | x | 1197 | 1407 | 1024 |
| TGA | 298 | 231 | 12537 (shows "Loading..." first) | 909 | x |
| PSD small / 32 MB / 293 MB | 109 / 317 / 344 | 242 / 235 / 269 | 2203 / 1407 / 2532 | 788 / 908 / 1132 | x |
| HDR | 324 | x | 5605 | x | x |
| EXR 276 MB | 1698 | x | 3891 | x | x |
| RAW ARW | 611 | x | 1076 | 2327 | 1910 |
| RAW CR2 | 638 | x | 920 | 3296 | 2136 |
| RAW RAF | 472 | x | 1335 | 2394 | 1925 |
| AI | 641 | x | x | x | x |
| PDF | 792 | x | x | x | x |
| GLB / STL / OBJ (3D) | 907 / 458 / 1651 | x | x | x | x |
| MP4 / MOV | plays / plays | x (black) / plays | x | plays / plays | x |
| MP3 / WAV | waveform / waveform | small player / small player | x | visualiser / visualiser | x |
| SAI, OCI, DUF, MAX, 3MF, MID | x (error panel NV-xxxx) | x | x (error page) | x | x |

Opened (of 32 test files): NuViewer 26, FastStone 19, ImageGlass 17, IrfanView 13, Image Simple Viewer 10.
Median of the six JPG/PNG rows: NuViewer 334, IrfanView 396, FastStone 679, Image Simple Viewer 912, ImageGlass 1333 ms.
How each program says "no": IrfanView = modal error dialog (and black / nothing for MP4, MID); ImageGlass = error page with a stack trace; Image Simple Viewer = a small
message with a blue button; FastStone = silently shows a different picture from the folder (or black for MID) - no error at all; NuViewer = an error panel with an NV code.

## Tests B + C - JPG/PNG mix folder (12 files, 0.8-18 MB, alternating formats), median of 3 passes, ms until the picture is complete
| | first open | paced next (per step) | paced back (per step) | rapid next x5 (one burst) | rapid back x5 |
|---|---|---|---|---|---|
| NuViewer | 304 | 38 (max 70) | 53 (max 79) | 84 | 149 |
| FastStone | 399 | 50 (max 97) | 53 (max 114) | 607 | 604 |
| ImageGlass | 1013 | 70 (max 554) | 90 (max 545) | 115 | 151 |
| IrfanView | 294 | 268 (max 310) | 267 (max 323) | 895 | 967 |
| Image Simple Viewer | 858 | not measurable | not measurable | not measurable | not measurable |
All four measurable programs landed on the right picture every time (0 wrong in 36 checks each). Image Simple Viewer says "could not access the rest of this folder -
enable full file system access in Settings": without that Windows permission it only shows the file it was started with (one step worked, then nothing), so
next/back could not be tested. I did not change that Windows setting.

## Test D - gallery, all visible thumbnails drawn (60 pictures: 50 PNG + 10 JPG, ms)
| | cold (new folder path) | warm |
|---|---|---|
| NuViewer (NuGallery, whole grid visible) | 711 / 718 | 359-366 |
| ImageGlass (thumbnail bar only, picture + ~35 thumbs) | 1170 / 1180 | 847-869 |
| IrfanView (thumbnail window) | 1974 / 1981 | 1915-1985 (no cache) |
| FastStone (browser) | 4283 / 4264 | 1465-1529 |
| Image Simple Viewer | no gallery | |

## Test E - gallery <-> viewer (3 round trips each, ms until the screen is complete)
| | gallery -> viewer | viewer -> gallery |
|---|---|---|
| NuViewer (Space / G) | 108, 101, 103 | 71, 60, 71 |
| IrfanView (double-click / Esc) | 130, 115, 137 | 564, 550, 560 |
| FastStone (Enter / Esc) | 280, 223, 246 | 294, 192, 204 |
| ImageGlass, Image Simple Viewer | no separate gallery mode (thumbnail bar / nothing) | |

## Where NuViewer is NOT first
- Damaged JPEG (no EOI, Samsung 12368x2448): 2081 ms against 425-434 ms for IrfanView and FastStone (repair + fallback path; all the others just show it).
- PSD 293 MB and PSD 32 MB: IrfanView 269 / 235 ms against 344 / 317 ms (a tie within noise for the big one, a clear 80 ms for the 32 MB one); TGA 231 vs 298 ms; JPG pano 299 vs 322 ms.
- Rapid back x5: ImageGlass 151 vs NuViewer 149 is a tie; first-open of the mix: IrfanView 294 vs 304 ms is a tie.
- Cannot open: SAI, OCI, DUF, MAX, 3MF, MID (nobody can, NuViewer shows a clear error).

## Things that happened to the other programs (for the record)
- ImageGlass opened a "Quick Setup" wizard on first start; I pressed "Skip this and launch ImageGlass" (that is the only setting I touched in it, plus maximising the window).
- An elevated IrfanView window (pid 19104, empty) was already open from the install; it could not be closed from here and was left alone.
- Image Simple Viewer was open from the install; I closed its window once, then maximised it so it opens big.
- NuViewer: `window.ini` was set to fullscreen for the run and restored afterwards from the backup in `T:\bench-results\backup`; `gallery.ini` restored too.
- Bug in my first harness version (found and fixed before any number above): the "bring window to front" helper tapped Alt, which opened NuViewer's menu bar and swallowed the arrow keys.
  Also an early run had the black backdrop above the viewers; both fixed and the runs repeated.
