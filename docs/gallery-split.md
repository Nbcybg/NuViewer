# Splitting the gallery from the viewer — survey and decisions

Goal (from the user): make the **viewer** as fast as it can be (it only ever shows one picture), and give the **gallery**
(NuGallery) room to grow (names, ratings, filters, big folders, selection, drag and drop, document thumbnails) without that
weight landing in the viewer.

## Survey: how tightly are they tied? (session 9, counted in the code as it is)

`nuviewer/src` is 6034 lines; the gallery is `gallery.rs` (265) and the film strip `strip.rs` (334).

**Already shared libraries, no coupling to the viewer:** `nv-core::folder` (listing, order, shuffle), `nv-core::gallery` (grid
layout maths), `nv-core::thumbs` (thumbnail maker + disk cache), `nv-core::loader` (worker pool), `nv-codecs`, `nv-render`,
`nv-win`. The thumbnail pipeline itself never touches the viewer. (Document thumbnails, session 9, live in `nv-codecs::docthumb`
and come out of `decode_for_thumbnail` like any other picture, so NuGallery inherits them for free.)

**What is tied, by count** (105 uses of `self.gallery.<field>` in the app, 82 of them outside `gallery.rs`):

| gallery field | uses | outside gallery.rs | what it really is |
|---|---|---|---|
| `dpi` | 28 | **25** | not gallery state at all: the window's DPI scale, read by hud, sidebar, controls, loading, editor, ... |
| `selected` | 36 | **24** | the gallery's cursor, written by the viewer on every navigation (this is the "Esc back" coupling) |
| `scroll` | 20 | 10 | gallery's own (viewer resets it when the folder changes) |
| `failed`, `keep` | 17 | 13 | gallery's own, cleared by the viewer when the folder changes |
| `cell`, `ready_ms`, `req_start` | 12 | 6 | gallery's own |

Other ties: 24 `Mode::Gallery` branches in 8 files (main 9, ops 6, actions 3, settings_ui 2, hud / mouse / order 1 each); thumbnail
requests: 79 mentions of "thumb" (gallery 33, main 20, strip 13, settings_ui 6, ...); sequence folding (`items` / `runs`, 40 uses,
almost all in `gallery.rs` but the `App` owns the vectors).

**So the real work is small and mechanical:** (1) move `dpi` out of `Gallery` into `App` (25 viewer lines would just rename);
(2) the viewer must tell the gallery which picture it is on instead of writing `gallery.selected` itself (24 lines become one
call); (3) `items` / `runs` move into `Gallery`; (4) the `Mode::Gallery` branches go away when the gallery is its own process.
Nothing in `nv-render` or the decoders has to change.

## Decisions

### 1. The film strip stays in the viewer

It is a navigation aid *while looking at a picture*: it only makes sense next to the picture. In NuGallery it would duplicate the
grid. What it needs from the gallery side is only the thumbnail maker, which is the shared library (`nv-core::thumbs` plus the
worker pool), not gallery code. The pool is created lazily (since session 7): a viewer session that never opens the strip
starts no thumbnail thread. So the viewer keeps `strip.rs`, links `nv-core::thumbs`, and pays nothing until the strip is used.
Measure again after the split: first-image time and idle memory (`tools/bench`), the strip must add zero when closed.

### 2. Esc / "back"

* Viewer opened **from NuGallery** (it launches `nuviewer.exe <file> --from-gallery <gallery hwnd>`, or forwards to the running
  viewer, which the single-instance code already does): **Esc closes the viewer and brings NuGallery to the front, with the
  last picture seen selected.** The viewer sends the gallery a `seen <path>` message (`WM_COPYDATA`, the same channel as the
  open request, a new kind next to `viewer`) on every navigation, debounced to one per 100 ms, and once more when it closes.
  The gallery moves its selection on receipt, so "back" lands on the picture you ended on even after a long walk with the arrows.
* Viewer opened **on its own** (Explorer, command line): Esc quits, as today.
* `G` in the viewer: starts NuGallery on the folder with the current file selected (`nugallery.exe <folder> --select <file>`) or
  focuses the one that is running; the viewer stays open.
* In-process (until the split): unchanged.

### 3. The shared file list: no shared list

Both programs build the list with the same crate (`nv_core::FolderList`) from the same inputs, and they are told the inputs, not
the result: the folder (or file), the order (`sort` / `sort_desc` come from the shared `settings.ini`; a shuffle seed is passed on
the command line, `--shuffle <seed>`, because it is not saved) and the selected file. Listing and sorting a folder costs 0.4 ms for 161 files, 1.9 ms for a folder of 996 (warm)
(`crates/nv-core/examples/scan_time.rs`), far less than a window opens in, so a shared-memory list or a list server would add
complexity and failure modes for nothing. Files added or removed later are seen by each program on its own (the viewer already
re-lists; the gallery will when it grows a folder watcher).

The one place where the lists *should* differ is documents: NuGallery lists documents (zip / cbz, Office, OpenDocument, Affinity,
text) next to pictures, the viewer's next / previous must not stop on a `.docx`. Today both are the same process, so the
switch is global: setting `documents` (Settings > Viewer, default **off**) makes folders list them, and the viewer then shows
their preview (a CBZ's first page at full size, an Office thumbnail, a card). After the split NuGallery starts with documents on
and the viewer with them off (`nv_core::folder::set_include_documents` is per process already), whatever the setting says.
The RAW + JPG rule is untouched: both stay separate entries (a unit test of the listing and the sequence folding already keeps it).

### 4. Order of work

Unchanged from the earlier plan (below), now with sizes: step 1 is a few hours (the four mechanical changes above); step 2
(`nugallery` binary + `GalleryHost` trait) is the real work; step 3 deletes code. Measure after each step with `tools/bench`.

## Target shape

```
nuviewer.exe          viewer: one picture, GPU texture cache, film strip (lazy), no grid, no gallery code
nugallery.exe         gallery: thumbnails (images + documents), names, filters, selection, drag and drop
   "open" request ->  nuviewer.exe <file> --from-gallery <hwnd>   (single-instance forward: a running viewer is reused)
   "seen <path>"  <-  viewer (debounced, and on close)           (the gallery's selection follows the viewer)
   "show folder"  <-  viewer's Gallery button / G key: starts or focuses nugallery.exe <folder> --select <file>
```

Both are the same Cargo workspace: `nuviewer/` stays, a new `nugallery/` binary crate is added, and what is shared is already in
`nv-core`, `nv-codecs`, `nv-render`, `nv-win`.

1. Move `dpi` to `App`; make the viewer *report* its current picture instead of writing `gallery.selected`; move `items` / `runs`
   into `Gallery` and make the gallery code take a `GalleryHost` trait (folder, renderer, toast, "open file"). The viewer
   implements it; nothing changes for the user.
2. Add `nugallery` with its own window, using the same `gallery.rs` through the trait.
3. Make the viewer's `G` key start (or focus) `nugallery.exe` and delete the gallery mode from the viewer.

## Done so far

* Session 7: all gallery-only code is in `gallery.rs`; the thumbnail workers start lazily (`App::thumb_pool`).
* Session 9: the survey and decisions above; folders can list documents (`nv_core::folder::set_include_documents`,
  `is_listed_ext`); thumbnails of documents (`nv-codecs::docthumb`: one ZIP reader for zip / cbz / epub, Office, OpenDocument;
  Affinity preview PNG; text cards; fallback cards with the extension, name and size); `decode_for_thumbnail` (the thumbnail
  source, as small as a thumbnail needs) separate from `decode_file_first_frame` (the whole first frame, used by the clipboard).
