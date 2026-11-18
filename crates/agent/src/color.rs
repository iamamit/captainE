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

/// The colour of the backdrop to key out, picked from the middle of the clip like an eyedropper: among the
/// clearly coloured pixels whose hue is within `reach` degrees of `near_hue` (120 = green, 240 = blue), the most
/// common 20-degree band, averaged. `None` when nothing that colourful covers a real part of the picture.
pub fn key_colour(
    clip: &Clip,
    media: &MediaRef,
    near_hue: f64,
    reach: f64,
) -> Result<Option<[u8; 3]>, String> {
    if !media.has_video || media.nested_timeline().is_some() {
        return Err(format!("{} has no footage to look at", clip.name));
    }
    let rate = clip.source_start.rate;
    let (w, h) = (media.width.max(2), media.height.max(2));
    let rw = 320.min(w) & !1;
    let rh = ((h as f64 * rw as f64 / w as f64).round() as u32).max(2) & !1;
    let f = clip.source_start.value + clip.source_span() / 2;
    let mut r = VideoReader::open(&media.path, rw, rh, rate, f).map_err(|e| e.to_string())?;
    let frame = r.frame(f).map_err(|e| e.to_string())?;
    let mut bins = [(0usize, [0.0f64; 3]); 18];
    for p in frame.rgba.chunks_exact(4) {
        let (r, g, b) = (
            p[0] as f64 / 255.0,
            p[1] as f64 / 255.0,
            p[2] as f64 / 255.0,
        );
        let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
        if mx < 0.15 || (mx - mn) / mx < 0.3 {
            continue;
        }
        let d = mx - mn;
        let hue = 60.0
            * if mx == r {
                ((g - b) / d).rem_euclid(6.0)
            } else if mx == g {
                (b - r) / d + 2.0
            } else {
                (r - g) / d + 4.0
            };
        let off = (hue - near_hue + 540.0).rem_euclid(360.0) - 180.0;
        if off.abs() > reach {
            continue;
        }
        let bin = &mut bins[(hue / 20.0) as usize % 18];
        bin.0 += 1;
        bin.1[0] += p[0] as f64;
        bin.1[1] += p[1] as f64;
        bin.1[2] += p[2] as f64;
    }
    let total = frame.rgba.len() / 4;
    let best = bins.iter().max_by_key(|b| b.0).unwrap();
    // a backdrop covers a real part of the picture, not a few pixels
    if best.0 * 50 < total {
        return Ok(None);
    }
    Ok(Some(best.1.map(|v| (v / best.0 as f64).round() as u8)))
}
