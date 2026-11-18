# captainE

A video editor written from scratch, with an AI co-editor built in and git-style branches for edits.

- **Timeline editing**: media pool, viewer, multi-track timeline, trim, roll, slide, slip, ripple, titles, transitions,
  keyframes, speed changes.
- **Colour and effects**: primaries, wheels, curves, HSL, LUTs, scopes, auto balance, shot matching; an effects library.
- **CoCaptain**, the AI co-editor: ask in plain words ("trim each clip to its best two seconds", "make the music
  quieter") and it edits through the same commands you use. Every change comes back as a suggestion to preview,
  apply or discard. It uses your own Claude login or API key.
- **Branches for edits**: try a cut on a branch, compare two branches side by side, merge.
- **Delivery**: H.264, HEVC, ProRes, GIF, PNG sequence, WAV/AAC; hardware encoding on macOS; HDR (HLG/PQ) phone
  footage is converted correctly. Interchange with OTIO, EDL and FCPXML.
- **Headless CLI** (`captaine`) for scripted edits and renders.

Version **0.1.0**, the first public release. macOS is the main platform; Linux and Windows builds pass CI.
See [CHANGELOG.md](CHANGELOG.md) for what is in it and [docs/STATUS.md](docs/STATUS.md) for what is left.

## Install and run

Needs [Rust](https://rustup.rs) (stable) and FFmpeg (`ffmpeg` and `ffprobe` on PATH, e.g. `brew install ffmpeg`).

```
git clone https://github.com/iamamit/captainE.git && cd captainE
cargo run --release -p captaine-app            # the editor window
sh tools/bundle.sh                              # macOS: build dist/captainE.app
cargo run --release -p captaine-cli -- --help   # the command line
```

CoCaptain needs either Claude Code signed in on the machine or `ANTHROPIC_API_KEY` set
(or a key in `~/.captaine/anthropic_key`).

## Command line

```
captaine init proj.json --fps 30 --size 1080x1920
captaine import proj.json clip.mov photo.heic music.mp3
captaine add proj.json clip.mov --dur 6
captaine title proj.json "WEEK 1" --template centered --at 0.5 --dur 3
captaine split proj.json 2.5 && captaine set proj.json 1 --opacity 0.8 --scale 1.1
captaine add proj.json music.mp3 --track A2 && captaine set proj.json 0 --track A2 --gain-db -14
captaine branch proj.json tighter --switch     # also: commit, diff, merge, log
captaine render proj.json out.mp4              # also: --codec hevc|prores|gif, --from/--to, --proxy
```

Commands: `init import add title split remove lift move trim roll slide slip tidy set track add-track undo redo show
log branch checkout commit diff merge proxy thumbs waveform frame loudness export-timeline import-timeline render`.

## Documents

**Where things stand:** [docs/STATUS.md](docs/STATUS.md): the plan, what is built and what is left, updated after
every build.

| # | Document | What it covers |
|---|---|---|
| 00 | [Vision](docs/00-vision.md) | What CaptainE is, who it's for, what makes it different, non-goals |
| 01 | [Landscape](docs/01-landscape.md) | Existing editors (open source and commercial), what to learn or reuse, licenses |
| 02 | [Architecture](docs/02-architecture.md) | The whole system: layers, modules, threads, data flow |
| 03 | [Timeline and data model](docs/03-timeline-and-data-model.md) | Time, timeline, clips, keyframes, the operation log, project format |
| 04 | [Rendering and playback](docs/04-rendering-and-playback.md) | Decoding, caching, proxies, GPU pipeline, compositing math, color, text |
| 05 | [Audio](docs/05-audio.md) | Audio engine, mixing, loudness, plugins, time-stretch |
| 06 | [Motion graphics and compositing](docs/06-motion-graphics-and-compositing.md) | Layers, node graph, keyframes, expressions, masks, tracking |
| 07 | [Branching and versioning](docs/07-branching-and-versioning.md) | Branches, commits, timeline diff, visual compare, merge, media storage |
| 08 | [AI co-editor (CoCaptain)](docs/08-ai-copilot.md) | Agent architecture, commands as tools, skills, agent-on-a-branch workflow |
| 09 | [AI media algorithms](docs/09-ai-media-algorithms.md) | Segmentation, matting, stabilization, interpolation, speech, noise, papers and licenses |
| 10 | [Tech stack](docs/10-tech-stack.md) | Language, GPU API, UI toolkit, libraries: options and recommendation |
| 11 | [Licensing and legal](docs/11-licensing-and-legal.md) | Open-source licenses, codec patents, what's safe to ship |
| 12 | [Roadmap](docs/12-roadmap.md) | Phases, deliverables, exit criteria, effort |
| 13 | [Risks and open questions](docs/13-risks-and-open-questions.md) | What could sink it, decisions still to make |
| 14 | [Phase 1 status](docs/14-phase-1-status.md) | What is built, decisions made while building, known gaps |
| 15 | [The editor window](docs/15-ui-status.md) | The egui UI: what follows the design, how to run it, known limits |
| 16 | [Phase 4 status](docs/16-phase-4-status.md) | Branches, compare, merge, CoCaptain on a branch |
| 20 | [Effects](docs/20-effects.md) | The Effects page and library: what captainE has, what is missing |
| — | [References](docs/references.md) | Every source used, grouped by topic |

## Contributing and licensing

captainE is written from scratch and distributed free (ad-supported). Read [CONTRIBUTING.md](CONTRIBUTING.md) before
changing anything: it covers what may and may not go into the code. [LICENSING.md](LICENSING.md) lists every tool used and its license.
