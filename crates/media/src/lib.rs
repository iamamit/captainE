//! Media probing. Uses the `ffprobe` binary for now; a libav binding replaces it in the engine phase.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::process::Command;

use captaine_model::{Id, InputSpace, MediaRef};
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
    /// Frame rate for video; the sample rate for audio-only media.
    pub rate: Rational,
    /// Length in ticks of `rate`.
    pub length: i64,
    /// Display size with rotation applied; 0 for audio-only.
    pub width: u32,
    pub height: u32,
    pub has_video: bool,
    pub has_audio: bool,
}

fn parse_rate(s: &str) -> Option<Rational> {
    let (n, d) = s.split_once('/')?;
    Rational::new(n.trim().parse().ok()?, d.trim().parse().ok()?)
}

/// Clockwise rotation (degrees, 0/90/180/270) that players apply to a video stream.
fn rotation(video: &serde_json::Value) -> i64 {
    let from_side = video["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()));
    let from_tag = video["tags"]["rotate"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok());
    let deg = from_side.or(from_tag).unwrap_or(0.0).round() as i64;
    deg.rem_euclid(360)
}

/// Parses `ffprobe -print_format json -show_streams -show_format` output.
pub fn parse_probe(json: &str) -> Result<ProbeInfo, MediaError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| MediaError::Parse(e.to_string()))?;
    let streams = v["streams"]
        .as_array()
        .ok_or_else(|| MediaError::Parse("no streams".into()))?;
    let secs = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok());
    // cover art in an mp3 shows up as a video stream: it is not video
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1);
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");

    if let Some(video) = video {
        let rate = ["avg_frame_rate", "r_frame_rate"]
            .iter()
            .find_map(|k| video[*k].as_str().and_then(parse_rate))
            .ok_or_else(|| MediaError::Parse("no frame rate".into()))?;
        let duration = secs(&video["duration"])
            .or_else(|| secs(&v["format"]["duration"]))
            .ok_or_else(|| MediaError::Parse("no duration (still image?)".into()))?;
        let (w, h) = (
            video["width"].as_u64().unwrap_or(0) as u32,
            video["height"].as_u64().unwrap_or(0) as u32,
        );
        let (width, height) = if rotation(video) % 180 == 90 {
            (h, w)
        } else {
            (w, h)
        };
        return Ok(ProbeInfo {
            rate,
            length: (duration * rate.as_f64()).round() as i64,
            width,
            height,
            has_video: true,
            has_audio: audio.is_some(),
        });
    }
    let audio = audio.ok_or_else(|| MediaError::Parse("no video or audio stream".into()))?;
    let sr: i64 = audio["sample_rate"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| MediaError::Parse("no sample rate".into()))?;
    let duration = secs(&audio["duration"])
        .or_else(|| secs(&v["format"]["duration"]))
        .ok_or_else(|| MediaError::Parse("no duration".into()))?;
    Ok(ProbeInfo {
        rate: Rational::new(sr, 1).ok_or_else(|| MediaError::Parse("bad sample rate".into()))?,
        length: (duration * sr as f64).round() as i64,
        width: 0,
        height: 0,
        has_video: false,
        has_audio: true,
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
        duration: Some(RationalTime::new(info.length, info.rate)),
        has_audio: info.has_audio,
        width: info.width,
        height: info.height,
        has_video: info.has_video,
        proxy: None,
    })
}

/// The rest of an FFmpeg filter chain (starting with a comma) that turns footage in `space` into Rec.709
/// SDR: HDR is tone mapped, wide-gamut SDR is squeezed into the Rec.709 primaries. Empty for Rec.709.
pub fn to_rec709(space: InputSpace) -> String {
    // without zscale in this FFmpeg the footage is used as it is
    if !ffmpeg_has_filter("zscale") {
        return String::new();
    }
    let (tin, pin) = match space {
        InputSpace::Auto | InputSpace::Rec709 => return String::new(),
        InputSpace::Rec2020 => ("bt709", "bt2020"),
        InputSpace::Hlg => ("arib-std-b67", "bt2020"),
        InputSpace::Pq => ("smpte2084", "bt2020"),
    };
    // HDR reference white (203 nits) lands on SDR white; the highlights above it are rolled off
    let (tone, npl) = if matches!(space, InputSpace::Hlg | InputSpace::Pq) {
        (",tonemap=tonemap=hable:desat=0", 203)
    } else {
        ("", 100)
    };
    format!(
        ",zscale=tin={tin}:pin={pin}:min=bt2020nc:t=linear:npl={npl},format=gbrpf32le,zscale=p=bt709{tone},zscale=t=bt709:m=bt709:r=tv,format=yuv420p"
    )
}

/// What a file says about its colours, asked of ffprobe once per file.
pub fn detect_space(path: &str) -> InputSpace {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, InputSpace>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().ok().and_then(|c| c.get(path).copied()) {
        return s;
    }
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=color_primaries,color_transfer",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
        .ok();
    let text = out
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let space = InputSpace::from_tags(&get("color_primaries="), &get("color_transfer="));
    if let Ok(mut c) = cache.lock() {
        c.insert(path.to_string(), space);
    }
    space
}

/// A file path written the way an FFmpeg filter option wants it: forward slashes, and the drive colon escaped
/// twice, once for the filter graph and once for the option (`C:\Users\me\a.trf` becomes `C\\:/Users/me/a.trf`).
pub fn filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\\\:")
}

#[cfg(test)]
mod filter_path_tests {
    use super::*;

    #[test]
    fn a_windows_path_becomes_one_ffmpeg_accepts() {
        let p = filter_path(Path::new("C:\\Users\\me\\a b.trf"));
        assert_eq!(p, "C\\\\:/Users/me/a b.trf");
        assert_eq!(filter_path(Path::new("/tmp/x.trf")), "/tmp/x.trf");
    }
}

/// Does this FFmpeg have the named filter? Asked once; some builds leave out `zscale` (needs libzimg) or
/// `vidstab`.
pub fn ffmpeg_has_filter(name: &str) -> bool {
    static FILTERS: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let list = FILTERS.get_or_init(|| {
        Command::new("ffmpeg")
            .args(["-hide_banner", "-filters"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    });
    list.lines()
        .any(|l| l.split_whitespace().nth(1) == Some(name))
}

/// Where a media file's generated helpers (proxy, thumbnails) live: `<dir>/<id>.<what>`.
pub fn cache_path(cache_dir: &Path, id: Id, what: &str) -> std::path::PathBuf {
    cache_dir.join(format!("{}.{what}", id.to_full_string()))
}

/// Transcode `src` to an edit-friendly proxy: every frame intra-coded (cheap, exact seeking), at most
/// `max_height` tall, no audio (the mixer reads the original). Returns when the file is complete.
pub fn make_proxy(src: &Path, dst: &Path, max_height: u32) -> Result<(), MediaError> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(MediaError::Io)?;
    }
    let tmp = dst.with_extension("partial.mp4");
    // HDR and wide-gamut footage becomes ordinary Rec.709 in the proxy, so the picture matches the original's
    let vf = format!(
        "scale=-2:'min({max_height},ih)'{}",
        to_rec709(detect_space(&src.to_string_lossy()))
    );
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-nostdin", "-i"])
        .arg(src)
        .args([
            "-an", "-vf", &vf, "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-g", "1",
            "-pix_fmt", "yuv420p",
        ])
        .arg(&tmp)
        .output()
        .map_err(MediaError::Io)?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(MediaError::Tool(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    std::fs::rename(&tmp, dst).map_err(MediaError::Io)
}

/// Times (seconds from `from_secs`) where the picture changes abruptly: a cut. `threshold` is FFmpeg's
/// scene score from 0 to 1; 0.3 catches hard cuts without reacting to camera movement.
pub fn detect_scenes(
    path: &str,
    from_secs: f64,
    dur_secs: f64,
    threshold: f64,
) -> Result<Vec<f64>, MediaError> {
    let out = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostdin", "-ss"])
        .arg(format!("{:.6}", from_secs.max(0.0)))
        .arg("-t")
        .arg(format!("{:.6}", dur_secs.max(0.0)))
        .arg("-i")
        .arg(path)
        .args(["-an", "-sn", "-vf"])
        .arg(format!("select='gt(scene,{threshold})',showinfo"))
        .args(["-f", "null", "-"])
        .output()
        .map_err(MediaError::Io)?;
    if !out.status.success() {
        return Err(MediaError::Tool(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
                .to_string(),
        ));
    }
    let log = String::from_utf8_lossy(&out.stderr);
    let mut times: Vec<f64> = log
        .lines()
        .filter(|l| l.contains("Parsed_showinfo"))
        .filter_map(|l| l.split("pts_time:").nth(1))
        .filter_map(|r| r.split_whitespace().next())
        .filter_map(|t| t.parse().ok())
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    Ok(times)
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
        assert_eq!(p.rate, Rational::FPS_29_97);
        assert_eq!((p.length, p.width, p.height), (300, 1920, 1080));
        assert!(p.has_audio && p.has_video);
    }

    #[test]
    fn falls_back_to_format_duration_and_r_frame_rate() {
        let j = r#"{"streams":[{"codec_type":"video","width":4,"height":4,"avg_frame_rate":"0/0","r_frame_rate":"24/1"}],
            "format":{"duration":"2.0"}}"#;
        let p = parse_probe(j).unwrap();
        assert_eq!(
            (p.rate, p.length, p.has_audio),
            (Rational::FPS_24, 48, false)
        );
    }

    #[test]
    fn phone_video_rotation_swaps_the_size() {
        let side = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","duration":"1",
            "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}],"format":{}}"#;
        let p = parse_probe(side).unwrap();
        assert_eq!((p.width, p.height), (1080, 1920));
        let tag = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","duration":"1",
            "tags":{"rotate":"90"}}],"format":{}}"#;
        assert_eq!(parse_probe(tag).unwrap().width, 1080);
        let flip = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","duration":"1",
            "side_data_list":[{"rotation":180}]}],"format":{}}"#;
        assert_eq!(parse_probe(flip).unwrap().width, 1920);
    }

    #[test]
    fn audio_only_is_measured_in_samples() {
        let j = r#"{"streams":[{"codec_type":"audio","sample_rate":"44100","duration":"2.5"}],"format":{}}"#;
        let p = parse_probe(j).unwrap();
        assert_eq!((p.has_video, p.has_audio, p.length), (false, true, 110_250));
        assert_eq!(p.rate, Rational::new(44100, 1).unwrap());
    }

    #[test]
    fn cover_art_is_not_video() {
        let j = r#"{"streams":[{"codec_type":"audio","sample_rate":"48000","duration":"1"},
            {"codec_type":"video","width":500,"height":500,"avg_frame_rate":"90000/1","disposition":{"attached_pic":1}}],"format":{}}"#;
        assert!(!parse_probe(j).unwrap().has_video);
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_probe(r#"{"streams":[],"format":{}}"#).is_err());
    }
}
