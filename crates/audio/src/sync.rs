//! Lining up recordings of the same event by their sound: the cross-correlation of their loudness shapes.

use crate::decode::decode;
use crate::dsp::fft;
use crate::AudioError;

/// How two recordings line up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alignment {
    /// Seconds to move the second recording later (negative: earlier) so it lines up with the first
    pub offset_secs: f64,
    /// How clear the match is (peak height in standard deviations of the rest): above 4 is trustworthy
    pub confidence: f64,
}

const RATE: u32 = 8000;
/// Loudness is compared at 200 values a second.
const HOP: usize = 40;
const ENV_RATE: f64 = RATE as f64 / HOP as f64;

/// Loudness shape (one number per 5 ms), zero mean and unit spread.
fn envelope(path: &str, from: f64, secs: f64) -> Result<Vec<f32>, AudioError> {
    let n = (secs * RATE as f64) as usize;
    let pcm = decode(path, from, n, RATE, 1)?;
    let mut env: Vec<f32> = pcm
        .chunks(HOP)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    // squash the dynamics so a few loud peaks do not dominate
    for v in &mut env {
        *v = (1.0 + *v * 40.0).ln();
    }
    let mean = env.iter().sum::<f32>() / env.len().max(1) as f32;
    for v in &mut env {
        *v -= mean;
    }
    let sd = (env.iter().map(|v| v * v).sum::<f32>() / env.len().max(1) as f32)
        .sqrt()
        .max(1e-6);
    for v in &mut env {
        *v /= sd;
    }
    Ok(env)
}

/// Find how recording `b` (starting `b_from` seconds into its file) lines up with recording `a` (from
/// `a_from`), comparing `window` seconds of each and searching up to `max_shift` seconds either way.
pub fn find_offset(
    a: &str,
    a_from: f64,
    b: &str,
    b_from: f64,
    window: f64,
    max_shift: f64,
) -> Result<Alignment, AudioError> {
    let (ea, eb) = (envelope(a, a_from, window)?, envelope(b, b_from, window)?);
    let n = (ea.len() + eb.len()).next_power_of_two();
    let load = |e: &[f32]| {
        let mut re = vec![0.0f32; n];
        re[..e.len()].copy_from_slice(e);
        let mut im = vec![0.0f32; n];
        fft(&mut re, &mut im, false);
        (re, im)
    };
    let ((ar, ai), (br, bi)) = (load(&ea), load(&eb));
    // corr[k] = sum_n a[n] * b[n + k]  ->  conj(A) * B, then back to time
    let mut cr = vec![0.0f32; n];
    let mut ci = vec![0.0f32; n];
    for i in 0..n {
        cr[i] = ar[i] * br[i] + ai[i] * bi[i];
        ci[i] = ar[i] * bi[i] - ai[i] * br[i];
    }
    fft(&mut cr, &mut ci, true);
    let max_lag = ((max_shift * ENV_RATE) as usize).min(n / 2 - 1);
    let at = |k: i64| cr[(k.rem_euclid(n as i64)) as usize] as f64;
    let (mut best, mut best_k) = (f64::MIN, 0i64);
    let mut sum = 0.0;
    let mut sum2 = 0.0;
    let mut count = 0.0;
    for k in -(max_lag as i64)..=(max_lag as i64) {
        let v = at(k);
        sum += v;
        sum2 += v * v;
        count += 1.0;
        if v > best {
            best = v;
            best_k = k;
        }
    }
    let mean = sum / count;
    let sd = (sum2 / count - mean * mean).max(1e-12).sqrt();
    // parabola through the peak for a finer answer than 5 ms
    let (l, c, r) = (at(best_k - 1), at(best_k), at(best_k + 1));
    let denom = l - 2.0 * c + r;
    let frac = if denom.abs() > 1e-9 {
        0.5 * (l - r) / denom
    } else {
        0.0
    };
    // b's events show up `k` envelope samples later than a's: b has to move earlier by that much
    let delta = (best_k as f64 + frac) / ENV_RATE;
    Ok(Alignment {
        offset_secs: -delta,
        confidence: (best - mean) / sd,
    })
}
