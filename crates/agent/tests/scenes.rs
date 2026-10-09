//! Scene detection against a synthetic video with hard cuts. Skipped when FFmpeg is missing.

use std::process::Command;

use captaine_agent::split_at_scenes;
use captaine_media::{detect_scenes, media_ref};
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

/// 4 s at 25 fps: black, white, dark grey, light grey for one second each (strong brightness steps).
fn colour_cuts() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("captaine-scenes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cuts.mp4");
    let src = "color=c=black:s=160x90:r=25:d=1[a];color=c=white:s=160x90:r=25:d=1[b];color=c=0x303030:s=160x90:r=25:d=1[c];color=c=0xe0e0e0:s=160x90:r=25:d=1[d];[a][b][c][d]concat=n=4:v=1:a=0";
    assert!(Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", src, "-c:v", "libx264", "-pix_fmt", "yuv420p"
        ])
        .arg(&path)
        .status()
        .unwrap()
        .success());
    path
}

#[test]
fn finds_the_cuts_and_plans_the_splits() {
    if !have_ffmpeg() {
        return;
    }
    let path = colour_cuts();
    let cuts = detect_scenes(path.to_str().unwrap(), 0.0, 4.0, 0.3).unwrap();
    assert_eq!(cuts.len(), 3, "{cuts:?}");
    for (c, want) in cuts.iter().zip([1.0, 2.0, 3.0]) {
        assert!((c - want).abs() < 0.1, "{cuts:?}");
    }
    // a window that covers only the middle two seconds sees one cut, at 1 s into the window
    let mid = detect_scenes(path.to_str().unwrap(), 1.5, 1.0, 0.3).unwrap();
    assert!(mid.len() <= 1);
    // plan and apply on a clip
    let media = media_ref(&path, Id::new()).unwrap();
    let rate = Rational::FPS_25;
    let clip = Clip::new(
        Id::new(),
        "cuts",
        media.id,
        RationalTime::new(0, media.duration.unwrap().rate),
        RationalTime::new(100, rate),
    );
    let (ops, secs) = split_at_scenes(&clip, &media, 25.0, 0.3, 0.5).unwrap();
    assert_eq!(ops.len(), 3);
    assert_eq!(secs.len(), 3);
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        ..ProjectSettings::default()
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
    ed.apply(Author::User, edit::append(ed.project(), tr, clip))
        .unwrap();
    ed.apply(
        Author::User,
        Op::Transaction {
            description: "scenes".into(),
            ops,
        },
    )
    .unwrap();
    let lens: Vec<i64> = ed.project().timeline(tl).unwrap().tracks[0]
        .items
        .iter()
        .map(|i| i.duration().value)
        .collect();
    assert_eq!(lens.len(), 4);
    assert!(lens.iter().all(|l| (*l - 25).abs() <= 2), "{lens:?}");
    // a high minimum piece length ignores them all
    let (none, _) = split_at_scenes(
        &Clip::new(
            Id::new(),
            "c",
            media.id,
            RationalTime::new(0, media.duration.unwrap().rate),
            RationalTime::new(100, rate),
        ),
        &media,
        25.0,
        0.3,
        1.5,
    )
    .unwrap();
    assert!(none.len() <= 1);
}
