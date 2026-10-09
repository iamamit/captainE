//! Export (docs/04): frames come from the engine's compositor and audio from its mixer, and
//! FFmpeg only encodes. The same project therefore renders the same way on every run.

use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use captaine_model::{Id, Project};
use captaine_render::{RenderOptions, Renderer};
use captaine_time::Rational;

#[derive(Debug)]
pub enum ExportError {
    NoTimeline(Id),
    Empty,
    Render(String),
    Audio(String),
    Io(std::io::Error),
    Ffmpeg(String),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoTimeline(id) => write!(f, "timeline {id} not found"),
            ExportError::Empty => write!(f, "nothing to export: the timeline is empty"),
            ExportError::Render(s) | ExportError::Audio(s) => write!(f, "{s}"),
            ExportError::Io(e) => write!(f, "i/o error: {e}"),
            ExportError::Ffmpeg(s) => write!(f, "ffmpeg failed:\n{s}"),
        }
    }
}
impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// x264 constant rate factor (lower = better, 18 is visually lossless)
    pub crf: u32,
    pub preset: String,
    pub audio_kbps: u32,
    pub render: RenderOptions,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            crf: 18,
            preset: "medium".into(),
            audio_kbps: 192,
            render: RenderOptions::default(),
        }
    }
}

fn rate_arg(r: Rational) -> String {
    format!("{}/{}", r.num(), r.den())
}

/// FFmpeg arguments that encode raw RGB frames from stdin plus a raw stereo f32 audio file.
pub fn encode_args(
    width: u32,
    height: u32,
    fps: Rational,
    sample_rate: u32,
    audio: &Path,
    out: &Path,
    o: &ExportOptions,
) -> Vec<String> {
    let mut a: Vec<String> = ["-y", "-hide_banner", "-loglevel", "error"]
        .map(String::from)
        .to_vec();
    a.extend(["-f", "rawvideo", "-pix_fmt", "rgb24", "-s"].map(String::from));
    a.push(format!("{width}x{height}"));
    a.extend([
        "-r".to_string(),
        rate_arg(fps),
        "-i".to_string(),
        "-".to_string(),
    ]);
    a.extend(["-f", "f32le", "-ar"].map(String::from));
    a.extend([
        sample_rate.to_string(),
        "-ac".into(),
        "2".into(),
        "-i".into(),
        audio.to_string_lossy().into_owned(),
    ]);
    // RGB -> limited-range BT.709 YUV, tagged as such so players decode it the way we composited it
    a.extend(
        [
            "-vf",
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
        ]
        .map(String::from),
    );
    a.extend(
        [
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ]
        .map(String::from),
    );
    a.extend(["-c:v", "libx264", "-preset"].map(String::from));
    a.extend([o.preset.clone(), "-crf".into(), o.crf.to_string()]);
    a.extend(["-c:a", "aac", "-b:a"].map(String::from));
    a.push(format!("{}k", o.audio_kbps));
    a.extend(["-movflags", "+faststart"].map(String::from));
    a.push(out.to_string_lossy().into_owned());
    a
}

fn temp_audio_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("captaine-audio-{}-{nanos}.f32", std::process::id()))
}

/// Render `timeline` to `out`. `progress(done, total)` is called after every frame.
pub fn export(
    project: &Project,
    timeline: Id,
    out: &Path,
    opts: &ExportOptions,
    progress: &mut dyn FnMut(i64, i64),
) -> Result<(), ExportError> {
    let tl = project
        .timeline(timeline)
        .ok_or(ExportError::NoTimeline(timeline))?;
    let total = tl.duration().value;
    if total == 0 {
        return Err(ExportError::Empty);
    }
    let sr = project.settings.sample_rate;

    // 1. mix the whole soundtrack to a raw file, a second at a time
    let audio_path = temp_audio_path();
    let result = (|| -> Result<(), ExportError> {
        let mut mixer = captaine_audio::Mixer::new(project, timeline)
            .map_err(|e| ExportError::Audio(e.to_string()))?;
        {
            let mut w = BufWriter::new(File::create(&audio_path)?);
            let n = mixer.total_samples();
            let mut at = 0;
            while at < n {
                let block = (sr as i64).min(n - at) as usize;
                let pcm = mixer
                    .render(at, block)
                    .map_err(|e| ExportError::Audio(e.to_string()))?;
                for s in pcm {
                    w.write_all(&s.clamp(-1.0, 1.0).to_le_bytes())?;
                }
                at += block as i64;
            }
            w.flush()?;
        }

        // 2. render frames into the encoder
        let mut renderer = Renderer::new(project, timeline, opts.render.clone())
            .map_err(|e| ExportError::Render(e.to_string()))?;
        let (w, h) = renderer.size();
        let args = encode_args(w, h, tl.rate, sr, &audio_path, out, opts);
        let mut child = Command::new("ffmpeg")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ExportError::Io)?;
        let mut stdin = child.stdin.take().expect("piped");
        let mut write_err = None;
        for tick in 0..total {
            let frame = match renderer.render(tick) {
                Ok(f) => f,
                Err(e) => {
                    drop(stdin);
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ExportError::Render(e.to_string()));
                }
            };
            if let Err(e) = stdin.write_all(&frame.to_rgb8()) {
                write_err = Some(e); // ffmpeg exited early: its stderr says why
                break;
            }
            progress(tick + 1, total);
        }
        drop(stdin);
        let res = child.wait_with_output()?;
        if !res.status.success() {
            let msg = String::from_utf8_lossy(&res.stderr).trim().to_string();
            return Err(ExportError::Ffmpeg(if msg.is_empty() {
                format!("{write_err:?}")
            } else {
                msg
            }));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&audio_path);
    result
}
