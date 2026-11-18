//! The effects library beyond the basics: directional blurs, distortions, generators, light, texture, stylize,
//! time and transform effects. Written for captainE from textbook image-processing methods; where a method has a
//! name or a paper it is cited next to the code.

use captaine_model::EffectKind;
use rayon::prelude::*;

use crate::effects::{blur, blur_plane, hash, luma8, map_channels, rgb_to_hsl};
use crate::Frame8;

/// When an effect runs: the frame number (randomness that changes every frame) and seconds since the clip
/// began (motion such as waves and shake).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FxTime {
    pub frame: u64,
    pub secs: f64,
}

impl FxTime {
    pub fn new(frame: u64, secs: f64) -> FxTime {
        FxTime { frame, secs }
    }
}

/// Runs `kind` if it is one of the library effects. `k` turns sizes given for a 1080-pixel-tall frame into this
/// frame's pixels. Returns false for kinds handled elsewhere.
pub(crate) fn apply(kind: EffectKind, f: &mut Frame8, p: &[f64], k: f64, at: FxTime) -> bool {
    match kind {
        EffectKind::DirectionalBlur => directional_blur(f, p[0], p[1] * k),
        EffectKind::RadialBlur => spin_blur(f, p[0], (p[1], p[2])),
        EffectKind::ZoomBlur => zoom_blur(f, p[0] / 100.0, (p[1], p[2])),
        EffectKind::TiltShift => tilt_shift(f, p[0] / 100.0, p[1] / 100.0, p[2] / 100.0, p[3] * k),
        EffectKind::Denoise => denoise(f, p[0] / 100.0, (p[1] * k).round().max(1.0) as usize),
        EffectKind::BrightnessContrast => brightness_contrast(f, p[0] / 100.0, p[1] / 100.0),
        EffectKind::Levels => levels(f, p),
        EffectKind::BlackWhite => black_white(f, p[0] / 100.0, [p[1], p[2], p[3]]),
        EffectKind::ColorIsolate => color_isolate(f, p[0], p[1], p[2], p[3] >= 0.5),
        EffectKind::ChannelMixer => channel_mixer(f, p),
        EffectKind::Threshold => threshold(f, p[0]),
        EffectKind::LensDistortion => lens_distortion(f, p[0] / 100.0, p[1] / 100.0),
        EffectKind::Bulge => bulge(f, p[0] / 100.0, p[1] / 100.0, (p[2], p[3])),
        EffectKind::Twirl => twirl(f, p[0], p[1] / 100.0, (p[2], p[3])),
        EffectKind::Wave => wave(f, p[0] * k, p[1] * k, p[2] >= 0.5, p[3], at.secs),
        EffectKind::Turbulence => turbulence(f, p[0] * k, p[1] * k, p[2], at.secs),
        EffectKind::CornerPin => corner_pin(f, p),
        EffectKind::Offset => offset(f, p[0] / 100.0, p[1] / 100.0),
        EffectKind::Tile => tile(f, p[0].round().max(1.0) as usize),
        EffectKind::Gradient => gradient(f, p),
        EffectKind::Grid => grid(f, p[0] * k, p[1] * k, p[2] / 100.0, [p[3], p[4], p[5]]),
        EffectKind::LensFlare => lens_flare(f, (p[0], p[1]), p[2] / 100.0, p[3] / 100.0),
        EffectKind::LightRays => {
            light_rays(f, (p[0], p[1]), p[2] / 100.0, p[3] / 100.0, p[4] / 100.0)
        }
        EffectKind::LightLeak => light_leak(f, p[0] / 100.0, p[1], p[2], p[3] as u64, at.secs),
        EffectKind::LumaKey => luma_key(f, p[0] / 100.0, p[1] / 100.0, p[2] >= 0.5),
        EffectKind::FilmDamage => film_damage(f, p, k, at),
        EffectKind::Analog => analog(f, p[0] / 100.0, p[1] / 100.0, p[2] / 100.0, k, at),
        EffectKind::Perspective => perspective(f, p[0], p[1], p[2] / 100.0),
        EffectKind::FindEdges => find_edges(f, p[0] / 100.0, p[1] >= 0.5),
        EffectKind::Emboss => emboss(f, p[0], p[1] * k, p[2] / 100.0, p[3] / 100.0),
        EffectKind::Halftone => halftone(f, p[0] * k, p[1], p[2] >= 0.5),
        EffectKind::Scanlines => scanlines(f, p[0] * k, p[1] / 100.0, p[2] * k, at.secs),
        EffectKind::Cartoon => cartoon(f, p[0].round() as u32, p[1] / 100.0, p[2] * k),
        EffectKind::Sketch => sketch(f, p[0] / 100.0, p[1] / 100.0, k),
        EffectKind::Paint => kuwahara(f, (p[0] * k).round().max(1.0) as usize),
        EffectKind::Glitch => glitch(f, p[0] / 100.0, p[1] * k, p[2], p[3] as u64, at.secs),
        EffectKind::Strobe => strobe(f, p[0], p[1] / 100.0, p[2] / 100.0, at.secs),
        EffectKind::CameraShake => camera_shake(f, p[0] * k, p[1], p[2], p[3] / 100.0, at.secs),
        // these act when the layer is placed (drop shadow) or on which source frame is shown (low frame rate)
        EffectKind::DropShadow | EffectKind::LowFrameRate => {}
        _ => return false,
    }
    true
}

// ---- sampling ---------------------------------------------------------------------------------------------------

/// What a sample outside the picture sees.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Transparent,
    Clamp,
    Wrap,
}

/// Bilinear sample at `(x, y)` in pixel units (pixel centres at +0.5): premultiplied RGBA in 0..1.
fn sample(f: &Frame8, x: f64, y: f64, edge: Edge) -> [f32; 4] {
    let (w, h) = (f.width as i64, f.height as i64);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let px = |xi: i64, yi: i64| -> [f32; 4] {
        let (xi, yi) = match edge {
            Edge::Transparent => {
                if xi < 0 || yi < 0 || xi >= w || yi >= h {
                    return [0.0; 4];
                }
                (xi, yi)
            }
            Edge::Clamp => (xi.clamp(0, w - 1), yi.clamp(0, h - 1)),
            Edge::Wrap => (xi.rem_euclid(w), yi.rem_euclid(h)),
        };
        let i = ((yi * w + xi) * 4) as usize;
        let a = f.rgba[i + 3] as f32 / 255.0;
        [
            f.rgba[i] as f32 / 255.0 * a,
            f.rgba[i + 1] as f32 / 255.0 * a,
            f.rgba[i + 2] as f32 / 255.0 * a,
            a,
        ]
    };
    let (a, b, c, d) = (
        px(x0, y0),
        px(x0 + 1, y0),
        px(x0, y0 + 1),
        px(x0 + 1, y0 + 1),
    );
    let mut o = [0.0f32; 4];
    for i in 0..4 {
        let top = a[i] + (b[i] - a[i]) * tx;
        let bottom = c[i] + (d[i] - c[i]) * tx;
        o[i] = top + (bottom - top) * ty;
    }
    o
}

/// Catmull-Rom weights for the four taps around a sample `t` (0..1) past the second one.
fn cubic_weights(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        0.5 * (-t3 + 2.0 * t2 - t),
        0.5 * (3.0 * t3 - 5.0 * t2 + 2.0),
        0.5 * (-3.0 * t3 + 4.0 * t2 + t),
        0.5 * (t3 - t2),
    ]
}

/// Bicubic (Catmull-Rom) sample: like `sample`, but a 4x4 neighbourhood, so a warp that moves the picture by
/// fractions of a pixel keeps it sharp instead of averaging neighbours. Ringing is clamped away.
fn sample_cubic(f: &Frame8, x: f64, y: f64, edge: Edge) -> [f32; 4] {
    let (w, h) = (f.width as i64, f.height as i64);
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (wx, wy) = (
        cubic_weights((fx - x0) as f32),
        cubic_weights((fy - y0) as f32),
    );
    let (x0, y0) = (x0 as i64, y0 as i64);
    let mut o = [0.0f32; 4];
    for (j, wyj) in wy.iter().enumerate() {
        let yi = y0 - 1 + j as i64;
        for (i, wxi) in wx.iter().enumerate() {
            let xi = x0 - 1 + i as i64;
            let (xi, yi) = match edge {
                Edge::Transparent => {
                    if xi < 0 || yi < 0 || xi >= w || yi >= h {
                        continue;
                    }
                    (xi, yi)
                }
                Edge::Clamp => (xi.clamp(0, w - 1), yi.clamp(0, h - 1)),
                Edge::Wrap => (xi.rem_euclid(w), yi.rem_euclid(h)),
            };
            let k = wxi * wyj;
            let p = ((yi * w + xi) * 4) as usize;
            let a = f.rgba[p + 3] as f32 / 255.0;
            for (c, v) in o.iter_mut().take(3).enumerate() {
                *v += f.rgba[p + c] as f32 / 255.0 * a * k;
            }
            o[3] += a * k;
        }
    }
    let a = o[3].clamp(0.0, 1.0);
    [
        o[0].clamp(0.0, a),
        o[1].clamp(0.0, a),
        o[2].clamp(0.0, a),
        a,
    ]
}

/// Write a premultiplied 0..1 colour as straight-alpha RGBA8.
fn put(px: &mut [u8], v: [f32; 4]) {
    let a = v[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        px.copy_from_slice(&[0, 0, 0, 0]);
        return;
    }
    for i in 0..3 {
        px[i] = (v[i] / a * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    px[3] = (a * 255.0).round() as u8;
}

/// Each output pixel shows the picture at `map(x, y)` (pixel units), or nothing for `None`.
fn warp(f: &mut Frame8, edge: Edge, map: impl Fn(f64, f64) -> Option<(f64, f64)> + Sync) {
    let src = f.clone();
    let w = f.width as usize;
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let v = match map(x as f64 + 0.5, y as f64 + 0.5) {
                    Some((sx, sy)) => sample_cubic(&src, sx, sy, edge),
                    None => [0.0; 4],
                };
                put(&mut row[x * 4..x * 4 + 4], v);
            }
        });
}

/// Each output pixel is the average of `n` samples at `at(x, y, s)` for `s` evenly spread over 0..1.
fn smear(f: &mut Frame8, n: usize, at: impl Fn(f64, f64, f64) -> (f64, f64) + Sync) {
    if n < 2 {
        return;
    }
    let src = f.clone();
    let w = f.width as usize;
    let inv = 1.0 / n as f32;
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for i in 0..n {
                    let s = (i as f64 + 0.5) / n as f64;
                    let (sx, sy) = at(x as f64 + 0.5, y as f64 + 0.5, s);
                    let v = sample(&src, sx, sy, Edge::Clamp);
                    for c in 0..4 {
                        acc[c] += v[c];
                    }
                }
                put(&mut row[x * 4..x * 4 + 4], acc.map(|v| v * inv));
            }
        });
}

/// Run `op` on every pixel's straight RGB (0..255 floats) in parallel; alpha stays.
fn per_pixel(f: &mut Frame8, op: impl Fn(usize, usize, [f32; 3]) -> [f32; 3] + Sync) {
    let w = f.width as usize;
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let px = &mut row[x * 4..x * 4 + 4];
                let o = op(x, y, [px[0] as f32, px[1] as f32, px[2] as f32]);
                for c in 0..3 {
                    px[c] = o[c].round().clamp(0.0, 255.0) as u8;
                }
            }
        });
}

/// The picture's brightness (Rec.709 weights), 0..1 per pixel.
fn luma_plane(f: &Frame8) -> Vec<f32> {
    f.rgba.par_chunks(4).map(luma8).collect()
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    if b <= a {
        return if x >= b { 1.0 } else { 0.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Screen blend (1 - (1-a)(1-b)) of `add` (0..1, may exceed 1) onto a 0..255 value.
fn screen(v: f32, add: f32) -> f32 {
    let a = v / 255.0;
    (1.0 - (1.0 - a) * (1.0 - add.clamp(0.0, 1.0))) * 255.0
}

// ---- noise ------------------------------------------------------------------------------------------------------

/// A random number in 0..1 for an integer coordinate.
fn rnd(seed: u64, i: i64) -> f64 {
    (hash(seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)) >> 11) as f64 / (1u64 << 53) as f64
}

fn rnd3(seed: u64, x: i64, y: i64, z: i64) -> f64 {
    let h = hash(
        seed ^ (x as u64).wrapping_mul(0x8CB9_2BA7_2F3D_8DD7)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ (z as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
    );
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth 1D value noise in -1..1.
fn noise1(seed: u64, t: f64) -> f64 {
    let i = t.floor();
    let u = t - i;
    let s = u * u * (3.0 - 2.0 * u);
    let (a, b) = (rnd(seed, i as i64), rnd(seed, i as i64 + 1));
    (a + (b - a) * s) * 2.0 - 1.0
}

/// Smooth 3D value noise in -1..1 (lattice values, smoothstep-weighted trilinear interpolation).
fn noise3(seed: u64, x: f64, y: f64, z: f64) -> f64 {
    let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
    let s = |u: f64| u * u * (3.0 - 2.0 * u);
    let (u, v, w) = (s(x - xi), s(y - yi), s(z - zi));
    let (xi, yi, zi) = (xi as i64, yi as i64, zi as i64);
    let c = |dx, dy, dz| rnd3(seed, xi + dx, yi + dy, zi + dz);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), u);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), u);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), u);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w) * 2.0 - 1.0
}

// ---- blur -------------------------------------------------------------------------------------------------------

/// Averages the picture over a one-parameter family of moves (sliding, turning about a point, scaling about a
/// point), `t` running over `t0..t1`. Such moves compose by adding their parameters, so a fine pass of `m` taps
/// followed by a coarse pass of `m` taps gives the same `m * m` evenly spaced taps for the cost of `2m`.
fn smear_along(
    f: &mut Frame8,
    taps: usize,
    t0: f64,
    t1: f64,
    act: impl Fn(f64, f64, f64) -> (f64, f64) + Sync,
) {
    let taps = taps.clamp(1, 64);
    if taps < 2 {
        return;
    }
    let m = (taps as f64).sqrt().ceil() as usize;
    let d = (t1 - t0) / m as f64;
    // fine pass: spread over one coarse step
    smear(f, m, |x, y, s| act(x, y, (s - 0.5 / m as f64) * d));
    // coarse pass: the steps themselves, from t0
    smear(f, m, |x, y, s| {
        act(x, y, t0 + (s - 0.5 / m as f64) * d * m as f64)
    });
}

fn directional_blur(f: &mut Frame8, angle_deg: f64, len: f64) {
    let (dy, dx) = angle_deg.to_radians().sin_cos();
    smear_along(f, len.ceil() as usize, -len / 2.0, len / 2.0, |x, y, t| {
        (x + dx * t, y + dy * t)
    });
}

fn centre(f: &Frame8, c: (f64, f64)) -> (f64, f64) {
    (c.0 / 100.0 * f.width as f64, c.1 / 100.0 * f.height as f64)
}

/// Samples along an arc around the centre: the picture blurred as if spinning.
fn spin_blur(f: &mut Frame8, degrees: f64, c: (f64, f64)) {
    let (cx, cy) = centre(f, c);
    let a = degrees.to_radians();
    let far = (f.width as f64).hypot(f.height as f64) / 2.0;
    smear_along(
        f,
        (a * far).ceil() as usize,
        -a / 2.0,
        a / 2.0,
        |x, y, t| {
            let (sin, cos) = t.sin_cos();
            let (px, py) = (x - cx, y - cy);
            (cx + px * cos - py * sin, cy + px * sin + py * cos)
        },
    );
}

/// Samples along the line to the centre: streaks pointing out of it. Scaling is spread evenly in log scale so the
/// moves compose (see [`smear_along`]).
fn zoom_blur(f: &mut Frame8, amount: f64, c: (f64, f64)) {
    let (cx, cy) = centre(f, c);
    let far = (f.width as f64).hypot(f.height as f64) / 2.0;
    let lo = (1.0 - amount.clamp(0.0, 0.95)).ln();
    smear_along(f, (amount * far).ceil() as usize, lo, 0.0, |x, y, t| {
        let k = t.exp();
        (cx + (x - cx) * k, cy + (y - cy) * k)
    });
}

/// A sharp horizontal band at `focus` (fraction of the height) with blur growing away from it.
fn tilt_shift(f: &mut Frame8, focus: f64, band: f64, falloff: f64, radius: f64) {
    let mut soft = f.clone();
    blur(&mut soft, radius);
    let h = f.height as f64;
    let w = f.width as usize;
    f.rgba
        .par_chunks_mut(w * 4)
        .zip(soft.rgba.par_chunks(w * 4))
        .enumerate()
        .for_each(|(y, (row, srow))| {
            let d = ((y as f64 + 0.5) / h - focus).abs() - band / 2.0;
            let m = smoothstep(0.0, falloff.max(1e-3), d) as f32;
            for (a, b) in row.iter_mut().zip(srow) {
                *a = (*a as f32 + (*b as f32 - *a as f32) * m).round() as u8;
            }
        });
}

/// Edge-preserving smoothing: a bilateral filter (Tomasi and Manduchi, 1998) that averages neighbours weighted by
/// both distance and how similar their colour is, so noise goes and edges stay.
fn denoise(f: &mut Frame8, strength: f64, r: usize) {
    if strength <= 0.0 {
        return;
    }
    let src = f.clone();
    let (w, h) = (f.width as usize, f.height as usize);
    let r = r.min(8) as i64;
    let sigma_s = (r as f32 / 1.5).max(0.5);
    let sigma_r = 4.0 + 46.0 * strength as f32;
    let (ks, kr) = (-0.5 / (sigma_s * sigma_s), -0.5 / (sigma_r * sigma_r));
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let c = [
                    src.rgba[i] as f32,
                    src.rgba[i + 1] as f32,
                    src.rgba[i + 2] as f32,
                ];
                let (mut acc, mut wsum) = ([0.0f32; 3], 0.0f32);
                for dy in -r..=r {
                    let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                    for dx in -r..=r {
                        let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                        let j = (yy * w + xx) * 4;
                        let q = [
                            src.rgba[j] as f32,
                            src.rgba[j + 1] as f32,
                            src.rgba[j + 2] as f32,
                        ];
                        let dc =
                            (q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2) + (q[2] - c[2]).powi(2);
                        let wt = ((dx * dx + dy * dy) as f32 * ks + dc / 3.0 * kr).exp();
                        for k in 0..3 {
                            acc[k] += q[k] * wt;
                        }
                        wsum += wt;
                    }
                }
                for k in 0..3 {
                    row[x * 4 + k] = (acc[k] / wsum).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
}

// ---- colour -----------------------------------------------------------------------------------------------------

fn table(op: impl Fn(f64) -> f64) -> [u8; 256] {
    std::array::from_fn(|i| (op(i as f64 / 255.0) * 255.0).round().clamp(0.0, 255.0) as u8)
}

fn brightness_contrast(f: &mut Frame8, b: f64, c: f64) {
    let gain = if c >= 0.0 {
        1.0 / (1.0 - 0.9 * c)
    } else {
        1.0 + c
    };
    let t = table(|v| (v - 0.5) * gain + 0.5 + b * 0.5);
    map_channels(f, &[t; 3]);
}

/// Input black and white points, gamma, then output black and white points (all in 0..255 levels).
fn levels(f: &mut Frame8, p: &[f64]) {
    let (ib, iw) = (p[0] / 255.0, (p[1] / 255.0).max(p[0] / 255.0 + 1.0 / 255.0));
    let (g, ob, ow) = (p[2].max(0.01), p[3] / 255.0, p[4] / 255.0);
    let t = table(|v| {
        let x = ((v - ib) / (iw - ib)).clamp(0.0, 1.0).powf(1.0 / g);
        ob + (ow - ob) * x
    });
    map_channels(f, &[t; 3]);
}

fn black_white(f: &mut Frame8, amount: f64, weights: [f64; 3]) {
    let wt = weights.map(|w| (w / 100.0) as f32);
    let a = amount as f32;
    per_pixel(f, |_, _, c| {
        let y = (c[0] * wt[0] + c[1] * wt[1] + c[2] * wt[2]).clamp(0.0, 255.0);
        c.map(|v| v + (y - v) * a)
    });
}

/// Keeps colours whose hue is within `range` degrees of `hue` and greys the rest (the reverse with `invert`).
fn color_isolate(f: &mut Frame8, hue: f64, range: f64, soft: f64, invert: bool) {
    per_pixel(f, |_, _, c| {
        let (h, s, _) = rgb_to_hsl(
            c[0] as f64 / 255.0,
            c[1] as f64 / 255.0,
            c[2] as f64 / 255.0,
        );
        let d = ((h - hue).rem_euclid(360.0)).min((hue - h).rem_euclid(360.0));
        let mut keep = 1.0 - smoothstep(range / 2.0, range / 2.0 + soft.max(0.5), d);
        // a grey pixel has no real hue
        keep *= smoothstep(0.03, 0.15, s);
        if invert {
            keep = 1.0 - keep;
        }
        let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        c.map(|v| y + (v - y) * keep as f32)
    });
}

fn channel_mixer(f: &mut Frame8, p: &[f64]) {
    let m: Vec<f32> = p.iter().map(|v| (*v / 100.0) as f32).collect();
    per_pixel(f, |_, _, c| {
        [
            c[0] * m[0] + c[1] * m[1] + c[2] * m[2],
            c[0] * m[3] + c[1] * m[4] + c[2] * m[5],
            c[0] * m[6] + c[1] * m[7] + c[2] * m[8],
        ]
    });
}

fn threshold(f: &mut Frame8, level: f64) {
    let l = level as f32;
    per_pixel(f, |_, _, c| {
        let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        if y >= l {
            [255.0; 3]
        } else {
            [0.0; 3]
        }
    });
}

// ---- distort ----------------------------------------------------------------------------------------------------

/// Radial lens model r_src = r (1 + k r^2) (barrel for k > 0), measured from the centre in units of half the
/// diagonal, then zoomed.
fn lens_distortion(f: &mut Frame8, curvature: f64, zoom: f64) {
    let (cx, cy) = (f.width as f64 / 2.0, f.height as f64 / 2.0);
    let half = cx.hypot(cy);
    let k = curvature * 0.5;
    let z = zoom.max(0.05);
    warp(f, Edge::Transparent, |x, y| {
        let (u, v) = ((x - cx) / half, (y - cy) / half);
        let m = (1.0 + k * (u * u + v * v)) / z;
        Some((cx + u * m * half, cy + v * m * half))
    });
}

fn radius_of(f: &Frame8, fraction: f64) -> f64 {
    (f.width.min(f.height) as f64 * fraction).max(1.0)
}

/// Inside the circle the distance from the centre is remapped to d^(1+a): the middle swells for a > 0 and pinches
/// for a < 0, meeting the untouched picture at the rim.
fn bulge(f: &mut Frame8, amount: f64, radius: f64, c: (f64, f64)) {
    let (cx, cy) = centre(f, c);
    let r = radius_of(f, radius);
    let e = 1.0 + amount.clamp(-0.9, 1.0);
    warp(f, Edge::Clamp, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let d = dx.hypot(dy) / r;
        if d >= 1.0 || d == 0.0 {
            return Some((x, y));
        }
        let m = d.powf(e) / d;
        Some((cx + dx * m, cy + dy * m))
    });
}

/// Rotation that is strongest at the centre and fades to none at the rim.
fn twirl(f: &mut Frame8, degrees: f64, radius: f64, c: (f64, f64)) {
    let (cx, cy) = centre(f, c);
    let r = radius_of(f, radius);
    let a = degrees.to_radians();
    warp(f, Edge::Clamp, |x, y| {
        let (dx, dy) = (x - cx, y - cy);
        let d = dx.hypot(dy) / r;
        if d >= 1.0 {
            return Some((x, y));
        }
        let t = a * (1.0 - d) * (1.0 - d);
        let (s, co) = t.sin_cos();
        Some((cx + dx * co - dy * s, cy + dx * s + dy * co))
    });
}

fn wave(f: &mut Frame8, amp: f64, wavelength: f64, vertical: bool, speed: f64, secs: f64) {
    let ph = secs * speed * std::f64::consts::TAU;
    let k = std::f64::consts::TAU / wavelength.max(1.0);
    warp(f, Edge::Clamp, |x, y| {
        Some(if vertical {
            (x, y + amp * (x * k + ph).sin())
        } else {
            (x + amp * (y * k + ph).sin(), y)
        })
    });
}

/// Displacement by two octaves of smooth noise that drifts with time.
fn turbulence(f: &mut Frame8, amount: f64, size: f64, speed: f64, secs: f64) {
    let s = 1.0 / size.max(1.0);
    let z = secs * speed;
    warp(f, Edge::Clamp, |x, y| {
        let n = |seed, sc: f64| noise3(seed, x * s * sc, y * s * sc, z * sc);
        let dx = n(11, 1.0) + 0.5 * n(13, 2.0);
        let dy = n(17, 1.0) + 0.5 * n(19, 2.0);
        Some((x + dx * amount, y + dy * amount))
    });
}

/// The projective map (homography) taking the unit square to four corners, solved in closed form (Heckbert, 1989,
/// "Fundamentals of Texture Mapping and Image Warping"), then inverted so each output pixel finds its source.
fn corner_pin(f: &mut Frame8, p: &[f64]) {
    let (w, h) = (f.width as f64, f.height as f64);
    let q: Vec<(f64, f64)> = (0..4)
        .map(|i| (p[2 * i] / 100.0 * w, p[2 * i + 1] / 100.0 * h))
        .collect();
    // corners in order: (0,0) (1,0) (1,1) (0,1)
    let (x0, y0, x1, y1, x2, y2, x3, y3) = (
        q[0].0, q[0].1, q[1].0, q[1].1, q[2].0, q[2].1, q[3].0, q[3].1,
    );
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
    let den = dx1 * dy2 - dx2 * dy1;
    if den.abs() < 1e-9 {
        return;
    }
    let g = (sx * dy2 - dx2 * sy) / den;
    let hh = (dx1 * sy - sx * dy1) / den;
    let m = [
        [x1 - x0 + g * x1, x3 - x0 + hh * x3, x0],
        [y1 - y0 + g * y1, y3 - y0 + hh * y3, y0],
        [g, hh, 1.0],
    ];
    let Some(inv) = invert3(m) else { return };
    warp(f, Edge::Transparent, |x, y| {
        let u = inv[0][0] * x + inv[0][1] * y + inv[0][2];
        let v = inv[1][0] * x + inv[1][1] * y + inv[1][2];
        let s = inv[2][0] * x + inv[2][1] * y + inv[2][2];
        if s.abs() < 1e-12 {
            return None;
        }
        let (u, v) = (u / s, v / s);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((u * w, v * h))
    });
}

fn invert3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let d = 1.0 / det;
    Some([
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
        ],
    ])
}

fn offset(f: &mut Frame8, ox: f64, oy: f64) {
    let (w, h) = (f.width as f64, f.height as f64);
    warp(f, Edge::Wrap, |x, y| Some((x - ox * w, y - oy * h)));
}

/// The picture shrunk by `n` (each small pixel the average of an n x n block) and repeated n x n times.
fn tile(f: &mut Frame8, n: usize) {
    if n < 2 {
        return;
    }
    let (w, h) = (f.width as usize, f.height as usize);
    let (sw, sh) = (w.div_ceil(n), h.div_ceil(n));
    let src = f.clone();
    let small: Vec<[f32; 4]> = (0..sw * sh)
        .into_par_iter()
        .map(|i| {
            let (tx, ty) = (i % sw, i / sw);
            let mut acc = [0.0f32; 4];
            let mut cnt = 0.0f32;
            for y in ty * n..((ty + 1) * n).min(h) {
                for x in tx * n..((tx + 1) * n).min(w) {
                    let j = (y * w + x) * 4;
                    let a = src.rgba[j + 3] as f32 / 255.0;
                    for (c, v) in acc.iter_mut().take(3).enumerate() {
                        *v += src.rgba[j + c] as f32 / 255.0 * a;
                    }
                    acc[3] += a;
                    cnt += 1.0;
                }
            }
            acc.map(|v| v / cnt.max(1.0))
        })
        .collect();
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                put(&mut row[x * 4..x * 4 + 4], small[(y % sh) * sw + x % sw]);
            }
        });
}

// ---- generate ---------------------------------------------------------------------------------------------------

fn mix_over(
    f: &mut Frame8,
    opacity: f32,
    colour: impl Fn(usize, usize) -> Option<[f32; 3]> + Sync,
) {
    per_pixel(f, |x, y, c| match colour(x, y) {
        Some(o) => [0, 1, 2].map(|i| c[i] + (o[i] - c[i]) * opacity),
        None => c,
    });
}

/// Bilinear blend of four corner colours, mixed over the picture.
fn gradient(f: &mut Frame8, p: &[f64]) {
    let col = |i: usize| [p[3 * i] as f32, p[3 * i + 1] as f32, p[3 * i + 2] as f32];
    let (tl, tr, bl, br) = (col(0), col(1), col(2), col(3));
    let (w, h) = (f.width as f32, f.height as f32);
    mix_over(f, (p[12] / 100.0) as f32, |x, y| {
        let (u, v) = ((x as f32 + 0.5) / w, (y as f32 + 0.5) / h);
        Some([0, 1, 2].map(|i| {
            let top = tl[i] + (tr[i] - tl[i]) * u;
            let bottom = bl[i] + (br[i] - bl[i]) * u;
            top + (bottom - top) * v
        }))
    });
}

fn grid(f: &mut Frame8, spacing: f64, thick: f64, opacity: f64, rgb: [f64; 3]) {
    let s = spacing.max(2.0);
    let t = thick.max(1.0).min(s);
    let (cx, cy) = (f.width as f64 / 2.0, f.height as f64 / 2.0);
    let c = rgb.map(|v| v as f32);
    mix_over(f, opacity as f32, |x, y| {
        let on = |p: f64, o: f64| (p + 0.5 - o + t / 2.0).rem_euclid(s) < t;
        (on(x as f64, cx) || on(y as f64, cy)).then_some(c)
    });
}

/// A light source: a hot core with a soft glow, a horizontal streak, a faint halo ring, and reflections ("ghosts")
/// strung along the line from the light through the middle of the frame, added with a screen blend.
fn lens_flare(f: &mut Frame8, c: (f64, f64), brightness: f64, size: f64) {
    let (lx, ly) = centre(f, c);
    let unit = f.height as f64 / 1080.0 * size;
    let (mx, my) = (f.width as f64 / 2.0, f.height as f64 / 2.0);
    let ghosts: [(f64, f64, [f32; 3]); 4] = [
        (0.6, 40.0, [0.6, 0.8, 1.0]),
        (1.3, 22.0, [1.0, 0.7, 0.4]),
        (1.7, 60.0, [0.5, 1.0, 0.6]),
        (2.2, 30.0, [0.9, 0.5, 1.0]),
    ];
    let b = brightness as f32;
    per_pixel(f, |x, y, col| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let d = (px - lx).hypot(py - ly) / unit;
        let core = (-(d / 18.0).powi(2)).exp() + 0.35 * (-(d / 90.0)).exp();
        let streak =
            (-((py - ly).abs() / unit / 3.0)).exp() * (-((px - lx).abs() / unit / 400.0)).exp();
        let halo = (-(((d - 160.0) / 10.0).powi(2))).exp() * 0.15;
        let mut add = [(core + 0.6 * streak + halo) as f32; 3];
        add[2] *= 1.05;
        for (t, r, tint) in ghosts {
            let (gx, gy) = (lx + (mx - lx) * t, ly + (my - ly) * t);
            let g = (px - gx).hypot(py - gy) / unit;
            let k = (1.0 - smoothstep(r * 0.8, r, g)) as f32 * 0.12;
            for i in 0..3 {
                add[i] += k * tint[i];
            }
        }
        [0, 1, 2].map(|i| screen(col[i], add[i] * b))
    });
}

/// Bright parts streak away from a centre: the highlights are zoom-blurred from it and added back.
fn light_rays(f: &mut Frame8, c: (f64, f64), length: f64, thresh: f64, intensity: f64) {
    let mut rays = f.clone();
    let t = thresh as f32;
    rays.rgba.par_chunks_mut(4).for_each(|p| {
        let y = luma8(p);
        let k = ((y - t) / (1.0 - t).max(1e-3)).clamp(0.0, 1.0);
        for v in &mut p[..3] {
            *v = (*v as f32 * k).round() as u8;
        }
        p[3] = 255;
    });
    zoom_blur(&mut rays, length.clamp(0.0, 1.0), c);
    // the streaks are an average along each ray, so they come out much dimmer than the highlights: lift them back
    let i = intensity as f32 * 4.0;
    let src = rays.rgba;
    let w = f.width as usize;
    per_pixel(f, |x, y, col| {
        let j = (y * w + x) * 4;
        [0, 1, 2].map(|c| screen(col[c], src[j + c] as f32 / 255.0 * i))
    });
}

/// Large soft blobs of warm light drifting across the frame, screen-blended.
fn light_leak(f: &mut Frame8, intensity: f64, speed: f64, hue: f64, seed: u64, secs: f64) {
    let (w, h) = (f.width as f64, f.height as f64);
    let t = secs * speed;
    let blobs: Vec<(f64, f64, f64, [f32; 3])> = (0..3)
        .map(|i| {
            let s = seed.wrapping_mul(31).wrapping_add(i);
            let x = 0.5 + 0.7 * noise1(s * 3 + 1, t * 0.7 + i as f64 * 3.1);
            let y = 0.5 + 0.6 * noise1(s * 3 + 2, t * 0.5 + i as f64 * 1.7);
            let r = (0.35 + 0.2 * noise1(s * 3 + 3, t * 0.3)) * w.max(h);
            let (rr, gg, bb) =
                crate::effects::hsl_to_rgb((hue + 25.0 * i as f64).rem_euclid(360.0), 0.95, 0.6);
            (x * w, y * h, r, [rr as f32, gg as f32, bb as f32])
        })
        .collect();
    let k = intensity as f32;
    per_pixel(f, |x, y, col| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let mut add = [0.0f32; 3];
        for (bx, by, r, c) in &blobs {
            let d = (px - bx).hypot(py - by) / r;
            let m = (-(d * d) * 2.5).exp() as f32;
            for i in 0..3 {
                add[i] += m * c[i];
            }
        }
        [0, 1, 2].map(|i| screen(col[i], add[i] * k))
    });
}

// ---- keying -----------------------------------------------------------------------------------------------------

fn luma_key(f: &mut Frame8, thresh: f64, soft: f64, invert: bool) {
    let (a, b) = ((thresh - soft / 2.0) as f32, (thresh + soft / 2.0) as f32);
    f.rgba.par_chunks_mut(4).for_each(|p| {
        let y = luma8(p);
        let mut keep = smoothstep(a as f64, b as f64, y as f64) as f32;
        if invert {
            keep = 1.0 - keep;
        }
        p[3] = (p[3] as f32 * keep).round() as u8;
    });
}

// ---- texture ----------------------------------------------------------------------------------------------------

/// Sepia toning, brightness flicker, vertical scratches and dust specks that change every frame.
fn film_damage(f: &mut Frame8, p: &[f64], k: f64, at: FxTime) {
    let (scratches, dust, flicker, sepia) =
        (p[0] / 100.0, p[1] / 100.0, p[2] / 100.0, p[3] / 100.0);
    let (w, h) = (f.width as f64, f.height as f64);
    let fr = at.frame;
    let gain = 1.0 + (rnd(fr, 1) - 0.5) * 0.35 * flicker;
    let n_scratch = (scratches * 4.0 * rnd(fr, 2) + scratches * 1.5).round() as i64;
    let lines: Vec<(f64, f64, f32)> = (0..n_scratch)
        .map(|i| {
            let x = rnd(fr, 10 + i) * w;
            let width = (0.6 + rnd(fr, 30 + i) * 1.4) * k;
            let shade = if rnd(fr, 50 + i) < 0.7 { 0.55 } else { 1.5 };
            (x, width, shade)
        })
        .collect();
    let n_dust = (dust * 40.0 * (0.5 + rnd(fr, 3))).round() as i64;
    let specks: Vec<(f64, f64, f64, f32)> = (0..n_dust)
        .map(|i| {
            (
                rnd(fr, 100 + i) * w,
                rnd(fr, 200 + i) * h,
                (1.0 + rnd(fr, 300 + i) * 4.0) * k,
                if rnd(fr, 400 + i) < 0.8 { 0.15 } else { 1.8 },
            )
        })
        .collect();
    let s = sepia as f32;
    per_pixel(f, |x, y, c| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        let yv = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let tone = [yv * 1.07, yv * 0.95, yv * 0.78];
        let mut o = [0, 1, 2].map(|i| (c[i] + (tone[i] - c[i]) * s) * gain as f32);
        for (lx, lw, shade) in &lines {
            if (px - lx).abs() < *lw {
                o = o.map(|v| v * shade);
            }
        }
        for (dx, dy, r, shade) in &specks {
            if (px - dx).hypot(py - dy) < *r {
                o = o.map(|v| v * shade);
            }
        }
        o
    });
}

/// Worn videotape: colour smeared sideways (low chroma bandwidth), a little line jitter, noise, and a rolling
/// band of tracking trouble.
fn analog(f: &mut Frame8, amount: f64, noise: f64, tracking: f64, k: f64, at: FxTime) {
    let (w, h) = (f.width as usize, f.height as usize);
    // chroma bleed: blur Cb and Cr along each line only
    let bleed = (2.0 + 10.0 * amount) * k;
    let src = f.clone();
    let reach = bleed.round().max(1.0) as i64;
    let band_y = ((at.secs * 0.25).fract() * 1.3 - 0.15) * h as f64;
    let band_h = (40.0 + 60.0 * tracking) * k;
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let in_band = ((y as f64) - band_y).abs() < band_h / 2.0 && tracking > 0.0;
            let jitter = (noise1(at.frame * 977 + y as u64 / 3, at.secs * 9.0) * amount * 2.0 * k
                + if in_band {
                    (rnd(at.frame, y as i64) - 0.5) * 30.0 * tracking * k
                } else {
                    0.0
                })
            .round() as i64;
            for x in 0..w {
                let sx = (x as i64 - jitter).clamp(0, w as i64 - 1) as usize;
                let j = (y * w + sx) * 4;
                let yv = luma8(&src.rgba[j..j + 4]) * 255.0;
                let (mut cb, mut cr, mut n) = (0.0f32, 0.0f32, 0.0f32);
                for dx in (-reach..=reach).step_by((reach as usize / 4).max(1)) {
                    let xx = (sx as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    let q = &src.rgba[(y * w + xx) * 4..(y * w + xx) * 4 + 3];
                    let (r, g, b) = (q[0] as f32, q[1] as f32, q[2] as f32);
                    cb += -0.1146 * r - 0.3854 * g + 0.5 * b;
                    cr += 0.5 * r - 0.4542 * g - 0.0458 * b;
                    n += 1.0;
                }
                let (cb, cr) = (cb / n * 0.85, cr / n * 0.85);
                let grain = ((rnd(at.frame.wrapping_mul(7919) ^ y as u64, x as i64) - 0.5)
                    * 70.0
                    * noise) as f32;
                let band_noise =
                    if in_band && rnd(at.frame ^ 0xABCD, (y * w + x) as i64) < 0.08 * tracking {
                        120.0
                    } else {
                        0.0
                    };
                let yy = yv + grain + band_noise as f32;
                let px = &mut row[x * 4..x * 4 + 4];
                px[0] = (yy + 1.5748 * cr).round().clamp(0.0, 255.0) as u8;
                px[1] = (yy - 0.1873 * cb - 0.4681 * cr).round().clamp(0.0, 255.0) as u8;
                px[2] = (yy + 1.8556 * cb).round().clamp(0.0, 255.0) as u8;
                px[3] = src.rgba[j + 3];
            }
        });
}

// ---- perspective ------------------------------------------------------------------------------------------------

/// The picture as a card turned in 3D and seen by a pinhole camera: each output ray is intersected with the
/// turned card to find which point of the picture it sees.
fn perspective(f: &mut Frame8, swivel: f64, tilt: f64, distance: f64) {
    let (w, h) = (f.width as f64, f.height as f64);
    let half = h / 2.0;
    let focal = 2.5;
    let z0 = focal * (1.0 + distance);
    let (sa, ca) = swivel.to_radians().sin_cos();
    let (sb, cb) = tilt.to_radians().sin_cos();
    // rotation: swivel about the vertical axis, then tilt about the horizontal one; columns are the card's axes
    let ax = [ca, 0.0, -sa];
    let ay = [sa * sb, cb, ca * sb];
    let n = [sa * cb, -sb, ca * cb];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    warp(f, Edge::Transparent, |x, y| {
        let d = [(x - w / 2.0) / half, (y - h / 2.0) / half, focal];
        let den = dot(n, d);
        if den.abs() < 1e-9 {
            return None;
        }
        let t = dot(n, [0.0, 0.0, z0]) / den;
        if t <= 0.0 {
            return None;
        }
        let p = [d[0] * t, d[1] * t, d[2] * t - z0];
        let (u, v) = (
            dot(ax, p) * focal / z0.max(1e-3),
            dot(ay, p) * focal / z0.max(1e-3),
        );
        Some((w / 2.0 + u * half, h / 2.0 + v * half))
    });
}

/// The layer's silhouette in the shadow colour, softened. The renderer draws it under the layer, moved by the
/// distance and angle.
pub(crate) fn shadow_of(f: &Frame8, p: &[f64], k: f64) -> Frame8 {
    let mut s = f.clone();
    let rgb = [p[4] as u8, p[5] as u8, p[6] as u8];
    s.rgba.par_chunks_mut(4).for_each(|px| {
        px[..3].copy_from_slice(&rgb);
    });
    blur(&mut s, p[2] * k);
    s
}

// ---- stylize ----------------------------------------------------------------------------------------------------

/// Gradient magnitude of brightness with the Sobel operator (Sobel and Feldman, 1968), 0..~1.
fn sobel(l: &[f32], w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let at = |dx: i64, dy: i64| {
                let xx = (x + dx).clamp(0, w as i64 - 1) as usize;
                let yy = (y + dy).clamp(0, h as i64 - 1) as usize;
                l[yy * w + xx]
            };
            let gx =
                at(1, -1) + 2.0 * at(1, 0) + at(1, 1) - at(-1, -1) - 2.0 * at(-1, 0) - at(-1, 1);
            let gy =
                at(-1, 1) + 2.0 * at(0, 1) + at(1, 1) - at(-1, -1) - 2.0 * at(0, -1) - at(1, -1);
            (gx * gx + gy * gy).sqrt() / 4.0
        })
        .collect()
}

fn find_edges(f: &mut Frame8, amount: f64, invert: bool) {
    let (w, h) = (f.width as usize, f.height as usize);
    let e = sobel(&luma_plane(f), w, h);
    let a = amount as f32;
    per_pixel(f, |x, y, c| {
        let m = (e[y * w + x] * 3.0).clamp(0.0, 1.0);
        let v = if invert { m * 255.0 } else { (1.0 - m) * 255.0 };
        c.map(|o| o + (v - o) * a)
    });
}

fn emboss(f: &mut Frame8, direction: f64, relief: f64, amount: f64, colour: f64) {
    let (w, h) = (f.width as usize, f.height as usize);
    let l = luma_plane(f);
    let (dy, dx) = direction.to_radians().sin_cos();
    // at least a pixel each way, or a small frame comes out flat grey
    let r = relief.max(1.0);
    let (a, c) = (amount as f32, colour as f32);
    per_pixel(f, |x, y, col| {
        let at = |s: f64| {
            let xx = (x as f64 + dx * s).round().clamp(0.0, w as f64 - 1.0) as usize;
            let yy = (y as f64 + dy * s).round().clamp(0.0, h as f64 - 1.0) as usize;
            l[yy * w + xx]
        };
        let v = (at(-r) - at(r)) * 255.0 * 2.0;
        let grey = 128.0 + v;
        let coloured = col.map(|o| o + v);
        let out = [0, 1, 2].map(|i| grey + (coloured[i] - grey) * c);
        [0, 1, 2].map(|i| col[i] + (out[i] - col[i]) * a)
    });
}

/// Round dots on a turned grid, bigger where the picture is darker (black ink on white paper), or dots of the
/// picture's own colour.
fn halftone(f: &mut Frame8, dot: f64, angle: f64, colour: bool) {
    let mut soft = f.clone();
    blur(&mut soft, dot / 2.0);
    let w = f.width as usize;
    let s = dot.max(2.0);
    let (sa, ca) = angle.to_radians().sin_cos();
    let src = soft.rgba;
    let (fw, fh) = (f.width as f64, f.height as f64);
    per_pixel(f, |x, y, _| {
        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
        // into the turned grid, find the cell centre, back to the picture
        let (u, v) = (px * ca + py * sa, -px * sa + py * ca);
        let (cu, cv) = ((u / s).floor() * s + s / 2.0, (v / s).floor() * s + s / 2.0);
        let (cx, cy) = (cu * ca - cv * sa, cu * sa + cv * ca);
        let (ix, iy) = (
            cx.clamp(0.0, fw - 1.0) as usize,
            cy.clamp(0.0, fh - 1.0) as usize,
        );
        let j = (iy * w + ix) * 4;
        let cell = [src[j] as f32, src[j + 1] as f32, src[j + 2] as f32];
        let lum = (0.2126 * cell[0] + 0.7152 * cell[1] + 0.0722 * cell[2]) / 255.0;
        let radius = s / 2.0 * 1.25 * (1.0 - lum as f64).max(0.0).sqrt();
        let d = (u - cu).hypot(v - cv);
        let ink = 1.0 - smoothstep(radius - 0.75, radius + 0.75, d) as f32;
        if colour {
            let k = if radius > 0.0 { ink } else { 0.0 };
            [0, 1, 2].map(|i| 255.0 + (cell[i] - 255.0) * k.max(0.0))
        } else {
            [255.0 * (1.0 - ink); 3]
        }
    });
}

fn scanlines(f: &mut Frame8, spacing: f64, opacity: f64, speed: f64, secs: f64) {
    let s = spacing.max(2.0);
    let off = secs * speed;
    let o = opacity as f32;
    per_pixel(f, |_, y, c| {
        let ph = ((y as f64 + 0.5 + off) / s * std::f64::consts::TAU).cos() as f32;
        let dark = o * (0.5 - 0.5 * ph);
        c.map(|v| v * (1.0 - dark))
    });
}

/// Flat colour: smoothed, then fewer colour steps, with dark lines where brightness changes sharply.
fn cartoon(f: &mut Frame8, levels: u32, edges: f64, smooth: f64) {
    if smooth > 0.0 {
        kuwahara(f, smooth.round().max(1.0) as usize);
    }
    crate::effects::posterize(f, levels.max(2));
    let (w, h) = (f.width as usize, f.height as usize);
    let e = sobel(&luma_plane(f), w, h);
    let k = edges as f32;
    per_pixel(f, |x, y, c| {
        let m = smoothstep(0.08, 0.25, e[y * w + x] as f64) as f32 * k;
        c.map(|v| v * (1.0 - m))
    });
}

/// Pencil drawing by a colour dodge of the grey picture with its blurred negative.
fn sketch(f: &mut Frame8, strength: f64, colour: f64, k: f64) {
    let (w, h) = (f.width as usize, f.height as usize);
    let l = luma_plane(f);
    let neg: Vec<f32> = l.iter().map(|v| 1.0 - v).collect();
    let soft = blur_plane(&neg, w, h, 6.0 * k);
    let (s, c) = (strength as f32, colour as f32);
    per_pixel(f, |x, y, col| {
        let i = y * w + x;
        let d = (l[i] / (1.0 - soft[i]).max(1e-3)).min(1.0);
        let pencil = d.powf(2.0) * 255.0;
        let tinted = col.map(|v| pencil * (0.6 + 0.4 * v / 255.0));
        let out = [0, 1, 2].map(|j| pencil + (tinted[j] - pencil) * c);
        [0, 1, 2].map(|j| col[j] + (out[j] - col[j]) * s)
    });
}

/// The Kuwahara filter (Kuwahara et al., 1976): each pixel takes the mean colour of whichever of its four
/// surrounding quadrants is most even, which flattens texture into brush-like patches while keeping edges.
/// Sums come from summed-area tables (Crow, 1984), so the cost does not grow with the brush size.
fn kuwahara(f: &mut Frame8, r: usize) {
    let (w, h) = (f.width as usize, f.height as usize);
    let (sw, sh) = (w + 1, h + 1);
    // tables for r, g, b and brightness squared
    let mut sat = vec![[0.0f64; 5]; sw * sh];
    for y in 0..h {
        let mut row = [0.0f64; 5];
        for x in 0..w {
            let i = (y * w + x) * 4;
            let (r_, g_, b_) = (f.rgba[i] as f64, f.rgba[i + 1] as f64, f.rgba[i + 2] as f64);
            let l = 0.2126 * r_ + 0.7152 * g_ + 0.0722 * b_;
            let v = [r_, g_, b_, l, l * l];
            for c in 0..5 {
                row[c] += v[c];
            }
            let above = sat[y * sw + x + 1];
            sat[(y + 1) * sw + x + 1] = std::array::from_fn(|c| row[c] + above[c]);
        }
    }
    let r = r as i64;
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, out)| {
            for x in 0..w {
                let (xi, yi) = (x as i64, y as i64);
                let mut best = (f64::MAX, [0.0f64; 3]);
                for (qx, qy) in [(-r, -r), (0, -r), (-r, 0), (0, 0)] {
                    let x0 = (xi + qx).clamp(0, w as i64 - 1) as usize;
                    let y0 = (yi + qy).clamp(0, h as i64 - 1) as usize;
                    let x1 = (xi + qx + r).clamp(0, w as i64 - 1) as usize + 1;
                    let y1 = (yi + qy + r).clamp(0, h as i64 - 1) as usize + 1;
                    let n = ((x1 - x0) * (y1 - y0)) as f64;
                    let s = |c: usize| {
                        sat[y1 * sw + x1][c] - sat[y0 * sw + x1][c] - sat[y1 * sw + x0][c]
                            + sat[y0 * sw + x0][c]
                    };
                    let mean_l = s(3) / n;
                    let var = s(4) / n - mean_l * mean_l;
                    if var < best.0 {
                        best = (var, [s(0) / n, s(1) / n, s(2) / n]);
                    }
                }
                for c in 0..3 {
                    out[x * 4 + c] = best.1[c].round().clamp(0.0, 255.0) as u8;
                }
            }
        });
}

/// Bands of the picture shifted sideways with their red split off, re-rolled `speed` times a second.
fn glitch(f: &mut Frame8, amount: f64, block: f64, speed: f64, seed: u64, secs: f64) {
    if amount <= 0.0 {
        return;
    }
    let step = (secs * speed.max(0.01)).floor() as i64;
    let s = seed.wrapping_mul(0x51_7CC1_B727_220A) ^ step as u64;
    let (w, h) = (f.width as usize, f.height as usize);
    let src = f.clone();
    let bands: Vec<(usize, usize, i64, i64)> = (0..(1.0 + amount * 8.0) as i64)
        .filter(|i| rnd(s, 1000 + i) < 0.4 + amount * 0.6)
        .map(|i| {
            let y0 = (rnd(s, 2 * i) * h as f64) as usize;
            let bh = ((0.2 + rnd(s, 2 * i + 1)) * block).max(1.0) as usize;
            let shift = ((rnd(s, 3000 + i) - 0.5) * 2.0 * amount * w as f64 * 0.15) as i64;
            let split = ((rnd(s, 4000 + i) - 0.5) * amount * block * 0.6) as i64;
            (y0, (y0 + bh).min(h), shift, split)
        })
        .collect();
    f.rgba
        .par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let Some(&(_, _, shift, split)) = bands.iter().find(|(a, b, _, _)| y >= *a && y < *b)
            else {
                return;
            };
            for x in 0..w {
                let at = |dx: i64| ((x as i64 - dx).rem_euclid(w as i64)) as usize;
                let (j, jr) = ((y * w + at(shift)) * 4, (y * w + at(shift + split)) * 4);
                row[x * 4] = src.rgba[jr];
                row[x * 4 + 1] = src.rgba[j + 1];
                row[x * 4 + 2] = src.rgba[j + 2];
                row[x * 4 + 3] = src.rgba[j + 3];
            }
        });
}

fn strobe(f: &mut Frame8, rate: f64, length: f64, brightness: f64, secs: f64) {
    let ph = (secs * rate.max(0.01)).fract();
    if ph >= length {
        return;
    }
    let k = brightness as f32;
    per_pixel(f, |_, _, c| c.map(|v| v + (255.0 - v) * k));
}

// ---- transform --------------------------------------------------------------------------------------------------

/// Smooth random movement and rotation from noise, zoomed in a little so the frame edge never shows.
fn camera_shake(f: &mut Frame8, amount: f64, rotation: f64, speed: f64, zoom: f64, secs: f64) {
    let t = secs * speed;
    let (ox, oy) = (noise1(101, t) * amount, noise1(202, t + 7.3) * amount);
    let rot = (noise1(303, t + 2.1) * rotation).to_radians();
    let z = zoom.max(1.0);
    let (cx, cy) = (f.width as f64 / 2.0, f.height as f64 / 2.0);
    let (s, c) = (-rot).sin_cos();
    warp(f, Edge::Clamp, |x, y| {
        let (dx, dy) = ((x - cx) / z, (y - cy) / z);
        Some((cx + dx * c - dy * s - ox, cy + dx * s + dy * c - oy))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use captaine_model::{Effect, EffectKind};

    /// A test picture with detail everywhere: diagonal colour ramps and a bright square.
    fn picture() -> Frame8 {
        let (w, h) = (96u32, 64u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let bright = (30..50).contains(&x) && (20..40).contains(&y);
                let (r, g, b) = if bright {
                    (250, 245, 235)
                } else {
                    (
                        (x * 255 / w) as u8,
                        (y * 255 / h) as u8,
                        (((x + y) * 3) % 256) as u8,
                    )
                };
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Frame8 {
            width: w,
            height: h,
            rgba,
        }
    }

    #[test]
    fn warps_keep_fine_detail() {
        // one-pixel stripes, moved a quarter of a pixel: the bicubic warp keeps more of their contrast than
        // averaging the two nearest pixels would (0.75/0.25 mix), and an unmoved picture is untouched
        let (w, h) = (16u32, 4u32);
        let rgba = (0..w * h)
            .flat_map(|i| {
                let v = if i % 2 == 0 { 230 } else { 20 };
                [v, v, v, 255]
            })
            .collect();
        let stripes = Frame8 {
            width: w,
            height: h,
            rgba,
        };
        let mut same = stripes.clone();
        warp(&mut same, Edge::Clamp, |x, y| Some((x, y)));
        assert_eq!(same.rgba, stripes.rgba);
        let mut moved = stripes.clone();
        warp(&mut moved, Edge::Clamp, |x, y| Some((x + 0.25, y)));
        let (a, b) = (moved.rgba[6 * 4] as f64, moved.rgba[7 * 4] as f64);
        let bilinear = 210.0 * 0.5;
        assert!(
            (a - b).abs() > bilinear + 20.0,
            "contrast {}",
            (a - b).abs()
        );
    }

    fn noisy() -> Frame8 {
        let mut f = picture();
        for (i, v) in f.rgba.iter_mut().enumerate() {
            if i % 4 != 3 {
                *v = (*v as f64 + (rnd(9, i as i64) - 0.5) * 60.0).clamp(0.0, 255.0) as u8;
            }
        }
        f
    }

    fn diff(a: &Frame8, b: &Frame8) -> f64 {
        a.rgba
            .iter()
            .zip(&b.rgba)
            .map(|(x, y)| (*x as f64 - *y as f64).abs())
            .sum::<f64>()
            / a.rgba.len() as f64
            / 255.0
    }

    fn run(kind: EffectKind, at: FxTime) -> Frame8 {
        let mut f = picture();
        let e = Effect::new(kind);
        assert!(apply(kind, &mut f, &e.params, 1.0, at));
        f
    }

    /// Every library effect, at its default settings, visibly changes the picture (drop shadow and low frame
    /// rate act elsewhere and are checked in the renderer tests).
    #[test]
    fn every_library_effect_changes_the_picture() {
        let at = FxTime::new(7, 0.37);
        for kind in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
            let mut f = picture();
            let mut e = Effect::new(kind);
            // these start neutral, like any adjustment; give them something to do
            match kind {
                EffectKind::BrightnessContrast => e.params = vec![30.0, 20.0],
                EffectKind::Levels => e.params[0] = 40.0,
                EffectKind::ChannelMixer => e.params[1] = 50.0,
                EffectKind::CornerPin => e.params[0] = 20.0,
                EffectKind::Offset => e.params[0] = 25.0,
                _ => {}
            }
            if !apply(kind, &mut f, &e.params, 1.0, at) {
                continue;
            }
            if matches!(kind, EffectKind::DropShadow | EffectKind::LowFrameRate) {
                assert_eq!(f, picture(), "{kind:?} leaves the frame alone");
                continue;
            }
            // strobe only flashes for part of each beat; denoise needs noise to remove
            let (f, before) = match kind {
                EffectKind::Strobe => (run(kind, FxTime::new(0, 0.01)), picture()),
                EffectKind::Denoise => {
                    let mut n = noisy();
                    apply(kind, &mut n, &e.params, 1.0, at);
                    (n, noisy())
                }
                _ => (f, picture()),
            };
            let d = diff(&before, &f);
            assert!(d > 0.003, "{kind:?} changed the picture by only {d}");
            assert_eq!((f.width, f.height), (96, 64));
        }
    }

    #[test]
    fn neutral_settings_leave_the_picture_alone() {
        let neutral: [(EffectKind, &[f64]); 6] = [
            (EffectKind::BrightnessContrast, &[0.0, 0.0]),
            (EffectKind::Levels, &[0.0, 255.0, 1.0, 0.0, 255.0]),
            (
                EffectKind::ChannelMixer,
                &[100.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, 100.0],
            ),
            (
                EffectKind::CornerPin,
                &[0.0, 0.0, 100.0, 0.0, 100.0, 100.0, 0.0, 100.0],
            ),
            (EffectKind::Offset, &[0.0, 0.0]),
            (EffectKind::Tile, &[1.0]),
        ];
        for (kind, p) in neutral {
            let mut f = picture();
            apply(kind, &mut f, p, 1.0, FxTime::default());
            assert!(diff(&picture(), &f) < 0.004, "{kind:?}");
        }
    }

    #[test]
    fn animated_effects_move_with_time_and_still_ones_do_not() {
        for kind in EffectKind::ALL.into_iter().filter(|k| !k.is_audio()) {
            let e = Effect::new(kind);
            let mut a = picture();
            if !apply(kind, &mut a, &e.params, 1.0, FxTime::new(3, 0.2)) {
                continue;
            }
            let mut b = picture();
            apply(kind, &mut b, &e.params, 1.0, FxTime::new(40, 1.7));
            if kind == EffectKind::Strobe {
                continue; // may be dark at both times
            }
            assert_eq!(
                kind.is_animated(),
                a != b,
                "{kind:?}: is_animated() says {}",
                kind.is_animated()
            );
        }
    }

    #[test]
    fn effects_do_what_their_names_say() {
        let p = picture();
        let bright = |f: &Frame8| {
            f.rgba
                .chunks(4)
                .map(|q| luma8(q) as f64 * 255.0)
                .sum::<f64>()
                / (f.rgba.len() / 4) as f64
        };
        // brightness up brightens; levels with a raised black point darkens
        let mut b = p.clone();
        brightness_contrast(&mut b, 0.4, 0.0);
        assert!(bright(&b) > bright(&p) + 20.0);
        let mut l = p.clone();
        levels(&mut l, &[80.0, 255.0, 1.0, 0.0, 255.0]);
        assert!(bright(&l) < bright(&p) - 10.0);
        // black & white has no colour left
        let mut g = p.clone();
        black_white(&mut g, 1.0, [30.0, 59.0, 11.0]);
        assert!(g.rgba.chunks(4).all(|q| q[0] == q[1] && q[1] == q[2]));
        // threshold leaves only black and white
        let mut t = p.clone();
        threshold(&mut t, 128.0);
        assert!(t
            .rgba
            .chunks(4)
            .all(|q| (q[0] == 0 || q[0] == 255) && q[0] == q[2]));
        // a luma key removes the dark corner and keeps the bright square
        let mut k = p.clone();
        luma_key(&mut k, 0.3, 0.05, false);
        assert_eq!(k.rgba[3], 0, "top-left is dark: keyed out");
        assert_eq!(
            k.rgba[((30 * 96 + 40) * 4 + 3) as usize],
            255,
            "bright square stays"
        );
        // offset by half wraps the picture around
        let mut o = p.clone();
        offset(&mut o, 0.5, 0.0);
        assert_eq!(&o.rgba[..4], &p.rgba[48 * 4..48 * 4 + 4]);
        // a corner pin onto the left half leaves the right half empty
        let mut c = p.clone();
        corner_pin(&mut c, &[0.0, 0.0, 50.0, 0.0, 50.0, 100.0, 0.0, 100.0]);
        assert_eq!(c.rgba[((10 * 96 + 80) * 4 + 3) as usize], 0);
        assert!(c.rgba[((10 * 96 + 20) * 4 + 3) as usize] > 0);
        // a 3D swivel of 0 with no distance change is the same picture
        let mut q = p.clone();
        perspective(&mut q, 0.0, 0.0, 0.0);
        assert!(diff(&p, &q) < 0.004);
        // colour isolate on red keeps the reddest pixels coloured and greys a green one
        let mut iso = Frame8 {
            width: 2,
            height: 1,
            rgba: vec![220, 30, 30, 255, 30, 200, 40, 255],
        };
        color_isolate(&mut iso, 0.0, 40.0, 10.0, false);
        assert!(iso.rgba[0] > iso.rgba[1] + 100);
        assert!((iso.rgba[4] as i32 - iso.rgba[5] as i32).abs() < 3);
    }
}
