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
fn clip(n: u128, media: u128, src: i64, dur: i64) -> Item {
    Item::Clip(Clip::new(id(n), format!("c{n}"), id(media), t(src), t(dur)))
}
fn gap(n: u128, dur: i64) -> Item {
    Item::Gap(Gap {
        id: id(n),
        duration: t(dur),
    })
}

const MEDIA: u128 = 1;
const MEDIA24: u128 = 8;
const TL: u128 = 2;
const V1: u128 = 3;

/// Project with one 1000-frame media, one timeline, one empty video track.
fn editor() -> Editor {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let media = MediaRef {
        id: id(MEDIA),
        name: "m.mov".into(),
        path: "/m.mov".into(),
        duration: Some(t(1000)),
        has_audio: true,
        width: 1920,
        height: 1080,
        has_video: true,
        proxy: None,
    };
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    let m24 = MediaRef {
        id: id(MEDIA24),
        name: "m24.mov".into(),
        path: "/m24.mov".into(),
        duration: Some(RationalTime::new(800, Rational::FPS_24)),
        has_audio: true,
        width: 1920,
        height: 1080,
        has_video: true,
        proxy: None,
    };
    ed.apply(Author::User, Op::AddMedia { media: m24 }).unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(id(TL), "main", R),
        },
    )
    .unwrap();
    let track = Track::new(id(V1), "V1", TrackKind::Video);
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: id(TL),
            index: 0,
            track,
        },
    )
    .unwrap();
    ed
}

fn insert(ed: &mut Editor, index: usize, item: Item) {
    ed.apply(
        Author::User,
        Op::InsertItem {
            track: id(V1),
            index,
            item,
        },
    )
    .unwrap();
}

fn durations(ed: &Editor) -> Vec<i64> {
    let tl = ed.project().timeline(id(TL)).unwrap();
    tl.tracks[0]
        .items
        .iter()
        .map(|i| i.duration().value)
        .collect()
}

fn find(ed: &Editor, n: u128) -> Item {
    let loc = ed.project().find_item(id(n)).unwrap();
    ed.project().item(loc).unwrap().clone()
}

#[test]
fn insert_and_ripple_delete() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    insert(&mut ed, 1, clip(11, MEDIA, 100, 60));
    insert(&mut ed, 1, clip(12, MEDIA, 200, 10));
    assert_eq!(durations(&ed), [30, 10, 60]);
    ed.apply(Author::User, Op::RemoveItem { item: id(12) })
        .unwrap();
    assert_eq!(durations(&ed), [30, 60]);
    assert_eq!(ed.project().timeline(id(TL)).unwrap().duration(), t(90));
    ed.undo();
    assert_eq!(durations(&ed), [30, 10, 60]);
}

#[test]
fn split_then_join_restores() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 50, 90));
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
    let Item::Clip(right) = find(&ed, 11) else {
        panic!()
    };
    assert_eq!(right.source_start, t(80));
    assert_eq!(right.duration, t(60));
    ed.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(11),
        },
    )
    .unwrap();
    assert_eq!(ed.project(), &before);
}

#[test]
fn join_rejects_non_continuous() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    insert(&mut ed, 1, clip(11, MEDIA, 31, 30)); // 1-frame jump in the source
    let err = ed
        .apply(
            Author::User,
            Op::Join {
                left: id(10),
                right: id(11),
            },
        )
        .unwrap_err();
    assert!(matches!(err, OpError::NotJoinable(_)));
}

#[test]
fn trim_in_and_out() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 100, 50));
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(10),
        },
    )
    .unwrap();
    let Item::Clip(c) = find(&ed, 10) else {
        panic!()
    };
    assert_eq!((c.source_start, c.duration), (t(110), t(40)));
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::Out,
            delta: t(5),
        },
    )
    .unwrap();
    assert_eq!(find(&ed, 10).duration(), t(45));
    // can't trim the front past the start of the media
    let err = ed
        .apply(
            Author::User,
            Op::Trim {
                item: id(10),
                edge: Edge::In,
                delta: t(-200),
            },
        )
        .unwrap_err();
    assert!(matches!(err, OpError::OutOfRange(_)));
    // can't lengthen past the end of the media (110 + 45 -> 1000 is the limit)
    let err = ed
        .apply(
            Author::User,
            Op::Trim {
                item: id(10),
                edge: Edge::Out,
                delta: t(900),
            },
        )
        .unwrap_err();
    assert!(matches!(err, OpError::OutOfRange(_)));
    // can't trim to nothing
    let err = ed
        .apply(
            Author::User,
            Op::Trim {
                item: id(10),
                edge: Edge::Out,
                delta: t(-45),
            },
        )
        .unwrap_err();
    assert!(matches!(err, OpError::BadDuration(_)));
}

#[test]
fn trim_in_with_different_media_rate() {
    // 24 fps source on a 30 fps timeline: 5 timeline frames = 4 source frames
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let src = |v| RationalTime::new(v, Rational::FPS_24);
    let media = MediaRef {
        id: id(1),
        name: "m".into(),
        path: "/m".into(),
        duration: Some(src(240)),
        has_audio: true,
        width: 1920,
        height: 1080,
        has_video: true,
        proxy: None,
    };
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(id(TL), "main", R),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: id(TL),
            index: 0,
            track: Track::new(id(V1), "V1", TrackKind::Video),
        },
    )
    .unwrap();
    let c = Item::Clip(Clip::new(id(10), "c", id(1), src(0), t(60)));
    insert(&mut ed, 0, c);
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(5),
        },
    )
    .unwrap();
    let Item::Clip(c) = find(&ed, 10) else {
        panic!()
    };
    assert_eq!(c.source_start, src(4));
    // 1 timeline frame is 0.8 source frames: the source start rounds down (it already showed frame 4 here)
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(1),
        },
    )
    .unwrap();
    let Item::Clip(c) = find(&ed, 10) else {
        panic!()
    };
    assert_eq!((c.source_start, c.duration), (src(4), t(54)));
    // and undo is exact even though rounding happened
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(3),
        },
    )
    .unwrap();
    assert!(ed.undo());
    assert_eq!(ed.project(), &before);
}

#[test]
fn split_between_source_frames_still_undoes_exactly() {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let src = |v| RationalTime::new(v, Rational::FPS_25);
    let media = MediaRef {
        id: id(1),
        name: "m".into(),
        path: "/m".into(),
        duration: Some(src(500)),
        has_audio: true,
        width: 16,
        height: 9,
        has_video: true,
        proxy: None,
    };
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    ed.apply(
        Author::User,
        Op::AddTimeline {
            timeline: Timeline::new(id(TL), "main", R),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddTrack {
            timeline: id(TL),
            index: 0,
            track: Track::new(id(V1), "V1", TrackKind::Video),
        },
    )
    .unwrap();
    let c = Item::Clip(Clip::new(id(10), "c", id(1), src(0), t(60)));
    insert(&mut ed, 0, c);
    let before = ed.snapshot();
    // tick 7 is 5.83 source frames in: the right half starts at source frame 5
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(7),
            new_id: id(11),
        },
    )
    .unwrap();
    let Item::Clip(r) = find(&ed, 11) else {
        panic!()
    };
    assert_eq!((r.source_start, r.duration), (src(5), t(53)));
    ed.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(11),
        },
    )
    .unwrap();
    assert_eq!(ed.project(), &before);
    ed.undo(); // undo the join: the original right half is back exactly
    let Item::Clip(r) = find(&ed, 11) else {
        panic!()
    };
    assert_eq!(r.source_start, src(5));
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn lift_replaces_with_gap_without_moving_anything() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    insert(&mut ed, 1, clip(11, MEDIA, 0, 30));
    ed.apply(
        Author::User,
        Op::ReplaceItem {
            item: id(10),
            with: gap(20, 30),
        },
    )
    .unwrap();
    assert!(matches!(find(&ed, 20), Item::Gap(_)));
    let err = ed
        .apply(
            Author::User,
            Op::ReplaceItem {
                item: id(11),
                with: gap(21, 31),
            },
        )
        .unwrap_err();
    assert!(matches!(err, OpError::OutOfRange(_)));
}

#[test]
fn rejects_bad_input() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    let e = |ed: &mut Editor, op| ed.apply(Author::User, op).unwrap_err();
    assert!(matches!(
        e(
            &mut ed,
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: clip(10, MEDIA, 0, 5)
            }
        ),
        OpError::IdInUse(_)
    ));
    assert!(matches!(
        e(
            &mut ed,
            Op::InsertItem {
                track: id(V1),
                index: 9,
                item: clip(11, MEDIA, 0, 5)
            }
        ),
        OpError::IndexOutOfBounds { .. }
    ));
    assert!(matches!(
        e(
            &mut ed,
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: clip(11, 99, 0, 5)
            }
        ),
        OpError::NotFound("media", _)
    ));
    assert!(matches!(
        e(
            &mut ed,
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: gap(11, 0)
            }
        ),
        OpError::BadDuration(_)
    ));
    let wrong_rate = Item::Gap(Gap {
        id: id(11),
        duration: RationalTime::new(5, Rational::FPS_24),
    });
    assert!(matches!(
        e(
            &mut ed,
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: wrong_rate
            }
        ),
        OpError::BadDuration(_)
    ));
    assert!(matches!(
        e(&mut ed, Op::RemoveMedia { id: id(MEDIA) }),
        OpError::MediaInUse(_)
    ));
    assert!(matches!(
        e(
            &mut ed,
            Op::Split {
                item: id(10),
                at: t(30),
                new_id: id(11)
            }
        ),
        OpError::OutOfRange(_)
    ));
    // failed ops leave no trace
    assert_eq!(ed.history().len(), 5);
}

#[test]
fn transaction_is_atomic_and_one_undo_step() {
    let mut ed = editor();
    let before = ed.snapshot();
    let ok = Op::Transaction {
        description: "Add two clips".into(),
        ops: vec![
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: clip(10, MEDIA, 0, 30),
            },
            Op::InsertItem {
                track: id(V1),
                index: 1,
                item: clip(11, MEDIA, 30, 30),
            },
        ],
    };
    ed.apply(Author::Agent("codit".into()), ok).unwrap();
    assert_eq!(durations(&ed), [30, 30]);
    let after = ed.snapshot();
    // second op fails (duplicate id) -> whole transaction rejected, nothing applied
    let bad = Op::Transaction {
        description: "bad".into(),
        ops: vec![
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: clip(12, MEDIA, 0, 30),
            },
            Op::InsertItem {
                track: id(V1),
                index: 0,
                item: clip(10, MEDIA, 0, 30),
            },
        ],
    };
    assert!(ed.apply(Author::User, bad).is_err());
    assert_eq!(ed.project(), &after);
    assert!(ed.undo());
    assert_eq!(ed.project(), &before);
    assert!(ed.redo());
    assert_eq!(ed.project(), &after);
}

#[test]
fn new_edit_clears_redo() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    ed.undo();
    assert!(ed.can_redo());
    insert(&mut ed, 0, clip(11, MEDIA, 0, 30));
    assert!(!ed.can_redo());
}

#[test]
fn ops_and_projects_survive_json() {
    let mut ed = editor();
    insert(&mut ed, 0, clip(10, MEDIA, 0, 30));
    let op = Op::Split {
        item: id(10),
        at: t(10),
        new_id: id(11),
    };
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.contains(r#""op":"Split""#));
    assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
    let log = serde_json::to_string(ed.history()).unwrap();
    let back: Vec<HistoryEntry> = serde_json::from_str(&log).unwrap();
    assert_eq!(back, ed.history());
    // replaying a saved log on an empty project rebuilds the same state
    let mut replay = Editor::new(Project::new(ProjectSettings::default()));
    for e in back {
        replay.apply(e.author, e.op).unwrap();
    }
    assert_eq!(replay.project(), ed.project());
}

/// Tiny deterministic PRNG so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo + 1) as u64) as i64
    }
}

fn random_op(rng: &mut Rng, ed: &Editor, next_id: &mut u128) -> Op {
    let mut fresh = || {
        *next_id += 1;
        id(*next_id)
    };
    let tl = ed.project().timeline(id(TL)).unwrap();
    let track = &tl.tracks[0];
    let n = track.items.len();
    let pick = |rng: &mut Rng| track.items.get(rng.below(n.max(1))).map(|i| i.id());
    match rng.below(9) {
        0 | 1 => {
            let new = fresh();
            let item = if rng.below(4) == 0 {
                Item::Gap(Gap {
                    id: new,
                    duration: t(rng.range(1, 40)),
                })
            } else {
                // half the clips come from a 24 fps source so rate conversion is fuzzed too
                let (media, rate) = if rng.below(2) == 0 {
                    (MEDIA, R)
                } else {
                    (MEDIA24, Rational::FPS_24)
                };
                Item::Clip(Clip::new(
                    new,
                    "r",
                    id(media),
                    RationalTime::new(rng.range(0, 600), rate),
                    t(rng.range(1, 150)),
                ))
            };
            Op::InsertItem {
                track: id(V1),
                index: rng.below(n + 2),
                item,
            }
        }
        2 => pick(rng).map_or(
            Op::SetName {
                item: fresh(),
                name: "x".into(),
            },
            |item| Op::RemoveItem { item },
        ),
        3 => Op::Split {
            item: pick(rng).unwrap_or(id(0)),
            at: t(rng.range(1, 160)),
            new_id: fresh(),
        },
        4 => Op::Trim {
            item: pick(rng).unwrap_or(id(0)),
            edge: if rng.below(2) == 0 {
                Edge::In
            } else {
                Edge::Out
            },
            delta: t(rng.range(-60, 60)),
        },
        5 => {
            let i = rng.below(n.max(1));
            match (track.items.get(i), track.items.get(i + 1)) {
                (Some(a), Some(b)) => Op::Join {
                    left: a.id(),
                    right: b.id(),
                },
                _ => Op::SetName {
                    item: fresh(),
                    name: "x".into(),
                },
            }
        }
        6 => match pick(rng).and_then(|i| {
            ed.project()
                .find_item(i)
                .and_then(|l| ed.project().item(l).cloned())
        }) {
            Some(it) => Op::ReplaceItem {
                item: it.id(),
                with: Item::Gap(Gap {
                    id: fresh(),
                    duration: it.duration(),
                }),
            },
            None => Op::SetName {
                item: fresh(),
                name: "x".into(),
            },
        },
        7 => Op::SetEnabled {
            item: pick(rng).unwrap_or(id(0)),
            enabled: rng.below(2) == 0,
        },
        _ => {
            let a = fresh();
            let b = fresh();
            Op::Transaction {
                description: "pair".into(),
                ops: vec![
                    Op::InsertItem {
                        track: id(V1),
                        index: 0,
                        item: Item::Gap(Gap {
                            id: a,
                            duration: t(5),
                        }),
                    },
                    Op::Split {
                        item: a,
                        at: t(2),
                        new_id: b,
                    },
                ],
            }
        }
    }
}

#[test]
fn fuzz_invariants_undo_and_redo() {
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ed = editor();
        let mut next_id = 1000u128;
        let mut states = vec![ed.snapshot()];
        for _ in 0..200 {
            let op = random_op(&mut rng, &ed, &mut next_id);
            let before = ed.snapshot();
            match ed.apply(Author::User, op.clone()) {
                Ok(()) => {
                    ed.project()
                        .validate()
                        .unwrap_or_else(|e| panic!("seed {seed}: {op:?} -> {e:?}"));
                    states.push(ed.snapshot());
                }
                Err(_) => assert_eq!(
                    ed.project(),
                    &before,
                    "seed {seed}: failed op changed state: {op:?}"
                ),
            }
        }
        assert!(
            states.len() > 40,
            "seed {seed}: too few ops succeeded ({})",
            states.len()
        );
        // undo step by step, each time landing on the exact earlier state
        for expected in states.iter().rev().skip(1) {
            assert!(ed.undo());
            assert_eq!(ed.project(), expected, "seed {seed}: undo mismatch");
        }
        assert_eq!(ed.project(), &states[0]);
        for expected in states.iter().skip(1) {
            assert!(ed.redo());
            assert_eq!(ed.project(), expected, "seed {seed}: redo mismatch");
        }
    }
}
