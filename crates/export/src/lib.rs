//! Export (docs/04): frames come from the engine's compositor and audio from its mixer, and
//! FFmpeg only encodes. The same project therefore renders the same way on every run.

use std::fmt;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use captaine_model::{Id, Project};
use captaine_render::{RenderOptions, Renderer};
use captaine_time::{Rational, RationalTime, Rounding};

/// Delete a half-written file. Windows keeps a killed program's file locked for a moment, so try a few times.
fn remove_when_free(path: &Path) {
    for _ in 0..40 {
        match fs::remove_file(path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

#[derive(Debug)]
pub enum ExportError {
    NoTimeline(Id),
    Empty,
    Render(String),
    Audio(String),
    Io(std::io::Error),
    Ffmpeg(String),
    Cancelled,
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::NoTimeline(id) => write!(f, "timeline {id} not found"),
            ExportError::Empty => write!(f, "nothing to export: the timeline is empty"),
            ExportError::Render(s) | ExportError::Audio(s) => write!(f, "{s}"),
            ExportError::Io(e) => write!(f, "i/o error: {e}"),
            ExportError::Ffmpeg(s) => write!(f, "ffmpeg failed:\n{s}"),
            ExportError::Cancelled => write!(f, "export cancelled"),
        }
    }
}
impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

/// Output format. ProRes goes in a `.mov`; most others fit `.mp4`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    /// H.264 (x264) + AAC
    H264,
    /// HEVC (x265, tagged `hvc1` so Apple players accept it) + AAC
    Hevc,
    /// ProRes 422 HQ + 16-bit PCM audio
    ProRes,
    /// H.264 on the Mac's hardware encoder: several times faster, a little larger
    H264Hw,
    /// HEVC on the hardware encoder
    HevcHw,
    /// Looping animated GIF, no sound
    Gif,
    /// A folder of numbered PNG pictures, no sound
    PngSequence,
    /// 16-bit WAV of the mix
    Wav,
    /// AAC (.m4a) of the mix
    Aac,
}

impl Codec {
    pub const ALL: [Codec; 9] = [
        Codec::H264,
        Codec::Hevc,
        Codec::ProRes,
        Codec::H264Hw,
        Codec::HevcHw,
        Codec::Gif,
        Codec::PngSequence,
        Codec::Wav,
        Codec::Aac,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC",
            Codec::ProRes => "ProRes 422 HQ",
            Codec::H264Hw => "H.264 (hardware)",
            Codec::HevcHw => "HEVC (hardware)",
            Codec::Gif => "GIF",
            Codec::PngSequence => "PNG sequence",
            Codec::Wav => "WAV audio",
            Codec::Aac => "AAC audio",
        }
    }

    /// File extension; empty for a sequence, which is a folder.
    pub fn extension(self) -> &'static str {
        match self {
            Codec::H264 | Codec::Hevc | Codec::H264Hw | Codec::HevcHw => "mp4",
            Codec::ProRes => "mov",
            Codec::Gif => "gif",
            Codec::PngSequence => "",
            Codec::Wav => "wav",
            Codec::Aac => "m4a",
        }
    }

    pub fn has_picture(self) -> bool {
        !matches!(self, Codec::Wav | Codec::Aac)
    }

    pub fn has_sound(self) -> bool {
        !matches!(self, Codec::Gif | Codec::PngSequence)
    }

    /// Whether the constant-quality slider means anything for it.
    pub fn uses_quality(self) -> bool {
        matches!(
            self,
            Codec::H264 | Codec::Hevc | Codec::H264Hw | Codec::HevcHw
        )
    }
}

/// A ready-made set of export choices.
#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub note: &'static str,
    pub codec: Codec,
    pub crf: u32,
}

pub const PRESETS: [Preset; 8] = [
    Preset {
        name: "Web / YouTube",
        note: "H.264, good quality, small file",
        codec: Codec::H264,
        crf: 20,
    },
    Preset {
        name: "Social (fast)",
        note: "Hardware H.264: quick to make, plays everywhere",
        codec: Codec::H264Hw,
        crf: 22,
    },
    Preset {
        name: "Small file",
        note: "HEVC at lower quality for sharing",
        codec: Codec::Hevc,
        crf: 26,
    },
    Preset {
        name: "Master (ProRes)",
        note: "Edit-friendly, near lossless, large",
        codec: Codec::ProRes,
        crf: 18,
    },
    Preset {
        name: "Animated GIF",
        note: "Looping, 480 px wide, no sound",
        codec: Codec::Gif,
        crf: 18,
    },
    Preset {
        name: "Still frames",
        note: "A folder of PNG pictures",
        codec: Codec::PngSequence,
        crf: 18,
    },
    Preset {
        name: "Audio WAV",
        note: "Uncompressed sound only",
        codec: Codec::Wav,
        crf: 18,
    },
    Preset {
        name: "Audio podcast",
        note: "AAC sound only",
        codec: Codec::Aac,
        crf: 18,
    },
];

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub codec: Codec,
    /// x264 constant rate factor (lower = better, 18 is visually lossless)
    pub crf: u32,
    pub preset: String,
    pub audio_kbps: u32,
    pub render: RenderOptions,
    /// Only these timeline ticks: `[start, end)`. None = everything.
    pub range: Option<(i64, i64)>,
    /// Set to stop the export between frames.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Write HDR (Rec.2020, Hybrid Log-Gamma, 10-bit): HEVC only
    pub hdr: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            codec: Codec::H264,
            crf: 18,
            preset: "medium".into(),
            audio_kbps: 192,
            render: RenderOptions::default(),
            range: None,
            cancel: None,
            hdr: false,
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
    if !o.codec.has_picture() {
        return audio_only_args(sample_rate, audio, out, o);
    }
    if o.codec.has_sound() {
        a.extend(["-f", "f32le", "-ar"].map(String::from));
        a.extend([
            sample_rate.to_string(),
            "-ac".into(),
            "2".into(),
            "-i".into(),
            audio.to_string_lossy().into_owned(),
        ]);
    }
    if o.codec == Codec::Gif {
        a.extend(["-an", "-vf"].map(String::from));
        a.push(
            "scale='min(480,iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4"
                .into(),
        );
        a.extend(["-loop", "0"].map(String::from));
        a.push(out.to_string_lossy().into_owned());
        return a;
    }
    if o.codec == Codec::PngSequence {
        a.extend(["-an", "-c:v", "png"].map(String::from));
        a.push(out.join("frame_%06d.png").to_string_lossy().into_owned());
        return a;
    }
    // RGB -> limited-range BT.709 YUV, tagged as such so players decode it the way we composited it;
    // HDR output goes on to Rec.2020 Hybrid Log-Gamma (graphic white at 75 % of the signal, as BT.2408 has it)
    let hdr = o.hdr && o.codec == Codec::Hevc;
    let pix = if o.codec == Codec::ProRes {
        "yuv422p10le"
    } else if hdr {
        "yuv420p10le"
    } else {
        "yuv420p"
    };
    a.extend([
        "-vf".to_string(),
        if hdr {
            "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p,setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv,zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt2020,zscale=t=arib-std-b67:m=bt2020nc:r=tv:npl=203,format=yuv420p10le".to_string()
        } else {
            format!("scale=out_color_matrix=bt709:out_range=tv,format={pix}")
        },
    ]);
    let tags = if hdr {
        [
            "-colorspace",
            "bt2020nc",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            "arib-std-b67",
            "-color_range",
            "tv",
        ]
    } else {
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
    };
    a.extend(tags.map(String::from));
    match o.codec {
        Codec::H264 => {
            a.extend(["-c:v", "libx264", "-preset"].map(String::from));
            a.extend([o.preset.clone(), "-crf".into(), o.crf.to_string()]);
        }
        Codec::Hevc => {
            a.extend(["-c:v", "libx265", "-tag:v", "hvc1", "-preset"].map(String::from));
            a.extend([
                o.preset.clone(),
                "-crf".into(),
                o.crf.to_string(),
                "-x265-params".into(),
                // libx265 only writes the colour description into the stream when asked here
                if hdr {
                    "log-level=error:colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc:range=limited"
                } else {
                    "log-level=error:colorprim=bt709:transfer=bt709:colormatrix=bt709:range=limited"
                }
                .into(),
            ]);
        }
        Codec::ProRes => {
            a.extend(["-c:v", "prores_ks", "-profile:v", "3", "-vendor", "apl0"].map(String::from));
        }
        Codec::H264Hw | Codec::HevcHw => {
            let (enc, tag) = if o.codec == Codec::H264Hw {
                ("h264_videotoolbox", None)
            } else {
                ("hevc_videotoolbox", Some("hvc1"))
            };
            a.extend(["-c:v", enc].map(String::from));
            if let Some(t) = tag {
                a.extend(["-tag:v".to_string(), t.to_string()]);
            }
            // the encoder takes a 0-100 quality rather than a CRF
            let q = (100u32.saturating_sub(o.crf * 2)).clamp(30, 90);
            a.extend([
                "-q:v".to_string(),
                q.to_string(),
                "-allow_sw".into(),
                "1".into(),
            ]);
        }
        Codec::Gif | Codec::PngSequence | Codec::Wav | Codec::Aac => unreachable!("handled above"),
    }
    if o.codec == Codec::ProRes {
        a.extend(["-c:a", "pcm_s16le"].map(String::from));
    } else {
        a.extend(["-c:a", "aac", "-b:a"].map(String::from));
        a.push(format!("{}k", o.audio_kbps));
        a.extend(["-movflags", "+faststart"].map(String::from));
    }
    a.push(out.to_string_lossy().into_owned());
    a
}

fn audio_only_args(sample_rate: u32, audio: &Path, out: &Path, o: &ExportOptions) -> Vec<String> {
    let mut a: Vec<String> = [
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "f32le",
        "-ar",
    ]
    .map(String::from)
    .to_vec();
    a.extend([
        sample_rate.to_string(),
        "-ac".into(),
        "2".into(),
        "-i".into(),
        audio.to_string_lossy().into_owned(),
    ]);
    if o.codec == Codec::Wav {
        a.extend(["-c:a", "pcm_s16le"].map(String::from));
    } else {
        a.extend(["-c:a", "aac", "-b:a"].map(String::from));
        a.push(format!("{}k", o.audio_kbps));
    }
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

/// Render `timeline` to `out`. `progress(done, total)` is called after every frame (or, for sound-only
/// exports, after every second of audio).
pub fn export(
    project: &Project,
    timeline: Id,
    out: &Path,
    opts: &ExportOptions,
    progress: &mut dyn FnMut(i64, i64),
) -> Result<(), ExportError> {
    if opts.hdr && opts.codec == Codec::Hevc && !captaine_media::ffmpeg_has_filter("zscale") {
        return Err(ExportError::Ffmpeg(
            "HDR export needs an FFmpeg built with zscale (libzimg); this one is not".into(),
        ));
    }
    let tl = project
        .timeline(timeline)
        .ok_or(ExportError::NoTimeline(timeline))?;
    let full = tl.duration().value;
    let (first, last) = match opts.range {
        Some((a, b)) => (a.clamp(0, full), b.clamp(0, full)),
        None => (0, full),
    };
    if last <= first {
        return Err(ExportError::Empty);
    }
    let total = last - first;
    let sr = project.settings.sample_rate;
    let cancelled = || {
        opts.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::Relaxed))
    };
    let sr_rate = Rational::new(sr as i64, 1).expect("positive sample rate");
    let to_samples = |ticks: i64| -> i64 {
        RationalTime::new(ticks, tl.rate)
            .rescale(sr_rate, Rounding::Nearest)
            .map_or(0, |t| t.value)
    };
    let (s0, s1) = (to_samples(first), to_samples(last));

    // 1. mix the soundtrack of the chosen range to a raw file, a second at a time
    let audio_path = temp_audio_path();
    let result = (|| -> Result<(), ExportError> {
        if opts.codec.has_sound() {
            let mut mixer = captaine_audio::Mixer::new(project, timeline)
                .map_err(|e| ExportError::Audio(e.to_string()))?;
            let mut w = BufWriter::new(File::create(&audio_path)?);
            let mut at = s0;
            while at < s1 {
                if cancelled() {
                    return Err(ExportError::Cancelled);
                }
                let block = (sr as i64).min(s1 - at) as usize;
                let pcm = mixer
                    .render(at, block)
                    .map_err(|e| ExportError::Audio(e.to_string()))?;
                for s in pcm {
                    w.write_all(&s.clamp(-1.0, 1.0).to_le_bytes())?;
                }
                at += block as i64;
                if !opts.codec.has_picture() {
                    progress(at - s0, s1 - s0);
                }
            }
            w.flush()?;
        }
        if !opts.codec.has_picture() {
            let args = encode_args(0, 0, tl.rate, sr, &audio_path, out, opts);
            let res = Command::new("ffmpeg")
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()?;
            if !res.status.success() {
                return Err(ExportError::Ffmpeg(
                    String::from_utf8_lossy(&res.stderr).trim().to_string(),
                ));
            }
            return Ok(());
        }
        if opts.codec == Codec::PngSequence {
            fs::create_dir_all(out)?;
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
        for tick in first..last {
            if cancelled() {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                remove_when_free(out);
                return Err(ExportError::Cancelled);
            }
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
            progress(tick + 1 - first, total);
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
