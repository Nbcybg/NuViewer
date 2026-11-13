# Camera RAW: developed from the sensor data

Session 9. The main way a RAW file is shown is now **developed from its sensor data**, not the JPEG preview inside it (that is
the camera's own rendering: its tone curve, picture style, sharpening).

## What is applied - only what the camera wrote into the file

* black level (per photosite; X-Trans 6x6 tiles too) and white level (per colour),
* the as-shot white balance (the camera's multipliers; clipped channels stay clipped, no highlight recovery),
* the camera's colour matrix (DNG-style XYZ -> camera). With two illuminants (A and D65, which every camera here gives) the
  matrix is interpolated in reciprocal temperature at the colour temperature of the as-shot white balance, solved the way the
  DNG specification describes. The old code took `HashMap::values().next()`: whichever illuminant came first, so the colours
  could differ from run to run. Deterministic now (unit-tested),
* the sRGB transfer curve.

Removed: the automatic exposure (log-average luminance -> 0.22, gain 1..6x) and the extended-Reinhard highlight roll-off. A
mid-grey photosite value comes out as exactly the sRGB value of that linear level (unit-tested: 50% of the range -> 188).
The result is darker and flatter than the camera JPEG, by design: Fuji X-T4 files here come out ~2.5 stops under the camera
JPEG (mean 28 against 75 for the same shot), Canon 60D ~1.2 stops, Sony A7 III DNG about equal to Adobe's preview.
The info panel (`I`, row **Processing**) states what was done: "RAW developed from the sensor data (full size), 6240x4160: as-shot
white balance, camera colour matrix interpolated for 5230 K, sRGB curve; no exposure or tone adjustment".

## Modes (Settings > CMYK & RAW, key `raw_mode`)

| mode | what | when |
|---|---|---|
| `full` (default) | every photosite a pixel: Malvar-He-Cutler demosaic (Bayer), 3x3 same-colour average (X-Trans), all cores | the picture you look at |
| `half` | 2x2 Bayer / 3x3 X-Trans blocks binned, no demosaic | faster, a quarter / a ninth of the pixels |
| `preview` | the camera's embedded JPEG (partial read, as before) | fastest |

A file that can't be developed (4-colour sensors, floating-point DNG, damaged data) falls back to its embedded preview, then WIC,
as before. `NUVIEWER_RAW=full|half|preview` overrides the setting (timing, comparing); `NUVIEWER_NO_DEVELOP=1` still means
preview; `NUVIEWER_RAW_DEBUG=1` prints why a file was not developed.

**Thumbnails** (gallery, film strip) keep using the embedded preview unless `raw_thumbs = true` (then: half-size development).
Reason: development reads the whole file; on the user's F: drive (~37 MB/s) a 59 MB Fujifilm RAF costs 1.6 s of reading
(measured: 1602 ms for the first read), where the partial read of the preview JPEG costs 0.4-0.5 s. A folder of 500 RAFs would take
13 minutes instead of 3.5. The gallery therefore shows the camera's rendering while the viewer shows the sensor's; switch the
setting if the two must match.

RAW files are now read with `progress::read_file`, so the loading panel shows the reading, a cancelled load stops at the next
chunk, and the HDD gating of the loader applies; rawler decodes from memory.

## Time, before and after (this PC, warm file cache, CPU time only; min of 5; `bench_decode`)

| file | before: embedded preview (what was shown) | after: developed `full` | after: `half` |
|---|---|---|---|
| Canon EOS 60D CR2, 5184x3456 | 67 ms (the camera's 5184x3456 JPEG) | 349 ms | 223 ms |
| Fujifilm X-T4 RAF, 6240x4160 | 81 ms (4416x2944 JPEG) | 273 ms | 123 ms |
| Sony A7 III DNG, 6000x4000 | 169 ms (preview 1024x683 too small, so half development 3000x2000 with auto exposure) | 332 ms | 169 ms |

Where it goes (`full`): rawler decoding the sensor data 147 / 17 / 77 ms (the Fuji's is uncompressed), development 190 / 222 /
249 ms. A first version took 373 / 305 / 340 ms; taking the clamping out of the demosaic taps (the crop is kept MARGIN photosites
inside the sensor, so every tap is inside the buffer) gave the rest. Before this session FastStone needed 2.3-2.4 s warm for
the same RAF (HANDOFF §5), so the viewer is still several times faster, but a RAW now costs a few hundred ms instead of tens.

Cold reads add the file size over the disk speed in every mode except `preview` (which reads only the preview).

## Checks done

* flat colours stay flat through both demosaics and the binned paths (synthetic mosaics, Bayer and X-Trans), black stays 0 and a clipped
  channel stays 255, black / white levels and white balance are applied, a gradient comes out monotonic and grey,
* full against half on real files: the full picture averaged down to the half-size picture differs by an RMS of 1.4-4.1 levels, the
  mean colours by less than 0.3 level (`examples/rawcheck.rs`),
* colour against the camera's own JPEG, channel ratios only (the tone curve differs): Canon R/G 0.99 against 0.97 and B/G 0.96
  against 0.93; Fujifilm B/G 0.69 against 0.81 (the camera's film simulation is bluer in the shadows); Sony within 2%.
  There is no reference raw converter on this PC, so "correct" here means "what the file's metadata says", not a match with any
  program.

Dev tools: `cargo run --release -p nv-codecs --example rawinfo -- <file>` (levels, white balance, matrices),
`--example bench_decode -- --raw full,half,preview <file>`, `--example rawcheck -- <file>`.
