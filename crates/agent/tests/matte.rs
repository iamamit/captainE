//! AI background removal end to end, on a bright ball drifting across a dull background.
//! Skipped when Apple's Vision framework is not reachable from Python.

use std::process::Command;
use std::sync::Arc;

use captaine_agent::ToolBox;
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::{RenderOptions, Renderer};
use captaine_time::{Rational, RationalTime};
use serde_json::json;

#[test]
fn the_background_goes_transparent_and_the_ball_stays() {
    if captaine_vision::unavailable().is_some()
        || Command::new("ffmpeg").arg("-version").output().is_err()
    {
        eprintln!("skipped: {:?}", captaine_vision::unavailable());
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-matte-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
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
    let rate = Rational::FPS_30;
    let media = media_ref(&video, Id::new()).unwrap();
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    }));
    let (tl, v1, v2) = (Id::new(), Id::new(), Id::new());
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
    for (i, (id, n)) in [(v1, "V1"), (v2, "V2")].into_iter().enumerate() {
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track: Track::new(id, n, TrackKind::Video),
            },
        )
        .unwrap();
    }
    // a green solid below, the ball clip (frames 0..60 of the file at 30 fps timeline) above
    let mut solid = Title::generator(
        Id::new(),
        Some(ShapeKind::Solid),
        RationalTime::new(60, rate),
    );
    solid.shape = Some(Shape {
        color: [0, 255, 0, 255],
        ..Shape::new(ShapeKind::Solid)
    });
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v1,
            index: 0,
            item: Item::Title(solid),
        },
    )
    .unwrap();
    let clip = Clip::new(
        Id::new(),
        "ball",
        media.id,
        RationalTime::new(0, media.duration.unwrap().rate),
        RationalTime::new(60, rate),
    );
    let id = clip.id;
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v2,
            index: 0,
            item: Item::Clip(clip),
        },
    )
    .unwrap();

    let at = |p: &Project, tick: i64, x: u32, y: u32| -> [u8; 3] {
        let c = Renderer::new(p, tl, RenderOptions::default())
            .unwrap()
            .render(tick)
            .unwrap();
        let rgba = c.to_rgba8();
        let i = ((y * c.width + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    // before: the ball clip covers the green solid everywhere
    let before = at(ed.project(), 0, 30, 30);
    assert!(
        before[1] < 100,
        "the dull background covers the green: {before:?}"
    );

    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    let out = tools
        .call("remove_background", &json!({"item_id": id.to_string()}))
        .unwrap();
    assert!(
        out.contains("Subject matte") || out.contains("matte") || out.contains("ball"),
        "{out}"
    );
    let p = tools.project().clone();
    p.validate().unwrap();
    // at 30 fps timeline over a 6 fps file, tick 0 shows the ball at x=160 and tick 55 at about x=410
    let (bg, ball) = (at(&p, 0, 30, 30), at(&p, 0, 160, 190));
    assert!(
        bg[1] > 200 && bg[0] < 60,
        "background replaced by the green below: {bg:?}"
    );
    assert!(
        ball[0] > 150 && ball[1] < 160,
        "the ball is still the ball: {ball:?}"
    );
    let (late_bg, late_ball) = (at(&p, 55, 160, 190), at(&p, 55, 410, 190));
    assert!(
        late_bg[1] > 200,
        "the ball has moved away from its first place: {late_bg:?}"
    );
    assert!(
        late_ball[0] > 150,
        "and is where the matte follows it: {late_ball:?}"
    );
    // the effect is an ordinary one: switching it off brings the background back
    let mut e = p.timeline(tl).unwrap().tracks[1].items[0]
        .as_clip()
        .unwrap()
        .effects
        .clone();
    e[0].enabled = false;
    let mut ed2 = Editor::new(p);
    ed2.apply(
        Author::User,
        Op::SetEffects {
            item: id,
            effects: e,
        },
    )
    .unwrap();
    assert!(at(ed2.project(), 0, 30, 30)[1] < 100);
    let _ = Arc::new(0);
    let _ = std::fs::remove_dir_all(dir);
}
