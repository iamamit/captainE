# 11 — Licensing and legal

Not legal advice; decisions to confirm with a lawyer before any commercial release.

## First decision: CaptainE's own license

| Choice | Effect |
|---|---|
| Open source, copyleft (GPL-3) | Can use GPL libraries freely (Rubber Band GPL, x264, VST3 GPL, Kdenlive/Olive/Natron code for reference or reuse); anyone can fork; a company can still sell services/cloud/support |
| Open source, permissive (MIT/Apache) | Maximum adoption; must avoid GPL dependencies (or make them optional plugins) |
| Proprietary / open-core | Can sell licenses; must avoid GPL code entirely, keep LGPL libraries dynamically linked, and buy commercial licenses (Rubber Band, x264, Qt if needed) |

This choice changes which libraries are allowed. **Until decided, only use permissive or LGPL libraries in the core**
(the list in [10](10-tech-stack.md) is chosen that way) and keep GPL-only pieces optional.

## Library license notes

- **FFmpeg**: LGPL-2.1+ by default; enabling GPL parts (e.g. libx264, libx265) makes the build GPL. For a
  non-GPL app: LGPL build, dynamic linking, ship the FFmpeg source/offer, allow relinking.
- **x264 / x265**: GPL or commercial license (x264 LLC). Prefer platform encoders (VideoToolbox, NVENC, VA-API)
  for H.264/HEVC export in a non-GPL build.
- **Rubber Band**: GPL or commercial (commercial also required for Apple App Store distribution). SoundTouch
  (LGPL) is the fallback.
- **VST3 SDK**: GPLv3 or proprietary license from Steinberg. **CLAP** (MIT) and **LV2** (ISC) have no such issue.
- **Qt**: LGPLv3 for most modules (dynamic linking, relinkable) or commercial.
- **Skia, OpenColorIO, OpenImageIO, OpenTimelineIO, HarfBuzz, ONNX Runtime, whisper.cpp**: permissive.
- **Reference code from GPL editors** (Kdenlive, Olive, Natron, Friction, Gyroflow): study freely; copying code makes
  CaptainE GPL.

## Codec patents

- **H.264/AVC and HEVC** are patent-encumbered (patent pools). Using the **operating system's or GPU vendor's
  codecs** (VideoToolbox, Media Foundation, NVDEC/NVENC) generally means the platform vendor has licensed them —
  the usual approach for desktop apps. Shipping our own software H.264/HEVC encoder is where licensing costs appear.
- **AV1, VP9** are royalty-free (AOMedia patent license); good default for web delivery.
- **ProRes**: Apple's codec; decode/encode via VideoToolbox on macOS is fine; third-party implementations are a
  gray area — check before shipping on other platforms.
- **Camera RAW** (BRAW, R3D, ARRIRAW): vendor SDKs with their own terms.

## AI models

- Check both **code license** and **weights license** (and training-data restrictions) for every model:
  SAM 2, RVM, RIFE, RAFT, Real-ESRGAN, Whisper, Silero VAD (MIT), DeepFilterNet, Demucs, TransNet V2.
- Show model licenses in the app's model manager; download models on demand rather than bundling where terms
  require it.
- Cloud LLM use (Claude API): user consent for sending transcripts/frames; a fully local mode for sensitive projects.

## Trademarks / naming

- Check "CaptainE" availability (trademark search, domain) before public launch.
- Avoid other products' trademarks in UI (e.g. "Magic Mask" is Blackmagic's term — use generic names).
