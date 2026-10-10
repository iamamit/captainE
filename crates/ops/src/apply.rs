use std::fmt;
use std::sync::Arc;

use captaine_model::{AudioProps, Clip, Gap, Id, Item, ItemLoc, Project, Title, Transition};
use captaine_time::{Rational, RationalTime};

use crate::{Edge, Op};

/// A speed ramp is spread over the whole clip, so cutting or trimming it would change the speeds.
fn ramp_guard(item: &Item) -> Result<()> {
    match item {
        Item::Clip(c) if c.retime.ramp_to > 0.0 => Err(OpError::BadProps(
            "remove the speed ramp before cutting or trimming this clip",
        )),
        _ => Ok(()),
    }
}

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
    if p.nesting_cycle().is_some() {
        return Err(OpError::OutOfRange("a sequence cannot contain itself"));
    }
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

/// Whether `item` may carry transition `t` right now: a clip must sit directly before it and be long
/// enough. Applying the op does not insist (later edits can make a transition too long, and undo must
/// always be able to put one back); the renderer shortens it to fit.
pub fn check_transition(p: &Project, item: Id, t: &Transition) -> Result<()> {
    let loc = item_loc(p, item)?;
    let tl = p.timeline(loc.timeline).expect("located");
    let items = &tl.tracks[loc.track].items;
    let Some(Item::Clip(c)) = items.get(loc.index) else {
        return Err(OpError::NotFound("clip", item));
    };
    let prev = loc.index.checked_sub(1).and_then(|i| items.get(i));
    let Some(Item::Clip(prev)) = prev else {
        return Err(OpError::OutOfRange(
            "a transition needs a clip right before this one",
        ));
    };
    if t.duration < 2 || t.duration > 2 * prev.duration.value.min(c.duration.value) {
        return Err(OpError::OutOfRange(
            "the transition is longer than the two clips allow",
        ));
    }
    Ok(())
}

/// An item that may be placed on a timeline running at `rate`.
fn check_item(p: &Project, rate: Rational, item: &Item) -> Result<()> {
    let d = item.duration();
    if d.rate != rate || d.value <= 0 {
        return Err(OpError::BadDuration(item.id()));
    }
    if let Item::Title(t) = item {
        t.check_props(rate).map_err(OpError::BadProps)?;
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
            let end =
                RationalTime::new(c.source_start.value + c.source_span(), c.source_start.rate);
            if end > len {
                return Err(OpError::OutOfRange("clip runs past the end of its media"));
            }
        }
    }
    Ok(())
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
            let used_inside = p
                .media
                .values()
                .any(|m| m.nested_timeline() == Some(*id) && p.media_in_use(m.id));
            if used_inside {
                return Err(OpError::OutOfRange(
                    "this sequence is used inside another one",
                ));
            }
            let tl = p
                .timelines
                .remove(id)
                .ok_or(OpError::NotFound("timeline", *id))?;
            Ok(Op::AddTimeline {
                timeline: (*tl).clone(),
            })
        }
        Op::SetCanvas { timeline, canvas } => {
            if let Some((w, h)) = canvas {
                if *w < 16 || *h < 16 || *w > 16384 || *h > 16384 || w % 2 != 0 || h % 2 != 0 {
                    return Err(OpError::OutOfRange("frame size must be even, 16 to 16384"));
                }
            }
            let tl = p
                .timeline_mut(*timeline)
                .ok_or(OpError::NotFound("timeline", *timeline))?;
            let old = std::mem::replace(&mut tl.canvas, *canvas);
            Ok(Op::SetCanvas {
                timeline: *timeline,
                canvas: old,
            })
        }
        Op::AddMarker { timeline, marker } => {
            marker.check().map_err(OpError::BadProps)?;
            ensure_free(p, marker.id)?;
            let tl = p
                .timeline_mut(*timeline)
                .ok_or(OpError::NotFound("timeline", *timeline))?;
            tl.markers.push(marker.clone());
            tl.markers.sort_by_key(|m| (m.tick, m.id));
            Ok(Op::RemoveMarker {
                timeline: *timeline,
                marker: marker.id,
            })
        }
        Op::RemoveMarker { timeline, marker } => {
            let tl = p
                .timeline_mut(*timeline)
                .ok_or(OpError::NotFound("timeline", *timeline))?;
            let i = tl
                .markers
                .iter()
                .position(|m| m.id == *marker)
                .ok_or(OpError::NotFound("marker", *marker))?;
            let old = tl.markers.remove(i);
            Ok(Op::AddMarker {
                timeline: *timeline,
                marker: old,
            })
        }
        Op::SetMarker { timeline, marker } => {
            marker.check().map_err(OpError::BadProps)?;
            let tl = p
                .timeline_mut(*timeline)
                .ok_or(OpError::NotFound("timeline", *timeline))?;
            let i = tl
                .markers
                .iter()
                .position(|m| m.id == marker.id)
                .ok_or(OpError::NotFound("marker", marker.id))?;
            let old = std::mem::replace(&mut tl.markers[i], marker.clone());
            tl.markers.sort_by_key(|m| (m.tick, m.id));
            Ok(Op::SetMarker {
                timeline: *timeline,
                marker: old,
            })
        }
        Op::RenameTimeline { id, name } => {
            let tl = p
                .timeline_mut(*id)
                .ok_or(OpError::NotFound("timeline", *id))?;
            let old = std::mem::replace(&mut tl.name, name.clone());
            Ok(Op::RenameTimeline { id: *id, name: old })
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
            ramp_guard(&old)?;
            let rest = RationalTime::new(old.duration().value - at.value, rate);
            let (left, right) = match &old {
                Item::Clip(c) => {
                    // the right half's first frame is the source frame the original showed at `at`
                    let off = RationalTime::new(c.advance(at.value), c.source_start.rate);
                    // a fade-in stays with the left half and a fade-out with the right one; clamp to fit
                    let mut l = Clip {
                        duration: *at,
                        audio: AudioProps {
                            fade_in: c.audio.fade_in.min(at.value),
                            fade_out: 0,
                            ..c.audio
                        },
                        anim: c.anim.window(0, at.value),
                        ..c.clone()
                    };
                    let r_start = if c.retime.backwards() {
                        // played back to front: the first half keeps the top of the window
                        l.source_start = RationalTime::new(
                            c.source_start.value + c.source_span() - l.source_span(),
                            c.source_start.rate,
                        );
                        c.source_start
                    } else {
                        arith(c.source_start.checked_add(off))?
                    };
                    let r = Clip {
                        id: *new_id,
                        source_start: r_start,
                        duration: rest,
                        audio: AudioProps {
                            fade_in: 0,
                            fade_out: c.audio.fade_out.min(rest.value),
                            ..c.audio
                        },
                        anim: c.anim.window(at.value, c.duration.value),
                        transition: None,
                        ..c.clone()
                    };
                    (Item::Clip(l), Item::Clip(r))
                }
                Item::Title(t) => {
                    let l = Title {
                        duration: *at,
                        fade_in: t.fade_in.min(at.value),
                        fade_out: 0,
                        anim: t.anim.window(0, at.value),
                        ..t.clone()
                    };
                    let r = Title {
                        id: *new_id,
                        duration: rest,
                        fade_in: 0,
                        fade_out: t.fade_out.min(rest.value),
                        anim: t.anim.window(at.value, t.duration.value),
                        ..t.clone()
                    };
                    (Item::Title(l), Item::Title(r))
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
                    let slack = if a.retime.backwards() {
                        // the later half sits below the earlier one in the source
                        let end = b.source_start.value + b.source_span();
                        (end - a.source_start.value).abs() <= 1
                    } else {
                        let end = a.source_start.value + a.advance(a.duration.value);
                        (end - b.source_start.value).abs() <= 1
                    } && a.source_start.rate == b.source_start.rate;
                    if b.transition.is_some() {
                        return Err(OpError::NotJoinable("a transition lies between the clips"));
                    }
                    if a.media != b.media
                        || a.retime != b.retime
                        || a.effects != b.effects
                        || a.link != b.link
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
                    let anim = a.anim.joined(a.duration.value, &b.anim);
                    // only two halves of one animation may be joined back into it
                    let total = a.duration.value + b.duration.value;
                    if anim.window(0, a.duration.value) != a.anim
                        || anim.window(a.duration.value, total) != b.anim
                    {
                        return Err(OpError::NotJoinable(
                            "the two clips are animated differently",
                        ));
                    }
                    Item::Clip(Clip {
                        source_start: if a.retime.backwards() {
                            b.source_start
                        } else {
                            a.source_start
                        },
                        duration: arith(a.duration.checked_add(b.duration))?,
                        audio,
                        anim,
                        ..a.clone()
                    })
                }
                (Item::Title(a), Item::Title(b)) => {
                    if a.name != b.name
                        || a.enabled != b.enabled
                        || a.text != b.text
                        || a.subtitle != b.subtitle
                        || a.style != b.style
                        || a.shape != b.shape
                        || a.adjustment != b.adjustment
                        || a.effects != b.effects
                        || a.video != b.video
                    {
                        return Err(OpError::NotJoinable("titles differ"));
                    }
                    let anim = a.anim.joined(a.duration.value, &b.anim);
                    let total = a.duration.value + b.duration.value;
                    if anim.window(0, a.duration.value) != a.anim
                        || anim.window(a.duration.value, total) != b.anim
                    {
                        return Err(OpError::NotJoinable(
                            "the two titles are animated differently",
                        ));
                    }
                    Item::Title(Title {
                        duration: arith(a.duration.checked_add(b.duration))?,
                        fade_out: b.fade_out,
                        anim,
                        ..a.clone()
                    })
                }
                (Item::Gap(a), Item::Gap(b)) => Item::Gap(Gap {
                    id: a.id,
                    duration: arith(a.duration.checked_add(b.duration))?,
                }),
                _ => return Err(OpError::NotJoinable("a clip and a gap")),
            };
            if let Item::Title(t) = &joined {
                t.check_props(timeline_rate(p, ll.timeline))
                    .map_err(OpError::BadProps)?;
            }
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
            ramp_guard(&old)?;
            let (new, inverse) = match (&old, edge) {
                (Item::Clip(c), _) => {
                    let duration = match edge {
                        Edge::In => arith(c.duration.checked_sub(*delta))?,
                        Edge::Out => arith(c.duration.checked_add(*delta))?,
                    };
                    let start = if c.retime.backwards() {
                        // the top of the window stays put on an Out trim and moves down with an In trim
                        let moved = Clip {
                            duration,
                            ..c.clone()
                        };
                        let top = c.source_start.value + c.source_span()
                            - 1
                            - if *edge == Edge::In {
                                c.advance(delta.value)
                            } else {
                                0
                            };
                        RationalTime::new(top + 1 - moved.source_span(), c.source_start.rate)
                    } else if *edge == Edge::In {
                        arith(c.source_start.checked_add(RationalTime::new(
                            c.advance(delta.value),
                            c.source_start.rate,
                        )))?
                    } else {
                        c.source_start
                    };
                    // fades that no longer fit are shortened; undo puts them back
                    let (fi, fo) = fit_fades(c.audio.fade_in, c.audio.fade_out, duration.value);
                    let audio = AudioProps {
                        fade_in: fi,
                        fade_out: fo,
                        ..c.audio
                    };
                    // trimming the front moves the clip's start, so its animation moves with the picture
                    let anim = match edge {
                        Edge::In => c.anim.shifted(-delta.value),
                        Edge::Out => c.anim.clone(),
                    };
                    let new = Item::Clip(Clip {
                        source_start: start,
                        duration,
                        audio,
                        anim,
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
                    let inverse = if !c.anim.is_empty() {
                        // keyframes were moved too: only the whole old clip puts everything back exactly
                        Op::ReplaceRaw {
                            item: *item,
                            with: old.clone(),
                        }
                    } else if undo.len() == 1 {
                        undo.remove(0)
                    } else {
                        Op::Transaction {
                            description: "Undo trim".into(),
                            ops: undo,
                        }
                    };
                    (new, inverse)
                }
                (Item::Title(t), _) => {
                    let duration = match edge {
                        Edge::In => arith(t.duration.checked_sub(*delta))?,
                        Edge::Out => arith(t.duration.checked_add(*delta))?,
                    };
                    let (fi, fo) = fit_fades(t.fade_in, t.fade_out, duration.value);
                    let anim = match edge {
                        Edge::In => t.anim.shifted(-delta.value),
                        Edge::Out => t.anim.clone(),
                    };
                    let new = Item::Title(Title {
                        duration,
                        fade_in: fi,
                        fade_out: fo,
                        anim,
                        ..t.clone()
                    });
                    // exact undo: put the whole old title back
                    (
                        new,
                        Op::ReplaceRaw {
                            item: *item,
                            with: old.clone(),
                        },
                    )
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
            let d = RationalTime::new(
                if c.retime.backwards() {
                    -c.advance(delta.value)
                } else {
                    c.advance(delta.value)
                },
                c.source_start.rate,
            );
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
        Op::SetLink { item, link } => {
            let loc = item_loc(p, *item)?;
            let Item::Clip(c) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("clip", *item));
            };
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = Item::Clip(Clip {
                link: *link,
                ..c.clone()
            });
            Ok(Op::SetLink {
                item: *item,
                link: c.link,
            })
        }
        Op::SetEffects { item, effects } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let (new, previous) = match p.item(loc).expect("located").clone() {
                Item::Clip(c) => (
                    Item::Clip(Clip {
                        effects: effects.clone(),
                        ..c.clone()
                    }),
                    c.effects,
                ),
                Item::Title(t) => (
                    Item::Title(Title {
                        effects: effects.clone(),
                        ..t.clone()
                    }),
                    t.effects,
                ),
                Item::Gap(_) => return Err(OpError::NotFound("clip", *item)),
            };
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetEffects {
                item: *item,
                effects: previous,
            })
        }
        Op::SetTransition { item, transition } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let old = p.item(loc).expect("located").clone();
            let Item::Clip(c) = &old else {
                return Err(OpError::NotFound("clip", *item));
            };
            let new = Item::Clip(Clip {
                transition: *transition,
                ..c.clone()
            });
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetTransition {
                item: *item,
                transition: c.transition,
            })
        }
        Op::SetRetime { item, retime } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let old = p.item(loc).expect("located").clone();
            let Item::Clip(c) = &old else {
                return Err(OpError::NotFound("clip", *item));
            };
            // the same stretch of source plays at the new speed, so the clip gets shorter or longer;
            // freezing and un-freezing keep its length
            let keep = c.retime.freeze || retime.freeze;
            let mut ticks = if keep {
                c.duration.value
            } else {
                ((c.duration.value as f64 * c.retime.mean_speed() / retime.mean_speed()).round()
                    as i64)
                    .max(1)
            };
            let new = loop {
                let (fi, fo) = fit_fades(c.audio.fade_in, c.audio.fade_out, ticks);
                let cand = Item::Clip(Clip {
                    retime: *retime,
                    duration: RationalTime::new(ticks, rate),
                    audio: AudioProps {
                        fade_in: fi,
                        fade_out: fo,
                        ..c.audio
                    },
                    anim: c.anim.window(0, ticks),
                    ..c.clone()
                });
                match check_item(p, rate, &cand) {
                    // rounding may push the window one frame past the end of the media
                    Err(OpError::OutOfRange(_)) if !keep && ticks > 1 => ticks -= 1,
                    Err(e) => return Err(e),
                    Ok(()) => break cand,
                }
            };
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::ReplaceRaw {
                item: *item,
                with: old,
            })
        }
        Op::SetVideo { item, video } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let (new, prev) = match p.item(loc).expect("located").clone() {
                Item::Clip(c) => (
                    Item::Clip(Clip {
                        video: *video,
                        ..c.clone()
                    }),
                    c.video,
                ),
                Item::Title(t) => (
                    Item::Title(Title {
                        video: *video,
                        ..t.clone()
                    }),
                    t.video,
                ),
                Item::Gap(_) => return Err(OpError::NotFound("clip", *item)),
            };
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetVideo {
                item: *item,
                video: prev,
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
        Op::SetTrackMix {
            track,
            pan,
            solo,
            locked,
        } => {
            if !pan.is_finite() || !(-1.0..=1.0).contains(pan) {
                return Err(OpError::BadProps("track pan is -1 to 1"));
            }
            let (tl, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            let t = p.timeline_mut(tl).expect("located").track_mut(ti);
            let prev = Op::SetTrackMix {
                track: *track,
                pan: t.pan,
                solo: t.solo,
                locked: t.locked,
            };
            t.pan = *pan;
            t.solo = *solo;
            t.locked = *locked;
            Ok(prev)
        }
        Op::SetTrackStyle { track, name, color } => {
            if name.trim().is_empty() || name.len() > 40 {
                return Err(OpError::BadProps("a track name is 1 to 40 characters"));
            }
            let (tl, ti) = p
                .find_track(*track)
                .ok_or(OpError::NotFound("track", *track))?;
            let t = p.timeline_mut(tl).expect("located").track_mut(ti);
            let prev = Op::SetTrackStyle {
                track: *track,
                name: std::mem::replace(&mut t.name, name.clone()),
                color: std::mem::replace(&mut t.color, *color),
            };
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
        Op::SetMediaMeta { media, meta } => {
            if !p.media.contains_key(media) {
                return Err(OpError::NotFound("media", *media));
            }
            if let Some(m) = meta {
                m.check().map_err(OpError::BadProps)?;
            }
            let prev = match meta.clone().filter(|m| !m.is_empty()) {
                Some(m) => p.meta.insert(*media, m),
                None => p.meta.remove(media),
            };
            Ok(Op::SetMediaMeta {
                media: *media,
                meta: prev,
            })
        }
        Op::SetAnimTrack { item, prop, keys } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let (new, prev) = match p.item(loc).expect("located").clone() {
                Item::Clip(c) => {
                    let prev = c.anim.keys(prop).to_vec();
                    let mut anim = c.anim.clone();
                    anim.set_track(prop, keys.clone());
                    (Item::Clip(Clip { anim, ..c }), prev)
                }
                Item::Title(t) => {
                    let prev = t.anim.keys(prop).to_vec();
                    let mut anim = t.anim.clone();
                    anim.set_track(prop, keys.clone());
                    (Item::Title(Title { anim, ..t }), prev)
                }
                Item::Gap(_) => return Err(OpError::NotFound("clip", *item)),
            };
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetAnimTrack {
                item: *item,
                prop: prop.clone(),
                keys: prev,
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
                Item::Title(t) => {
                    let prev = std::mem::replace(&mut t.enabled, *enabled);
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
                Item::Title(t) => {
                    let prev = std::mem::replace(&mut t.name, name.clone());
                    Ok(Op::SetName {
                        item: *item,
                        name: prev,
                    })
                }
                Item::Gap(_) => Err(OpError::NotFound("clip", *item)),
            }
        }
        Op::SetTitle { item, title } => {
            let loc = item_loc(p, *item)?;
            let rate = timeline_rate(p, loc.timeline);
            let Item::Title(old) = p.item(loc).expect("located").clone() else {
                return Err(OpError::NotFound("title", *item));
            };
            if title.id != *item || title.duration != old.duration {
                return Err(OpError::OutOfRange("SetTitle keeps the id and the length"));
            }
            let new = Item::Title(title.clone());
            check_item(p, rate, &new)?;
            p.timeline_mut(loc.timeline)
                .expect("located")
                .track_mut(loc.track)
                .items[loc.index] = new;
            Ok(Op::SetTitle {
                item: *item,
                title: old,
            })
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
