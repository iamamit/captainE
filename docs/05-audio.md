# 05 — Audio

## Engine

- Separate **real-time audio thread** (via `cpal` or the OS directly: CoreAudio, WASAPI, ALSA/PipeWire).
  Rules: no allocation, no locks, no I/O, no logging on that thread; lock-free queues for parameter changes.
- Internal format: 32-bit float, project sample rate (48 kHz default), per-track channel layouts (mono, stereo,
  5.1, 7.1, later object/ambisonic).
- Decode + resample ahead of time into ring buffers (the decode side runs on the job system); high-quality
  resampling (e.g. libsoxr-class quality).
- **Master clock**: playback time is driven by the audio device clock; video follows it.

## Signal flow (Fairlight/Premiere-style)

```
clip → clip gain/fades/keyframed volume → clip FX → pan → track (fader, EQ, dynamics, inserts, sends)
     → buses (submix, FX returns) → master (limiter, loudness meter) → output / export
```

- Automation lanes per parameter (volume, pan, mute, FX params), with read/touch/latch/write modes later.
- Clip-level and track-level effects both exist (codit showed both are needed: dialogue leveler per clip, EQ per track).
- Sidechain inputs for ducking (music under voice).

## Built-in processors (first set)

EQ (parametric, 6+ bands), compressor, expander/gate, limiter (true-peak aware), de-esser, de-hum, reverb, delay,
pitch, stereo width. These are well-documented DSP (biquad filters per the RBJ Audio EQ Cookbook, feed-forward
compressors with RMS/peak detectors, etc.) and can be implemented directly.

## Plugins

| Format | License | Plan |
|---|---|---|
| **CLAP** | MIT | First-class host support; modern, permissive, good threading model |
| **LV2** | ISC | Host support (Linux especially; Ardour/Qtractor show it works) |
| VST3 | GPLv3 or proprietary | Only if the licensing fits the chosen project license ([11](11-licensing-and-legal.md)) |
| AU | Apple | macOS host later |

Plugins run out-of-process optionally (crash isolation), in-process for low latency.

## Loudness and metering

- **ITU-R BS.1770** (K-weighting, gated integrated loudness, true peak) and **EBU R128** (−23 LUFS, ≤ −1 dBTP;
  momentary 400 ms, short-term 3 s, integrated, loudness range).
- **libebur128** (verified against the EBU conformance suite) for measurement.
- Platform targets as export presets (e.g. streaming ≈ −14 LUFS, podcast ≈ −16 LUFS, broadcast −23 LUFS).
- The agent's audio skills (from codit: speech-aware loudness, burst dipping, measure-before-claiming) run on this.

## Time-stretch and pitch

- **Rubber Band** (phase vocoder, transient-aware; GPL or commercial license) for high quality.
- **SoundTouch** (WSOLA, LGPL) as the permissive-license fallback.
- Used for clip speed changes with pitch preservation, and for fitting music to length.

## AI audio (see 09)

Speech recognition (Whisper / whisper.cpp) with word timestamps, voice activity detection (Silero VAD, MIT),
speech enhancement / noise removal (DeepFilterNet, RNNoise), music/dialogue separation (Demucs / HT Demucs),
automatic ducking, filler-word and silence detection, loudness normalization.
