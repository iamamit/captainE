# 13 — Risks and open questions

## Biggest risks

| Risk | Why it's real | Mitigation |
|---|---|---|
| **Scope** | Olive's full rewrite took years and stalled; pro editors are built by large teams over a decade | Creator MVP first ([12](12-roadmap.md)); every phase shippable; agent + branches as the differentiators, not feature parity |
| **Playback performance** | Long-GOP decode, 4K, many layers; users judge an editor by scrubbing feel | Phase 0 spikes; HW decode; proxies; layered caches; measure from day one (frame-time telemetry) |
| **Merge semantics** | Ripple edits make naive diff/merge wrong; conflicts in time are unfamiliar to users | Intent-aware ops and rebasing; start with "compare + pick changes" before full auto-merge |
| **Color correctness** | Wrong transfer functions/matrices are easy to get subtly wrong | OCIO everywhere; reference test images; compare against known-good renders |
| **A/V sync and timing** | Variable frame rate phone footage, drop-frame timecode | Rational time; VFR handling (conform or timestamp-based); tests with phone/screen recordings |
| **Licensing** | GPL/patent mistakes are expensive to undo | Decide license in Phase 0; permissive/LGPL-only core; platform codecs |
| **AI cost and latency** | LLM calls on long projects; local models on laptops | Text index of media, small context slices, local perception models, caching |
| **Solo maintenance** | Cross-platform GPU/driver issues | macOS first; wgpu to reduce API surface; automated render tests |

## Open questions (to decide)

1. **License and business model**: open source (which license) vs open-core vs commercial. (codit is planned as
   open source; CaptainE could follow or differ.)
2. **UI toolkit**: Qt/QML vs egui vs Slint — decided by the Phase 0 spike.
3. **Project storage**: pure custom object store vs building on a real Git repository (as vit does) for tooling
   compatibility.
4. **Sequential (OTIO) tracks vs free-position tracks** internally — sequential simplifies ripple and diff; confirm
   it doesn't make overlapping edits awkward.
5. **Expression language**: JavaScript subset (After Effects familiarity) vs a small custom language (safety, speed).
6. **Which LLMs**: Claude only, or pluggable providers + local models for offline use.
7. **Web version**: keep the engine portable to WebGPU/WebCodecs for a browser "lite" editor later?
8. **Name**: trademark and domain check for "CaptainE".

## Things to validate with real users early

- Does agent-on-a-branch feel natural, or do people want direct edits?
- Is the Compare view understandable without Git knowledge? (Use editor words: "version", "alternative", "keep this
  change", not "rebase".)
- Which 20 tasks do creators ask for most? (Feeds skills and priorities — same catalog as codit's skills plan.)
