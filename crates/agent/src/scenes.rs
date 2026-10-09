use captaine_model::{Clip, Id, MediaRef};
use captaine_ops::Op;
use captaine_time::RationalTime;

/// Ops that cut `clip` wherever the picture changes abruptly, plus the cut times in seconds from the
/// clip's start. Cuts closer than `min_secs` to each other or to the ends are ignored.
pub fn split_at_scenes(
    clip: &Clip,
    media: &MediaRef,
    fps: f64,
    threshold: f64,
    min_secs: f64,
) -> Result<(Vec<Op>, Vec<f64>), String> {
    if !media.has_video {
        return Err(format!("{} has no picture to analyse", clip.name));
    }
    let src = clip.source_start.to_seconds_f64();
    // the source plays at `speed`: source seconds -> timeline seconds
    let speed = if clip.retime.freeze {
        return Err("a frozen clip has no scenes".into());
    } else {
        clip.retime.mean_speed()
    };
    let span = clip.source_span() as f64 / clip.source_start.rate.as_f64();
    let found = captaine_media::detect_scenes(&media.path, src, span, threshold)
        .map_err(|e| e.to_string())?;
    let total = clip.duration.value;
    let mut ticks: Vec<i64> = Vec::new();
    for t in found {
        let local = if clip.retime.backwards() { span - t } else { t };
        let tick = (local / speed * fps).round() as i64;
        let far = |a: i64| (a as f64 / fps) >= min_secs;
        if tick > 0
            && tick < total
            && far(tick)
            && far(total - tick)
            && ticks.last().map_or(true, |p| far(tick - p))
        {
            ticks.push(tick);
        }
    }
    ticks.sort_unstable();
    ticks.dedup();
    let secs = ticks.iter().map(|t| *t as f64 / fps).collect();
    // from the end, so each earlier cut point stays valid and the original clip stays the left piece
    let ops = ticks
        .iter()
        .rev()
        .map(|t| Op::Split {
            item: clip.id,
            at: RationalTime::new(*t, clip.duration.rate),
            new_id: Id::new(),
        })
        .collect();
    Ok((ops, secs))
}
