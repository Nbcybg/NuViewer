# Splitting the gallery from the viewer — plan and what is already done

Goal (from the user): make the **viewer** as fast as it can be (it only ever shows one picture), and give the **gallery**
room to grow (names, ratings, filters, big folders, selection, drag and drop) without that weight landing in the viewer.

## Done in session 7 (the preparation)

* All gallery-only code is in `nuviewer/src/gallery.rs`: the `Gallery` state, the metrics constants, layout, selection,
  scrolling, thumbnail requests, drawing (`draw_gallery`), entering / leaving.
* The thumbnail workers (`Loader::thumbs`, 2-4 threads) are **no longer started with the app**. `App::thumb_pool` creates
  them the first time the gallery asks for a thumbnail, and `thumb_request(vec![])` never starts them just to cancel.
  A viewer session that never opens the gallery now spawns none of those threads.
* The thumbnail pipeline itself (`nv-core::thumbs`, `nv-core::gallery` layout maths, the disk cache) already had no
  dependency on the viewer.
* What the gallery uses from the rest of `App` is listed at the top of `gallery.rs`. It is small: folder list, renderer
  thumbnail textures, `hud` text, and the one thing it returns, "open file N" (`leave_gallery(true)`).

## Still coupled (what a real split has to cut)

| Coupling | Where | Plan |
|---|---|---|
| `items` / `runs` (sequence folding) live on `App` | `main.rs`, `sequence.rs` | move into `Gallery` (needs `rebuild_items` to take the folder) |
| Gallery draws through the viewer's `Renderer` and `hud` overlays | `draw_gallery`, `hud.rs` | the gallery process owns its own `Renderer` (the crates are reusable: `nv-render` has no viewer code) |
| Keys and mouse go through the viewer's `Keymap` / `handle` | `actions.rs`, `main.rs` | give the gallery its own small action list; share `nv_core::keys` |
| Single-instance forwarding opens a path in the one window | `nv-win::instance` | add a "kind" to the copy-data message: `viewer` / `gallery`; each program has its own mutex and window class |
| File operations (rename, delete, move, undo) | `ops.rs` | already `target()`-based; move the `UndoOp` stack into a small shared module or a file in `%LOCALAPPDATA%` |

## Target shape

```
nuviewer.exe          viewer: one picture, GPU texture cache, no thumbnail code at all
nugallery.exe         gallery: thumbnails, names, filters, selection
   "open" request ->  nuviewer.exe <file>   (the existing single-instance forward, so a running viewer is reused)
   "show folder" <-   viewer's "Gallery" button / G key: starts nugallery.exe <folder> --select <file>
```

Both are the same Cargo workspace: `nuviewer/` stays, a new `nugallery/` binary crate is added, and what is shared is
already in `nv-core`, `nv-codecs`, `nv-render`, `nv-win`. Order of work:

1. Move `items` / `runs` into `Gallery` and make the gallery code take a `GalleryHost` trait (folder, renderer, toast,
   "open file"). The viewer implements it; nothing changes for the user.
2. Add `nugallery` with its own window, using the same `gallery.rs` through the trait.
3. Make the viewer's `G` key start (or focus) `nugallery.exe` and delete the gallery mode from the viewer.

Measure after each step with `tools/bench` (first-image time and idle memory) so the change is justified by numbers.
