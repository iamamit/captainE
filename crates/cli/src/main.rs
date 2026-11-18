use std::path::PathBuf;
use std::process::ExitCode;

use captaine_model::*;
use captaine_ops::{edit, Edge, Op};
use captaine_project::Session;
use captaine_time::{Rational, RationalTime, Timecode};
use clap::{Parser, Subcommand};

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
    /// Add a title (default: on the top video track at the start)
    Title {
        project: PathBuf,
        text: String,
        /// Second line
        #[arg(long)]
        sub: Option<String>,
        #[arg(long, value_enum, default_value = "lower-third")]
        template: TemplateArg,
        /// Timeline position in seconds
        #[arg(long, default_value_t = 0.0)]
        at: f64,
        /// Length in seconds
        #[arg(long, default_value_t = 3.0)]
        dur: f64,
        /// Track name (default: the top video track)
        #[arg(long)]
        track: Option<String>,
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
        /// Title text (titles only)
        #[arg(long)]
        text: Option<String>,
        /// Title second line (titles only)
        #[arg(long)]
        sub: Option<String>,
        /// Title size in pixels at 1080p (titles only)
        #[arg(long)]
        size: Option<f64>,
        /// 0.0 (invisible) to 1.0 (solid)
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
        /// -1.0 (left) to 1.0 (right)
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
    /// Add empty tracks: video (above the top video track) or audio (after the last audio track)
    AddTrack {
        project: PathBuf,
        /// video or audio
        kind: String,
        #[arg(long, default_value_t = 1)]
        count: usize,
        /// Track name (default: the next V/A number)
        #[arg(long)]
        name: Option<String>,
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
    /// History: the commits of a branch, then any edits not yet committed
    Log {
        project: PathBuf,
        #[arg(long)]
        branch: Option<String>,
    },
    /// List branches, create one (`branch p.json try`), or delete one (`--delete try`)
    Branch {
        project: PathBuf,
        name: Option<String>,
        /// Delete this branch
        #[arg(long)]
        delete: Option<String>,
        /// Start the new branch from this commit (default: where you are)
        #[arg(long)]
        from: Option<String>,
        /// Switch to the new branch (your uncommitted edits come along)
        #[arg(long)]
        switch: bool,
    },
    /// Switch branch (uncommitted edits are committed on the branch you leave)
    Checkout {
        project: PathBuf,
        branch: String,
    },
    /// Turn the edits since the last commit into a commit
    Commit {
        project: PathBuf,
        #[arg(short, long)]
        message: Option<String>,
    },
    /// What differs between two versions: branch names, commit ids, or `working`
    Diff {
        project: PathBuf,
        a: String,
        b: String,
    },
    /// Merge a branch into the current one
    Merge {
        project: PathBuf,
        branch: String,
        /// Only show what would happen
        #[arg(long)]
        dry_run: bool,
        /// Who wins when both sides changed the same thing
        #[arg(long, value_enum, default_value = "ours")]
        conflicts: SideArg,
        /// Leave out changes to this item or track (by name); repeatable
        #[arg(long)]
        skip: Vec<String>,
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Make lightweight proxy files for faster editing (kept in <project>.cache/)
    Proxy {
        project: PathBuf,
        /// Proxy height in pixels
        #[arg(long, default_value_t = 540)]
        height: u32,
    },
    /// A strip of thumbnails from a media file
    Thumbs {
        project: PathBuf,
        media: String,
        out: PathBuf,
        #[arg(long, default_value_t = 8)]
        count: u32,
        /// Width of each thumbnail in pixels
        #[arg(long, default_value_t = 160)]
        width: u32,
    },
    /// A waveform picture of a media file's audio
    Waveform {
        project: PathBuf,
        media: String,
        out: PathBuf,
    },
    /// Render one frame (seconds) to a PNG
    Frame {
        project: PathBuf,
        at: f64,
        out: PathBuf,
        /// Read proxies where they exist
        #[arg(long)]
        proxy: bool,
    },
    /// Measure the loudness of the mixdown (LUFS, peaks)
    Loudness {
        project: PathBuf,
    },
    /// Write the timeline for another editor: .otio, .edl or .fcpxml (by the file name's extension)
    ExportTimeline {
        project: PathBuf,
        out: PathBuf,
    },
    /// Read a .otio (as a new sequence) or .edl (onto the current timeline) into the project
    ImportTimeline {
        project: PathBuf,
        file: PathBuf,
    },
    /// Render the timeline to a video file
    Render {
        project: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = 18)]
        crf: u32,
        #[arg(long, default_value = "medium")]
        preset: String,
        #[arg(long, value_enum, default_value = "h264")]
        codec: CodecArg,
        /// HDR output (Rec.2020 HLG, 10-bit): with --codec hevc
        #[arg(long)]
        hdr: bool,
        /// Draft quality: read proxies where they exist
        #[arg(long)]
        proxy: bool,
        /// Only render from this time (seconds)
        #[arg(long)]
        from: Option<f64>,
        /// ... up to this time (seconds)
        #[arg(long)]
        to: Option<f64>,
        /// Output height in pixels (e.g. 2160 for 4K from a 1080p timeline); the framing stays the same
        #[arg(long)]
        height: Option<u32>,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum TemplateArg {
    LowerThird,
    Centered,
    Caption,
    Minimal,
    Boxed,
    Quote,
}

#[derive(Clone, clap::ValueEnum)]
enum SideArg {
    Ours,
    Theirs,
}

#[derive(Clone, clap::ValueEnum)]
enum CodecArg {
    H264,
    Hevc,
    Prores,
    H264Hw,
    HevcHw,
    Gif,
    Png,
    Wav,
    Aac,
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

/// `<project file>.cache/` next to the project: proxies and other regenerable files.
fn cache_dir(project: &std::path::Path) -> PathBuf {
    let mut name = project
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".cache");
    project.with_file_name(name)
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
                Item::Title(x) => (
                    format!("{}{}", x.name, if x.enabled { "" } else { " [off]" }),
                    format!("{} · \"{}\"", x.style.template.label(), x.text),
                ),
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
            let (mut out, mut failed) = (Vec::new(), Vec::new());
            for f in &files {
                // one bad file does not stop the rest
                let m = match captaine_media::media_ref(f, Id::new()) {
                    Ok(m) => m,
                    Err(e) => {
                        failed.push(format!("could not import {}: {e}", f.display()));
                        continue;
                    }
                };
                if s.editor.project().media.values().any(|x| x.path == m.path) {
                    out.push(format!("{} is already in the project", m.name));
                    continue;
                }
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
            if out.is_empty() {
                return Err(failed.join("\n"));
            }
            for f in &failed {
                eprintln!("{f}");
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
        Cmd::Title {
            project,
            text,
            sub,
            template,
            at,
            dur,
            track,
        } => edit_with(&project, &|s| {
            let tl = timeline(s)?;
            let rate = tl.rate;
            let (from, until) = (ticks(at, rate), ticks(at, rate) + ticks(dur, rate).max(1));
            // a track is free when nothing but gaps covers the title's span
            let free = |t: &captaine_model::Track| {
                let mut start = 0;
                t.items.iter().all(|it| {
                    let end = start + it.duration().value;
                    let clear = !it.is_content() || end <= from || start >= until;
                    start = end;
                    clear
                })
            };
            let pick = |tl: &Timeline| {
                tl.tracks
                    .iter()
                    .rev()
                    .filter(|t| t.kind == TrackKind::Video)
                    .find(|t| free(t))
                    .map(|t| t.id)
            };
            let track_id = match &track {
                Some(n) => find_track(tl, n)?.id,
                // the highest video track with room, so a title never covers a clip by surprise;
                // when every track is busy there, the title gets a new track of its own
                None => match pick(tl) {
                    Some(id) => id,
                    None => {
                        let tl = tl.clone();
                        let op = edit::add_tracks(&tl, TrackKind::Video, 1, None)
                            .map_err(|e| e.to_string())?;
                        s.apply(op)?;
                        pick(timeline(s)?).ok_or("no video track")?
                    }
                },
            };
            let template = match template {
                TemplateArg::LowerThird => TitleTemplate::LowerThird,
                TemplateArg::Centered => TitleTemplate::Centered,
                TemplateArg::Caption => TitleTemplate::Caption,
                TemplateArg::Minimal => TitleTemplate::Minimal,
                TemplateArg::Boxed => TitleTemplate::Boxed,
                TemplateArg::Quote => TitleTemplate::Quote,
            };
            let mut t = Title::new(
                Id::new(),
                template,
                RationalTime::new(ticks(dur, rate).max(1), rate),
            );
            t.text = text.clone();
            t.subtitle = sub.clone().unwrap_or_default();
            t.name = format!("Title: {text}");
            let start = RationalTime::new(ticks(at, rate), rate);
            let op = edit::overwrite_item(s.editor.project(), track_id, start, Item::Title(t));
            plan(s, op)?;
            Ok(format!(
                "added {} title \"{text}\" at {}",
                template.label(),
                tc(ticks(at, rate), rate)
            ))
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
            text,
            sub,
            size,
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
            if let Item::Title(tt) = &item {
                let mut v = tt.video;
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
                let mut nt = tt.clone();
                nt.video = v;
                if let Some(t) = &text {
                    nt.text = t.clone();
                }
                if let Some(t) = &sub {
                    nt.subtitle = t.clone();
                }
                if let Some(sz) = size {
                    nt.style.size = sz;
                }
                if let Some(f) = fade_in {
                    nt.fade_in = ticks(f, rate);
                }
                if let Some(f) = fade_out {
                    nt.fade_out = ticks(f, rate);
                }
                let mut ops = Vec::new();
                if nt != *tt {
                    ops.push(Op::SetTitle {
                        item: id,
                        title: nt,
                    });
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
                    description: format!("Set properties of {}", tt.name),
                    ops,
                })?;
                return Ok(format!("updated {track} item {index}"));
            }
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
        Cmd::AddTrack {
            project,
            kind,
            count,
            name,
        } => edit_with(&project, &|s| {
            let kind = match kind.as_str() {
                "video" | "v" => TrackKind::Video,
                "audio" | "a" => TrackKind::Audio,
                other => return Err(format!("kind is video or audio, not '{other}'")),
            };
            let tl = timeline(s)?.clone();
            let op = captaine_ops::edit::add_tracks(&tl, kind, count, name.as_deref())
                .map_err(|e| e.to_string())?;
            let names: Vec<String> = match &op {
                Op::Transaction { ops, .. } => ops
                    .iter()
                    .filter_map(|o| match o {
                        Op::AddTrack { track, .. } => Some(track.name.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            s.apply(op)?;
            Ok(format!("added {}", names.join(", ")))
        }),
        Cmd::Proxy { project, height } => edit_with(&project, &|s| {
            let dir = cache_dir(&project);
            let todo: Vec<MediaRef> = s
                .editor
                .project()
                .media
                .values()
                .filter(|m| m.has_video && m.proxy.is_none())
                .cloned()
                .collect();
            let jobs: Vec<(PathBuf, PathBuf)> = todo
                .iter()
                .map(|m| {
                    eprintln!("proxy for {} ...", m.name);
                    (
                        PathBuf::from(&m.path),
                        captaine_media::cache_path(&dir, m.id, "proxy.mp4"),
                    )
                })
                .collect();
            let failed = std::sync::Mutex::new(None);
            captaine_media::make_proxies(&jobs, height, 2, &|i, r| {
                if let Err(e) = r {
                    *failed.lock().unwrap() = Some(format!("{}: {e}", todo[i].name));
                }
            });
            if let Some(e) = failed.into_inner().unwrap() {
                return Err(e);
            }
            let ops: Vec<Op> = todo
                .iter()
                .zip(&jobs)
                .map(|(m, (_, dst))| Op::SetProxy {
                    media: m.id,
                    proxy: Some(dst.to_string_lossy().into_owned()),
                })
                .collect();
            if ops.is_empty() {
                return Ok("every video already has a proxy".into());
            }
            let n = ops.len();
            s.apply(Op::Transaction {
                description: "Create proxies".into(),
                ops,
            })?;
            Ok(format!("made {n} proxy file(s) in {}", dir.display()))
        }),
        Cmd::Thumbs {
            project,
            media,
            out,
            count,
            width,
        } => {
            let s = Session::load(&project)?;
            let m = find_media(s.editor.project(), &media)?;
            if !m.has_video {
                return Err(format!("{} has no picture", m.name));
            }
            let len = m.duration.ok_or("media length unknown")?;
            let mut frames = Vec::new();
            for i in 0..count.max(1) {
                let idx = (len.value - 1).max(0) * (2 * i as i64 + 1) / (2 * count.max(1) as i64);
                let mut r =
                    captaine_render::VideoReader::open(&m.path, m.width, m.height, len.rate, idx)?;
                frames.push(r.frame(idx)?.thumbnail(width));
            }
            let (w, h, rgb) = captaine_render::contact_sheet(&frames);
            captaine_render::save_rgb_png(&out, w, h, &rgb)?;
            println!("wrote {} ({count} thumbnails)", out.display());
            Ok(())
        }
        Cmd::Waveform {
            project,
            media,
            out,
        } => {
            let s = Session::load(&project)?;
            let m = find_media(s.editor.project(), &media)?;
            if !m.has_audio {
                return Err(format!("{} has no audio", m.name));
            }
            let len = m.duration.ok_or("media length unknown")?;
            let secs = len.to_seconds_f64();
            let (w, h) = (900u32, 120u32);
            let pk = captaine_audio::media_peaks(
                &m.path,
                secs,
                (w as f64 / secs).ceil().max(1.0) as u32,
            )
            .map_err(|e| e.to_string())?;
            let mut rgb = vec![24u8; (w * h * 3) as usize];
            for x in 0..w {
                let p = pk[((x as usize) * pk.len() / w as usize).min(pk.len() - 1)];
                let y = |v: f32| ((1.0 - v.clamp(-1.0, 1.0)) * 0.5 * (h - 1) as f32).round() as u32;
                for yy in y(p.max)..=y(p.min) {
                    let i = ((yy * w + x) * 3) as usize;
                    rgb[i..i + 3].copy_from_slice(&[120, 200, 255]);
                }
            }
            captaine_render::save_rgb_png(&out, w, h, &rgb)?;
            println!("wrote {} ({secs:.1} s)", out.display());
            Ok(())
        }
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
        Cmd::Log { project, branch } => {
            let s = Session::load(&project)?;
            let branch = branch.unwrap_or_else(|| s.branch().to_string());
            if s.has_uncommitted() && branch == s.branch() {
                println!("* {} uncommitted edit(s):", s.uncommitted_count());
                for e in s.editor.history().iter().rev().take(s.uncommitted_count()) {
                    println!("    {}", e.op.describe());
                }
            }
            for c in s.log(&branch)? {
                let tags = if c.branches.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", c.branches.join(", "))
                };
                println!(
                    "{}  {}  {} · {}{tags}",
                    c.id,
                    c.message,
                    captaine_project::author_label(&c.author),
                    captaine_project::time_ago(c.time)
                );
                let summary = s.commit_summary(&c.id);
                if !summary.is_empty() {
                    println!("          {summary}");
                }
            }
            Ok(())
        }
        Cmd::Branch {
            project,
            name,
            delete,
            from,
            switch,
        } => {
            let mut s = Session::load(&project)?;
            if let Some(d) = delete {
                s.delete_branch(&d)?;
                println!("deleted branch {d}");
            } else if let Some(n) = name {
                match from {
                    Some(c) => s.create_branch_at(&n, &c)?,
                    None => s.create_branch(&n, false)?,
                }
                if switch {
                    s.checkout(&n)?;
                }
                println!(
                    "created branch {n}{}",
                    if switch { " and switched to it" } else { "" }
                );
            } else {
                for b in s.branch_infos() {
                    let rel = match (b.ahead, b.behind) {
                        _ if b.current => String::new(),
                        (a, 0) => format!("  {a} ahead"),
                        (0, bh) => format!("  {bh} behind"),
                        (a, bh) => format!("  {a} ahead, {bh} behind"),
                    };
                    println!(
                        "{} {}{rel}  — {} ({})",
                        if b.current { "*" } else { " " },
                        b.name,
                        b.last_message,
                        captaine_project::time_ago(b.last_time)
                    );
                }
                return Ok(());
            }
            s.save(&project)
        }
        Cmd::Checkout { project, branch } => {
            let mut s = Session::load(&project)?;
            s.checkout(&branch)?;
            s.save(&project)?;
            println!("on {branch}");
            Ok(())
        }
        Cmd::Commit { project, message } => {
            let mut s = Session::load(&project)?;
            match s.commit(message.as_deref())? {
                Some(id) => {
                    s.save(&project)?;
                    println!("committed {id}: {}", s.log(s.branch())?[0].message);
                }
                None => println!("nothing to commit"),
            }
            Ok(())
        }
        Cmd::Diff { project, a, b } => {
            let s = Session::load(&project)?;
            let changes = s.compare(&a, &b)?;
            let rate = timeline(&s).map(|t| t.rate).unwrap_or(Rational::FPS_30);
            for c in changes.iter().filter(|c| !c.is_noise()) {
                println!("  {}", c.summary);
            }
            println!(
                "{a} → {b}: {}",
                captaine_project::captaine_vcs::summarize(&changes, rate)
            );
            Ok(())
        }
        Cmd::Merge {
            project,
            branch,
            dry_run,
            conflicts,
            skip,
            message,
        } => {
            let mut s = Session::load(&project)?;
            let setup = s.prepare_merge(&branch)?;
            if setup.up_to_date {
                println!("already up to date");
                return Ok(());
            }
            use captaine_project::captaine_vcs::{Choice, Status};
            let mut choices = std::collections::HashMap::new();
            for e in &setup.plan.entries {
                let skipped = skip.contains(&e.change.name);
                let conflict = matches!(e.status, Status::Conflict(_));
                let take = !skipped && (!conflict || matches!(conflicts, SideArg::Theirs));
                choices.insert(
                    e.change.id.clone(),
                    if take { Choice::Take } else { Choice::Skip },
                );
                let mark = match (&e.status, take) {
                    (Status::Conflict(why), t) => {
                        format!("CONFLICT ({why}) -> {}", if t { "theirs" } else { "ours" })
                    }
                    (_, true) => "take".to_string(),
                    (_, false) => "skip".to_string(),
                };
                println!("  [{mark}] {}", e.change.summary);
            }
            if dry_run {
                println!("(dry run: nothing merged)");
                s.save(&project)?; // preparing the merge saved work in progress
                return Ok(());
            }
            match s.merge(&branch, &choices, message.as_deref())? {
                captaine_project::MergeReport::FastForward => {
                    println!("fast-forwarded {} to {branch}", s.branch())
                }
                captaine_project::MergeReport::UpToDate => println!("already up to date"),
                captaine_project::MergeReport::Merged {
                    applied, skipped, ..
                } => {
                    println!(
                        "merged {branch} into {}: {applied} change(s) brought in",
                        s.branch()
                    );
                    for (what, why) in skipped {
                        println!("  could not bring in {what}: {why}");
                    }
                }
            }
            s.save(&project)
        }
        Cmd::Frame {
            project,
            at,
            out,
            proxy,
        } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            let tick = ticks(at, tl.rate).clamp(0, (tl.duration().value - 1).max(0));
            let opts = captaine_render::RenderOptions {
                use_proxy: proxy,
                ..Default::default()
            };
            let mut r = captaine_render::Renderer::new(s.editor.project(), tl.id, opts)
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
        Cmd::ExportTimeline { project, out } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            let p = s.editor.project();
            let ext = out
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let text = match ext.as_str() {
                "otio" => captaine_interchange::otio::export(p, tl.id)?,
                "edl" => captaine_interchange::edl::export(p, tl.id, false)?,
                "fcpxml" => captaine_interchange::fcpxml::export(p, tl.id)?,
                _ => return Err("the output name must end in .otio, .edl or .fcpxml".into()),
            };
            std::fs::write(&out, text).map_err(|e| e.to_string())?;
            println!("wrote {}", out.display());
            Ok(())
        }
        Cmd::ImportTimeline { project, file } => edit_with(&project, &|s| {
            let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
            let ext = file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let ops = match ext.as_str() {
                "otio" => captaine_interchange::otio::import(s.editor.project(), &text)?,
                "edl" => {
                    captaine_interchange::edl::import(s.editor.project(), timeline(s)?.id, &text)?
                }
                _ => return Err("only .otio and .edl files can be imported".into()),
            };
            let n = ops.len();
            s.apply(Op::Transaction {
                description: format!("Import {}", file.display()),
                ops,
            })?;
            Ok(format!(
                "imported {} ({n} change{})",
                file.display(),
                if n == 1 { "" } else { "s" }
            ))
        }),
        Cmd::Render {
            project,
            out,
            crf,
            preset,
            codec,
            hdr,
            proxy,
            from,
            to,
            height,
        } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            // another output size: the whole project scaled, positions included, so the framing is unchanged
            let sized = height.map(|h| {
                let (_, ch) = s.editor.project().canvas(tl.id);
                captaine_render::preview_project(s.editor.project(), h as f64 / ch.max(1) as f64)
            });
            let source = sized.as_ref().unwrap_or(s.editor.project());
            let fps = tl.rate.as_f64();
            let range = match (from, to) {
                (None, None) => None,
                (a, b) => Some((
                    (a.unwrap_or(0.0) * fps).round() as i64,
                    b.map_or(i64::MAX, |b| (b * fps).round() as i64),
                )),
            };
            let opts = captaine_export::ExportOptions {
                codec: match codec {
                    CodecArg::H264 => captaine_export::Codec::H264,
                    CodecArg::Hevc => captaine_export::Codec::Hevc,
                    CodecArg::Prores => captaine_export::Codec::ProRes,
                    CodecArg::H264Hw => captaine_export::Codec::H264Hw,
                    CodecArg::HevcHw => captaine_export::Codec::HevcHw,
                    CodecArg::Gif => captaine_export::Codec::Gif,
                    CodecArg::Png => captaine_export::Codec::PngSequence,
                    CodecArg::Wav => captaine_export::Codec::Wav,
                    CodecArg::Aac => captaine_export::Codec::Aac,
                },
                range,
                crf,
                preset,
                hdr,
                render: captaine_render::RenderOptions {
                    use_proxy: proxy,
                    ..Default::default()
                },
                ..Default::default()
            };
            let started = std::time::Instant::now();
            let mut last_pct = u64::MAX;
            captaine_export::export(source, tl.id, &out, &opts, &mut |done, total| {
                let pct = (done * 100 / total) as u64;
                if pct != last_pct && pct % 5 == 0 {
                    eprint!("\rrendering {pct:>3}%");
                    last_pct = pct;
                }
            })
            .map_err(|e| e.to_string())?;
            eprintln!();
            println!(
                "rendered {} ({}) in {:.1}s",
                out.display(),
                tc(
                    match range {
                        Some((a, b)) => b.min(tl.duration().value) - a.max(0),
                        None => tl.duration().value,
                    },
                    tl.rate
                ),
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
