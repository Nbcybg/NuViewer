# Video, sound, 360 and the other playback pieces (session 14)

## Pieces

| Piece | Where | Notes |
|---|---|---|
| Video + sound playback | `crates/nv-media` | Media Foundation source readers (video as BGRA, sound as 48 kHz stereo PCM) + waveOut. `Player` = one video thread + one sound thread, `Scrubber` = a second reader for the seek-bar pictures. |
| Late binding | `nv-media/src/load.rs` | `mfplat.dll`, `mfreadwrite.dll`, `winmm.dll` are loaded with `LoadLibrary` the first time a media file is opened. A program that links the crate pays nothing at start-up (NuQuick stays at its ~2 MB). |
| Poster pictures | `nv-core/src/posters.rs`, `nv-codecs/src/media.rs`, `wave.rs` | A WAV is drawn natively (`wave.rs`, streamed, big files sampled); other sound = decoded peaks; video = a frame near 10% in. Registered with `nv_core::posters::register()` by the viewer, NuGallery and NuQuick. |
| Professional clips | `nv-codecs/src/media.rs` | R3D / MXF / BRAW are recognised by header; a proxy (same name `.mp4`/`.mov`, a `Proxy` folder, `_proxy`) is played instead; without one the message says what to do. ffmpeg (`nv-core/src/ffmpeg.rs`) makes pictures of what Windows can't decode and a 720p playable copy in `%LOCALAPPDATA%\NuViewer\proxy` (cap 2 GB). |
| The bar | `nuviewer/src/controls.rs`, `media.rs` | One bar for video, sound, animation, image sequence, slideshow: play, step, seek track with a picture of the spot under the pointer, speed 1x/2x/4x/0.5x, loop, volume (video/sound). |
| 360 view | `nv-render/shaders/pano.hlsl`, `nuviewer/src/display.rs` | Equirectangular only. The mip is chosen from the field of view (derivatives jump at the atan2 seam). `Shift+P`; a picture 2:1 and >= 2000 px announces itself. |
| Dim, canvas | `nuviewer/src/display.rs`, `nv_core::prefs` | Dim = a black veil under the UI. Canvas blur = the same texture drawn from a small mip over the window (no extra pass, no blur shader). |

## Clock and sync

`Shared::now_ms()` is the wall clock times the speed; the sound thread measures where the sound card really is (every 250 ms) and
moves the clock when it is off by more than 90 ms. Video frames wait for the clock, frames more than 120 ms late are dropped (never
longer than 250 ms without showing one). A seek bumps a generation counter; both threads restart from the new position and the video
decodes forward from the key frame without showing the frames before the target.
Speed changes restart the sound segment (pitch follows the speed: it is resampled, not time-stretched).
A video with no usable sound device keeps playing silently.

## Not done / not verified

- Audible output was not heard: `waveOutGetNumDevs()` is 0 in the tool session that runs the tests here, so the sound path
  was only exercised up to "no device". Everything else (decode, clock, seek, scrub, frames on screen) ran on a real H.264/AAC file.
- Windows has no decoder for the user's Fuji X-T4 MOV files (10-bit 4:2:2): they play through the ffmpeg proxy.
- Real R3D / MXF / BRAW files: none on this PC, only the header detection is unit-tested.
- Time-stretched (pitch-preserving) sound at 2x/4x; frame-exact stepping in video.
- Subtitles, multiple audio tracks, HDR tone mapping.
