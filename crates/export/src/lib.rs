//! Export a timeline to a video file by building an FFmpeg filter graph.
//!
//! This is the Phase 1 stand-in for the GPU renderer: it handles the first video track only
//! (clips and gaps in order, audio taken from the same clips), which is enough to cut and ship.

use std::fmt;
use std::path::Path;
use std::process::Command;

use captaine_model::{Clip, Id, Item, Project, TrackKind};
use captaine_time::{Rational, RationalTime, Rounding};

#[derive(Debug)]
pub enum ExportError {
    NoTimeline(Id),
    Empty,
    MissingMedia(Id),
    Time(&'static str),
    Io(std::io::Error),
    Ffmpeg(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoTimeline(id) => write!(f, "timeline {id} not found"),
            ExportError::Empty => write!(f, "nothing to export: the first video track is empty"),
            ExportError::MissingMedia(id) => write!(f, "media {id} not found"),
            ExportError::Time(w) => write!(f, "time conversion failed: {w}"),
            ExportError::Io(e) => write!(f, "cannot run ffmpeg (is FFmpeg installed?): {e}"),
            ExportError::Ffmpeg(s) => write!(f, "ffmpeg failed:\n{s}"),
        }
    }
}
impl std::error::Error for ExportError {}

fn secs(t: RationalTime) -> f64 {
    t.to_seconds_f64()
}

/// The ffmpeg arguments (after the program name) that render `timeline` to `out`.
pub fn plan(project: &Project, timeline: Id, out: &Path) -> Result<Vec<String>, ExportError> {
    let tl = project
        .timeline(timeline)
        .ok_or(ExportError::NoTimeline(timeline))?;
    let track = tl
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .ok_or(ExportError::Empty)?;
    if track.items.is_empty() {
        return Err(ExportError::Empty);
    }
    let (w, h) = (project.settings.width, project.settings.height);
    let fps = tl.rate;
    let sr = project.settings.sample_rate;

    let mut inputs: Vec<&str> = Vec::new();
    let mut graph: Vec<String> = Vec::new();
    let mut concat = String::new();

    for (n, item) in track.items.iter().enumerate() {
        let frames = item.duration().value;
        let secs_out = secs(item.duration());
        let live: Option<(&Clip, usize)> = match item {
            Item::Clip(c) if c.enabled => {
                let m = project
                    .media
                    .get(&c.media)
                    .ok_or(ExportError::MissingMedia(c.media))?;
                let idx = inputs.iter().position(|p| *p == m.path).unwrap_or_else(|| {
                    inputs.push(&m.path);
                    inputs.len() - 1
                });
                Some((c, idx))
            }
            _ => None,
        };
        match live {
            Some((c, i)) => {
                let m = &project.media[&c.media];
                let src_rate = c.source_start.rate;
                let src_len = c
                    .duration
                    .rescale(src_rate, Rounding::Ceil)
                    .ok_or(ExportError::Time("clip length"))?;
                let start = c.source_start.value;
                graph.push(format!(
                    "[{i}:v]trim=start_frame={start}:end_frame={end},setpts=PTS-STARTPTS,fps={fps},trim=end_frame={frames},\
                     setpts=PTS-STARTPTS,scale={w}:{h}:force_original_aspect_ratio=decrease,\
                     pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p[v{n}]",
                    end = start + src_len.value,
                    fps = rate_arg(fps),
                ));
                if m.has_audio {
                    graph.push(format!(
                        "[{i}:a]atrim=start={:.6}:duration={:.6},asetpts=PTS-STARTPTS,aresample={sr},\
                         aformat=sample_fmts=fltp:channel_layouts=stereo,apad,atrim=duration={:.6}[a{n}]",
                        secs(c.source_start),
                        secs_out,
                        secs_out,
                    ));
                } else {
                    graph.push(silence(n, sr, secs_out));
                }
            }
            None => {
                graph.push(format!(
                    "color=c=black:s={w}x{h}:r={}:d={secs_out:.6},format=yuv420p,setsar=1[v{n}]",
                    rate_arg(fps)
                ));
                graph.push(silence(n, sr, secs_out));
            }
        }
        concat.push_str(&format!("[v{n}][a{n}]"));
    }
    graph.push(format!(
        "{concat}concat=n={}:v=1:a=1[v][a]",
        track.items.len()
    ));

    let mut args: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error"]
        .map(String::from)
        .to_vec();
    for p in &inputs {
        args.push("-i".into());
        args.push((*p).to_string());
    }
    args.extend(["-filter_complex".into(), graph.join(";")]);
    for a in [
        "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "medium", "-crf", "18",
        "-pix_fmt", "yuv420p",
    ] {
        args.push(a.into());
    }
    for a in ["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"] {
        args.push(a.into());
    }
    args.push(out.to_string_lossy().into_owned());
    Ok(args)
}

fn rate_arg(r: Rational) -> String {
    format!("{}/{}", r.num(), r.den())
}

fn silence(n: usize, sr: u32, secs: f64) -> String {
    format!("anullsrc=r={sr}:cl=stereo,atrim=duration={secs:.6},asetpts=PTS-STARTPTS[a{n}]")
}

/// Renders `timeline` to `out` with the `ffmpeg` binary.
pub fn export(project: &Project, timeline: Id, out: &Path) -> Result<(), ExportError> {
    let args = plan(project, timeline, out)?;
    let res = Command::new("ffmpeg")
        .args(&args)
        .output()
        .map_err(ExportError::Io)?;
    if res.status.success() {
        Ok(())
    } else {
        Err(ExportError::Ffmpeg(
            String::from_utf8_lossy(&res.stderr).trim().to_string(),
        ))
    }
}
