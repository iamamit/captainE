//! Sound effects on interleaved stereo f32: EQ, compressor, limiter, de-esser, noise reduction.

use std::f64::consts::PI;

use captaine_model::{Effect, EffectKind};

/// Run every enabled sound effect of the list, in order, over `buf` (interleaved stereo).
pub fn apply_audio_effects(buf: &mut [f32], sr: u32, effects: &[Effect]) {
    for e in effects.iter().filter(|e| e.enabled && e.kind.is_audio()) {
        let p = &e.params;
        match e.kind {
            EffectKind::Eq => eq(buf, sr, p),
            EffectKind::Compressor => compressor(buf, sr, p[0], p[1], p[2], p[3], p[4]),
            EffectKind::Limiter => limiter(buf, sr, p[0], p[1]),
            EffectKind::DeEsser => de_esser(buf, sr, p[0], p[1] / 100.0),
            EffectKind::NoiseReduction => noise_reduction(buf, p[0], p[1] / 100.0),
            // needs the rest of the mix, so the mixer applies it
            EffectKind::Ducking => {}
            _ => {}
        }
    }
}

/// A biquad filter (RBJ cookbook), direct form I, one state per channel.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn from(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Biquad {
        Biquad {
            b: [b0 / a0, b1 / a0, b2 / a0],
            a: [a1 / a0, a2 / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn peaking(sr: f64, f: f64, gain_db: f64, q: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let alpha = w.sin() / (2.0 * q);
        let c = w.cos();
        Biquad::from(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    fn shelf(sr: f64, f: f64, gain_db: f64, high: bool) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let (s, c) = w.sin_cos();
        let alpha = s / 2.0 * 2f64.sqrt(); // slope 1
        let t = 2.0 * a.sqrt() * alpha;
        if high {
            Biquad::from(
                a * ((a + 1.0) + (a - 1.0) * c + t),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - t),
                (a + 1.0) - (a - 1.0) * c + t,
                2.0 * ((a - 1.0) - (a + 1.0) * c),
                (a + 1.0) - (a - 1.0) * c - t,
            )
        } else {
            Biquad::from(
                a * ((a + 1.0) - (a - 1.0) * c + t),
                2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                a * ((a + 1.0) - (a - 1.0) * c - t),
                (a + 1.0) + (a - 1.0) * c + t,
                -2.0 * ((a - 1.0) + (a + 1.0) * c),
                (a + 1.0) + (a - 1.0) * c - t,
            )
        }
    }

    fn band_pass(sr: f64, f: f64, q: f64) -> Biquad {
        let w = 2.0 * PI * f.min(sr * 0.49) / sr;
        let alpha = w.sin() / (2.0 * q);
        Biquad::from(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * w.cos(), 1.0 - alpha)
    }

    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

fn eq(buf: &mut [f32], sr: u32, p: &[f64]) {
    let sr = sr as f64;
    let mut bands = Vec::new();
    if p[0] != 0.0 {
        bands.push(Biquad::shelf(sr, p[1], p[0], false));
    }
    if p[2] != 0.0 {
        bands.push(Biquad::peaking(sr, p[3], p[2], p[4]));
    }
    if p[5] != 0.0 {
        bands.push(Biquad::peaking(sr, p[6], p[5], p[7]));
    }
    if p[8] != 0.0 {
        bands.push(Biquad::shelf(sr, p[9], p[8], true));
    }
    if bands.is_empty() {
        return;
    }
    for ch in 0..2 {
        let mut chain = bands.clone();
        for s in buf.iter_mut().skip(ch).step_by(2) {
            let mut v = *s as f64;
            for b in chain.iter_mut() {
                v = b.run(v);
            }
            *s = v as f32;
        }
    }
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-9).log10()
}

fn undb(d: f64) -> f64 {
    10f64.powf(d / 20.0)
}

/// One-pole smoothing coefficient for a time constant in milliseconds.
fn coef(ms: f64, sr: u32) -> f64 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr as f64)).exp()
}

/// Feed-forward compressor with a hard knee, linked across both channels.
fn compressor(
    buf: &mut [f32],
    sr: u32,
    threshold: f64,
    ratio: f64,
    attack: f64,
    release: f64,
    makeup: f64,
) {
    let (atk, rel) = (coef(attack, sr), coef(release, sr));
    let mut reduction = 0.0f64; // dB of gain reduction currently applied (>= 0)
    for f in buf.chunks_exact_mut(2) {
        let level = db((f[0].abs().max(f[1].abs())) as f64);
        let over = (level - threshold).max(0.0);
        let target = over * (1.0 - 1.0 / ratio);
        let c = if target > reduction { atk } else { rel };
        reduction = c * reduction + (1.0 - c) * target;
        let g = undb(makeup - reduction) as f32;
        f[0] *= g;
        f[1] *= g;
    }
}

/// A limiter whose gain drops instantly (so nothing passes the ceiling) and recovers smoothly.
fn limiter(buf: &mut [f32], sr: u32, ceiling_db: f64, release: f64) {
    let ceil = undb(ceiling_db);
    let rel = coef(release, sr);
    let mut g = 1.0f64;
    for f in buf.chunks_exact_mut(2) {
        let peak = f[0].abs().max(f[1].abs()) as f64;
        let target = if peak > ceil { ceil / peak } else { 1.0 };
        g = if target < g {
            target
        } else {
            rel * g + (1.0 - rel) * target
        };
        f[0] = (f[0] as f64 * g) as f32;
        f[1] = (f[1] as f64 * g) as f32;
    }
}

/// Split-band de-esser: the 5-9 kHz band is turned down only while it is louder than the threshold.
fn de_esser(buf: &mut [f32], sr: u32, threshold: f64, amount: f64) {
    let mut bp = [Biquad::band_pass(sr as f64, 6500.0, 1.2); 2];
    let (atk, rel) = (coef(1.0, sr), coef(40.0, sr));
    let mut env = 0.0f64;
    for f in buf.chunks_exact_mut(2) {
        let band = [bp[0].run(f[0] as f64), bp[1].run(f[1] as f64)];
        let level = band[0].abs().max(band[1].abs());
        let c = if level > env { atk } else { rel };
        env = c * env + (1.0 - c) * level;
        let over = (db(env) - threshold).max(0.0);
        let cut = (over * amount).min(24.0);
        // remove the excess from the band only
        let gain = undb(-cut);
        for ch in 0..2 {
            f[ch] = (f[ch] as f64 - band[ch] * (1.0 - gain)) as f32;
        }
    }
}

// ---- noise reduction ------------------------------------------------------------------------------

const N: usize = 2048;
const HOP: usize = N / 4;

/// In-place iterative radix-2 FFT.
pub(crate) fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos() as f32, ang.sin() as f32);
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
    if inverse {
        for v in re.iter_mut().chain(im.iter_mut()) {
            *v /= n as f32;
        }
    }
}

/// Spectral subtraction. The noise level of every frequency bin is learned from the quietest part of the
/// buffer (a low percentile over time), then subtracted from every frame with a floor.
fn noise_reduction(buf: &mut [f32], reduction_db: f64, sensitivity: f64) {
    let frames = buf.len() / 2;
    if frames < N * 2 || reduction_db <= 0.0 {
        return;
    }
    let window: Vec<f32> = (0..N)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos()).sqrt() as f32)
        .collect();
    let floor = undb(-reduction_db) as f32;
    let over = 1.0 + sensitivity as f32 * 2.0;
    let pct = 0.10 + 0.15 * (1.0 - sensitivity);
    for ch in 0..2 {
        let x: Vec<f32> = buf.iter().skip(ch).step_by(2).copied().collect();
        let padded_len = frames + N;
        let mut padded = vec![0.0f32; padded_len + N];
        padded[N / 2..N / 2 + frames].copy_from_slice(&x);
        let starts: Vec<usize> = (0..=(padded_len - N)).step_by(HOP).collect();
        // analysis
        let mut spectra: Vec<(Vec<f32>, Vec<f32>)> = Vec::with_capacity(starts.len());
        for &s in &starts {
            let mut re: Vec<f32> = (0..N).map(|i| padded[s + i] * window[i]).collect();
            let mut im = vec![0.0; N];
            fft(&mut re, &mut im, false);
            spectra.push((re, im));
        }
        // noise floor per bin
        let bins = N / 2 + 1;
        let mut noise = vec![0.0f32; bins];
        let mut col = vec![0.0f32; spectra.len()];
        for (k, nz) in noise.iter_mut().enumerate() {
            for (t, (re, im)) in spectra.iter().enumerate() {
                col[t] = re[k].hypot(im[k]);
            }
            col.sort_by(|a, b| a.total_cmp(b));
            *nz = col[((col.len() - 1) as f64 * pct) as usize];
        }
        // subtraction and overlap-add
        let mut out = vec![0.0f32; padded.len()];
        for (&s, (re, im)) in starts.iter().zip(spectra.iter_mut()) {
            let mut gains = vec![1.0f32; bins];
            for k in 0..bins {
                let mag = re[k].hypot(im[k]).max(1e-12);
                gains[k] = ((mag - over * noise[k]) / mag).clamp(floor, 1.0);
            }
            // smooth across bins to avoid isolated tonal "musical noise"
            let sm: Vec<f32> = (0..bins)
                .map(|k| {
                    let (a, b) = (gains[k.saturating_sub(1)], gains[(k + 1).min(bins - 1)]);
                    0.25 * a + 0.5 * gains[k] + 0.25 * b
                })
                .collect();
            for k in 0..N {
                let g = sm[if k < bins { k } else { N - k }];
                re[k] *= g;
                im[k] *= g;
            }
            fft(re, im, true);
            for i in 0..N {
                out[s + i] += re[i] * window[i] / 2.0;
            }
        }
        for (i, v) in out[N / 2..N / 2 + frames].iter().enumerate() {
            buf[i * 2 + ch] = *v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn tone(freq: f64, amp: f64, secs: f64) -> Vec<f32> {
        (0..(secs * SR as f64) as usize)
            .flat_map(|i| {
                let v = (amp * (2.0 * PI * freq * i as f64 / SR as f64).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    fn rms(b: &[f32]) -> f64 {
        (b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / b.len() as f64).sqrt()
    }

    fn with(kind: EffectKind, params: &[f64]) -> Vec<Effect> {
        let mut e = Effect::new(kind);
        e.params = params.to_vec();
        assert_eq!(e.check(), Ok(()));
        vec![e]
    }

    fn settled(b: &[f32]) -> &[f32] {
        &b[b.len() / 2..]
    }

    #[test]
    fn eq_boosts_and_cuts_around_its_frequency() {
        let mut e = Effect::new(EffectKind::Eq);
        e.params[2] = 12.0; // mid 1: +12 dB at 500 Hz
        let fx = vec![e];
        let mut at = tone(500.0, 0.1, 1.0);
        let before = rms(settled(&at));
        apply_audio_effects(&mut at, SR, &fx);
        let ratio = rms(settled(&at)) / before;
        assert!(
            (ratio - 3.98).abs() < 0.3,
            "+12 dB is about x4, got {ratio}"
        );
        let mut far = tone(8000.0, 0.1, 1.0);
        let b2 = rms(settled(&far));
        apply_audio_effects(&mut far, SR, &fx);
        assert!(
            (rms(settled(&far)) / b2 - 1.0).abs() < 0.1,
            "far away it is untouched"
        );
        // low shelf cut takes the bass down and leaves the treble
        let mut e = Effect::new(EffectKind::Eq);
        e.params[0] = -12.0;
        let fx = vec![e];
        let mut lo = tone(40.0, 0.1, 1.0);
        let bl = rms(settled(&lo));
        apply_audio_effects(&mut lo, SR, &fx);
        assert!(rms(settled(&lo)) / bl < 0.4);
        // flat EQ changes nothing
        let mut flat = tone(1000.0, 0.1, 0.2);
        let copy = flat.clone();
        apply_audio_effects(&mut flat, SR, &[Effect::new(EffectKind::Eq)]);
        assert_eq!(flat, copy);
    }

    #[test]
    fn compressor_reduces_loud_parts_by_the_ratio() {
        // 0.5 amplitude sine = -9 dBFS peak; threshold -21, ratio 4 -> ~9 dB of reduction on 12 dB over
        let mut t = tone(300.0, 0.5, 1.5);
        let before = rms(settled(&t));
        apply_audio_effects(
            &mut t,
            SR,
            &with(EffectKind::Compressor, &[-21.0, 4.0, 5.0, 100.0, 0.0]),
        );
        let reduced = 20.0 * (rms(settled(&t)) / before).log10();
        assert!((-10.5..-7.0).contains(&reduced), "{reduced} dB");
        // below the threshold nothing happens; makeup adds gain
        let mut q = tone(300.0, 0.01, 1.0);
        let bq = rms(settled(&q));
        apply_audio_effects(
            &mut q,
            SR,
            &with(EffectKind::Compressor, &[-21.0, 4.0, 5.0, 100.0, 6.0]),
        );
        let up = 20.0 * (rms(settled(&q)) / bq).log10();
        assert!((up - 6.0).abs() < 0.3, "{up}");
    }

    #[test]
    fn limiter_never_passes_its_ceiling() {
        let mut t = tone(220.0, 1.8, 1.0);
        apply_audio_effects(&mut t, SR, &with(EffectKind::Limiter, &[-6.0, 80.0]));
        let ceil = 10f32.powf(-6.0 / 20.0);
        assert!(
            t.iter().all(|s| s.abs() <= ceil + 1e-4),
            "peak {}",
            t.iter().fold(0f32, |m, s| m.max(s.abs()))
        );
        assert!(rms(settled(&t)) > 0.3, "it still carries signal");
    }

    #[test]
    fn de_esser_turns_down_sibilance_but_not_voice() {
        let mut s = tone(6500.0, 0.3, 1.0);
        let bs = rms(settled(&s));
        apply_audio_effects(&mut s, SR, &with(EffectKind::DeEsser, &[-30.0, 100.0]));
        assert!(rms(settled(&s)) / bs < 0.5);
        let mut v = tone(400.0, 0.3, 1.0);
        let bv = rms(settled(&v));
        apply_audio_effects(&mut v, SR, &with(EffectKind::DeEsser, &[-30.0, 100.0]));
        assert!(rms(settled(&v)) / bv > 0.9);
    }

    #[test]
    fn noise_reduction_quietens_hiss_between_words() {
        // deterministic noise; a 440 Hz tone burst in the middle second
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
        };
        let mut b = tone(440.0, 0.4, 3.0);
        for (i, f) in b.chunks_exact_mut(2).enumerate() {
            let burst = (SR as usize..2 * SR as usize).contains(&i);
            for c in f.iter_mut() {
                *c = if burst { *c } else { 0.0 } + noise() * 0.06;
            }
        }
        let quiet_before = rms(&b[10_000..40_000]);
        let burst_before = rms(&b[(SR as usize + 20_000) * 2..(SR as usize + 60_000) * 2]);
        apply_audio_effects(&mut b, SR, &with(EffectKind::NoiseReduction, &[24.0, 60.0]));
        let quiet_after = rms(&b[10_000..40_000]);
        let burst_after = rms(&b[(SR as usize + 20_000) * 2..(SR as usize + 60_000) * 2]);
        assert!(
            quiet_after < quiet_before * 0.35,
            "{quiet_before} -> {quiet_after}"
        );
        assert!(
            burst_after > burst_before * 0.7,
            "the tone survives: {burst_before} -> {burst_after}"
        );
        assert!(b.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn fft_round_trips() {
        let orig: Vec<f32> = (0..256).map(|i| ((i * 7) % 13) as f32 - 6.0).collect();
        let (mut re, mut im) = (orig.clone(), vec![0.0; 256]);
        fft(&mut re, &mut im, false);
        fft(&mut re, &mut im, true);
        for (a, b) in re.iter().zip(&orig) {
            assert!((a - b).abs() < 1e-3);
        }
    }

    #[test]
    fn disabled_and_picture_effects_are_ignored() {
        let mut t = tone(300.0, 0.5, 0.2);
        let copy = t.clone();
        let mut c = Effect::new(EffectKind::Compressor);
        c.enabled = false;
        apply_audio_effects(&mut t, SR, &[c, Effect::new(EffectKind::Blur)]);
        assert_eq!(t, copy);
    }
}
