//! Renders a real timeline through ffmpeg and checks the output. Skipped when FFmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_media::{media_ref, probe};
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
        && Command::new("ffprobe").arg("-version").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// 4 s of test video at 25 fps (with or without a sine tone), 320x180.
fn make_source(path: &Path, audio: bool) {
    let mut args = vec![
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x180:rate=25:duration=4",
    ];
    if audio {
        args.extend(["-f", "lavfi", "-i", "sine=frequency=440:duration=4"]);
    }
    args.extend(["-c:v", "libx264", "-pix_fmt", "yuv420p"]);
    if audio {
        args.extend(["-c:a", "aac"]);
    }
    args.push(path.to_str().unwrap());
    assert!(Command::new("ffmpeg")
        .args(&args)
        .status()
        .unwrap()
        .success());
}

#[test]
fn cut_with_gap_and_silent_source_renders_exact_length() {
    if !have_ffmpeg() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let (a, b, out) = (scratch("a.mp4"), scratch("b.mp4"), scratch("out.mp4"));
    make_source(&a, true);
    make_source(&b, false);

    let settings = ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    };
    let mut ed = Editor::new(Project::new(settings));
    let (ma, mb) = (
        media_ref(&a, Id::new()).unwrap(),
        media_ref(&b, Id::new()).unwrap(),
    );
    assert_eq!(ma.duration.unwrap().value, 100);
    let (tl, v1) = (Id::new(), Id::new());
    let t = |v| RationalTime::new(v, Rational::FPS_30);
    for op in [
        Op::AddMedia { media: ma.clone() },
        Op::AddMedia { media: mb.clone() },
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
        },
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(v1, "V1", TrackKind::Video),
        },
    ] {
        ed.apply(Author::User, op).unwrap();
    }
    let clip = |m: &MediaRef, start: i64, dur: i64| {
        Item::Clip(Clip {
            id: Id::new(),
            name: m.name.clone(),
            media: m.id,
            source_start: RationalTime::new(start, Rational::FPS_25),
            duration: t(dur),
            enabled: true,
        })
    };
    // 1 s of A (from 1 s in), half a second of black, 1 s of silent B, a disabled clip (renders black)
    let items = [
        clip(&ma, 25, 30),
        Item::Gap(Gap {
            id: Id::new(),
            duration: t(15),
        }),
        clip(&mb, 0, 30),
        match clip(&ma, 0, 15) {
            Item::Clip(c) => Item::Clip(Clip {
                enabled: false,
                ..c
            }),
            g => g,
        },
    ];
    for (i, item) in items.into_iter().enumerate() {
        ed.apply(
            Author::User,
            Op::InsertItem {
                track: v1,
                index: i,
                item,
            },
        )
        .unwrap();
    }

    captaine_export::export(ed.project(), tl, &out).unwrap();

    let info = probe(&out).unwrap();
    assert_eq!((info.width, info.height), (640, 360));
    assert_eq!(info.fps, Rational::FPS_30);
    assert_eq!(info.frames, 90); // 30 + 15 + 30 + 15
    assert!(info.has_audio);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

#[test]
fn empty_timeline_is_an_error() {
    let mut p = Project::new(ProjectSettings::default());
    let tl = Id::new();
    let mut t = Timeline::new(tl, "x", Rational::FPS_30);
    t.tracks.push(std::sync::Arc::new(Track::new(
        Id::new(),
        "V1",
        TrackKind::Video,
    )));
    p.timelines.insert(tl, std::sync::Arc::new(t));
    assert!(matches!(
        captaine_export::plan(&p, tl, Path::new("o.mp4")),
        Err(captaine_export::ExportError::Empty)
    ));
}
