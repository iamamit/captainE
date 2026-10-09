//! Edits driven by where things are in the picture: attach a title to a moving subject, and reframe a
//! clip for another aspect ratio so its subject stays in view.

use captaine_model::{Clip, Id, Interp, Keyframe, MediaRef, Title, VideoProps};
use captaine_ops::Op;
use captaine_vision::{Rect, Scratch};

/// How many pictures per second are analysed. Motion between them is interpolated by the keyframes.
const SAMPLE_FPS: f64 = 6.0;

/// Frames of the part of `media` that `clip` plays, sampled for analysis. Returns the directory guard and
/// the number of frames; sample `i` is at `i / SAMPLE_FPS` seconds of source after the clip's first frame.
fn sample(clip: &Clip, media: &MediaRef) -> Result<(Scratch, usize), String> {
    if !media.has_video {
        return Err(format!("{} has no picture", clip.name));
    }
    if let Some(why) = captaine_vision::unavailable() {
        return Err(why);
    }
    let span = clip.source_span() as f64 / clip.source_start.rate.as_f64();
    let n = ((span * SAMPLE_FPS).floor() as usize).max(2);
    let dir = Scratch::new("follow")?;
    let got = captaine_vision::extract_frames(
        &media.path,
        clip.source_start.to_seconds_f64(),
        n,
        SAMPLE_FPS,
        640,
        &dir.0.join("f"),
    )?;
    if got < 2 {
        return Err("too few frames could be read".into());
    }
    Ok((dir, got))
}

/// Timeline ticks into the clip at which sample `i` is shown.
fn tick_of(clip: &Clip, i: usize, fps: f64) -> i64 {
    let src_secs = i as f64 / SAMPLE_FPS;
    let secs = src_secs / clip.retime.speed;
    let secs = if clip.retime.backwards() {
        clip.source_span() as f64 / clip.source_start.rate.as_f64() / clip.retime.speed - secs
    } else {
        secs
    };
    (secs * fps).round() as i64
}

fn smooth(v: &[f64], radius: usize) -> Vec<f64> {
    (0..v.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(radius), (i + radius + 1).min(v.len()));
            v[a..b].iter().sum::<f64>() / (b - a) as f64
        })
        .collect()
}

/// A subject box to start from: the biggest face, else what attracts the eye, else the middle.
fn first_subject(frames: &std::path::Path) -> Result<Rect, String> {
    if let Some(b) = captaine_vision::faces(frames)?.first().and_then(|f| {
        f.iter()
            .max_by(|a, b| (a[2] * a[3]).total_cmp(&(b[2] * b[3])))
            .copied()
    }) {
        return Ok(b);
    }
    if let Some(b) = captaine_vision::subjects(frames)?
        .first()
        .copied()
        .flatten()
    {
        return Ok(b);
    }
    if let Some(b) = captaine_vision::saliency(frames)?
        .first()
        .and_then(|f| f.first().copied())
    {
        return Ok(b);
    }
    Ok([0.375, 0.375, 0.25, 0.25])
}

/// Follow a subject through `clip` and move `target` (a title or layer) with it. `start_box` is where the
/// subject is in the clip's first frame (fractions, origin top-left); None picks one. `canvas` is the
/// sequence frame size and `target_start` / `clip_start` are timeline ticks. The keyframes sit on the
/// target's position, in canvas pixels from the centre.
#[allow(clippy::too_many_arguments)]
pub fn track_attach(
    clip: &Clip,
    media: &MediaRef,
    target: &Title,
    clip_start: i64,
    target_start: i64,
    canvas: (u32, u32),
    fps: f64,
    start_box: Option<Rect>,
) -> Result<Vec<Op>, String> {
    let (scratch, n) = sample(clip, media)?;
    let frames = scratch.0.join("f");
    let start = match start_box {
        Some(b) => b,
        None => first_subject(&frames)?,
    };
    let path = captaine_vision::track(&frames, start)?;
    let (cw, ch) = (canvas.0 as f64, canvas.1 as f64);
    let (mw, mh) = (media.width.max(1) as f64, media.height.max(1) as f64);
    let fit = (cw / mw).min(ch / mh);
    let (fw, fh) = (
        mw * fit * clip.video.transform.scale.0.abs(),
        mh * fit * clip.video.transform.scale.1.abs(),
    );
    let (px, py) = clip.video.transform.position;
    let xs: Vec<f64> = path
        .iter()
        .take(n)
        .map(|(b, _)| px + (b[0] + b[2] / 2.0 - 0.5) * fw)
        .collect();
    let ys: Vec<f64> = path
        .iter()
        .take(n)
        .map(|(b, _)| py + (b[1] + b[3] / 2.0 - 0.5) * fh)
        .collect();
    let (xs, ys) = (smooth(&xs, 1), smooth(&ys, 1));
    let dur = target.duration.value;
    let mut kx = Vec::new();
    let mut ky = Vec::new();
    for i in 0..xs.len() {
        let t = clip_start + tick_of(clip, i, fps) - target_start;
        if t < 0 || t >= dur {
            continue;
        }
        kx.push(Keyframe {
            t,
            v: xs[i],
            interp: Interp::Linear,
        });
        ky.push(Keyframe {
            t,
            v: ys[i],
            interp: Interp::Linear,
        });
    }
    if kx.len() < 2 {
        return Err(
            "the title and the clip hardly overlap in time, so there is nothing to follow".into(),
        );
    }
    Ok(vec![
        Op::SetAnimTrack {
            item: target.id,
            prop: "position.x".into(),
            keys: kx,
        },
        Op::SetAnimTrack {
            item: target.id,
            prop: "position.y".into(),
            keys: ky,
        },
    ])
}

/// How to keep the subject: a face if there are faces in at least a third of the samples, else attention.
fn subject_path(frames: &std::path::Path, n: usize) -> Result<Vec<(f64, f64)>, String> {
    let faces = captaine_vision::faces(frames)?;
    let with_face = faces.iter().filter(|f| !f.is_empty()).count();
    let boxes: Vec<Option<Rect>> = if with_face * 3 >= faces.len().max(1) {
        faces
            .iter()
            .map(|f| {
                f.iter()
                    .max_by(|a, b| (a[2] * a[3]).total_cmp(&(b[2] * b[3])))
                    .copied()
            })
            .collect()
    } else {
        // no faces: the main subject, found by lifting it out of the picture; failing that, attention
        let subjects = captaine_vision::subjects(frames)?;
        if subjects.iter().filter(|b| b.is_some()).count() * 3 >= subjects.len().max(1) {
            subjects
        } else {
            captaine_vision::saliency(frames)?
                .iter()
                .map(|f| f.first().copied())
                .collect()
        }
    };
    // holes (no detection) repeat the last known place
    let mut last = (0.5, 0.5);
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(n);
    for b in boxes.into_iter().take(n) {
        if let Some(b) = b {
            last = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
        }
        out.push(last);
    }
    while out.len() < n {
        out.push(last);
    }
    // a steady camera move rather than a nervous one
    let xs = smooth(&out.iter().map(|p| p.0).collect::<Vec<_>>(), 2);
    let ys = smooth(&out.iter().map(|p| p.1).collect::<Vec<_>>(), 2);
    Ok(xs.into_iter().zip(ys).collect())
}

/// Ops that fill a `canvas`-sized frame with `clip`'s picture, panning so the subject stays in view.
pub fn reframe(
    clip: &Clip,
    media: &MediaRef,
    canvas: (u32, u32),
    fps: f64,
) -> Result<Vec<Op>, String> {
    let (scratch, n) = sample(clip, media)?;
    let path = subject_path(&scratch.0.join("f"), n)?;
    let (cw, ch) = (canvas.0 as f64, canvas.1 as f64);
    let (mw, mh) = (media.width.max(1) as f64, media.height.max(1) as f64);
    let (rx, ry) = (cw / mw, ch / mh);
    let fit = rx.min(ry);
    let fill = rx.max(ry) / fit;
    let (fw, fh) = (mw * fit * fill, mh * fit * fill);
    let (slack_x, slack_y) = (((fw - cw) / 2.0).max(0.0), ((fh - ch) / 2.0).max(0.0));
    let video = VideoProps {
        transform: captaine_model::Transform {
            scale: (fill, fill),
            position: (0.0, 0.0),
            ..clip.video.transform
        },
        ..clip.video
    };
    let mut kx = Vec::new();
    let mut ky = Vec::new();
    for (i, (cx, cy)) in path.iter().enumerate() {
        let t = tick_of(clip, i, fps).clamp(0, clip.duration.value - 1);
        // moving the picture the other way brings the subject to the middle, never past the frame's edge
        let x = (-(cx - 0.5) * fw).clamp(-slack_x, slack_x);
        let y = (-(cy - 0.5) * fh).clamp(-slack_y, slack_y);
        if kx.last().is_some_and(|k: &Keyframe| k.t == t) {
            continue;
        }
        kx.push(Keyframe {
            t,
            v: x,
            interp: Interp::EaseInOut,
        });
        ky.push(Keyframe {
            t,
            v: y,
            interp: Interp::EaseInOut,
        });
    }
    let mut ops = vec![Op::SetVideo {
        item: clip.id,
        video,
    }];
    if slack_x > 1.0 && kx.len() >= 2 {
        ops.push(Op::SetAnimTrack {
            item: clip.id,
            prop: "position.x".into(),
            keys: kx,
        });
    }
    if slack_y > 1.0 && ky.len() >= 2 {
        ops.push(Op::SetAnimTrack {
            item: clip.id,
            prop: "position.y".into(),
            keys: ky,
        });
    }
    let _ = Id::new;
    Ok(ops)
}
