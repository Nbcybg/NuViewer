# NuViewer error codes

When something goes wrong NuViewer shows a short code, `NV-xxxx`, together with what happened and what to try. The code is on the
picture area when a picture cannot be opened (press `I` for the details), on the end of a failed toast message ("Couldn't delete: ... [NV-7002]"),
in the window title, and in `%LOCALAPPDATA%\NuViewer\crash.log` for internal errors. Quote the code when you report a problem.

The first digit is the family: 1xxx file access, 2xxx format, 3xxx contents, 4xxx limits, 5xxx graphics, 6xxx the program itself, 7xxx file operations, 8xxx colour, 9xxx anything else.

| Code | Meaning | What to do |
|---|---|---|
| NV-1001 | File not found | The file was moved, renamed or deleted after the folder was listed. Reopen the folder. |
| NV-1002 | Access denied or file in use | Another program has the file open or you may not read it. Close the other program or check the file's permissions. |
| NV-1003 | Reading the file failed | The disk, network share or phone storage stopped answering. Check the connection and try again. |
| NV-1004 | The file is empty | The file has no data (a copy or download that did not finish). |
| NV-2001 | Unsupported file type | NuViewer has no decoder for this format. Open it with another program (Right click > Open with). |
| NV-2002 | Windows codec missing | This format needs a Windows extension: HEIC/HEIF needs "HEIF Image Extensions" and "HEVC Video Extensions", AVIF needs "AV1 Video Extension" (Microsoft Store). |
| NV-2003 | Unsupported variant of the format | The file uses a compression, bit depth or colour mode that is not supported yet. |
| NV-2004 | Needs a component that is not installed | Part of the file needs an extra program (for example Ghostscript for PostScript without a preview). |
| NV-3000 | The file could not be decoded | The decoder refused the file. It may be damaged, or not really this format. |
| NV-3001 | The file is damaged or incomplete | The data ends early or does not add up. Try the original, or download or copy it again. |
| NV-3002 | Not a valid picture of this type | The header is wrong: the extension probably does not match what is inside. |
| NV-3003 | Document or drawing could not be rendered | The PDF, SVG, metafile or 3D model has content that the renderer cannot draw. |
| NV-4001 | Picture too large | More pixels than NuViewer will decode (250 megapixels). Open it in a program made for huge pictures. |
| NV-4002 | Out of memory | There is not enough free memory for this picture. Close other programs and try again. |
| NV-4003 | More than the graphics card can show | The picture is bigger than the graphics card's texture limit; it is shown reduced when possible. |
| NV-5001 | Sending the picture to the graphics card failed | The graphics driver refused the upload. Update the driver, or try again. |
| NV-5002 | Graphics device unavailable | Direct3D could not start. Update the graphics driver; a remote session or a very old GPU may not be supported. |
| NV-6001 | The decoder crashed on this file | A bug in a decoder was caught and the viewer carried on. The file is probably damaged; please report it if it opens elsewhere. |
| NV-6002 | Internal error | A bug in the viewer (it closes cleanly; details are in %LOCALAPPDATA%\NuViewer\crash.log) or a decoder returned data of the wrong size. Please report it, with the file if there was one. |
| NV-6003 | Thumbnail could not be made | The picture could not be read for the gallery; it may still open in the viewer. |
| NV-7001 | Rename failed | The name is already used, contains a character Windows does not allow, or the file is in use. |
| NV-7002 | Delete failed | The file is in use or protected. Close the program that has it open. |
| NV-7003 | Move or copy failed | The destination is read-only, full, or has a file of that name that is in use. |
| NV-7004 | Saving the picture failed | The folder is read-only or the disk is full. The original is untouched. |
| NV-7005 | Undo failed | What the undo needs (the file or its backup) is no longer there. |
| NV-7006 | Clipboard failed | Another program is holding the clipboard. Try again. |
| NV-7007 | Settings not saved | The settings file could not be written (%APPDATA%\NuViewer\settings.ini). |
| NV-8001 | Colour profile problem | The ICC profile could not be read or used; the picture is shown without conversion. |
| NV-9999 | Unexpected error | Something unexpected happened. The text after the code says what. |

The table lives in `crates/nv-codecs/src/errors.rs` (`TABLE`); a unit test checks that this page lists every code.
