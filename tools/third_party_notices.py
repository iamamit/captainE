#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.txt: every third-party crate captainE builds with, its license, and the license
texts its authors ship (MIT/Apache/BSD/OFL require these to travel with the app). Run after dependency changes:
    python3 tools/third_party_notices.py
Reads `cargo metadata` and the crate sources cargo already downloaded; nothing is fetched."""
import json, os, pathlib, subprocess, collections

root = pathlib.Path(__file__).resolve().parent.parent
meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1"], cwd=root,
                                 capture_output=True, text=True, check=True).stdout)
ours = {p["id"] for p in meta["packages"] if p["source"] is None}
# only what ends up in the programs (normal and build dependencies), not test-only crates
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
shipped, todo = set(), [i for i in ours]
while todo:
    n = nodes.get(todo.pop())
    for d in (n or {}).get("deps", []):
        if any(k["kind"] in (None, "build") for k in d["dep_kinds"]) and d["pkg"] not in shipped:
            shipped.add(d["pkg"]); todo.append(d["pkg"])
pkgs = sorted((p for p in meta["packages"] if p["id"] in shipped and p["id"] not in ours),
              key=lambda p: (p["name"], p["version"]))
texts = collections.OrderedDict()   # license text -> crates using exactly that text
lines = ["captainE — third-party software notices", "=" * 44, "",
         "captainE is built with the open-source components below. Each is used under the license shown;",
         "the full license texts follow. captainE calls FFmpeg (ffmpeg/ffprobe) as a separate program that is",
         "installed on the computer; FFmpeg is not part of this distribution.", "",
         f"{len(pkgs)} components:", ""]
for p in pkgs:
    lic = p.get("license") or "see license file"
    lines.append(f"  {p['name']} {p['version']} — {lic}" + (f" — {p['repository']}" if p.get("repository") else ""))
    d = pathlib.Path(p["manifest_path"]).parent
    # license files at the top of the crate or one folder down (fonts keep theirs next to the font files)
    found = [f for f in d.iterdir() if f.is_file()] + [f for s in d.iterdir() if s.is_dir() for f in s.iterdir() if f.is_file()]
    for f in sorted(found):
        if f.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE", "OFL", "UFL")):
            t = f.read_text(errors="replace").strip()
            texts.setdefault(t, []).append(f"{p['name']} {p['version']} ({f.name})")
lines += ["", "", "License texts", "=" * 13, ""]
for t, users in texts.items():
    lines += ["-" * 78, "Used by: " + ", ".join(users), "-" * 78, t, ""]
out = root / "THIRD_PARTY_NOTICES.txt"
out.write_text("\n".join(lines) + "\n")
print(f"wrote {out} ({len(pkgs)} components, {len(texts)} distinct license texts)")
