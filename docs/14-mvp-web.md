# 14 — MVP: creator editor for desktop browsers

This replaces the scope of phases 1–4 in [12-roadmap](12-roadmap.md) for the first release. The earlier docs
(Rust engine, Qt, branching, color) stay as the long-term vision.

## Decisions

| Topic | Decision | Why |
|---|---|---|
| Target | Chrome / Edge / Safari on **PC and Mac** (no phone layout) | Full WebCodecs/WebGPU, big screens, simplest QA |
| Stack | TypeScript, React, Vite, zustand; Vitest (unit), Playwright (e2e) | Fast iteration; the editor can be run and screenshotted in CI and in the dev sandbox |
| Business model | Free tier + subscription (Stripe); ads only outside the editing view | Rendering is client-side so per-user server cost is ~0; only AI calls cost money, so they get daily caps |
| Time | Integer microseconds | Exact, no float drift across edits |
| Edits | Every change is a `Command` (plain data) applied by `apply()` and logged by `History` | UI, AI prompt tool, templates and undo all share one path; the log enables branching later |

## Feature scope (phase 1)

Core editing, text and captions, CapCut-style animation, filters/overlays, audio, export, **AI prompt tool**,
**background removal**, **beat sync**, **templates** — see the build order below.

## Build order

1. **Foundation (done):** project model, command layer with undo/redo and batch, keyframes, canvas preview,
   timeline (select/drag/split/delete), inspector, import, playback. Unit + e2e tested.
2. Export to MP4 in the browser (WebCodecs + muxer), audio mixed in.
3. Animation presets (in/out/loop), transitions, text styles, stickers/overlays, filters and adjustments.
4. Auto-captions (speech-to-text with word timing) and caption styles.
5. AI prompt tool: chat panel → Claude with tools generated from `Command`; changes preview first, one-step undo.
6. Beat sync: onset/tempo detection on the music track, beat markers, snap and auto-cut to beats.
7. Templates: JSON bundle of tracks/text/animations with placeholder slots, applied as a command batch.
8. Background removal: in-browser person segmentation (Apache-2.0 models: MediaPipe / MODNet) → alpha mask in the
   compositor; masks cached per clip.
9. Accounts, billing, usage caps, watermark on the free tier.

## Constraints and risks

- Real GPU speed (background removal, 4K) can't be measured in the dev sandbox; it needs checking on real machines.
- Avoid GPL dependencies (e.g. Robust Video Matting) since the product is commercial.
- No bundled copyrighted music; use royalty-free libraries or user uploads.
- Export is slower than a native app; target 1080p short-form first.
