//! End to end: build a project, export it, check the file. Skipped when FFmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_export::*;
use captaine_media::{media_ref, probe};
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-export-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

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

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, Rational::FPS_30)
}

/// Frame md5s of the decoded output, to compare two renders independent of container details.
fn frame_hashes(path: &Path) -> String {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

struct Built {
    ed: Editor,
    tl: Id,
}

fn build(a: &Path, b: &Path) -> Built {
    let settings = ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    };
    let mut ed = Editor::new(Project::new(settings));
    let (ma, mb) = (
        media_ref(a, Id::new()).unwrap(),
        media_ref(b, Id::new()).unwrap(),
    );
    assert_eq!(
        (ma.width, ma.height, ma.duration.unwrap().value),
        (320, 180, 100)
    );
    let (tl, v1, v2, a1) = (Id::new(), Id::new(), Id::new(), Id::new());
    ed.apply(Author::User, Op::AddMedia { media: ma.clone() })
        .unwrap();
    ed.apply(Author::User, Op::AddMedia { media: mb.clone() })
        .unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
        },
    )
    .unwrap();
    for (i, (id, kind)) in [
        (v1, TrackKind::Video),
        (v2, TrackKind::Video),
        (a1, TrackKind::Audio),
    ]
    .into_iter()
    .enumerate()
    {
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track: Track::new(id, format!("T{i}"), kind),
            },
        )
        .unwrap();
    }
    let clip = |m: &MediaRef, rate: Rational, start: i64, dur: i64| {
        Clip::new(
            Id::new(),
            m.name.clone(),
            m.id,
            RationalTime::new(start, rate),
            t(dur),
        )
    };
    let r25 = Rational::FPS_25;
    // V1: 1 s of A (with its embedded audio), 0.5 s gap, 1 s of silent B, a disabled clip (black)
    let items = [
        Item::Clip(clip(&ma, r25, 25, 30)),
        Item::Gap(Gap {
            id: Id::new(),
            duration: t(15),
        }),
        Item::Clip(clip(&mb, r25, 0, 30)),
        Item::Clip(Clip {
            enabled: false,
            ..clip(&ma, r25, 0, 15)
        }),
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
    // V2: a small picture-in-picture of B over the first second, half transparent
    let pip = clip(&mb, r25, 25, 30);
    let pip_id = pip.id;
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v2,
            index: 0,
            item: Item::Clip(pip),
        },
    )
    .unwrap();
    let video = VideoProps {
        transform: Transform {
            position: (200.0, -100.0),
            scale: (0.3, 0.3),
            rotation_deg: 10.0,
        },
        opacity: 0.5,
        blend: BlendMode::Normal,
    };
    ed.apply(
        Author::User,
        Op::SetVideo {
            item: pip_id,
            video,
        },
    )
    .unwrap();
    // A1: the tone from A starting at 1.5 s, fading in
    let tone = Clip {
        audio: AudioProps {
            fade_in: 10,
            ..AudioProps::default()
        },
        ..clip(&ma, r25, 0, 45)
    };
    ed.apply(
        Author::User,
        edit::overwrite(ed.project(), a1, t(45), tone).unwrap(),
    )
    .unwrap();
    Built { ed, tl }
}

#[test]
fn exports_the_right_length_and_is_repeatable() {
    if !have_ffmpeg() {
        return;
    }
    let (a, b) = (scratch("a.mp4"), scratch("b.mp4"));
    make_source(&a, true);
    make_source(&b, false);
    let built = build(&a, &b);
    assert_eq!(
        built
            .ed
            .project()
            .timeline(built.tl)
            .unwrap()
            .duration()
            .value,
        90
    );

    let (o1, o2) = (scratch("o1.mp4"), scratch("o2.mp4"));
    let opts = ExportOptions::default();
    let mut last = (0, 0);
    export(built.ed.project(), built.tl, &o1, &opts, &mut |d, t| {
        last = (d, t)
    })
    .unwrap();
    assert_eq!(last, (90, 90));
    export(built.ed.project(), built.tl, &o2, &opts, &mut |_, _| {}).unwrap();

    let info = probe(&o1).unwrap();
    assert_eq!((info.width, info.height), (640, 360));
    assert_eq!(info.rate, Rational::FPS_30);
    assert_eq!(info.length, 90);
    assert!(info.has_audio);
    assert_eq!(
        frame_hashes(&o1),
        frame_hashes(&o2),
        "two exports of the same project must match frame for frame"
    );
    assert_eq!(
        std::fs::read(&o1).unwrap(),
        std::fs::read(&o2).unwrap(),
        "and byte for byte"
    );
}

#[test]
fn empty_timeline_and_bad_ids_are_errors() {
    let mut p = Project::new(ProjectSettings::default());
    let tl = Id::new();
    p.timelines.insert(
        tl,
        std::sync::Arc::new(Timeline::new(tl, "x", Rational::FPS_30)),
    );
    let opts = ExportOptions::default();
    assert!(matches!(
        export(&p, tl, Path::new("o.mp4"), &opts, &mut |_, _| {}),
        Err(ExportError::Empty)
    ));
    assert!(matches!(
        export(&p, Id::new(), Path::new("o.mp4"), &opts, &mut |_, _| {}),
        Err(ExportError::NoTimeline(_))
    ));
}

#[test]
fn encode_args_describe_the_input() {
    let a = encode_args(
        1280,
        720,
        Rational::FPS_29_97,
        48_000,
        Path::new("a.f32"),
        Path::new("o.mp4"),
        &ExportOptions::default(),
    );
    let j = a.join(" ");
    assert!(j.contains("-s 1280x720 -r 30000/1001 -i -"), "{j}");
    assert!(j.contains("-ar 48000 -ac 2 -i a.f32"), "{j}");
    assert!(j.ends_with("o.mp4"));
}
