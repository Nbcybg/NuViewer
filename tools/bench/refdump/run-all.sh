#!/bin/sh
# run-all.sh <folder with .skel files> [out dir]: compare NuViewer's poses with spine-core 4.1.55 (the runtime BD2-L2D-Viewer uses)
# needs: npm i @esotericsoftware/spine-core@4.1.55 next to refdump.mjs (REF dir), spinebench built (SB)
SB=${SB:-T:/cargo-target/NuViewer/release/examples/spinebench.exe}
REF=${REF:-$(dirname "$0")}
OUT=${2:-/t/spine-cmp}
mkdir -p "$OUT"
find "$1" -name "*.skel" | while read -r f; do
  n=$(basename "$f" .skel)
  [ -f "$OUT/$n.ref.txt" ] || (cd "$REF" && node refdump.mjs "$f" "$OUT/$n.ref.txt" >/dev/null)
  "$SB" "$f" --batch "$OUT/$n.our.txt" 2>&1 | tail -1
  echo "== $n ($f)"
  node "$REF/cmp.mjs" "$OUT/$n.our.txt" "$OUT/$n.ref.txt" "${3:-3}" | cut -c1-200
done
