//! Stabilising a deliberately shaky shot. Skipped when FFmpeg lacks vid.stab.

use std::process::Command;

use captaine_agent::ToolBox;
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::VideoReader;
use captaine_time::{Rational, RationalTime};
use serde_json::json;

fn has_vidstab() -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-filters"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("vidstabdetect"))
}

/// A busy picture with a magenta marker, cropped through a window that shakes.
fn shaky(path: &std::path::Path) {
    let graph = "testsrc2=s=720x420:r=24:d=3,drawbox=x=350:y=200:w=36:h=36:c=0xff00ff:t=fill,\
        crop=w=640:h=360:x='40+22*sin(n*1.7)+10*sin(n*0.31)':y='30+16*sin(n*2.3)+8*cos(n*0.5)'";
    assert!(Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", graph, "-c:v", "libx264", "-pix_fmt",
            "yuv420p", "-crf", "12"
        ])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

/// Centre of the magenta marker in each frame.
fn marker_track(path: &str, w: u32, h: u32, frames: i64) -> Vec<(f64, f64)> {
    let mut r = VideoReader::open(path, w, h, Rational::FPS_24, 0).unwrap();
    (0..frames)
        .map(|i| {
            let f = r.frame(i).unwrap();
            let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
            for y in 0..h {
                for x in 0..w {
                    let k = ((y * w + x) * 4) as usize;
                    let (r, g, b) = (f.rgba[k] as i32, f.rgba[k + 1] as i32, f.rgba[k + 2] as i32);
                    if r > 190 && b > 190 && g < 90 {
                        sx += x as f64;
                        sy += y as f64;
                        n += 1.0;
                    }
                }
            }
            (sx / n.max(1.0), sy / n.max(1.0))
        })
        .collect()
}

/// How much the marker wobbles: spread around its own slow movement.
fn jitter(track: &[(f64, f64)]) -> f64 {
    let smooth = |v: Vec<f64>| -> Vec<f64> {
        (0..v.len())
            .map(|i| {
                let (a, b) = (i.saturating_sub(10), (i + 11).min(v.len()));
                v[a..b].iter().sum::<f64>() / (b - a) as f64
            })
            .collect()
    };
    let xs: Vec<f64> = track.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = track.iter().map(|p| p.1).collect();
    let (sx, sy) = (smooth(xs.clone()), smooth(ys.clone()));
    let n = track.len() as f64;
    let var: f64 = (0..track.len())
        .map(|i| (xs[i] - sx[i]).powi(2) + (ys[i] - sy[i]).powi(2))
        .sum::<f64>()
        / n;
    var.sqrt()
}

#[test]
fn the_wobble_goes_down_and_the_edit_stays_valid() {
    if !has_vidstab() {
        eprintln!("skipped: no vid.stab");
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-stab-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("shaky.mp4");
    shaky(&video);
    let media = media_ref(&video, Id::new()).unwrap();
    let rate = Rational::FPS_24;
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    }));
    let (tl, tr) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: media.clone(),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "m", rate),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(tr, "V1", TrackKind::Video),
        },
    )
    .unwrap();
    // the clip plays 2 s from 0.5 s into the file
    let clip = Clip::new(
        Id::new(),
        "shaky",
        media.id,
        RationalTime::new(12, rate),
        RationalTime::new(48, rate),
    );
    let id = clip.id;
    ed.apply(Author::User, edit::append(ed.project(), tr, clip))
        .unwrap();
    let before = marker_track(&media.path, 640, 360, 60);
    let slice = &before[12..60];
    let wobble_before = jitter(slice);
    assert!(
        wobble_before > 8.0,
        "the test footage really shakes: {wobble_before}"
    );

    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    tools
        .call(
            "stabilize",
            &json!({"item_id": id.to_string(), "smoothing": 20}),
        )
        .unwrap();
    let p = tools.project().clone();
    p.validate().unwrap();
    let steadied = p.timeline(tl).unwrap().tracks[0].items[0]
        .as_clip()
        .unwrap()
        .clone();
    assert_ne!(
        steadied.media, media.id,
        "the clip now plays the steadied copy"
    );
    assert_eq!(
        (steadied.source_start.value, steadied.duration.value),
        (0, 48),
        "same place and length"
    );
    let copy = &p.media[&steadied.media];
    let after = marker_track(&copy.path, 640, 360, 48);
    let wobble_after = jitter(&after);
    assert!(
        wobble_after < wobble_before * 0.6,
        "steadier: {wobble_before:.1} -> {wobble_after:.1}"
    );
    // it is one undo
    let mut ed2 = Editor::new(ed.project().clone());
    for e in tools.new_ops.clone() {
        ed2.apply(Author::User, e).unwrap();
    }
    ed2.undo();
    assert_eq!(ed2.project(), ed.project());
    let _ = std::fs::remove_dir_all(dir);
}
