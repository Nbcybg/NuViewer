# Can you trust NuViewer?

One developer, a young version number, an `.exe` from a stranger: the doubt is fair. This page lists what NuViewer does and does
not do, **how to check each statement yourself**, and what is still missing. Where something is not done yet, it says so.

## What is checked by the programs' own tests

`cargo test` fails if any of these stop being true (`crates/nv-core/tests/trust.rs`):

| Promise | The check |
|---|---|
| **No network code.** NuViewer never connects to the Internet: no update check, no telemetry, no crash upload, no licence server. | No networking library (`reqwest`, `ureq`, `hyper`, `tokio`, `curl`, `openssl` ...) is in `Cargo.lock`, and no source file uses `std::net`, sockets, WinHTTP, WinINet or the Windows networking APIs. `python tools/deps-licences.py` makes the same check against `cargo metadata`. |
| **A licence in the source.** MIT, see `LICENSE`. | The licence file exists and every crate carries `license.workspace = true`. |

Two optional things start another program, and neither is NuViewer talking to the network:

* **Web-page previews** (`.html` and friends, only if you list documents): a headless Edge or Chrome draws the page. It is started
  with name resolution switched off (`--host-resolver-rules=MAP * ~NOTFOUND`), so the page can only use what is on your disk.
* **FFmpeg**, only if *you* installed it, for video and sound Windows cannot play.

"Open the website / support" buttons hand an address to Windows, which opens your browser. That is the only way a web address is used.

## Your pictures are never changed behind your back

NuViewer is a viewer first. This is the complete list of what it writes:

| Where | What | When |
|---|---|---|
| `%APPDATA%\NuViewer\settings.ini` | your settings | when you change one |
| `%LOCALAPPDATA%\NuViewer\thumbs` | the thumbnail cache (small JPEGs) | when a folder is shown in the gallery |
| `%LOCALAPPDATA%\NuViewer\pages` | pictures of web pages | when you preview a page |
| `%LOCALAPPDATA%\NuViewer\undo` | the original of a picture before you save a rotation or burn in an edit (Ctrl+Z restores it) | only on an explicit save |
| next to a picture: `NAME.EXT.xmp` / `NAME.xmp` | the **star rating and colour label** you gave it (the `xmp:Rating` and `xmp:Label` fields; everything else already in that sidecar is kept byte for byte) | when you press a rating key |
| next to a picture: `NAME.EXT.nuedit` | the edits you made in NuEditor (the picture itself stays as it is) | when you leave NuEditor with "pair file" chosen |
| a new file next to the picture | a copy you asked for: convert, "save a copy", snapshot, PDF, archive | only when you ask |
| Recycle Bin / another folder | delete, move, rename, copy: exactly what you asked, with Ctrl+Z | only when you ask |

Never done: the picture file itself is not rewritten by rating, labelling, grouping, plug-ins or looking at it. Rotating and burning an
edit in are the two commands that rewrite a picture, they always keep the original in the `undo` folder first, and rotation asks
before saving unless you switched that off.

**RAW + JPG.** A RAW and a JPG of the same shot are two separate pictures unless you switch grouping on (Shift+Q); then the JPG stands for both
and delete, move, copy and rename act on **both files**, so a pair can never be split by accident. Nothing is ever merged on disk.

## Plug-ins can only do what you put in the folder

A plug-in (`docs/plugins.md`) is a folder you copy into `%APPDATA%\NuViewer\plugins`. NuViewer does not download, update or install
plug-ins, and does not load any code of theirs into its own process. A plug-in is a separate program that starts only when you use
its format or its menu command, can only name programs inside its own folder, gets each file name as one argument (no shell, so a name
cannot turn into a command), and a decoder is stopped after its time limit. `plugins = off` in `settings.ini` loads nothing.
A plug-in runs with your user rights like any program you download: **only install plug-ins you trust.**

## Check the download yourself

* **SHA-256.** `tools\package.ps1` writes `SHA256SUMS.txt` next to the installer and the zip. Compare with
  `Get-FileHash NuViewer-0.5.0-setup.exe -Algorithm SHA256`.
* **Build it yourself.** Everything is in this repository; Rust (stable), Windows 10/11:
  ```
  git clone https://github.com/Nbcybg/NuViewer
  cd NuViewer
  cargo build --release -p nuviewer -p nugallery -p nueditor -p nuquick -p nuimporter
  ```
  Give the same Rust version and the same `Cargo.lock` and you get the same program logic (byte-identical files are not promised:
  the linker embeds paths and times).
* **Run it with no network.** Block `nuviewer.exe` in the Windows firewall, or unplug the cable: nothing changes, because nothing
  uses it. Process Monitor / Resource Monitor show no network activity from it either.
* **Every library** it is built from, with version and licence, is in `docs/third-party/dependencies.csv`
  (regenerate with `python tools/deps-licences.py`).
* **The speed claims.** The five-viewer benchmark is `tools/bench/compare` and its results `docs/benchmark-compare-2026-11-17.md`. It is run by
  the developer on one PC; the harness is published so you can run it on yours and say what you get.

## What is not done yet

* **The programs are not code-signed.** A signing certificate costs money and the developer has not bought one. Windows SmartScreen may
  say "unknown publisher" on the first start. `tools\package.ps1` already signs everything when `NUVIEWER_SIGN_PFX` points to a
  certificate, so signing is one environment variable away the day there is one. Until then the SHA-256 sums and the source are the proof.
* **One developer.** The answer to "will it be maintained?" is the changelog (`CHANGELOG.md`), the public history of this repository,
  and the licence: if it stops, the MIT licence lets anyone carry it on.
* **No independent review yet.** If you read the code or run the checks above and find a problem, please say so (below).

## Talk to the developer

* Website: <https://nuviewer.noobcyborg.com>
* Support and user feedback (what is missing, what stops you switching, what broke):
  <https://discord.com/channels/1572474506872750212/1572474507363491842>
* Security problem? Say so in the channel above without posting a file that contains anything private; the developer will answer there.
