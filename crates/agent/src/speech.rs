//! Edits that start from what is said: captions, cutting hesitations, cutting chosen words.

use captaine_model::{Clip, Id, MediaRef};
use captaine_ops::Op;
use captaine_speech::Segment;
use captaine_time::RationalTime;

/// Transcribe the stretch of source a clip plays. Word times count from the clip's first frame, on the
/// timeline (so a clip at double speed reports half the source times).
pub fn clip_transcript(
    clip: &Clip,
    media: &MediaRef,
    language: Option<&str>,
) -> Result<Vec<Segment>, String> {
    if !media.has_audio {
        return Err(format!("{} has no sound to transcribe", clip.name));
    }
    if clip.retime.backwards() || clip.retime.freeze {
        return Err("a reversed or frozen clip has no speech to read".into());
    }
    let src = clip.source_start.to_seconds_f64();
    let span = clip.source_span() as f64 / clip.source_start.rate.as_f64();
    let mut segs = captaine_speech::transcribe(&media.path, src, span, language)?;
    let speed = clip.retime.mean_speed();
    if speed != 1.0 {
        for s in &mut segs {
            s.start /= speed;
            s.end /= speed;
            for w in &mut s.words {
                w.start /= speed;
                w.end /= speed;
            }
        }
    }
    Ok(segs)
}

/// Ops that remove the given ranges (seconds from the clip's start) from the clip; the rest closes up.
/// Returns the ops and the seconds taken out.
pub fn cut_ranges(clip: &Clip, fps: f64, ranges: &[(f64, f64)]) -> (Vec<Op>, f64) {
    let mut cuts: Vec<(i64, i64)> = ranges
        .iter()
        .map(|(a, b)| ((a * fps).round() as i64, (b * fps).round() as i64))
        .map(|(a, b)| (a.max(0), b.min(clip.duration.value)))
        .filter(|(a, b)| b > a)
        .collect();
    cuts.sort_unstable();
    // from the end, so earlier cut points stay valid; each split leaves the original clip as the left piece
    let mut ops = Vec::new();
    let mut removed = 0;
    for (a, b) in cuts.iter().rev() {
        removed += b - a;
        if *b < clip.duration.value {
            ops.push(Op::Split {
                item: clip.id,
                at: RationalTime::new(*b, clip.duration.rate),
                new_id: Id::new(),
            });
        }
        if *a > 0 {
            let mid = Id::new();
            ops.push(Op::Split {
                item: clip.id,
                at: RationalTime::new(*a, clip.duration.rate),
                new_id: mid,
            });
            ops.push(Op::RemoveItem { item: mid });
        } else {
            // the cut reaches the clip's first frame: the left piece is the removed one
            ops.push(Op::RemoveItem { item: clip.id });
        }
    }
    (ops, removed as f64 / fps)
}
