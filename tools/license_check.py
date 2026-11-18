#!/usr/bin/env python3
"""Fail when a library that ships in captainE has a license outside the allowed list (CONTRIBUTING.md, rule 3).

    python3 tools/license_check.py

Only normal and build dependencies count; test-only crates are not shipped. A crate offered under several
licenses ("MIT OR GPL-2.0") passes when one choice is allowed. Run in CI on every push.
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "0BSD",
    "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "BSL-1.0", "Unlicense", "CDLA-Permissive-2.0",
    # file-level copyleft: fine while the crate is used unmodified
    "MPL-2.0",
    # bundled font licenses (free to ship with their text)
    "OFL-1.1", "Ubuntu-font-1.0",
}
# reviewed one by one; add a crate here only with a note in docs/19-ip-and-license-audit.md
REVIEWED = set()


def allowed(expr: str) -> bool:
    """SPDX expression: OR = any one choice is enough, AND = every part must be allowed."""
    expr = expr.replace("/", " OR ").strip()
    if expr.startswith("(") and expr.endswith(")") and expr.count("(") == 1:
        expr = expr[1:-1]
    # split on top-level OR
    depth, parts, cur = 0, [], ""
    for tok in re.split(r"(\(|\)|\s+OR\s+)", expr):
        if tok == "(":
            depth += 1
        elif tok == ")":
            depth -= 1
        if depth == 0 and tok is not None and re.fullmatch(r"\s+OR\s+", tok or ""):
            parts.append(cur)
            cur = ""
        elif tok:
            cur += tok
    parts.append(cur)
    for p in parts:
        p = p.strip().strip("()").strip()
        ands = [a.strip().strip("()").strip() for a in re.split(r"\s+AND\s+", p)]
        if all(a in ALLOWED or allowed(a) if (" OR " in a) else a in ALLOWED for a in ands):
            return True
    return False


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1"], cwd=root,
                                     capture_output=True, text=True, check=True).stdout)
    ours = {p["id"] for p in meta["packages"] if p["source"] is None}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    shipped, todo = set(), list(ours)
    while todo:
        for d in nodes.get(todo.pop(), {}).get("deps", []):
            if any(k["kind"] in (None, "build") for k in d["dep_kinds"]) and d["pkg"] not in shipped:
                shipped.add(d["pkg"])
                todo.append(d["pkg"])
    bad = []
    for p in meta["packages"]:
        if p["id"] not in shipped or p["id"] in ours or p["name"] in REVIEWED:
            continue
        lic = p.get("license")
        if not lic:
            bad.append(f"{p['name']} {p['version']}: no SPDX license (license file: {p.get('license_file')})")
        elif not allowed(lic):
            bad.append(f"{p['name']} {p['version']}: {lic}")
    if bad:
        print("License check FAILED: these shipped crates are not on the allowed list (see CONTRIBUTING.md):")
        print("\n".join("  " + b for b in bad))
        return 1
    print(f"License check passed: {len(shipped - ours)} shipped third-party crates, all on the allowed list.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
