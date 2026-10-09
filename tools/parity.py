#!/usr/bin/env python3
"""Weighted parity score from docs/parity.tsv. Run: python3 tools/parity.py [--detail]"""
import csv, sys, pathlib, collections

path = pathlib.Path(__file__).resolve().parent.parent / "docs" / "parity.tsv"
rows = []
with open(path) as f:
    for r in csv.reader((l for l in f if not l.startswith("#")), delimiter="\t"):
        if len(r) >= 5:
            rows.append(dict(id=r[0], area=r[1], feature=r[2], weight=float(r[3]), status=float(r[4]), note=r[5] if len(r) > 5 else ""))

total = sum(r["weight"] for r in rows)
score = sum(r["weight"] * r["status"] for r in rows)
print(f"parity score: {100*score/total:.1f}%   ({len(rows)} features, weight {total:.0f})")
areas = collections.OrderedDict()
for r in rows:
    a = areas.setdefault(r["area"], [0.0, 0.0])
    a[0] += r["weight"] * r["status"]
    a[1] += r["weight"]
for k, (s, t) in areas.items():
    print(f"  {k:<9} {100*s/t:5.1f}%")
if "--detail" in sys.argv:
    print()
    # what would move the score most: weight left to gain
    for r in sorted(rows, key=lambda r: -r["weight"] * (1 - r["status"]))[:25]:
        print(f"  {r['weight']*(1-r['status']):4.2f}  {r['id']}  {r['feature']}  [{r['status']}]")
need = 0.75 * total - score
print(f"\nweight still to earn for 75%: {max(need,0):.1f} of {total:.0f}")
