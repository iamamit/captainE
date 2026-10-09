//! SubRip subtitles to and from caption titles. Captions are titles in the Caption style, normally on a
//! video track named "Captions".

use captaine_model::{Id, Item, Project, Title, TitleTemplate, Track, TrackKind};
use captaine_ops::{apply, edit, Op};
use captaine_time::RationalTime;

pub const CAPTION_TRACK: &str = "Captions";

/// Every caption on the timeline as SRT cues, in time order.
pub fn export(project: &Project, timeline: Id) -> Result<String, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let fps = tl.rate.as_f64();
    let mut cues: Vec<(f64, f64, String)> = Vec::new();
    for track in &tl.tracks {
        for (item, start) in track.items.iter().zip(track.item_starts()) {
            let Item::Title(t) = item else { continue };
            let is_caption = track.name == CAPTION_TRACK
                || (t.style.template == TitleTemplate::Caption && t.shape.is_none());
            if is_caption && t.enabled && !t.text.trim().is_empty() {
                cues.push((
                    start as f64 / fps,
                    (start + t.duration.value) as f64 / fps,
                    t.text.clone(),
                ));
            }
        }
    }
    if cues.is_empty() {
        return Err("there are no captions on this timeline".into());
    }
    cues.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(captaine_speech::write_srt(&cues))
}

/// Ops that put the cues on the "Captions" track (made if missing), shifted by `offset_secs`.
pub fn import(
    project: &Project,
    timeline: Id,
    text: &str,
    offset_secs: f64,
) -> Result<Vec<Op>, String> {
    let cues = captaine_speech::parse_srt(text)?;
    caption_ops(project, timeline, &cues, offset_secs)
}

/// Caption titles for `(start, end, text)` cues given in seconds from `offset_secs` on the timeline.
pub fn caption_ops(
    project: &Project,
    timeline: Id,
    cues: &[(f64, f64, String)],
    offset_secs: f64,
) -> Result<Vec<Op>, String> {
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let rate = tl.rate;
    let fps = rate.as_f64();
    let mut scratch = project.clone();
    let mut ops: Vec<Op> = Vec::new();
    let track = match tl
        .tracks
        .iter()
        .find(|t| t.name == CAPTION_TRACK && t.kind == TrackKind::Video)
    {
        Some(t) => t.id,
        None => {
            let track = Track::new(Id::new(), CAPTION_TRACK, TrackKind::Video);
            let id = track.id;
            let op = Op::AddTrack {
                timeline,
                index: tl.tracks.len(),
                track,
            };
            scratch = apply(&scratch, &op).map_err(|e| e.to_string())?.0;
            ops.push(op);
            id
        }
    };
    for (start, end, text) in cues {
        let a = ((start + offset_secs) * fps).round() as i64;
        let b = (((end + offset_secs) * fps).round() as i64).max(a + 1);
        let mut t = Title::new(
            Id::new(),
            TitleTemplate::Caption,
            RationalTime::new(b - a, rate),
        );
        t.text = text.clone();
        t.name = format!("Caption: {}", text.lines().next().unwrap_or(""));
        let op = edit::overwrite_item(
            &scratch,
            track,
            RationalTime::new(a.max(0), rate),
            Item::Title(t),
        )
        .map_err(|e| e.to_string())?;
        scratch = apply(&scratch, &op).map_err(|e| e.to_string())?.0;
        ops.push(op);
    }
    Ok(ops)
}
