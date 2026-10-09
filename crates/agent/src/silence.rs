use captaine_model::{Clip, Id, MediaRef};
use captaine_ops::Op;
use captaine_time::RationalTime;

/// The ops that cut the silences out, and the `(start, end)` seconds removed (relative to the clip start).
pub type SilencePlan = (Vec<Op>, Vec<(f64, f64)>);

/// Plan cutting the silent stretches out of `clip`: returns the ops and the `(start, end)` timeline seconds
/// (relative to the clip's start) that are removed. Each cut keeps `keep_secs` of room on either side.
pub fn remove_silences(
    clip: &Clip,
    media: &MediaRef,
    fps: f64,
    threshold_db: f32,
    min_secs: f64,
    keep_secs: f64,
) -> Result<SilencePlan, String> {
    let src = clip.source_start.to_seconds_f64();
    let dur = clip.duration.value as f64 / fps;
    let found = captaine_audio::find_silences(&media.path, src, dur, threshold_db, min_secs)
        .map_err(|e| e.to_string())?;
    let mut cuts: Vec<(i64, i64)> = found
        .iter()
        .filter(|(a, b)| b - a - 2.0 * keep_secs >= 0.15)
        .map(|(a, b)| {
            (
                ((a + keep_secs) * fps).round() as i64,
                ((b - keep_secs) * fps).round() as i64,
            )
        })
        .filter(|(a, b)| *a > 0 && *b < clip.duration.value && b > a)
        .collect();
    // work from the end so earlier cut points stay valid; each split leaves the original clip as the left piece
    cuts.reverse();
    let mut ops = Vec::new();
    for (a, b) in &cuts {
        let mid = Id::new();
        ops.push(Op::Split {
            item: clip.id,
            at: RationalTime::new(*b, clip.duration.rate),
            new_id: Id::new(),
        });
        ops.push(Op::Split {
            item: clip.id,
            at: RationalTime::new(*a, clip.duration.rate),
            new_id: mid,
        });
        ops.push(Op::RemoveItem { item: mid });
    }
    let removed = cuts
        .iter()
        .rev()
        .map(|(a, b)| (*a as f64 / fps, *b as f64 / fps))
        .collect();
    Ok((ops, removed))
}
