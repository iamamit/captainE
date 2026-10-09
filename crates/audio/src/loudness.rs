use ebur128::{EbuR128, Mode};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    /// Integrated loudness (ITU-R BS.1770 / EBU R128), LUFS. `-inf` for silence.
    pub integrated_lufs: f64,
    pub loudness_range_lu: f64,
    /// Highest sample peak across channels, dBFS
    pub sample_peak_db: f64,
    /// Highest true peak across channels, dBTP
    pub true_peak_db: f64,
}

/// Measure interleaved stereo f32 samples.
pub fn measure(samples: &[f32], sample_rate: u32) -> Result<Loudness, String> {
    let mut m = EbuR128::new(
        2,
        sample_rate,
        Mode::I | Mode::LRA | Mode::SAMPLE_PEAK | Mode::TRUE_PEAK,
    )
    .map_err(|e| e.to_string())?;
    m.add_frames_f32(samples).map_err(|e| e.to_string())?;
    let db = |lin: f64| {
        if lin <= 0.0 {
            f64::NEG_INFINITY
        } else {
            20.0 * lin.log10()
        }
    };
    let peak = |f: &dyn Fn(u32) -> Result<f64, ebur128::Error>| {
        (0..2).filter_map(|c| f(c).ok()).fold(0.0f64, f64::max)
    };
    Ok(Loudness {
        integrated_lufs: m.loudness_global().map_err(|e| e.to_string())?,
        loudness_range_lu: m.loudness_range().map_err(|e| e.to_string())?,
        sample_peak_db: db(peak(&|c| m.sample_peak(c))),
        true_peak_db: db(peak(&|c| m.true_peak(c))),
    })
}
