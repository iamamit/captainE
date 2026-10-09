#!/usr/bin/env python3
"""tools/setstatus.py ID STATUS 'note' : update one row of docs/parity.tsv"""
import sys
i, st, note = sys.argv[1:4]
p = 'docs/parity.tsv'
L = open(p).read().split('\n')
for n, l in enumerate(L):
    if l.startswith(i + '\t'):
        f = l.split('\t'); f[4] = st; f[5] = note; L[n] = '\t'.join(f)
open(p, 'w').write('\n'.join(L))
