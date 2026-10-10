//! Steadying shaky footage with FFmpeg's two-pass vid.stab: the part of the file the clip plays is
//! analysed, then re-rendered smoothed, and the clip is pointed at the steadied copy.

use std::path::PathBuf;
use std::process::Command;

use captaine_model::{Clip, Item, MediaRef};
use captaine_ops::Op;
use captaine_time::RationalTime;

pub fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    home.join(".captaine/stabilized")
}

fn key(media: &MediaRef, clip: &Clip, smoothing: u32) -> String {
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
        smoothing,
    )
        .hash(&mut h);
    format!("{:016x}", h.finish())
}

fn run(args: &[String]) -> Result<(), String> {
    let out = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-nostdin"])
        .args(args)
        .output()
        .map_err(|e| format!("cannot run ffmpeg: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Ops that swap `clip` for a steadied copy of the footage it plays. `smoothing` is how many frames each
/// side the camera path is averaged over (about 10 for handheld, 30 for a walking shot).
pub fn stabilize(clip: &Clip, media: &MediaRef, smoothing: u32) -> Result<Vec<Op>, String> {
    if !media.has_video || media.nested_timeline().is_some() {
        return Err(format!("{} has no footage to steady", clip.name));
    }
    let rate = clip.source_start.rate;
    let frames = clip.source_span();
    let from = clip.source_start.to_seconds_f64();
    let dur = frames as f64 / rate.as_f64();
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = key(media, clip, smoothing);
    let out = dir.join(format!("{name}.mp4"));
    if !out.exists() {
        let trf = dir.join(format!("{name}.trf"));
        let seek = vec![
            "-ss".to_string(),
            format!("{from:.6}"),
            "-t".into(),
            format!("{dur:.6}"),
            "-i".into(),
            media.path.clone(),
        ];
        let mut detect = seek.clone();
        detect.extend([
            "-an".into(),
            "-vf".into(),
            format!(
                "vidstabdetect=shakiness=6:accuracy=15:result={}",
                captaine_media::filter_path(&trf)
            ),
            "-f".into(),
            "null".into(),
            "-".into(),
        ]);
        run(&detect)?;
        let mut render = seek;
        render.extend([
            "-vf".into(),
            format!(
                "vidstabtransform=smoothing={smoothing}:input={}:optzoom=1:zoom=0,unsharp=5:5:0.6:3:3:0.3",
                captaine_media::filter_path(&trf)
            ),
            "-frames:v".into(),
            frames.to_string(),
            "-c:v".into(),
            "libx264".into(),
            "-crf".into(),
            "14".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-c:a".into(),
            "aac".into(),
            out.display().to_string(),
        ]);
        let r = run(&render);
        let _ = std::fs::remove_file(&trf);
        r?;
    }
    let steady =
        captaine_media::media_ref(&out, captaine_model::Id::new()).map_err(|e| e.to_string())?;
    let steady_rate = steady.duration.map_or(rate, |d| d.rate);
    let mut new_clip = clip.clone();
    new_clip.media = steady.id;
    new_clip.source_start = RationalTime::new(0, steady_rate);
    if steady
        .duration
        .is_some_and(|d| new_clip.source_span() > d.value)
    {
        return Err("the steadied copy came out shorter than the clip".into());
    }
    Ok(vec![
        Op::AddMedia { media: steady },
        Op::ReplaceRaw {
            item: clip.id,
            with: Item::Clip(new_clip),
        },
    ])
}
