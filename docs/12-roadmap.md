# 12 — Roadmap

Principle: **a usable editor early**, then depth. Each phase ends with something runnable and tested on real
footage (the user's tutorials, gym Reels, podcast-style content). Effort estimates assume one developer working
with AI assistance; they are rough and will be revised after Phase 0.

## Phase 0 — Spikes and decisions (2–4 weeks)

Answer the risky questions with small throwaway prototypes before writing the real code:

| Spike | Question | Done when |
|---|---|---|
| Decode + display | Can Rust + FFmpeg + VideoToolbox + wgpu show 4K H.264/HEVC frames zero-copy? | 4K plays at full frame rate with low CPU |
| Scrub | How fast can we seek long-GOP media? What proxy format and cache sizes do we need? | Numbers for seek latency; proxy plan |
| UI toolkit | Qt/QML (cxx-qt) vs egui vs Slint for a 5,000-clip timeline at 60 fps + embedded wgpu viewer | Toolkit chosen, documented |
| Audio | cpal real-time thread + FFmpeg decode + resample, A/V sync clock | Stable playback, measured drift |
| OTIO | Round-trip OTIO/FCPXML/EDL with our time type | Lossless round trip on sample timelines |
| Agent | Tools generated from an op schema; one end-to-end "cut silences" on a toy model | Agent completes it and diff is readable |

Also decide: **CaptainE's license** ([11](11-licensing-and-legal.md)) and the UI toolkit.

## Phase 1 — Engine core (2–3 months)

- `time`, `model`, `ops` with undo/redo and the operation log; project directory format; autosave.
- Media: probe, decode (SW + HW), proxies, thumbnails, waveforms; frame caches and prefetch.
- Render: GPU compositor (transforms, opacity, blend modes, premultiplied linear-light), YUV/RGB conversion,
  basic OCIO color pipeline (Rec.709 end to end).
- Audio: mixer with clip gain/fades/pan, tracks, master, meters (EBU R128).
- Export: H.264/HEVC via platform encoders, ProRes on macOS, AV1; loudness-normalized audio.
- Headless CLI: open project → render; this is also the test harness.

**Exit:** a project with 50 clips, 4 video + 4 audio tracks plays smoothly and exports identically every time;
>90% test coverage on time/ops.

## Phase 2 — Editor MVP (2–3 months)

- UI shell: media browser/bins, timeline (tracks, snapping, zoom, selection), viewer (source + program),
  inspector, audio meters.
- Editing: insert/overwrite/append, blade, trim (ripple/roll/slip/slide), lift/extract, link/unlink,
  markers, in/out, keyboard-driven workflow (JKL, I/O, standard shortcuts).
- Titles (basic Text+ equivalent), transitions (dissolve, dip), keyframes for transform/opacity/volume.
- OTIO/FCPXML/EDL import/export.

**Exit:** the user can edit one real tutorial episode end to end in CaptainE.

## Phase 3 — Built-in co-editor (1–2 months)

- Agent runtime, chat panel, tools generated from ops, queries (summary, transcript, frames, loudness).
- Analysis jobs: Whisper transcripts with word times, VAD, shot detection, thumbnails index.
- Skills from codit ported: tighten talking head, voice polish, loudness for platform, captions, export presets.
- Plan → act → verify loop; eval suite on fixture projects.

**Exit:** "tighten this talking head and make the audio YouTube-ready" works on real footage and passes evals.

## Phase 4 — Branches and compare (1–2 months)

- Commits, branches, history panel; diff engine; Compare view (stacked timelines + synced/wipe/difference viewers).
- Three-way merge with intent-aware rebasing of ripple edits; conflict UI.
- Agent works on `agent/*` branches by default; merge from the chat.

**Exit:** agent branch → compare → partial merge works; history survives crashes; diffs are readable.

## Phase 5 — Audio depth (1–2 months)

- EQ/dynamics/de-esser/limiter; buses, sends, sidechain ducking; automation lanes.
- CLAP/LV2 hosting; time-stretch; AI voice cleanup (DeepFilterNet), stems (Demucs).

## Phase 6 — Motion graphics (2–4 months)

- Compositions and layers (After Effects model), shapes and text via Skia, masks, track mattes, precomps,
  graph editor, expressions, text animators.
- video-template-engine templates and Lottie import.
- AI subject matting (SAM 2 + RVM) → "text behind subject" as a one-step skill.

**Exit:** the user builds a motion title and a text-behind-subject shot without leaving CaptainE.

## Phase 7 — Color (2–3 months)

- Node-based grading: CDL, wheels, curves, qualifiers, windows with tracking, LUTs (tetrahedral), scopes,
  ACES workflows, shot matching.

## Phase 8 — Advanced AI and effects (ongoing)

- Retime with optical flow/RIFE, speed ramps; stabilization (L1 paths); auto-reframe for 9:16; super-resolution,
  deblur (with estimates); OFX plugin hosting; tracking (point/planar).

## Phase 9 — Collaboration and release

- Remotes (push/pull branches, media sync with chunked content-addressed storage), review links.
- Packaging, auto-update, crash reporting, docs; Windows and Linux builds; public beta.

## Rough total

- **Usable creator MVP** (Phases 0–4): about **7–11 months**.
- **Premiere + After-Effects-class depth** (Phases 5–9): **another 1–2 years**, ongoing — every pro editor is the
  work of large teams over many years, so prioritize by what real users (starting with the user's own content)
  actually need.
