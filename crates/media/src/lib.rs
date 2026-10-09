//! Media probing. Uses the `ffprobe` binary for now; a libav binding replaces it in the engine phase.

use std::fmt;
use std::path::Path;
use std::process::Command;

use captaine_model::{Id, MediaRef};
use captaine_time::{Rational, RationalTime};

#[derive(Debug)]
pub enum MediaError {
    Io(std::io::Error),
    /// ffprobe ran but failed
    Tool(String),
    Parse(String),
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaError::Io(e) => write!(f, "cannot run ffprobe (is FFmpeg installed?): {e}"),
            MediaError::Tool(s) => write!(f, "ffprobe failed: {s}"),
            MediaError::Parse(s) => write!(f, "unexpected ffprobe output: {s}"),
        }
    }
}
impl std::error::Error for MediaError {}

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeInfo {
    pub fps: Rational,
    pub frames: i64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}

fn parse_rate(s: &str) -> Option<Rational> {
    let (n, d) = s.split_once('/')?;
    Rational::new(n.trim().parse().ok()?, d.trim().parse().ok()?)
}

/// Parses `ffprobe -print_format json -show_streams -show_format` output.
pub fn parse_probe(json: &str) -> Result<ProbeInfo, MediaError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| MediaError::Parse(e.to_string()))?;
    let streams = v["streams"]
        .as_array()
        .ok_or_else(|| MediaError::Parse("no streams".into()))?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or_else(|| MediaError::Parse("no video stream".into()))?;
    let rate = ["avg_frame_rate", "r_frame_rate"]
        .iter()
        .find_map(|k| video[*k].as_str().and_then(parse_rate))
        .ok_or_else(|| MediaError::Parse("no frame rate".into()))?;
    let secs = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok());
    let duration = secs(&video["duration"])
        .or_else(|| secs(&v["format"]["duration"]))
        .ok_or_else(|| MediaError::Parse("no duration (still image?)".into()))?;
    Ok(ProbeInfo {
        fps: rate,
        frames: (duration * rate.as_f64()).round() as i64,
        width: video["width"].as_u64().unwrap_or(0) as u32,
        height: video["height"].as_u64().unwrap_or(0) as u32,
        has_audio: streams.iter().any(|s| s["codec_type"] == "audio"),
    })
}

pub fn probe(path: &Path) -> Result<ProbeInfo, MediaError> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .output()
        .map_err(MediaError::Io)?;
    if !out.status.success() {
        return Err(MediaError::Tool(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    parse_probe(&String::from_utf8_lossy(&out.stdout))
}

/// Probes `path` and builds its media-pool entry.
pub fn media_ref(path: &Path, id: Id) -> Result<MediaRef, MediaError> {
    let info = probe(path)?;
    let path = path.canonicalize().map_err(MediaError::Io)?;
    Ok(MediaRef {
        id,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_string_lossy().into_owned(),
        duration: Some(RationalTime::new(info.frames, info.fps)),
        has_audio: info.has_audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ntsc_clip_with_audio() {
        let j = r#"{"streams":[
            {"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30000/1001","duration":"10.010000"},
            {"codec_type":"audio"}],"format":{"duration":"10.01"}}"#;
        let p = parse_probe(j).unwrap();
        assert_eq!(p.fps, Rational::FPS_29_97);
        assert_eq!(p.frames, 300);
        assert!(p.has_audio);
    }

    #[test]
    fn falls_back_to_format_duration_and_r_frame_rate() {
        let j = r#"{"streams":[{"codec_type":"video","width":4,"height":4,"avg_frame_rate":"0/0","r_frame_rate":"24/1"}],
            "format":{"duration":"2.0"}}"#;
        let p = parse_probe(j).unwrap();
        assert_eq!(
            (p.fps, p.frames, p.has_audio),
            (Rational::FPS_24, 48, false)
        );
    }

    #[test]
    fn rejects_audio_only() {
        assert!(parse_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{}}"#).is_err());
    }
}
