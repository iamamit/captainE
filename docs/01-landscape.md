# 01 — Landscape: what exists and what to learn from it

## Open-source editors and engines

| Project | What it is | Architecture | License | What to take from it |
|---|---|---|---|---|
| **MLT** (Media Lovin' Toolkit) | Multimedia engine for NLEs and playout | Producers → filters/transitions → consumers; XML project schema; plugins; FFmpeg for codecs; Movit (GPU), frei0r, LADSPA, SoX | LGPL | The producer/filter/consumer model; one XML schema used by several apps; how a reusable engine is separated from the UI |
| **Kdenlive** | Full NLE (KDE) | C++, Qt/KDE Frameworks for UI, MLT underneath; project file = MLT XML | GPL | What a mature open NLE needs (proxies, effects stacks, keyframes, subtitles); pitfalls of building on a general engine |
| **Shotcut** | NLE | Qt + MLT (same engine as Kdenlive) | GPL | Same engine, different UX: shows the engine/UI split works |
| **Flowblade** | NLE (Linux) | Python + MLT | GPL | Simple insert/overwrite editing model |
| **Pitivi** | NLE (GNOME) | Python + **GStreamer Editing Services** (GES) | LGPL (GES) | GES timeline model: layers (what users arrange) vs tracks (output streams); the timeline is itself a GStreamer element; NLE = thread-safe plugins that turn the timeline into a pipeline |
| **OpenShot / libopenshot** | NLE + C++ library | Readers → Frames → Timeline/Clips → Writers; keyframe curves; caching; Python/Qt UI over C++ libs | LGPL (lib, also commercial) | A clean library API (Reader/Frame/Writer/Keyframe) worth imitating; the lesson that playback performance needs serious caching |
| **Olive** (0.2 rewrite) | NLE with node-based compositing | Every clip/effect/composite is a node in a DAG; GPU-first half/float pipeline; OpenColorIO color management; automated disk cache | GPL | Closest to CaptainE's "timeline + nodes" idea; also a warning: the 0.2 rewrite took years and stalled; scope carefully |
| **Blender VSE** | Sequencer inside Blender | Strips on channels; compositor nodes separate; Python API | GPL | Strip model, proxies, scripting; the long-standing pain of *not* integrating the compositor with the sequencer |
| **Natron** | Node compositor (Nuke-like) | OpenFX engine (almost all of OFX 1.4), OpenImageIO, OpenColorIO | GPL | How to host OFX plugins; node-graph UX; color-managed compositing |
| **Friction** (fork of enve) | 2D motion graphics (After-Effects-like) | C++, Qt, **Skia** GPU rendering; vector + raster, path effects, expressions, multiple scenes | GPL | Skia as the 2D vector engine; expression editor; path/shape animation |
| **Glaxnimate** | Vector animation, Lottie editor | Qt | GPL | Lottie import/export |
| **Gyroflow** | Gyro-based stabilization | Pure-Rust core library (no deps), Qt/QML UI, FFmpeg, GPU processing; plugins for Resolve/Premiere/FCP | GPL-3 | A modern Rust media app done well: Rust core + GPU + Qt UI; real-time preview |
| **OpenTimelineIO** | Timeline interchange (ASWF) | Timeline → Stack → Tracks → Clips/Gaps/Transitions; `RationalTime`; adapters for EDL/FCP XML/AAF/… | Apache-2.0 | Adopt its data model and `RationalTime` directly; use it for import/export |
| **Motion Canvas** | Code-driven vector animation (TypeScript generators) + live editor | Canvas renderer | MIT | Animation-as-code ideas for the template engine |
| **Remotion** | Video as React components, rendered frame by frame | Headless browser rendering | Source-available, company license above a size | Frame-accurate `seek(t)` rendering model (the video-template-engine already follows it) |
| **Browser editors** (OpenReel, VEdit, browser-studio, KubeezCut) | Editors fully in the browser | WebCodecs (decode/encode), WebGPU (compositing, zero-copy `texture_external`), Web Audio, FFmpeg.wasm fallback | Various (MIT mostly) | Proves WebCodecs + WebGPU is viable for editing; a possible web/lite version later |

## Commercial editors (to study, not copy)

| Product | Notable for |
|---|---|
| DaVinci Resolve | Pages (Cut/Edit/Fusion/Color/Fairlight/Deliver) over one project; node grading; **Compare Timelines** window (added/deleted/moved/trimmed highlighted, synced playheads, Difference Index) — direct inspiration for CaptainE's branch compare |
| Premiere Pro + After Effects | The edit/motion split CaptainE wants to remove; Dynamic Link; Essential Graphics templates; OCIO/ACES support added recently |
| Final Cut Pro | Magnetic timeline, roles, compound clips; very fast on Apple silicon |
| Descript | Text-based editing; **Underlord** agent ("remove filler words", "add zooms to hide jump cuts", highlight reels) |
| CapCut | Creator-focused templates, auto captions, one-click effects |

## Version control for timelines (prior art)

| Project | Idea |
|---|---|
| Get Syncd | Git-style history/compare/restore/branches for DaVinci Resolve, timelines stored as OTIO text |
| vit | Timeline as small diffable JSON documents in a real git repo; merges branches at clip and field granularity |
| branchcut | Branch, "intent diff" and three-way merge for edit decision lists; versions the EDL, never the pixels |
| vit-studio | Clip placements, grades, audio levels, effects, markers as JSON over git |

These are small projects bolted onto existing editors. CaptainE can do this natively and much better because the
edit history is its own data model ([07](07-branching-and-versioning.md)).

## Research on AI-assisted editing

| Work | Key idea |
|---|---|
| LAVE (IUI 2024) | LLM agent with plan-and-execute over editing functions; auto-generated language descriptions of footage so the LLM can reason about it |
| ExpressEdit (IUI 2024) | Natural language **plus sketching on the frame** for spatial references; based on 176 real editor commands |
| Leake et al., SIGGRAPH 2017 | Dialogue scenes edited automatically from script + takes using film-editing idioms |
| Fried et al., SIGGRAPH 2019 | Text-based editing of talking-head video (edit the transcript, video follows) |
| VideoDiff (2025) | Human-AI co-creation with *alternatives* — directly relevant to agent branches |

## Takeaways

1. The mature open editors (Kdenlive, Shotcut, OpenShot) are built on general engines (MLT, libopenshot) that were
   not designed for GPU-first, node-based, agent-driven editing. Reusing them would inherit their limits.
2. The ecosystem *libraries* are excellent and should be reused: FFmpeg, OpenTimelineIO, OpenColorIO,
   OpenImageIO, OpenFX, Skia, HarfBuzz, libebur128, whisper.cpp, ONNX Runtime.
3. Nobody has combined a GPU-first NLE, integrated compositing, native branching and a first-class agent. That's
   the gap.
