//! Auto colour and shot matching: measure a few frames of a clip and suggest a grade.

use captaine_model::{Clip, ColorProps, MediaRef};
use captaine_render::{auto_correction, match_correction, ColorStats, VideoReader};

/// Measure five frames spread over the part of the file the clip plays.
pub fn clip_stats(clip: &Clip, media: &MediaRef) -> Result<ColorStats, String> {
    if !media.has_video || media.nested_timeline().is_some() {
        return Err(format!("{} has no footage to measure", clip.name));
    }
    let rate = clip.source_start.rate;
    let (w, h) = (media.width.max(2), media.height.max(2));
    let rw = 320.min(w) & !1;
    let rh = ((h as f64 * rw as f64 / w as f64).round() as u32).max(2) & !1;
    let span = clip.source_span();
    let mut frames = Vec::new();
    for i in 0..5 {
        let f = clip.source_start.value + ((i as f64 + 0.5) / 5.0 * span as f64) as i64;
        let mut r = VideoReader::open(&media.path, rw, rh, rate, f).map_err(|e| e.to_string())?;
        frames.push(r.frame(f).map_err(|e| e.to_string())?);
    }
    let refs: Vec<&captaine_render::Frame8> = frames.iter().map(|f| f.as_ref()).collect();
    ColorStats::of(&refs).ok_or_else(|| "the picture is too dark or empty to measure".into())
}

/// The clip's colour settings after an automatic correction.
pub fn auto_color(clip: &Clip, media: &MediaRef) -> Result<ColorProps, String> {
    Ok(auto_correction(&clip_stats(clip, media)?))
}

/// The clip's colour settings after matching its look to `reference`.
pub fn match_color(
    clip: &Clip,
    media: &MediaRef,
    reference: &Clip,
    ref_media: &MediaRef,
) -> Result<ColorProps, String> {
    Ok(match_correction(
        &clip_stats(reference, ref_media)?,
        &clip_stats(clip, media)?,
    ))
}
