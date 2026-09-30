# 02 — Architecture

## Layers

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ UI                                                                            │
│  Timeline · Viewer · Inspector · Node editor · Media browser · Mixer · Chat   │
│  Branch panel / Compare view · Scopes                                         │
└───────────────▲───────────────────────────────▲──────────────────────────────┘
                │ commands / queries             │ commands / queries
┌───────────────┴──────────┐        ┌────────────┴─────────────┐
│ AI co-editor (codit)     │        │ Scripting / plugins API  │
│ planner · skills · tools │        │ (Python/Lua/JS bindings) │
└───────────────▲──────────┘        └────────────▲─────────────┘
                └──────────────┬─────────────────┘
┌──────────────────────────────┴───────────────────────────────────────────────┐
│ Command layer (the ONLY way to change a project)                              │
│  typed operations · validation · undo/redo · operation log → versioning       │
└──────────────────────────────▲───────────────────────────────────────────────┘
┌──────────────────────────────┴───────────────────────────────────────────────┐
│ Project model (in-memory, immutable snapshots)                                │
│  media pool · timelines (OTIO-like) · compositions · node graphs · keyframes  │
├───────────────────────────────┬──────────────────────────────────────────────┤
│ Versioning store              │ Media store                                    │
│  commits · branches · diff/merge │ content-addressed media, proxies, caches    │
└───────────────────────────────┴──────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────┐
│ Engine                                                                        │
│  Evaluator (timeline → render graph for time t)                               │
│  Video: decode (HW) → GPU textures → effects/compositing → color → output     │
│  Audio: decode → resample → clip FX → track FX → buses → master → output      │
│  Caches: decoded frames · rendered frames · waveforms · thumbnails · analysis  │
│  Schedulers: playback · prefetch · background render · export · AI jobs       │
└──────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────┐
│ Platform                                                                      │
│  FFmpeg (demux/decode/encode) · VideoToolbox/NVDEC/VA-API/Vulkan Video        │
│  wgpu (Metal/Vulkan/D3D12) · CoreAudio/WASAPI/ALSA · ONNX Runtime/CoreML      │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Core design decisions

1. **Command layer is the single entry point.** Every change — a drag in the UI, an agent tool call, a script, a
   merge — is a typed operation (`InsertClip`, `Trim`, `SetParam`, `AddKeyframe`, …). This gives undo/redo, the
   history log, diffs, merges, macro recording and agent tools from one mechanism. The codit experience (patch
   undo, retargeting after timeline swaps) showed how painful it is when this doesn't exist.
2. **Immutable project snapshots.** The model is a persistent (structurally shared) tree. Each operation produces a
   new snapshot; old snapshots stay valid. Rendering threads read a snapshot without locks; the UI swaps in new
   ones. This also makes "compare two versions" cheap.
3. **Pull-based evaluation.** To show frame *t*, the evaluator walks the timeline at *t* and builds a small render
   graph (sources → effects → composite → color → output). Nothing renders that isn't needed for the requested
   frame and region.
4. **GPU-first video.** Decoded frames go to GPU textures (zero-copy where the platform allows); all effects and
   compositing run as GPU shaders in linear light, float/half-float precision.
5. **Separate audio engine with its own real-time thread.** Audio never waits for video; video syncs to the audio
   clock during playback.
6. **Everything slow is a background job** with progress, cancellation and cached results: proxies, waveforms,
   thumbnails, AI analysis (transcripts, segmentation, scene cuts), renders.
7. **Rational time everywhere** ([03](03-timeline-and-data-model.md)).

## Modules

| Module | Responsibility | Key libraries |
|---|---|---|
| `time` | `RationalTime`, `TimeRange`, frame/sample/timecode conversion | own (OTIO-compatible) |
| `model` | Project, media pool, timelines, compositions, node graphs, keyframes; immutable | own |
| `ops` | Operations, validation, apply/invert, operation log | own |
| `vcs` | Commits, branches, diff, merge, conflict detection | own (+ BLAKE3) |
| `media` | Probing, demux, decode (SW/HW), encode, proxies, content-addressed store | FFmpeg, platform HW APIs |
| `cache` | Frame caches (RAM/VRAM/disk), eviction, prefetch | own |
| `render` | Render graph, GPU compositor, effects, color pipeline, text | wgpu, OpenColorIO, Skia, HarfBuzz |
| `audio` | Real-time mixer, clip/track FX, buses, metering, plugin hosting | cpal, CLAP/LV2 hosting, libebur128, Rubber Band/SoundTouch |
| `motion` | Compositions, layers, shapes, masks, expressions, tracking | Skia, own expression engine |
| `ai` | Model runtime, analysis jobs (ASR, segmentation, VAD, …), results store | ONNX Runtime / CoreML, whisper.cpp |
| `agent` | Co-editor: planner, tools generated from `ops`, skills, context builder | LLM API (Claude), local models later |
| `interchange` | OTIO/EDL/FCPXML/AAF import/export, Lottie, video-template-engine templates | OpenTimelineIO |
| `ui` | Panels, timeline widget, viewer surface, compare view | see [10](10-tech-stack.md) |
| `plugins` | OFX host, CLAP/LV2 host, scripting API | OpenFX headers |

## Threads

| Thread / pool | Work | Priority |
|---|---|---|
| UI thread | Input, drawing panels, applying ops | high |
| Audio real-time thread | Mix the next buffer (no allocation, no locks, no I/O) | real-time |
| Playback scheduler | Decides which frames are needed next, feeds decode/render queues, keeps A/V sync | high |
| Decode pool | Demux + decode (HW sessions per stream) | high/normal |
| GPU render queue | Evaluate render graphs, composite, color, readback for export | high |
| Prefetch / cache workers | Fill ahead/behind the playhead, then "idle" regions | normal/background |
| Job system | Proxies, waveforms, thumbnails, analysis, AI models, export | background |
| Agent worker | LLM calls, tool execution (via command layer), verification | normal |

## Data flow: playback of one frame

1. Scheduler needs frame at time *t* on the program timeline (plus look-ahead *t+1…t+n*).
2. Evaluator resolves *t* → active items per track (clip, source time, effects, transitions, keyframed params at
   *t*) → render graph.
3. For each source: ask the frame cache; miss → decode request (seek to nearest keyframe, decode forward; HW
   decoder where possible; proxy if proxy mode is on).
4. Render graph runs on GPU: per-clip transform/effects → track compositing (Porter-Duff, premultiplied, linear
   light) → timeline color → output transform (display/export color space).
5. Result goes to the viewer surface (and to scopes if open) and into the rendered-frame cache.
6. Audio thread plays in sync; video frame is presented when the audio clock reaches its timestamp.

## Data flow: an agent edit

1. User: "tighten the pauses in this interview". Agent builds context (timeline summary, transcript with word
   times, selection).
2. By default the agent creates or switches to a branch (`agent/tighten-pauses`), plans, then issues operations
   through the same command layer.
3. Agent verifies (measures gaps, renders a few frames, checks audio), writes a summary.
4. User opens **Compare** (agent branch vs main), accepts all, some, or none (merge). See
   [07](07-branching-and-versioning.md), [08](08-ai-copilot.md).
