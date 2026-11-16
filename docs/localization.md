# Text and languages (version 0.2.0)

Nothing the user reads is decided by the code alone any more:

* `crates/nv-text/lang/en.txt` — the English text, one `key = text` per line (`\n` is a line break, `\t` a tab, `{0}` `{1}` are filled in by the program). The **top** of the file has keyed texts (`snapshot.saved = Frame saved: {0}`); below the marker line are the **phrases the older code still writes in English** (`Previous picture = Previous picture`), found by `tools/lang-extract.py` (the phrase is its own key). Run `python tools/lang-extract.py` after adding UI text; `--check` lists phrases that are not in the file yet.
* A translation is the same file under another name: `lang/<code>.txt` next to the program or in `%APPDATA%\NuViewer\lang` (`NUVIEWER_LANG_DIR` overrides). What it leaves out shows in English. `crates/nv-text/lang/th.txt` is a Thai starter (menus, settings, the new features).
* Choose it in **Settings > Language & Explorer** (or `language = th` in `settings.ini`; `auto` follows the Windows display language). The button there copies `en-template.txt` into the language folder.
* How it works: `nv_text::t("key")` for keyed text, `nv_text::tr("English phrase")` for phrases. The window layer (labels, buttons, check boxes, group boxes, combo items, menu entries, menu-bar labels), toasts, the help pages, action labels and the editor's slider names all go through `tr`, so one file covers them. English is the built-in table and costs nothing at start-up (`tr` does nothing until another language is loaded).
* Not done: strings that are *built* from pieces in the code (`format!("{n} files")`) and a few drawn directly in the gallery and editor panels stay English until they get keys; the gallery's file names, EXIF names and similar data are not text of ours.
