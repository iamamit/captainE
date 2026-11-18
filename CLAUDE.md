# Instructions for AI coding assistants working on captainE

captainE is distributed **free and earns from ads**. That is commercial use. Every change must keep it safe
from legal claims by Adobe, Blackmagic, Apple or anyone else.

**Before writing code, read and follow [CONTRIBUTING.md](CONTRIBUTING.md).** In short:

1. **Write everything from scratch.** Never copy, translate or closely paraphrase code from other products,
   GPL projects (Kdenlive, Shotcut, Olive, OpenShot, Natron, Blender), Stack Overflow or blogs. Don't reproduce
   another product's source from memory. Cite papers or standards for algorithms.
2. **No foreign assets.** No icons, fonts, LUTs, presets, sounds, music or images from other products or the
   web. Test media is generated (FFmpeg `testsrc`, `sine`) or our own footage.
3. **Dependencies:** only MIT, Apache-2.0, BSD, ISC, Zlib, Unicode, CC0, BSL-1.0, or MPL-2.0 used unmodified.
   Never GPL, AGPL, NonCommercial or unlicensed. Ask before anything else. Run `python3 tools/license_check.py`;
   regenerate `THIRD_PARTY_NOTICES.txt` with `python3 tools/third_party_notices.py` when dependencies change.
   Never bundle FFmpeg with x264/x265. Shipped H.264/HEVC encoding goes through the OS (VideoToolbox).
4. **No competitor trademarks or feature names** in the UI ("Reframe", not "Auto Reframe"; "CoCaptain", not
   "Copilot"; no "Magic Mask", "Lumetri" and so on).
5. **Ads only in captainE's own UI, never in exported videos.** No user media, projects or chats go to ad or
   analytics code. EU consent rules apply.

**After every build that adds, changes or removes a feature, update [docs/STATUS.md](docs/STATUS.md)**: the phase
table (built / left), the pages table, "Recent changes", the parity score, and `docs/parity.tsv` when a feature's
status moves. It is the one place that says what is planned, what is done and what is left.

When a request would break one of these rules, say so and suggest a from-scratch alternative instead of
doing it.

Project facts: Rust workspace (`crates/*`), egui UI (`crates/app`), headless CLI (`captaine`), CoCaptain agent
(`crates/agent`). Check changes with `cargo test --workspace --release`,
`cargo clippy --workspace --all-targets` and `cargo fmt --all -- --check`.
