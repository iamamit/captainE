mod store;

use std::path::PathBuf;
use std::process::ExitCode;

use captaine_model::*;
use captaine_ops::{edit, Edge, Op};
use captaine_time::{Rational, RationalTime, Timecode};
use clap::{Parser, Subcommand};
use store::Session;

#[derive(Parser)]
#[command(
    name = "captaine",
    about = "CaptainE headless editor: import, edit, render"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Track and item numbers are those shown by `show` (items count from 0 on each track).
#[derive(Subcommand)]
enum Cmd {
    /// Create a project with a timeline and tracks V1, V2, A1, A2
    Init {
        project: PathBuf,
        /// 24, 25, 30, 60, 23.976, 29.97, 59.94 or N/D
        #[arg(long, default_value = "30")]
        fps: String,
        #[arg(long, default_value = "1920x1080")]
        size: String,
    },
    /// Add media files to the project's media pool
    Import {
        project: PathBuf,
        files: Vec<PathBuf>,
    },
    /// Put a clip on a track: appended, or at --at (overwriting what is there)
    Add {
        project: PathBuf,
        media: String,
        #[arg(long, default_value = "V1")]
        track: String,
        /// Timeline position in seconds (default: end of the track)
        #[arg(long)]
        at: Option<f64>,
        /// Start in the source, in seconds
        #[arg(long, default_value_t = 0.0)]
        from: f64,
        /// Length in seconds (default: to the end of the media)
        #[arg(long)]
        dur: Option<f64>,
    },
    /// Cut the item under a timeline time (seconds)
    Split {
        project: PathBuf,
        at: f64,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Ripple delete an item
    Remove {
        project: PathBuf,
        index: usize,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Replace an item with a gap (nothing moves)
    Lift {
        project: PathBuf,
        index: usize,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Drag a clip to another time (and track), overwriting what is there
    Move {
        project: PathBuf,
        index: usize,
        #[arg(long, default_value = "V1")]
        track: String,
        /// Destination track (default: same)
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        at: f64,
    },
    /// Ripple-trim an item's `in` or `out` edge by seconds (negative = earlier)
    Trim {
        project: PathBuf,
        index: usize,
        edge: EdgeArg,
        #[arg(allow_negative_numbers = true)]
        secs: f64,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Move the cut between an item and the next one by seconds
    Roll {
        project: PathBuf,
        index: usize,
        #[arg(allow_negative_numbers = true)]
        secs: f64,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Move an item along the track, taking time from one neighbour and giving it to the other
    Slide {
        project: PathBuf,
        index: usize,
        #[arg(allow_negative_numbers = true)]
        secs: f64,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Move the source window under a clip without moving the clip
    Slip {
        project: PathBuf,
        index: usize,
        #[arg(allow_negative_numbers = true)]
        secs: f64,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Merge neighbouring gaps and drop a trailing gap
    Tidy {
        project: PathBuf,
        #[arg(long, default_value = "V1")]
        track: String,
    },
    /// Change a clip's properties
    Set {
        project: PathBuf,
        index: usize,
        #[arg(long, default_value = "V1")]
        track: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        opacity: Option<f64>,
        /// Offset from center in canvas pixels
        #[arg(long, allow_negative_numbers = true)]
        x: Option<f64>,
        #[arg(long, allow_negative_numbers = true)]
        y: Option<f64>,
        /// 1.0 = fitted to the canvas
        #[arg(long)]
        scale: Option<f64>,
        /// Degrees clockwise
        #[arg(long, allow_negative_numbers = true)]
        rotate: Option<f64>,
        #[arg(long)]
        blend: Option<BlendArg>,
        #[arg(long, allow_negative_numbers = true)]
        gain_db: Option<f64>,
        #[arg(long, allow_negative_numbers = true)]
        pan: Option<f64>,
        /// Fade lengths in seconds
        #[arg(long)]
        fade_in: Option<f64>,
        #[arg(long)]
        fade_out: Option<f64>,
        #[arg(long)]
        mute: Option<bool>,
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Change a track: visibility/mute and gain
    Track {
        project: PathBuf,
        track: String,
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long, allow_negative_numbers = true)]
        gain_db: Option<f64>,
    },
    Undo {
        project: PathBuf,
    },
    Redo {
        project: PathBuf,
    },
    /// List the tracks and their items
    Show {
        project: PathBuf,
    },
    /// The operation log
    Log {
        project: PathBuf,
    },
    /// Render one frame (seconds) to a PNG
    Frame {
        project: PathBuf,
        at: f64,
        out: PathBuf,
    },
    /// Measure the loudness of the mixdown (LUFS, peaks)
    Loudness {
        project: PathBuf,
    },
    /// Render the timeline to a video file
    Render {
        project: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = 18)]
        crf: u32,
        #[arg(long, default_value = "medium")]
        preset: String,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum EdgeArg {
    In,
    Out,
}

#[derive(Clone, clap::ValueEnum)]
enum BlendArg {
    Normal,
    Add,
    Multiply,
    Screen,
}

fn parse_fps(s: &str) -> Result<Rational, String> {
    let known = [
        ("23.976", Rational::FPS_23_976),
        ("29.97", Rational::FPS_29_97),
        ("59.94", Rational::FPS_59_94),
    ];
    if let Some((_, r)) = known.iter().find(|(k, _)| *k == s) {
        return Ok(*r);
    }
    let r = match s.split_once('/') {
        Some((n, d)) => Rational::new(
            n.parse().map_err(|_| "bad fps")?,
            d.parse().map_err(|_| "bad fps")?,
        ),
        None => Rational::new(s.parse().map_err(|_| format!("bad fps '{s}'"))?, 1),
    };
    r.ok_or_else(|| format!("bad fps '{s}'"))
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("size must look like 1920x1080")?;
    let (w, h): (u32, u32) = (
        w.parse().map_err(|_| "bad width")?,
        h.parse().map_err(|_| "bad height")?,
    );
    if w == 0 || h == 0 || w % 2 != 0 || h % 2 != 0 {
        return Err("width and height must be even and non-zero".into());
    }
    Ok((w, h))
}

fn ticks(secs: f64, rate: Rational) -> i64 {
    (secs * rate.as_f64()).round() as i64
}

fn timeline(s: &Session) -> Result<&Timeline, String> {
    s.editor
        .project()
        .timelines
        .values()
        .next()
        .map(|t| t.as_ref())
        .ok_or_else(|| "project has no timeline".to_string())
}

fn find_track<'a>(tl: &'a Timeline, name: &str) -> Result<&'a Track, String> {
    tl.tracks
        .iter()
        .map(|t| t.as_ref())
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            format!(
                "no track '{name}' (tracks: {})",
                tl.tracks
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

fn find_media(p: &Project, key: &str) -> Result<MediaRef, String> {
    let hits: Vec<_> = p
        .media
        .values()
        .filter(|m| m.name == key || m.id.to_string() == key)
        .collect();
    match hits.as_slice() {
        [m] => Ok((*m).clone()),
        [] => Err(format!("no media named '{key}' (import it first)")),
        _ => Err(format!("'{key}' is ambiguous; use the id")),
    }
}

/// (id, item, start tick) of item `index` on `track`
fn nth_item(s: &Session, track: &str, index: usize) -> Result<(Id, Item, i64), String> {
    let t = find_track(timeline(s)?, track)?;
    let starts = t.item_starts();
    t.items
        .get(index)
        .map(|i| (i.id(), i.clone(), starts[index]))
        .ok_or_else(|| {
            format!(
                "no item {index} on {}; it has {} (numbered from 0)",
                t.name,
                t.items.len()
            )
        })
}

fn tc(t: i64, rate: Rational) -> String {
    Timecode::from_frames(t, rate, rate.is_ntsc())
        .map(|t| t.to_string())
        .unwrap_or_else(|_| format!("{t}f"))
}

fn show(s: &Session) -> Result<(), String> {
    let tl = timeline(s)?;
    let set = &s.editor.project().settings;
    println!(
        "{}  {}x{} @ {}  length {}",
        tl.name,
        set.width,
        set.height,
        tl.rate,
        tc(tl.duration().value, tl.rate)
    );
    for t in tl.tracks.iter().rev() {
        let kind = if t.kind == TrackKind::Video {
            "video"
        } else {
            "audio"
        };
        let off = if t.enabled { "" } else { "  [off]" };
        let gain = if t.gain_db != 0.0 {
            format!("  {:+.1} dB", t.gain_db)
        } else {
            String::new()
        };
        println!("\n{} ({kind}){off}{gain}", t.name);
        for (i, (item, start)) in t.items.iter().zip(t.item_starts()).enumerate() {
            let (label, extra) = match item {
                Item::Clip(c) => {
                    let mut e = Vec::new();
                    if c.video != VideoProps::default() {
                        e.push(format!("opacity {:.2}", c.video.opacity));
                    }
                    if c.audio != AudioProps::default() {
                        e.push(format!("{:+.1} dB", c.audio.gain_db));
                    }
                    (
                        format!("{}{}", c.name, if c.enabled { "" } else { " [off]" }),
                        e.join(", "),
                    )
                }
                Item::Gap(_) => ("(gap)".to_string(), String::new()),
            };
            println!(
                "  {i:>2}  {:<12} {:<12} {label} {extra}",
                tc(start, tl.rate),
                tc(item.duration().value, tl.rate)
            );
        }
    }
    Ok(())
}

fn run(cmd: Cmd) -> Result<(), String> {
    // read-modify-write helper for the edit commands
    let edit_with = |path: &PathBuf,
                     f: &dyn Fn(&mut Session) -> Result<String, String>|
     -> Result<(), String> {
        let mut s = Session::load(path)?;
        let msg = f(&mut s)?;
        s.save(path)?;
        println!("{msg}");
        Ok(())
    };
    let plan = |s: &mut Session, op: Result<Op, captaine_ops::OpError>| -> Result<(), String> {
        s.apply(op.map_err(|e| e.to_string())?)
    };
    match cmd {
        Cmd::Init { project, fps, size } => {
            if project.exists() {
                return Err(format!("{} already exists", project.display()));
            }
            let rate = parse_fps(&fps)?;
            let (width, height) = parse_size(&size)?;
            let mut s = Session::new(ProjectSettings {
                frame_rate: rate,
                width,
                height,
                sample_rate: 48_000,
            });
            let tl = Id::new();
            s.apply(Op::AddTimeline {
                timeline: Timeline::new(tl, "Main", rate),
            })?;
            for (i, (name, kind)) in [
                ("V1", TrackKind::Video),
                ("V2", TrackKind::Video),
                ("A1", TrackKind::Audio),
                ("A2", TrackKind::Audio),
            ]
            .into_iter()
            .enumerate()
            {
                s.apply(Op::AddTrack {
                    timeline: tl,
                    index: i,
                    track: Track::new(Id::new(), name, kind),
                })?;
            }
            s.save(&project)?;
            println!(
                "created {} ({rate} fps, {width}x{height}; tracks V1 V2 A1 A2)",
                project.display()
            );
            Ok(())
        }
        Cmd::Import { project, files } => edit_with(&project, &|s| {
            let mut out = Vec::new();
            for f in &files {
                let m = captaine_media::media_ref(f, Id::new())
                    .map_err(|e| format!("{}: {e}", f.display()))?;
                let d = m.duration.map_or(0, |d| d.value);
                let r = m.duration.map_or(Rational::FPS_30, |d| d.rate);
                let kind = match (m.has_video, m.has_audio) {
                    (true, true) => format!("{}x{}, audio", m.width, m.height),
                    (true, false) => format!("{}x{}", m.width, m.height),
                    _ => "audio only".to_string(),
                };
                let len = if m.has_video {
                    format!("{d} frames @ {r}")
                } else {
                    format!("{:.1} s", d as f64 / r.as_f64())
                };
                out.push(format!("imported {} ({len}, {kind})", m.name));
                s.apply(Op::AddMedia { media: m })?;
            }
            Ok(out.join("\n"))
        }),
        Cmd::Add {
            project,
            media,
            track,
            at,
            from,
            dur,
        } => edit_with(&project, &|s| {
            let m = find_media(s.editor.project(), &media)?;
            let tl = timeline(s)?;
            let tr = find_track(tl, &track)?;
            if tr.kind == TrackKind::Video && !m.has_video {
                return Err(format!(
                    "{} has no picture; put it on an audio track",
                    m.name
                ));
            }
            if tr.kind == TrackKind::Audio && !m.has_audio {
                return Err(format!("{} has no audio", m.name));
            }
            let (track_id, rate, track_name) = (tr.id, tl.rate, tr.name.clone());
            let len = m.duration.ok_or("media length unknown")?;
            let src_rate = len.rate;
            let start = ticks(from, src_rate);
            let frames = match dur {
                Some(d) => ticks(d, rate),
                None => ticks((len.value - start) as f64 / src_rate.as_f64(), rate),
            };
            let clip = Clip::new(
                Id::new(),
                m.name.clone(),
                m.id,
                RationalTime::new(start, src_rate),
                RationalTime::new(frames, rate),
            );
            let msg = match at {
                None => {
                    s.apply(edit::append(s.editor.project(), track_id, clip))?;
                    format!("added {} to the end of {}", m.name, track_name)
                }
                Some(at) => {
                    let op = edit::overwrite(
                        s.editor.project(),
                        track_id,
                        RationalTime::new(ticks(at, rate), rate),
                        clip,
                    );
                    plan(s, op)?;
                    format!(
                        "placed {} on {} at {}",
                        m.name,
                        track_name,
                        tc(ticks(at, rate), rate)
                    )
                }
            };
            Ok(msg)
        }),
        Cmd::Split { project, at, track } => edit_with(&project, &|s| {
            let tl = timeline(s)?;
            let (rate, id) = (tl.rate, find_track(tl, &track)?.id);
            let t = ticks(at, rate);
            let op = edit::split_at(s.editor.project(), id, RationalTime::new(t, rate));
            plan(s, op).map_err(|e| {
                format!("{e} (a cut must fall between an item's first and last frame)")
            })?;
            Ok(format!("split {track} at {}", tc(t, rate)))
        }),
        Cmd::Remove {
            project,
            index,
            track,
        } => edit_with(&project, &|s| {
            let (id, ..) = nth_item(s, &track, index)?;
            s.apply(Op::RemoveItem { item: id })?;
            Ok(format!(
                "removed {track} item {index} (later items moved up)"
            ))
        }),
        Cmd::Lift {
            project,
            index,
            track,
        } => edit_with(&project, &|s| {
            let (id, ..) = nth_item(s, &track, index)?;
            let op = edit::lift(s.editor.project(), id);
            plan(s, op)?;
            Ok(format!("lifted {track} item {index}"))
        }),
        Cmd::Move {
            project,
            index,
            track,
            to,
            at,
        } => edit_with(&project, &|s| {
            let (id, ..) = nth_item(s, &track, index)?;
            let tl = timeline(s)?;
            let rate = tl.rate;
            let dest = find_track(tl, to.as_deref().unwrap_or(&track))?.id;
            let op = edit::move_item(
                s.editor.project(),
                id,
                dest,
                RationalTime::new(ticks(at, rate), rate),
            );
            plan(s, op)?;
            Ok(format!(
                "moved {track} item {index} to {}",
                tc(ticks(at, rate), rate)
            ))
        }),
        Cmd::Trim {
            project,
            index,
            edge,
            secs,
            track,
        } => edit_with(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (id, ..) = nth_item(s, &track, index)?;
            let edge = match edge {
                EdgeArg::In => Edge::In,
                EdgeArg::Out => Edge::Out,
            };
            s.apply(Op::Trim {
                item: id,
                edge,
                delta: RationalTime::new(ticks(secs, rate), rate),
            })?;
            Ok(format!("trimmed {track} item {index}"))
        }),
        Cmd::Roll {
            project,
            index,
            secs,
            track,
        } => edit_with(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (left, ..) = nth_item(s, &track, index)?;
            let (right, ..) = nth_item(s, &track, index + 1)?;
            let op = edit::roll(
                s.editor.project(),
                left,
                right,
                RationalTime::new(ticks(secs, rate), rate),
            );
            plan(s, op)?;
            Ok(format!("rolled the cut after {track} item {index}"))
        }),
        Cmd::Slide {
            project,
            index,
            secs,
            track,
        } => edit_with(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (id, ..) = nth_item(s, &track, index)?;
            let op = edit::slide(
                s.editor.project(),
                id,
                RationalTime::new(ticks(secs, rate), rate),
            );
            plan(s, op)?;
            Ok(format!("slid {track} item {index}"))
        }),
        Cmd::Slip {
            project,
            index,
            secs,
            track,
        } => edit_with(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (id, ..) = nth_item(s, &track, index)?;
            s.apply(Op::Slip {
                item: id,
                delta: RationalTime::new(ticks(secs, rate), rate),
            })?;
            Ok(format!("slipped {track} item {index}"))
        }),
        Cmd::Tidy { project, track } => edit_with(&project, &|s| {
            let id = find_track(timeline(s)?, &track)?.id;
            match edit::clean_gaps(s.editor.project(), id).map_err(|e| e.to_string())? {
                Some(op) => {
                    s.apply(op)?;
                    Ok(format!("tidied {track}"))
                }
                None => Ok(format!("{track} has nothing to tidy")),
            }
        }),
        Cmd::Set {
            project,
            index,
            track,
            name,
            opacity,
            x,
            y,
            scale,
            rotate,
            blend,
            gain_db,
            pan,
            fade_in,
            fade_out,
            mute,
            enabled,
        } => edit_with(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (id, item, _) = nth_item(s, &track, index)?;
            let Item::Clip(c) = item else {
                return Err("that item is a gap".into());
            };
            let mut ops = Vec::new();
            let mut v = c.video;
            if let Some(o) = opacity {
                v.opacity = o;
            }
            if let Some(x) = x {
                v.transform.position.0 = x;
            }
            if let Some(y) = y {
                v.transform.position.1 = y;
            }
            if let Some(k) = scale {
                v.transform.scale = (k, k);
            }
            if let Some(r) = rotate {
                v.transform.rotation_deg = r;
            }
            if let Some(b) = &blend {
                v.blend = match b {
                    BlendArg::Normal => BlendMode::Normal,
                    BlendArg::Add => BlendMode::Add,
                    BlendArg::Multiply => BlendMode::Multiply,
                    BlendArg::Screen => BlendMode::Screen,
                };
            }
            if v != c.video {
                ops.push(Op::SetVideo { item: id, video: v });
            }
            let mut a = c.audio;
            if let Some(g) = gain_db {
                a.gain_db = g;
            }
            if let Some(p) = pan {
                a.pan = p;
            }
            if let Some(f) = fade_in {
                a.fade_in = ticks(f, rate);
            }
            if let Some(f) = fade_out {
                a.fade_out = ticks(f, rate);
            }
            if let Some(m) = mute {
                a.muted = m;
            }
            if a != c.audio {
                ops.push(Op::SetAudio { item: id, audio: a });
            }
            if let Some(n) = &name {
                ops.push(Op::SetName {
                    item: id,
                    name: n.clone(),
                });
            }
            if let Some(e) = enabled {
                ops.push(Op::SetEnabled {
                    item: id,
                    enabled: e,
                });
            }
            if ops.is_empty() {
                return Err("nothing to change; pass at least one option (see --help)".into());
            }
            s.apply(Op::Transaction {
                description: format!("Set properties of {}", c.name),
                ops,
            })?;
            Ok(format!("updated {track} item {index}"))
        }),
        Cmd::Track {
            project,
            track,
            enabled,
            gain_db,
        } => edit_with(&project, &|s| {
            let t = find_track(timeline(s)?, &track)?;
            let (id, en, g) = (
                t.id,
                enabled.unwrap_or(t.enabled),
                gain_db.unwrap_or(t.gain_db),
            );
            s.apply(Op::SetTrack {
                track: id,
                enabled: en,
                gain_db: g,
            })?;
            Ok(format!(
                "{track}: {}, {g:+.1} dB",
                if en { "on" } else { "off" }
            ))
        }),
        Cmd::Undo { project } => edit_with(&project, &|s| {
            s.undo()
                .map(|d| format!("undid: {d}"))
                .ok_or("nothing to undo".to_string())
        }),
        Cmd::Redo { project } => edit_with(&project, &|s| {
            s.redo()
                .map(|d| format!("redid: {d}"))
                .ok_or("nothing to redo".to_string())
        }),
        Cmd::Show { project } => show(&Session::load(&project)?),
        Cmd::Log { project } => {
            for (i, e) in Session::load(&project)?.editor.history().iter().enumerate() {
                println!("{:>3}  {:?}  {}", i + 1, e.author, e.op.describe());
            }
            Ok(())
        }
        Cmd::Frame { project, at, out } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            let tick = ticks(at, tl.rate).clamp(0, (tl.duration().value - 1).max(0));
            let mut r =
                captaine_render::Renderer::new(s.editor.project(), tl.id, Default::default())
                    .map_err(|e| e.to_string())?;
            r.render(tick).map_err(|e| e.to_string())?.save_png(&out)?;
            println!("wrote {} (frame at {})", out.display(), tc(tick, tl.rate));
            Ok(())
        }
        Cmd::Loudness { project } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            let sr = s.editor.project().settings.sample_rate;
            let mut mixer =
                captaine_audio::Mixer::new(s.editor.project(), tl.id).map_err(|e| e.to_string())?;
            let all = mixer
                .render(0, mixer.total_samples() as usize)
                .map_err(|e| e.to_string())?;
            let l = captaine_audio::measure(&all, sr)?;
            println!("integrated {:.1} LUFS   range {:.1} LU   sample peak {:.1} dBFS   true peak {:.1} dBTP", l.integrated_lufs, l.loudness_range_lu, l.sample_peak_db, l.true_peak_db);
            Ok(())
        }
        Cmd::Render {
            project,
            out,
            crf,
            preset,
        } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            let opts = captaine_export::ExportOptions {
                crf,
                preset,
                ..Default::default()
            };
            let started = std::time::Instant::now();
            let mut last_pct = u64::MAX;
            captaine_export::export(
                s.editor.project(),
                tl.id,
                &out,
                &opts,
                &mut |done, total| {
                    let pct = (done * 100 / total) as u64;
                    if pct != last_pct && pct % 5 == 0 {
                        eprint!("\rrendering {pct:>3}%");
                        last_pct = pct;
                    }
                },
            )
            .map_err(|e| e.to_string())?;
            eprintln!();
            println!(
                "rendered {} ({}) in {:.1}s",
                out.display(),
                tc(tl.duration().value, tl.rate),
                started.elapsed().as_secs_f64()
            );
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse().cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
