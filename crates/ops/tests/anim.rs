//! Keyframes through the command layer.

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
fn k(t: i64, v: f64) -> Keyframe {
    Keyframe::new(t, v)
}

/// V1 holds clip 10 (0..120 frames) and a title 11 (120..144); A1 empty.
fn editor() -> Editor {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let media = MediaRef {
        id: id(1),
        name: "m".into(),
        path: "/m".into(),
        duration: Some(t(1000)),
        has_audio: true,
        width: 16,
        height: 9,
        has_video: true,
        proxy: None,
    };
    let mut v1 = Track::new(id(3), "V1", TrackKind::Video);
    v1.items = vec![
        Item::Clip(Clip::new(id(10), "c", id(1), t(50), t(120))),
        Item::Title(Title::new(id(11), TitleTemplate::Centered, t(24))),
    ];
    let mut tl = Timeline::new(id(2), "main", R);
    tl.tracks.push(Arc::new(v1));
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed
}

fn clip(ed: &Editor, n: u128) -> Clip {
    ed.project()
        .timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .find_map(|i| i.as_clip().filter(|c| c.id == id(n)).cloned())
        .unwrap()
}

fn set(item: u128, prop: &str, keys: Vec<Keyframe>) -> Op {
    Op::SetAnimTrack {
        item: id(item),
        prop: prop.into(),
        keys,
    }
}

#[test]
fn set_track_validates_and_undoes_exactly() {
    let mut ed = editor();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        set(10, "opacity", vec![k(0, 0.0), k(48, 100.0)]),
    )
    .unwrap();
    assert_eq!(clip(&ed, 10).video_at(24).opacity, 0.5);
    assert_eq!(clip(&ed, 10).video_at(0).opacity, 0.0);
    assert_eq!(
        clip(&ed, 10).video_at(500).opacity,
        1.0,
        "after the last key it holds"
    );
    assert_eq!(
        clip(&ed, 10).video.opacity,
        1.0,
        "the base value is untouched"
    );
    // keys given out of order are sorted
    ed.apply(
        Author::User,
        set(10, "position.x", vec![k(60, 100.0), k(0, 0.0)]),
    )
    .unwrap();
    assert_eq!(clip(&ed, 10).video_at(30).transform.position.0, 50.0);
    // an empty list removes the track
    ed.apply(Author::User, set(10, "position.x", vec![]))
        .unwrap();
    assert!(!clip(&ed, 10).anim.has("position.x"));
    ed.undo();
    assert!(clip(&ed, 10).anim.has("position.x"));
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
    // bad input is refused and changes nothing
    let e = |ed: &mut Editor, op| ed.apply(Author::User, op).unwrap_err();
    assert!(matches!(
        e(&mut ed, set(10, "nonsense", vec![k(0, 1.0)])),
        OpError::BadProps(_)
    ));
    assert!(matches!(
        e(&mut ed, set(10, "opacity", vec![k(0, 500.0)])),
        OpError::BadProps(_)
    ));
    assert!(matches!(
        e(&mut ed, set(10, "pan", vec![k(0, 3.0)])),
        OpError::BadProps(_)
    ));
    assert!(matches!(
        e(&mut ed, set(10, "opacity", vec![k(0, f64::NAN)])),
        OpError::BadProps(_)
    ));
    assert!(
        matches!(
            e(&mut ed, set(11, "volume", vec![k(0, -6.0)])),
            OpError::BadProps(_)
        ),
        "a title has no sound"
    );
    assert!(matches!(
        e(&mut ed, set(99, "opacity", vec![k(0, 1.0)])),
        OpError::NotFound(..)
    ));
    let bad = Keyframe {
        t: 0,
        v: 1.0,
        interp: Interp::Bezier(2.0, 0.0, 0.5, 1.0),
    };
    assert!(matches!(
        e(&mut ed, set(10, "opacity", vec![bad])),
        OpError::BadProps(_)
    ));
    assert_eq!(ed.project(), &before);
    // titles can be animated too, and so can volume on a clip
    ed.apply(
        Author::User,
        set(11, "scale", vec![k(0, 50.0), k(24, 100.0)]),
    )
    .unwrap();
    ed.apply(
        Author::User,
        set(10, "volume", vec![k(0, -20.0), k(100, 0.0)]),
    )
    .unwrap();
    assert_eq!(clip(&ed, 10).audio_at(50).gain_db, -10.0);
    let title = ed.project().timelines.values().next().unwrap().tracks[0].items[1]
        .as_title()
        .unwrap()
        .clone();
    assert_eq!(title.video_at(12).transform.scale.0, 0.75);
    assert!(
        ed.history()
            .last()
            .unwrap()
            .op
            .describe()
            .contains("volume")
            || ed
                .history()
                .iter()
                .any(|h| h.op.describe().contains("volume"))
    );
}

#[test]
fn animation_follows_the_picture_when_the_front_is_trimmed() {
    let mut ed = editor();
    ed.apply(
        Author::User,
        set(10, "opacity", vec![k(0, 0.0), k(100, 100.0)]),
    )
    .unwrap();
    let before = ed.snapshot();
    let at_old_60 = clip(&ed, 10).video_at(60).opacity;
    // cut 30 frames off the front: what was at frame 60 is now at frame 30
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(30),
        },
    )
    .unwrap();
    assert!((clip(&ed, 10).video_at(30).opacity - at_old_60).abs() < 1e-9);
    assert_eq!(clip(&ed, 10).source_start, t(80));
    ed.undo();
    assert_eq!(
        ed.project(),
        &before,
        "undoing the trim restores the keyframes too"
    );
    // trimming the end leaves keys alone
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::Out,
            delta: t(-20),
        },
    )
    .unwrap();
    assert_eq!(
        clip(&ed, 10).anim,
        before.timelines.values().next().unwrap().tracks[0].items[0]
            .as_clip()
            .unwrap()
            .anim
    );
}

#[test]
fn cutting_keeps_the_animation_continuous_and_joining_puts_it_back() {
    let mut ed = editor();
    ed.apply(
        Author::User,
        set(10, "position.x", vec![k(0, 0.0), k(120, 240.0)]),
    )
    .unwrap();
    ed.apply(
        Author::User,
        set(10, "opacity", vec![k(10, 10.0), k(50, 90.0), k(100, 30.0)]),
    )
    .unwrap();
    let before = ed.snapshot();
    let whole = clip(&ed, 10);
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(40),
            new_id: id(12),
        },
    )
    .unwrap();
    let (l, r) = (clip(&ed, 10), clip(&ed, 12));
    for tick in [0, 10, 25, 39] {
        assert!(
            (l.video_at(tick).transform.position.0 - whole.video_at(tick).transform.position.0)
                .abs()
                < 1e-9,
            "left at {tick}"
        );
        assert!(
            (l.video_at(tick).opacity - whole.video_at(tick).opacity).abs() < 1e-9,
            "left opacity {tick}"
        );
    }
    for tick in [0, 5, 40, 79] {
        assert!(
            (r.video_at(tick).transform.position.0
                - whole.video_at(40 + tick).transform.position.0)
                .abs()
                < 1e-9,
            "right at {tick}"
        );
        assert!(
            (r.video_at(tick).opacity - whole.video_at(40 + tick).opacity).abs() < 1e-9,
            "right opacity {tick}"
        );
    }
    // joining the two halves gives back an animation that behaves like the original
    ed.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(12),
        },
    )
    .unwrap();
    let joined = clip(&ed, 10);
    for tick in 0..120 {
        assert!(
            (joined.video_at(tick).opacity - whole.video_at(tick).opacity).abs() < 1e-9,
            "joined opacity at {tick}"
        );
    }
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
    // two clips animated differently are not one clip
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(40),
            new_id: id(12),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        set(12, "opacity", vec![k(0, 5.0), k(10, 6.0)]),
    )
    .unwrap();
    assert!(matches!(
        ed.apply(
            Author::User,
            Op::Join {
                left: id(10),
                right: id(12)
            }
        )
        .unwrap_err(),
        OpError::NotJoinable(_)
    ));
}

#[test]
fn titles_split_and_trim_their_animation_too() {
    let mut ed = editor();
    ed.apply(
        Author::User,
        set(11, "opacity", vec![k(0, 0.0), k(24, 100.0)]),
    )
    .unwrap();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Split {
            item: id(11),
            at: t(12),
            new_id: id(13),
        },
    )
    .unwrap();
    let items = &ed.project().timelines.values().next().unwrap().tracks[0].items;
    let (l, r) = (items[1].as_title().unwrap(), items[2].as_title().unwrap());
    assert!(
        (l.video_at(6).opacity - 0.25).abs() < 1e-9
            && (r.video_at(0).opacity - 0.5).abs() < 1e-9
            && (r.video_at(6).opacity - 0.75).abs() < 1e-9
    );
    ed.undo();
    assert_eq!(ed.project(), &before);
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(11),
            edge: Edge::In,
            delta: t(12),
        },
    )
    .unwrap();
    let tt = ed.project().timelines.values().next().unwrap().tracks[0].items[1]
        .as_title()
        .unwrap()
        .clone();
    assert!(
        (tt.video_at(0).opacity - 0.5).abs() < 1e-9,
        "the title now starts half way through its fade"
    );
    ed.undo();
    assert_eq!(ed.project(), &before);
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn fuzz_animation_with_cuts_trims_and_undo() {
    for seed in 1..=60u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ed = editor();
        let mut states = vec![ed.snapshot()];
        let mut next = 1000u128;
        for _ in 0..80 {
            let items: Vec<Id> = ed.project().timelines.values().next().unwrap().tracks[0]
                .items
                .iter()
                .filter(|i| i.is_content())
                .map(|i| i.id())
                .collect();
            let pick = items[rng.below(items.len())];
            next += 1;
            let op = match rng.below(5) {
                0 | 1 => {
                    let props = ["opacity", "position.x", "scale", "rotation", "exposure"];
                    let p = props[rng.below(props.len())];
                    let keys: Vec<Keyframe> = (0..1 + rng.below(4))
                        .map(|_| Keyframe {
                            t: rng.below(120) as i64,
                            v: if p == "opacity" || p == "scale" {
                                10.0 + rng.below(90) as f64
                            } else {
                                rng.below(100) as f64 - 50.0
                            },
                            interp: [Interp::Linear, Interp::Hold, Interp::EaseInOut][rng.below(3)],
                        })
                        .collect();
                    Op::SetAnimTrack {
                        item: pick,
                        prop: p.into(),
                        keys,
                    }
                }
                2 => Op::Split {
                    item: pick,
                    at: t(1 + rng.below(30) as i64),
                    new_id: id(next),
                },
                3 => Op::Trim {
                    item: pick,
                    edge: if rng.below(2) == 0 {
                        Edge::In
                    } else {
                        Edge::Out
                    },
                    delta: t(rng.below(21) as i64 - 10),
                },
                _ => {
                    let a = items[rng.below(items.len())];
                    let b = items[rng.below(items.len())];
                    Op::Join { left: a, right: b }
                }
            };
            if ed.apply(Author::User, op).is_ok() {
                ed.project()
                    .validate()
                    .unwrap_or_else(|e| panic!("seed {seed}: {e:?}"));
                states.push(ed.snapshot());
            }
        }
        assert!(states.len() > 20, "seed {seed}: {}", states.len());
        for expected in states.iter().rev().skip(1) {
            assert!(ed.undo());
            assert_eq!(ed.project(), expected, "seed {seed}: undo mismatch");
        }
        for expected in states.iter().skip(1) {
            assert!(ed.redo());
            assert_eq!(ed.project(), expected, "seed {seed}: redo mismatch");
        }
    }
}
