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
    decode_filtered(path, start_secs, count, sample_rate, channels, 1.0, false)
}

/// ffmpeg `atempo` only takes 0.5..=2 per stage, so a speed is split into a chain of them.
fn tempo_chain(mut speed: f64) -> String {
    let mut parts = Vec::new();
    while speed > 2.0 {
        parts.push("atempo=2".to_string());
        speed /= 2.0;
    }
    while speed < 0.5 {
        parts.push("atempo=0.5".to_string());
        speed *= 2.0;
    }
    parts.push(format!("atempo={speed:.6}"));
    parts.join(",")
}

/// Like [`decode`], but the source is played at `speed` (pitch kept) and optionally backwards.
/// `start_secs` is where the source stretch to read begins; `count` output frames come back, padded with
/// silence if the file ends early. When reversed, the stretch is read forwards and flipped.
pub(crate) fn decode_filtered(
    path: &str,
    start_secs: f64,
    count: usize,
    sample_rate: u32,
    channels: u32,
    speed: f64,
    reverse: bool,
) -> Result<Vec<f32>, AudioError> {
    if !Path::new(path).exists() {
        return Err(AudioError::Decode(format!("media file not found: {path}")));
    }
    let secs = count as f64 * speed / sample_rate as f64;
    let mut filters = Vec::new();
    if reverse {
        filters.push("areverse".to_string());
    }
    if speed != 1.0 {
        filters.push(tempo_chain(speed));
    }
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-ss",
            &format!("{:.6}", start_secs.max(0.0)),
            "-t",
            &format!("{:.6}", secs + if reverse { 0.0 } else { 0.01 }),
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
        .args(if filters.is_empty() {
            vec![]
        } else {
            vec!["-af".to_string(), filters.join(",")]
        })
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

/// `count` mono samples at `sample_rate` starting `start_secs` into the file.
pub fn decode_mono_window(
    path: &str,
    start_secs: f64,
    count: usize,
    sample_rate: u32,
) -> Result<Vec<f32>, AudioError> {
    decode(path, start_secs, count, sample_rate, 1)
}
