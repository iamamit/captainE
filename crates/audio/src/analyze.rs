use crate::decode::decode;
use crate::AudioError;

/// Runs of quiet audio in `samples` (mono): `(start_secs, end_secs)` where the level stays below
/// `threshold_db` (dBFS RMS over 20 ms windows) for at least `min_secs`.
pub fn silences_in(
    samples: &[f32],
    sample_rate: u32,
    threshold_db: f32,
    min_secs: f64,
) -> Vec<(f64, f64)> {
    let win = (sample_rate as usize / 50).max(1);
    let threshold = 10f32.powf(threshold_db / 20.0);
    let mut out = Vec::new();
    let mut run_start: Option<usize> = None;
    let windows = samples.len() / win;
    for w in 0..=windows {
        let quiet = if w < windows {
            let chunk = &samples[w * win..(w + 1) * win];
            (chunk.iter().map(|s| s * s).sum::<f32>() / win as f32).sqrt() < threshold
        } else {
            false // close a run that reaches the end
        };
        match (quiet, run_start) {
            (true, None) => run_start = Some(w),
            (false, Some(s)) => {
                let (a, b) = (
                    s as f64 * win as f64 / sample_rate as f64,
                    w as f64 * win as f64 / sample_rate as f64,
                );
                if b - a >= min_secs {
                    out.push((a, b));
                }
                run_start = None;
            }
            _ => {}
        }
    }
    out
}

/// Decode `dur_secs` of a media file from `start_secs` and find its silences. Times are relative to `start_secs`.
pub fn find_silences(
    path: &str,
    start_secs: f64,
    dur_secs: f64,
    threshold_db: f32,
    min_secs: f64,
) -> Result<Vec<(f64, f64)>, AudioError> {
    const RATE: u32 = 8000;
    let stereo = decode(
        path,
        start_secs,
        (dur_secs * RATE as f64).ceil() as usize,
        RATE,
        2,
    )?;
    let mono: Vec<f32> = stereo
        .chunks_exact(2)
        .map(|f| (f[0] + f[1]) * 0.5)
        .collect();
    Ok(silences_in(&mono, RATE, threshold_db, min_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f64, rate: u32, amp: f32) -> Vec<f32> {
        (0..(secs * rate as f64) as usize)
            .map(|i| amp * (i as f32 * 0.3).sin())
            .collect()
    }

    #[test]
    fn finds_gaps_between_speech_like_bursts() {
        let r = 8000;
        let mut s = tone(1.0, r, 0.5);
        s.extend(vec![0.0; r as usize * 2]); // 2 s of silence
        s.extend(tone(0.5, r, 0.5));
        s.extend(vec![0.0005; (r as f64 * 0.2) as usize]); // too short to count
        s.extend(tone(0.5, r, 0.5));
        s.extend(vec![0.0; r as usize]); // silence running to the end
        let found = silences_in(&s, r, -40.0, 0.5);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            (found[0].0 - 1.0).abs() < 0.03 && (found[0].1 - 3.0).abs() < 0.03,
            "{found:?}"
        );
        assert!((found[1].1 - 5.2).abs() < 0.05, "{found:?}");
    }

    #[test]
    fn nothing_quiet_and_nothing_loud() {
        assert!(silences_in(&tone(2.0, 8000, 0.5), 8000, -40.0, 0.3).is_empty());
        assert_eq!(
            silences_in(&vec![0.0; 8000], 8000, -40.0, 0.3),
            vec![(0.0, 1.0)]
        );
        assert!(silences_in(&[], 8000, -40.0, 0.3).is_empty());
    }
}
