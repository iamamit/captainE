use crate::decode::decode;
use crate::AudioError;

/// Smallest and largest sample in one display bucket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub min: f32,
    pub max: f32,
}

/// Reduce samples to one [`Peak`] per `bucket` samples (the last bucket may be shorter).
pub fn peaks(samples: &[f32], bucket: usize) -> Vec<Peak> {
    assert!(bucket > 0);
    samples
        .chunks(bucket)
        .map(|c| {
            c.iter().fold(
                Peak {
                    min: f32::MAX,
                    max: f32::MIN,
                },
                |p, &s| Peak {
                    min: p.min.min(s),
                    max: p.max.max(s),
                },
            )
        })
        .collect()
}

/// Waveform overview of a media file: `per_second` peaks for each second, from a low-rate mono decode.
pub fn media_peaks(
    path: &str,
    duration_secs: f64,
    per_second: u32,
) -> Result<Vec<Peak>, AudioError> {
    const RATE: u32 = 8000;
    let count = (duration_secs * RATE as f64).ceil() as usize;
    // decode stereo and average: ffmpeg's own mono downmix would sum the channels (+3 dB for identical ones)
    let stereo = decode(path, 0.0, count, RATE, 2)?;
    let mono: Vec<f32> = stereo
        .chunks_exact(2)
        .map(|f| (f[0] + f[1]) * 0.5)
        .collect();
    Ok(peaks(&mono, (RATE / per_second.max(1)).max(1) as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_min_and_max() {
        let p = peaks(&[0.0, 0.5, -0.25, 0.1, 0.9], 2);
        assert_eq!(
            p,
            [
                Peak { min: 0.0, max: 0.5 },
                Peak {
                    min: -0.25,
                    max: 0.1
                },
                Peak { min: 0.9, max: 0.9 }
            ]
        );
    }
}
