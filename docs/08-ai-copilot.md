# 08 — The AI co-editor (built-in codit)

## Lessons carried over from codit (Resolve)

| In Resolve (codit) | Consequence | In CaptainE |
|---|---|---|
| Official API covers part of the app; the rest needed reverse engineering and UI automation | Months of work, fragile, some things impossible (Magic Mask needs a user click) | The agent uses the **same command layer** as the UI: anything a person can do, the agent can do |
| Undo had to be rebuilt (timeline swaps, patch undo, retargeting) | Complex and error-prone | Undo/redo is native in the op log; agent changes are ordinary ops |
| The agent can't see the viewer; frames exported to disk to "look" | Slow | Direct access to rendered frames, scopes, waveforms, loudness, transcripts |
| Big edits are risky | Needed "ask first" rules | The agent works **on a branch**; the user compares and merges |
| Prompt grew huge | Diluted instructions | Constitution + **skills** + tools from day one (see codit's `docs/skills-plan.md`) |

## Architecture

```
Chat panel ──► Agent runtime
                 ├─ Context builder: project summary, selection, playhead, timeline slice around focus,
                 │   transcript + word times, shot list, analysis results, recent history, open branch
                 ├─ Planner (plan → act → verify loop)
                 ├─ Skills (SKILL.md playbooks, loaded on demand)
                 ├─ Tools  = generated from the command layer (ops) + queries + analysis jobs + render/inspect
                 ├─ Verifier: render frames, measure audio, diff the branch, check constraints
                 └─ Model router: cloud LLM (Claude) for reasoning; local models for perception (see 09)
```

### Tools come from the ops

- Each operation type has a schema (typed params, docs). The agent tool list is **generated** from it, so new
  features are automatically agent-usable and the UI and agent never drift apart.
- Tools are grouped (editing, audio, color, motion, media, analysis, versioning, export) and loaded per task to
  keep context small.
- Queries give the agent structured facts instead of screenshots: `timeline.summary`, `clip.details`,
  `audio.measure(range)`, `transcript(range)`, `frame(t, size)`, `scopes(t)`, `diff(branchA, branchB)`.

### Plan → act → verify

1. Understand: read context; ask **one** question only if genuinely ambiguous.
2. Plan: for anything beyond a small edit, show a short plan (and a time estimate for slow jobs).
3. Act: ops in a transaction on the agent branch.
4. Verify: measure/render/diff; fix and re-check (bounded rounds).
5. Report: what changed (generated from the diff), what couldn't be done and why; offer "merge into main".

### Agent-on-a-branch workflow

- Default for multi-step edits: the agent creates `agent/<task>` from the current branch, works there, then shows
  the **Compare** view. The user merges all, some, or nothing. (Like a Copilot coding-agent pull request.)
- Small, explicit edits ("mute A2") may apply directly to the current branch (user setting).
- Alternatives: "give me two versions of the intro" → two agent branches, compared side by side (the VideoDiff
  research idea of co-creation with alternatives).

### Skills

Same design as codit: `prompt` (always) + `skills/<name>/SKILL.md` (on demand) + tools. The use-case catalog and
first skills from the codit plan carry over (talking-head tightening, voice polish, loudness for platform, tutorial
edit, punch-ins, titles, captions, export presets, text behind subject). In CaptainE every step exists as a tool, so
skills are never blocked by a missing API.

### Understanding footage

LLMs can't watch hours of video, so CaptainE builds a **text index** of the media (the LAVE approach: language
descriptions of footage) from background analysis ([09](09-ai-media-algorithms.md)): transcript with word
timestamps and speakers, shot boundaries, per-shot captions/tags from a vision-language model, faces/people,
on-screen text (OCR for screen recordings), audio events, loudness, motion/blur/exposure problems. The agent
searches this index, then looks at specific frames only when needed.

### Multimodal input

- Point at the viewer: "blur this", "track this", "put the title here" — clicks/sketches become spatial references
  (the ExpressEdit research: natural language plus sketching).
- Select clips/ranges, then ask; the selection is part of the context.

### Models

- Reasoning: Claude via API (Claude Agent SDK patterns from codit); model choice configurable; later optional
  local LLMs for offline use.
- Perception runs locally (whisper.cpp, SAM 2, VAD, etc.) through ONNX Runtime/CoreML, so media never leaves the
  machine unless the user allows it.
- Cost/latency control: cached analysis, small context slices, tool results summarized.

### Safety and trust

- Every agent change is an op on a branch: visible, diffable, undoable.
- The agent never claims results it didn't verify (codit rule: measure before saying audio is fine).
- Slow/expensive jobs (AI deblur, super-resolution, big renders) show an estimate and ask first (codit lesson).
- Nothing destructive to media files; deletes of media require explicit confirmation.

## Evaluation

- An eval suite of real requests on fixture projects (from the codit skills plan), scored on the resulting diff and
  measurements. Run on every change to prompts, skills, tools or models.
