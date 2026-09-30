# 04 — Rendering and playback

## Decoding

- **FFmpeg (libavformat/libavcodec)** for demuxing and software decoding of almost every format; LGPL build only
  ([11](11-licensing-and-legal.md)).
- **Hardware decode** through FFmpeg's hwaccel layer or native APIs:
  - macOS: **VideoToolbox** (CVPixelBuffer on IOSurface → Metal texture, zero-copy)
  - NVIDIA: **NVDEC** (CUDA/GL/D3D interop for zero-copy)
  - Intel/AMD on Linux: **VA-API**; cross-platform: **Vulkan Video**
  - Windows: D3D11VA/D3D12VA
- Using OS/hardware decoders also moves codec **patent** licensing to the platform vendor for H.264/HEVC.
- **Seeking**: long-GOP codecs (H.264/HEVC) must decode from the previous keyframe; editing performance depends on
  it. Hence proxies (below) and a per-stream "decoder session" that stays warm near the playhead.
- Image sequences and stills via **OpenImageIO** (EXR, DPX, TIFF, PNG, …); camera RAW via vendor SDKs later
  (e.g. Blackmagic RAW SDK), LibRaw for stills.

## Proxies and optimized media

- Background job transcodes to an intra-frame, edit-friendly codec at lower resolution (ProRes Proxy/LT on macOS
  via VideoToolbox; DNxHR or MJPEG elsewhere; licensing checked per codec).
- Proxy ↔ original switch per viewer; export always uses originals.
- Proxies are keyed by the media's content hash, so they survive renames/moves.

## Caching (the key to "feels instant")

Layered caches, all keyed by content (media hash + source time + render-graph hash):

| Layer | Holds | Where |
|---|---|---|
| Compressed packet cache | Recent demuxed packets per stream | RAM |
| Decoded frame cache | Decoded source frames near the playhead | VRAM/RAM |
| Rendered frame cache | Final composited frames for timeline ranges | VRAM/RAM, spill to disk |
| Disk render cache | Expensive sections (heavy effects, AI) rendered once | disk (`cache/`) |

Prefetch strategy (from how pro systems do it):
- Priority queues: **urgent** (frames for the playhead now) → **normal** (look-ahead window in the play direction,
  a little behind) → **background** (scattered frames across the timeline for scrubbing, idle-time render of
  heavy regions).
- Deadline-aware: a frame that will be late is skipped, not waited for; playback drops frames rather than stalls.
- Eviction under memory pressure, least-recently-useful first; everything is regenerable.
- Invalidation: an op changes the render-graph hash only for affected ranges; unaffected cached frames stay valid.

## GPU pipeline

- Graphics API: **wgpu** (Metal on macOS, Vulkan on Linux/Windows, D3D12 on Windows) with WGSL shaders — one code
  path, native performance. (Alternative: direct Metal/Vulkan; decided in [10](10-tech-stack.md).)
- Working space: **linear light, float16/float32 RGBA, premultiplied alpha**.
- Per frame: source textures (YUV → RGB conversion in shader, correct matrix/range/transfer per stream) → per-clip
  transform (with good resampling: bicubic/Lanczos; mipmaps for downscale) → effects (shader passes or node graph)
  → track compositing → timeline color → output transform → viewer (display color space) or readback for encode.
- Compute shaders for heavy effects (blurs, noise reduction, optical flow warps), ping-pong textures between passes.

## Compositing math

- **Porter–Duff (1984)** operators on premultiplied RGBA: `over` is the default; also in, out, atop, xor.
  Premultiplied alpha is required for correct filtering, interpolation and blending.
- Blend modes (normal, multiply, screen, overlay, add, difference, …) as defined in the W3C Compositing and
  Blending spec; performed in linear light unless the user picks "legacy" (gamma-space) blending for look-matching.
- Transitions are two-input nodes (dissolve, dip, wipe, custom shader).

## Color management

- **OpenColorIO v2** as the color engine: native **ACES** implementation (no LUT files needed), GPU renderer that
  computes the same operators as the CPU.
- Per-clip input transform (from metadata or user), a timeline working space (e.g. ACEScct or a linear space),
  output transforms per deliverable (Rec.709/Gamma 2.4, sRGB, Rec.2100 PQ/HLG).
- Grading primitives (Color page equivalent, later phases): **ASC CDL** (slope/offset/power/saturation) as the
  interchange baseline; lift/gamma/gain wheels; curves; HSL qualifiers; power windows; 3D LUTs with
  **tetrahedral interpolation** (smoother than trilinear) evaluated on the GPU in float.
- Scopes: waveform, parade, vectorscope, histogram — compute shaders on the rendered frame.

## Text

- Shaping: **HarfBuzz**; fonts: FreeType/system font APIs; layout: own (lines, alignment, per-character
  animation for titles).
- Rendering: vector paths through **Skia** for quality at any size; **MSDF** (multi-channel signed distance field,
  Chlumský) glyph atlases for fast real-time text in the viewer.
- Subtitles: SRT/WebVTT import/export; burned-in or as separate streams on export.

## Export

- Encode via FFmpeg (LGPL codecs) or platform encoders (VideoToolbox H.264/HEVC/ProRes; NVENC; VA-API);
  AV1 through royalty-free encoders (SVT-AV1: BSD-style license plus the AOMedia patent license — verify the exact
  terms before shipping).
- Render queue with presets (YouTube, Reels/Shorts, podcast, master), smart re-use of cached frames, and
  **loudness-aware audio export** ([05](05-audio.md)).
- Deterministic renders: same project + commit → identical output (important for comparing branches).
