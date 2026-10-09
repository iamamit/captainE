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
        ..VideoProps::default()
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

#[test]
fn codec_arguments() {
    let args = |codec| {
        let o = ExportOptions {
            codec,
            ..ExportOptions::default()
        };
        encode_args(
            640,
            360,
            Rational::FPS_30,
            48_000,
            Path::new("a.f32"),
            Path::new("o.mov"),
            &o,
        )
        .join(" ")
    };
    assert!(args(Codec::H264).contains("-c:v libx264") && args(Codec::H264).contains("-c:a aac"));
    let hevc = args(Codec::Hevc);
    assert!(hevc.contains("-c:v libx265 -tag:v hvc1") && hevc.contains("-crf 18"));
    let prores = args(Codec::ProRes);
    assert!(
        prores.contains("prores_ks")
            && prores.contains("yuv422p10le")
            && prores.contains("pcm_s16le")
    );
    assert!(!prores.contains("faststart"));
}

#[test]
fn hevc_and_prores_files_come_out_right() {
    if !have_ffmpeg() {
        return;
    }
    let (a, b) = (scratch("c_a.mp4"), scratch("c_b.mp4"));
    make_source(&a, true);
    make_source(&b, false);
    let built = build(&a, &b);
    for (codec, name, expect) in [
        (Codec::Hevc, "x.mp4", "hevc"),
        (Codec::ProRes, "x.mov", "prores"),
    ] {
        let out = scratch(name);
        let opts = ExportOptions {
            codec,
            preset: "ultrafast".into(),
            ..ExportOptions::default()
        };
        export(built.ed.project(), built.tl, &out, &opts, &mut |_, _| {}).unwrap();
        let probe_out = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&probe_out.stdout).trim(), expect);
        assert_eq!(probe(&out).unwrap().length, 90);
        assert!(probe(&out).unwrap().has_audio);
    }
}

#[test]
fn a_title_is_burned_into_the_export() {
    if !have_ffmpeg() {
        return;
    }
    // no media at all: just a lower third for one second
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: Rational::FPS_24,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    }));
    let (tl, v1) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_24),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: tl,
            index: 0,
            track: Track::new(v1, "V1", TrackKind::Video),
        },
    )
    .unwrap();
    let mut title = Title::new(
        Id::new(),
        TitleTemplate::LowerThird,
        RationalTime::new(24, Rational::FPS_24),
    );
    title.text = "Coastline, Day 3".into();
    title.subtitle = "Lower third".into();
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v1,
            index: 0,
            item: Item::Title(title),
        },
    )
    .unwrap();

    let out = scratch("title.mp4");
    export(
        ed.project(),
        tl,
        &out,
        &ExportOptions {
            preset: "ultrafast".into(),
            ..Default::default()
        },
        &mut |_, _| {},
    )
    .unwrap();
    let info = probe(&out).unwrap();
    assert_eq!((info.width, info.height, info.length), (640, 360, 24));
    // grab one frame as grey bytes: white text on black, so there are bright pixels, and only in the lower left
    let raw = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&out)
        .args([
            "-vf",
            "select=eq(n\\,12)",
            "-frames:v",
            "1",
            "-pix_fmt",
            "gray",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap()
        .stdout;
    assert_eq!(raw.len(), 640 * 360);
    let bright = |x0: usize, x1: usize, y0: usize, y1: usize| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| raw[y * 640 + x] > 200)
            .count()
    };
    assert!(
        bright(0, 400, 220, 340) > 100,
        "white text in the lower left"
    );
    assert_eq!(bright(0, 640, 0, 180), 0, "nothing in the top half");
    // and it is repeatable
    let again = scratch("title2.mp4");
    export(
        ed.project(),
        tl,
        &again,
        &ExportOptions {
            preset: "ultrafast".into(),
            ..Default::default()
        },
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&again).unwrap());
}

/// A built project (see `build`), ready for the new-format tests.
fn project_for_formats() -> (Built, PathBuf) {
    let a = scratch("fmt-a.mp4");
    let b = scratch("fmt-b.mp4");
    make_source(&a, true);
    make_source(&b, false);
    (build(&a, &b), a)
}

fn probe_streams(path: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name,codec_type,width,height",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn duration_secs(path: &Path) -> f64 {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn range_gif_png_audio_and_hardware_exports() {
    if !have_ffmpeg() {
        return;
    }
    let (built, _) = project_for_formats();
    let (p, tl) = (built.ed.project(), built.tl);
    let quick = |codec, range: Option<(i64, i64)>| ExportOptions {
        codec,
        preset: "ultrafast".into(),
        range,
        ..Default::default()
    };
    // a range: 30 frames out of the middle is one second long
    let out = scratch("range.mp4");
    export(
        p,
        tl,
        &out,
        &quick(Codec::H264, Some((15, 45))),
        &mut |_, _| {},
    )
    .unwrap();
    let info = probe(&out).unwrap();
    assert_eq!(info.length, 30);
    // the range renders the very same pictures as the full export (compared as lossless PNGs)
    let (dfull, drange) = (scratch("png-full"), scratch("png-range"));
    for d in [&dfull, &drange] {
        let _ = std::fs::remove_dir_all(d);
    }
    export(
        p,
        tl,
        &dfull,
        &quick(Codec::PngSequence, Some((0, 25))),
        &mut |_, _| {},
    )
    .unwrap();
    export(
        p,
        tl,
        &drange,
        &quick(Codec::PngSequence, Some((15, 25))),
        &mut |_, _| {},
    )
    .unwrap();
    for k in 1..=10 {
        let a = std::fs::read(drange.join(format!("frame_{k:06}.png"))).unwrap();
        let b = std::fs::read(dfull.join(format!("frame_{:06}.png", 15 + k))).unwrap();
        assert_eq!(a, b, "range frame {k}");
    }
    // empty or backwards range
    assert!(export(
        p,
        tl,
        &out,
        &quick(Codec::H264, Some((40, 40))),
        &mut |_, _| {}
    )
    .is_err());

    // GIF: a picture, no sound, at most 480 wide
    let gif = scratch("anim.gif");
    export(
        p,
        tl,
        &gif,
        &quick(Codec::Gif, Some((0, 30))),
        &mut |_, _| {},
    )
    .unwrap();
    let s = probe_streams(&gif);
    assert!(s.contains("gif") && !s.contains("audio"), "{s}");
    assert!(s.contains("480"), "{s}");

    // PNG sequence: one file per frame
    let dir = scratch("frames");
    let _ = std::fs::remove_dir_all(&dir);
    export(
        p,
        tl,
        &dir,
        &quick(Codec::PngSequence, Some((0, 12))),
        &mut |_, _| {},
    )
    .unwrap();
    let n = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(n, 12);
    assert!(dir.join("frame_000001.png").exists());

    // audio only
    let wav = scratch("mix.wav");
    export(
        p,
        tl,
        &wav,
        &quick(Codec::Wav, Some((0, 60))),
        &mut |_, _| {},
    )
    .unwrap();
    assert!((duration_secs(&wav) - 2.0).abs() < 0.05);
    assert!(probe_streams(&wav).contains("pcm_s16le"));
    let m4a = scratch("mix.m4a");
    let mut seen = 0;
    export(p, tl, &m4a, &quick(Codec::Aac, None), &mut |d, t| {
        seen = d;
        assert!(d <= t);
    })
    .unwrap();
    assert!(seen > 0);
    assert!(probe_streams(&m4a).contains("aac"));

    // hardware encoder (macOS only): H.264 that decodes to the right length
    if cfg!(target_os = "macos") {
        let hw = scratch("hw.mp4");
        export(
            p,
            tl,
            &hw,
            &quick(Codec::H264Hw, Some((0, 30))),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(probe(&hw).unwrap().length, 30);
        assert!(probe_streams(&hw).contains("h264"));
    }
}

#[test]
fn an_export_can_be_cancelled() {
    if !have_ffmpeg() {
        return;
    }
    let (built, _) = project_for_formats();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let out = scratch("cancelled.mp4");
    let flag = cancel.clone();
    let r = export(
        built.ed.project(),
        built.tl,
        &out,
        &ExportOptions {
            preset: "ultrafast".into(),
            cancel: Some(cancel),
            ..Default::default()
        },
        &mut |done, _| {
            if done == 5 {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    );
    assert!(matches!(r, Err(ExportError::Cancelled)), "{r:?}");
    assert!(
        !out.exists(),
        "a cancelled export leaves no half-written file"
    );
}

#[test]
fn every_preset_names_a_real_format() {
    for p in PRESETS {
        assert!(!p.name.is_empty() && !p.note.is_empty());
        assert!(Codec::ALL.contains(&p.codec));
    }
}

fn colour_tags(path: &Path) -> String {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=pix_fmt,color_space,color_transfer,color_primaries",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn hevc_can_be_written_as_hdr_and_the_tags_say_so() {
    if !have_ffmpeg() {
        return;
    }
    let (a, b) = (scratch("hdr_a.mp4"), scratch("hdr_b.mp4"));
    make_source(&a, true);
    make_source(&b, false);
    let built = build(&a, &b);
    let run = |name: &str, hdr: bool| {
        let out = scratch(name);
        let opts = ExportOptions {
            codec: Codec::Hevc,
            preset: "ultrafast".into(),
            hdr,
            range: Some((0, 6)),
            ..ExportOptions::default()
        };
        export(built.ed.project(), built.tl, &out, &opts, &mut |_, _| {}).unwrap();
        out
    };
    let sdr = run("hdr_sdr.mp4", false);
    let hlg = run("hdr_hlg.mp4", true);
    // csv order of ffprobe's fields: pix_fmt,color_space,color_transfer,color_primaries
    let (s, h) = (colour_tags(&sdr), colour_tags(&hlg));
    assert!(s.contains("bt709") && s.starts_with("yuv420p,"), "{s}");
    assert!(h.starts_with("yuv420p10le,"), "{h}");
    assert!(h.contains("arib-std-b67") && h.contains("bt2020"), "{h}");
    // and the editor reads its own HDR file back as HLG
    assert_eq!(
        captaine_media::detect_space(&hlg.to_string_lossy()),
        captaine_model::InputSpace::Hlg
    );
    assert_eq!(
        captaine_media::detect_space(&sdr.to_string_lossy()),
        captaine_model::InputSpace::Rec709
    );
}
