# captainE status: the plan, what is built, what is left

**One page to answer "where are we?".** Update it after every build that adds, changes or removes a feature (see
the rule in [CLAUDE.md](../CLAUDE.md)). The plan is [12-roadmap.md](12-roadmap.md); the feature scorecard is
[parity.tsv](parity.tsv) (`python3 tools/parity.py --detail`).

Last updated: **2026-10-18** · parity score **78.2%** (target 75%, reached 2026-10-10) · last commit `44aba16`;
everything under "Recent changes" since then is not committed yet.

## Roadmap phases

| Phase (docs/12) | State | Built | Left |
|---|---|---|---|
| 0 Spikes and decisions | Done | egui chosen ([15](15-ui-status.md)); FFmpeg + VideoToolbox decode; cpal audio; OTIO round trip | — |
| 1 Engine core | Done | time, model, ops (undo/redo, op log), media probe, proxies, thumbnails, waveforms, CPU compositor, mixer, R128 loudness, H.264/HEVC/ProRes export, CLI ([14](14-phase-1-status.md)) | GPU compositor; OCIO colour pipeline; AV1 export |
| 2 Editor MVP | Done | window with pool, viewer (source + program), inspector, timeline (snapping, zoom, ripple/roll/slip/slide, blade, lift/extract, link), markers, JKL, titles, transitions, keyframes, OTIO/EDL/FCPXML | FCPXML import; AAF |
| 3 Built-in co-editor | Done | CoCaptain (Claude agent, 49 tools incl. look / look_media (sees frames), import, new/open project, export, review branch, Preview/Apply), local skills, Whisper transcripts, scene detection, live eval harnesses (`cocaptain_ask`, `cocaptain_color_eval`) | VAD-based analysis index; broader eval suite on fixture projects |
| 4 Branches and compare | Done | commits, branches, history, diff, Compare view, three-way merge, agent branches ([16](16-phase-4-status.md)) | — |
| 5 Audio depth | Partly | EQ, compressor, limiter, de-esser, spectral noise reduction, auto duck, CLAP hosting, pitch-kept speed, voice-over recording, loudness normalise | buses and sends, automation lanes, LV2, real time-stretch, AI voice cleanup (DeepFilterNet), stems (Demucs), input device choice / countdown for voice-over |
| 6 Motion graphics | Partly | titles (6 templates, any font), shapes, masks (drawn polygons), adjustment layers, animation presets, AI subject cut-out, 60 picture effects with keyframable settings ([20](20-effects.md)) | compositions and precomps, graph editor, expressions, per-letter text animators, bezier paths, Lottie import |
| 7 Color | Partly | **Color page**: shot strip, primaries, lift/gamma/gain wheels, curves, HSL, LUTs, scopes, before/after, copy/paste/reset/match grade; auto colour; HDR/HLG/PQ input | node graph, qualifiers (secondaries), tracked windows, ACES, tetrahedral LUT sampling, CDL |
| 8 Advanced AI and effects | Partly | speed ramps, stabilisation (vid.stab), auto-reframe, object tracking (one box), background removal | optical flow / frame interpolation, super-resolution, deblur, OFX hosting, point/planar tracking |
| 9 Collaboration and release | Partly | macOS .app bundle, CI on macOS/Linux/Windows, crash log, licensing rules and checks ([LICENSING](../LICENSING.md), [CONTRIBUTING](../CONTRIBUTING.md)) | remotes (push/pull, media sync), review links, auto-update, Windows installer, Linux package, notarisation, public beta, ads integration (UI only, consent) |

## Window: pages in the top bar

| Page | State |
|---|---|
| Media | **Removed 2026-10-16.** It was a placeholder from the first mockup with no plan behind it; media is brought in and organised in the Edit page's pool |
| Cut | Placeholder (shows "planned"). No plan in the docs; candidate to remove or replace |
| Edit | Real |
| Color | Real (2026-10-15) |
| Effects | **Real (2026-10-16).** Library (60 picture effects, 24 transitions, search, favourites, live previews), Effect Controls with keyframes on every effect setting; see [20](20-effects.md) |
| Audio | **Planned, not built** (asked for 2026-10-16). Phase 5: full-size mixer with buses and sends, volume automation lanes, loudness, voice cleanup, voice-over recording with input choice and countdown |
| Deliver | **Real page (2026-10-16)**: render settings (presets, format, quality, range, file in `exports/` next to the project by default), viewer and timeline for marks, render queue with progress, cancel and Show in Finder; the top bar's Render button shows progress |

## Weakest features (parity status 0.5 or 0)

- d03 Hardware encode: VideoToolbox only; no NVENC/QSV, no hardware ProRes.
- a08 Voice-over: no input device choice, countdown or punch-in.
- i06 Generative fill/extend: out of scope (kept so the score stays honest).

## Recent changes

Newest first. Keep this to the last few weeks; older detail lives in the numbered docs.

- 2026-11-18: Release 0.1.0 after a full pass over the CLI and the editor window with real iPhone footage
  (HEVC HLG, variable frame rate, HEIC, PNG). Fixed: HEIC import (converted with `sips` on macOS so the photo, not
  the gain map, is read), stills in import/decode/proxies and their own bin, import that keeps going past a bad
  file and skips duplicates, EDL re-import keeping clip properties and per-kind track numbers, titles landing on a
  free track, CoCaptain accepting a plain-words final reply. See CHANGELOG.md.
- 2026-11-04: Delete a gap. Click an empty stretch between clips to select it (it lights up), then ⌫/⌦ or
  right-click → Delete Gap: everything after it moves left on every track, so picture, sound and titles stay in
  sync (`edit::close_gap`, one undo step). Refused with a message when another track has something inside that
  span. "Clean Up Gaps" only merged neighbouring gaps and trimmed the trailing one, so a gap left by a cut in the
  middle of a sequence could not be closed before.
- 2026-11-04: Long caption and centred titles wrap inside the frame instead of running off the sides (it showed
  on 9:16 reels); the caption pill grows upwards from its bottom line.

- 2026-10-26: Pictures and files go into the CoCaptain chat. Paste a picture with ⌘V (a screenshot or a copied
  image; on macOS a key monitor catches ⌘V because the windowing layer drops it when the clipboard holds no text),
  pick files with the + button, or drop files on the chat (dropped anywhere else they are still imported). Pictures
  are sent with the message as images (scaled to at most 1568 wide); videos and other files go by path, for the
  look and import tools. They show as thumbnails above the message box (× removes one) and in the sent message.
- 2026-10-26: CoCaptain works the editor itself, not only the timeline: `import_media` (files into the pool as
  part of its edit, so it can import and place in one request), `new_project`, `open_project` and `export_video`
  (done by the editor after the reply; a new project lands in ~/Movies/captainE with a free name). Asked "create a
  blank project" it now makes one instead of saying it cannot. Also fixed: switching projects (File > New/Open)
  saved nothing first, so edits from the last moments could be lost; the open project is now saved before it
  goes. 49 tools.
- 2026-10-26: CoCaptain is a docked chat. It runs the full height on the right of every page (drag its edge to
  resize; the width is kept): the conversation fills it, replies are plain text with bullets and bold, what it did
  folds under each reply ("2 steps"), the pictures it looked at show as thumbnails (hover for a big one), and while
  it works a spinner says what it is doing. The message box sits at the bottom (Enter sends, Shift+Enter new line).
  Seeing the video, a new conversation and the Claude connection moved into the ••• menu; starter prompts show
  only in an empty conversation. When CoCaptain wants to look before the user allowed it, it stops at once and the
  chat asks "CoCaptain wants to see your video" with Allow and continue, which re-asks the same request.
- 2026-10-26: CoCaptain can see (phase 1 of "watch a reference and edit like a person", plan in docs/08). New
  tools `look` (the timeline at chosen times or every clip's middle, with CoCaptain's own pending edits, as a
  labelled contact sheet) and `look_media` (frames of any video file). Pictures travel as image blocks; through
  Claude Code the request is now stream-json input. "Let CoCaptain see the video" in the panel (off by default,
  kept in prefs) allows it; at most 40 pictures a request, older ones dropped from the conversation. System prompt
  rewritten (it still claimed speed, keyframes, transitions and transcripts were impossible); room raised to 8192
  tokens and 120 tool calls. Live: asked "does the colour grading look good?" on Six Locations it looked at two
  sheets and named the shots that need work and why.
- 2026-10-26: Sign in to Claude from CoCaptain. When CoCaptain works through Claude Code and its sign-in has run
  out, the panel says "Signed out" (red dot) instead of "Ready" and shows a Sign in to Claude button: it runs
  `claude auth login`, which opens the sign-in page in the browser, takes a code pasted from the page if one is
  shown, and checks `claude auth status` again when it finishes. A request that comes back signed out brings the
  same button up.
- 2026-10-26: CoCaptain's delete without ripple no longer leaves a gap at the end of a track (deleting an 89 s
  audio clip left 89 s of gap, so the timeline stayed 89 s long); gaps are merged and a trailing one dropped.
- 2026-10-26: Cleaner cut-outs. The subject matte gains a Refine setting (default 100%): a guided filter
  (He, Sun and Tang 2010) snaps the matte's soft, stretched edge to the edges in the picture and the edge is then
  steepened, so no blurred ring of the old background is left around the subject. Old projects open with it on.
- 2026-10-26: A media file replaced on disk under the same name now shows its new picture in the viewer after the
  next edit. The renderer kept its decoders and cached frames across edits without checking the file, so the old
  picture stayed until the app restarted; it now compares each file's size and modification time and decodes a
  changed one again.
- 2026-10-26: Remove background finds small subjects. A person far from the camera (a figure ~6% of the frame
  wide) came back as an empty matte: Vision's segmentation misses subjects that small. When the whole-frame matte
  is empty, people (person mode) or salient objects (subject mode) are now located, a square crop around each is
  segmented at a size the model sees, and the pieces are put back, with the faint haze of the crop's matte cut
  off. Matte frames are read 1280 wide instead of 640, and the matte cache key carries a version so mattes from
  the old method are made again.
- 2026-10-26: Title text fixes. The drop shadow is now blurred (glyphs drawn into a box around the text, blurred
  by shadow/2, scaled so thin faces cast as dark a shadow as heavy ones) instead of two hard offset copies, which
  read as stacked outlines. Faux bold (shifted extra passes) only thickens the bundled font; a chosen font file
  keeps the weight picked, so a thin face such as Avenir Next Ultra Light no longer turns into parallel strokes.
  The Centered template's built-in hard shadow is skipped when the title has its own shadow set.

- 2026-10-18: Sharper still shots moved off-centre: a clip with no moving position, scale or rotation, drawn at
  about 1:1, is placed on whole canvas pixels (a fraction of a pixel of offset blurred the whole frame); warp
  effects (corner pin, lens distortion, bulge, twirl...) sample bicubic instead of bilinear. A 4K clip in a 3:4
  reel, moved 18 px and corner-pinned, measured 362 -> 652 (straight FFmpeg crop: 786); unwarped 543 -> 759.
- 2026-10-17: Sharper renders: each clip is decoded at the size it is shown (its scale, the smallest scale key,
  camera-shake zoom) with FFmpeg's Lanczos filter, so the compositor draws it 1:1 instead of averaging a 4K frame
  down. A 4K clip on a 1080p timeline measured 574 -> 651 (DaVinci Resolve 21: 710, FFmpeg Lanczos: 708);
  exports of 4K footage about 3x faster.
- 2026-10-17: HSL fix: hue and lightness changes on a colour band now fade out toward grey (by chroma), so
  darkening a green floor no longer darkens warm-grey walls and clothes, as in pro colour tools.
- 2026-10-17: Timeline selection box: drag across empty space to select every clip it touches (shift adds),
  as in other editors; the selection moves as a group. Parity e05 to 1.
- 2026-10-17: CoCaptain's message box takes several lines: Shift+Enter for a new line, Enter sends; it grows to
  a few lines, then scrolls.
- 2026-10-16: Deliver is a page instead of a popup. Timeline: with more tracks than fit, they scroll inside it
  (wheel, or the scrollbar; shift-wheel pans time) instead of pushing the timeline off the window. Inspector:
  one "+ Add effect" menu by category instead of 60 buttons.
- 2026-10-16: Viewer playback resolution: 1/2 and 1/4 are now of what Auto shows (they were of the timeline, so on a
  4K or vertical reel in a small viewer they rendered more pixels than Auto and looked the same); the menu says it
  applies while playing.
- 2026-10-16: **Effects page.** 40 new picture effects (60 in all) and 10 new transitions (24), keyframes on every
  effect setting, library with search/favourites/live previews, drag onto clips and cuts, Effect Controls; CoCaptain
  tool descriptions generated from the catalogue, effect keyframes in `set_keyframes`, chroma key picks the
  backdrop colour from the shot; sharpen and small blurs fixed at small frame sizes; spin/zoom/directional blur twice
  as fast; live CoCaptain effects test (69 plain-English requests on the PushDay reel: 62, then 67, then 68 of 69,
  the last case passing on its own after its check was corrected). Audio page added
  to the plan (not built). Parity 77.2% -> 78.2% (v06, v07).
- 2026-10-16: Media page tab removed (placeholder). Menu bar: hover switches between open menus, Escape closes,
  shortcuts pause while a menu is open.
- 2026-10-16: CoCaptain colour eval 23/23 live; fixes: LUT path in params, match_color vs copy_grade wording,
  colour keyframes shown in get_timeline, warned about in set_clip and cleared by reset/copy grade.
- 2026-10-15: Color page; wheel puck fix; edit_effect tool; 27 colour tool tests; CoCaptain rename; mark in/out
  icons; cE badge; licensing docs and CI license check; performance pass ([18](18-reel-test-and-performance.md)).
