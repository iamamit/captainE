//! Error branches, lifecycle ops and descriptions: the paths the happy-path tests do not reach.

use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_30;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}
fn id(n: u128) -> Id {
    Id::from_u128(n)
}
fn media(n: u128) -> MediaRef {
    MediaRef {
        id: id(n),
        name: format!("m{n}"),
        path: format!("/m{n}"),
        duration: Some(t(500)),
        has_audio: true,
        width: 16,
        height: 9,
        has_video: true,
        proxy: None,
    }
}
fn clip(n: u128, dur: i64) -> Item {
    Item::Clip(Clip::new(id(n), format!("c{n}"), id(1), t(0), t(dur)))
}
fn gap(n: u128, dur: i64) -> Item {
    Item::Gap(Gap {
        id: id(n),
        duration: t(dur),
    })
}

/// media 1; timeline 2 with video track 3 holding [clip 10 (30), gap 11 (20), gap 12 (10), clip 13 (30)].
fn editor() -> Editor {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let mut tr = Track::new(id(3), "V1", TrackKind::Video);
    tr.items = vec![clip(10, 30), gap(11, 20), gap(12, 10), clip(13, 30)];
    let mut tl = Timeline::new(id(2), "main", R);
    tl.tracks.push(Arc::new(tr));
    ed.apply(Author::User, Op::AddMedia { media: media(1) })
        .unwrap();
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed
}

fn err(ed: &mut Editor, op: Op) -> OpError {
    let before = ed.snapshot();
    let e = ed.apply(Author::User, op).unwrap_err();
    assert_eq!(
        ed.project(),
        &before,
        "a failed op must not change the project"
    );
    e
}

#[test]
fn every_error_has_a_readable_message() {
    let all = [
        OpError::NotFound("item", id(1)),
        OpError::IdInUse(id(1)),
        OpError::MediaInUse(id(1)),
        OpError::IndexOutOfBounds { index: 3, len: 2 },
        OpError::BadDuration(id(1)),
        OpError::Inexact("x"),
        OpError::OutOfRange("y"),
        OpError::NotJoinable("z"),
        OpError::BadProps("w"),
    ];
    let text: Vec<String> = all.iter().map(|e| e.to_string()).collect();
    assert!(text[0].contains("item") && text[0].contains("not found"));
    assert!(text[3].contains('3') && text[3].contains('2'));
    for (e, s) in all.iter().zip(&text) {
        assert!(!s.is_empty(), "{e:?}");
    }
    let _: &dyn std::error::Error = &all[0];
}

#[test]
fn every_op_describes_itself() {
    let item = id(10);
    let ops = vec![
        Op::AddMedia { media: media(2) },
        Op::RemoveMedia { id: id(2) },
        Op::AddTimeline {
            timeline: Timeline::new(id(9), "t2", R),
        },
        Op::RemoveTimeline { id: id(9) },
        Op::AddTrack {
            timeline: id(2),
            index: 0,
            track: Track::new(id(8), "A1", TrackKind::Audio),
        },
        Op::RemoveTrack { track: id(8) },
        Op::InsertItem {
            track: id(3),
            index: 2,
            item: gap(50, 1),
        },
        Op::RemoveItem { item },
        Op::ReplaceItem {
            item,
            with: gap(51, 30),
        },
        Op::Split {
            item,
            at: t(5),
            new_id: id(52),
        },
        Op::Join {
            left: item,
            right: id(52),
        },
        Op::Trim {
            item,
            edge: Edge::In,
            delta: t(2),
        },
        Op::SetRange {
            item,
            source_start: t(1),
            duration: t(2),
        },
        Op::ReplaceRaw {
            item,
            with: gap(53, 3),
        },
        Op::Slip { item, delta: t(1) },
        Op::SetVideo {
            item,
            video: VideoProps::default(),
        },
        Op::SetAudio {
            item,
            audio: AudioProps::default(),
        },
        Op::SetTrack {
            track: id(3),
            enabled: false,
            gain_db: 0.0,
        },
        Op::MoveTrack {
            track: id(3),
            index: 0,
        },
        Op::SetProxy {
            media: id(1),
            proxy: None,
        },
        Op::SetEnabled {
            item,
            enabled: true,
        },
        Op::SetEnabled {
            item,
            enabled: false,
        },
        Op::SetName {
            item,
            name: "x".into(),
        },
        Op::Transaction {
            description: "Bundle".into(),
            ops: vec![],
        },
    ];
    for op in &ops {
        assert!(!op.describe().is_empty(), "{op:?}");
    }
    assert_eq!(ops.last().unwrap().describe(), "Bundle");
    assert!(ops[10].describe().starts_with("Join"));
    assert!(ops[20].describe().starts_with("Enable") && ops[21].describe().starts_with("Disable"));
}

#[test]
fn timeline_and_track_lifecycle() {
    let mut ed = editor();
    let before = ed.snapshot();
    // second timeline with its own track and clip, then remove it again
    let mut tl2 = Timeline::new(id(20), "second", R);
    let mut tr = Track::new(id(21), "V1", TrackKind::Video);
    tr.items.push(clip(22, 15));
    tl2.tracks.push(Arc::new(tr));
    ed.apply(Author::User, Op::AddTimeline { timeline: tl2 })
        .unwrap();
    assert_eq!(ed.project().timelines.len(), 2);
    ed.apply(Author::User, Op::RemoveTimeline { id: id(20) })
        .unwrap();
    assert!(ed.undo()); // brings it back
    assert!(ed.project().timeline(id(20)).is_some());
    ed.undo(); // and the add itself
    assert_eq!(ed.project(), &before);

    // rejected timelines
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTimeline {
                timeline: Timeline::new(id(2), "dup", R)
            }
        ),
        OpError::IdInUse(_)
    ));
    let mut dup_tracks = Timeline::new(id(30), "d", R);
    dup_tracks
        .tracks
        .push(Arc::new(Track::new(id(3), "V1", TrackKind::Video)));
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTimeline {
                timeline: dup_tracks
            }
        ),
        OpError::IdInUse(_)
    ));
    let mut dup_items = Timeline::new(id(31), "d", R);
    let mut tr = Track::new(id(32), "V1", TrackKind::Video);
    tr.items = vec![gap(33, 5), gap(33, 5)];
    dup_items.tracks.push(Arc::new(tr));
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTimeline {
                timeline: dup_items
            }
        ),
        OpError::IdInUse(_)
    ));
    let mut dup_across = Timeline::new(id(34), "d", R);
    let mut tr = Track::new(id(35), "V1", TrackKind::Video);
    tr.items = vec![gap(36, 5)];
    dup_across.tracks.push(Arc::new(tr));
    let mut tr2 = Track::new(id(37), "V2", TrackKind::Video);
    tr2.items = vec![gap(36, 5)];
    dup_across.tracks.push(Arc::new(tr2));
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTimeline {
                timeline: dup_across
            }
        ),
        OpError::IdInUse(_)
    ));
    let mut bad_item = Timeline::new(id(38), "d", R);
    let mut tr = Track::new(id(39), "V1", TrackKind::Video);
    tr.items = vec![Item::Gap(Gap {
        id: id(40),
        duration: RationalTime::new(5, Rational::FPS_24),
    })];
    bad_item.tracks.push(Arc::new(tr));
    assert!(matches!(
        err(&mut ed, Op::AddTimeline { timeline: bad_item }),
        OpError::BadDuration(_)
    ));
    assert!(matches!(
        err(&mut ed, Op::RemoveTimeline { id: id(99) }),
        OpError::NotFound("timeline", _)
    ));

    // tracks
    let new_track = |n: u128| Track::new(id(n), "A1", TrackKind::Audio);
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTrack {
                timeline: id(99),
                index: 0,
                track: new_track(60)
            }
        ),
        OpError::NotFound("timeline", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTrack {
                timeline: id(2),
                index: 5,
                track: new_track(60)
            }
        ),
        OpError::IndexOutOfBounds { .. }
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTrack {
                timeline: id(2),
                index: 0,
                track: new_track(3)
            }
        ),
        OpError::IdInUse(_)
    ));
    let mut dup_in_track = new_track(61);
    dup_in_track.items = vec![gap(62, 5), gap(62, 5)];
    assert!(matches!(
        err(
            &mut ed,
            Op::AddTrack {
                timeline: id(2),
                index: 0,
                track: dup_in_track
            }
        ),
        OpError::IdInUse(_)
    ));
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: id(2),
            index: 1,
            track: new_track(63),
        },
    )
    .unwrap();
    assert!(matches!(
        err(
            &mut ed,
            Op::MoveTrack {
                track: id(63),
                index: 7
            }
        ),
        OpError::IndexOutOfBounds { .. }
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::MoveTrack {
                track: id(98),
                index: 0
            }
        ),
        OpError::NotFound("track", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetTrack {
                track: id(63),
                enabled: true,
                gain_db: f64::INFINITY
            }
        ),
        OpError::BadProps(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetTrack {
                track: id(98),
                enabled: true,
                gain_db: 0.0
            }
        ),
        OpError::NotFound("track", _)
    ));
    ed.apply(Author::User, Op::RemoveTrack { track: id(63) })
        .unwrap();
    assert!(matches!(
        err(&mut ed, Op::RemoveTrack { track: id(63) }),
        OpError::NotFound("track", _)
    ));
}

#[test]
fn media_and_proxy() {
    let mut ed = editor();
    assert!(matches!(
        err(&mut ed, Op::AddMedia { media: media(1) }),
        OpError::IdInUse(_)
    ));
    assert!(matches!(
        err(&mut ed, Op::RemoveMedia { id: id(77) }),
        OpError::NotFound("media", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetProxy {
                media: id(77),
                proxy: None
            }
        ),
        OpError::NotFound("media", _)
    ));
    ed.apply(
        Author::User,
        Op::SetProxy {
            media: id(1),
            proxy: Some("/p.mp4".into()),
        },
    )
    .unwrap();
    assert_eq!(ed.project().media[&id(1)].proxy.as_deref(), Some("/p.mp4"));
    ed.undo();
    assert_eq!(ed.project().media[&id(1)].proxy, None);
}

#[test]
fn gaps_split_trim_and_join() {
    let mut ed = editor();
    // trim a gap from either end, exact undo
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(11),
            edge: Edge::Out,
            delta: t(5),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(11),
            edge: Edge::In,
            delta: t(5),
        },
    )
    .unwrap();
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
    // split a gap, join the halves; join the two neighbouring gaps
    ed.apply(
        Author::User,
        Op::Split {
            item: id(11),
            at: t(8),
            new_id: id(70),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::Join {
            left: id(11),
            right: id(70),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::Join {
            left: id(11),
            right: id(12),
        },
    )
    .unwrap();
    let tr = &ed.project().timeline(id(2)).unwrap().tracks[0];
    assert_eq!(tr.items[1].duration(), t(30));
    ed.undo();
    ed.undo();
    ed.undo();
    assert_eq!(
        ed.project().timeline(id(2)).unwrap().tracks[0].items.len(),
        4
    );

    // joins that must fail
    assert!(matches!(
        err(
            &mut ed,
            Op::Join {
                left: id(10),
                right: id(11)
            }
        ),
        OpError::NotJoinable(_)
    )); // clip + gap
    assert!(matches!(
        err(
            &mut ed,
            Op::Join {
                left: id(10),
                right: id(13)
            }
        ),
        OpError::NotJoinable(_)
    )); // not neighbours
    assert!(matches!(
        err(
            &mut ed,
            Op::Join {
                left: id(10),
                right: id(99)
            }
        ),
        OpError::NotFound("item", _)
    ));
    // properties that differ block a join
    let mut ed2 = editor();
    ed2.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(10),
            new_id: id(71),
        },
    )
    .unwrap();
    ed2.apply(
        Author::User,
        Op::SetEnabled {
            item: id(71),
            enabled: false,
        },
    )
    .unwrap();
    assert!(matches!(
        err(
            &mut ed2,
            Op::Join {
                left: id(10),
                right: id(71)
            }
        ),
        OpError::NotJoinable(_)
    ));
    ed2.apply(
        Author::User,
        Op::SetEnabled {
            item: id(71),
            enabled: true,
        },
    )
    .unwrap();
    let louder = AudioProps {
        gain_db: 3.0,
        ..AudioProps::default()
    };
    ed2.apply(
        Author::User,
        Op::SetAudio {
            item: id(71),
            audio: louder,
        },
    )
    .unwrap();
    assert!(matches!(
        err(
            &mut ed2,
            Op::Join {
                left: id(10),
                right: id(71)
            }
        ),
        OpError::NotJoinable(_)
    ));
    ed2.apply(
        Author::User,
        Op::SetAudio {
            item: id(71),
            audio: AudioProps::default(),
        },
    )
    .unwrap();
    ed2.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(71),
        },
    )
    .unwrap();

    // split / trim / insert / replace errors
    assert!(matches!(
        err(
            &mut ed,
            Op::Split {
                item: id(10),
                at: t(5),
                new_id: id(10)
            }
        ),
        OpError::IdInUse(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::Split {
                item: id(10),
                at: RationalTime::new(5, Rational::FPS_24),
                new_id: id(72)
            }
        ),
        OpError::OutOfRange(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::Split {
                item: id(99),
                at: t(5),
                new_id: id(72)
            }
        ),
        OpError::NotFound("item", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::Trim {
                item: id(10),
                edge: Edge::Out,
                delta: RationalTime::new(1, Rational::FPS_24)
            }
        ),
        OpError::BadDuration(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::Trim {
                item: id(11),
                edge: Edge::In,
                delta: t(20)
            }
        ),
        OpError::BadDuration(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::InsertItem {
                track: id(98),
                index: 0,
                item: gap(73, 1)
            }
        ),
        OpError::NotFound("track", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::ReplaceItem {
                item: id(10),
                with: gap(13, 30)
            }
        ),
        OpError::IdInUse(_)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::ReplaceItem {
                item: id(99),
                with: gap(74, 30)
            }
        ),
        OpError::NotFound("item", _)
    ));
    assert!(matches!(
        err(&mut ed, Op::RemoveItem { item: id(99) }),
        OpError::NotFound("item", _)
    ));
}

#[test]
fn clip_only_ops_refuse_gaps() {
    let mut ed = editor();
    let gap_id = id(11);
    let d = t(1);
    assert!(matches!(
        err(
            &mut ed,
            Op::Slip {
                item: gap_id,
                delta: d
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetRange {
                item: gap_id,
                source_start: t(0),
                duration: t(5)
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetVideo {
                item: gap_id,
                video: VideoProps::default()
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetAudio {
                item: gap_id,
                audio: AudioProps::default()
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetEnabled {
                item: gap_id,
                enabled: false
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::SetName {
                item: gap_id,
                name: "x".into()
            }
        ),
        OpError::NotFound("clip", _)
    ));
    assert!(matches!(
        err(
            &mut ed,
            Op::Slip {
                item: id(10),
                delta: RationalTime::new(1, Rational::FPS_24)
            }
        ),
        OpError::BadDuration(_)
    ));
    // SetRange is exact and strict
    ed.apply(
        Author::User,
        Op::SetRange {
            item: id(10),
            source_start: t(100),
            duration: t(40),
        },
    )
    .unwrap();
    assert!(matches!(
        err(
            &mut ed,
            Op::SetRange {
                item: id(10),
                source_start: t(499),
                duration: t(40)
            }
        ),
        OpError::OutOfRange(_)
    ));
    // ReplaceRaw may change the length
    ed.apply(
        Author::User,
        Op::ReplaceRaw {
            item: id(10),
            with: clip(10, 12),
        },
    )
    .unwrap();
    assert_eq!(
        ed.project().timeline(id(2)).unwrap().tracks[0].items[0].duration(),
        t(12)
    );
    assert!(matches!(
        err(
            &mut ed,
            Op::ReplaceRaw {
                item: id(99),
                with: gap(75, 1)
            }
        ),
        OpError::NotFound("item", _)
    ));
}

#[test]
fn trimming_a_clip_shortens_fades_that_no_longer_fit_and_undo_restores_them() {
    let mut ed = editor();
    let faded = AudioProps {
        fade_in: 12,
        fade_out: 12,
        ..AudioProps::default()
    };
    ed.apply(
        Author::User,
        Op::SetAudio {
            item: id(10),
            audio: faded,
        },
    )
    .unwrap();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::Out,
            delta: t(-20),
        },
    )
    .unwrap(); // 30 -> 10 frames
    let Item::Clip(c) = &ed.project().timeline(id(2)).unwrap().tracks[0].items[0] else {
        panic!()
    };
    assert_eq!((c.audio.fade_out, c.audio.fade_in), (10, 0));
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn split_clamps_fades_and_undoes_exactly() {
    let mut ed = editor();
    let faded = AudioProps {
        fade_in: 12,
        fade_out: 12,
        ..AudioProps::default()
    };
    ed.apply(
        Author::User,
        Op::SetAudio {
            item: id(10),
            audio: faded,
        },
    )
    .unwrap();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(5),
            new_id: id(80),
        },
    )
    .unwrap();
    let items = &ed.project().timeline(id(2)).unwrap().tracks[0].items;
    let (l, r) = (items[0].as_clip().unwrap(), items[1].as_clip().unwrap());
    assert_eq!((l.audio.fade_in, l.audio.fade_out), (5, 0));
    assert_eq!((r.audio.fade_in, r.audio.fade_out), (0, 12));
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn editor_edges() {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    assert!(!ed.can_undo() && !ed.can_redo());
    assert!(!ed.undo());
    assert!(!ed.redo());
    ed.apply(
        Author::Script("test".into()),
        Op::AddMedia { media: media(1) },
    )
    .unwrap();
    assert!(ed.can_undo());
    assert_eq!(ed.history()[0].author, Author::Script("test".into()));
    let snap = ed.snapshot();
    ed.undo();
    assert_ne!(ed.project(), &snap);
    assert!(ed.can_redo());
    // the other authors exist and serialize
    for a in [Author::User, Author::Agent("codit".into()), Author::Merge] {
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Author>(&j).unwrap(), a);
    }
}

#[test]
fn edit_helpers_report_bad_input() {
    let ed = editor();
    let p = ed.project();
    let bad_rate = RationalTime::new(1, Rational::FPS_24);
    assert!(edit::split_at(p, id(3), bad_rate).is_err());
    assert!(edit::split_at(p, id(98), t(5)).is_err());
    assert!(edit::overwrite(
        p,
        id(3),
        bad_rate,
        Clip::new(id(90), "x", id(1), t(0), t(5))
    )
    .is_err());
    assert!(edit::overwrite(p, id(3), t(-5), Clip::new(id(90), "x", id(1), t(0), t(5))).is_err());
    assert!(edit::lift(p, id(99)).is_err());
    assert!(edit::move_item(p, id(99), id(3), t(0)).is_err());
    assert!(edit::move_item(p, id(11), id(3), t(0)).is_err()); // gaps cannot be dragged
    assert!(edit::move_item(p, id(10), id(3), bad_rate).is_err());
    assert!(edit::roll(p, id(10), id(13), t(1)).is_err());
    assert!(edit::roll(p, id(99), id(13), t(1)).is_err());
    assert!(edit::slide(p, id(99), t(1)).is_err());
    assert!(edit::slide(p, id(13), t(1)).is_err()); // last item has no right neighbour
                                                    // append to a track that does not exist just yields an op that will fail on apply
    let op = edit::append(p, id(98), Clip::new(id(91), "x", id(1), t(0), t(5)));
    assert!(apply(p, &op).is_err());
}
