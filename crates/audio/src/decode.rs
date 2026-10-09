use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
pub enum AudioError {
    NoTimeline,
    Decode(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::NoTimeline => write!(f, "timeline not found"),
            AudioError::Decode(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for AudioError {}

/// Decode `count` stereo frames starting `start_secs` into the file, as interleaved f32 at `sample_rate`.
/// Short files are padded with silence so the result is always exactly `count * 2` samples.
pub fn decode_pcm(
    path: &str,
    start_secs: f64,
    count: usize,
    sample_rate: u32,
) -> Result<Vec<f32>, AudioError> {
    decode(path, start_secs, count, sample_rate, 2)
}

pub(crate) fn decode(
    path: &str,
    start_secs: f64,
    count: usize,
    sample_rate: u32,
    channels: u32,
) -> Result<Vec<f32>, AudioError> {
    if !Path::new(path).exists() {
        return Err(AudioError::Decode(format!("media file not found: {path}")));
    }
    let secs = count as f64 / sample_rate as f64;
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-ss",
            &format!("{:.6}", start_secs.max(0.0)),
            "-t",
            &format!("{:.6}", secs + 0.01),
        ])
        .args([
            "-i",
            path,
            "-vn",
            "-sn",
            "-ac",
            &channels.to_string(),
            "-ar",
            &sample_rate.to_string(),
        ])
        .args(["-f", "f32le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            AudioError::Decode(format!("cannot run ffmpeg (is FFmpeg installed?): {e}"))
        })?;
    let want = count * channels as usize;
    let mut bytes = Vec::with_capacity(want * 4);
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_end(&mut bytes)
        .map_err(|e| AudioError::Decode(e.to_string()))?;
    let _ = child.wait();
    let mut out: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    out.resize(want, 0.0);
    Ok(out)
}
