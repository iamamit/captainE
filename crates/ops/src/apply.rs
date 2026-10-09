use std::fmt;
use std::sync::Arc;

use captaine_model::{AudioProps, Clip, Gap, Id, Item, ItemLoc, Project};
use captaine_time::{Rational, RationalTime, Rounding};

use crate::{Edge, Op};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    NotFound(&'static str, Id),
    IdInUse(Id),
    MediaInUse(Id),
    IndexOutOfBounds {
        index: usize,
        len: usize,
    },
    /// Duration not at the timeline's rate, or not positive
    BadDuration(Id),
    /// A time that cannot be expressed exactly in the needed rate
    Inexact(&'static str),
    OutOfRange(&'static str),
    NotJoinable(&'static str),
    BadProps(&'static str),
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpError::NotFound(what, id) => write!(f, "{what} {id} not found"),
            OpError::IdInUse(id) => write!(f, "id {id} is already in use"),
            OpError::MediaInUse(id) => write!(f, "media {id} is used by a clip"),
            OpError::IndexOutOfBounds { index, len } => {
                write!(f, "index {index} out of bounds (len {len})")
            }
            OpError::BadDuration(id) => write!(
                f,
                "item {id} needs a positive duration at the timeline rate"
            ),
            OpError::Inexact(w) => write!(f, "not exactly representable: {w}"),
            OpError::OutOfRange(w) => write!(f, "out of range: {w}"),
            OpError::NotJoinable(w) => write!(f, "cannot join: {w}"),
            OpError::BadProps(w) => write!(f, "bad property: {w}"),
        }
    }
}
impl std::error::Error for OpError {}

type Result<T> = std::result::Result<T, OpError>;

/// Applies `op`, returning the new project and the op that undoes it. `project` is untouched.
pub fn apply(project: &Project, op: &Op) -> Result<(Project, Op)> {
    let mut p = project.clone();
    let inverse = apply_in_place(&mut p, op)?;
    debug_assert_eq!(p.validate(), Ok(()), "op {op:?} broke an invariant");
    Ok((p, inverse))
}

fn item_loc(p: &Project, id: Id) -> Result<ItemLoc> {
    p.find_item(id).ok_or(OpError::NotFound("item", id))
}

fn timeline_rate(p: &Project, id: Id) -> Rational {
    p.timeline(id).expect("located timeline exists").rate
}

fn ensure_free(p: &Project, id: Id) -> Result<()> {
    if p.id_in_use(id) {
        Err(OpError::IdInUse(id))
    } else {
        Ok(())
    }
}

/// An item that may be placed on a timeline running at `rate`.
fn check_item(p: &Project, rate: Rational, item: &Item) -> Result<()> {
    let d = item.duration();
    if d.rate != rate || d.value <= 0 {
        return Err(OpError::BadDuration(item.id()));
    }
    if let Item::Clip(c) = item {
        c.check_props(rate).map_err(OpError::BadProps)?;
        let m = p
            .media
            .get(&c.media)
            .ok_or(OpError::NotFound("media", c.media))?;
        if c.source_start.value < 0 {
            return Err(OpError::OutOfRange("clip starts before its media"));
        }
        if let Some(len) = m.duration {
            let src = c
                .duration
                .rescale(c.source_start.rate, Rounding::Ceil)
                .ok_or(OpError::Inexact("clip length"))?;
            let end = RationalTime::new(c.source_start.value + src.value, c.source_start.rate);
            if end > len {
                return Err(OpError::OutOfRange("clip runs past the end of its media"));
            }
        }
    }
    Ok(())
}

/// Timeline time as a source offset at `rate`, rounded down (the renderer's rule: the source frame shown
/// `k` ticks into a clip is `floor(k * source_rate / timeline_rate)`), so cuts never change what is on screen.
fn source_offset(delta: RationalTime, rate: Rational) -> Result<RationalTime> {
    delta
        .rescale(rate, Rounding::Floor)
        .ok_or(OpError::Inexact("time in source"))
}

/// Shrink fade lengths so they fit a clip of `duration` ticks (fade-out has priority).
fn fit_fades(fade_in: i64, fade_out: i64, duration: i64) -> (i64, i64) {
    let fo = fade_out.min(duration);
    (fade_in.min(duration - fo), fo)
}

fn arith<T>(v: Option<T>) -> Result<T> {
    v.ok_or(OpError::Inexact("time arithmetic"))
}

fn apply_in_place(p: &mut Project, op: &Op) -> Result<Op> {
    match op {
        Op::AddMedia { media } => {
            ensure_free(p, media.id)?;
            p.media.insert(media.id, media.clone());
            Ok(Op::RemoveMedia { id: media.id })
        }
        Op::RemoveMedia { id } => {
            if p.media_in_use(*id) {
                return Err(OpError::MediaInUse(*id));
            }
            let m = p.media.remove(id).ok_or(OpError::NotFound("media", *id))?;
            Ok(Op::AddMedia { media: m })
        }
        Op::AddTimeline { timeline } => {
            ensure_free(p, timeline.id)?;
            let mut seen = vec![timeline.id];
            for t in &timeline.tracks {
                ensure_free(p, t.id)?;
                seen.push(t.id);
                for i in &t.items {
                    ensure_free(p, i.id())?;
                    seen.push(i.id());
                    check_item(p, timeline.rate, i)?;
                }
            }
            seen.sort();
            if let Some(w) = seen.windows(2).find(|w| w[0] == w[1]) {
                return Err(OpError::IdInUse(w[0]));
            }
            p.timelines.insert(timeline.id, Arc::new(timeline.clone()));
            Ok(Op::RemoveTimeline { id: timeline.id })
        }
        Op::RemoveTimeline { id } => {
            let tl = p
                .timelines
                .remove(id)
                .ok_or(OpError::NotFound("timeline", *id))?;
            Ok(Op::AddTimeline {
                timeline: (*tl).clone(),
            })
        }
        Op::AddTrack {
            timeline,
            index,
            track,
        } => {
            ensure_free(p, track.id)?;
            let rate = p
                .timeline(*timeline)
                .ok_or(OpError::NotFound("timeline", *timeline))?
                .rate;
            let mut ids = vec![track.id];
            for i in &track.items {
                ensure_free(p, i.id())?;
                if ids.contains(&i.id()) {
                    return Err(OpError::IdInUse(i.id()));
                }
                ids.push(i.id());
                check_item(p, rate, i)?;
            }
            let tl = p.timeline_mut(*timeline).expect("checked above");
            if *index > tl.tracks.len() {
                return Err(OpError::IndexOutOfBounds {
                    index: *index,
                    len: tl.tracks.len(),
                });
            }
            tl.tracks.insert(*index, Arc::new(track.clone()));
            Ok(Op::RemoveTrack { track: track.id })
        }
        Op::RemoveTrack { track } => {
            let (tl_id, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            let tl = p.timeline_mut(tl_id).expect("located");
            let old = tl.tracks.remove(ti);
            Ok(Op::AddTrack {
                timeline: tl_id,
                index: ti,
                track: (*old).clone(),
            })
        }
        Op::InsertItem { track, index, item } => {
            ensure_free(p, item.id())?;
            let (tl_id, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            check_item(p, timeline_rate(p, tl_id), item)?;
            let t = p.timeline_mut(tl_id).expect("located").track_mut(ti);
            if *index > t.items.len() {
                return Err(OpError::IndexOutOfBounds {
                    index: *index,
                    len: t.items.len(),
                });
            }
            t.items.insert(*index, item.clone());
            Ok(Op::RemoveItem { item: item.id() })
        }
        Op::RemoveItem { item } => {
            let loc = item_loc(p, *item)?;
            let track_id = p.timeline(loc.timeline).expect("located").tracks[loc.track].id;
            let t = p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track);
            let old = t.items.remove(loc.index);
            Ok(Op::InsertItem {
                track: track_id,
                index: loc.index,
                item: old,
            })
        }
        Op::ReplaceItem { item, with } => {
            let loc = item_loc(p, *item)?;
            if with.id() != *item {
                ensure_free(p, with.id())?;
            }
            let rate = timeline_rate(p, loc.timeline);
            check_item(p, rate, with)?;
            let slot = &mut p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index];
            if slot.duration() != with.duration() {
                return Err(OpError::OutOfRange("replacement must keep the duration"));
            }
            let old = std::mem::replace(slot, with.clone());
            Ok(Op::ReplaceItem {
                item: with.id(),
                with: old,
            })
        }
        Op::Split { item, at, new_id } => {
            ensure_free(p, *new_id)?;
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let old = p.item(loc).expect("located").clone();
            if at.rate != rate || at.value <= 0 || at.value >= old.duration().value {
                return Err(OpError::OutOfRange("split point must be inside the item"));
            }
            let rest = RationalTime::new(old.duration().value - at.value, rate);
            let (left, right) = match &old {
                Item::Clip(c) => {
                    // the right half's first frame is the source frame the original showed at `at`
                    let off = source_offset(*at, c.source_start.rate)?;
                    // a fade-in stays with the left half and a fade-out with the right one; clamp to fit
                    let l = Clip {
                        duration: *at,
                        audio: AudioProps {
                            fade_in: c.audio.fade_in.min(at.value),
                            fade_out: 0,
                            ..c.audio
                        },
                        ..c.clone()
                    };
                    let r = Clip {
                        id: *new_id,
                        source_start: arith(c.source_start.checked_add(off))?,
                        duration: rest,
                        audio: AudioProps {
                            fade_in: 0,
                            fade_out: c.audio.fade_out.min(rest.value),
                            ..c.audio
                        },
                        ..c.clone()
                    };
                    (Item::Clip(l), Item::Clip(r))
                }
                Item::Gap(g) => (
                    Item::Gap(Gap {
                        id: g.id,
                        duration: *at,
                    }),
                    Item::Gap(Gap {
                        id: *new_id,
                        duration: rest,
                    }),
                ),
            };
            let t = p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track);
            t.items[loc.index] = left;
            t.items.insert(loc.index + 1, right);
            // exact undo: restore the original item and drop the new half
            Ok(Op::Transaction {
                description: "Undo split".into(),
                ops: vec![
                    Op::ReplaceRaw {
                        item: *item,
                        with: old,
                    },
                    Op::RemoveItem { item: *new_id },
                ],
            })
        }
        Op::Join { left, right } => {
            let ll = item_loc(p, *left)?;
            let rl = item_loc(p, *right)?;
            if ll.timeline != rl.timeline || ll.track != rl.track || rl.index != ll.index + 1 {
                return Err(OpError::NotJoinable(
                    "items are not neighbours on one track",
                ));
            }
            let (l, r) = (
                p.item(ll).expect("located").clone(),
                p.item(rl).expect("located").clone(),
            );
            let track_id = p.timeline(ll.timeline).expect("located").tracks[ll.track].id;
            let joined = match (&l, &r) {
                (Item::Clip(a), Item::Clip(b)) => {
                    // contiguous = the right clip starts where the left one ends, give or take the
                    // one source frame lost when a cut falls between source frames
                    let end = source_offset(a.duration, a.source_start.rate)?;
                    let end = arith(a.source_start.checked_add(end))?;
                    let slack = (end.value - b.source_start.value).abs() <= 1
                        && end.rate == b.source_start.rate;
                    if a.media != b.media
                        || a.name != b.name
                        || a.enabled != b.enabled
                        || a.video != b.video
                        || (AudioProps {
                            fade_in: 0,
                            fade_out: 0,
                            ..a.audio
                        }) != (AudioProps {
                            fade_in: 0,
                            fade_out: 0,
                            ..b.audio
                        })
                        || !slack
                    {
                        return Err(OpError::NotJoinable(
                            "clips are not a continuous run of one source",
                        ));
                    }
                    let audio = AudioProps {
                        fade_out: b.audio.fade_out,
                        ..a.audio
                    };
                    Item::Clip(Clip {
                        duration: arith(a.duration.checked_add(b.duration))?,
                        audio,
                        ..a.clone()
                    })
                }
                (Item::Gap(a), Item::Gap(b)) => Item::Gap(Gap {
                    id: a.id,
                    duration: arith(a.duration.checked_add(b.duration))?,
                }),
                _ => return Err(OpError::NotJoinable("a clip and a gap")),
            };
            if let Item::Clip(c) = &joined {
                c.check_props(timeline_rate(p, ll.timeline))
                    .map_err(OpError::BadProps)?;
            }
            let t = p
                .timeline_mut(ll.timeline)
                .expect("located")
                .track_mut(ll.track);
            t.items[ll.index] = joined;
            t.items.remove(rl.index);
            // exact undo: put the left item back as it was and re-insert the right one
            Ok(Op::Transaction {
                description: "Undo join".into(),
                ops: vec![
                    Op::ReplaceRaw {
                        item: *left,
                        with: l.clone(),
                    },
                    Op::InsertItem {
                        track: track_id,
                        index: rl.index,
                        item: r,
                    },
                ],
            })
        }
        Op::Trim { item, edge, delta } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            if delta.rate != rate {
                return Err(OpError::BadDuration(*item));
            }
            let old = p.item(loc).expect("located").clone();
            let (new, inverse) = match (&old, edge) {
                (Item::Clip(c), _) => {
                    let (start, duration) = match edge {
                        Edge::In => (
                            arith(
                                c.source_start
                                    .checked_add(source_offset(*delta, c.source_start.rate)?),
                            )?,
                            arith(c.duration.checked_sub(*delta))?,
                        ),
                        Edge::Out => (c.source_start, arith(c.duration.checked_add(*delta))?),
                    };
                    // fades that no longer fit are shortened; undo puts them back
                    let (fi, fo) = fit_fades(c.audio.fade_in, c.audio.fade_out, duration.value);
                    let audio = AudioProps {
                        fade_in: fi,
                        fade_out: fo,
                        ..c.audio
                    };
                    let new = Item::Clip(Clip {
                        source_start: start,
                        duration,
                        audio,
                        ..c.clone()
                    });
                    let mut undo = vec![Op::SetRange {
                        item: *item,
                        source_start: c.source_start,
                        duration: c.duration,
                    }];
                    if audio != c.audio {
                        undo.push(Op::SetAudio {
                            item: *item,
                            audio: c.audio,
                        });
                    }
                    let inverse = if undo.len() == 1 {
                        undo.remove(0)
                    } else {
                        Op::Transaction {
                            description: "Undo trim".into(),
                            ops: undo,
                        }
                    };
                    (new, inverse)
                }
                (Item::Gap(g), _) => {
                    let d = match edge {
                        Edge::In => arith(g.duration.checked_sub(*delta))?,
                        Edge::Out => arith(g.duration.checked_add(*delta))?,
                    };
                    let inv = Op::Trim {
                        item: *item,
                        edge: *edge,
                        delta: RationalTime::new(-delta.value, rate),
                    };
                    (
                        Item::Gap(Gap {
                            id: g.id,
                            duration: d,
                        }),
                        inv,
                    )
                }
            };
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(inverse)
        }
        Op::SetRange {
            item,
            source_start,
            duration,
        } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let Item::Clip(c) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("clip", *item));
            };
            let new = Item::Clip(Clip {
                source_start: *source_start,
                duration: *duration,
                ..c.clone()
            });
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetRange {
                item: *item,
                source_start: c.source_start,
                duration: c.duration,
            })
        }
        Op::ReplaceRaw { item, with } => {
            // like ReplaceItem but the new item may differ in length (used by undo of Join)
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            check_item(p, rate, with)?;
            let slot = &mut p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index];
            let old = std::mem::replace(slot, with.clone());
            Ok(Op::ReplaceRaw {
                item: with.id(),
                with: old,
            })
        }
        Op::Slip { item, delta } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            if delta.rate != rate {
                return Err(OpError::BadDuration(*item));
            }
            let Item::Clip(c) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("clip", *item));
            };
            let d = source_offset(*delta, c.source_start.rate)?;
            let new = Item::Clip(Clip {
                source_start: arith(c.source_start.checked_add(d))?,
                ..c.clone()
            });
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetRange {
                item: *item,
                source_start: c.source_start,
                duration: c.duration,
            })
        }
        Op::SetVideo { item, video } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let Item::Clip(c) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("clip", *item));
            };
            let new = Clip {
                video: *video,
                ..c.clone()
            };
            check_item(p, rate, &Item::Clip(new.clone()))?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = Item::Clip(new);
            Ok(Op::SetVideo {
                item: *item,
                video: c.video,
            })
        }
        Op::SetAudio { item, audio } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let Item::Clip(c) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("clip", *item));
            };
            let new = Clip {
                audio: *audio,
                ..c.clone()
            };
            check_item(p, rate, &Item::Clip(new.clone()))?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = Item::Clip(new);
            Ok(Op::SetAudio {
                item: *item,
                audio: c.audio,
            })
        }
        Op::SetTrack {
            track,
            enabled,
            gain_db,
        } => {
            if !gain_db.is_finite() {
                return Err(OpError::BadProps("non-finite gain"));
            }
            let (tl, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            let t = p.timeline_mut(tl).expect("located").track_mut(ti);
            let prev = Op::SetTrack {
                track: *track,
                enabled: t.enabled,
                gain_db: t.gain_db,
            };
            t.enabled = *enabled;
            t.gain_db = *gain_db;
            Ok(prev)
        }
        Op::MoveTrack { track, index } => {
            let (tl_id, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            let tl = p.timeline_mut(tl_id).expect("located");
            if *index >= tl.tracks.len() {
                return Err(OpError::IndexOutOfBounds {
                    index: *index,
                    len: tl.tracks.len(),
                });
            }
            let t = tl.tracks.remove(ti);
            tl.tracks.insert(*index, t);
            Ok(Op::MoveTrack {
                track: *track,
                index: ti,
            })
        }
        Op::SetProxy { media, proxy } => {
            let m = p
                .media
                .get_mut(media)
                .ok_or(OpError::NotFound("media", *media))?;
            let prev = std::mem::replace(&mut m.proxy, proxy.clone());
            Ok(Op::SetProxy {
                media: *media,
                proxy: prev,
            })
        }
        Op::SetEnabled { item, enabled } => {
            let loc = item_loc(p, *item)?;
            let slot = &mut p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index];
            match slot {
                Item::Clip(c) => {
                    let prev = std::mem::replace(&mut c.enabled, *enabled);
                    Ok(Op::SetEnabled {
                        item: *item,
                        enabled: prev,
                    })
                }
                Item::Gap(_) => Err(OpError::NotFound("clip", *item)),
            }
        }
        Op::SetName { item, name } => {
            let loc = item_loc(p, *item)?;
            let slot = &mut p
                .timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index];
            match slot {
                Item::Clip(c) => {
                    let prev = std::mem::replace(&mut c.name, name.clone());
                    Ok(Op::SetName {
                        item: *item,
                        name: prev,
                    })
                }
                Item::Gap(_) => Err(OpError::NotFound("clip", *item)),
            }
        }
        Op::Transaction { description, ops } => {
            // all-or-nothing: `p` is only the caller's private copy, so a failure just drops it
            let mut inverses = Vec::with_capacity(ops.len());
            for o in ops {
                inverses.push(apply_in_place(p, o)?);
            }
            inverses.reverse();
            Ok(Op::Transaction {
                description: format!("Undo: {description}"),
                ops: inverses,
            })
        }
    }
}
