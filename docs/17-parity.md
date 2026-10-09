# 17 — The parity yardstick

"Compete with 75% of Adobe" needs a number we can check, so this is the number.

**What it is.** A weighted list of the features a video creator actually uses across Premiere Pro (editing, delivery),
After Effects (motion and effects, the useful core), Audition (audio) and Lumetri (color), plus captions and AI helpers.
`docs/parity.tsv` has 62 features with a weight from 1 to 5 (how much a creator would miss it) and a status:

| Status | Means |
|---|---|
| 0 | not there |
| 0.25 | the engine has it, or it is very partial |
| 0.5 | works, clearly short of Adobe |
| 0.75 | solid |
| 1 | at parity for a creator |

`python3 tools/parity.py --detail` prints the score, the score per area, and what would move it most.

**Rules, so the score stays honest.**
- A feature counts only when it is built, tested, **and reachable from the window** (not just the engine).
- Weights are my judgement of creator value, not a market survey; they are in the file where you can change them.
- It deliberately includes things we will not reach (generative AI is in the list with weight 1) so the number cannot be
  gamed by dropping them. Enterprise features (shared storage, review portals, Creative Cloud) are not in the list at all.
- "75%" therefore means: *a creator can do three quarters of what they would do in Adobe's tools, in this editor.* It does
  not mean the same polish, plugin ecosystem or speed.

**Start (2026-10-09): 26.4%.** Target 75%.

**Reached (2026-10-10): 75.2%** by `python3 tools/parity.py` (62 features, every counted feature built, tested and reachable from the window). Scorecard: `docs/parity.tsv`.
