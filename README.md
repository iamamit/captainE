# CaptainE

A video editor built from scratch that combines:

- **Editing** like Premiere Pro / DaVinci Resolve's Edit page (a non-linear timeline editor),
- **Motion graphics and compositing** like After Effects / Fusion,
- **A built-in AI co-editor** (codit, like Copilot in VS Code) that edits through the same commands a person uses,
- **Git-style branches** for edits: work on a branch, compare it with another branch side by side, merge.

Status: **early code with a working editor window** (`cargo run --release -p captaine-app`) (planning started 2026-10-01, coding 2026-10-09). The engine core exists and is tested;
there is no GUI yet. See [Building](#building) for what runs today.

## Documents

| # | Document | What it covers |
|---|---|---|
| 00 | [Vision](docs/00-vision.md) | What CaptainE is, who it's for, what makes it different, non-goals |
| 01 | [Landscape](docs/01-landscape.md) | Existing editors (open source and commercial), what to learn or reuse, licenses |
| 02 | [Architecture](docs/02-architecture.md) | The whole system: layers, modules, threads, data flow |
| 03 | [Timeline and data model](docs/03-timeline-and-data-model.md) | Time, timeline, clips, keyframes, the operation log, project format |
| 04 | [Rendering and playback](docs/04-rendering-and-playback.md) | Decoding, caching, proxies, GPU pipeline, compositing math, color, text |
| 05 | [Audio](docs/05-audio.md) | Audio engine, mixing, loudness, plugins, time-stretch |
| 06 | [Motion graphics and compositing](docs/06-motion-graphics-and-compositing.md) | After-Effects-style layers, node graph, keyframes, expressions, masks, tracking |
| 07 | [Branching and versioning](docs/07-branching-and-versioning.md) | Branches, commits, timeline diff, visual compare, merge, media storage |
| 08 | [AI co-editor (built-in codit)](docs/08-ai-copilot.md) | Agent architecture, commands as tools, skills, agent-on-a-branch workflow |
| 09 | [AI media algorithms](docs/09-ai-media-algorithms.md) | Segmentation, matting, stabilization, interpolation, speech, noise, papers and licenses |
| 10 | [Tech stack](docs/10-tech-stack.md) | Language, GPU API, UI toolkit, libraries: options and recommendation |
| 11 | [Licensing and legal](docs/11-licensing-and-legal.md) | Open-source licenses, codec patents, what's safe to ship |
| 12 | [Roadmap](docs/12-roadmap.md) | Phases, deliverables, exit criteria, effort |
| 13 | [Risks and open questions](docs/13-risks-and-open-questions.md) | What could sink it, decisions still to make |
| 14 | [Phase 1 status](docs/14-phase-1-status.md) | What is built, decisions made while building, known gaps |
| 15 | [The editor window](docs/15-ui-status.md) | The egui UI: what follows the design, how to run it, known limits |
| — | [References](docs/references.md) | Every source used, grouped by topic |

## Relationship to the other projects

- **codit** (`iamamit/codit`, `iamamit/codit-core`): the AI co-editor inside DaVinci Resolve. Its agent design, skills,
  undo model and lessons carry over; in CaptainE the agent is native instead of driving another app.
- **video-template-engine** (`iamamit/video-template-engine`): reusable motion-graphics templates. CaptainE's motion
  graphics layer should load these templates directly.

## Building

Needs Rust (stable) and FFmpeg (`ffmpeg`/`ffprobe` on PATH) for probing and rendering.

```
cargo test                      # unit, fuzz and real-ffmpeg render tests
cargo run -p captaine-cli -- --help
```

| Crate | Role |
|---|---|
| `time` | `Rational`, `RationalTime`, `TimeRange`, SMPTE timecode incl. drop-frame |
| `model` | Immutable project: media pool, timelines, tracks, clips, gaps, transform/audio properties; stable ids; copy-on-write snapshots |
| `ops` | Invertible operations, editing gestures (overwrite, move, roll, slide, slip, ...), `Editor` with undo/redo |
| `media` | Probe files and make proxies (ffprobe/ffmpeg for now) |
| `render` | Timeline evaluator, frame decoder and cache, linear-light CPU compositor |
| `audio` | Decode, mixer (gain, pan, fades), EBU R128 loudness, waveform peaks |
| `export` | Render + mix + encode (H.264, HEVC, ProRes); byte-identical across runs |
| `project` | Project file = settings + operation log, replayed into an `Editor` |
| `agent` | The Claude co-editor: 13 tools over the ops layer, the agent loop, the Anthropic transport |
| `app` | **The editor window** (egui): media pool, viewer, inspector, timeline, Copilot |
| `cli` | `captaine` binary: `init import add split remove lift move trim roll slide slip tidy set track undo redo show log proxy thumbs waveform frame loudness render` |

Phase 1 status, decisions and known gaps: [docs/14](docs/14-phase-1-status.md).

```
captaine init proj.json --fps 30 --size 1920x1080
captaine import proj.json clip.mp4 music.wav
captaine add proj.json clip.mp4 --dur 6
captaine split proj.json 2.5 && captaine set proj.json 1 --opacity 0.8 --scale 0.6 --x 300
captaine add proj.json music.wav --track A2 && captaine set proj.json 0 --track A2 --gain-db -14
captaine render proj.json out.mp4        # also: --codec hevc|prores, --proxy
```
