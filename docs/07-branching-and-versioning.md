# 07 — Branching and versioning (Git for edits)

## What the user gets

- **History**: every change, who made it (you, the agent, a merge), when, with a readable description.
- **Branches**: "try a tighter cut" on a branch without touching main; switch instantly.
- **Compare**: two branches (or two commits) side by side — like DaVinci Resolve's *Compare Timelines* window,
  but for everything: clips added (green), removed (red), moved/trimmed (amber), effects/params/keyframes changed,
  audio mix changed; synced or independent playheads; a change list ("difference index") you can click through;
  play both versions.
- **Merge**: take all, or pick changes one by one; conflicts shown visually.
- **Agent branches**: the co-editor works on its own branch by default (like a pull request); you review and merge.

## Design

### Commits = snapshots + operations

- The **operation log** ([03](03-timeline-and-data-model.md)) is the source of truth for fine-grained history and undo.
- A **commit** groups operations and points to a **snapshot** of the project model (content-addressed: hash of
  its serialized form, structurally shared like Git trees: unchanged timelines/tracks/clips are not stored again).
- Autosave creates lightweight commits continuously; the user or agent can name/"squash" them into meaningful
  commits ("Rough cut", "Tightened pauses").
- **Branches** and **tags** are named refs to commits, stored in `refs/`.

### Media is not versioned — references are

- Timelines reference media by **content hash**; media files are never copied into history.
- Optional managed media store for sharing/backup: **content-defined chunking (FastCDC) + BLAKE3** addressed
  chunks, so re-encoded or partially changed large files dedupe well (as git-cdc and Xet do); git-annex/DVC-style
  remotes (disk, S3, SSH) for sync.
- Generated media (proxies, renders, AI outputs) are cache, keyed by content; never part of history.

### Diff

Diff works on the **model**, matched by stable IDs, not on files:

| Change | Detected as |
|---|---|
| Clip present only in B | Added |
| Clip only in A | Removed |
| Same clip ID, different record position | Moved |
| Same clip, different source in/out or duration | Trimmed / slipped |
| Same clip, param/keyframe/effect differences | Modified (with a param-level list) |
| Track/bus/mixer changes | Mix changed |
| Markers | Added/removed/moved |

Output: a structured change list (for UI and for the agent to explain: "B removes 14 pauses totalling 38 s and adds
3 zoom-ins") plus timeline ranges that differ (for the visual compare).

Clips without shared history (e.g. imported from another editor) are matched heuristically: same media + overlapping
source range + nearby position — the approach timeline-compare tools and "intent diff" projects (branchcut) use.

### Merge

Three-way merge (base = common ancestor):

- Non-overlapping changes merge automatically (A trimmed clip 3, B recolored clip 7).
- Same object, different fields → merge per field (A changed opacity, B changed position).
- Conflicts: same field changed differently, or structural conflicts on the same time range (both branches
  re-cut 00:40–01:10). Shown in the compare view; resolve by picking a side per change or per range.
- **Ripple is the hard part**: in sequential tracks, a ripple delete in A shifts everything after it, so B's
  changes later in the timeline would appear "moved". Merge therefore works on **operations with intent**
  (e.g. "ripple delete range R") and re-bases B's operations over A's (time offsets transformed), similar to
  operational transformation, instead of comparing absolute positions.

### Collaboration (later)

- Same model supports sync between machines: push/pull commits and refs to a remote.
- Live co-editing could later use **CRDTs** (Automerge/Yjs, "local-first software", Kleppmann et al. 2019;
  JSON CRDTs with move operations) on top of the operation log. Not needed for v1: branches + merges cover the
  editor/reviewer workflow.

## UI

- Branch switcher (like VS Code's), commit timeline/history panel, "compare with…" on any branch/commit.
- Compare view: two stacked timelines (like Resolve) or one timeline with change overlays; synced viewers
  (side-by-side, wipe, difference blend) to see pixel differences.
- Per-change accept/reject buttons during merge, with the agent able to explain each change.

## Prior art

Get Syncd (Git-style history for Resolve via OTIO), vit (git-backed JSON timelines, clip/field-level merge),
branchcut (intent diff + three-way merge of EDLs), vit-studio; DaVinci Resolve's Compare Timelines window; Git's
object model; Xet/git-cdc content-defined chunking; Automerge/Yjs CRDTs. See [references](references.md).

## Status (2026-10-09): built

See [16](16-phase-4-status.md). One departure from the text above: merge works on **states matched by id**
(base, ours, theirs), with gaps as real items and per-field merging, rather than re-basing operations; the merge result is
then turned into ordinary ops so the merge commit is readable and undoable. This avoids needing an operational
transform for every op and handled ripple edits on either side correctly in the fuzz tests.
