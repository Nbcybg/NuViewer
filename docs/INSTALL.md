# Installing, updating, uninstalling, running portable

NuViewer is two downloads. They hold the same programs (NuViewer, NuGallery, NuEditor, NuImporter, NuQuick); pick the one that fits.

| Download | For | Where it lives | What it changes on the PC |
|---|---|---|---|
| `NuViewer-<version>-setup.exe` | everyday use | `%LOCALAPPDATA%\Programs\NuViewer` (or the folder you choose) | file-type entries for your user, Start menu shortcuts, an "Apps > Installed apps" entry. No administrator rights. |
| `NuViewer-<version>-portable-win64.zip` | a USB stick, a work PC you may not install on, trying it out | wherever you unzip it | nothing: settings, thumbnails, presets and backups stay in the `Data` folder beside the programs |

`SHA256SUMS.txt` next to the downloads holds the SHA-256 of both (`Get-FileHash <file> -Algorithm SHA256`, or `sha256sum -c SHA256SUMS.txt`). The programs are not code-signed yet (see `docs/TRUST.md`), so Windows SmartScreen may say "unknown publisher": that is the reason to check the sums.

## Setup

Double-click the setup. It asks for

* the install folder,
* which image formats should open with NuViewer (you can change that later with *NuViewer file types* in the Start menu),
* a Start menu shortcut (NuViewer, NuGallery, NuEditor, NuImporter, file types, Uninstall) and a desktop shortcut.

Running a newer setup over an installed copy updates it: running programs are asked to close, settings are kept.

**Portable copy from the setup:** tick *Portable copy* in the setup window (or run `NuViewer-<version>-setup.exe /portable /D=E:\NuViewer`). It only unpacks the programs into the folder and writes `portable.txt`; nothing is registered.

### Silent install

```
NuViewer-<version>-setup.exe /S                       per-user install with the default formats
NuViewer-<version>-setup.exe /S /D=D:\Apps\NuViewer    another folder
NuViewer-<version>-setup.exe /S /formats=jpg,png,cr2   or: all, none, default
NuViewer-<version>-setup.exe /S /nostartmenu /desktop /launch
NuViewer-<version>-setup.exe /S /log=C:\temp\nv.txt    write the outcome as text
NuViewer-<version>-setup.exe /portable /D=E:\NuViewer /S
```

## Uninstall

*Settings > Apps > Installed apps > NuViewer > Uninstall*, or *Uninstall NuViewer* in the Start menu, or `uninstall.exe` in the install folder. It removes the programs, the shortcuts, the file-type entries, the Explorer "Convert to PNG / JPG" entries, NuQuick's start-with-Windows entry and the Add/Remove entry, and asks what else should go:

| Choice | Default | What it is |
|---|---|---|
| thumbnail cache and logs | on | made again when needed |
| saved originals of pictures NuEditor burned in | **off** | the only copy of such a picture's earlier state |
| settings, presets, language files | **off** | `%APPDATA%\NuViewer` |

Your pictures and the `.nuedit` pair files next to them are never touched.

Silent: `uninstall.exe /uninstall /S` keeps settings and backups and removes the cache; add `/purge` to remove everything, or `/keepcache` to keep the cache too.

## Portable

Unzip `NuViewer-<version>-portable-win64.zip` anywhere and run `nuviewer.exe` or `nugallery.exe`. The file `portable.txt` beside the programs is what turns portable mode on: at start-up the programs point their data folders at `Data\Roaming` and `Data\Local` in that folder (settings, window position, gallery state, presets, language files, thumbnail cache, backups, crash log). Copy or move the whole folder to move everything; delete it to remove everything; delete `portable.txt` to go back to the per-user folders.

What portable mode does not do: it registers no file types and no Explorer menu. `Choose-FileTypes.cmd`, `Register-FileTypes.cmd` and `Unregister-FileTypes.cmd` in the folder do that on request (they write to your user registry, which is then no longer "nothing changed").

The installed copy and a portable copy can run side by side, but the programs find each other by name: opening a picture from one while the other is already running may hand it to the one that is running.

## For packagers

`tools\package.ps1` builds everything: the five programs, the setup (which carries them, the language files, the licence and the readme inside itself) and the portable zip, checks that the zip holds the programs, `portable.txt`, the licence and the language files, and writes `SHA256SUMS.txt`. Signing is optional: set `NUVIEWER_SIGN_PFX` (and `NUVIEWER_SIGN_PASSWORD`) and every program and the setup are signed with `signtool.exe`.
