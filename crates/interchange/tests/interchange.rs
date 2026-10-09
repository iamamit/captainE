use std::sync::Arc;

use captaine_interchange::*;
use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_24;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

fn media(name: &str, path: &str) -> MediaRef {
    MediaRef {
        id: Id::new(),
        name: name.into(),
        path: path.into(),
        duration: Some(t(240)),
        has_audio: true,
        width: 1920,
        height: 1080,
        has_video: true,
        proxy: None,
    }
}

/// V1: a (0..48), gap 12, b retimed 2x (60..84), c with a dissolve (84..120); V2: a small overlay;
/// A1: music. Plus a title on V2.
fn project() -> (Project, Id) {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: R,
        width: 1920,
        height: 1080,
        sample_rate: 48_000,
    }));
    let (a, b, c) = (
        media("Alpha Clip.mp4", "/footage/Alpha Clip.mp4"),
        media("beta.mov", "/footage/beta.mov"),
        media("song.wav", "/music/song.wav"),
    );
    for m in [&a, &b, &c] {
        ed.apply(Author::User, Op::AddMedia { media: m.clone() })
            .unwrap();
    }
    let tl = Id::new();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(tl, "Main cut", R),
        },
    )
    .unwrap();
    let ids: Vec<Id> = (0..3).map(|_| Id::new()).collect();
    for (i, (id, kind, name)) in [
        (ids[0], TrackKind::Video, "V1"),
        (ids[1], TrackKind::Video, "V2"),
        (ids[2], TrackKind::Audio, "A1"),
    ]
    .into_iter()
    .enumerate()
    {
        ed.apply(
            Author::User,
            Op::AddTrack {
                timeline: tl,
                index: i,
                track: Track::new(id, name, kind),
            },
        )
        .unwrap();
    }
    let clip = |m: &MediaRef, start: i64, dur: i64| {
        Clip::new(Id::new(), m.name.clone(), m.id, t(start), t(dur))
    };
    let ca = clip(&a, 24, 48);
    let mut cb = clip(&b, 0, 12); // 2x speed: 12 ticks cover 24 source frames
    cb.retime = Retime {
        speed: 2.0,
        ..Retime::default()
    };
    cb.video.opacity = 0.8;
    cb.effects = vec![Effect::new(EffectKind::Blur)];
    let mut cc = clip(&a, 100, 36);
    cc.transition = Some(Transition {
        kind: TransitionKind::Dissolve,
        duration: 10,
    });
    let ops = [
        Op::InsertItem {
            track: ids[0],
            index: 0,
            item: Item::Clip(ca),
        },
        Op::InsertItem {
            track: ids[0],
            index: 1,
            item: Item::Gap(Gap {
                id: Id::new(),
                duration: t(12),
            }),
        },
        Op::InsertItem {
            track: ids[0],
            index: 2,
            item: Item::Clip(cb),
        },
        Op::InsertItem {
            track: ids[0],
            index: 3,
            item: Item::Clip(cc),
        },
        Op::InsertItem {
            track: ids[1],
            index: 0,
            item: Item::Gap(Gap {
                id: Id::new(),
                duration: t(20),
            }),
        },
        Op::InsertItem {
            track: ids[1],
            index: 1,
            item: Item::Clip(clip(&b, 50, 30)),
        },
        Op::InsertItem {
            track: ids[1],
            index: 2,
            item: Item::Title(Title::new(Id::new(), TitleTemplate::Centered, t(24))),
        },
        Op::InsertItem {
            track: ids[2],
            index: 0,
            item: Item::Clip(clip(&c, 0, 120)),
        },
    ];
    for op in ops {
        ed.apply(Author::User, op).unwrap();
    }
    (ed.project().clone(), tl)
}

/// A comparable picture of a timeline without ids.
fn shape(p: &Project, tl: Id) -> Vec<(String, Vec<String>)> {
    let tl = p.timeline(tl).unwrap();
    tl.tracks
        .iter()
        .map(|tr| {
            let items = tr
                .items
                .iter()
                .map(|it| match it {
                    Item::Clip(c) => format!(
                        "clip {} src {} dur {} retime {:?} transition {:?} opacity {} fx {:?} media {}",
                        c.name, c.source_start.value, c.duration.value, c.retime, c.transition, c.video.opacity,
                        c.effects.iter().map(|e| e.kind).collect::<Vec<_>>(),
                        p.media[&c.media].path
                    ),
                    Item::Title(x) => format!("title {} dur {}", x.text, x.duration.value),
                    Item::Gap(g) => format!("gap {}", g.duration.value),
                })
                .collect();
            (format!("{} {:?}", tr.name, tr.kind), items)
        })
        .collect()
}

fn apply_all(p: &Project, ops: Vec<Op>) -> Project {
    let mut ed = Editor::new(p.clone());
    for op in ops {
        ed.apply(Author::User, op).unwrap();
    }
    ed.project().clone()
}

#[test]
fn otio_round_trip_loses_nothing() {
    let (p, tl) = project();
    let text = otio::export(&p, tl).unwrap();
    // it is the OpenTimelineIO structure other tools expect
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["OTIO_SCHEMA"], "Timeline.1");
    let tracks = v["tracks"]["children"].as_array().unwrap();
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[2]["kind"], "Audio");
    let v1 = tracks[0]["children"].as_array().unwrap();
    let kinds: Vec<&str> = v1
        .iter()
        .map(|c| c["OTIO_SCHEMA"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        ["Clip.2", "Gap.1", "Clip.2", "Transition.1", "Clip.2"]
    );
    assert_eq!(v1[2]["effects"][0]["time_scalar"], 2.0);
    assert!(v1[0]["media_reference"]["target_url"]
        .as_str()
        .unwrap()
        .contains("Alpha%20Clip.mp4"));
    // read it into a project that already has the media
    let mut fresh = Project::new(p.settings.clone());
    fresh.media = p.media.clone();
    let ops = otio::import(&fresh, &text).unwrap();
    let back = apply_all(&fresh, ops);
    let new_tl = *back.timelines.keys().next().unwrap();
    assert_eq!(shape(&back, new_tl), shape(&p, tl));
    assert_eq!(
        back.media.len(),
        3,
        "the media were matched by path, not added again"
    );
    back.validate().unwrap();
}

#[test]
fn otio_from_another_tool_with_unknown_fields_is_read() {
    // a minimal file as another tool might write it: no captaine metadata, 25 fps, speed change in OTIO terms
    let dir = std::env::temp_dir().join(format!("captaine-otio-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = serde_json::json!({
        "OTIO_SCHEMA": "Timeline.1", "name": "Theirs",
        "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [{
            "OTIO_SCHEMA": "Track.1", "kind": "Video", "name": "Video 1", "children": [
                {"OTIO_SCHEMA": "Gap.1", "source_range": {"OTIO_SCHEMA": "TimeRange.1",
                    "start_time": {"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":25},
                    "duration": {"OTIO_SCHEMA":"RationalTime.1","value":10,"rate":25}}},
            ]}]},
        "global_start_time": {"OTIO_SCHEMA":"RationalTime.1","value":0,"rate":25}
    });
    let ops = otio::import(&Project::new(ProjectSettings::default()), &doc.to_string()).unwrap();
    let p = apply_all(&Project::new(ProjectSettings::default()), ops);
    let tl = p.timelines.values().next().unwrap();
    assert_eq!((tl.name.as_str(), tl.rate), ("Theirs", Rational::FPS_25));
    assert_eq!(tl.tracks[0].items[0].duration().value, 10);
    assert!(otio::import(&p, "{\"OTIO_SCHEMA\":\"Clip.2\"}").is_err());
    assert!(otio::import(&p, "not json").is_err());
    // a clip whose file is missing is reported, not invented
    let doc = serde_json::json!({
        "OTIO_SCHEMA": "Timeline.1", "name": "x",
        "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [{"OTIO_SCHEMA": "Track.1", "kind": "Video", "children": [
            {"OTIO_SCHEMA": "Clip.2", "name": "c",
             "source_range": {"start_time": {"value":0,"rate":25}, "duration": {"value":10,"rate":25}},
             "media_reference": {"OTIO_SCHEMA":"ExternalReference.1","target_url":"file:///no/such/file.mp4"}}]}]}
    });
    let e = otio::import(&p, &doc.to_string()).unwrap_err();
    assert!(e.contains("no/such/file.mp4"), "{e}");
}

#[test]
fn edl_round_trip_keeps_cuts_and_speed() {
    let (p, tl) = project();
    let text = edl::export(&p, tl, false).unwrap();
    assert!(text.starts_with("TITLE: Main cut\nFCM: NON-DROP FRAME"));
    assert!(text.contains("* FROM CLIP NAME: Alpha Clip.mp4"));
    assert!(text.contains("M2   BETA"), "{text}");
    // first event: 1 s into the source, 2 s long, at the start
    assert!(
        text.contains(
            "001  ALPHACLI V     C        00:00:01:00 00:00:03:00 00:00:00:00 00:00:02:00"
        ),
        "{text}"
    );
    // import into an empty timeline of a project with the same media
    let mut fresh = Project::new(p.settings.clone());
    fresh.media = p.media.clone();
    let tid = Id::new();
    let mut tl2 = Timeline::new(tid, "new", R);
    for (n, k) in [
        ("V1", TrackKind::Video),
        ("V2", TrackKind::Video),
        ("A1", TrackKind::Audio),
    ] {
        tl2.tracks.push(Arc::new(Track::new(Id::new(), n, k)));
    }
    fresh.timelines.insert(tid, Arc::new(tl2));
    let ops = edl::import(&fresh, tid, &text).unwrap();
    let back = apply_all(&fresh, ops);
    let tl = back.timeline(tid).unwrap();
    let spans = |i: usize| -> Vec<(i64, i64, i64)> {
        tl.tracks[i]
            .items
            .iter()
            .zip(tl.tracks[i].item_starts())
            .filter_map(|(it, st)| {
                it.as_clip()
                    .map(|c| (st, c.duration.value, c.source_start.value))
            })
            .collect()
    };
    assert_eq!(
        spans(0),
        [(0, 48, 24), (60, 12, 0), (72, 36, 100)].map(|x| x)
    );
    let b = tl.tracks[0].items[2].as_clip().unwrap();
    assert!((b.retime.speed - 2.0).abs() < 1e-6 && !b.retime.reverse);
    assert_eq!(
        tl.tracks[2].items.len(),
        1,
        "the audio event lands on the audio track"
    );
    // an unknown clip is named in the error
    let empty = Project::new(p.settings.clone());
    let mut e2 = empty.clone();
    let tid2 = Id::new();
    let mut t3 = Timeline::new(tid2, "x", R);
    t3.tracks
        .push(Arc::new(Track::new(Id::new(), "V1", TrackKind::Video)));
    e2.timelines.insert(tid2, Arc::new(t3));
    let err = edl::import(&e2, tid2, &text).unwrap_err();
    assert!(err.contains("Alpha Clip.mp4"), "{err}");
    assert!(edl::import(&fresh, tid, "garbage").is_err());
}

#[test]
fn fcpxml_is_well_formed_and_describes_the_cut() {
    let (p, tl) = project();
    let xml = fcpxml::export(&p, tl).unwrap();
    assert!(xml.contains("<fcpxml version=\"1.9\">"));
    assert_eq!(xml.matches("<asset ").count(), 3);
    assert!(xml.contains("frameDuration=\"1/24s\""));
    assert!(xml.contains("Alpha%20Clip.mp4"));
    assert!(
        xml.contains("<gap name=\"Gap\" offset=\"48/24s\" duration=\"12/24s\"/>"),
        "{xml}"
    );
    assert!(
        xml.contains("<timeMap>"),
        "the retimed clip carries a time map"
    );
    // connected clips ride on lanes
    assert!(xml.contains("lane=\"1\""), "{xml}");
    assert!(xml.contains("lane=\"-2\""), "audio goes below: {xml}");
    // well-formed XML, when xmllint is around
    let path = std::env::temp_dir().join(format!("captaine-fcp-{}.fcpxml", std::process::id()));
    std::fs::write(&path, &xml).unwrap();
    if let Ok(out) = std::process::Command::new("xmllint")
        .arg("--noout")
        .arg(&path)
        .output()
    {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn drop_frame_and_urls() {
    assert_eq!(rate_from_f64(23.976), Rational::FPS_23_976);
    assert_eq!(rate_from_f64(29.97), Rational::FPS_29_97);
    assert_eq!(rate_from_f64(25.0), Rational::FPS_25);
    let url = file_url("/a b/c#d.mp4");
    assert_eq!(url, "file:///a%20b/c%23d.mp4");
    assert_eq!(path_from_url(&url), "/a b/c#d.mp4");
    assert_eq!(path_from_url("/plain/path.mp4"), "/plain/path.mp4");
}
