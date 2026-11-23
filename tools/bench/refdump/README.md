# Spine reference check: NuViewer against spine-core 4.1.55

The user's rule is "match BD2-L2D-Viewer" (it runs `@esotericsoftware/spine-player` 4.1.55). These scripts pose every animation of
every skeleton in a folder with spine-core 4.1.55 (idle on track 0, `mixN_*` on track 1, the skin BD2 would pick) and compare it
with `spinebench --batch`, which does the same with NuViewer's own code.

```
npm i @esotericsoftware/spine-core@4.1.55        # next to refdump.mjs (any folder, copy the .mjs files there)
cargo build --release -p nv-codecs --example spinebench
node refdump.mjs <file.skel> ref.txt              # reference poses (needs <file>.atlas next to the skel; textures are not read)
spinebench <file.skel> --batch ours.txt           # NuViewer's poses
node cmp.mjs ours.txt ref.txt                     # "DRAWN WRONG n/N": samples where a slot's vertex box moved > 3 units,
                                                  # or an attachment / colour / draw order differs (bones listed separately)
node first.mjs <file.skel> ours.txt ref.txt <anim> [n] [threshold]   # first bones (parents first) that differ, with names
node inspect.mjs <file.skel> <bone>               # constraints that touch a bone or its parents
```

`run-all.sh <folder>` does the loop. Sample times are 0.137 and 0.613 of each animation: round fractions land on key times
and flip an attachment by float noise.

State on 2026-11-24 for `Brown-Dust-2-Assets-main/assets/spine/interaction` (23 files, ~4000 samples): 21 files draw identically to
BD2 in every sample; the other two differ in 3 samples each (13 and 7 units): a 1-bone IK whose target is its own child, and a
bone squashed to scale 0 (its direction is float noise).

Things the runtime does that are easy to "fix" by mistake (all matched on purpose):
Bezier curves are a 10-segment polyline, not the exact curve; `updateAppliedTransform` has no special case for OnlyTranslation (4.1);
a constraint that moves a bone's world matrix does not rebuild the bone itself; deform timelines only act while their attachment is
the one shown; the animation applied last replaces a slot's deform; colour channels are clamped to 0..1.
