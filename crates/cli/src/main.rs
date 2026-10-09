mod store;

use std::path::PathBuf;
use std::process::ExitCode;

use captaine_model::*;
use captaine_ops::{Edge, Op};
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

#[derive(Subcommand)]
enum Cmd {
    /// Create a project with one timeline and one video track
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
    /// Append a clip to V1 (media by name or id prefix)
    Add {
        project: PathBuf,
        media: String,
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
    },
    /// Ripple delete an item by its number in `show`
    Remove {
        project: PathBuf,
        index: usize,
    },
    /// Ripple-trim an item's `in` or `out` edge by seconds (negative = earlier)
    Trim {
        project: PathBuf,
        index: usize,
        edge: EdgeArg,
        #[arg(allow_negative_numbers = true)]
        secs: f64,
    },
    Undo {
        project: PathBuf,
    },
    Redo {
        project: PathBuf,
    },
    /// List the timeline
    Show {
        project: PathBuf,
    },
    /// The operation log
    Log {
        project: PathBuf,
    },
    /// Render the timeline with ffmpeg
    Render {
        project: PathBuf,
        out: PathBuf,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum EdgeArg {
    In,
    Out,
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

fn v1(tl: &Timeline) -> Result<&Track, String> {
    tl.tracks
        .iter()
        .find(|t| t.kind == TrackKind::Video)
        .map(|t| t.as_ref())
        .ok_or_else(|| "no video track".to_string())
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

fn nth_item(s: &Session, index: usize) -> Result<(Id, Item, i64), String> {
    let track = v1(timeline(s)?)?;
    let starts = track.item_starts();
    track
        .items
        .get(index)
        .map(|i| (i.id(), i.clone(), starts[index]))
        .ok_or_else(|| {
            format!(
                "no item {index}; the track has {} (numbered from 0)",
                track.items.len()
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
    let track = v1(tl)?;
    println!(
        "{}  {}x{} @ {}  length {}",
        tl.name,
        s.editor.project().settings.width,
        s.editor.project().settings.height,
        tl.rate,
        tc(tl.duration().value, tl.rate)
    );
    println!(
        "{:>3}  {:<5} {:<24} {:<12} {:<12}",
        "#", "kind", "name", "start", "length"
    );
    for (i, (item, start)) in track.items.iter().zip(track.item_starts()).enumerate() {
        let (kind, name) = match item {
            Item::Clip(c) => (if c.enabled { "clip" } else { "off" }, c.name.as_str()),
            Item::Gap(_) => ("gap", ""),
        };
        println!(
            "{i:>3}  {kind:<5} {name:<24} {:<12} {:<12}",
            tc(start, tl.rate),
            tc(item.duration().value, tl.rate)
        );
    }
    Ok(())
}

fn run(cmd: Cmd) -> Result<(), String> {
    // read-modify-write helper for the edit commands
    let edit = |path: &PathBuf,
                f: &dyn Fn(&mut Session) -> Result<String, String>|
     -> Result<(), String> {
        let mut s = Session::load(path)?;
        let msg = f(&mut s)?;
        s.save(path)?;
        println!("{msg}");
        Ok(())
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
            s.apply(Op::AddTrack {
                timeline: tl,
                index: 0,
                track: Track::new(Id::new(), "V1", TrackKind::Video),
            })?;
            s.save(&project)?;
            println!(
                "created {} ({rate} fps, {width}x{height})",
                project.display()
            );
            Ok(())
        }
        Cmd::Import { project, files } => edit(&project, &|s| {
            let mut out = Vec::new();
            for f in &files {
                let m = captaine_media::media_ref(f, Id::new())
                    .map_err(|e| format!("{}: {e}", f.display()))?;
                out.push(format!(
                    "imported {} ({} frames @ {}{})",
                    m.name,
                    m.duration.map_or(0, |d| d.value),
                    m.duration.map_or(Rational::FPS_30, |d| d.rate),
                    if m.has_audio { ", audio" } else { "" }
                ));
                s.apply(Op::AddMedia { media: m })?;
            }
            Ok(out.join("\n"))
        }),
        Cmd::Add {
            project,
            media,
            from,
            dur,
        } => edit(&project, &|s| {
            let m = find_media(s.editor.project(), &media)?;
            let tl = timeline(s)?;
            let (track, rate) = (v1(tl)?.id, tl.rate);
            let len = m.duration.ok_or("media length unknown")?;
            let src_rate = len.rate;
            let start = ticks(from, src_rate);
            let frames = match dur {
                Some(d) => ticks(d, rate),
                None => ticks((len.value - start) as f64 / src_rate.as_f64(), rate),
            };
            let index = v1(tl)?.items.len();
            let item = Item::Clip(Clip {
                id: Id::new(),
                name: m.name.clone(),
                media: m.id,
                source_start: RationalTime::new(start, src_rate),
                duration: RationalTime::new(frames, rate),
                enabled: true,
            });
            s.apply(Op::InsertItem { track, index, item })?;
            Ok(format!("added {} as item {index}", m.name))
        }),
        Cmd::Split { project, at } => edit(&project, &|s| {
            let tl = timeline(s)?;
            let rate = tl.rate;
            let t = ticks(at, tl.rate);
            let track = v1(tl)?;
            let idx = track
                .item_starts()
                .iter()
                .zip(&track.items)
                .position(|(st, it)| t > *st && t < st + it.duration().value);
            let idx = idx.ok_or("that time is not inside an item (cuts need to be between its first and last frame)")?;
            let (id, _, start) = nth_item(s, idx)?;
            s.apply(Op::Split {
                item: id,
                at: RationalTime::new(t - start, rate),
                new_id: Id::new(),
            })?;
            Ok(format!("split item {idx} at {}", tc(t, rate)))
        }),
        Cmd::Remove { project, index } => edit(&project, &|s| {
            let (id, ..) = nth_item(s, index)?;
            s.apply(Op::RemoveItem { item: id })?;
            Ok(format!("removed item {index}"))
        }),
        Cmd::Trim {
            project,
            index,
            edge,
            secs,
        } => edit(&project, &|s| {
            let rate = timeline(s)?.rate;
            let (id, ..) = nth_item(s, index)?;
            let edge = match edge {
                EdgeArg::In => Edge::In,
                EdgeArg::Out => Edge::Out,
            };
            s.apply(Op::Trim {
                item: id,
                edge,
                delta: RationalTime::new(ticks(secs, rate), rate),
            })?;
            Ok(format!("trimmed item {index}"))
        }),
        Cmd::Undo { project } => edit(&project, &|s| {
            s.undo()
                .map(|d| format!("undid: {d}"))
                .ok_or("nothing to undo".to_string())
        }),
        Cmd::Redo { project } => edit(&project, &|s| {
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
        Cmd::Render { project, out } => {
            let s = Session::load(&project)?;
            let tl = timeline(&s)?;
            captaine_export::export(s.editor.project(), tl.id, &out).map_err(|e| e.to_string())?;
            println!(
                "rendered {} ({})",
                out.display(),
                tc(tl.duration().value, tl.rate)
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
