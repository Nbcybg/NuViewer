# NuViewer: 100 simulated users — why use it, why not, what they use now (2026-11-18)

Method: the `founder-consumer` skill (E:\founder-skill-main) — 100 sub-agents, each one a dealt persona (segment, age, income,
adoption behaviour, the viewer they use today, a first doubt). Every one saw the same pitch, built only from NuViewer 0.4.1's README
and `docs/benchmark-compare-2026-11-17.md` (speed numbers named as "the developer's own benchmark", limits listed). Seed 2611.
Raw data: `docs/founder-panel-2026-11-18/` (`answers/P001..P100.json`, `personas.json`, `results.md`, the pitch and customer profile).
Tool: the skill's `panel.py`, wrapped (`panel_nu.py`, kept in the same folder) to add "app used today", a try-vs-default funnel and
a switch decision; the skill's own files are untouched.

**These are simulated buyers, not customers.** Use the shape of the answers (which objections, which segments), not the percentages.

## Headline

| step | people (of 100) |
|---|---:|
| would install and try it (free, portable zip) | **90** |
| would make it the default viewer for a month | **11** |
| of those: replace the old viewer / default but keep the old one too | 8 / 3 |
| use alongside the old viewer (not default) | 58 |
| try, then drop after a few days | 24 |
| ignore it (will not even install) | 10 |

Days opened in the first month: 5.9 on average over all 100; 18.9 for the 11 who adopt.

## Who adopts, who does not

By viewer used today (default / of N):

| today | default | note |
|---|---:|---|
| Windows Photos | 5 / 20 | cannot open TGA/DDS/EXR/HDR/PSD/3D, slow |
| FastStone | 2 / 12 | stutters on rapid next in big folders |
| Adobe Bridge/Lightroom/Photoshop as a viewer | 2 / 8 | slow to launch just to look; catalogue stays there |
| Explorer, no viewer chosen | 1 / 7 | |
| ImageGlass | 1 / 8 | opens fewer formats |
| IrfanView | **0 / 17** | 12 use it alongside, 3 ignore it |
| XnView MP | **0 / 15** | metadata + batch tool, 7 try-then-drop |
| other fast viewers (Honeyview, qView, nomacs...) | 0 / 8 | |
| specialist tools (Blender, VLC, darktable) | 0 / 5 | |

By segment: 3D / game artists 3/12 (best), power users 2/13, digital artists 2/17, photographers 2/20, office/home 2/28, comic/archive readers 0/10.
By behaviour: speed-obsessed **7/15**, feature maximalist 2/10, format hunters 2/12; set-and-forget 0/20, minimalists 0/10 (7 drop it), trust-cautious 0/15 (10 ignore it),
early adopters 0/10 (9 "alongside" — they try everything and keep nothing), workflow-locked pros 0/8.

## Why they would use it (the 11 who adopt: 6 "need", 5 "speed")

1. **Formats their current app cannot open** — TGA, DDS, EXR, HDR, PSD with alpha, GLB/OBJ/FBX, cbz in one lightweight viewer (26 of 32 test files vs 19/17/13). Mentioned by ~60 people.
2. **Next/back and rapid-next speed** (85 ms vs 600-900 ms for 5 quick nexts). ~90 people mention speed, but only the speed-obsessed switch for it alone; a few tens of ms on a single open means nothing to most.
3. Replacing a heavy app (Bridge, Photoshop) just to glance at a file, plus the Space-bar preview and gallery/batch convert in one free app.
4. Cost of trying is zero (free, per-user, portable zip, no admin) — this is why 90 try.

Quotes: "26 of 32 types vs ImageGlass's 17 is exactly the kind of gap that makes me switch" (P011) · "next/back at 40-50 ms ... is exactly what culling is" (P029) ·
"It opens none of my work formats: no TGA, DDS, EXR, HDR or any 3D model" (P087, about Windows Photos).

## Why they do not use it (the other 89: trust 41, habit 39, need 9)

| objection | people mentioning it (keyword count, overlaps) |
|---|---:|
| one developer, version 0.4.1, will it be maintained? | 99 |
| unknown .exe: not stated signed / open source / no independent reviews | 47 (all 10 who refuse to even try) |
| habit: shortcuts, plug-ins, file associations, "Set default" done by hand | 49 |
| "I would not feel a few tens of ms" / the benchmark is the developer's own, one PC | 31 |
| no plug-ins, no community, nobody to ask | 36 |
| catalogue / XMP / ratings / metadata / culling marks not listed | 34 |
| does too much (gallery, editor, importer, tray) — wants view-only | 17 |
| RAW and JPG always separate = double the pictures to cull | 27 (some like it, some do not) |
| no comic mode shown (right-to-left, double page, resume) | 21 |
| Windows only (also on Mac/Linux) | 17 |
| printing a relative's photo not mentioned | 14 |
| HEIC needs Microsoft's HEVC extension | 11 |

Why IrfanView and XnView users stay (0 of 32 adopt): their setup works, XnView doubles as metadata/batch tool, IrfanView has plug-ins and shortcuts; "nothing bugs me".
Quote: "I'd also have to relearn shortcuts and redo file associations by hand ... for no gain" (P070) · "a single developer at 0.4.1 might not be maintained next year" (many).

## What would flip a no (most repeated)

1. Code signing, published source, independent reviews, a changelog showing a year of regular releases (the single biggest lever, cheapest to fix).
2. Their own files opening correctly on the first try (alpha, DDS channels, EXR exposure, RAW colour vs Lightroom/Camera Raw).
3. XMP ratings/labels and cull marks that Bridge/Lightroom/XnView see; proof originals and sidecars are never touched.
4. A viewer-only mode with gallery/editor/importer/tray agent switched off.
5. A visible comic mode, a print command, HEIC out of the box.

## What to do with this (checked against the repo)

- **Trust first.** Sign the installer, publish source or at least a public repo + changelog + SHA-256/VirusTotal link, name a licence. (Cargo.toml has no licence field; I did not check whether the installer is signed. The pitch said nothing about either, so the panel assumed "not stated = no".)
- **Fix the page, not only the app.** Several asks already exist but the pitch did not say so: read mode `B`/`Shift+B`/`Ctrl+B` for comics (README), print in the NuGallery menu. Put them in the first screen.
- **Real gap:** XMP rating/labels for culling (still in the backlog in the project notes). 34 photographers/artists named it; it decides whether Lightroom/Bridge/XnView users go from "alongside" to "default".
- **Benchmark credibility:** 31 distrust a self-run benchmark. Publish the harness (`tools/bench/compare`) and ask others to reproduce it.
- **Aim at the right people:** Windows Photos / Explorer users who hit unsupported formats, FastStone users bothered by rapid-next lag, and 3D/game artists. Do not spend effort converting IrfanView/XnView loyalists.
- **Optional viewer-only switch** would convert some minimalists (0/10 now; 7 drop it for "does too much").
- RAW+JPG stays separate (project rule); the panel only shows people both like it for culling and dislike the doubled count — explain it, do not change it.

## Price (hypothetical; NuViewer is free)

84 of 100 named a price at which a paid viewer would feel expensive: median $15, median "would not buy" $30. 16 would never pay for a viewer. Free + optional support link fits.

## Honest limits

- Simulated people from one language model. The 90% "would try" is inflated (trying a free zip is cheap); the 11% default rate is a rough shape only, in either direction.
- I did not show you the customer profile before dealing (the skill suggests it). Segment and current-app shares are my assumptions, based on web roundups
  ([ElevenForum thread](https://www.elevenforum.com/t/best-picture-viewer-file-management-app.33698/), [gummysearch Reddit summary](https://gummysearch.com/tools/best-products/image-viewer/), [laptops251 2026 guide](https://laptops251.com/best-photo-viewer-windows-11)), not on measured market share.
- Several agents wrote complaints about their current app that the brief did not give (they said so); treat "pains" as persona voice, not evidence about IrfanView/XnView/FastStone.
- Theme counts come from keyword matching on the free text, so people are counted under several themes.
- Confirm with real people: a landing page with download counts, 10 interviews with IrfanView/XnView/Photos users, and a post in a viewer thread asking "what stops you switching".
