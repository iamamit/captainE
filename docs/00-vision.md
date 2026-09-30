# 00 — Vision

## One sentence

A professional video editor where editing, motion graphics and an AI co-editor live in one app, and every edit
can be branched, compared and merged like code.

## Why build it

| Today | Problem | CaptainE |
|---|---|---|
| Edit in Premiere/Resolve, animate in After Effects/Fusion | Round-trips between apps, dynamic links, render-and-reimport | One timeline; any clip can open as a composition in place |
| AI features bolted onto editors (Descript Underlord, Premiere AI, codit-in-Resolve) | The agent drives an app through a limited scripting API or UI automation; it can't see or do everything | The agent uses the **same command API** as the UI, so it can do anything a person can |
| "Save As v2, v3, final_final" | No real history, no safe experimentation, painful collaboration | Branches, commits, diffs and merges for timelines, like Git for code |
| Undo is a linear stack per session | Can't keep two alternative cuts alive, can't see what changed | Every change is an operation in a log: undo, history, diff and merge all come from it |

The codit project proved the pain directly: codit-core had to reverse-engineer Resolve's file formats and drive
its UI because the official API covers only part of the app. In an editor we build ourselves, the agent is a
first-class user from day one.

## Who it's for (in order)

1. **Creators** making YouTube videos, tutorials, podcasts, Reels/Shorts: talking head, screen recordings,
   captions, zooms, music, motion titles. (The user's own content is the first test case.)
2. **Small teams**: editor + reviewer + motion designer, sharing branches instead of project copies.
3. **Later**: professional long-form editing and finishing (color, conform, deliverables).

## What makes it different

1. **Agent-native.** The AI co-editor is part of the core, not a plugin. It plans, acts through commands,
   checks the result (looks at frames, measures audio) and reports. See [08](08-ai-copilot.md).
2. **Branches for edits.** Try an alternative cut on a branch; compare it with main side by side (added / removed
   / moved / trimmed clips, changed effects); merge what you like. The agent works on its own branch by default,
   like a pull request. See [07](07-branching-and-versioning.md).
3. **Edit + motion in one.** Premiere-style timeline for cutting, After-Effects-style layered compositions and a
   node graph for effects, all in one project. See [06](06-motion-graphics-and-compositing.md).
4. **Open formats.** Timeline interchange through OpenTimelineIO, color through OpenColorIO/ACES, effects
   through OpenFX, audio plugins through CLAP/LV2, templates from the video-template-engine.

## Non-goals (for a long time)

- 3D modelling/animation (Blender's job), node-based VFX at Nuke scale, broadcast playout, live switching.
- Hardware control surfaces, tape/SDI capture.
- Our own codecs.
- Cloud-first editing. CaptainE is **local-first**: media and projects live on the user's machine; sync and
  collaboration come on top.

## Principles

- **Correctness of time first.** Frames and samples are exact (rational time), never floating-point seconds.
- **Everything is an operation.** UI, agent, scripts and merges all go through the same command layer.
- **Non-destructive.** Media files are never modified; all edits are data.
- **Fast feedback.** Scrubbing and playback feel instant (caching, proxies, GPU), even on a laptop.
- **Honest AI.** The agent never claims what it didn't check; every AI change is visible, diffable and undoable.
