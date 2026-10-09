#![allow(dead_code)]

use std::sync::Arc;

use captaine_model::*;
use captaine_ops::*;
use captaine_time::{Rational, RationalTime};

pub const R: Rational = Rational::FPS_24;

pub fn t(v: i64) -> RationalTime {
    RationalTime::new(v, R)
}

/// Distinct, well-spread ids so short forms never collide.
pub fn id(n: u128) -> Id {
    Id::from_u128(n.wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835) | (1u128 << 100))
}

pub const MEDIA: u128 = 1;
pub const MEDIA2: u128 = 2;
pub const TL: u128 = 10;
pub const V1: u128 = 11;
pub const V2: u128 = 12;
pub const A1: u128 = 13;
pub const C1: u128 = 21;
pub const C2: u128 = 22;
pub const C3: u128 = 23;

pub fn media(n: u128, secs: i64) -> MediaRef {
    MediaRef {
        id: id(n),
        name: format!("m{n}"),
        path: format!("/m{n}.mp4"),
        duration: Some(t(24 * secs)),
        has_audio: true,
        width: 640,
        height: 360,
        has_video: true,
        proxy: None,
    }
}

pub fn clip(n: u128, src: i64, dur: i64) -> Item {
    Item::Clip(Clip::new(id(n), format!("c{n}"), id(MEDIA), t(src), t(dur)))
}

/// V1: c1 (0..4 s), c2 (4..8 s), c3 (8..12 s). V2 and A1 empty.
pub fn base_project() -> Project {
    let mut ed = Editor::new(Project::new(ProjectSettings {
        frame_rate: R,
        width: 640,
        height: 360,
        sample_rate: 48_000,
    }));
    let mut v1 = Track::new(id(V1), "V1", TrackKind::Video);
    v1.items = vec![clip(C1, 0, 96), clip(C2, 100, 96), clip(C3, 200, 96)];
    let mut tl = Timeline::new(id(TL), "Timeline 1", R);
    tl.tracks.push(Arc::new(v1));
    tl.tracks
        .push(Arc::new(Track::new(id(V2), "V2", TrackKind::Video)));
    tl.tracks
        .push(Arc::new(Track::new(id(A1), "A1", TrackKind::Audio)));
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: media(MEDIA, 600),
        },
    )
    .unwrap();
    ed.apply(
        Author::User,
        Op::AddMedia {
            media: media(MEDIA2, 600),
        },
    )
    .unwrap();
    ed.apply(Author::User, Op::AddTimeline { timeline: tl })
        .unwrap();
    ed.project().clone()
}

/// Apply ops to a project.
pub fn run(p: &Project, ops: &[Op]) -> Project {
    ops.iter().fold(p.clone(), |p, op| {
        apply(&p, op).unwrap_or_else(|e| panic!("{op:?}: {e}")).0
    })
}

pub fn track(p: &Project, name: &str) -> Track {
    p.timelines
        .values()
        .next()
        .unwrap()
        .tracks
        .iter()
        .find(|t| t.name == name)
        .unwrap()
        .as_ref()
        .clone()
}

pub fn items(p: &Project, name: &str) -> Vec<Item> {
    track(p, name).items
}

/// (start, duration) of every item on a track
pub fn spans(p: &Project, name: &str) -> Vec<(i64, i64)> {
    let t = track(p, name);
    t.items
        .iter()
        .zip(t.item_starts())
        .map(|(i, s)| (s, i.duration().value))
        .collect()
}

pub fn clip_of(p: &Project, n: u128) -> Clip {
    p.timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .flat_map(|t| t.items.iter())
        .find_map(|i| i.as_clip().filter(|c| c.id == id(n)).cloned())
        .unwrap_or_else(|| panic!("no clip {n}"))
}

pub fn has(p: &Project, item: Id) -> bool {
    p.timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .any(|t| t.items.iter().any(|i| i.id() == item))
}

pub fn video(opacity: f64) -> VideoProps {
    VideoProps {
        opacity,
        ..VideoProps::default()
    }
}

/// Tiny deterministic generator.
pub struct Rng(pub u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// One random, valid edit made through the same gestures a person uses. `None` if the draw did not apply.
pub fn random_edit(p: &Project, rng: &mut Rng, next: &mut u128) -> Option<Op> {
    let tl = p.timelines.values().next()?;
    let tracks: Vec<Id> = tl.tracks.iter().map(|t| t.id).collect();
    let video_tracks: Vec<Id> = tl
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Video)
        .map(|t| t.id)
        .collect();
    let all: Vec<Item> = tl
        .tracks
        .iter()
        .flat_map(|t| t.items.iter().cloned())
        .collect();
    let pick = |rng: &mut Rng| all.get(rng.below(all.len())).map(|i| i.id());
    let content: Vec<Id> = all
        .iter()
        .filter(|i| i.is_content())
        .map(|i| i.id())
        .collect();
    let pick_content = |rng: &mut Rng| content.get(rng.below(content.len())).copied();
    *next += 1;
    let fresh = id(1000 + *next);
    let at = t(rng.below(300) as i64);
    let dur = t(1 + rng.below(80) as i64);
    let res = match rng.below(11) {
        0 => {
            let track = video_tracks[rng.below(video_tracks.len())];
            let clip = Clip::new(
                fresh,
                format!("n{next}"),
                id(if rng.below(2) == 0 { MEDIA } else { MEDIA2 }),
                t(rng.below(400) as i64),
                dur,
            );
            edit::overwrite(p, track, at, clip)
        }
        1 => {
            let track = video_tracks[rng.below(video_tracks.len())];
            let mut title = Title::new(fresh, TitleTemplate::LowerThird, dur);
            title.text = format!("T{next}");
            edit::overwrite_item(p, track, at, Item::Title(title))
        }
        2 => pick_content(rng)
            .map(|i| edit::lift(p, i))
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        3 => pick_content(rng)
            .map(|i| Ok(Op::RemoveItem { item: i }))
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        4 => pick_content(rng)
            .map(|i| edit::move_item(p, i, tracks[rng.below(tracks.len())], at))
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        5 => edit::split_at(p, tracks[rng.below(tracks.len())], at),
        6 => pick_content(rng)
            .map(|i| {
                Ok(Op::Trim {
                    item: i,
                    edge: if rng.below(2) == 0 {
                        Edge::In
                    } else {
                        Edge::Out
                    },
                    delta: t(rng.below(41) as i64 - 20),
                })
            })
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        7 => pick_content(rng)
            .map(|i| {
                let v = VideoProps {
                    opacity: (rng.below(11) as f64) / 10.0,
                    transform: Transform {
                        position: (rng.below(100) as f64, 0.0),
                        ..Transform::default()
                    },
                    ..VideoProps::default()
                };
                Ok(Op::SetVideo { item: i, video: v })
            })
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        8 => {
            let track = tracks[rng.below(tracks.len())];
            Ok(Op::SetTrack {
                track,
                enabled: rng.below(2) == 0,
                gain_db: -(rng.below(7) as f64),
            })
        }
        9 => pick(rng)
            .map(|a| edit::roll(p, a, pick(rng).unwrap_or(a), t(rng.below(21) as i64 - 10)))
            .unwrap_or(Err(OpError::NotFound("item", fresh))),
        _ => edit::clean_gaps(p, tracks[rng.below(tracks.len())]).map(|o| {
            o.unwrap_or(Op::SetTrack {
                track: tracks[0],
                enabled: true,
                gain_db: 0.0,
            })
        }),
    };
    let op = res.ok()?;
    apply(p, &op).ok()?;
    Some(op)
}

/// Make `n` successful random edits.
pub fn edit_randomly(p: &Project, seed: u64, n: usize) -> (Project, Vec<Op>) {
    let mut rng = Rng::new(seed);
    let mut next = seed as u128 * 10_000;
    let (mut cur, mut ops) = (p.clone(), Vec::new());
    for _ in 0..n * 6 {
        if ops.len() >= n {
            break;
        }
        if let Some(op) = random_edit(&cur, &mut rng, &mut next) {
            cur = apply(&cur, &op).unwrap().0;
            ops.push(op);
        }
    }
    (cur, ops)
}
