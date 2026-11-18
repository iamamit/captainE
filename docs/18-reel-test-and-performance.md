# 18 · Real-footage test: the PushDay reel, and the performance pass it led to

*2026-10-15.* The gym reel first cut in DaVinci Resolve (project `reel`, timeline `PushDay Reel`) was rebuilt
in captainE from the same two DJI 4K HEVC files (3840x2160, 29.97 fps, 5:32 and 7:29 long). The edit was
done through captainE's own CoCaptain tools, run headless, and its output was compared frame by frame with
Resolve's render.

Project: `~/project/gym-reels/captaine/PushDay.captaine.json` (branch `agent/pushday-reel`).
Script: `~/project/gym-reels/captaine/scripts/make_pushday.py` → `pushday_reel.json` (65 tool calls).
Output: `exports/PushDay_captainE.mp4`. Side by side with Resolve: `exports/PushDay_compare.mp4`.

## What the reel uses

The reel has 11 shots cut to the beat. Most shots have a slow push-in zoom. Four shots have speed ramps, and four
have a punch-in with a 5-frame shake. There are two whip pans. A grade layer applies colour wheels, sharpen, glow,
vignette and 2.39 letterbox bars. A two-word title is set in Futura Condensed ExtraBold, with red and white words.
The audio is the song plus 8 sound effects, mastered to -14 LUFS.

Against Resolve, the frames match in framing and timing. The colour is within 1–3 code values per channel
(`ref/`, `scripts/iterate.sh`).

## Missing features found and built

| Feature | Where |
|---|---|
| `add_track` for CoCaptain and `captaine add-track` (a shared `ops::edit::add_tracks`, also used by the app) | agent, cli, ops, app |
| Title colours from CoCaptain: `color`, `outline_color`, `accent_color` | agent `edit_title` |
| Font collections: `Futura.ttc#Condensed ExtraBold` or `#4` | render `text::load_font` |
| Whip left and whip right transitions: an eased push with motion blur, the picture wrapped sideways so there is no edge, the switch hidden in the peak blur (the same way Resolve's Edges: Wrap whip works) | model, render |
| Letterbox effect at any aspect (2.39 cinema bars, or pillarbox) | model, render, agent |
| Live reload: the app notices when the project file is changed by another program (CLI, script, headless CoCaptain), and with unsaved local edits it warns instead | app |
| Automatic proxies for footage above HD, made in the background, two at a time (a setting, on by default) | app, media |
| `copilot_script` example: runs a JSON list of CoCaptain tool calls on a branch; `--live` saves after each call so the open window follows along | app/examples |

## Performance: measured on this project (M4 Pro, 12 cores)

| | before | after |
|---|---|---|
| Preview playback, stall at each cut | 300–470 ms | ~35 ms (real time) |
| Preview playback, frames per second (unpaced) | 41 | 162 |
| Scrub, one jump | 310 ms (no proxy) | 67 ms (proxy) |
| Grade layer, one 4K frame | 129 ms | 25 ms |
| Grade layer, one preview frame (960x540) | 7.8 ms | 2.8 ms |
| 4K export of the 15 s reel | 148 s | 55 s |
| Proxies for both clips (13 min of 4K) | 2 min 54 s | 1 min 44 s |
| Thumbnail on import | full 4K software decode | 640 px, video chip |

Output quality is unchanged: the new export against the old one gives PSNR 45.4 dB and SSIM 0.992. A test checks
that playing through cuts gives byte-identical frames to seeking to each frame.

### What made the difference

1. **Decoder read-ahead.** During playback and export, the decoder of the next clip starts 0.8 s before the cut.
   It runs at the right frame, so its seek is finished by the time the cut arrives
   (`Renderer::read_ahead`; only for sequential frames, so scrubbing does not start decoders it will not use).
   Old decoders are shut down on a background thread.
2. **Hardware decoding only where it pays.** VideoToolbox gets a 4K HEVC original to its first frame in about
   half the time software takes. A proxy is small and every frame is a keyframe; there, starting the hardware
   decoder (~75 ms) costs more than it saves, so proxies decode in software.
3. **Grade as lookup tables.** Exposure, temperature, contrast and tint are folded into three 256-entry tables
   per clip per frame and applied when each texel is read. Only saturation and highlights/shadows run per pixel,
   and opaque pixels skip the un-premultiply.
4. **Effects at the size they need.** Glow gathers its bright pass straight into a picture a quarter or an
   eighth of the size and blurs that. Sharpen is a luma unsharp mask (one channel instead of four), with a
   transpose-free vertical blur. Glow, colour wheels, letterbox and the adjustment blend run on every core.
5. **Memory.** Canvases are recycled (`Renderer::recycle`) instead of allocating 133 MB per 4K frame.
   Adjustment layers run their effects in place (`apply_effects_owned`).
6. **Export pipeline.** A writer thread converts frame N to 8-bit and pipes it to the encoder while frame N+1
   renders, and the conversion is parallel.

Benchmarks: `cargo run --release -p captaine-app --example project_bench -- <project> [--realtime] [--scrub N]
[--full]` prints the per-stage time per frame. Also `crates/render/examples/adjust_bench.rs` and `bench.rs`.

## Still open

- Scrubbing without proxies is bound by FFmpeg starting up and seeking (a process per jump). In-process decoding
  (AVFoundation or libav) would remove the ~40 ms process start.
- Compositing is on the CPU. A wgpu compositor is the next big step for 4K effects in real time.
- Speed ramps are linear. Resolve's time-remap curves (slow, then fast) are approximated by a linear ramp.
- Export decodes in software so that output is bit-exact. Hardware decode for export could be an option.
