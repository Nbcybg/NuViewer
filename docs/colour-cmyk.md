# CMYK pictures and rendering intent

Session 9. Everything here is about **how CMYK reaches the screen**: nothing is converted to RGB in a decoder any more.

## The rule

A decoder that finds CMYK hands over the **four ink amounts, raw**: an `Image` with `cmyk = true` whose RGBA buffer holds
C, M, Y, K in bytes 0..3 (0 = no ink, 255 = full ink) and `icc` = the file's CMYK profile if it has one. The conversion happens
once, in `nv_codecs::cmyk`, when the picture is finished for the screen (`apply_display_colour`), for a thumbnail
(`apply_icc_assumed`) or for a file / the clipboard (`apply_icc`). `downscale` averages raw inks ink by ink, so thumbnails of
CMYK pictures are made from the inks too. `Image::cmyk` is cleared by the conversion; nothing displays or saves a buffer that
still has it set.

## Which profile

1. The picture's own CMYK profile (JPEG APP2, PSD image resource 1039, TIFF / WIC colour context, JP2 ICC box).
2. If it has none (or the profile can't be used): what the user chose in **Settings > CMYK & RAW**: Coated FOGRA39
   (built in, the default), SWOP (`RSWOP.icm`, the copy Windows ships) or a `.icc` / `.icm` file. The viewer **says so**:
   a message when the picture opens ("CMYK: no embedded profile, assuming "Coated FOGRA39 (ISO 12647-2:2004)", relative
   colorimetric - change it in Settings > CMYK & RAW"), "CMYK profile assumed" in the top bar, and the `Processing` row of the info
   panel (`I`). An embedded profile also gets a `Processing` row ("CMYK, profile "X" (embedded), ...").
3. Colour management switched off: the old quick formula `(1-C)(1-K)`, so a CMYK picture is still a picture.

The built-in FOGRA39 is the profile that is embedded (as the output intent) in the "free CMYK chart" PDF the user gave; the
file is `crates/nv-codecs/profiles/CoatedFOGRA39.icc` and `tests/cmyk_fogra39.rs` checks that it is byte for byte the one in
that PDF (when the PDF is on the machine).

## The colour engine: moxcms does support CMYK

Checked in the source (moxcms 0.9.2) and by test: CMYK sources work through the profile's A2B tables (`Layout::Rgba` carries
the four inks), perceptual / relative colorimetric / saturation. What it lacks, and what is done in `cmyk.rs`:

* **Black point compensation** for CMYK sources (its `detect_black_point` returns nothing for CMYK) and
* **absolute colorimetric** (it ignores the intent for LUT profiles).

Both are done in floating point on linear light (CMYK -> linear output primaries, unclipped), then back through the output's
curve. BPC takes the profile's black point (all four inks through the relative table, made neutral) out of the light and
stretches the rest to white (what LittleCMS does for relative colorimetric); absolute scales the light by the paper's
media white point against D50. RGB profiles are matrix / shaper profiles with black = 0, so BPC has nothing to do for them.
No other library was needed; no new dependency.

Settings (`settings.ini`): `cmyk = fogra39 | swop | file`, `cmyk_file`, `intent = relative | perceptual | saturation |
absolute`, `bpc = true | false`. Defaults: FOGRA39, relative colorimetric, BPC **off** - this is what ColorSync does, and it
is what reproduces the user's table (below). The intent applies to every profile conversion; BPC to CMYK pictures under relative
colorimetric intent. Changing any of them reloads the open pictures; the thumbnail cache key includes all of them.

## Adobe's inverted CMYK

* **JPEG** (`jpeg.rs`): the Adobe APP14 marker's *presence* means the samples are stored inverted (255 = no ink, which is how
  Photoshop and everything made for it writes CMYK / YCCK JPEGs); the decoder inverts them back. Without the marker the samples
  are plain ink amounts. YCCK (marker transform 2) is turned back into CMYK first: the first three components are YCbCr of the
  *inverted* C, M, Y, K is stored as is. zune-jpeg is asked for the raw components (it can do CMYK -> CMYK and YCCK -> YCCK
  without converting). Tests: a Pillow-made Adobe CMYK JPEG, a YCCK round trip through `jpeg-encoder`, and a marker-less file.
* **PSD / PSB** (`psdraw.rs`): Photoshop keeps CMYK inverted in the merged image; inverted back to inks. The `psd` crate is no
  longer used for CMYK documents (it converts without a profile). The ICC profile is read from image resource 1039 for RGB and CMYK.
* **TIFF** (WIC): the native 32 bpp CMYK pixel format is copied as it is (TIFF "separated": 0 = no ink); WIC's own
  CMYK -> RGB conversion is no longer used. The frame's colour context (embedded profile) is read for CMYK **and RGB** TIFFs
  (RGB TIFFs used to ignore their profile).
* **JPEG 2000** (`jp2.rs`): four colour channels (no alpha) are CMYK ink amounts, with the ICC profile if the file names one.
  **Not tested with a real CMYK JP2** (no encoder here); the polarity is assumed to be ink amounts.

## PDF, AI, EPS: not through the profile

PDF and PDF-compatible `.ai` pages are drawn by `Windows.Data.Pdf`, which returns finished RGB; EPS goes through Ghostscript
or the file's preview. Their CMYK never reaches our engine, and they ignore the PDF's output intent. Measured with one-rectangle
PDFs (device CMYK fills): Windows gives C100 -> (0,158,213), M100 -> (231,37,130), K100 -> (24,23,23); FOGRA39 -> sRGB gives
(0,160,228), (230,12,128), (43,43,42). So Windows uses its own CMYK conversion, close to but not the same as FOGRA39. Doing
better needs our own PDF renderer (a different, large job); not done.

## Validation (the user's ColorSync table, and an independent engine)

`crates/nv-codecs/tests/cmyk_fogra39.rs`; the profile comes from the PDF. LittleCMS 2 (through Pillow's ImageCms) is the
independent reference for 12 patches x every intent.

| patch | ColorSync (user) | NuViewer, relative, no BPC | LittleCMS, same |
|---|---|---|---|
| C100 | ~0, 160, 227 | 0, 160, 228 | 0, 160, 228 |
| M100 | ~229, 13, 128 | 230, 12, 128 | 230, 12, 128 |
| dark rich black (C60 M40 Y40 K100) | ~29, 33, 33 | 29, 33, 33 | 30, 33, 33 |

("Dark rich black" was taken to be C60 M40 Y40 K100: it is the patch that gives 29/30, 33, 33 - a pure K100 gives 43,43,42 and
all four inks at 100% give 28,27,24.) The user's values match **relative colorimetric without black point compensation**.

Worst difference against LittleCMS over the 12 patches (8-bit levels): perceptual 1, relative 1, saturation 1, relative + BPC 4
(only in the near-black patches), absolute 12 (saturated out-of-gamut patches such as yellow: LittleCMS keeps them unclipped a
little longer than moxcms does; the paper white agrees to within 2).

## Speed (12 MP, all cores, this PC)

| conversion | time |
|---|---|
| Display P3 -> sRGB (RGB, matrix profile) | ~9 ms (+ the buffer copy of the benchmark) |
| CMYK -> sRGB, relative / perceptual (8-bit table) | ~33 / 38 ms |
| CMYK, relative + BPC (floating point) | ~105 ms |
| CMYK, absolute (floating point) | ~145 ms |
| building the transform from the 650 KB FOGRA39 profile | 10-16 ms (cached for the last 6 profiles) |

`cargo run --release -p nv-codecs --example bench_colour`.
