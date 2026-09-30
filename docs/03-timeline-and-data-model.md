# 03 — Timeline and data model

## Time

- **`RationalTime { value: i64, rate: Rational }`** — time as `value / rate` seconds, as in OpenTimelineIO. Frame
  rates like 29.97 are exact (`30000/1001`). Never store time as float seconds in the model.
- **`TimeRange { start, duration }`** — half-open `[start, start+duration)`. (codit hit Resolve's exclusive
  `endFrame` quirk; CaptainE defines ranges once and everywhere.)
- Conversions: frame ↔ time at a rate, sample ↔ time at a sample rate, SMPTE timecode (drop-frame and
  non-drop-frame) ↔ time. One well-tested module.
- Audio is edited at sample precision internally even when the UI snaps to frames (for sub-frame audio edits).

## Object model (OTIO-compatible core, plus editor state)

```
Project
├── settings            (frame rate, resolution, color management, audio format)
├── media_pool          (bins → MediaRef: content hash, path(s), probe info, proxies, analysis refs)
├── timelines[]         (Timeline)
├── compositions[]      (Composition, see 06)
└── node_graphs[]       (effect graphs attached to clips/tracks/compositions)

Timeline
├── settings            (can override project: rate, resolution, start timecode)
├── video_tracks[] / audio_tracks[] / subtitle_tracks[]
│   └── items[]: Clip | Gap | Transition | CompositionRef | Generator | Title
├── markers[]           (time range, color, name, note, metadata)
└── buses / mixer state (audio routing, see 05)

Clip
├── id                  (stable UUID — needed for diff/merge)
├── media: MediaRef + source_range (what part of the source)
├── record_range        (derived from position in track — OTIO style: items are sequential)
├── enabled, color label, name
├── time_map            (speed/retime curve: constant, reverse, freeze, ramps)
├── video: transform, crop, opacity, blend mode, effects stack → node graph
├── audio: gain, pan, fades, clip effects, channel mapping
└── metadata            (free-form, plugin and agent namespaces)
```

- Tracks in OTIO are **sequential** (item position = sum of previous durations, gaps are explicit). This makes
  ripple edits natural and diffs readable. Free positioning (like Resolve's "anywhere on a track") is expressed with
  gaps.
- **Stable IDs** on every item, keyframe and node. Diff and merge match objects by ID, not by position.

## Parameters and keyframes

- Every animatable property is a `Param<T>`: a constant value or an `AnimCurve<T>`.
- `AnimCurve` = sorted keyframes `{time, value, interp_out, interp_in, tangents}` with interpolation **hold,
  linear, bezier (temporal), auto-bezier, continuous**; spatial interpolation for positions (motion paths), as in
  After Effects' temporal vs spatial interpolation.
- Bezier easing stored like Lottie/After Effects: normalized in/out handles in 0..1 time/value space. Lottie
  import/export maps 1:1.
- Optional **expression** per param (see [06](06-motion-graphics-and-compositing.md)); evaluated after keyframes.
- Keyframe time is relative to the owner (clip or composition), so moving a clip moves its animation.

## The operation log

Every change is an **operation**:

```
Op {
  id, parent_snapshot, author: user | agent:<name> | script | merge,
  kind: InsertClip | RemoveItems | Trim | Slip | Slide | Roll | Ripple | Move | SetParam |
        AddKeyframe | RemoveKeyframe | AddEffect | ConnectNodes | AddTrack | ... ,
  payload (typed), timestamp, description (human-readable, generated)
}
```

- `apply(snapshot, op) → snapshot'` is pure; every op has an **inverse** computed at apply time (for undo).
- Ops are **small and semantic** ("trim clip X's out point by −12 frames"), not "replace the whole timeline".
  This is what makes readable history, diffs and merges possible ([07](07-branching-and-versioning.md)).
- **Transactions** group ops into one user-level action ("ripple delete 5 silences" = one undo step).
- Agent tools map 1:1 to ops or to transactions of ops ([08](08-ai-copilot.md)).

## Project file

- A project is a **directory** (or package on macOS):
  - `project.json` — settings, pointers to the current branch/commit
  - `objects/` — content-addressed snapshots and op batches (see 07)
  - `refs/` — branches, tags
  - `cache/` — regenerable (thumbnails, waveforms, analysis, render cache); safe to delete
  - media stays where it is (referenced by path + content hash), or is optionally collected into a managed store
- Human-readable export at any time: **OTIO** (lossless for the timeline core, with CaptainE extras in metadata).
- Autosave = committing the op log continuously; no "save" needed, no lost work.

## Editing operations to support (NLE baseline)

Insert, overwrite, replace, fit-to-fill, append; lift, extract (ripple delete); trim (ripple and non-ripple),
roll, slip, slide; blade/split; link/unlink audio-video; nest (compound/composition); markers; in/out points;
three-point and four-point editing; snapping; multicam (later); subtitles/captions tracks.
