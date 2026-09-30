# 10 — Tech stack

## Requirements that decide it

- Real-time audio thread and heavy multithreading without data races.
- GPU compute and rendering on macOS (Metal), Windows and Linux.
- Mature bindings to C libraries: FFmpeg, OpenColorIO, OpenImageIO, HarfBuzz, Skia, libebur128, ONNX Runtime.
- A complex, custom, high-performance UI (timeline with thousands of items, node editor, waveforms) plus
  ordinary panels (inspector, bins, settings, chat).
- One developer (plus AI assistance) at first: productivity matters.
- macOS (Apple silicon) first — it's the development machine.

## Core language

| Option | For | Against |
|---|---|---|
| **Rust** | Memory and thread safety for a heavily concurrent engine; wgpu; good FFmpeg/GStreamer bindings; Gyroflow proves a Rust core + GPU + Qt UI works for media | Some C++-only libraries (OpenColorIO, OpenImageIO) need binding work; fewer media-engine examples than C++ |
| C++ | Every media library is C/C++; Kdenlive, Olive, Natron, OpenShot, Friction are C++/Qt | Concurrency bugs; slower to build safely alone |
| TypeScript (web: WebCodecs + WebGPU) | Fastest UI iteration; browser editors (OpenReel, VEdit) show it works | Codec coverage, file-system access, performance ceilings and memory limits for long 4K projects; hardware decode control |

**Recommendation: Rust for the engine** (time, model, ops, vcs, media, cache, render, audio, ai runtime), exposing a
clean API. Keep the engine UI-independent so a web/lite version or headless render/agent server is possible later.

## GPU

- **wgpu** (WebGPU API in Rust: Metal, Vulkan, D3D12, GL; also runs in browsers). One shader language (WGSL).
- Escape hatch for platform-specific zero-copy interop (IOSurface/CVPixelBuffer ↔ Metal textures, CUDA interop)
  through wgpu's native interop hooks; verify in a spike.
- Skia for 2D vector (has a GPU backend; interop with wgpu textures to verify).

## UI toolkit

| Option | Fit | Risks |
|---|---|---|
| **Qt 6 / QML** (via cxx-qt from Rust) | The most proven choice for pro media apps (Kdenlive, Shotcut, Natron, Friction, Gyroflow, DaVinci Resolve itself uses Qt) | C++/Rust bridge complexity; commercial licensing if not LGPL-compatible (Qt is LGPLv3 for most modules) |
| **Slint** | Rust-native declarative UI, several renderers (incl. Skia) | Younger; big custom widgets (timeline) need work; licensing tiers |
| **egui / iced** (pure Rust) | Simple, fast immediate-mode (egui); good for custom-drawn timelines | Less "native" feel; accessibility; large-app UX polish |
| **Tauri (web UI) + native viewer** | Fast UI development in web tech; the chat/inspector/bins are easy | Embedding a native GPU viewer and a 60 fps timeline in a WebView is hard; IPC overhead |

**Recommendation: decide by spike (Phase 0).** Build the same two things in the top candidates — a timeline widget
with 5,000 clips scrolling at 60 fps, and a wgpu viewer surface playing 4K — then pick. Current lean: **Qt/QML +
Rust engine (cxx-qt)** for the proven path, with custom-drawn timeline and viewer; or **egui** if the spike shows
it's good enough, to stay all-Rust.

## Libraries

| Area | Library | License |
|---|---|---|
| Timeline interchange | OpenTimelineIO (C++ core; bindings) | Apache-2.0 |
| Demux/decode/encode | FFmpeg (LGPL build) — `ffmpeg-next`/`rsmpeg` bindings | LGPL-2.1+ |
| Images/EXR | OpenImageIO, OpenEXR | Apache-2.0 / BSD-3 |
| Color | OpenColorIO v2 (+ ACES configs) | BSD-3 |
| 2D vector | Skia (`skia-safe`) | BSD-3 |
| Text | HarfBuzz, FreeType, msdfgen | MIT / FreeType / MIT |
| Audio I/O | cpal | Apache-2.0/MIT |
| Loudness | libebur128 (or the Rust `ebur128` crate) | MIT |
| Time-stretch | SoundTouch (LGPL) / Rubber Band (GPL or commercial) | see 11 |
| Audio plugins | CLAP (MIT), LV2 (ISC) | permissive |
| Effects plugins | OpenFX headers | BSD |
| ML runtime | ONNX Runtime (MIT), CoreML (Apple), whisper.cpp (MIT) | permissive |
| Hashing / storage | BLAKE3, FastCDC | CC0/Apache / MIT |
| Serialization | serde (JSON for readable, a binary format for snapshots) | MIT/Apache |
| Agent | Claude API (Anthropic SDK); MCP for external tools | — |

## Repository layout (planned)

```
captainE/
  crates/
    time/ model/ ops/ vcs/ media/ cache/ render/ audio/ motion/ ai/ agent/ interchange/ plugins/
  app/            (UI shell)
  skills/         (agent playbooks)
  shaders/        (WGSL)
  docs/
  tests/fixtures/ (small media + projects for tests and agent evals)
```
