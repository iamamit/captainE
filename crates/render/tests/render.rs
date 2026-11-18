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
fn a_media_file_replaced_on_disk_shows_its_new_picture_after_an_edit() {
    if !have_ffmpeg() {
        return;
    }
    let path = scratch("replaced.mp4");
    solid_video(&path, "red");
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &path);
    f.ed.apply(
        Author::User,
        edit::append(f.ed.project(), f.v1, clip(&m, 0, 30)),
    )
    .unwrap();
    let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    assert!(r.render(0).unwrap().to_rgba8()[0] > 200, "red first");
    // the file is replaced under the same name; the next edit hands the renderer the project again
    solid_video(&path, "blue");
    r.set_project(f.ed.project()).unwrap();
    let px = r.render(0).unwrap().to_rgba8();
    assert!(
        px[0] < 60 && px[2] > 200,
        "the new blue picture, not the cached red one: {:?}",
        &px[..4]
    );
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

#[test]
fn the_other_transitions_reveal_the_new_picture_their_own_way() {
    if !have_ffmpeg() {
        return;
    }
    let (red, blue) = (scratch("t2-red.mp4"), scratch("t2-blue.mp4"));
    solid_video(&red, "0xff0000");
    solid_video(&blue, "0x0000ff");
    let mut f = fixture((96, 54));
    let (mr, mb) = (add_media(&mut f, &red), add_media(&mut f, &blue));
    let (a, b) = (clip(&mr, 10, 30), clip(&mb, 10, 30));
    let bid = b.id;
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, a))
        .unwrap();
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, b))
        .unwrap();
    let set = |f: &mut Fixture, kind| {
        f.ed.apply(
            Author::User,
            Op::SetTransition {
                item: bid,
                transition: Some(Transition { kind, duration: 10 }),
            },
        )
        .unwrap();
    };
    let px = |f: &Fixture, tick: i64, x: u32, y: u32| -> [u8; 3] {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        rgb_of(&r.render(tick).unwrap(), x, y)
    };
    // wipe down: the top is already blue, the bottom still red
    set(&mut f, TransitionKind::WipeDown);
    assert!(px(&f, 27, 48, 4)[2] > 250 && px(&f, 27, 48, 50)[0] > 250);
    // wipe up is the other way round
    set(&mut f, TransitionKind::WipeUp);
    assert!(px(&f, 27, 48, 50)[2] > 250 && px(&f, 27, 48, 4)[0] > 250);
    // slide: the old picture stays under the new one sliding in from the right
    set(&mut f, TransitionKind::SlideLeft);
    assert!(px(&f, 27, 4, 27)[0] > 250, "the old picture has not moved");
    assert!(px(&f, 27, 92, 27)[2] > 250, "the new one is arriving");
    assert!(px(&f, 35, 4, 27)[2] > 250);
    // iris: the middle opens first, the corners stay old until late
    set(&mut f, TransitionKind::Iris);
    assert!(px(&f, 27, 48, 27)[2] > 250 && px(&f, 27, 2, 2)[0] > 250);
    assert!(px(&f, 35, 2, 2)[2] > 250);
    // zoom in: starts small and faint over the old picture, ends full size
    set(&mut f, TransitionKind::ZoomIn);
    assert!(px(&f, 26, 2, 2)[0] > 250 && px(&f, 26, 2, 2)[2] < 20);
    assert!(px(&f, 35, 2, 2)[2] > 250);
    // whips: the old picture swings away filling the whole frame (wrapped, no black edge), then the new one
    for kind in [TransitionKind::WhipLeft, TransitionKind::WhipRight] {
        set(&mut f, kind);
        for x in [1, 48, 94] {
            assert!(
                px(&f, 27, x, 27)[0] > 240,
                "{kind:?}: still the old picture at x={x}"
            );
            assert!(
                px(&f, 33, x, 27)[2] > 240,
                "{kind:?}: the new picture at x={x}"
            );
        }
    }
    // the newer ones: all start on the old picture and end on the new one
    for kind in TransitionKind::ALL.into_iter().skip(14) {
        set(&mut f, kind);
        assert!(px(&f, 24, 48, 27)[0] > 240, "{kind:?} starts red");
        assert!(px(&f, 35, 48, 27)[2] > 240, "{kind:?} ends blue");
    }
    set(&mut f, TransitionKind::BlurDissolve);
    let m = px(&f, 30, 48, 27);
    assert!(m[0] > 90 && m[2] > 90, "a mix {m:?}");
    set(&mut f, TransitionKind::DipToWhite);
    assert!(
        px(&f, 30, 48, 27).iter().all(|c| *c > 200),
        "white at the cut"
    );
    set(&mut f, TransitionKind::Flash);
    assert!(
        px(&f, 30, 48, 27).iter().all(|c| *c > 150),
        "a bright flash"
    );
    set(&mut f, TransitionKind::BarnDoors);
    assert!(px(&f, 28, 48, 27)[2] > 240 && px(&f, 28, 2, 27)[0] > 240);
    set(&mut f, TransitionKind::ClockWipe);
    assert!(px(&f, 28, 70, 10)[2] > 240, "swept past one o'clock");
    assert!(px(&f, 28, 20, 10)[0] > 240, "not yet at eleven");
    for kind in [
        TransitionKind::CrossZoom,
        TransitionKind::Spin,
        TransitionKind::Glitch,
    ] {
        set(&mut f, kind);
        assert!(px(&f, 27, 48, 27)[0] > 200, "{kind:?}: old picture first");
        assert!(px(&f, 33, 48, 27)[2] > 200, "{kind:?}: then the new");
    }
    // blue is dark, so a luma fade lets it in late
    set(&mut f, TransitionKind::LumaFade);
    assert!(px(&f, 31, 48, 27)[0] > 200);
    set(&mut f, TransitionKind::LightLeak);
    assert!(px(&f, 30, 48, 27)[1] > 25, "warm light added");
    assert_eq!(TransitionKind::ALL.len(), 24);
}

/// A flat mid-grey clip tagged as the given HDR (or wide-gamut) encoding, 10-bit HEVC.
fn tagged_video(path: &Path, primaries: &str, trc: &str, matrix: &str) {
    let params =
        format!("log-level=error:colorprim={primaries}:transfer={trc}:colormatrix={matrix}");
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=0xc0c0c0:s=96x54:r=30:d=1",
            "-vf",
            "format=yuv420p10le",
            "-c:v",
            "libx265",
            "-x265-params",
            &params,
            "-tag:v",
            "hvc1",
        ])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn hdr_footage_is_tone_mapped_into_rec709_unless_told_otherwise() {
    if !have_ffmpeg() || !captaine_media::ffmpeg_has_filter("zscale") {
        return;
    }
    let (hlg, pq, sdr) = (
        scratch("c-hlg.mp4"),
        scratch("c-pq.mp4"),
        scratch("c-sdr.mp4"),
    );
    tagged_video(&hlg, "bt2020", "arib-std-b67", "bt2020nc");
    tagged_video(&pq, "bt2020", "smpte2084", "bt2020nc");
    tagged_video(&sdr, "bt709", "bt709", "bt709");
    assert_eq!(
        captaine_media::detect_space(&hlg.to_string_lossy()),
        InputSpace::Hlg
    );
    assert_eq!(
        captaine_media::detect_space(&pq.to_string_lossy()),
        InputSpace::Pq
    );
    assert_eq!(
        captaine_media::detect_space(&sdr.to_string_lossy()),
        InputSpace::Rec709
    );
    let mut f = fixture((96, 54));
    let shown = |f: &mut Fixture, m: &MediaRef, space: Option<InputSpace>| -> u8 {
        if let Some(s) = space {
            f.ed.apply(
                Author::User,
                Op::SetMediaMeta {
                    media: m.id,
                    meta: Some(MediaMeta {
                        input: s,
                        ..MediaMeta::default()
                    }),
                },
            )
            .unwrap();
        }
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        grey_of(&r.render(3).unwrap(), 48, 27)
    };
    let (mh, mp, ms) = (
        add_media(&mut f, &hlg),
        add_media(&mut f, &pq),
        add_media(&mut f, &sdr),
    );
    // one clip per file, one after the other on V1
    for m in [&mh, &mp, &ms] {
        f.ed.apply(
            Author::User,
            edit::append(f.ed.project(), f.v1, clip(m, 0, 20)),
        )
        .unwrap();
    }
    let at = |f: &Fixture, tick: i64| {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        grey_of(&r.render(tick).unwrap(), 48, 27)
    };
    let (h_auto, p_auto, s_auto) = (at(&f, 3), at(&f, 23), at(&f, 43));
    // SDR footage is untouched (light grey stays as it is)
    assert!((s_auto as i32 - 190).abs() < 12, "{s_auto}");
    // told to treat the HDR files as plain Rec.709 they show the raw values...
    let h_raw = shown(&mut f, &mh, Some(InputSpace::Rec709));
    let p_raw = shown(&mut f, &mp, Some(InputSpace::Rec709));
    assert!((h_raw as i32 - 190).abs() < 20, "raw HLG grey {h_raw}");
    // ...while the automatic reading maps them: visibly different from the raw ones
    assert!(
        (h_auto as i32 - h_raw as i32).abs() > 12,
        "HLG {h_auto} vs raw {h_raw}"
    );
    assert!(
        (p_auto as i32 - p_raw as i32).abs() > 12,
        "PQ {p_auto} vs raw {p_raw}"
    );
    // the choice is part of the project and undoes
    f.ed.undo();
    f.ed.undo();
    assert_eq!(at(&f, 3), h_auto);
}

/// Frames that differ from one to the next (a moving test pattern), so an off-by-one source frame shows.
fn moving_video(path: &Path) {
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=96x54:r=30:d=4",
        ])
        .args(["-c:v", "libx264", "-g", "30", "-pix_fmt", "yuv420p"])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn playing_through_cuts_shows_the_same_frames_as_seeking_to_each() {
    // playback starts the next clip's decoder before the cut (read-ahead); every frame must still be exactly
    // the one a fresh render of that moment gives
    if !have_ffmpeg() {
        return;
    }
    let src = scratch("moving.mp4");
    moving_video(&src);
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &src);
    for (from, dur) in [(10, 20), (70, 15), (40, 25)] {
        f.ed.apply(
            Author::User,
            edit::append(f.ed.project(), f.v1, clip(&m, from, dur)),
        )
        .unwrap();
    }
    let mut playing = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
    for tick in 0..60 {
        let played = playing.render(tick).unwrap();
        let mut fresh = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        let sought = fresh.render(tick).unwrap();
        assert!(
            played.data == sought.data,
            "frame {tick} differs between playing and seeking"
        );
        playing.recycle(played);
    }
}

/// Library effects that act in the renderer rather than on the picture: keyframed settings, the low frame rate
/// hold, and the drop shadow drawn under a scaled-down layer.
#[test]
fn keyframed_effect_settings_low_frame_rate_and_drop_shadow() {
    if !have_ffmpeg() {
        return;
    }
    let (count, red) = (scratch("fx-count.mp4"), scratch("fx-red.mp4"));
    counter_video(&count, 60);
    solid_video(&red, "0xff0000");
    let render = |f: &Fixture, tick: i64| {
        let mut r = Renderer::new(f.ed.project(), f.tl, RenderOptions::default()).unwrap();
        r.render(tick).unwrap()
    };

    // keyframes on an effect setting: brightness goes from -100 to +100 over the clip
    let mut f = fixture((96, 54));
    let m = add_media(&mut f, &count);
    let c = clip(&m, 20, 30);
    let id = c.id;
    f.ed.apply(Author::User, edit::append(f.ed.project(), f.v1, c))
        .unwrap();
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: id,
            effects: vec![Effect::new(EffectKind::BrightnessContrast)],
        },
    )
    .unwrap();
    let key = |t, v| Keyframe {
        t,
        v,
        interp: Interp::Linear,
    };
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "fx.brightness_contrast.brightness".into(),
            keys: vec![key(0, -100.0), key(29, 100.0)],
        },
    )
    .unwrap();
    let (early, late) = (
        grey_of(&render(&f, 0), 48, 27),
        grey_of(&render(&f, 29), 48, 27),
    );
    assert!(early < 10, "darkened at the start: {early}");
    assert!(late > 200, "brightened at the end: {late}");

    // low frame rate 10 on a 30 fps timeline: each source frame is held for three timeline frames
    f.ed.apply(
        Author::User,
        Op::SetAnimTrack {
            item: id,
            prop: "fx.brightness_contrast.brightness".into(),
            keys: vec![],
        },
    )
    .unwrap();
    let mut low = Effect::new(EffectKind::LowFrameRate);
    low.params[0] = 10.0;
    f.ed.apply(
        Author::User,
        Op::SetEffects {
            item: id,
            effects: vec![low],
        },
    )
    .unwrap();
    let shown: Vec<i64> = (0..7)
        .map(|t| frame_no(grey_of(&render(&f, t), 48, 27)))
        .collect();
    assert_eq!(shown, vec![20, 20, 20, 23, 23, 23, 26], "{shown:?}");

    // drop shadow: a white shadow down and to the right of a red clip at half size on black
    let mut g = fixture((96, 54));
    let m = add_media(&mut g, &red);
    let mut c = clip(&m, 0, 30);
    c.video.transform.scale = (0.5, 0.5);
    let mut shadow = Effect::new(EffectKind::DropShadow);
    shadow.params = vec![200.0, 45.0, 0.0, 100.0, 255.0, 255.0, 255.0];
    c.effects = vec![shadow];
    g.ed.apply(Author::User, edit::append(g.ed.project(), g.v1, c))
        .unwrap();
    let pic = render(&g, 5);
    let rgb = |x, y| rgb_of(&pic, x, y);
    assert!(
        rgb(48, 27)[0] > 240 && rgb(48, 27)[1] < 20,
        "the clip itself on top"
    );
    // the clip covers x 24..72, y 13..40; 200 px at 1080 is 10 px here, along 45 degrees
    assert!(
        rgb(76, 44)[1] > 200,
        "shadow below right: {:?}",
        rgb(76, 44)
    );
    assert!(rgb(20, 10)[1] < 20, "nothing above left: {:?}", rgb(20, 10));
}
