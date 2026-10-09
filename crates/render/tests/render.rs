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

#[test]
fn decoding_at_a_fraction_of_the_size_still_picks_the_right_frames() {
    if !have_ffmpeg() {
        return;
    }
    let path = scratch("counter_scaled.mp4");
    counter_video(&path, 60);
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &path);
    f.ed.apply(
        Author::User,
        edit::append(f.ed.project(), f.v1, clip(&m, 10, 30)),
    )
    .unwrap();
    let opts = RenderOptions {
        decode_scale: 0.5,
        ..RenderOptions::default()
    };
    let shown = preview_project(f.ed.project(), 0.5);
    let mut r = Renderer::new(&shown, f.tl, opts).unwrap();
    assert_eq!(r.size(), (48, 26)); // the canvas shrank with the preview
    for tick in [0, 1, 2, 15, 7, 29] {
        let canvas = r.render(tick).unwrap();
        assert_eq!(frame_no(grey_of(&canvas, 24, 13)), 10 + tick, "tick {tick}");
    }
}

#[test]
fn speed_reverse_and_freeze_pick_the_right_source_frames() {
    if !have_ffmpeg() {
        return;
    }
    let path = scratch("counter-retime.mp4");
    counter_video(&path, 60);
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &path);
    // source 10..40 as a 30-tick clip
    let c = clip(&m, 10, 30);
    let id = c.id;
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, c))
        .unwrap();
    let shown = |f: &Fixture, ticks: &[i64]| -> Vec<i64> {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        ticks
            .iter()
            .map(|&k| frame_no(grey_of(&r.render(k).unwrap(), 48, 27)))
            .collect()
    };
    // double speed: the same 30 source frames take 15 ticks
    f.ed.apply(
        Author::User,
        Op::SetRetime {
            item: id,
            retime: Retime {
                speed: 2.0,
                ..Retime::default()
            },
        },
    )
    .unwrap();
    assert_eq!(shown(&f, &[0, 1, 2, 14]), [10, 12, 14, 38]);
    assert_eq!(
        f.ed.project().timeline(f.tl).unwrap().duration().value,
        15,
        "the clip got shorter"
    );
    // half speed: 60 ticks, each source frame twice
    f.ed.apply(
        Author::User,
        Op::SetRetime {
            item: id,
            retime: Retime {
                speed: 0.5,
                ..Retime::default()
            },
        },
    )
    .unwrap();
    assert_eq!(f.ed.project().timeline(f.tl).unwrap().duration().value, 60);
    assert_eq!(shown(&f, &[0, 1, 2, 3, 59]), [10, 10, 11, 11, 39]);
    // reversed at normal speed: 39, 38, ... 10
    f.ed.apply(
        Author::User,
        Op::SetRetime {
            item: id,
            retime: Retime {
                reverse: true,
                ..Retime::default()
            },
        },
    )
    .unwrap();
    assert_eq!(f.ed.project().timeline(f.tl).unwrap().duration().value, 30);
    assert_eq!(shown(&f, &[0, 1, 2, 29, 3]), [39, 38, 37, 10, 36]);
    // sequential playback of a reversed clip comes out of the run cache
    let seq = shown(&f, &(0..30).collect::<Vec<_>>());
    assert_eq!(seq, (10..40).rev().collect::<Vec<_>>());
    // a freeze holds one frame for the whole clip, and undo walks back to the start
    f.ed.apply(
        Author::User,
        Op::SetRetime {
            item: id,
            retime: Retime {
                freeze: true,
                ..Retime::default()
            },
        },
    )
    .unwrap();
    assert_eq!(shown(&f, &[0, 15, 29]), [10, 10, 10]);
    for _ in 0..4 {
        f.ed.undo();
    }
    assert_eq!(shown(&f, &[0, 29]), [10, 39]);
}

fn rgb_of(canvas: &Canvas, x: u32, y: u32) -> [u8; 3] {
    let rgba = canvas.to_rgba8();
    let i = ((y * canvas.width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

#[test]
fn transitions_blend_the_two_clips_around_the_cut() {
    if !have_ffmpeg() {
        return;
    }
    let (red, blue) = (scratch("t-red.mp4"), scratch("t-blue.mp4"));
    solid_video(&red, "0xff0000");
    solid_video(&blue, "0x0000ff");
    let mut f = fixture((96, 54));
    let (mr, mb) = (add_media(&mut f, &red), add_media(&mut f, &blue));
    // red ticks 0..30 (source 10..40), blue ticks 30..60 (source 10..40)
    let (a, b) = (clip(&mr, 10, 30), clip(&mb, 10, 30));
    let bid = b.id;
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, a))
        .unwrap();
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, b))
        .unwrap();
    let set = |f: &mut Fixture, kind, duration| {
        f.ed.apply(
            Author::User,
            Op::SetTransition {
                item: bid,
                transition: Some(Transition { kind, duration }),
            },
        )
        .unwrap();
    };
    let at = |f: &Fixture, tick: i64| -> [u8; 3] {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        rgb_of(&r.render(tick).unwrap(), 48, 27)
    };
    let left = |f: &Fixture, tick: i64| -> [u8; 3] {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        rgb_of(&r.render(tick).unwrap(), 8, 27)
    };
    // no transition: a hard cut
    assert!(at(&f, 29)[0] > 250);
    assert!(at(&f, 30)[2] > 250);
    // a 10-frame dissolve covers ticks 25..35
    set(&mut f, TransitionKind::Dissolve, 10);
    assert!(at(&f, 24)[0] > 250 && at(&f, 24)[2] < 5, "before it starts");
    assert!(at(&f, 35)[2] > 250 && at(&f, 35)[0] < 5, "after it ends");
    let (early, late) = (at(&f, 26), at(&f, 33));
    assert!(early[0] > 200 && early[2] < 130, "{early:?}");
    assert!(late[2] > 190 && late[0] < 130, "{late:?}");
    let mid = at(&f, 30);
    assert!(
        mid[0] > 100 && mid[2] > 100 && mid[1] < 10,
        "a purple mix {mid:?}"
    );
    // dip to black: nearly black at the cut
    set(&mut f, TransitionKind::Dip, 10);
    let m = at(&f, 29);
    assert!(m[0] < 100 && m[2] < 100, "{m:?}");
    assert!(at(&f, 26)[0] > 90);
    // wipe right: the new picture grows from the left edge
    set(&mut f, TransitionKind::WipeRight, 10);
    assert!(left(&f, 27)[2] > 250, "left edge is already blue");
    assert!(at(&f, 27)[0] > 250, "the middle is still red");
    assert!(at(&f, 33)[2] > 250);
    // push left: the new picture slides in from the right
    set(&mut f, TransitionKind::PushLeft, 10);
    assert!(
        left(&f, 27)[0] > 250,
        "the old picture is leaving to the left"
    );
    assert!(at(&f, 33)[2] > 250);
    // the transition is part of the project: undo removes it
    f.ed.undo();
    f.ed.undo();
    f.ed.undo();
    f.ed.undo();
    assert!(at(&f, 29)[0] > 250);
    // asking for a transition the clips cannot hold is caught by the check
    let first = f.ed.project().timeline(f.tl).unwrap().tracks[0].items[0].id();
    let t = |duration| Transition {
        kind: TransitionKind::Dissolve,
        duration,
    };
    assert!(check_transition(f.ed.project(), first, &t(10)).is_err());
    assert!(check_transition(f.ed.project(), bid, &t(61)).is_err());
    assert!(check_transition(f.ed.project(), bid, &t(60)).is_ok());
}

#[test]
fn a_keyed_clip_shows_what_is_under_it() {
    if !have_ffmpeg() {
        return;
    }
    let (red, green) = (scratch("k-red.mp4"), scratch("k-green.mp4"));
    solid_video(&red, "0xff0000");
    solid_video(&green, "0x00ff00");
    let mut f = fixture((96, 54));
    let (mr, mg) = (add_media(&mut f, &red), add_media(&mut f, &green));
    let (under, over) = (clip(&mr, 0, 30), clip(&mg, 0, 30));
    let oid = over.id;
    for (track, c) in [(f.v1, under), (f.v2, over)] {
        f.ed.apply(Author::User, edit::append(f.ed.project(), track, c))
            .unwrap();
    }
    let centre = |f: &Fixture| {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        rgb_of(&r.render(5).unwrap(), 48, 27)
    };
    assert!(centre(&f)[1] > 250, "the green clip covers the red one");
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: oid,
            effects: vec![Effect::new(EffectKind::ChromaKey)],
        },
    )
    .unwrap();
    let keyed = centre(&f);
    assert!(
        keyed[0] > 250 && keyed[1] < 5,
        "red shows through: {keyed:?}"
    );
    // an effect can be switched off and undone
    let mut off = Effect::new(EffectKind::ChromaKey);
    off.enabled = false;
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: oid,
            effects: vec![off],
        },
    )
    .unwrap();
    assert!(centre(&f)[1] > 250);
    f.ed.undo();
    assert!(centre(&f)[0] > 250);
    // a parameter out of range is refused
    let mut bad = Effect::new(EffectKind::Blur);
    bad.params = vec![1000.0];
    assert!(f
        .ed
        .apply(
            Author::User,
            Op::SetEffects {
                item: oid,
                effects: vec![bad]
            }
        )
        .is_err());
}
