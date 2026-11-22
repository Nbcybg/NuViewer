# Formats added in 0.6.0 (animation programs) - what is read and how far it was checked

None of these is a picture format. Each shows the best picture the file (or the folder around it) holds, else a card with what can be read. They were written to the documented shapes of the files and tested on files made for the tests (`crates/nv-codecs/src/animfmt.rs`); **no real project file of any of these programs was available**, so a real file may show a card where a picture could have been found.

| Program | Extensions | Shown | Read from |
|---|---|---|---|
| Spine | `atlas`, `skel`, `spine` | the atlas page picture (the sprite sheet) | `.atlas` text (pages, sizes, region count) + the page PNG next to it; `.skel` borrows the sibling `.atlas` and reports the Spine version (3.x / 4.x) found in its header; `.spine` is a card (a zip is searched for a preview picture) |
| Toon Boom Harmony | `xstage` | card: size, frame rate, drawing / peg / composite counts | the scene XML (several spellings of the fields are tried) |
| Toon Boom Storyboard Pro | `sbpz` | the thumbnail or biggest picture in the package | zip |
| Moho / Anime Studio | `moho`, `anme` | the preview picture of a zipped project, else a card with canvas size, frame rate, end frame | zip or JSON text (keys named width / height / fps / endFrame anywhere in it) |
| Adobe Animate | `xfl` | the biggest picture of `LIBRARY\`, else a card (stage, frame rate, layers, keyframes) | the folder around the stub: `DOMDocument.xml`, `LIBRARY\` (`fla`, the zipped kind, was read before) |
| Pencil2D | `pclx` | the first frame: the bitmap layers of the lowest frame number composited | `main.xml` + `data/<layer>.<frame>.png`; vector layers are not drawn |
| Synfig Studio | `sif`, `sifz`, `sfg` | card: canvas size, frame rate, time range, layers | XML (gzip for `sifz`, zip for `sfg`) |
| OpenToonz | `tnz` | card: resolution, frame rate, output range, levels | the scene XML |
| TVPaint | `tvpp` | card | closed format |

Also new in 0.6.0 and in the same spirit: editing a GIF as a whole in NuEditor, and "Extract frames" (`docs/session22.md`).
