# 06 — Motion graphics and compositing (the After Effects side)

## Goal

Stop the edit/motion round-trip. Any clip or range on the timeline can become a **composition**, edited in place,
and compositions are just items on tracks. Two views of the same data:

- **Layer view** (After Effects style): a stack of layers with transforms, masks, effects and keyframes on a local
  timeline. Best for titles, lower thirds, UI animations, text behind subject, screen-recording callouts.
- **Node view** (Fusion/Nuke/Olive style): the underlying effect graph. Best for keying, complex compositing,
  reusable effect chains.

Internally a layer stack **is** a node graph (each layer = source → transform → effects → merge over the layers
below), so both views edit the same thing.

## Composition model

```
Composition { id, size, rate, duration, background, layers[] }
Layer {
  id, name, kind: Footage | Solid | Shape | Text | Composition (precomp) | Adjustment | Null | Camera(later),
  in/out (local time), parent (for parenting), transform {anchor, position, scale, rotation, opacity},
  masks[] (bezier paths, feather, expansion, mode), matte (track matte: alpha/luma, inverted),
  blend_mode, effects[] (node graph), 3D flag (later)
}
```

## Animation

- Keyframes on every param with temporal + spatial interpolation (hold, linear, bezier, auto/continuous bezier);
  graph editor for value/speed curves; motion paths for positions. ([03](03-timeline-and-data-model.md))
- **Expressions**: a small sandboxed language (JavaScript subset, or a simple expression language) with the
  common After Effects functions: `time`, `wiggle(freq, amp)`, `loopOut("cycle")`, `linear(t, tMin, tMax, v1, v2)`,
  `ease()`, references to other layers' params. Deterministic (seeded noise) so renders are reproducible.
- Text animators: per-character/word/line ranges with offsets (typewriter, cascade, kinetic type).

## Rendering 2D vector content

- **Skia** for shapes, paths, strokes, gradients, text outlines (GPU-accelerated; Friction proves it works for
  After-Effects-style animation). Results feed the GPU compositor as textures.
- Shape layers: rectangles, ellipses, polygons/stars, bezier paths; operators: trim paths, repeater, offset,
  rounded corners, merge paths.

## Templates and interchange

- **video-template-engine** templates (`template.json` + deterministic `seek(t)` HTML/JS): load as a layer type,
  rendered frame-accurately; later converted to native compositions where possible. This is how the agent adds
  polished motion graphics without generating them from scratch.
- **Lottie** (Bodymovin JSON) import/export: keyframes with `t`, `s`, `e`, bezier `i`/`o` easing handles and
  hold flags map directly onto CaptainE curves.
- OpenFX plugins as effect nodes ([02](02-architecture.md)); OFX is a C API of header files only, with a
  host ↔ plugin negotiation of image formats, regions and threading.

## Compositing features (phased)

| Phase | Features |
|---|---|
| First | Transforms, opacity, blend modes, masks, track mattes, text, shapes, precomps, adjustment layers, basic effects (blur, glow, drop shadow, color) |
| Second | Chroma key (with spill suppression), luma key, AI subject matting (segmentation + matting, see 09) → "text behind subject" in one step |
| Third | Point/planar tracking (KLT feature tracking, homography estimation), stabilization, corner pinning, motion blur (shutter-angle based) |
| Later | 2.5D/3D layers with a camera, particles, OFX third-party plugins |

## Why this matters for the agent

Requests like "add text behind me", "zoom into the code on screen with a callout", "animate this title in" are the
most common creator asks. In Resolve, codit struggles with them (Fusion comps built by API don't even render until
re-imported). In CaptainE they're ordinary compositions the agent can build with ops and verify by rendering frames.
