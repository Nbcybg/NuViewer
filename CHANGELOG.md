# Changelog

Notable changes to NuViewer, newest first. Details and measurements for each round of work are in `HANDOFF.md`.

## Unreleased

### Added
- **Layer panel** (`Ctrl+L`, View menu) for **PSD, GIMP XCF and OpenRaster** files: every layer and group with an eye, its name and an **opacity slider**; a **filter** box; the layer under the pointer is **highlighted** on the picture (the rest is dimmed, its box is outlined, even when it is switched off); Ctrl+click / right click shows one layer alone;
  double click on a slider = as saved, triple click = 0; *All on / All off / Reset*; **Save** writes the picture as the panel has it (PNG keeps transparency, JPEG gets the canvas colour behind it). Details and limits: `docs/session23.md`.
- `Ctrl+L` is in the key list. The XCF reader now goes through the same compositor (22 blend modes).

- **Spine player** (`docs/spine.md`): `.skel` / skeleton `.json` of Spine **3.8 - 4.3** open and play (CPU-drawn, so zoom / pan / snapshot / exports work on real transparency); animation, skin and slot lists in the layer panel; speed, loop, ping-pong; snapshot, PNG frame sequence and WebM with alpha. Constraints: IK, transform (old and 4.3's property mappings), **path**, **physics** (springs that keep swinging while it plays; exports step them by 1/fps), **sliders**; **two-colour tint**; **image sequences**; animations **ease** into each other over 0.25 s. Checked on real files of every version (Esoteric's example skeletons: 3.8, 4.0, 4.1, 4.2, 4.3; the four 4.3 `.skel` read to the same poses as their JSON), a 5 MB 4.1 game file and a small 4.2 sample; the 3.8 / 4.0 binary reader has no real file yet. The first fit ignores letterbox frames and parts parked far away.
- **Live2D player** (`docs/live2d.md`): `*.model3.json` / `.moc3` play with motions, expressions and the parts as layers (opacity 0-1), snapshot, PNG frames, WebM alpha. It needs Live2D's Cubism Core DLL, which you download yourself (Settings > Archives & tools says why and where); tested with a mock Core only. A `.skel` this reader cannot read uses the `.json` beside it when there is one.

### Fixed
- **Spine atlas regions packed at 180 or 270 degrees** were drawn as shards (only 90 was known), and **regions packed at 90 degrees were drawn mirrored**; all four turns are now tested with a four-colour picture, for regions and for meshes.
- Spine 4.1 / 4.2 JSON with `deform` under the animation's `attachments` key played no deform at all; deform of clipping polygons and paths is read too.
- Settings > Archives & tools: the ffmpeg and Live2D status lines (a long path) and the Live2D explanation were cut off at the group's edge; paths are shortened in the middle now.
- PSD layer visibility and group settings (the `psd` crate reads the hidden flag the wrong way round); read by our own scan of the layer records.

## 0.6.0 - 2026-11-22

The big list of 2026-11-22. Details and what was and was not verified: `docs/session22.md`.

### Added
- **NuGallery group view**: View > Sort by > *Group by* (and the toolbar Sort menu, `Ctrl+Shift+J` steps through them): **type**, **app** (Photoshop, Clip Studio Paint, Blender, Spine, a camera maker ...),
  **name** (first letter, Thai names by their first consonant), **date** (today, yesterday, last 7 / 30 days, then month by month) and **size** (tiny ... gigantic), each **ascending or descending**. Every group has a header strip
  with its count (click it to select the group); folders stay a group of their own in front; inside a group the ordinary sort applies; arrow keys cross group borders in the same column. Remembered (`gallery.ini`: `group`, `group_desc`).
- **Sliders**: **double click** puts a slider back to the value it had before the last save (NuEditor: when the picture was opened or the edits were last kept; Settings: when the window opened); **triple click** sets it to zero (its default).
  New `Event::TripleClick` in `nv-win`; the Settings sliders are subclassed track bars.
- **NuEditor and camera RAW**: a RAW file is opened from its sensor data (or the camera's JPEG look: Settings > NuEditor); the Temperature and Tint sliders show the **camera's own values** (`5230 K`, tint `+4`, read from the file, never estimated)
  around the as-shot balance, and typing a kelvin works. A RAW is never burned in. From a grouped RAW + JPG tile NuGallery opens the RAW; with the two as separate tiles each file is edited as itself.
- **NuEditor edits an animated GIF as a whole** (every frame gets the same recipe, written back as an animated GIF with per-frame palettes and transparency) and, for a video, an animation that is not a GIF, a PDF or a 3D model, edits **the frame being looked at**
  (saved as a picture the way the video snapshot settings say). Undo / redo depth is a setting (10-5000, Settings > NuEditor).
- **NuNote** (`nunote.exe`), the built-in notepad: text, Markdown, JSON, HTML, XML, CSS, JavaScript / TypeScript, YAML, TOML, INI, Rust, Python, C-family, shell, SQL, batch and PowerShell with syntax colours; **rendered Markdown** (headings, emphasis, lists, tasks, quotes,
  tables, code, links) beside or instead of the source; keeps the file's encoding (UTF-8 with / without BOM, UTF-16, ANSI) and line ends; find / replace / go to line, word wrap, zoom, drag and drop, remembers its window. `E` in the viewer and `F4` in the gallery open it for text files.
- **Extract frames** in the viewer, gallery, editor and importer (`Ctrl+Shift+F`, right-click, File menu): a video or animation as numbered pictures (every n-th with the setting), a PDF as one picture per page, a 3D model as its turntable from the view on screen. Format, quality and folder are the video snapshot's settings;
  **transparency comes out** (a run is PNG when the chosen format cannot carry it; PDF pages are transparent where nothing is painted). Progress window with Cancel.
- **Share to...** (`Ctrl+Shift+H`, right-click, File menu) in every program: the Windows share sheet with the picture(s); the editor shares the picture as it looks now.
- **3D view**: **bones** (rigged glTF / GLB / VRM; `Shift+W`) with their **names** (`Shift+N`), **outline** (`Shift+O`), **silhouette** (`Shift+V`), **support preview** (`Shift+U`: surfaces a printer would need support under, in red) and a **turntable** (`Shift+A` or `P`) driven from the video bar (play / pause,
  speed button, a seek track that is the angle); speed in Settings > 3D.
- **Formats** (cards or the picture they hold): Spine (`atlas` `skel` `spine`), Toon Boom (`xstage` `sbpz`), Moho / Anime Studio (`moho` `anme`), Adobe Animate (`xfl`), Pencil2D (`pclx`), Synfig (`sif` `sifz` `sfg`), OpenToonz (`tnz`), TVPaint (`tvpp`).
- **Languages**: seven international languages (English, 简体中文, Español, 日本語, Deutsch, Français, Português (Brasil)) plus Thai are prepared (`nv-text` registry with plural rules, number formats and fonts; mock-up files `lang/<code>.txt`; the Settings language list). **Not translated yet**: they show English.
- **Icons** redrawn (dark glass tile, gradient "nu" mark, a badge for what the program does; one for NuNote).
- **Setup, uninstall and portable**: the setup carries all six programs, the language files, the licence and the readme; Start menu entries for every program and *Uninstall*; the uninstaller also removes the Explorer menu entries and asks what else should go (cache / saved originals of burned-in pictures / settings;
  silent: `/purge`, `/keepcache`); Add/Remove entry with website, help link and install date; **portable**: the zip (`NuViewer-0.6.0-portable-win64.zip`) and `setup.exe /portable` keep everything in `Data\` beside the programs (`portable.txt`). `docs/INSTALL.md`.
- Window placement: one rule for every window (`nv-win::winpos`): back where it was, maximised if it was, never half off a screen, never on a screen that is gone, centred in the work area otherwise; written when a move or resize ends (not only on exit); dialogs are centred on the work area of their owner's screen at that screen's scale.

### Not done / not verified
- Not code-signed. Spine / Toon Boom / Moho / Animate / Pencil2D / Synfig / OpenToonz readers were written to the documented file shapes and tested on files made here, **not on real project files** (none on this PC); the 3D bones, outline, silhouette and support preview compile and use the new shader but were **not looked at on a real rigged model**.
- Language files other than English and Thai are empty mock-ups. The share sheet, the progress window, NuNote's find dialog and the Settings slider clicks were not clicked through by hand.

## 0.5.0 - 2026-11-18

What the simulated-user panel (`docs/founder-panel-2026-11-18.md`) asked for: trust first, culling with ratings, RAW + JPG as a choice, plug-ins, and one address for the website and for help.

### Added
- **RAW + JPG as one picture or two** (`Shift+Q` in the viewer, `Ctrl+Shift+G` in NuGallery, the View / right-click menus and Settings > Viewer). The factory rule stays *two separate pictures*.
  Grouped, the JPG / HEIF stands for the pair and delete, move, copy, rename and drag act on both files; Ctrl+Z restores both; a RAW without a JPG is always listed. Opening a RAW whose JPG
  stands for it shows the JPG. `settings.ini`: `pair_group = true`.
- **Star ratings and colour labels in XMP sidecars** (`Shift+0…5`, `Shift+6…9`, `` Shift+` ``, `Ctrl+Shift+X` to reject): viewer, NuGallery (several selected pictures at once), right-click **Rating** menu.
  Same fields as Bridge / Lightroom / Camera Raw / darktable / XnView (`xmp:Rating` -1…5, `xmp:Label`); an existing sidecar keeps everything else byte for byte, the picture is never touched,
  the sidecar is replaced atomically. Ratings inside a picture's own XMP are read. NuGallery: stars and label on the thumbnails, **Show rating** filter (n stars and up, not rated, rejected,
  hide rejected, a colour), the rating on the status line; the viewer: in the top bar. New error code NV-7008 (rating not saved).
- **Plug-in system, interface 1** (`docs/plugins.md`): a folder in `%APPDATA%\NuViewer\plugins` with a `plugin.ini` adds a **picture format** (an external program that writes a PNG; the formats appear
  in lists, thumbnails and the viewer) and/or **commands** in the right-click **Plug-ins** menu. Plug-ins are separate programs restricted to their own folder, started without a shell, stopped
  at their time limit, and can never take over a format NuViewer reads itself. `plugins = off` loads none. Two examples in `docs/plugin-examples/`.
- **Trust:** `LICENSE` (MIT) and a licence field in every crate; `docs/TRUST.md` (what is written where, how to verify, what is not done); tests that fail if a networking library or any
  network call enters the code (`crates/nv-core/tests/trust.rs`); `docs/third-party/dependencies.csv` (239 crates, regenerate with `tools/deps-licences.py`) and `docs/third-party/LGPL-rawler.md`;
  `tools\package.ps1` writes `SHA256SUMS.txt` and signs everything when a certificate is given in `NUVIEWER_SIGN_PFX` (not signed yet: no certificate).
- The README opens with the website, the support / feedback address, the trust summary and the comic mode and print commands that were hard to find.

### Changed
- **Website and help:** `https://nuviewer.noobcyborg.com`; **Support** and **User feedback** both go to the NuViewer Discord channel (credits in every help page, Settings > General). The Buy Me a Coffee
  link was removed: it had been a placeholder that nobody had confirmed.
- Settings > Viewer has "RAW + JPG of one shot as one picture". Undo of a pair is one step ("Undo delete (RAW + JPG)").

### Not done / not verified
- Not code-signed (no certificate). The programs were compiled and unit / integration tested here; the new menus, keys and thumbnails were **not** clicked through by hand in a running window in this round.
- The gallery inside the viewer (`G`) does not draw stars on its tiles (NuGallery does); the rating keys work there and the top bar shows the result.

## 0.4.1 - 2026-11-18

Colour tools, a borderless window, text editing in every text box, and a speed check against 0.3.2. Details: `docs/session19.md` (second part).

### Added
- **Colour picker (K)** in the viewer, the gallery (on the preview picture) and NuEditor (on the picture as edited): the value under the pointer follows it, a click
  copies it to the clipboard (Settings > General: `#RRGGBB`, `rgb()`, `hsl()`, `hsv()`, `R, G, B` or `oklch()`; Shift+click copies the other common format). Esc or K stops.
- **Palette (Ctrl+K)**: the dominant colours of the picture (2 to 16, k-means in OKLab), with the share of each; a click on a swatch copies its value.
  Nothing is read, kept or drawn until one of the two is switched on, so looking at pictures costs nothing; measured below.
- **Hue curves and the colour warper** in NuEditor (panel section "Hue curves": tabs Hue / Sat / Lum / Warp), worked in OKLCH: three curves over the hue circle with free points
  (hue shift up to 90 degrees, chroma, lightness), and a 12 x 5 warper wheel: drag a dot to the colour it should become. Out-of-gamut results lose chroma instead of being
  clipped, so the hue does not turn. Saved in the pair file (`huecurve` and `warp` lines) and shown in the gallery's and the viewer's thumbnails like the other edits.
- **Borderless window** (Shift+F11 and View > Borderless window) in the viewer, the gallery and NuEditor: no title bar or frame at the same place; its edges resize it, Alt + drag moves it.
- Every text box can be edited like one: the viewer's rename box (select all, copy, cut, paste, Shift+arrows, click / drag / double click, right-click menu), the Settings and dialog
  edit boxes (Ctrl+A), the gallery's rename / new folder / property fields (right-click menu), NuEditor's typed numbers (Ctrl+A / C / X / V, Delete, right-click menu).

### Changed
- NuGallery's toolbar no longer has the close button (the menu bar's x does that).
- The file-type chooser has a group "Archives and disc images".

### Speed (measured here, 0.3.2 against this build, medians of 9 to 17 runs)
- Viewer: first picture 85 / 86 ms, next picture 12.4 / 12.7 ms; window 42 / 52 ms (min 35 / 37, the median is noise). NuGallery: start 36 / 35 ms, 8 x 12 MP thumbnails cold 138 / 137 ms,
  warm 42 / 45 ms. NuEditor: first picture 58 / 57 ms. The programs are 4% bigger (fonts, archives, new formats).
- NuEditor's engine on a 1.9 MP window proxy: no edit 2.7 ms, three tables 3.2 ms, saturation + vibrance 13.6 ms, hue wheel 24 ms, hue curves 35 to 56 ms, warper 22 ms (before the
  work below: 54 and 45 ms). Hue curves and the warper are a 41^3 lattice of the exact OKLCH maths read by trilinear interpolation (about 20 ns a pixel instead of 100); the cells that
  touch a colour that had to be gamut-mapped are worked out in full.

## 0.4.0 - 2026-11-17

New file types: fonts, many archives and disc images, painting / design / 3D files. Table of every type: `docs/formats.csv` (343 rows). Details: `docs/session19.md`.

### Added
- **Fonts** (`ttf` `otf` `ttc` `otc` `woff` `woff2` `eot`): a specimen sheet drawn with the font itself (shaped, so Thai marks, Arabic and ligatures are right): your sentence at
  80 / 56 / 36 / 24 / 16 / 12 px, capitals, small letters, figures, a line for every script the font covers (Thai, Cyrillic, Greek, Hebrew, Arabic, Devanagari, kana, Hangul, CJK),
  and what the font says about itself (name, version, designer, licence, variable axes, glyph count). Thumbnails: the big letters, the sentence and the name. WOFF2 (Brotli, with the
  rebuilt `glyf` / `loca` / `hmtx`), WOFF and EOT are turned back into plain fonts first. `pfb` / `pfa` (Type 1) and `fon` (bitmap) get a card.
- **The sentence is a setting:** Settings > General > "Font files": up to 100 characters, default "The quick brown fox jumps over the lazy dog". A font that has none of the sentence
  (a Thai font and an English sentence) is shown in a sentence of a script it has. Thumbnails are made again when the sentence changes.
- **Archives and disc images** read from their own table of contents, without unpacking (and opened as folders in NuGallery through 7-Zip / Windows' tar): `cab` `dmg` `iso` `wim` `esd` `swm`
  `cpio` `arj` `lzh` `lha` `z` `taz` `lz` `lzma` `lz4` `tbz` `tbz2` `tb2` `txz` `tlz` `tzst` `rpm` `deb` `ar`; the existing `tar` `tgz` `gz` `xz` `zst` `7z` `rar` got the same (tar inside gzip / zstd is listed).
  The viewer shows a page with up to 70 entries and sizes, the gallery a card with the first ones. Zip-based packages (`jar` `war` `apk` `aab` `xpi` `vsix` `nupkg` `whl` `ipa` `crx`
  `appx` `msix` `kmz` `pk3` `pk4` `zipx`) show their first picture.
- **Game packs** (`pak`): Quake / Sin / Daikatana packs (the largest picture inside, else the list), Chromium / Electron resource packs (the largest PNG), Unreal Engine pak (facts).
- **Paint and design files:** Krita `kra` and OpenRaster `ora` (merged picture), GIMP `xcf` (layers composited), Paint Shop Pro `psp` / `pspimage` (stored JPEG composite),
  CorelDRAW `cdr` / `cdt` (stored thumbnail), Chasys Draw IES `cd5` (card), XPS / OpenXPS / DWFx (thumbnail, else the first page drawn by Windows), DWF (picture or card),
  Universal 3D `u3d` (card: meshes and nodes), Lattice `xvl` (card).
- **VRML** (`wrl` `vrml` `wrz`): VRML 97 and 1.0 as a 3D model you can turn (faces, boxes, spheres, cones, cylinders, elevation grids, colours).
- The file-type chooser (`--associate`) has entries for all of these (off by default).

### Not done / not verified
- `xcf`, `psp`, `cdr`, `arj`, `lzh`, `rar` listing and the Unreal pak footer were written from the format descriptions and checked against files made for the tests only (none of those programs here).
- `u3d` and `xvl` geometry is not drawn; `cd5` has no published format; `dwf` drawing streams are not drawn; MicroType-compressed `eot` is refused with a message.

## 0.3.2 - 2026-11-17

The user's list of 16 NuGallery bugs, 2 viewer bugs and "check every video". Details, causes and what was measured: `docs/session18.md`.

### Fixed
- **A video (or animated GIF / WebP) that was the first picture stayed on its first frame** while the time ran: the CPU-made first frame kept standing in for the
  real picture whenever it covered the same pixels. The real picture is drawn now and the stand-in dropped (also fixes the old picture staying after NuEditor changed the
  file). 10 of 10 test clips (mp4 / mov / mkv / hevc / webm / avi / wmv / flv, with and without sound) play; 0 of 10 did before.
- **Zip files** that open in 7-Zip but not here (a streamed or cut-off download: no table of contents, sizes after the data) open now, with what can be read of them;
  they open as a folder at once and fill while they are unpacked, instead of after everything was unpacked. Archives with no pictures (documents, sounds, text) open too.
  Damaged zips get their thumbnail the same way.
- **Two pictures selected after coming back from the viewer:** the picture the viewer ended on is the one selection now.
- **Back / Forward** go back to where the folder was left: the scroll position, the focused picture and the selection (they used to jump to the first picture).
- **Folder tree:** the right click menu works, files can be dragged onto a folder (and dropped from Explorer onto it) instead of "those files are already in that folder",
  Cut / Copy / Paste / Delete / Rename / New folder / Properties work on its folders (also with the keyboard once it is clicked), clicking a folder no longer scrolls the tree
  under the pointer, deep folders have a sideways scroll bar (Shift + wheel).
- **Address bar:** select with the mouse (drag, double click for a word), right click for Cut / Copy / Paste / Select all.
- Long names: a hint shows the whole name (with type, size and date), the Details panel writes the whole name over several lines.
- The viewer's **Fit** really fits: a picture smaller than the window is enlarged (Settings > Viewer: "Fit also enlarges small pictures", at most 8x).

### Added
- **Open with** (the programs Windows offers, plus "Choose another app") and **Properties** in the right click menus of the gallery and the viewer; **Settings**, **View**,
  **Sort by** and **Show only** in the gallery's right click menu.
- **Hover hints** on every toolbar button, folder, file and the address bar.
- **Video and sound preview in the gallery** with a play / pause button, a seek bar, the time and a volume / mute control (the wheel is the volume).
- **Archives are marked** with a yellow tag (ZIP, RAR, 7Z ...) on their tile.
- Fit and 100% are one toggle (the sidebar button, the menus, F and A).
- Thumbnails: big JPEGs show their EXIF picture at once (128 KB read instead of the whole file) and are replaced by the real thumbnail; when nothing else waits, the
  rest of the folder is made into the thumbnail cache so that scrolling finds it ready.

## 0.3.1 - 2026-11-17

### Added
- **More clipart / legacy formats:** Kodak Photo CD `.pcd` (base 768 x 512), multi-page PCX `.dcx`, Windows Clipboard files `.clp`, Cineon `.cin` and SMPTE `.dpx` film frames,
  Paint.NET `.pdn` (stored preview), Quake 2 `.wal` textures, `.wad` texture archives (Half-Life / Quake / Doom: a contact sheet), Corel PHOTO-PAINT `.cpt` (card),
  PostScript `.ps` (like EPS), vector drawings: HP-GL plots `.hpgl .hgl .hpg .plt`, binary CGM `.cgm`, and flat 2D `.dxf` drawings (lines, polylines, circles, arcs, blocks).
  Already supported and checked against the request: BMP, WMF, EMF, DIB, TIF, SRW, SRF, RAW, RW2, RWL, PBM, PGM, SND, AU, DWG (through a converter).
- **`docs/formats.csv`:** every file type NuViewer opens (272), with its full format name, type, how it is shown, whether NuEditor can edit it, and what reads it.
  Made from `crates/nv-codecs/src/formats.rs`; a test fails when a decoder gets an extension that has no row.
- `.lip` is documented as an old Clip Studio file (the picture inside is shown when one can be found; no sample file to verify against).

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
