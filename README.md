# CaptainE

A video editor built from scratch that combines:

- **Editing** like Premiere Pro / DaVinci Resolve's Edit page (a non-linear timeline editor),
- **Motion graphics and compositing** like After Effects / Fusion,
- **A built-in AI co-editor** (codit, like Copilot in VS Code) that edits through the same commands a person uses,
- **Git-style branches** for edits: work on a branch, compare it with another branch side by side, merge.

Status: planning docs plus the first web MVP foundation (see [MVP plan](docs/14-mvp-web.md)). Run `npm install && npm run dev`; tests: `npm test`, `npm run e2e`.

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
| 14 | [MVP: creator editor for desktop browsers](docs/14-mvp-web.md) | First release scope, stack and build order |
| — | [References](docs/references.md) | Every source used, grouped by topic |

## Relationship to the other projects

- **codit** (`iamamit/codit`, `iamamit/codit-core`): the AI co-editor inside DaVinci Resolve. Its agent design, skills,
  undo model and lessons carry over; in CaptainE the agent is native instead of driving another app.
- **video-template-engine** (`iamamit/video-template-engine`): reusable motion-graphics templates. CaptainE's motion
  graphics layer should load these templates directly.
