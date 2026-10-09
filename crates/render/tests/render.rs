//! Renders real decoded video. Skipped when FFmpeg is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::*;
use captaine_time::{Rational, RationalTime};

fn have_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("captaine-render-test-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

/// 96x54 @ 30 fps, lossless; frame N is a flat grey of value 4*N (so a pixel tells you the frame number).
fn counter_video(path: &Path, frames: u32) {
    let src = format!(
        "color=c=black:s=96x54:r=30:d={},geq=r='4*N':g='4*N':b='4*N'",
        frames as f64 / 30.0
    );
    let ok = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", &src, "-c:v", "libx264", "-crf", "0",
            "-pix_fmt", "yuv444p",
        ])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn solid_video(path: &Path, color: &str) {
    let src = format!("color=c={color}:s=96x54:r=30:d=2");
    let ok = Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", &src, "-c:v", "libx264", "-crf", "0",
            "-pix_fmt", "yuv444p",
        ])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn t(v: i64) -> RationalTime {
    RationalTime::new(v, Rational::FPS_30)
}

struct Fixture {
    ed: Editor,
    tl: Id,
    v1: Id,
    v2: Id,
}

fn fixture(settings_size: (u32, u32)) -> Fixture {
    let settings = ProjectSettings {
        frame_rate: Rational::FPS_30,
        width: settings_size.0,
        height: settings_size.1,
        sample_rate: 48_000,
    };
    let mut ed = Editor::new(Project::new(settings));
    let (tl, v1, v2) = (Id::new(), Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main", Rational::FPS_30),
        },
    )
    .unwrap();
    for (i, id) in [v1, v2].into_iter().enumerate() {
        let track = Track::new(id, format!("V{}", i + 1), TrackKind::Video);
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track,
            },
        )
        .unwrap();
    }
    Fixture { ed, tl, v1, v2 }
}

fn add_media(f: &mut Fixture, path: &Path) -> MediaRef {
    let m = media_ref(path, Id::new()).unwrap();
    f.ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    m
}

fn clip(m: &MediaRef, src: i64, dur: i64) -> Clip {
    Clip::new(
        Id::new(),
        m.name.clone(),
        m.id,
        RationalTime::new(src, Rational::FPS_30),
        t(dur),
    )
}

fn grey_of(canvas: &Canvas, x: u32, y: u32) -> u8 {
    let rgba = canvas.to_rgba8();
    rgba[((y * canvas.width + x) * 4) as usize]
}

/// The frame number encoded in a pixel (value = 4N, within rounding noise).
fn frame_no(v: u8) -> i64 {
    (v as f64 / 4.0).round() as i64
}

#[test]
fn frames_are_exact_sequentially_and_after_seeks() {
    if !have_ffmpeg() {
        return;
    }
    let path = scratch("counter.mp4");
    counter_video(&path, 60);
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &path);
    assert_eq!(m.duration.unwrap().value, 60);
    // timeline 0..30 shows source 10..40
    let c = clip(&m, 10, 30);
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, c))
        .unwrap();
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    for tick in 0..30 {
        let canvas = r.render(tick).unwrap();
        assert_eq!(
            frame_no(grey_of(&canvas, 48, 27)),
            10 + tick,
            "sequential tick {tick}"
        );
    }
    // jump around: forward skip, backward, far forward
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    for tick in [5, 6, 20, 3, 29, 0, 17] {
        let canvas = r.render(tick).unwrap();
        assert_eq!(
            frame_no(grey_of(&canvas, 48, 27)),
            10 + tick,
            "seek to tick {tick}"
        );
    }
    // nothing at the end of the timeline: black
    let canvas = r.render(30).unwrap();
    assert_eq!(grey_of(&canvas, 48, 27), 0);
}

#[test]
fn holds_the_last_frame_past_the_end_of_the_file() {
    if !have_ffmpeg() {
        return;
    }
    let path = scratch("short.mp4");
    counter_video(&path, 20);
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &path);
    // media is cut off at 20 frames in the file's header but we ask for the final frame region
    let c = clip(&m, 15, 5);
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, c))
        .unwrap();
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    assert_eq!(frame_no(grey_of(&r.render(4).unwrap(), 10, 10)), 19);
}

#[test]
fn stacks_layers_with_opacity_and_transform() {
    if !have_ffmpeg() {
        return;
    }
    let (red, blue) = (scratch("red.mp4"), scratch("blue.mp4"));
    solid_video(&red, "red");
    solid_video(&blue, "blue");
    let mut f = fixture((192, 108)); // canvas is 2x the media: pictures are upscaled to fit
    let (mr, mb) = (add_media(&mut f, &red), add_media(&mut f, &blue));
    f.ed.apply(
        Author::User,
        edit::append(f.ed.project(), f.v1, clip(&mr, 0, 30)),
    )
    .unwrap();
    let top = clip(&mb, 0, 30);
    let top_id = top.id;
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v2, top))
        .unwrap();
    // blue: half size, shifted right, at 50%
    let video = VideoProps {
        transform: Transform {
            position: (48.0, 0.0),
            scale: (0.5, 0.5),
            rotation_deg: 0.0,
        },
        opacity: 0.5,
        blend: BlendMode::Normal,
        ..VideoProps::default()
    };
    f.ed.apply(
        Author::User,
        Op::SetVideo {
            item: top_id,
            video,
        },
    )
    .unwrap();
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    let c = r.render(3).unwrap();
    let px = |x, y| {
        let p = c.to_rgba8();
        let i = ((y * c.width + x) * 4) as usize;
        [p[i], p[i + 1], p[i + 2]]
    };
    let only_red = px(10, 54); // only red here (YUV round trip may be off by one)
    assert!(
        only_red[0] >= 253 && only_red[1] <= 2 && only_red[2] <= 2,
        "{only_red:?}"
    );
    // blue covers x 96..192 (center 144, half of 192 wide), y 27..81: 50% blue over red, mixed in linear light
    let mixed = px(144, 54);
    assert!(mixed[1] <= 2);
    assert!(
        (mixed[0] as i32 - 188).abs() <= 3 && (mixed[2] as i32 - 188).abs() <= 3,
        "{mixed:?}"
    );
}

#[test]
fn missing_media_file_is_a_clear_error() {
    let mut f = fixture((96, 54));
    let m = MediaRef {
        id: Id::new(),
        name: "gone.mp4".into(),
        path: "/definitely/not/here.mp4".into(),
        duration: Some(t(100)),
        has_audio: false,
        width: 96,
        height: 54,
        has_video: true,
        proxy: None,
    };
    f.ed.apply(Author::User, Op::AddMedia { media: m.clone() })
        .unwrap();
    f.ed.apply(
        Author::User,
        edit::append(f.ed.project(), f.v1, clip(&m, 0, 10)),
    )
    .unwrap();
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    let err = r.render(0).unwrap_err().to_string();
    assert!(
        err.contains("not found") && err.contains("here.mp4"),
        "{err}"
    );
}
