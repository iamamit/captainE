//! Tracking and reframing on a bright ball drifting left to right. Skipped without Apple's Vision.

use std::process::Command;

use captaine_agent::{reframe, track_attach};
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

fn ball(dir: &std::path::Path) -> std::path::PathBuf {
    let video = dir.join("ball.mp4");
    let filter = "gradients=s=640x360:c0=0x203040:c1=0x405060:seed=3:r=6:d=2,format=rgb24,\
        geq=r='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,235\\,r(X\\,Y))':\
        g='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,120\\,g(X\\,Y))':\
        b='if(lt(hypot(X-(160+25*N)\\,Y-190)\\,80)\\,30\\,b(X\\,Y))'";
    assert!(Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", filter, "-c:v", "libx264", "-pix_fmt",
            "yuv420p", "-crf", "10"
        ])
        .arg(&video)
        .status()
        .unwrap()
        .success());
    video
}

#[test]
fn a_title_follows_the_ball_and_a_vertical_reframe_pans_with_it() {
    if captaine_vision::unavailable().is_some()
        || Command::new("ffmpeg").arg("-version").output().is_err()
    {
        eprintln!("skipped: {:?}", captaine_vision::unavailable());
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-follow-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let media = media_ref(&ball(&dir), Id::new()).unwrap();
    let rate = Rational::FPS_30;
    let clip = Clip::new(
        Id::new(),
        "ball",
        media.id,
        RationalTime::new(0, media.duration.unwrap().rate),
        RationalTime::new(60, rate),
    );
    let title = Title::new(
        Id::new(),
        TitleTemplate::Caption,
        RationalTime::new(60, rate),
    );
    let fps = 30.0;

    // attach: the ball starts left of centre (x = 160 of 640) and ends right of it (about 410)
    let start = [0.125, 0.3, 0.25, 0.45];
    let ops = track_attach(&clip, &media, &title, 0, 0, (640, 360), fps, Some(start)).unwrap();
    let Op::SetAnimTrack { prop, keys: kx, .. } = &ops[0] else {
        panic!("x track first")
    };
    assert_eq!(prop, "position.x");
    assert!(kx.len() >= 8, "{}", kx.len());
    let (first, last) = (kx.first().unwrap().v, kx.last().unwrap().v);
    assert!(
        first < -100.0 && first > -230.0,
        "starts about 160 px left of centre: {first}"
    );
    assert!(
        last > first + 200.0,
        "ends well to the right: {first} -> {last}"
    );
    assert!(
        kx.windows(2).all(|w| w[1].v >= w[0].v - 15.0),
        "keeps moving right"
    );
    assert!(kx.last().unwrap().t >= 50, "keys cover the clip's length");
    let Op::SetAnimTrack { keys: ky, .. } = &ops[1] else {
        panic!("y track second")
    };
    assert!(
        ky.iter().all(|k| k.v.abs() < 60.0),
        "the ball stays at about the same height"
    );
    // a title that starts later only gets the keys that fall inside it
    let late = track_attach(&clip, &media, &title, 0, 30, (640, 360), fps, Some(start)).unwrap();
    let Op::SetAnimTrack { keys: lk, .. } = &late[0] else {
        panic!()
    };
    assert!(
        lk.iter().all(|k| k.t >= 0 && k.t < 60) && lk.first().unwrap().t == 0
            || lk.first().unwrap().t >= 0
    );
    // automatic: no box given, it finds the ball itself
    let auto = track_attach(&clip, &media, &title, 0, 0, (640, 360), fps, None).unwrap();
    let Op::SetAnimTrack { keys: ak, .. } = &auto[0] else {
        panic!()
    };
    assert!(ak.last().unwrap().v > ak.first().unwrap().v + 150.0);

    // reframe to vertical: the picture is scaled to fill and slides left as the ball goes right
    let ops = reframe(&clip, &media, (360, 640), fps).unwrap();
    let Op::SetVideo { video, .. } = &ops[0] else {
        panic!("video first")
    };
    assert!(
        video.transform.scale.0 > 3.0 && video.transform.scale.0 < 3.3,
        "{:?}",
        video.transform.scale
    );
    let Some(Op::SetAnimTrack { prop, keys, .. }) = ops.get(1) else {
        panic!("a horizontal pan")
    };
    assert_eq!(prop, "position.x");
    let (a, b) = (keys.first().unwrap().v, keys.last().unwrap().v);
    assert!(a > 100.0 && b < a - 150.0, "{a} -> {b}");
    assert!(
        keys.iter().all(|k| k.v.abs() <= 400.0),
        "never past the edge of the picture"
    );
    assert!(
        ops.len() == 2,
        "the picture fills the height exactly, so no vertical move"
    );
    // the plan applies cleanly
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        width: 360,
        height: 640,
        sample_rate: 48_000,
    }));
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: media.clone(),
        },
    )
    .unwrap();
    let (tl, tr) = (Id::new(), Id::new());
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "v", rate),
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
            description: "reframe".into(),
            ops,
        },
    )
    .unwrap();
    ed.project().validate().unwrap();
    let _ = std::fs::remove_dir_all(dir);
}
