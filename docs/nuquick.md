# NuQuick: Space-bar preview for Explorer

Windows has no Quick Look. NuQuick is the viewer cut down to the one job: select a picture in Explorer (or on the desktop),
press **Space**, see it at once; **Space** or **Esc** closes, **Enter** opens it in NuViewer, the **arrow keys** keep moving
Explorer's selection and the preview follows. Goal: faster than macOS Quick Look. Program: `nuquick.exe` (crate `nuquick/`).

## How it works

One small resident process, nothing on screen except a tray icon (Pause / Start with Windows / Settings / Quit). **Switching it on and off:
NuViewer's Settings > "Space preview" tab** (Run now; Start when I sign in to Windows). The state shown there is the real one (is the agent's
window there, is the Run entry there). A start by hand shows a balloon "NuQuick is on"; the Run entry starts it with `--silent` (no balloon).

| Part | What it does |
|---|---|
| Foreground WinEvent (always on) | one event per change of the front window; the only always-on hook |
| Keyboard hook (`WH_KEYBOARD_LL`) + selection WinEvent | installed **only while Explorer or the desktop is in front**, removed the moment anything else is. A game / anti-cheat never has a hook of ours installed |
| Selection lookup (`explorer.rs`) | `IShellWindows` -> `IShellBrowser` -> `IFolderView2` (the marked item, only while something is selected); ~1.5 ms warm, ~4 ms measured under load, 28 ms the first time (warmed at start) |
| Decode worker (`view.rs`) | latest request wins; decodes the first frame (RAW = the camera's embedded preview), shrinks it to 90% of the screen, applies orientation + colour on the small copy; cache of 4 pictures |
| Pre-decode | when the selection settles (120 ms) the picture under it is decoded ahead of time (skipped for files > 80 MB and network paths), so Space usually only shows a finished bitmap |
| Preview window | borderless, topmost, `WS_EX_NOACTIVATE`: it never takes the focus from Explorer. Name + type / size / file size caption. Pre-placed when the decode finishes. No drop shadow (it costs ~8 ms per show) |

**Like Quick Look:** with the preview up, selecting something else (arrow keys, mouse, type-ahead) shows that, and it stays up until Space / Esc
(or a click on it, or another window coming to the front) closes it. Something that is not a picture (a folder, a document, a picture we cannot decode)
gets a card with its name, type and size, so the preview never shows a stale picture. While it is up the selection is also asked for every 150 ms, so it
does not depend on Explorer's events arriving.

**Drag and remember:** the preview window can be dragged anywhere (press and move; a press without moving is a click and closes it). Its centre is
remembered (`%LOCALAPPDATA%\NuViewer\nuquick-pos.txt`), so the next preview, of any size, opens centred on the same point, kept inside that screen.
Whether Quick Look on a Mac reopens where it was left was not checked here; this does.

**Info cards (like Quick Look's):** Space on a folder, a document, a video, a program ... shows a card: the file's own icon (Explorer's thumbnail when the
file has one), then Kind, Size (or the number of items in a folder), Created, Modified and Where. Built on the decode thread (the shell may take a moment).
Enter opens a picture in NuViewer and anything else the way Windows opens it. The list of formats now only decides what is decoded as a picture.

**Threads (since the flaky-Space fix, 2026-11-16):** the keyboard hook lives on a thread of its own and decides with atomics only; the Explorer lookup
(COM, 1.5 ms to over 500 ms when Explorer is busy) runs on a lookup thread. Before, the lookup ran on the hook's thread: a slow one made Windows skip
the hook (~300 ms limit) and eventually remove it, so Space "worked sometimes". Space is now taken whenever Explorer's list has the focus and the answer
decides afterwards (not a picture: the key is sent again to Explorer); on Space the last known selection is shown at once and verified by a new lookup.

Space is only taken when: Explorer / desktop is in front, the focus is on the **file list** (not a rename / search / address box), no
modifier is down, no letter or digit was typed in the last 0.7 s (type-ahead search keeps its spaces), and the selected item is a
format we show. Otherwise Space goes to Explorer untouched. If the hook took Space for what turns out not to be a picture, the key is
re-sent (from another thread: `SendInput` waits for low-level hooks and ours lives on the thread that would be waiting).

Formats (`view::EXTS` + every RAW): JPEG, PNG/APNG, GIF (first frame), WebP, BMP/DIB, ICO, TIFF, AVIF, HEIC/HEIF (needs Windows' HEVC extension),
JPEG XL, JPEG 2000, PSD/PSB, TGA, DDS, SVG, QOI, PNM, HDR, EXR, PCX and camera RAW. Left out on purpose: PDF / Office / archives, 3D, video:
a slow guess would make Space feel slow, which is the one thing it must not. Widening the list costs nothing in the background (codecs sit
on disk until used), only the per-format decode time matters.

## What it costs in the background (measured 2026-11-16, 16 GB RAM, Ryzen 5 4500)

| | |
|---|---|
| Idle, nothing happening | private 2.3 MB, working set 0.1 MB after the start-up trim, CPU 0 ms per 5 s |
| After use (COM, decoded pictures) | private ~10 MB, working set ~16 MB; the cache and working set are given back 60 s after the last Space (30 s after Explorer loses the front) |
| Per key press while Explorer is in front | 0.45 us for the check, plus the hook round trip; none while another program is in front |
| While browsing pictures | one decode per settled selection: ~60 ms CPU for a 12 MP JPEG (about 0.5% of the 12 threads at one picture per second), none when not browsing |

## Speed (message-driven test, `tools/test/e2e-quick.ps1`, 12 MP JPEG, release build)

- Space on a picture that was selected a moment before (pre-decoded): **~9 ms** from the key to pixels on screen (lookup ~5 + show 1.6 + paint 1.4).
- Space before the pre-decode finished: ~13-20 ms (the decode ran during the lookup).
- Stand-alone `nuquick.exe --preview file --timing` (process start + decode + show): ~100 ms warm. That is the fallback cost without the resident agent.
- With the drop shadow switched on (`NUQUICK_SHADOW=1`) show takes 9 ms instead of 1.6; kept off.
- macOS Quick Look was not measured here (no Mac): compare with Activity Monitor (`QuickLookUIService`, `quicklookd`) and a screen recording.

## Switches

`nuquick.exe` agent (default) | `--preview <file> [--timing]` show one picture in the same window (takes the focus; Esc / Space / click closes) |
`--quit` ask the running agent to exit | `--startup-on` / `--startup-off` the HKCU Run entry (also in the tray menu) |
`NUQUICK_TEST=1` log to `%TEMP%\nuquick-test.log` and allow a second agent next to the user's (tests only) | `NUQUICK_SHADOW=1` drop shadow.

## Not verified (needs real input, which the tests must not send while a game has the focus)

- The real **Space key through the hook** (`key_decision`) in a live Explorer window: only the code after the hook (lookup, decode, show, close,
  not-a-picture) is exercised, by posted test messages (`WM_APP+10` = Space, `+2` = close, `+11` = quit).
- Selection events from Explorer while moving with the arrow keys (the e2e saw them arrive; the follow-the-selection behaviour while the preview is up is untested).
- Enter -> NuViewer; the tray menu; DPI above 100%; several monitors; Explorer tabs; the desktop (lookup measured in a prototype only); an elevated (admin) Explorer
  (a non-elevated hook cannot see it: UIPI); a restart of explorer.exe (tray icon is re-added on `TaskbarCreated`, the shell objects are re-created on a failed call).
- The installer carrying `nuquick.exe` compiles (`cargo check -p nuviewer-setup`); `tools/package.ps1` was not run.

## Ideas

A lean build (no gallery / editor / PDF / 3D code: 13 MB exe today, mostly decoders) would cut start-up page-ins but not the resident cost; keep the
shell-extension route (an `IPreviewHandler`) out: it runs inside Explorer's preview pane, not on Space. Animated GIF / WebP, video poster frames,
PDF first page, a zoom with the wheel, a Mac-style grow-from-the-item animation, common file dialogs (same list control, other process).
