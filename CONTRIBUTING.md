# Building captainE: the rules

captainE is **distributed free and earns from ads**. Free distribution does **not** make the project
non-commercial: ad revenue is commercial use. Every rule below assumes that, and applies to every contributor,
**human or AI** (Claude Code, CoCaptain, Codex, Copilot and others).

The goal is simple: **build our own things from scratch, take nothing we have no right to, so that no company
(Adobe, Blackmagic, Apple, Avid or anyone else) has grounds to sue.** When unsure, stop and ask. Do not guess.

Related: [LICENSING.md](LICENSING.md) lists every tool and why it is free.
[docs/11](docs/11-licensing-and-legal.md) explains the background.
[docs/19](docs/19-ip-and-license-audit.md) is the latest audit.

---

## 1. Write it yourself

- **Write every feature from scratch** from public knowledge: textbooks, papers, published standards, official
  format specs, and our own experiments.
- **Never copy code** from other products or projects whose license we don't follow. That covers Adobe,
  Blackmagic and Apple sample code under restrictive terms, decompiled apps, leaked sources, and **GPL editors**
  (Kdenlive, Shotcut, Olive, OpenShot, Natron, Blender, Friction). Reading their docs to understand an idea is
  fine; copying, translating or closely paraphrasing their code is not.
- **Stack Overflow, blogs and forum code are not free to paste.** Stack Overflow content is CC BY-SA
  (share-alike). Learn the idea, then write your own version.
- **No reverse engineering of other products.** Don't decompile Premiere or Resolve, and don't take apart their
  private project files (`.prproj`, `.drp`) to clone them. Exchange timelines only through **published** formats:
  EDL, OpenTimelineIO, FCPXML and SRT.
- **AI-written code follows the same rules.** If an assistant produces something that looks like a recognisable
  copy of another project's code, rewrite it. Never ask an AI to reproduce another product's source.
- When an algorithm comes from a paper or standard, cite it in a comment (for example "EBU R128",
  "Fritsch–Carlson 1980"). That records where the idea came from.

## 2. Assets: only our own

Do not add to the repo or the app any icons, sounds, music, fonts, LUTs, presets, templates, titles, transitions
or images taken from other products or the web. That includes Adobe Stock and Adobe Fonts, Resolve's LUTs and
titles, Apple's SF Symbols outside Apple's terms, and Envato or Motion Array packs.

Allowed:

- things we draw or generate ourselves (the app's icons are drawn in code);
- assets under **CC0 / public domain**, with the source noted;
- fonts under **OFL** (or a similar license that allows bundling), with their license text added to
  `THIRD_PARTY_NOTICES.txt`.

Test media must be **generated** (FFmpeg's `testsrc`, `color`, `sine`) or footage **we shot ourselves**. Never
commit commercial songs or other people's videos. The songs and stock in personal projects stay out of the repo.

## 3. Libraries: allowed licenses only

| Status | Licenses |
|---|---|
| **Allowed** | MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode, CC0, BSL-1.0, Unlicense, CDLA-Permissive; OFL and the Ubuntu Font Licence for fonts; **MPL-2.0** only when the library is used unmodified |
| **Ask first** | LGPL (only as a separate program or a dynamic library, never compiled in), anything not on the allowed list |
| **Never** | GPL, AGPL, SSPL, BUSL, "Commons Clause", any **NonCommercial (NC)** license, code with **no license**, proprietary SDKs we have no agreement for |

- `python3 tools/license_check.py` enforces this table on every push (CI fails on a forbidden license).
- After adding or updating a dependency, regenerate the notices with `python3 tools/third_party_notices.py`.
- **External programs** (FFmpeg and so on) are run, not linked. **Never bundle an FFmpeg build that includes
  x264/x265 or other GPL parts.** If FFmpeg is ever bundled, use an LGPL build and ship its license and source
  offer.
- **Codecs:** H.264/HEVC encoding in shipped builds goes through the operating system (VideoToolbox on macOS,
  Media Foundation on Windows), whose vendors license the patents. Do not ship our own H.264/HEVC encoder.
- **AI models:** check both the code license **and the weights license** before using a model. No
  non-commercial weights. Download models on demand; don't bundle them unless their license allows it.
- **Camera SDKs** (Blackmagic RAW, RED, ARRI): only under the vendor's agreement, reviewed first.

## 4. Names and look

- Don't use other companies' **trademarks or feature names** in the UI or the code's public names. Write
  "Reframe", not "Auto Reframe" (Adobe). Write "Subject mask", not "Magic Mask" (Blackmagic). Write
  "CoCaptain", not "Copilot" (Microsoft/GitHub). Avoid "Lumetri", "Fairlight", "Fusion", "Essential Graphics",
  "Warp Stabilizer", "Speed Warp", "Smart Reframe".
- Saying what we are compatible with or compare to is fine in docs ("exports Final Cut Pro XML", "like
  Premiere's timeline"). **No competitor names or logos in ads, store listings or app branding.**
- Design our own look. The common editor layout (media pool, viewer, inspector, timeline) is fine. Copying
  another app's exact screens, icons, colours or trade dress is not.

## 5. Ads and user data (because ads pay for captainE)

- **Ads go in captainE's own interface only. Never in the user's exported videos**, unless the user turns on a
  visible option.
- **Privacy:** no tracking or ad SDK may read media, projects, transcripts or CoCaptain chats. EU users (GDPR
  and ePrivacy) must **consent** before personalised ads or any tracking. Without consent, show only
  non-personalised ads.
- Check every ad or analytics SDK under rule 3: its license, and its data-sharing terms.
- CoCaptain sends project details to Claude **only** with the user's own key or login, and the UI says what is
  sent. Frames of the user's video go only after they turn on "Let CoCaptain see the video" (off by default).

## 6. Before you merge: checklist

- [ ] All new code was written for captainE (rule 1). Sources of algorithms are cited in comments.
- [ ] No new third-party assets, or only CC0/OFL ones with their source and license recorded (rule 2).
- [ ] `python3 tools/license_check.py` passes. `THIRD_PARTY_NOTICES.txt` was regenerated if dependencies
      changed (rule 3).
- [ ] No other company's trademark or feature name in the UI (rule 4).
- [ ] No user data reaches ads or analytics (rule 5).
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --all -- --check` pass.
- [ ] [docs/STATUS.md](docs/STATUS.md) says what this change built or removed (and `docs/parity.tsv` if a status moved).

If something here is unclear, or a case isn't covered, **ask the maintainer before writing the code.**
