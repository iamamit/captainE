//! Picture effects on a decoded frame (straight-alpha RGBA8), run before the layer is transformed.

use captaine_model::{Effect, EffectKind};

use crate::Frame8;

/// The frame with every enabled effect applied in order, or `None` when there is nothing to do.
pub fn apply_effects(frame: &Frame8, effects: &[Effect]) -> Option<Frame8> {
    let mut active = effects.iter().filter(|e| e.enabled).peekable();
    active.peek()?;
    let mut out = frame.clone();
    let k = (frame.height as f64 / 1080.0).max(0.01);
    for e in active {
        let p = &e.params;
        match e.kind {
            EffectKind::Blur => blur(&mut out, p[0] * k),
            EffectKind::Sharpen => sharpen(&mut out, p[0] / 100.0, 1.5 * k),
            EffectKind::Vignette => vignette(&mut out, p[0] / 100.0, p[1] / 100.0),
            EffectKind::Mirror => mirror(&mut out, p[0] >= 0.5),
            EffectKind::Crop => crop(&mut out, [p[0], p[1], p[2], p[3]].map(|v| v / 100.0)),
            EffectKind::ChromaKey => chroma_key(
                &mut out,
                [p[0], p[1], p[2]],
                p[3] / 100.0,
                p[4] / 100.0,
                p[5] / 100.0,
            ),
            EffectKind::Pixelate => pixelate(&mut out, (p[0] * k).round().max(1.0) as usize),
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
            &fx(EffectKind::ChromaKey, &[0.0, 255.0, 0.0, 35.0, 15.0, 50.0]),
        )
        .unwrap();
        assert_eq!(px(&out, 0, 0)[3], 0, "green is keyed out");
        assert_eq!(px(&out, 3, 0)[3], 0);
        assert_eq!(px(&out, 0, 1), [200, 40, 40, 255], "the subject stays");
        // a slightly different green is still removed with enough tolerance, not with none
        let g2 = Frame8::solid(2, 2, [30, 220, 40, 255]);
        let wide = apply_effects(
            &g2,
            &fx(EffectKind::ChromaKey, &[0.0, 255.0, 0.0, 35.0, 15.0, 0.0]),
        )
        .unwrap();
        let none = apply_effects(
            &g2,
            &fx(EffectKind::ChromaKey, &[0.0, 255.0, 0.0, 0.0, 0.0, 0.0]),
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
            &fx(EffectKind::ChromaKey, &[0.0, 255.0, 0.0, 5.0, 60.0, 100.0]),
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
