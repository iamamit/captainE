//! Auto colour and shot matching on synthetic footage with a deliberate cast.

use std::process::Command;

use captaine_agent::ToolBox;
use captaine_media::media_ref;
use captaine_model::*;
use captaine_ops::*;
use captaine_render::{ColorStats, Frame8, RenderOptions, Renderer};
use captaine_time::{Rational, RationalTime};
use serde_json::json;

/// A gradient picture tinted `rgb` (0..1 per channel) and scaled by `gain`.
fn footage(path: &std::path::Path, rgb: [f32; 3], gain: f32) {
    let e = |c: f32| format!("clip({:.3}*(0.25+0.75*X/W)*255\\,0\\,255)", c * gain);
    let graph = format!(
        "gradients=s=320x180:c0=0x808080:c1=0xffffff:seed=1:r=24:d=1,format=rgb24,geq=r='{}':g='{}':b='{}'",
        e(rgb[0]),
        e(rgb[1]),
        e(rgb[2])
    );
    assert!(Command::new("ffmpeg")
        .args([
            "-y", "-v", "error", "-f", "lavfi", "-i", &graph, "-c:v", "libx264", "-pix_fmt",
            "yuv444p", "-crf", "8"
        ])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

fn stats_of(p: &Project, tl: Id) -> ColorStats {
    let c = Renderer::new(p, tl, RenderOptions::default())
        .unwrap()
        .render(6)
        .unwrap();
    let f = Frame8 {
        width: c.width,
        height: c.height,
        rgba: c.to_rgba8(),
    };
    ColorStats::of(&[&f]).unwrap()
}

#[test]
fn auto_color_and_match_color_move_the_picture_the_right_way() {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("captaine-color-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cool, warm) = (dir.join("cool.mp4"), dir.join("warm.mp4"));
    footage(&cool, [0.55, 0.65, 1.0], 0.4); // dark, blue
    footage(&warm, [1.0, 0.85, 0.6], 0.95); // bright, warm
    let rate = Rational::FPS_24;
    let (mc, mw) = (
        media_ref(&cool, Id::new()).unwrap(),
        media_ref(&warm, Id::new()).unwrap(),
    );
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: rate,
        width: 320,
        height: 180,
        sample_rate: 48_000,
    }));
    let (tl, v1, v2) = (Id::new(), Id::new(), Id::new());
    for m in [&mc, &mw] {
        ed.apply(Author::User, Op::AddMedia { media: m.clone() })
            .unwrap();
    }
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
    let mk = |m: &MediaRef| {
        Clip::new(
            Id::new(),
            m.name.clone(),
            m.id,
            RationalTime::new(0, rate),
            RationalTime::new(20, rate),
        )
    };
    let (cool_clip, warm_clip) = (mk(&mc), mk(&mw));
    let (cid, wid) = (cool_clip.id, warm_clip.id);
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v1,
            index: 0,
            item: Item::Clip(cool_clip),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: v2,
            index: 0,
            item: Item::Clip(warm_clip),
        },
    )
    .unwrap();
    // look at the cool one alone: hide the warm one
    ed.apply(
        Author::User,
        Op::SetEnabled {
            item: wid,
            enabled: false,
        },
    )
    .unwrap();
    let before = stats_of(ed.project(), tl);

    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    let out = tools
        .call("auto_color", &json!({"item_id": cid.to_string()}))
        .unwrap();
    assert!(out.contains("exposure"), "{out}");
    let after = stats_of(tools.project(), tl);
    assert!(
        after.median_lin > before.median_lin * 1.3,
        "brighter: {} -> {}",
        before.median_lin,
        after.median_lin
    );
    let cast = |s: &ColorStats| (s.mean_lin[2] / s.mean_lin[0]).ln().abs();
    assert!(
        cast(&after) < cast(&before) * 0.6,
        "the blue cast shrank: {} -> {}",
        cast(&before),
        cast(&after)
    );

    // match the cool clip to the warm one: it gets warmer and brighter than it was
    ed.apply(
        Author::User,
        Op::SetEnabled {
            item: wid,
            enabled: true,
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::SetEnabled {
            item: cid,
            enabled: true,
        },
    )
    .unwrap();
    // measure each alone by disabling the other
    let alone = |ed: &mut Editor, keep: Id, drop: Id| {
        ed.apply(
            Author::User,
            Op::SetEnabled {
                item: drop,
                enabled: false,
            },
        )
        .unwrap();
        ed.apply(
            Author::User,
            Op::SetEnabled {
                item: keep,
                enabled: true,
            },
        )
        .unwrap();
        stats_of(ed.project(), tl)
    };
    let reference = alone(&mut ed, wid, cid);
    let target_before = alone(&mut ed, cid, wid);
    let mut tools = ToolBox::new(ed.project(), &[], None, 0.0).unwrap();
    tools
        .call(
            "match_color",
            &json!({"item_id": cid.to_string(), "reference_id": wid.to_string()}),
        )
        .unwrap();
    let mut p = tools.project().clone();
    // look at the matched clip alone
    for (id, on) in [(cid, true), (wid, false)] {
        let loc = p.find_item(id).unwrap();
        let Item::Clip(c) = p.item(loc).unwrap().clone() else {
            unreachable!()
        };
        let mut ed2 = Editor::new(p.clone());
        ed2.apply(
            Author::User,
            Op::SetEnabled {
                item: c.id,
                enabled: on,
            },
        )
        .unwrap();
        p = ed2.project().clone();
    }
    let matched = stats_of(&p, tl);
    let dist = |a: &ColorStats, b: &ColorStats| {
        (a.median_lin / b.median_lin).ln().abs()
            + ((a.mean_lin[0] / a.mean_lin[2]) / (b.mean_lin[0] / b.mean_lin[2]))
                .ln()
                .abs()
    };
    assert!(
        dist(&matched, &reference) < dist(&target_before, &reference) * 0.5,
        "{} -> {}",
        dist(&target_before, &reference),
        dist(&matched, &reference)
    );
    // a clip without footage is refused clearly
    let _ = std::fs::remove_dir_all(dir);
}
