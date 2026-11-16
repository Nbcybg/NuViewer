#!/usr/bin/env python3
"""Collects the English phrases the programs show (labels, buttons, check boxes, menu entries, help lines, messages) and writes
them into crates/nv-text/lang/en.txt, under the "phrases" marker, as `phrase = phrase`. A translator copies the file, changes
what is after the `=`, and every window, menu and message that goes through nv_text::tr() follows.

  python tools/lang-extract.py          rewrite the phrases section of en.txt
  python tools/lang-extract.py --check  list literals in UI calls that are not in en.txt yet (exit 1 if there are any)
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EN = ROOT / "crates" / "nv-text" / "lang" / "en.txt"
MARK = "# ---- phrases written in the code (the English text is the key) ----"
CALLS = re.compile(r"\.(label|label_id|button|check|group|combo|tab|side_tab)\(|Entry::(item|checked|enabled)\(|action_entry\(|\bb\.(item|checked|sub)\(|\btoast\(|\.say\(|Item::(item|checked)\(|\bmessage\(")
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')
FILES = [p for d in ("nuviewer/src", "nugallery/src", "nueditor/src", "nuimporter/src", "nuquick/src", "crates/nv-win/src") for p in (ROOT / d).rglob("*.rs")]
FILES += [ROOT / "crates/nv-core/src/keys.rs", ROOT / "crates/nv-editor/src/recipe.rs"]


def usable(s: str) -> bool:
    if len(s) < 3 or s != s.strip() or "�" in s or not re.search(r"[A-Za-z]{2}", s):
        return False
    if any(c in s for c in "{}=\\\n") or s.startswith(("http", "--", ".")):
        return False
    if re.fullmatch(r"[A-Za-z0-9_.]+", s) and "." in s:  # keys, file names
        return False
    return True


def collect() -> list[str]:
    found: dict[str, None] = {}
    for f in FILES:
        text = f.read_text(encoding="utf-8", errors="replace")
        in_help = False
        for line in text.splitlines():
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if re.search(r"\b(const|static) [A-Z_]*(HELP|LABELS|ITEMS|NAMES)[A-Z_]*\b", line):
                in_help = True
            use = bool(CALLS.search(line)) or in_help or (f.name == "keys.rs" and "a!(" in line) or (f.name == "recipe.rs" and re.match(r"\s+[A-Z][A-Za-z]+, [A-Za-z]+, \"", line))
            if use:
                for m in LITERAL.finditer(line):
                    s = m.group(1)
                    if usable(s):
                        # "Label\tCtrl+X": the label is the phrase
                        found.setdefault(s.split("\\t")[0], None)
            if in_help and (stripped.endswith("];") or stripped == "];"):
                in_help = False
    return sorted(found, key=str.lower)


def existing_keys() -> set[str]:
    keys = set()
    for line in EN.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            keys.add(line.split("=", 1)[0].strip())
    return keys


def main() -> int:
    phrases = [p for p in collect() if "\\t" not in p]
    head, _, _ = EN.read_text(encoding="utf-8").partition(MARK)
    have = {l.split("=", 1)[0].strip() for l in head.splitlines() if "=" in l and not l.strip().startswith("#")}
    phrases = [p for p in phrases if p not in have]
    if "--check" in sys.argv:
        known = existing_keys()
        missing = [p for p in phrases if p not in known]
        for p in missing:
            print(p)
        print(f"{len(missing)} phrases are not in en.txt yet", file=sys.stderr)
        return 1 if missing else 0
    body = "\n".join(f"{p} = {p}" for p in phrases)
    EN.write_text(head.rstrip("\n") + "\n\n" + MARK + "\n" + body + "\n", encoding="utf-8")
    print(f"{len(phrases)} phrases written to {EN.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
