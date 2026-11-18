# Changelog

## 0.1.0 (2026-11-18)

First public release.

**Editor window**: Cut, Edit, Color, Effects and Deliver pages; media pool with bins and search; program and source
viewers with scopes; multi-track timeline with select, trim, ripple, roll, slide, slip, razor and snapping;
titles and transitions; keyframes; speed changes; inspector, history, metadata, mixer and transcript panels;
autosave with backups.

**Colour**: primaries, lift/gamma/gain wheels, curves, HSL, LUTs, auto balance, match shots, copy and paste grades.

**CoCaptain**: an AI co-editor that edits through the editor's own commands and returns each change as a
suggestion to preview and apply. Uses the user's Claude Code login or API key.

**Branches**: commit, branch, compare side by side, three-way merge with conflict choices.

**Delivery**: H.264, HEVC, ProRes, GIF, PNG sequence, WAV and AAC; VideoToolbox on macOS; HDR phone footage is
converted to Rec.709. OTIO, EDL and FCPXML import and export.

**CLI**: `captaine` for scripted edits, frames, thumbnails, waveforms, loudness and renders.

Fixed during release testing:

- iPhone HEIC photos import (the photo, not its HDR gain map) and PNG/JPEG stills can be placed and trimmed like
  footage; stills get their own bin.
- One unreadable file no longer stops the rest of an import; a file already in the project is not added twice.
- Reading an EDL back keeps each clip's opacity, gain and effects; EDL tracks are numbered per kind (V1, A2).
- A title added without a track goes on a free video track (a new one if needed) instead of covering a clip.
- CoCaptain no longer discards finished work when its last reply is plain words.
- `render --from/--to` reports the rendered length; `set --opacity` and `--pan` document their ranges.
