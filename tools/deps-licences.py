"""Writes docs/third-party/dependencies.csv: every crate NuViewer is built from, its version and its licence, taken from
`cargo metadata` (so it can be regenerated and compared at any time).

    python tools/deps-licences.py            (run from the repository root; needs cargo on the PATH)
"""
import csv
import json
import os
import subprocess
import sys

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--offline"], cwd=root, text=True, encoding="utf-8"))
members = set(meta["workspace_members"])
by_id = {p["id"]: p for p in meta["packages"]}
# Only what is really built into the programs: follow the dependency graph from the workspace members (normal + build deps).
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
seen, todo = set(), list(members)
while todo:
    i = todo.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] in (None, "build") for k in d["dep_kinds"]):
            todo.append(d["pkg"])
rows = sorted({(by_id[i]["name"], by_id[i]["version"], by_id[i].get("license") or "see its repository", by_id[i].get("repository") or "") for i in seen if i not in members})
out = os.path.join(root, "docs", "third-party", "dependencies.csv")
with open(out, "w", encoding="utf-8", newline="") as f:
    w = csv.writer(f, lineterminator="\n")
    w.writerow(["crate", "version", "licence", "repository"])
    w.writerows(rows)
print(f"{len(rows)} crates -> {out}")
no_net = [r[0] for r in rows if r[0] in {"reqwest", "ureq", "hyper", "curl", "isahc", "attohttpc", "surf", "tokio", "native-tls", "rustls", "openssl", "h2", "minreq", "tungstenite", "websocket", "socket2", "mio"}]
print("network crates among them:", no_net or "none")
sys.exit(1 if no_net else 0)
