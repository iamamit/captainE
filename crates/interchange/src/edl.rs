//! CMX3600 edit decision lists.

use captaine_model::{Clip, Id, Item, Project, Retime, TrackKind};
use captaine_ops::{apply, edit, Op};
use captaine_time::{Rational, RationalTime, Rounding, Timecode};

fn tc(frames: i64, rate: Rational, drop: bool) -> String {
    Timecode::from_frames(frames.max(0), rate, drop)
        .map(|t| t.to_string())
        .unwrap_or_else(|_| "00:00:00:00".into())
}

/// Reel names are at most eight letters or digits.
fn reel(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let mut r: String = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_uppercase();
    if r.is_empty() {
        r = "AX".into();
    }
    r
}

/// The timeline as a CMX3600 list: one event per clip, video tracks first (bottom to top), then audio.
pub fn export(project: &Project, timeline: Id, drop_frame: bool) -> Result<String, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let rate = tl.rate;
    let drop = drop_frame && rate.is_ntsc();
    let mut out = format!(
        "TITLE: {}\nFCM: {}\n\n",
        tl.name,
        if drop { "DROP FRAME" } else { "NON-DROP FRAME" }
    );
    let mut n = 0;
    for kind in [TrackKind::Video, TrackKind::Audio] {
        for (ti, track) in tl.tracks.iter().enumerate().filter(|(_, t)| t.kind == kind) {
            for (item, start) in track.items.iter().zip(track.item_starts()) {
                let Item::Clip(c) = item else { continue };
                let Some(m) = project.media.get(&c.media) else {
                    continue;
                };
                n += 1;
                // source times in frames of the timeline rate
                let to_tl = |frames: i64| {
                    RationalTime::new(frames, c.source_start.rate)
                        .rescale(rate, Rounding::Nearest)
                        .map_or(0, |t| t.value)
                };
                let (s_in, s_out) = (
                    to_tl(c.source_start.value),
                    to_tl(c.source_start.value + c.source_span()),
                );
                let kindc = if kind == TrackKind::Video { "V" } else { "A" };
                let r = reel(&m.name);
                out += &format!(
                    "{n:03}  {r:<8} {kindc:<5} C        {} {} {} {}\n",
                    tc(s_in, rate, drop),
                    tc(s_out, rate, drop),
                    tc(start, rate, drop),
                    tc(start + c.duration.value, rate, drop),
                );
                if c.retime.mean_speed() != 1.0 && !c.retime.freeze {
                    let fps = rate.as_f64()
                        * c.retime.mean_speed()
                        * if c.retime.reverse { -1.0 } else { 1.0 };
                    out += &format!(
                        "M2   {r:<8} {fps:>7.1}                {}\n",
                        tc(s_in, rate, drop)
                    );
                }
                out += &format!("* FROM CLIP NAME: {}\n", m.name);
                out += &format!("* TRACK: {}{}\n", kindc, ti + 1);
                if !c.name.is_empty() && c.name != m.name {
                    out += &format!("* COMMENT: {}\n", c.name);
                }
            }
        }
    }
    for m in &tl.markers {
        let color = ["BLUE", "GREEN", "YELLOW", "RED", "PURPLE", "CYAN"][(m.color as usize).min(5)];
        out += &format!("* LOC: {} {color:<7} {}\n", tc(m.tick, rate, drop), m.name);
    }
    Ok(out)
}

/// Parsed, not yet placed.
struct Event {
    reel: String,
    kind: char,
    src_in: i64,
    src_out: i64,
    rec_in: i64,
    rec_out: i64,
    clip_name: Option<String>,
    speed: Option<f64>,
    /// 1-based position of the track in the timeline, from a `* TRACK:` comment
    track_no: Option<usize>,
}

/// Ops that place the events of `text` on `timeline`, looking the media up by name among the project's
/// media. Unknown media are reported together.
pub fn import(project: &Project, timeline: Id, text: &str) -> Result<Vec<Op>, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let rate = tl.rate;
    let mut drop = false;
    let mut events: Vec<Event> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(r) = l.strip_prefix("FCM:") {
            drop = r.contains("DROP") && !r.contains("NON");
        } else if let Some(name) = l.strip_prefix("* FROM CLIP NAME:") {
            if let Some(e) = events.last_mut() {
                e.clip_name = Some(name.trim().to_string());
            }
        } else if let Some(t) = l.strip_prefix("* TRACK:") {
            if let Some(e) = events.last_mut() {
                e.track_no = t
                    .trim()
                    .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                    .parse()
                    .ok();
            }
        } else if l.starts_with("M2") {
            let f: Vec<&str> = l.split_whitespace().collect();
            if let (Some(e), Some(fps)) = (
                events.last_mut(),
                f.get(2).and_then(|x| x.parse::<f64>().ok()),
            ) {
                e.speed = Some(fps / rate.as_f64());
            }
        } else if l.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 8 || f[3] != "C" {
                continue; // only cuts; dissolves and wipes are not read
            }
            let parse = |s: &str| -> Result<i64, String> {
                let mut t = s.to_string();
                if drop {
                    t = t.replace(':', ";").replacen(';', ":", 2);
                }
                Timecode::parse(&t, rate)
                    .and_then(|t| t.to_frames())
                    .map_err(|e| format!("bad timecode {s}: {e}"))
            };
            let n = f.len();
            events.push(Event {
                reel: f[1].to_string(),
                kind: f[2].chars().next().unwrap_or('V'),
                src_in: parse(f[n - 4])?,
                src_out: parse(f[n - 3])?,
                rec_in: parse(f[n - 2])?,
                rec_out: parse(f[n - 1])?,
                clip_name: None,
                speed: None,
                track_no: None,
            });
        }
    }
    if events.is_empty() {
        return Err("no cut events found in the list".into());
    }
    let mut scratch = project.clone();
    let mut ops = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for e in events {
        let want = e.clip_name.clone().unwrap_or_else(|| e.reel.clone());
        let stem = |s: &str| {
            s.rsplit_once('.')
                .map_or(s.to_string(), |(a, _)| a.to_string())
        };
        let media = project
            .media
            .values()
            .find(|m| m.name == want || stem(&m.name) == stem(&want) || reel(&m.name) == e.reel);
        let Some(media) = media else {
            if !missing.contains(&want) {
                missing.push(want);
            }
            continue;
        };
        let want_kind = if e.kind == 'A' {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        let hinted = e
            .track_no
            .and_then(|n| tl.tracks.get(n.checked_sub(1)?))
            .filter(|t| t.kind == want_kind);
        let Some(track) = hinted.or_else(|| tl.tracks.iter().find(|t| t.kind == want_kind)) else {
            return Err(format!(
                "the timeline has no {} track",
                if e.kind == 'A' { "audio" } else { "video" }
            ));
        };
        let src_rate = media.duration.map_or(rate, |d| d.rate);
        let from_tl = |frames: i64| {
            RationalTime::new(frames, rate)
                .rescale(src_rate, Rounding::Nearest)
                .map_or(0, |t| t.value)
        };
        let mut clip = Clip::new(
            Id::new(),
            media.name.clone(),
            media.id,
            RationalTime::new(from_tl(e.src_in), src_rate),
            RationalTime::new((e.rec_out - e.rec_in).max(1), rate),
        );
        if let Some(sp) = e.speed {
            clip.retime = Retime {
                speed: sp.abs(),
                reverse: sp < 0.0,
                freeze: false,
                ..Retime::default()
            };
        }
        let _ = e.src_out;
        let op = edit::overwrite(&scratch, track.id, RationalTime::new(e.rec_in, rate), clip)
            .map_err(|x| x.to_string())?;
        scratch = apply(&scratch, &op).map_err(|x| x.to_string())?.0;
        ops.push(op);
    }
    if !missing.is_empty() {
        return Err(format!(
            "these clips are not in the project, import them first: {}",
            missing.join(", ")
        ));
    }
    Ok(ops)
}
