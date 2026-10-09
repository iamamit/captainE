//! Picture effects on a decoded frame (straight-alpha RGBA8), run before the layer is transformed.

use captaine_model::{Effect, EffectKind};

use crate::Frame8;

/// The frame with every enabled effect applied in order, or `None` when there is nothing to do.
pub fn apply_effects(frame: &Frame8, effects: &[Effect]) -> Option<Frame8> {
    apply_effects_with(frame, effects, &[])
}

/// Like [`apply_effects`], with the matte picture for each Subject matte effect: `mattes[i]` belongs to
/// `effects[i]` (missing or `None`: the matte is not available, so the effect passes the picture through).
pub fn apply_effects_with(
    frame: &Frame8,
    effects: &[Effect],
    mattes: &[Option<std::sync::Arc<Frame8>>],
) -> Option<Frame8> {
    apply_effects_at(frame, effects, mattes, 0)
}

/// Like [`apply_effects_with`]; `phase` (the frame number) makes film grain change from frame to frame.
pub fn apply_effects_at(
    frame: &Frame8,
    effects: &[Effect],
    mattes: &[Option<std::sync::Arc<Frame8>>],
    phase: u64,
) -> Option<Frame8> {
    let mut active = effects
        .iter()
        .enumerate()
        .filter(|(_, e)| e.enabled && !e.kind.is_audio())
        .peekable();
    active.peek()?;
    let mut out = frame.clone();
    let k = (frame.height as f64 / 1080.0).max(0.01);
    for (i, e) in active {
        let p = &e.params;
        match e.kind {
            EffectKind::Blur => blur(&mut out, p[0] * k),
            EffectKind::Sharpen => sharpen(&mut out, p[0] / 100.0, 1.5 * k),
            EffectKind::Vignette => vignette(&mut out, p[0] / 100.0, p[1] / 100.0),
            EffectKind::Mirror => mirror(&mut out, p[0] >= 0.5),
            EffectKind::Crop => crop(&mut out, [p[0], p[1], p[2], p[3]].map(|v| v / 100.0)),
            EffectKind::ChromaKey => {
                chroma_key(
                    &mut out,
                    [p[0], p[1], p[2]],
                    p[3] / 100.0,
                    p[4] / 100.0,
                    p[5] / 100.0,
                );
                if p[6] != 0.0 {
                    shrink_alpha(&mut out, p[6] * k);
                }
                if p[7] >= 0.5 {
                    show_alpha(&mut out);
                }
            }
            EffectKind::Glow => glow(&mut out, p[0] / 100.0, p[1] * k, p[2] / 100.0),
            EffectKind::Grain => grain(
                &mut out,
                p[0] / 100.0,
                (p[1] * k).max(1.0),
                p[2] / 100.0,
                phase,
            ),
            EffectKind::Aberration => aberration(&mut out, p[0] * k),
            EffectKind::Duotone => duotone(
                &mut out,
                [p[0], p[1], p[2]],
                [p[3], p[4], p[5]],
                p[6] / 100.0,
            ),
            EffectKind::Invert => invert(&mut out, p[0] / 100.0),
            EffectKind::Posterize => posterize(&mut out, p[0].round() as u32),
            EffectKind::Pixelate => pixelate(&mut out, (p[0] * k).round().max(1.0) as usize),
            EffectKind::ColorWheels => wheels(&mut out, p),
            EffectKind::Curves => curves(&mut out, p),
            EffectKind::Hsl => hsl(&mut out, p),
            EffectKind::Lut => lut(&mut out, &e.text, p[0] / 100.0),
            EffectKind::Mask => mask(&mut out, p),
            EffectKind::Matte => {
                if let Some(Some(m)) = mattes.get(i) {
                    subject_matte(&mut out, m, p[0] >= 0.5, p[1] * k, p[2] * k);
                }
            }
            _ => {}
        }
    }
    Some(out)
}

/// Premultiplied floats, 4 per pixel.
fn to_premul(f: &Frame8) -> Vec<f32> {
    let mut v = Vec::with_capacity(f.rgba.len());
    for px in f.rgba.chunks_exact(4) {
        let a = px[3] as f32 / 255.0;
        v.extend([
            px[0] as f32 * a,
            px[1] as f32 * a,
            px[2] as f32 * a,
            px[3] as f32,
        ]);
    }
    v
}

fn from_premul(f: &mut Frame8, v: &[f32]) {
    for (px, s) in f.rgba.chunks_exact_mut(4).zip(v.chunks_exact(4)) {
        let a = s[3] / 255.0;
        if a <= 0.0 {
            px.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for c in 0..3 {
            px[c] = (s[c] / a).round().clamp(0.0, 255.0) as u8;
        }
        px[3] = s[3].round().clamp(0.0, 255.0) as u8;
    }
}

/// One horizontal box-blur pass of half-width `r` (edges repeat), over `ch` interleaved channels.
fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize) {
    let n = (2 * r + 1) as f32;
    for y in 0..h {
        let row = &src[y * w * 4..(y + 1) * w * 4];
        let out = &mut dst[y * w * 4..(y + 1) * w * 4];
        for c in 0..4 {
            let at = |x: isize| row[(x.clamp(0, w as isize - 1) as usize) * 4 + c];
            let mut sum = 0.0;
            for x in -(r as isize)..=(r as isize) {
                sum += at(x);
            }
            for x in 0..w {
                out[x * 4 + c] = sum / n;
                sum += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
            }
        }
    }
}

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    for y in 0..h {
        for x in 0..w {
            let (i, j) = ((y * w + x) * 4, (x * h + y) * 4);
            out[j..j + 4].copy_from_slice(&src[i..i + 4]);
        }
    }
    out
}

/// Gaussian-like blur (three box passes each way) of standard deviation `sigma` pixels, on premultiplied data.
fn blur_premul(v: Vec<f32>, w: usize, h: usize, sigma: f64) -> Vec<f32> {
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round() as usize;
    if r == 0 {
        return v;
    }
    let mut a = v;
    let mut b = vec![0.0; a.len()];
    for _ in 0..3 {
        box_pass(&a, &mut b, w, h, r);
        std::mem::swap(&mut a, &mut b);
    }
    let mut t = transpose(&a, w, h);
    let mut tb = vec![0.0; t.len()];
    for _ in 0..3 {
        box_pass(&t, &mut tb, h, w, r);
        std::mem::swap(&mut t, &mut tb);
    }
    transpose(&t, h, w)
}

fn blur(f: &mut Frame8, radius_px: f64) {
    if radius_px < 0.5 {
        return;
    }
    let (w, h) = (f.width as usize, f.height as usize);
    let v = blur_premul(to_premul(f), w, h, radius_px / 2.0);
    from_premul(f, &v);
}

fn sharpen(f: &mut Frame8, amount: f64, sigma: f64) {
    let (w, h) = (f.width as usize, f.height as usize);
    let src = to_premul(f);
    let soft = blur_premul(src.clone(), w, h, sigma);
    let mut out = src.clone();
    for ((o, s), b) in out
        .chunks_exact_mut(4)
        .zip(src.chunks_exact(4))
        .zip(soft.chunks_exact(4))
    {
        for c in 0..3 {
            o[c] = (s[c] + amount as f32 * (s[c] - b[c])).clamp(0.0, s[3]);
        }
    }
    from_premul(f, &out);
}

fn vignette(f: &mut Frame8, amount: f64, softness: f64) {
    let (w, h) = (f.width as f64, f.height as f64);
    let inner = 0.9 * (1.0 - softness);
    for y in 0..f.height as usize {
        for x in 0..f.width as usize {
            let (dx, dy) = ((x as f64 + 0.5) / w - 0.5, (y as f64 + 0.5) / h - 0.5);
            let d = (dx * dx + dy * dy).sqrt() / 0.5f64.hypot(0.5);
            let t = ((d - inner) / (1.0 - inner).max(1e-6)).clamp(0.0, 1.0);
            let s = t * t * (3.0 - 2.0 * t);
            let g = (1.0 - amount * s) as f32;
            let i = (y * f.width as usize + x) * 4;
            for c in 0..3 {
                f.rgba[i + c] = (f.rgba[i + c] as f32 * g).round() as u8;
            }
        }
    }
}

fn mirror(f: &mut Frame8, vertical: bool) {
    let (w, h) = (f.width as usize, f.height as usize);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = if vertical {
                (x, if y >= h / 2 { h - 1 - y } else { y })
            } else {
                (if x >= w / 2 { w - 1 - x } else { x }, y)
            };
            if (sx, sy) != (x, y) {
                let (i, j) = ((y * w + x) * 4, (sy * w + sx) * 4);
                let px = [f.rgba[j], f.rgba[j + 1], f.rgba[j + 2], f.rgba[j + 3]];
                f.rgba[i..i + 4].copy_from_slice(&px);
            }
        }
    }
}

fn crop(f: &mut Frame8, edges: [f64; 4]) {
    let (w, h) = (f.width as f64, f.height as f64);
    let (x0, y0, x1, y1) = (
        edges[0] * w,
        edges[1] * h,
        (1.0 - edges[2]) * w,
        (1.0 - edges[3]) * h,
    );
    for y in 0..f.height as usize {
        for x in 0..f.width as usize {
            let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
            if cx < x0 || cx >= x1 || cy < y0 || cy >= y1 {
                let i = (y * f.width as usize + x) * 4;
                f.rgba[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
}

/// Chroma (Cb, Cr) in -0.5..0.5 of an sRGB colour, BT.601.
fn chroma(r: f64, g: f64, b: f64) -> (f64, f64) {
    let (r, g, b) = (r / 255.0, g / 255.0, b / 255.0);
    (
        -0.168736 * r - 0.331264 * g + 0.5 * b,
        0.5 * r - 0.418688 * g - 0.081312 * b,
    )
}

fn chroma_key(f: &mut Frame8, key: [f64; 3], tolerance: f64, softness: f64, spill: f64) {
    let (kcb, kcr) = chroma(key[0], key[1], key[2]);
    let lo = tolerance * 0.5;
    let soft = (softness * 0.5).max(1e-4);
    // which channel the key colour is made of, for spill suppression
    let dom = (0..3)
        .max_by(|&a, &b| key[a].total_cmp(&key[b]))
        .unwrap_or(1);
    for px in f.rgba.chunks_exact_mut(4) {
        let (cb, cr) = chroma(px[0] as f64, px[1] as f64, px[2] as f64);
        let d = ((cb - kcb).powi(2) + (cr - kcr).powi(2)).sqrt();
        let keep = ((d - lo) / soft).clamp(0.0, 1.0);
        px[3] = (px[3] as f64 * keep).round() as u8;
        if spill > 0.0 && d < lo + soft * 3.0 {
            // pull the key channel down to the average of the other two
            let others = (0..3)
                .filter(|&c| c != dom)
                .map(|c| px[c] as f64)
                .sum::<f64>()
                / 2.0;
            let cur = px[dom] as f64;
            if cur > others {
                px[dom] = (cur - (cur - others) * spill).round() as u8;
            }
        }
    }
}

fn pixelate(f: &mut Frame8, block: usize) {
    if block <= 1 {
        return;
    }
    let (w, h) = (f.width as usize, f.height as usize);
    for by in (0..h).step_by(block) {
        for bx in (0..w).step_by(block) {
            let (ex, ey) = ((bx + block).min(w), (by + block).min(h));
            let mut sum = [0u32; 4];
            for y in by..ey {
                for x in bx..ex {
                    let i = (y * w + x) * 4;
                    for (acc, v) in sum.iter_mut().zip(&f.rgba[i..i + 4]) {
                        *acc += *v as u32;
                    }
                }
            }
            let n = ((ex - bx) * (ey - by)) as u32;
            let avg = sum.map(|s| ((s + n / 2) / n) as u8);
            for y in by..ey {
                for x in bx..ex {
                    let i = (y * w + x) * 4;
                    f.rgba[i..i + 4].copy_from_slice(&avg);
                }
            }
        }
    }
}

/// Grow (`px` > 0: the edge moves inward) or shrink the opaque area by `px` pixels.
fn shrink_alpha(f: &mut Frame8, px: f64) {
    let r = px.abs().round() as usize;
    if r == 0 {
        return;
    }
    let (w, h) = (f.width as usize, f.height as usize);
    let a: Vec<f32> = f
        .rgba
        .chunks_exact(4)
        .map(|p| p[3] as f32 / 255.0)
        .collect();
    let m = morph(&a, w, h, r, px > 0.0);
    for (i, v) in m.iter().enumerate() {
        f.rgba[i * 4 + 3] = (v * 255.0).round() as u8;
    }
}

/// Show the alpha channel as a grey picture (white = kept): for checking a key.
fn show_alpha(f: &mut Frame8) {
    for p in f.rgba.chunks_exact_mut(4) {
        let a = p[3];
        p[0] = a;
        p[1] = a;
        p[2] = a;
        p[3] = 255;
    }
}

fn luma8(p: &[u8]) -> f32 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0
}

/// Bloom: the bright parts, blurred, added back on top.
fn glow(f: &mut Frame8, threshold: f64, radius_px: f64, intensity: f64) {
    let (w, h) = (f.width as usize, f.height as usize);
    let t = threshold as f32;
    let mut bright = vec![0.0f32; w * h * 4];
    for (i, p) in f.rgba.chunks_exact(4).enumerate() {
        let l = luma8(p);
        // how far above the threshold, softly, so the edge of the bloom is not a hard line
        let k = ((l - t) / (1.0 - t).max(0.05)).clamp(0.0, 1.0);
        let a = p[3] as f32 / 255.0;
        for c in 0..3 {
            bright[i * 4 + c] = p[c] as f32 * k * a;
        }
        bright[i * 4 + 3] = 255.0 * k * a;
    }
    let soft = blur_premul(bright, w, h, (radius_px / 2.0).max(0.5));
    let gain = intensity as f32;
    for (i, p) in f.rgba.chunks_exact_mut(4).enumerate() {
        for c in 0..3 {
            // screen-style add so a bright area never clips harshly
            let (a, b) = (
                p[c] as f32 / 255.0,
                (soft[i * 4 + c] / 255.0 * gain).min(1.0),
            );
            p[c] = ((a + b - a * b) * 255.0).round() as u8;
        }
    }
}

fn hash(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// Film grain: noise in blocks of `size` pixels that is different every frame (`phase`).
fn grain(f: &mut Frame8, amount: f64, size: f64, colour: f64, phase: u64) {
    let (w, h) = (f.width as usize, f.height as usize);
    let cell = size.max(1.0);
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = ((x as f64 / cell) as u64, (y as f64 / cell) as u64);
            let n = hash(
                cx.wrapping_mul(73856093)
                    ^ cy.wrapping_mul(19349663)
                    ^ phase.wrapping_mul(83492791),
            );
            let base = ((n & 0xffff) as f64 / 65535.0 - 0.5) * 2.0;
            let i = (y * w + x) * 4;
            // a little of it per channel when colour grain is asked for
            let lum = f.rgba[i..i + 3].iter().map(|v| *v as f64).sum::<f64>() / 3.0 / 255.0;
            // grain is strongest in the mid-tones
            let weight = 1.0 - (lum * 2.0 - 1.0).abs() * 0.6;
            for c in 0..3 {
                let tint =
                    ((hash(n ^ (c as u64 + 1) * 0x9e37) & 0xffff) as f64 / 65535.0 - 0.5) * 2.0;
                let v = base * (1.0 - colour) + tint * colour;
                let d = v * amount * 0.35 * weight * 255.0;
                f.rgba[i + c] = (f.rgba[i + c] as f64 + d).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Red pulled toward the edges, blue pushed in: the colour fringes of a cheap lens.
fn aberration(f: &mut Frame8, amount_px: f64) {
    if amount_px < 0.1 {
        return;
    }
    let (w, h) = (f.width as usize, f.height as usize);
    let src = f.rgba.clone();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let maxr = cx.hypot(cy);
    let sample = |x: f64, y: f64, c: usize| -> u8 {
        let (x, y) = (x.clamp(0.0, w as f64 - 1.0), y.clamp(0.0, h as f64 - 1.0));
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let at = |xx: usize, yy: usize| src[(yy * w + xx) * 4 + c] as f64;
        let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
        let bot = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
        (top * (1.0 - fy) + bot * fy).round() as u8
    };
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f64 - cx, y as f64 - cy);
            let k = amount_px * (dx.hypot(dy) / maxr) / dx.hypot(dy).max(1.0);
            let i = (y * w + x) * 4;
            f.rgba[i] = sample(x as f64 - dx * k, y as f64 - dy * k, 0);
            f.rgba[i + 2] = sample(x as f64 + dx * k, y as f64 + dy * k, 2);
        }
    }
}

fn duotone(f: &mut Frame8, dark: [f64; 3], light: [f64; 3], strength: f64) {
    for p in f.rgba.chunks_exact_mut(4) {
        let l = luma8(p) as f64;
        for c in 0..3 {
            let mapped = dark[c] + (light[c] - dark[c]) * l;
            p[c] = (p[c] as f64 * (1.0 - strength) + mapped * strength)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
}

fn invert(f: &mut Frame8, amount: f64) {
    for p in f.rgba.chunks_exact_mut(4) {
        for v in p.iter_mut().take(3) {
            *v = (*v as f64 * (1.0 - amount) + (255 - *v) as f64 * amount).round() as u8;
        }
    }
}

fn posterize(f: &mut Frame8, levels: u32) {
    let n = levels.max(2) as f64 - 1.0;
    for p in f.rgba.chunks_exact_mut(4) {
        for v in p.iter_mut().take(3) {
            *v = ((*v as f64 / 255.0 * n).round() / n * 255.0).round() as u8;
        }
    }
}

/// Keep the picture only inside (or, inverted, outside) a rectangle or ellipse with a soft edge.
fn mask(f: &mut Frame8, p: &[f64]) {
    let (w, h) = (f.width as f64, f.height as f64);
    let (cx, cy) = (p[1] / 100.0 * w, p[2] / 100.0 * h);
    let (hw, hh) = (p[3] / 100.0 * w / 2.0, p[4] / 100.0 * h / 2.0);
    let (sin, cos) = p[5].to_radians().sin_cos();
    let feather = (p[6] / 100.0 * w.min(h)).max(0.5);
    let ellipse = p[0] >= 0.5;
    let invert = p[7] >= 0.5;
    for y in 0..f.height as usize {
        for x in 0..f.width as usize {
            // into the mask's own axes
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            // signed distance (in pixels) to the edge, negative inside
            let dist = if ellipse {
                let (a, b) = (hw.max(0.5), hh.max(0.5));
                let k = ((u / a).powi(2) + (v / b).powi(2)).sqrt();
                (k - 1.0) * a.min(b)
            } else {
                (u.abs() - hw).max(v.abs() - hh)
            };
            let mut inside = (0.5 - dist / feather).clamp(0.0, 1.0);
            if invert {
                inside = 1.0 - inside;
            }
            let i = (y * f.width as usize + x) * 4;
            f.rgba[i + 3] = (f.rgba[i + 3] as f64 * inside).round() as u8;
        }
    }
}

/// Keep the picture where `matte` (a grey picture of any size) is white. The matte is stretched to the
/// frame, softened by `feather` pixels and grown (`choke` > 0 shrinks the subject) by `choke` pixels.
fn subject_matte(f: &mut Frame8, matte: &Frame8, invert: bool, feather: f64, choke: f64) {
    let (w, h) = (f.width as usize, f.height as usize);
    // the matte as a frame-sized float grid in 0..1
    let mut m = vec![0.0f32; w * h];
    let (mw, mh) = (matte.width as f32, matte.height as f32);
    for y in 0..h {
        let sy = ((y as f32 + 0.5) / h as f32 * mh - 0.5).clamp(0.0, mh - 1.0);
        let (y0, fy) = (sy.floor() as usize, sy.fract());
        let y1 = (y0 + 1).min(matte.height as usize - 1);
        for x in 0..w {
            let sx = ((x as f32 + 0.5) / w as f32 * mw - 0.5).clamp(0.0, mw - 1.0);
            let (x0, fx) = (sx.floor() as usize, sx.fract());
            let x1 = (x0 + 1).min(matte.width as usize - 1);
            let at = |xx: usize, yy: usize| {
                matte.rgba[(yy * matte.width as usize + xx) * 4] as f32 / 255.0
            };
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bot = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            m[y * w + x] = top * (1.0 - fy) + bot * fy;
        }
    }
    // choke: erode (positive) or dilate (negative) with a separable min/max, then soften
    let r = choke.abs().round() as usize;
    if r > 0 {
        m = morph(&m, w, h, r, choke > 0.0);
    }
    if feather >= 0.5 {
        let mut px = vec![0.0f32; w * h * 4];
        for (i, v) in m.iter().enumerate() {
            px[i * 4] = *v;
            px[i * 4 + 3] = 1.0;
        }
        let soft = blur_premul(px, w, h, feather / 2.0);
        for (i, v) in m.iter_mut().enumerate() {
            *v = soft[i * 4];
        }
    }
    for (i, v) in m.iter().enumerate() {
        let keep = if invert { 1.0 - v } else { *v };
        f.rgba[i * 4 + 3] = (f.rgba[i * 4 + 3] as f32 * keep.clamp(0.0, 1.0)).round() as u8;
    }
}

/// Minimum (erode) or maximum (dilate) over a square window of half-width `r`.
fn morph(m: &[f32], w: usize, h: usize, r: usize, erode: bool) -> Vec<f32> {
    let pick = |a: f32, b: f32| if erode { a.min(b) } else { a.max(b) };
    let mut tmp = vec![0.0f32; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut v = m[y * w + x];
            for d in 1..=r {
                v = pick(v, m[y * w + x.saturating_sub(d)]);
                v = pick(v, m[y * w + (x + d).min(w - 1)]);
            }
            tmp[y * w + x] = v;
        }
    }
    let mut out = vec![0.0f32; m.len()];
    for y in 0..h {
        for x in 0..w {
            let mut v = tmp[y * w + x];
            for d in 1..=r {
                v = pick(v, tmp[y.saturating_sub(d) * w + x]);
                v = pick(v, tmp[(y + d).min(h - 1) * w + x]);
            }
            out[y * w + x] = v;
        }
    }
    out
}

/// Apply one 256-entry table per colour channel.
fn map_channels(f: &mut Frame8, tables: &[[u8; 256]; 3]) {
    for px in f.rgba.chunks_exact_mut(4) {
        for c in 0..3 {
            px[c] = tables[c][px[c] as usize];
        }
    }
}

/// Lift / gamma / gain, each with a master and a per-channel tint, on display values.
fn wheels(f: &mut Frame8, p: &[f64]) {
    let mut tables = [[0u8; 256]; 3];
    for (c, table) in tables.iter_mut().enumerate() {
        let lift = 0.5 * (p[0] + p[1 + c]);
        let gamma = 2f64.powf(-(p[4] + p[5 + c]));
        let gain = 1.0 + (p[8] + p[9 + c]);
        for (i, t) in table.iter_mut().enumerate() {
            let x = i as f64 / 255.0;
            let v = (x * gain + lift * (1.0 - x)).clamp(0.0, 1.0).powf(gamma);
            *t = (v * 255.0).round() as u8;
        }
    }
    map_channels(f, &tables);
}

/// Smooth, never-overshooting curve through points at x = 0, .25, .5, .75, 1 (Fritsch-Carlson).
fn curve_table(y: &[f64]) -> [f64; 256] {
    let h = 0.25;
    let d: Vec<f64> = (0..4).map(|i| (y[i + 1] - y[i]) / h).collect();
    let mut m = [0.0; 5];
    m[0] = d[0];
    m[4] = d[3];
    for i in 1..4 {
        m[i] = if d[i - 1] * d[i] <= 0.0 {
            0.0
        } else {
            (d[i - 1] + d[i]) / 2.0
        };
    }
    for i in 0..4 {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
            let n = a.hypot(b);
            if n > 3.0 {
                m[i] = 3.0 * a / n * d[i];
                m[i + 1] = 3.0 * b / n * d[i];
            }
        }
    }
    let mut out = [0.0; 256];
    for (i, o) in out.iter_mut().enumerate() {
        let x = i as f64 / 255.0;
        let k = ((x / h) as usize).min(3);
        let t = (x - k as f64 * h) / h;
        let (t2, t3) = (t * t, t * t * t);
        *o = ((2.0 * t3 - 3.0 * t2 + 1.0) * y[k]
            + (t3 - 2.0 * t2 + t) * h * m[k]
            + (-2.0 * t3 + 3.0 * t2) * y[k + 1]
            + (t3 - t2) * h * m[k + 1])
            .clamp(0.0, 1.0);
    }
    out
}

fn curves(f: &mut Frame8, p: &[f64]) {
    let master = curve_table(&p[0..5]);
    let mut tables = [[0u8; 256]; 3];
    for (c, table) in tables.iter_mut().enumerate() {
        let per = curve_table(&p[5 + 5 * c..10 + 5 * c]);
        for (i, t) in table.iter_mut().enumerate() {
            let m = master[i];
            let v = per[(m * 255.0).round() as usize];
            *t = (v * 255.0).round() as u8;
        }
    }
    map_channels(f, &tables);
}

fn rgb_to_hsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 {
        d / (2.0 - mx - mn)
    } else {
        d / (mx + mn)
    };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let pp = 2.0 * l - q;
    let f = |mut t: f64| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            pp + (q - pp) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            pp + (q - pp) * (2.0 / 3.0 - t) * 6.0
        } else {
            pp
        }
    };
    let hh = h / 360.0;
    (f(hh + 1.0 / 3.0), f(hh), f(hh - 1.0 / 3.0))
}

/// Per-band hue shift, saturation and lightness; neighbouring bands blend smoothly.
fn hsl(f: &mut Frame8, p: &[f64]) {
    for px in f.rgba.chunks_exact_mut(4) {
        let (h, s, l) = rgb_to_hsl(
            px[0] as f64 / 255.0,
            px[1] as f64 / 255.0,
            px[2] as f64 / 255.0,
        );
        if s == 0.0 {
            continue;
        }
        let (mut dh, mut ds, mut dl) = (0.0, 0.0, 0.0);
        for band in 0..6 {
            let dist = {
                let d = (h - band as f64 * 60.0).abs();
                d.min(360.0 - d)
            };
            let w = (1.0 - dist / 60.0).max(0.0);
            dh += w * p[band * 3];
            ds += w * (p[band * 3 + 1] / 100.0 - 1.0);
            dl += w * p[band * 3 + 2] / 100.0;
        }
        let (r, g, b) = hsl_to_rgb(
            h + dh,
            (s * (1.0 + ds)).clamp(0.0, 1.0),
            (l + dl).clamp(0.0, 1.0),
        );
        px[0] = (r * 255.0).round() as u8;
        px[1] = (g * 255.0).round() as u8;
        px[2] = (b * 255.0).round() as u8;
    }
}

/// A parsed 3D LUT (.cube): `size`^3 entries, red varying fastest.
pub struct Lut3d {
    size: usize,
    min: [f32; 3],
    max: [f32; 3],
    data: Vec<[f32; 3]>,
}

impl Lut3d {
    pub fn parse(text: &str) -> Result<Lut3d, String> {
        let mut size = 0usize;
        let (mut min, mut max) = ([0.0f32; 3], [1.0f32; 3]);
        let mut data = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut w = line.split_whitespace();
            let first = w.next().unwrap_or("");
            let nums = |w: std::str::SplitWhitespace| -> Vec<f32> {
                w.filter_map(|x| x.parse().ok()).collect()
            };
            match first {
                "TITLE" | "LUT_1D_SIZE" => {
                    if first == "LUT_1D_SIZE" {
                        return Err("1D LUTs are not supported".into());
                    }
                }
                "LUT_3D_SIZE" => {
                    size = w
                        .next()
                        .and_then(|x| x.parse().ok())
                        .ok_or("bad LUT_3D_SIZE")?
                }
                "DOMAIN_MIN" => {
                    let v = nums(w);
                    if v.len() == 3 {
                        min = [v[0], v[1], v[2]];
                    }
                }
                "DOMAIN_MAX" => {
                    let v = nums(w);
                    if v.len() == 3 {
                        max = [v[0], v[1], v[2]];
                    }
                }
                _ => {
                    let v: Vec<f32> = std::iter::once(first)
                        .chain(w)
                        .filter_map(|x| x.parse().ok())
                        .collect();
                    if v.len() == 3 {
                        data.push([v[0], v[1], v[2]]);
                    }
                }
            }
        }
        if !(2..=129).contains(&size) || data.len() != size * size * size {
            return Err(format!("expected {size}^3 entries, found {}", data.len()));
        }
        Ok(Lut3d {
            size,
            min,
            max,
            data,
        })
    }

    fn at(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.data[r + self.size * (g + self.size * b)]
    }

    /// Trilinear lookup of a colour in 0..1.
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = (self.size - 1) as f32;
        let mut idx = [0usize; 3];
        let mut fr = [0f32; 3];
        for c in 0..3 {
            let t = ((rgb[c] - self.min[c]) / (self.max[c] - self.min[c])).clamp(0.0, 1.0) * n;
            idx[c] = (t.floor() as usize).min(self.size - 2);
            fr[c] = t - idx[c] as f32;
        }
        let mut out = [0f32; 3];
        for corner in 0..8 {
            let (dr, dg, db) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
            let w = (if dr == 1 { fr[0] } else { 1.0 - fr[0] })
                * (if dg == 1 { fr[1] } else { 1.0 - fr[1] })
                * (if db == 1 { fr[2] } else { 1.0 - fr[2] });
            let v = self.at(idx[0] + dr, idx[1] + dg, idx[2] + db);
            for c in 0..3 {
                out[c] += w * v[c];
            }
        }
        out
    }
}

type LutCache = std::sync::Mutex<std::collections::HashMap<String, Option<std::sync::Arc<Lut3d>>>>;

fn load_lut(path: &str) -> Option<std::sync::Arc<Lut3d>> {
    static CACHE: std::sync::OnceLock<LutCache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let stamp = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let key = format!("{path}@{stamp}");
    let mut map = cache.lock().ok()?;
    map.entry(key)
        .or_insert_with(|| {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|t| Lut3d::parse(&t).ok())
                .map(std::sync::Arc::new)
        })
        .clone()
}

fn lut(f: &mut Frame8, path: &str, strength: f64) {
    let Some(l) = load_lut(path) else { return };
    let s = strength as f32;
    for px in f.rgba.chunks_exact_mut(4) {
        let src = [
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        ];
        let out = l.sample(src);
        for c in 0..3 {
            px[c] = ((src[c] + (out[c] - src[c]) * s) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod matte_tests {
    use std::sync::Arc;

    use super::*;

    fn white_left_half(w: u32, h: u32) -> Frame8 {
        let mut m = Frame8::solid(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                m.rgba[i..i + 3].fill(255);
            }
        }
        m
    }

    fn alpha(f: &Frame8, x: u32, y: u32) -> u8 {
        f.rgba[((y * f.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn the_matte_keeps_white_and_hides_black_whatever_its_size() {
        let pic = Frame8::solid(64, 32, [200, 100, 50, 255]);
        let mut e = Effect::new(EffectKind::Matte);
        e.params = vec![0.0, 0.0, 0.0, 0.0];
        // a matte a quarter of the size is stretched over the picture
        let out = apply_effects_with(
            &pic,
            std::slice::from_ref(&e),
            &[Some(Arc::new(white_left_half(16, 8)))],
        )
        .unwrap();
        assert_eq!((alpha(&out, 5, 16), alpha(&out, 60, 16)), (255, 0));
        // inverted
        e.params[0] = 1.0;
        let inv = apply_effects_with(
            &pic,
            std::slice::from_ref(&e),
            &[Some(Arc::new(white_left_half(16, 8)))],
        )
        .unwrap();
        assert_eq!((alpha(&inv, 5, 16), alpha(&inv, 60, 16)), (0, 255));
        // no matte available: the picture passes through
        e.params[0] = 0.0;
        let none = apply_effects_with(&pic, std::slice::from_ref(&e), &[None]).unwrap();
        assert_eq!(none, pic);
    }

    #[test]
    fn feather_softens_and_choke_shrinks_the_edge() {
        // sizes are relative to a 1080-pixel-tall frame
        let pic = Frame8::solid(64, 1080, [200, 100, 50, 255]);
        let matte = Arc::new(white_left_half(64, 1080));
        let mut e = Effect::new(EffectKind::Matte);
        e.params = vec![0.0, 0.0, 0.0, 0.0];
        let hard =
            apply_effects_with(&pic, std::slice::from_ref(&e), &[Some(matte.clone())]).unwrap();
        assert_eq!((alpha(&hard, 30, 540), alpha(&hard, 33, 540)), (255, 0));
        e.params = vec![0.0, 30.0, 0.0, 0.0];
        let soft =
            apply_effects_with(&pic, std::slice::from_ref(&e), &[Some(matte.clone())]).unwrap();
        let edge = alpha(&soft, 31, 540);
        assert!(edge > 30 && edge < 225, "{edge}");
        e.params = vec![0.0, 0.0, 10.0, 0.0];
        let choked = apply_effects_with(&pic, std::slice::from_ref(&e), &[Some(matte)]).unwrap();
        assert!(
            alpha(&choked, 28, 540) < alpha(&hard, 28, 540),
            "the subject lost its outer pixels"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(kind: EffectKind, params: &[f64]) -> Vec<Effect> {
        let mut e = Effect::new(kind);
        e.params = params.to_vec();
        assert_eq!(e.check(), Ok(()));
        vec![e]
    }

    /// Left half red, right half blue.
    fn halves(w: u32, h: u32) -> Frame8 {
        let mut f = Frame8::solid(w, h, [0, 0, 0, 255]);
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                f.rgba[i..i + 3].copy_from_slice(if x < w / 2 {
                    &[255, 0, 0]
                } else {
                    &[0, 0, 255]
                });
            }
        }
        f
    }

    fn px(f: &Frame8, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * f.width + x) * 4) as usize;
        [f.rgba[i], f.rgba[i + 1], f.rgba[i + 2], f.rgba[i + 3]]
    }

    #[test]
    fn nothing_enabled_means_no_work() {
        let f = halves(8, 8);
        assert!(apply_effects(&f, &[]).is_none());
        let mut e = Effect::new(EffectKind::Blur);
        e.enabled = false;
        assert!(apply_effects(&f, &[e]).is_none());
    }

    #[test]
    fn chroma_key_removes_the_key_colour_and_keeps_the_rest() {
        let mut f = Frame8::solid(4, 2, [0, 255, 0, 255]);
        f.rgba[16..20].copy_from_slice(&[200, 40, 40, 255]); // one red pixel on a green screen
        let out = apply_effects(
            &f,
            &fx(
                EffectKind::ChromaKey,
                &[0.0, 255.0, 0.0, 35.0, 15.0, 50.0, 0.0, 0.0],
            ),
        )
        .unwrap();
        assert_eq!(px(&out, 0, 0)[3], 0, "green is keyed out");
        assert_eq!(px(&out, 3, 0)[3], 0);
        assert_eq!(px(&out, 0, 1), [200, 40, 40, 255], "the subject stays");
        // a slightly different green is still removed with enough tolerance, not with none
        let g2 = Frame8::solid(2, 2, [30, 220, 40, 255]);
        let wide = apply_effects(
            &g2,
            &fx(
                EffectKind::ChromaKey,
                &[0.0, 255.0, 0.0, 35.0, 15.0, 0.0, 0.0, 0.0],
            ),
        )
        .unwrap();
        let none = apply_effects(
            &g2,
            &fx(
                EffectKind::ChromaKey,
                &[0.0, 255.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ),
        )
        .unwrap();
        assert_eq!(px(&wide, 0, 0)[3], 0);
        assert_eq!(px(&none, 0, 0)[3], 255);
    }

    #[test]
    fn spill_removal_pulls_green_out_of_the_edge_pixels() {
        let f = Frame8::solid(2, 2, [180, 230, 120, 255]); // a green-tinted skin edge
        let on = apply_effects(
            &f,
            &fx(
                EffectKind::ChromaKey,
                &[0.0, 255.0, 0.0, 5.0, 60.0, 100.0, 0.0, 0.0],
            ),
        )
        .unwrap();
        let p = px(&on, 0, 0);
        assert!(p[1] <= 150, "green was pulled toward red/blue: {p:?}");
    }

    #[test]
    fn blur_softens_a_hard_edge_and_keeps_flat_areas() {
        let f = halves(64, 1080); // sizes are relative to a 1080-pixel-tall frame
        let out = apply_effects(&f, &fx(EffectKind::Blur, &[8.0])).unwrap();
        let edge = px(&out, 32, 540);
        assert!(
            edge[0] > 60 && edge[2] > 60,
            "red and blue mix at the edge: {edge:?}"
        );
        assert_eq!(
            px(&out, 2, 540)[2],
            0,
            "far from the edge it is untouched in blue"
        );
        let flat = Frame8::solid(32, 1080, [90, 120, 150, 255]);
        let b = apply_effects(&flat, &fx(EffectKind::Blur, &[20.0])).unwrap();
        assert_eq!(px(&b, 16, 540), [90, 120, 150, 255]);
    }

    #[test]
    fn sharpen_adds_contrast_at_edges() {
        let mut f = Frame8::solid(32, 1080, [100, 100, 100, 255]);
        for y in 0..1080 {
            for x in 16..32 {
                let i = (y * 32 + x) * 4;
                f.rgba[i..i + 3].copy_from_slice(&[160, 160, 160]);
            }
        }
        let out = apply_effects(&f, &fx(EffectKind::Sharpen, &[200.0])).unwrap();
        assert!(px(&out, 15, 540)[0] < 100, "dark side gets darker");
        assert!(px(&out, 16, 540)[0] > 160, "bright side gets brighter");
    }

    #[test]
    fn vignette_darkens_corners_not_the_middle() {
        let f = Frame8::solid(40, 40, [200, 200, 200, 255]);
        let out = apply_effects(&f, &fx(EffectKind::Vignette, &[80.0, 60.0])).unwrap();
        assert_eq!(px(&out, 20, 20)[0], 200);
        assert!(px(&out, 0, 0)[0] < 100);
    }

    #[test]
    fn mirror_and_crop_and_pixelate() {
        let f = halves(8, 4);
        let m = apply_effects(&f, &fx(EffectKind::Mirror, &[0.0])).unwrap();
        assert_eq!(
            px(&m, 7, 0)[0],
            255,
            "the left half is mirrored onto the right"
        );
        let c = apply_effects(&f, &fx(EffectKind::Crop, &[25.0, 0.0, 25.0, 50.0])).unwrap();
        assert_eq!(px(&c, 0, 0)[3], 0);
        assert_eq!(px(&c, 3, 0)[3], 255);
        assert_eq!(px(&c, 3, 3)[3], 0, "the bottom half is cropped");
        let mut g = Frame8::solid(4, 1080, [0, 0, 0, 255]);
        g.rgba[0..3].copy_from_slice(&[200, 200, 200]);
        let pz = apply_effects(&g, &fx(EffectKind::Pixelate, &[2.0])).unwrap();
        assert_eq!(px(&pz, 0, 0), px(&pz, 1, 1), "one block, one colour");
        assert_eq!(px(&pz, 0, 0)[0], 50);
    }

    #[test]
    fn color_wheels_lift_gamma_and_gain() {
        let f = Frame8::solid(2, 2, [128, 128, 128, 255]);
        let mut p = vec![0.0; 12];
        p[8] = 0.5; // gain master up
        let up = apply_effects(&f, &fx(EffectKind::ColorWheels, &p)).unwrap();
        assert!(px(&up, 0, 0)[0] > 180);
        let mut q = vec![0.0; 12];
        q[9] = 0.5; // gain red up
        let warm = apply_effects(&f, &fx(EffectKind::ColorWheels, &q)).unwrap();
        let w = px(&warm, 0, 0);
        assert!(w[0] > w[1] && w[1] == w[2]);
        let mut g = vec![0.0; 12];
        g[4] = 0.5; // gamma master up: midtones brighter, black stays black
        let black = Frame8::solid(2, 2, [0, 0, 0, 255]);
        assert_eq!(
            px(
                &apply_effects(&black, &fx(EffectKind::ColorWheels, &g)).unwrap(),
                0,
                0
            )[0],
            0
        );
        assert!(
            px(
                &apply_effects(&f, &fx(EffectKind::ColorWheels, &g)).unwrap(),
                0,
                0
            )[0] > 150
        );
        // the defaults change nothing
        let same = apply_effects(&f, &fx(EffectKind::ColorWheels, &[0.0; 12])).unwrap();
        assert_eq!(same.rgba, f.rgba);
    }

    #[test]
    fn curves_default_is_identity_and_a_lifted_midpoint_brightens() {
        let mut ramp = Frame8::solid(256, 1, [0, 0, 0, 255]);
        for x in 0..256 {
            ramp.rgba[x * 4..x * 4 + 3].fill(x as u8);
        }
        let d = Effect::new(EffectKind::Curves);
        let ident = apply_effects(&ramp, std::slice::from_ref(&d)).unwrap();
        for x in 0..256 {
            assert!((ident.rgba[x * 4] as i32 - x as i32).abs() <= 1, "x={x}");
        }
        let mut lifted = d;
        lifted.params[2] = 0.7; // master midpoint up
        let out = apply_effects(&ramp, &[lifted]).unwrap();
        assert!(out.rgba[128 * 4] > 160);
        assert_eq!(out.rgba[0], 0);
        assert_eq!(out.rgba[255 * 4], 255);
        // never overshoots even with a harsh curve
        let mut harsh = Effect::new(EffectKind::Curves);
        harsh.params[1] = 0.0;
        harsh.params[3] = 1.0;
        let o = apply_effects(&ramp, &[harsh]).unwrap();
        let vals: Vec<u8> = o.rgba.chunks(4).map(|p| p[0]).collect();
        assert!(
            vals.windows(2).all(|w| w[0] <= w[1]),
            "monotone inputs stay monotone"
        );
    }

    #[test]
    fn hsl_shifts_only_the_chosen_band() {
        let mut f = Frame8::solid(2, 1, [255, 0, 0, 255]);
        f.rgba[4..8].copy_from_slice(&[0, 0, 255, 255]);
        let mut p = Effect::new(EffectKind::Hsl);
        p.params[1] = 0.0; // red saturation to zero
        let out = apply_effects(&f, &[p]).unwrap();
        let (r, b) = (px(&out, 0, 0), px(&out, 1, 0));
        assert!(r[0].abs_diff(r[1]) < 3, "red turned grey: {r:?}");
        assert_eq!(b, [0, 0, 255, 255], "blue untouched");
        let mut shift = Effect::new(EffectKind::Hsl);
        shift.params[0] = 60.0; // red hue +60 degrees -> yellow
        let y = px(&apply_effects(&f, &[shift]).unwrap(), 0, 0);
        assert!(y[0] > 200 && y[1] > 200 && y[2] < 30, "{y:?}");
    }

    #[test]
    fn a_cube_lut_is_parsed_and_applied() {
        // 2-point LUT that swaps red and blue
        let mut text = String::from("TITLE \"swap\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    text += &format!("{b} {g} {r}\n");
                }
            }
        }
        let l = Lut3d::parse(&text).unwrap();
        let o = l.sample([1.0, 0.0, 0.0]);
        assert_eq!(o, [0.0, 0.0, 1.0]);
        assert!(Lut3d::parse("LUT_3D_SIZE 2\n0 0 0").is_err());
        assert!(Lut3d::parse("LUT_1D_SIZE 4").is_err());
        let path = std::env::temp_dir().join(format!("captaine-lut-{}.cube", std::process::id()));
        std::fs::write(&path, &text).unwrap();
        let f = Frame8::solid(2, 2, [255, 0, 0, 255]);
        let mut e = Effect::new(EffectKind::Lut);
        e.text = path.to_string_lossy().into();
        let out = apply_effects(&f, &[e.clone()]).unwrap();
        assert_eq!(px(&out, 0, 0), [0, 0, 255, 255]);
        e.params[0] = 50.0;
        let half = px(&apply_effects(&f, &[e]).unwrap(), 0, 0);
        assert!(half[0].abs_diff(128) <= 1 && half[2].abs_diff(128) <= 1);
        // a missing file leaves the picture alone
        let mut gone = Effect::new(EffectKind::Lut);
        gone.text = "/no/such.cube".into();
        assert_eq!(apply_effects(&f, &[gone]).unwrap().rgba, f.rgba);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn masks_keep_the_inside_and_feather_the_edge() {
        let f = Frame8::solid(100, 100, [200, 100, 50, 255]);
        let mut m = Effect::new(EffectKind::Mask);
        m.params = vec![0.0, 50.0, 50.0, 40.0, 40.0, 0.0, 0.0, 0.0]; // hard 40% rectangle
        let r = apply_effects(&f, &[m.clone()]).unwrap();
        assert_eq!(px(&r, 50, 50)[3], 255);
        assert_eq!(px(&r, 5, 5)[3], 0);
        assert_eq!(px(&r, 35, 50)[3], 255, "just inside the left edge");
        assert_eq!(px(&r, 28, 50)[3], 0, "just outside");
        // inverted: the opposite
        m.params[7] = 1.0;
        let inv = apply_effects(&f, &[m.clone()]).unwrap();
        assert_eq!((px(&inv, 50, 50)[3], px(&inv, 5, 5)[3]), (0, 255));
        // an ellipse leaves the corners of its box empty
        let mut e = Effect::new(EffectKind::Mask);
        e.params = vec![1.0, 50.0, 50.0, 80.0, 80.0, 0.0, 0.0, 0.0];
        let el = apply_effects(&f, &[e.clone()]).unwrap();
        assert_eq!(px(&el, 50, 50)[3], 255);
        assert_eq!(px(&el, 14, 14)[3], 0, "corner of the box");
        // feathering gives partial alpha at the edge
        e.params[6] = 20.0;
        let soft = apply_effects(&f, &[e]).unwrap();
        let edge = px(&soft, 10, 50)[3];
        assert!(edge > 20 && edge < 235, "{edge}");
    }

    fn alpha(f: &Frame8, x: u32, y: u32) -> u8 {
        f.rgba[((y * f.width + x) * 4 + 3) as usize]
    }

    #[test]
    fn glow_spreads_light_from_bright_areas_only() {
        // a bright block in a dark frame, tall enough that pixel sizes mean something
        let mut f = Frame8::solid(120, 1080, [10, 10, 10, 255]);
        for y in 520..560 {
            for x in 50..70 {
                let i = (y * 120 + x) * 4;
                f.rgba[i..i + 3].fill(250);
            }
        }
        let out = apply_effects(&f, &fx(EffectKind::Glow, &[60.0, 30.0, 150.0])).unwrap();
        assert!(
            px(&out, 90, 540)[0] > 25,
            "light has spread to the side: {:?}",
            px(&out, 90, 540)
        );
        assert!(px(&out, 90, 540)[0] < 250);
        assert!(px(&out, 5, 100)[0] <= 12, "far away stays dark");
        let dark = Frame8::solid(8, 1080, [20, 20, 20, 255]);
        let same = apply_effects(&dark, &fx(EffectKind::Glow, &[60.0, 30.0, 150.0])).unwrap();
        assert_eq!(
            px(&same, 4, 500),
            [20, 20, 20, 255],
            "nothing bright, nothing glows"
        );
    }

    #[test]
    fn grain_changes_every_frame_but_repeats_for_the_same_frame() {
        let f = Frame8::solid(64, 64, [128, 128, 128, 255]);
        let e = fx(EffectKind::Grain, &[50.0, 1.5, 20.0]);
        let (a, b) = (
            apply_effects_at(&f, &e, &[], 1).unwrap(),
            apply_effects_at(&f, &e, &[], 2).unwrap(),
        );
        assert_ne!(a, b, "a new frame, new grain");
        assert_eq!(
            a,
            apply_effects_at(&f, &e, &[], 1).unwrap(),
            "the same frame twice is identical"
        );
        let spread = |x: &Frame8| {
            x.rgba
                .chunks(4)
                .map(|p| (p[0] as i32 - 128).abs())
                .max()
                .unwrap()
        };
        assert!(spread(&a) > 8 && spread(&a) < 90, "{}", spread(&a));
        let none = apply_effects(&f, &fx(EffectKind::Grain, &[0.0, 1.5, 20.0])).unwrap();
        assert_eq!(none, f);
    }

    #[test]
    fn aberration_fringes_the_edges_and_leaves_the_middle() {
        let mut f = Frame8::solid(200, 1080, [0, 0, 0, 255]);
        // a vertical white bar near the right edge
        for y in 0..1080 {
            for x in 170..176 {
                f.rgba[(y * 200 + x) * 4..(y * 200 + x) * 4 + 3].fill(255);
            }
        }
        let out = apply_effects(&f, &fx(EffectKind::Aberration, &[30.0])).unwrap();
        let edge = |x| px(&out, x, 540);
        assert!(
            edge(167)[2] > 100 && edge(167)[0] < 100,
            "blue leans inward at the right edge: {:?}",
            edge(167)
        );
        assert!(
            edge(178)[0] > 100 && edge(178)[2] < 100,
            "red leans outward: {:?}",
            edge(178)
        );
        let mid = Frame8::solid(200, 1080, [90, 90, 90, 255]);
        assert_eq!(
            apply_effects(&mid, &fx(EffectKind::Aberration, &[20.0])).unwrap(),
            mid,
            "a flat picture is untouched"
        );
    }

    #[test]
    fn duotone_invert_and_posterize() {
        let mut f = Frame8::solid(2, 1, [0, 0, 0, 255]);
        f.rgba[4..7].fill(255);
        let d = apply_effects(
            &f,
            &fx(
                EffectKind::Duotone,
                &[0.0, 0.0, 100.0, 255.0, 200.0, 0.0, 100.0],
            ),
        )
        .unwrap();
        assert_eq!(
            (px(&d, 0, 0), px(&d, 1, 0)),
            ([0, 0, 100, 255], [255, 200, 0, 255])
        );
        let i = apply_effects(&f, &fx(EffectKind::Invert, &[100.0])).unwrap();
        assert_eq!((px(&i, 0, 0)[0], px(&i, 1, 0)[0]), (255, 0));
        let half = apply_effects(&f, &fx(EffectKind::Invert, &[50.0])).unwrap();
        assert!(px(&half, 0, 0)[0].abs_diff(128) <= 1);
        let ramp = {
            let mut r = Frame8::solid(256, 1, [0, 0, 0, 255]);
            for x in 0..256 {
                r.rgba[x * 4..x * 4 + 3].fill(x as u8);
            }
            r
        };
        let p = apply_effects(&ramp, &fx(EffectKind::Posterize, &[4.0])).unwrap();
        let mut levels: Vec<u8> = p.rgba.chunks(4).map(|v| v[0]).collect();
        levels.dedup();
        assert_eq!(levels, [0, 85, 170, 255]);
    }

    #[test]
    fn chroma_key_shrinks_its_edge_and_can_show_the_matte() {
        // a green screen with a red subject block
        let mut f = Frame8::solid(60, 1080, [0, 255, 0, 255]);
        for y in 0..1080 {
            for x in 20..40 {
                f.rgba[(y * 60 + x) * 4..(y * 60 + x) * 4 + 3].copy_from_slice(&[200, 40, 40]);
            }
        }
        let key = |shrink: f64, view: f64| {
            apply_effects(
                &f,
                &fx(
                    EffectKind::ChromaKey,
                    &[0.0, 255.0, 0.0, 35.0, 0.0, 0.0, shrink, view],
                ),
            )
            .unwrap()
        };
        let plain = key(0.0, 0.0);
        assert_eq!(
            (
                alpha(&plain, 30, 540),
                alpha(&plain, 5, 540),
                alpha(&plain, 21, 540)
            ),
            (255, 0, 255)
        );
        let choked = key(4.0, 0.0);
        assert_eq!(
            alpha(&choked, 21, 540),
            0,
            "the edge pixels went with the shrink"
        );
        assert_eq!(alpha(&choked, 30, 540), 255);
        let grown = key(-4.0, 0.0);
        assert!(
            alpha(&grown, 17, 540) > 0,
            "a negative shrink fattens the subject"
        );
        // matte view: opaque grey, white where the subject is
        let view = key(0.0, 1.0);
        assert_eq!(
            (px(&view, 30, 540), px(&view, 5, 540)),
            ([255, 255, 255, 255], [0, 0, 0, 255])
        );
    }

    #[test]
    fn effects_saved_before_they_gained_settings_still_open() {
        // a chroma key written when it had six numbers
        let old = r#"{"kind":"ChromaKey","enabled":true,"params":[0.0,255.0,0.0,35.0,15.0,50.0]}"#;
        let e: Effect = serde_json::from_str(old).unwrap();
        assert_eq!(e.params.len(), EffectKind::ChromaKey.params().len());
        assert_eq!(&e.params[6..], &[0.0, 0.0]);
        assert_eq!(e.check(), Ok(()));
    }

    #[test]
    fn bad_parameters_are_rejected() {
        let mut e = Effect::new(EffectKind::Crop);
        e.params = vec![60.0, 0.0, 60.0, 0.0];
        assert!(e.check().is_err(), "nothing left");
        e.params = vec![0.0; 3];
        assert!(e.check().is_err(), "wrong count");
        let mut b = Effect::new(EffectKind::Blur);
        b.params = vec![f64::NAN];
        assert!(b.check().is_err());
        b.params = vec![500.0];
        assert!(b.check().is_err());
    }
}
