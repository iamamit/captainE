# 14 — Phase 1 status (engine core)

Phase 1 of [the roadmap](12-roadmap.md) is built and tested **headless**: a project can be created, edited with
the standard gestures, mixed, composited and exported from the `captaine` CLI. There is no GUI and no real-time
playback yet; that is Phase 2.

## Exit criteria

| Criterion (docs/12) | Result |
|---|---|
| A project with 50 clips, 4 video + 4 audio tracks … exports identically every time | `crates/export/tests/exit.rs`: 50 clips (25 fps and 30 fps sources, one rotated), random transform/opacity/gain/pan/fades; two exports, one after an undo+redo, are **byte-identical** |
| >90% test coverage on time/ops | `cargo llvm-cov`: time 96–100%, ops 98–100% (lines) |
| Plays smoothly | **Not met.** There is no player yet. Offline render is faster than real time: about 85 fps for 3 layers at 720p (release build, CPU compositor) |

About 120 tests, including two fuzzers (random ops and random editing gestures, with 24/25 fps media on a 30 fps
timeline) that check every invariant after every step and that undo and redo return the *exact* earlier state.

## What exists

| Crate | Done |
|---|---|
| `time` | `Rational`, `RationalTime` (exact NTSC), `TimeRange` (half-open), SMPTE timecode with drop-frame |
| `model` | Project, media pool (video, audio-only, rotation-aware size, proxy path), timelines, tracks, clips, gaps; transform, opacity, 4 blend modes, gain/pan/fades; copy-on-write snapshots |
| `ops` | Invertible primitive ops and the gestures built on them: insert, ripple delete, lift, overwrite, move (across tracks), split/join, ripple trim, roll, slide, slip, properties, track settings, transactions; `Editor` with undo/redo and an op history |
| `media` | ffprobe probing (rotation, audio-only, cover art), proxy creation |
| `render` | Timeline evaluator, ffmpeg frame reader with seek/skip, LRU frame cache, linear-light premultiplied compositor (bilinear, rotation, edge coverage), sRGB LUTs, PNG output, thumbnails |
| `audio` | Windowed PCM decode, mixer (placement to the sample, gain, pan, fades, mute, track gain), EBU R128 loudness and peaks, waveform peaks |
| `export` | Engine-rendered frames + mix streamed into one ffmpeg encode: H.264, HEVC, ProRes |
| `cli` | `init import add split remove lift move trim roll slide slip tidy set track undo redo show log proxy thumbs waveform frame loudness render` |

## Decisions made while building

1. **Mixed frame rates.** Source offsets round **down** (`floor(k * source_rate / timeline_rate)`), the same rule
   the renderer uses to pick a source frame, so a cut never changes the picture at the cut. Because rounding
   loses information, the exact inverse of `Trim` and `Slip` is `SetRange`, of `Split` is "restore the original
   item and drop the new half", and of `Join` is "restore both items". Undo is therefore exact under any rate mix
   (found by the 50-clip exit test, fixed, and fuzzed).
2. **Fades** are clamped, not rejected, when a trim or split leaves them too long; undo restores them.
3. **Overwrite semantics** for placing and moving clips (like dragging in Premiere/Resolve); ripple for
   insert/remove/trim. Gaps are explicit items and `tidy` merges them.
4. **Embedded audio.** A clip on a video track also plays its media's audio (use `set --mute true` to silence);
   audio tracks hold music/voice-over. Link/unlink as a separate concept is Phase 2.
5. **Determinism.** Output uses tables for color conversion and no dithering or threading in the compositor, so the
   same project gives the same bytes. The check is in CI-style tests, not a promise across ffmpeg versions.

## Known gaps against the Phase 1 plan

- **No GPU compositor and no hardware decode.** The CPU renderer is the reference the GPU one must match. Decode
  is an `ffmpeg` subprocess per clip: it seeks accurately and streams sequentially, but a scrub restarts the
  process (tens of milliseconds). Replacing both is the next performance step (Phase 0 spikes).
- **Color management is a stand-in.** sRGB transfer curve in the compositor, BT.709 tagging on export. Untagged
  HD sources are decoded by ffmpeg's default matrix, which may differ from what a player would assume. OCIO
  arrives with Phase 7.
- **No real-time audio output** (cpal) and no live meters; loudness is measured offline.
- **Export** covers H.264, HEVC and ProRes; AV1 and platform hardware encoders are not wired.
- **Not in Phase 1 by design:** keyframes, transitions, speed changes, titles, OTIO/EDL/FCPXML, the UI.
- Whole-timeline operations use linear scans (`find_item`, `find_track`); fine for hundreds of clips, needs an
  index before thousands.
