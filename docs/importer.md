# NuImporter (version 0.2.0)

One small window that brings pictures in from wherever they are, and then does what you chose with them.

| Source | How | Notes |
|---|---|---|
| **Scanner** | "Scanner…" runs Windows' own scan window (WIA, through PowerShell's `WIA.CommonDialog`) | Not tried on real hardware here (no scanner on the test PC); the script is unit-tested, the dialog is Windows'. |
| **Screenshot** | "Screenshot: drag a region" / "whole screen". The window hides, the screen is captured, you drag a rectangle (Enter = everything, Esc / right button = cancel). **NuQuick** listens for **Ctrl+Print Screen** (region) and **Ctrl+Alt+Print Screen** (whole screen) from anywhere and starts `nuimporter.exe --shot region|full` | Registered hot keys (no keyboard hook, nothing runs while idle). If another program owns the key the registration just fails. Switch off in Settings > Import. |
| **Clipboard** | A picture (PNG entry with alpha first, else a 24/32-bit bitmap), or files copied in Explorer | |
| **Camera, cable** | "Camera: take a picture" asks the first WIA camera to shoot and transfers the new picture (falls back to Windows' camera dialog) | WIA only: cameras that expose PTP to Windows. Not tried on real hardware. |
| **Camera, cable or Wi-Fi (tether)** | "Camera: watch a folder": let the camera's own software (Canon EOS Utility, Fujifilm X Acquire, a Wi-Fi/FTP app, a card reader ...) write into a folder; every new picture whose size has stopped changing is imported | Tested end to end. |
| **A file with pictures inside** | PowerPoint / Word / Excel / OpenDocument / zip / cbz / epub / Pages / Keynote (everything with pictures in a zip), and **ISO** disc images (ISO 9660 + Joliet read natively; UDF-only images through 7-Zip when it is installed) | Pictures keep their own bytes when the format is "As it was". |
| **A folder** | All pictures of a folder | |

## What happens next (Settings > Import, or the boxes in the window)

* **Save it at once** in the import folder (default `Pictures\NuViewer Imports`) with the name pattern `{source} {date} {time} {n}`.
* **Open it in NuEditor first** (`nueditor.exe <temp file> --import <source>`): edit, then closing asks *Yes = save in the import folder as the chosen format, No = don't keep it, Cancel = keep editing*. With several pictures at once it asks once whether to save them all.
* **Put it on the clipboard** (nothing is saved).
* Formats: PNG, JPEG (quality), WebP (lossless), BMP, TIFF, QOI, TGA, or "As it was".
* "Also open the saved picture in NuEditor".

NuEditor itself got the same formats for "new file" / "save a copy" (BMP, TIFF, TGA, QOI added), Ctrl+C (the edited picture to the clipboard) and the import mode above.

Back (`‹`) goes to NuGallery, Close (`✕`) closes.
