# 09 — AI and media algorithms

Everything here runs **locally** as background jobs (ONNX Runtime, CoreML on Apple silicon, CUDA where present),
results cached by media hash. Licenses must be checked per model *and* per weights before shipping
([11](11-licensing-and-legal.md)); "open code" does not always mean "open weights for commercial use".

## Vision

| Feature (user-facing) | Algorithm / paper | Notes |
|---|---|---|
| Magic mask / object selection & tracking (click on a person → mask through the shot) | **SAM 2** — Segment Anything in Images and Videos (Ravi et al., ICLR 2025): promptable segmentation, streaming memory across frames, clicks/boxes/masks as prompts | Code and checkpoints on GitHub (Meta); the direct basis for "text behind subject", object blur, isolate-and-grade |
| Background removal / people matting (hair-level alpha) | **Robust Video Matting** (Lin et al., WACV 2022): recurrent architecture with temporal memory, real-time (4K 76 fps on a GTX 1080 Ti) | Better edges than segmentation alone; combine: SAM 2 mask → matting refine |
| Optical flow (for retime, motion blur, stabilization, warps) | **RAFT** (Teed & Deng, ECCV 2020): all-pairs 4D correlation volumes, recurrent updates | Heavier; classic Farnebäck/DIS flow as fast fallback |
| Frame interpolation (smooth slow motion, speed ramps) | **RIFE** (Huang et al.): IFNet estimates intermediate flow directly, 4–27× faster than SuperSlomo/DAIN; FILM (Google) for large motion | Arbitrary timestep → speed ramps |
| Stabilization | **Grundmann, Kwatra, Essa, CVPR 2011** — L1-optimal camera paths (constant/linear/parabolic segments like a real camera operator) via linear programming; feature tracking (KLT) + homographies; **Gyroflow**-style gyro stabilization when metadata exists | Well-defined, implementable without deep learning |
| Shot/scene cut detection | **TransNet V2** (Souček & Lokoč 2020) — handles gradual transitions; **PySceneDetect**-style HSV histogram thresholds as fast CPU baseline | Needed for the media index and "detect cuts" |
| Super-resolution / upscaling | **Real-ESRGAN** (trained on synthetic degradations; tiled inference) | Slow → background job with estimate |
| Deblur | Learned deblurring models (research varies) | Resolve's shows how slow this is (~11 s per 4K frame): estimate and ask first |
| Face detection / reframing (auto 9:16 crops following the speaker) | Face/person detection + saliency; smooth crop path with the same L1 path optimisation as stabilization | Key creator feature (Shorts/Reels from 16:9) |
| On-screen text (OCR) | Platform OCR (Apple Vision) or open OCR models | Indexes screen recordings: "zoom into the terminal when I type npm install" |
| Shot understanding | Vision-language model captions per shot → text index | LAVE-style footage descriptions for the agent |
| Color matching between shots | Statistical transfer (mean/variance in a perceptual space), then CDL fit | Classic; no DL required |

## Audio

| Feature | Algorithm / paper | Notes |
|---|---|---|
| Transcription with word timestamps, speakers | **Whisper** (Radford et al. 2022, 680k h weak supervision, 99 languages) via **whisper.cpp** / MLX (codit already uses mlx-whisper) | Word timing alignment; diarization model for speakers |
| Speech/silence detection | **Silero VAD** (MIT) | Silence removal, jump-cut detection, ducking |
| Filler words ("um", "uh"), repeated takes | Transcript + VAD + text similarity between takes | Talking-head tightening |
| Noise removal / speech enhancement | **DeepFilterNet** (48 kHz, real-time on CPU; open weights) ; RNNoise | Voice isolation |
| Dialogue / music / effects separation | **HT Demucs** (Rouard et al., ICASSP 2023), hybrid time/spectral with cross-domain transformer | Music remixer, stems |
| Loudness | ITU-R BS.1770 / EBU R128 via libebur128 | Not AI, but core to the agent's audio loop |
| Auto-ducking | VAD on voice + sidechain envelope on music | |
| Beat detection (cut to music) | Onset detection / beat tracking (classical DSP) | Montage and Reels editing |

## Editing intelligence (research to learn from)

| Work | Use in CaptainE |
|---|---|
| Fried et al. 2019, text-based editing of talking-head video | Edit-by-transcript; later: smoothing jump cuts |
| Leake et al. 2017, idiom-based dialogue editing | Encode editing idioms (speaker on screen when talking, avoid jump cuts, reaction shots) as agent skills + scoring |
| LAVE 2024 | Footage descriptions + plan-and-execute agent |
| ExpressEdit 2024 | Language + sketch on frame for spatial references |
| VideoDiff 2025 | Generating and comparing alternatives → agent branches |

## Runtime

- Model manager: download on demand, verify hashes, versioned; per-model license shown to the user.
- Jobs: progress, cancel, estimates from measured throughput on the user's machine (codit's deblur lesson).
- Results store: masks, flows, transcripts, embeddings keyed by (media hash, range, model version).
