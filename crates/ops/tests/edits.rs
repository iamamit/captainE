//! Tests for property ops and the composed editing gestures.

use captaine_model::*;
use captaine_ops::edit;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

const R: Rational = Rational::FPS_30;
fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}
fn id(n: u128) -> Id {
    Id::from_u128(n)
}

const V1: u128 = 3;
const V2: u128 = 4;

fn clip(n: u128, src: i64, dur: i64) -> Clip {
    Clip::new(id(n), format!("c{n}"), id(1), t(src), t(dur))
}

fn editor() -> Editor {
    let mut ed = Editor::new(Project::new(ProjectSettings::default()));
    let media = MediaRef {
        id: id(1),
        name: "m".into(),
        path: "/m".into(),
        duration: Some(t(1000)),
        has_audio: true,
        width: 1920,
        height: 1080,
        has_video: true,
        proxy: None,
    };
    let mut tl = Timeline::new(id(2), "main", R);
    tl.tracks.push(std::sync::Arc::new(Track::new(
        id(V1),
        "V1",
        TrackKind::Video,
    )));
    tl.tracks.push(std::sync::Arc::new(Track::new(
        id(V2),
        "V2",
        TrackKind::Video,
    )));
    ed.apply(Author::User, Op::AddMedia { media }).unwrap();
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed
}

fn put(ed: &mut Editor, track: u128, c: Clip) {
    let op = edit::append(ed.project(), id(track), c);
    ed.apply(Author::User, op).unwrap();
}

/// (start, duration, is_gap) per item
fn spans(ed: &Editor, track: u128) -> Vec<(i64, i64, bool)> {
    let (tl, ti) = ed.project().find_track(id(track)).unwrap();
    let tr = &ed.project().timeline(tl).unwrap().tracks[ti];
    tr.items
        .iter()
        .zip(tr.item_starts())
        .map(|(it, st)| (st, it.duration().value, it.as_clip().is_none()))
        .collect()
}

fn clip_at(ed: &Editor, track: u128, start: i64) -> Clip {
    let (tl, ti) = ed.project().find_track(id(track)).unwrap();
    let tr = &ed.project().timeline(tl).unwrap().tracks[ti];
    tr.items
        .iter()
        .zip(tr.item_starts())
        .find(|(_, s)| *s == start)
        .and_then(|(i, _)| i.as_clip().cloned())
        .unwrap_or_else(|| panic!("no clip at {start}"))
}

fn run(ed: &mut Editor, plan: impl FnOnce(&Project) -> Result<Op, OpError>) {
    let op = plan(ed.project()).unwrap();
    ed.apply(Author::User, op).unwrap();
    ed.project().validate().unwrap();
}

#[test]
fn properties_round_trip_and_validate() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 0, 60));
    let before = ed.snapshot();
    let video = VideoProps {
        transform: Transform {
            position: (10.0, -5.0),
            scale: (0.5, 0.5),
            rotation_deg: 15.0,
        },
        opacity: 0.5,
        blend: BlendMode::Screen,
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
    let audio = AudioProps {
        muted: false,
        gain_db: -6.0,
        pan: 0.25,
        fade_in: 10,
        fade_out: 20,
    };
    ed.apply(
        Author::User,
        Op::SetAudio {
            item: id(10),
            audio,
        },
    )
    .unwrap();
    assert_eq!(clip_at(&ed, V1, 0).video, video);
    assert!(ed
        .apply(
            Author::User,
            Op::SetVideo {
                item: id(10),
                video: VideoProps {
                    opacity: 1.5,
                    ..video
                }
            }
        )
        .is_err());
    assert!(ed
        .apply(
            Author::User,
            Op::SetAudio {
                item: id(10),
                audio: AudioProps {
                    fade_in: 50,
                    fade_out: 50,
                    ..audio
                }
            }
        )
        .is_err());
    assert!(ed
        .apply(
            Author::User,
            Op::SetAudio {
                item: id(10),
                audio: AudioProps {
                    gain_db: f64::NAN,
                    ..audio
                }
            }
        )
        .is_err());
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn old_files_without_new_fields_still_load() {
    let json = r#"{"id":"00000000-0000-0000-0000-00000000000a","name":"c","media":"00000000-0000-0000-0000-000000000001",
        "source_start":{"value":0,"rate":{"num":30,"den":1}},"duration":{"value":5,"rate":{"num":30,"den":1}},"enabled":true}"#;
    let c: Clip = serde_json::from_str(json).unwrap();
    assert_eq!(c.video, VideoProps::default());
    assert_eq!(c.audio, AudioProps::default());
}

#[test]
fn slip_moves_the_source_window_only() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 100, 60));
    run(&mut ed, |_| {
        Ok(Op::Slip {
            item: id(10),
            delta: t(25),
        })
    });
    let c = clip_at(&ed, V1, 0);
    assert_eq!((c.source_start, c.duration), (t(125), t(60)));
    assert!(ed
        .apply(
            Author::User,
            Op::Slip {
                item: id(10),
                delta: t(-500)
            }
        )
        .is_err());
    ed.undo();
    assert_eq!(clip_at(&ed, V1, 0).source_start, t(100));
}

#[test]
fn track_settings_and_reorder() {
    let mut ed = editor();
    let before = ed.snapshot();
    ed.apply(
        Author::User,
        Op::SetTrack {
            track: id(V1),
            enabled: false,
            gain_db: -3.0,
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::MoveTrack {
            track: id(V1),
            index: 1,
        },
    )
    .unwrap();
    let tl = ed.project().timeline(id(2)).unwrap();
    assert_eq!(tl.tracks[1].id, id(V1));
    assert!(!tl.tracks[1].enabled);
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn overwrite_splits_and_replaces() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 0, 90));
    // new 30-frame clip over frames 30..60 of the existing one
    run(&mut ed, |p| {
        edit::overwrite(p, id(V1), t(30), clip(20, 500, 30))
    });
    assert_eq!(
        spans(&ed, V1),
        [(0, 30, false), (30, 30, false), (60, 30, false)]
    );
    let mid = clip_at(&ed, V1, 30);
    assert_eq!(mid.source_start, t(500));
    // the right piece keeps its original source position
    assert_eq!(clip_at(&ed, V1, 60).source_start, t(60));
    ed.undo();
    assert_eq!(spans(&ed, V1), [(0, 90, false)]);
}

#[test]
fn overwrite_past_end_pads_with_gap_and_can_extend() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 0, 30));
    run(&mut ed, |p| {
        edit::overwrite(p, id(V1), t(50), clip(20, 0, 20))
    });
    assert_eq!(
        spans(&ed, V1),
        [(0, 30, false), (30, 20, true), (50, 20, false)]
    );
    // overlapping the tail extends the track
    run(&mut ed, |p| {
        edit::overwrite(p, id(V1), t(60), clip(21, 0, 30))
    });
    assert_eq!(
        spans(&ed, V1),
        [
            (0, 30, false),
            (30, 20, true),
            (50, 10, false),
            (60, 30, false)
        ]
    );
}

#[test]
fn lift_and_clean_gaps() {
    let mut ed = editor();
    for (n, s) in [(10, 0), (11, 100), (12, 200)] {
        put(&mut ed, V1, clip(n, s, 30));
    }
    run(&mut ed, |p| edit::lift(p, id(11)));
    assert_eq!(
        spans(&ed, V1),
        [(0, 30, false), (30, 30, true), (60, 30, false)]
    );
    run(&mut ed, |p| edit::lift(p, id(12)));
    run(&mut ed, |p| edit::clean_gaps(p, id(V1)).map(|o| o.unwrap()));
    assert_eq!(spans(&ed, V1), [(0, 30, false)]);
    assert!(edit::clean_gaps(ed.project(), id(V1)).unwrap().is_none());
}

#[test]
fn move_between_tracks_leaves_no_junk() {
    let mut ed = editor();
    for (n, s) in [(10, 0), (11, 100), (12, 200)] {
        put(&mut ed, V1, clip(n, s, 30));
    }
    let before = ed.snapshot();
    run(&mut ed, |p| edit::move_item(p, id(12), id(V2), t(15)));
    assert_eq!(spans(&ed, V1), [(0, 30, false), (30, 30, false)]);
    assert_eq!(spans(&ed, V2), [(0, 15, true), (15, 30, false)]);
    // moving within a track reorders: put clip 10 after the others
    run(&mut ed, |p| edit::move_item(p, id(10), id(V1), t(60)));
    assert_eq!(
        spans(&ed, V1),
        [(0, 30, true), (30, 30, false), (60, 30, false)]
    );
    ed.undo();
    ed.undo();
    assert_eq!(ed.project(), &before);
}

#[test]
fn roll_and_slide_keep_total_length() {
    let mut ed = editor();
    for (n, s) in [(10, 0), (11, 100), (12, 200)] {
        put(&mut ed, V1, clip(n, s, 60));
    }
    run(&mut ed, |p| edit::roll(p, id(10), id(11), t(10)));
    assert_eq!(
        spans(&ed, V1),
        [(0, 70, false), (70, 50, false), (120, 60, false)]
    );
    assert_eq!(clip_at(&ed, V1, 70).source_start, t(110));
    run(&mut ed, |p| edit::slide(p, id(11), t(-20)));
    assert_eq!(
        spans(&ed, V1),
        [(0, 50, false), (50, 50, false), (100, 80, false)]
    );
    assert_eq!(ed.project().timeline(id(2)).unwrap().duration(), t(180));
    assert!(edit::slide(ed.project(), id(10), t(5)).is_err()); // first item has no left neighbour
}

#[test]
fn split_at_time() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 0, 60));
    run(&mut ed, |p| edit::split_at(p, id(V1), t(20)));
    assert_eq!(spans(&ed, V1), [(0, 20, false), (20, 40, false)]);
    assert!(edit::split_at(ed.project(), id(V1), t(20)).is_err()); // already a cut
    assert!(edit::split_at(ed.project(), id(V1), t(500)).is_err());
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
fn fuzz_gestures_keep_invariants_and_undo() {
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut ed = editor();
        let mut next = 1000u128;
        let mut states = vec![ed.snapshot()];
        for _ in 0..150 {
            let p = ed.project();
            let tracks = [id(V1), id(V2)];
            let track = tracks[rng.below(2)];
            let (tl, ti) = p.find_track(track).unwrap();
            let items = p.timeline(tl).unwrap().tracks[ti].items.clone();
            let pick = |rng: &mut Rng| {
                items
                    .get(rng.below(items.len().max(1)))
                    .map(|i| i.id())
                    .unwrap_or(id(0))
            };
            next += 1;
            let at = t(rng.below(200) as i64);
            let dur = t(1 + rng.below(80) as i64);
            let op = match rng.below(8) {
                0 | 1 => {
                    // alternate between 30 fps and 24 fps sources on the 30 fps timeline
                    let (m, rate) = if rng.below(2) == 0 {
                        (1, Rational::FPS_30)
                    } else {
                        (7, Rational::FPS_24)
                    };
                    let src = RationalTime::new(rng.below(600) as i64, rate);
                    edit::overwrite(p, track, at, Clip::new(id(next), "n", id(m), src, dur))
                }
                2 => edit::lift(p, pick(&mut rng)),
                3 => edit::move_item(p, pick(&mut rng), tracks[rng.below(2)], at),
                4 => edit::roll(
                    p,
                    pick(&mut rng),
                    pick(&mut rng),
                    t(rng.below(41) as i64 - 20),
                ),
                5 => edit::slide(p, pick(&mut rng), t(rng.below(41) as i64 - 20)),
                6 => edit::split_at(p, track, at),
                _ => edit::clean_gaps(p, track).map(|o| {
                    o.unwrap_or(Op::SetTrack {
                        track,
                        enabled: true,
                        gain_db: 0.0,
                    })
                }),
            };
            let Ok(op) = op else { continue };
            let before = ed.snapshot();
            match ed.apply(Author::User, op.clone()) {
                Ok(()) => {
                    ed.project()
                        .validate()
                        .unwrap_or_else(|e| panic!("seed {seed}: {op:?} -> {e:?}"));
                    states.push(ed.snapshot());
                }
                Err(_) => assert_eq!(ed.project(), &before),
            }
        }
        assert!(
            states.len() > 40,
            "seed {seed}: only {} ops worked",
            states.len()
        );
        for expected in states.iter().rev().skip(1) {
            assert!(ed.undo());
            assert_eq!(ed.project(), expected, "seed {seed}");
        }
    }
}

#[test]
fn range_extract_and_lift_keep_tracks_in_sync() {
    let mut ed = editor();
    put(&mut ed, V1, clip(10, 0, 60));
    put(&mut ed, V1, clip(11, 100, 60));
    put(&mut ed, V2, clip(12, 200, 90));
    let before = ed.snapshot();
    // cut 30..90 out of everything: it spans the end of clip 10, the start of clip 11, and the tail of 12
    run(&mut ed, |p| edit::extract_range(p, id(2), t(30), t(90)));
    assert_eq!(ed.project().timeline(id(2)).unwrap().duration(), t(60));
    assert_eq!(spans(&ed, V1), [(0, 30, false), (30, 30, false)]);
    assert_eq!(spans(&ed, V2), [(0, 30, false)]);
    // what is left of clip 11 is its tail
    assert_eq!(clip_at(&ed, V1, 30).source_start, t(130));
    ed.undo();
    assert_eq!(ed.project(), &before);

    run(&mut ed, |p| edit::lift_range(p, id(2), t(30), t(90)));
    assert_eq!(
        spans(&ed, V1),
        [(0, 30, false), (30, 60, true), (90, 30, false)]
    );
    assert_eq!(spans(&ed, V2), [(0, 30, false)]); // the trailing gap is dropped
    ed.undo();
    assert_eq!(ed.project(), &before);

    assert!(edit::extract_range(ed.project(), id(2), t(90), t(30)).is_err());
    assert!(edit::extract_range(ed.project(), id(2), t(500), t(600)).is_err()); // nothing there
    assert!(edit::lift_range(ed.project(), id(99), t(0), t(10)).is_err());
}
