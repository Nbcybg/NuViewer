# Session 22 (2026-11-22) - the long list: group view, sliders, RAW, i18n, icons, installer, formats, 3D, share, frames, NuNote, window place

Version 0.6.0. For every point of the user's list: what was done, where, and what was verified. "Verified" = a unit / integration test that ran here, or a window that was started and looked at (PrintWindow screenshots, no real input sent).

| # | Point | Where | Verified |
|---|---|---|---|
| 1 | Group view in NuGallery: type, app, name, date, size; ascending / descending | `nugallery/src/groups.rs` (group keys, labels, ordering), `grid.rs` (bands + header strips + arrows across groups), `model.rs` (`arrange_grouped`, `Listing.groups`), `menus.rs`, `draw.rs`, `state.rs` | 14 grid tests (hit / rect / scroll / visible / arrows with groups), 10 group tests, listing + persistence tests. **Not looked at in a running gallery window** (the CPU first-frame paint has no headers; they appear with the GPU frame) |
| 2 | Slider double click = value before save, triple click = zero | `nv-win` `Event::TripleClick` (+ click arithmetic test), `ui.rs` track-bar subclass (`slider_click_target`), `nueditor` rows / hue-wheel bands / numeric items | arithmetic tests; **not clicked by hand** |
| 3 | NuEditor reads and relies on RAW values | `nv-codecs` `CameraData.tint` (distance from the blackbody line, Kim locus, tested), `nv-editor/src/rawbase.rs` (kelvin / tint <-> slider, tested), editor shows K and tint, typed kelvin; RAW opened from sensor data (setting), never burned in; gallery opens the RAW of a grouped pair | unit tests; real RAW files not used in this round |
| 4 | i18n for 7 languages (mock-up) | `nv-text/src/registry.rs`, `lang/*.txt`, Settings list, `docs/localization.md` | 6 registry tests |
| 5 | Modern icons | `crates/nv-codecs/examples/makeicons.rs` -> `assets/icons/*.svg|ico` (six programs) | looked at at 256 / 48 / 32 / 16 px |
| 6 | Setup / uninstall complete, setup and portable | `setup/` (shortcuts for all programs, language files + licence in the payload, uninstall options, Add/Remove details, `/portable`), `nv-win/src/portable.rs`, `tools/package.ps1`, `docs/INSTALL.md` | 7 installer tests in a scratch registry key, portable tests; the real setup exe was **not run** |
| 7 | Formats: Spine, Toon Boom, Moho, Animate, cartoon animation | `nv-codecs/src/animfmt.rs`, `docs/formats-0.6.md` | 10 tests on made-up files; **no real files** |
| 8 | 3D: bones (names on / off), outline, support preview, silhouette | `nv-codecs` `Mesh.bones` (glTF skins, tested), `ModelStyle`, `nv-render` `mesh.rs` + `mesh.hlsl` (outline hull, overhang, bone overlay) | skeleton parsing tested; shaders compile; **not looked at on a model** |
| 9 | Share to... in every app | `nv-win/src/share.rs` (Windows share sheet) + viewer / gallery / editor / importer / NuQuick | file filtering tested; **the sheet was not opened** |
| 10 | Extract frames; PDF as pictures; alpha check | `nv-core/src/frames.rs`, `ffmpeg.rs`, `pdf.rs` (`render_with`), `nv-win/progress.rs` | animations, PDF pages (alpha out as PNG, white paper as JPEG), 3D turntable, many files, cancel: 9 tests. The PDF test **found that Windows paints white paper by default** (no alpha ever came out): the background is now set transparent. Video needs ffmpeg and was not run in a test |
| 11 | 3D turntable, speed, same controller as video | `nuviewer/src/turntable.rs`, `controls.rs` (`MediaKind::Turntable`) | turning arithmetic tested; **not run** |
| 12 | NuEditor undo / redo history count in Settings | `settings.rs` `editor_history`, Settings > NuEditor | settings round trip |
| 13 | NuEditor: RAW alone edits the RAW, with a JPG separate edits the JPG; GIF whole; other animations / PDF / 3D: the frame in view | gallery F4 / viewer E, `nueditor/src/flow.rs` + `nv-editor/src/gifenc.rs`, `nv-core/src/frames.rs` (`frame_for_editing`) | GIF edit round trip (3 frames inverted, burn in, new file), frame-for-editing tests |
| 14 | PDF extract in viewer and gallery | same as 10 | see 10 |
| 15 | Text files: edit, syntax and Markdown views, a simple built-in notepad | new program `nunote/` (highlighter, Markdown -> RTF, text files, Win32 + RichEdit) | 22 unit tests; started on a Markdown, JSON and HTML sample and looked at (colours, rendered page, status bar) |
| 16 | Remember the window position; check windows that did not open centred | `nv-win/src/winpos.rs` (the rule), `lib.rs` (`run_as`, `WM_EXITSIZEMOVE`), `winstate.rs` (maximised), `ui.rs` (dialogs on the owner's work area at its scale) | 7 placement tests. **Cause found for the off-centre windows:** the saved rectangle was only checked for "partly on a screen"; a window left half off a screen, on a gone monitor's place or bigger than the work area came back that way, dialogs were centred on the whole monitor (taskbar included) with the system scale, and the position was written only on exit (lost after a crash or kill) |

## Decisions made without asking

- **Seven languages:** English, Chinese (Simplified), Spanish, Japanese, German, French, Portuguese (Brazil), with Thai kept as the home language (eight slots). Change `registry.rs` if another set is wanted.
- **"App" in the group view** = the program a file belongs to, taken from its extension (a table in `groups.rs`): Photoshop, Clip Studio Paint, Krita, Blender, Spine, Toon Boom, Moho, a camera maker for RAW files ...; anything else is "Other"; common picture files are "Any image viewer".
- **"Pre-support"** in the 3D view is a *support preview*: surfaces facing down more than 45 degrees from the vertical are drawn red (what a printer would need support under). Supports that are already modelled are not told apart.
- **Extract frames** uses the video snapshot's settings for format, quality and folder (no separate dialog); a 3D model gives a turntable of `frames_model` pictures (36) starting at the current view (1 = only the view).
- **A GIF is edited whole** (all frames, per-frame palettes); other animations, video, PDF and 3D hand the *frame in view* to the editor.
- **Bones** only for glTF / GLB / VRM skins. FBX and other rigs are not read.

## Open

- Real files for the animation formats and a rigged model to look at the new 3D modes; a hand test of the share sheet, the progress window, NuNote's find box and the slider clicks.
- Translations. Code signing. The CPU first frame of the gallery does not draw group headers.
