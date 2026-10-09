//! Speed, reverse and freeze through the command layer.

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

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// V1 holds one clip, 10 (96 frames from source frame 100), over media of 1000 frames.
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
    v1.items = vec![Item::Clip(Clip::new(id(10), "c", id(1), t(100), t(96)))];
    let mut tl = Timeline::new(id(2), "main", R);
    tl.tracks.push(Arc::new(v1));
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed
}

fn clips(ed: &Editor) -> Vec<Clip> {
    ed.project().timelines.values().next().unwrap().tracks[0]
        .items
        .iter()
        .filter_map(|i| i.as_clip().cloned())
        .collect()
}

fn retime(item: u128, speed: f64, reverse: bool, freeze: bool) -> Op {
    Op::SetRetime {
        item: id(item),
        retime: Retime {
            speed,
            reverse,
            freeze,
        },
    }
}

#[test]
fn changing_speed_resizes_the_clip_over_the_same_source() {
    let mut ed = editor();
    let before = ed.snapshot();
    ed.apply(Author::User, retime(10, 2.0, false, false))
        .unwrap();
    let c = &clips(&ed)[0];
    assert_eq!((c.duration.value, c.source_start.value), (48, 100));
    assert_eq!(c.source_span(), 96);
    assert_eq!(c.source_frame_at(10), 120);
    ed.apply(Author::User, retime(10, 0.25, false, false))
        .unwrap();
    assert_eq!(clips(&ed)[0].duration.value, 384);
    ed.apply(Author::User, retime(10, 1.0, true, false))
        .unwrap();
    let c = &clips(&ed)[0];
    assert_eq!(c.duration.value, 96);
    assert_eq!((c.source_frame_at(0), c.source_frame_at(95)), (195, 100));
    // a freeze keeps the length and shows the first frame of the window
    ed.apply(Author::User, retime(10, 1.0, false, true))
        .unwrap();
    let c = &clips(&ed)[0];
    assert_eq!((c.duration.value, c.source_frame_at(50)), (96, 100));
    for _ in 0..4 {
        assert!(ed.undo());
    }
    assert_eq!(ed.project(), &before);
    // out-of-range speeds are refused
    assert!(ed
        .apply(Author::User, retime(10, 0.0, false, false))
        .is_err());
    assert!(ed
        .apply(Author::User, retime(10, 50.0, false, false))
        .is_err());
    assert!(ed
        .apply(Author::User, retime(10, f64::NAN, false, false))
        .is_err());
    assert_eq!(ed.project(), &before);
}

#[test]
fn a_reversed_clip_splits_and_joins_back() {
    let mut ed = editor();
    ed.apply(Author::User, retime(10, 1.0, true, false))
        .unwrap();
    let whole: Vec<i64> = (0..96).map(|k| clips(&ed)[0].source_frame_at(k)).collect();
    ed.apply(
        Author::User,
        Op::Split {
            item: id(10),
            at: t(30),
            new_id: id(11),
        },
    )
    .unwrap();
    let cs = clips(&ed);
    let halves: Vec<i64> = (0..30)
        .map(|k| cs[0].source_frame_at(k))
        .chain((0..66).map(|k| cs[1].source_frame_at(k)))
        .collect();
    assert_eq!(halves, whole, "a cut changes nothing on screen");
    ed.apply(
        Author::User,
        Op::Join {
            left: id(10),
            right: id(11),
        },
    )
    .unwrap();
    assert_eq!(clips(&ed).len(), 1);
    assert_eq!(clips(&ed)[0].source_start.value, 100);
    // trimming the front of a reversed clip removes frames from the top of its window
    ed.apply(
        Author::User,
        Op::Trim {
            item: id(10),
            edge: Edge::In,
            delta: t(10),
        },
    )
    .unwrap();
    let c = &clips(&ed)[0];
    assert_eq!((c.source_start.value, c.source_frame_at(0)), (100, 185));
}

#[test]
fn fuzz_retime_with_cuts_trims_and_undo() {
    for seed in 1..=80u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ed = editor();
        let mut states = vec![ed.snapshot()];
        let mut next = 1000u128;
        for _ in 0..80 {
            let items: Vec<Id> = clips(&ed).iter().map(|c| c.id).collect();
            let pick = items[rng.below(items.len())];
            next += 1;
            let op = match rng.below(7) {
                6 => Op::SetTransition {
                    item: pick,
                    transition: (rng.below(3) > 0).then(|| Transition {
                        kind: TransitionKind::ALL[rng.below(6)],
                        duration: 2 + rng.below(80) as i64,
                    }),
                },
                0 | 1 => Op::SetRetime {
                    item: pick,
                    retime: Retime {
                        speed: [0.25, 0.5, 1.0, 1.5, 2.0, 4.0][rng.below(6)],
                        reverse: rng.below(2) == 0,
                        freeze: rng.below(6) == 0,
                    },
                },
                2 => Op::Split {
                    item: pick,
                    at: t(1 + rng.below(60) as i64),
                    new_id: id(next),
                },
                3 => Op::Trim {
                    item: pick,
                    edge: if rng.below(2) == 0 {
                        Edge::In
                    } else {
                        Edge::Out
                    },
                    delta: t(rng.below(41) as i64 - 20),
                },
                4 => Op::Slip {
                    item: pick,
                    delta: t(rng.below(41) as i64 - 20),
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
