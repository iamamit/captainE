//! Loudness: measure a clip or the whole mix in LUFS and set levels to hit a target.

use std::sync::Arc;

use captaine_audio::{measure, Loudness, Mixer};
use captaine_model::{Clip, Id, Item, Project, Timeline, Track, TrackKind};
use captaine_ops::Op;

/// Loudness of the whole sequence's mix.
pub fn sequence_loudness(project: &Project, timeline: Id) -> Result<Loudness, String> {
    let mut mixer = Mixer::new(project, timeline).map_err(|e| e.to_string())?;
    let n = mixer.total_samples();
    if n <= 0 {
        return Err("the sequence is empty".into());
    }
    let sr = project.settings.sample_rate;
    let mut all = Vec::with_capacity(n as usize * 2);
    let mut at = 0;
    while at < n {
        let block = (sr as i64).min(n - at) as usize;
        all.extend(mixer.render(at, block).map_err(|e| e.to_string())?);
        at += block as i64;
    }
    measure(&all, sr)
}

/// Loudness of one clip on its own (with its effects and fades, without the track it sits on).
pub fn clip_loudness(project: &Project, clip: &Clip) -> Result<Loudness, String> {
    let tl_rate = clip.duration.rate;
    let mut only = project.clone();
    only.timelines.clear();
    let mut t = Timeline::new(Id::new(), "measure", tl_rate);
    let mut track = Track::new(Id::new(), "A", TrackKind::Audio);
    let mut c = clip.clone();
    c.transition = None;
    c.link = None;
    c.enabled = true;
    track.items.push(Item::Clip(c));
    t.tracks.push(Arc::new(track));
    let id = t.id;
    only.timelines.insert(id, Arc::new(t));
    sequence_loudness(&only, id)
}

/// The change in dB that brings `measured` to `target` LUFS, kept inside what is sensible.
pub fn gain_for(measured: &Loudness, target: f64) -> Result<f64, String> {
    if !measured.integrated_lufs.is_finite() {
        return Err("it is silent, so there is nothing to bring up".into());
    }
    Ok((target - measured.integrated_lufs).clamp(-40.0, 40.0))
}

/// Ops that bring one clip to `target` LUFS by changing its volume.
pub fn normalize_clip(project: &Project, clip: &Clip, target: f64) -> Result<Vec<Op>, String> {
    let l = clip_loudness(project, clip)?;
    let g = gain_for(&l, target)?;
    let mut audio = clip.audio;
    audio.gain_db = (audio.gain_db + g).clamp(-60.0, 12.0);
    Ok(vec![Op::SetAudio {
        item: clip.id,
        audio,
    }])
}

/// Ops that bring the whole sequence to `target` LUFS by moving every track's level the same amount.
pub fn normalize_sequence(
    project: &Project,
    timeline: Id,
    target: f64,
) -> Result<(Vec<Op>, f64), String> {
    let l = sequence_loudness(project, timeline)?;
    let g = gain_for(&l, target)?;
    let tl = project.timeline(timeline).ok_or("timeline not found")?;
    let ops = tl
        .tracks
        .iter()
        .filter(|t| t.items.iter().any(|i| i.as_clip().is_some()))
        .map(|t| Op::SetTrack {
            track: t.id,
            enabled: t.enabled,
            gain_db: (t.gain_db + g).clamp(-60.0, 12.0),
        })
        .collect();
    Ok((ops, g))
}
