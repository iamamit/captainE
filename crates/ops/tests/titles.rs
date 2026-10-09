//! Titles are items like clips: they split, trim, join, move, lift and undo exactly.

use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_24;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}
fn id(n: u128) -> Id {
    Id::from_u128(n)
}

fn title(n: u128, dur: i64) -> Title {
    let mut x = Title::new(id(n), TitleTemplate::LowerThird, t(dur));
    x.text = "Coastline".into();
    x.subtitle = "Day 3".into();
    x
}

/// timeline 2 with V1 (3) and V2 (4); V2 holds a 72-frame title at 0
fn editor() -> Editor {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let mut tl = Timeline::new(id(2), "main", R);
    tl.tracks
        .push(Arc::new(Track::new(id(3), "V1", TrackKind::Video)));
    let mut v2 = Track::new(id(4), "V2", TrackKind::Video);
    v2.items.push(Item::Title(title(10, 72)));
    tl.tracks.push(Arc::new(v2));
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed
}

fn v2(ed: &Editor) -> Vec<Item> {
    ed.project().timeline(id(2)).unwrap().tracks[1]
        .items
        .clone()
}

#[test]
fn split_trim_join_and_undo() {
    let mut ed = editor();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(30),
            new_id: id(11),
        },
    )
    .unwrap();
    assert_eq!(
        v2(&ed)
            .iter()
            .map(|i| i.duration().value)
            .collect::<Vec<_>>(),
        [30, 42]
    );
    assert_eq!(v2(&ed)[1].as_title().unwrap().text, "Coastline");
    ed.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(11),
        },
    )
    .unwrap();
    assert_eq!(v2(&ed)[0].duration(), t(72));
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);

    // trims change only the length, and undo is exact (even when fades had to shrink)
    let mut tt = title(10, 72);
    tt.fade_in = 20;
    tt.fade_out = 20;
    ed.apply(
        Author::User,
        Op::SetTitle {
            item: id(10),
            title: tt.clone(),
        },
    )
    .unwrap();
    let with_fades = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::Out,
            delta: t(-50),
        },
    )
    .unwrap(); // 72 -> 22
    let shrunk = v2(&ed)[0].as_title().unwrap().clone();
    assert_eq!(shrunk.duration, t(22));
    assert!(shrunk.fade_in + shrunk.fade_out <= 22);
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(2),
        },
    )
    .unwrap();
    assert_eq!(v2(&ed)[0].duration(), t(20));
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &with_fades);
    assert!(ed
        .apply(
            Author::User,
            Op::Trim {
                item: id(10),
                edge: Edge::In,
                delta: t(72)
            }
        )
        .is_err());
}

#[test]
fn joins_only_identical_titles() {
    let mut ed = editor();
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(30),
            new_id: id(11),
        },
    )
    .unwrap();
    let mut changed = v2(&ed)[1].as_title().unwrap().clone();
    changed.text = "Different".into();
    ed.apply(
        Author::User,
        Op::SetTitle {
            item: id(11),
            title: changed,
        },
    )
    .unwrap();
    assert!(matches!(
        ed.apply(
            Author::User,
            Op::Join {
                left: id(10),
                right: id(11)
            }
        )
        .unwrap_err(),
        OpError::NotJoinable(_)
    ));
}

#[test]
fn edit_properties_through_ops() {
    let mut ed = editor();
    let mut tt = title(10, 72);
    tt.text = "New text".into();
    tt.style.template = TitleTemplate::Centered;
    tt.style.size = 120.0;
    tt.fade_in = 12;
    ed.apply(
        Author::User,
        Op::SetTitle {
            item: id(10),
            title: tt.clone(),
        },
    )
    .unwrap();
    assert_eq!(v2(&ed)[0].as_title().unwrap(), &tt);
    // the id and the length are fixed; bad values are refused
    assert!(ed
        .apply(
            Author::User,
            Op::SetTitle {
                item: id(10),
                title: Title {
                    duration: t(10),
                    ..tt.clone()
                }
            }
        )
        .is_err());
    assert!(ed
        .apply(
            Author::User,
            Op::SetTitle {
                item: id(10),
                title: Title {
                    id: id(99),
                    ..tt.clone()
                }
            }
        )
        .is_err());
    assert!(ed
        .apply(
            Author::User,
            Op::SetTitle {
                item: id(10),
                title: Title {
                    fade_in: 80,
                    ..tt.clone()
                }
            }
        )
        .is_err());
    let mut bad = tt.clone();
    bad.style.size = 0.0;
    assert!(ed
        .apply(
            Author::User,
            Op::SetTitle {
                item: id(10),
                title: bad
            }
        )
        .is_err());
    assert!(matches!(
        ed.apply(
            Author::User,
            Op::SetTitle {
                item: id(77),
                title: tt.clone()
            }
        )
        .unwrap_err(),
        OpError::NotFound(..)
    ));
    // transform, name and enabled work on titles as on clips
    let video = VideoProps {
        opacity: 0.5,
        transform: Transform {
            position: (10.0, 20.0),
            ..Transform::default()
        },
        ..VideoProps::default()
    };
    ed.apply(
        Author::User,
        Op::SetVideo {
            item: id(10),
            video,
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::SetName {
            item: id(10),
            name: "Lower third".into(),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::SetEnabled {
            item: id(10),
            enabled: false,
        },
    )
    .unwrap();
    let got = v2(&ed)[0].as_title().unwrap().clone();
    assert_eq!(
        (got.video, got.name.as_str(), got.enabled),
        (video, "Lower third", false)
    );
    ed.apply(
        Author::User,
        Op::SetVideo {
            item: id(10),
            video: VideoProps {
                opacity: 3.0,
                ..video
            },
        },
    )
    .unwrap_err();
    for _ in 0..3 {
        ed.undo(); // enabled, name, video
    }
    assert_eq!(v2(&ed)[0].as_title().unwrap(), &tt.clone());
    ed.project().validate().unwrap();
}

#[test]
fn titles_move_lift_and_survive_json() {
    let mut ed = editor();
    // drag the title from V2 to V1 at frame 48 (overwrite semantics)
    let op = edit::move_item(ed.project(), id(10), id(3), t(48)).unwrap();
    ed.apply(Author::User, op).unwrap();
    let tl = ed.project().timeline(id(2)).unwrap();
    assert!(
        tl.tracks[1].items.is_empty(),
        "the track it left is tidied up"
    );
    assert_eq!(tl.tracks[0].item_starts().last().copied(), Some(48));
    assert!(tl.tracks[0].items.last().unwrap().as_title().is_some());
    // lift leaves a gap
    let op = edit::lift(ed.project(), id(10)).unwrap();
    ed.apply(Author::User, op).unwrap();
    assert!(ed.project().timeline(id(2)).unwrap().tracks[0]
        .items
        .iter()
        .all(|i| i.as_title().is_none()));
    ed.undo();
    ed.undo();
    // range gestures treat titles as content
    let op = edit::lift_range(ed.project(), id(2), t(0), t(60)).unwrap();
    ed.apply(Author::User, op).unwrap();
    ed.project().validate().unwrap();
    ed.undo();
    // the log survives a round trip through JSON
    let json = serde_json::to_string(ed.history()).unwrap();
    let back: Vec<HistoryEntry> = serde_json::from_str(&json).unwrap();
    let mut replay = Editor::new(Project::new(ProjectSettings::default()));
    for e in back {
        replay.apply(e.author, e.op).unwrap();
    }
    assert_eq!(replay.project(), ed.project());
}
