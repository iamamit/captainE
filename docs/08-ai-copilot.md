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
  the **Compare** view. The user merges all, some, or nothing. (Like a CoCaptain coding-agent pull request.)
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

## Status (2026-10-09): built

`crates/agent` implements the loop below for the **Edit-on-a-scratch-copy** variant (branches do not exist yet):

- **Tools** (13, in `tools.rs`) wrap the same ops and gestures a person's actions produce: `get_timeline`, `list_media`,
  `find_silences` (read-only); `split`, `delete_item`, `trim_item`, `move_item`, `add_clip`, `set_clip`, `add_title`,
  `edit_title`, `remove_silences`, `set_track`. Times are seconds; ids are the short ids shown by `get_timeline`.
- **The agent never touches the real project.** It edits a scratch `Editor`; everything it did comes back as one
  `Op::Transaction` the user can preview in the viewer, apply (author `Agent("copilot")`, one undo step) or discard.
  A follow-up request builds on a still-pending suggestion.
- **Verification before offering:** the combined ops are applied to the real project once more and refused if they
  do not apply.
- **Privacy:** project structure (names, times, properties) and analysis results are sent. Pictures only when the
  user turns on "Let CoCaptain see the video" (off by default): then the look tools send small labelled frames
  (at most 40 per request, older ones dropped from the conversation). No sound, no file contents. The key comes from `ANTHROPIC_API_KEY` or `~/.captaine/anthropic_key`
  (mode 600), never from a project file. Model: `claude-sonnet-5-5`, override with `CAPTAINE_MODEL`.
- **Without a key** the panel falls back to built-in rule-based skills, so it is never dead.
- **Tests:** a scripted model drives the whole loop (tool errors and recovery, history, pending edits, step limit,
  cut-off replies); the HTTP transport is tested against a local fake server (headers, 401/429/500, bad JSON);
  one `#[ignore]`d test talks to the real API.
- **Eyes (2026-10-26):** `look` renders the timeline at chosen times (or the middle of every clip) from the scratch
  copy, so CoCaptain sees its own pending edits; `look_media` shows frames of any video file. Pictures go to the
  model as image blocks (through Claude Code as stream-json input). The system prompt says: look before you judge,
  look again after you edit.
- **Not built yet:** beat detection (so no "edit by what was said" or "sync to music"), skills as
  playbooks, evals, working on a real branch (needs `vcs`).
