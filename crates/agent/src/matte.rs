//! AI background removal: a matte video of the subject, kept in a cache and attached as an effect.

use std::path::PathBuf;

use captaine_model::{Clip, Effect, EffectKind, MediaRef};
use captaine_vision::MatteMode;

/// Where matte videos are kept.
pub fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    home.join(".captaine/mattes")
}

fn key(media: &MediaRef, clip: &Clip, mode: MatteMode) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mtime = std::fs::metadata(&media.path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (
        &media.path,
        mtime,
        clip.source_start.value,
        clip.source_span(),
        mode == MatteMode::Person,
    )
        .hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Make (or reuse) the matte video of the part of `media` that `clip` plays, and return the effect that
/// uses it. This runs Vision over every frame, so it takes a while on long clips.
pub fn remove_background(clip: &Clip, media: &MediaRef, mode: MatteMode) -> Result<Effect, String> {
    if !media.has_video {
        return Err(format!("{} has no picture", clip.name));
    }
    if let Some(why) = captaine_vision::unavailable() {
        return Err(why);
    }
    let rate = clip.source_start.rate;
    let path = cache_dir().join(format!("{}.mkv", key(media, clip, mode)));
    if !path.exists() {
        let from = clip.source_start.to_seconds_f64();
        let dur = clip.source_span() as f64 / rate.as_f64();
        captaine_vision::matte_video(&media.path, from, dur, rate.as_f64(), mode, &path)?;
    }
    let mut e = Effect::new(EffectKind::Matte);
    e.text = path.to_string_lossy().into_owned();
    e.params[3] = clip.source_start.value as f64;
    Ok(e)
}
