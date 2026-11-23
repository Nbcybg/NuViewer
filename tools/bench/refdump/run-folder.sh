#!/bin/sh
# run-folder.sh <folder> <out summary file> [fracs]: compare every .skel under a folder with spine-core 4.1.55, one line per file.
# Quiet on purpose: prints only files that differ or that the reference cannot read (4.2/4.3 files). Temp dumps are deleted at once.
SB=${SB:-T:/cargo-target/NuViewer/release/examples/spinebench.exe}
REF=${REF:-C:/Users/noobc/AppData/Local/Temp/claude/6a817ca287ada8b4/bd4bafaf-9e64-4337-9239-f55944b81493/scratchpad/ref}
TMP=${TMP:-/t/spine-cmp/tmp}
FR=${3:-0.37}
mkdir -p "$TMP"
: > "$2"
ok=0; wrong=0; refail=0; ourfail=0
find "$1" -name "*.skel" | while read -r f; do
  n=$(basename "$f" .skel)
  if ! (cd "$REF" && node refdump.mjs "$f" "$TMP/r.txt" "$FR" >/dev/null 2>"$TMP/e.txt"); then
    echo "REFFAIL $f $(head -c 80 "$TMP/e.txt" | tr '\n' ' ')" >> "$2"; continue
  fi
  if ! "$SB" "$f" --batch "$TMP/o.txt" --fracs "$FR" >/dev/null 2>"$TMP/e2.txt"; then
    echo "OURFAIL $f $(grep -m1 'load:' "$TMP/e2.txt" | head -c 120)" >> "$2"; continue
  fi
  r=$(node "$REF/cmp.mjs" "$TMP/o.txt" "$TMP/r.txt" 0 | tail -1)
  case "$r" in
    *"DRAWN WRONG 0/"*) echo "OK $f" >> "$2" ;;
    *) echo "DIFF $f :: $(echo "$r" | cut -c1-150)" >> "$2" ;;
  esac
done
echo FINISHED >> "$2"
