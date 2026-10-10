//! Editing gestures built from the primitive ops: overwrite, lift, move, roll, slide, gap cleanup.
//!
//! Each function *plans* against the current project and returns one [`Op`] (usually a
//! transaction), so a gesture is a single undo step and an agent can call it like any other op.

use captaine_model::{Clip, Gap, Id, Item, Project};
use captaine_time::RationalTime;

use crate::{apply, Edge, Op, OpError};

type Result<T> = std::result::Result<T, OpError>;

/// Applies ops to a scratch copy while recording them, so later steps see earlier ones.
struct Planner {
    project: Project,
    ops: Vec<Op>,
}

impl Planner {
    fn new(project: &Project) -> Planner {
        Planner {
            project: project.clone(),
            ops: Vec::new(),
        }
    }

    fn push(&mut self, op: Op) -> Result<()> {
        self.project = apply(&self.project, &op)?.0;
        self.ops.push(op);
        Ok(())
    }

    fn finish(mut self, description: &str) -> Op {
        if self.ops.len() == 1 {
            return self.ops.pop().expect("one op");
        }
        Op::Transaction {
            description: description.to_string(),
            ops: self.ops,
        }
    }

    fn track(&self, id: Id) -> Result<(captaine_time::Rational, &captaine_model::Track)> {
        let (tl, ti) = self
            .project
            .find_track(id)
            .ok_or(OpError::NotFound("track", id))?;
        let tl = self.project.timeline(tl).expect("located");
        Ok((tl.rate, tl.tracks[ti].as_ref()))
    }

    /// Make sure an item boundary exists at `tick` (when it falls strictly inside an item).
    fn ensure_cut(&mut self, track: Id, tick: i64) -> Result<()> {
        let (rate, t) = self.track(track)?;
        let starts = t.item_starts();
        let hit = t
            .items
            .iter()
            .zip(&starts)
            .find(|(it, st)| tick > **st && tick < **st + it.duration().value)
            .map(|(it, st)| (it.id(), *st));
        if let Some((item, start)) = hit {
            self.push(Op::Split {
                item,
                at: RationalTime::new(tick - start, rate),
                new_id: Id::new(),
            })?;
        }
        Ok(())
    }

    fn overwrite(&mut self, track: Id, at: i64, item: Item) -> Result<()> {
        let (rate, t) = self.track(track)?;
        if at < 0 {
            return Err(OpError::OutOfRange(
                "cannot place before the start of the timeline",
            ));
        }
        let end = at + item.duration().value;
        let len = t.duration_ticks();
        if at > len {
            let index = t.items.len();
            let gap = Item::Gap(Gap {
                id: Id::new(),
                duration: RationalTime::new(at - len, rate),
            });
            self.push(Op::InsertItem {
                track,
                index,
                item: gap,
            })?;
        }
        self.ensure_cut(track, at)?;
        self.ensure_cut(track, end)?;
        let (_, t) = self.track(track)?;
        let covered: Vec<Id> = t
            .items
            .iter()
            .zip(t.item_starts())
            .filter(|(_, st)| *st >= at && *st < end)
            .map(|(it, _)| it.id())
            .collect();
        for item in covered {
            self.push(Op::RemoveItem { item })?;
        }
        let (_, t) = self.track(track)?;
        let index = t.item_starts().iter().filter(|st| **st < at).count();
        self.push(Op::InsertItem { track, index, item })
    }

    /// Merge neighbouring gaps and drop a trailing gap.
    fn clean_gaps(&mut self, track: Id) -> Result<()> {
        loop {
            let (_, t) = self.track(track)?;
            let pair = t.items.windows(2).find_map(|w| match (&w[0], &w[1]) {
                (Item::Gap(a), Item::Gap(b)) => Some((a.id, b.id)),
                _ => None,
            });
            match pair {
                Some((left, right)) => self.push(Op::Join { left, right })?,
                None => break,
            }
        }
        let (_, t) = self.track(track)?;
        if let Some(Item::Gap(g)) = t.items.last() {
            let item = g.id;
            self.push(Op::RemoveItem { item })?;
        }
        Ok(())
    }
}

fn locate(p: &Project, item: Id) -> Result<captaine_model::ItemLoc> {
    p.find_item(item).ok_or(OpError::NotFound("item", item))
}

/// Cut the item under `at` (timeline ticks) on `track`.
pub fn split_at(project: &Project, track: Id, at: RationalTime) -> Result<Op> {
    let mut pl = Planner::new(project);
    let (rate, _) = pl.track(track)?;
    if at.rate != rate {
        return Err(OpError::BadDuration(track));
    }
    let before = pl.ops.len();
    pl.ensure_cut(track, at.value)?;
    if pl.ops.len() == before {
        return Err(OpError::OutOfRange(
            "no item has this time strictly inside it",
        ));
    }
    Ok(pl.finish("Split"))
}

/// Replace a clip with a gap of the same length (nothing moves).
pub fn lift(project: &Project, item: Id) -> Result<Op> {
    let loc = locate(project, item)?;
    let d = project.item(loc).expect("located").duration();
    Ok(Op::ReplaceItem {
        item,
        with: Item::Gap(Gap {
            id: Id::new(),
            duration: d,
        }),
    })
}

/// Place `clip` at `at` on `track`, replacing whatever is there. Pads with a gap past the end.
pub fn overwrite(project: &Project, track: Id, at: RationalTime, clip: Clip) -> Result<Op> {
    overwrite_item(project, track, at, Item::Clip(clip))
}

/// Like [`overwrite`] for any item (a title, for instance).
pub fn overwrite_item(project: &Project, track: Id, at: RationalTime, item: Item) -> Result<Op> {
    let mut pl = Planner::new(project);
    let (rate, _) = pl.track(track)?;
    if at.rate != rate {
        return Err(OpError::BadDuration(track));
    }
    pl.overwrite(track, at.value, item)?;
    Ok(pl.finish("Overwrite"))
}

/// Ripple insert: put `item` at `at` on `track`, cutting whatever is there in two and pushing everything
/// from that point later. Past the end of the track it is the same as an overwrite.
pub fn insert_item(project: &Project, track: Id, at: RationalTime, item: Item) -> Result<Op> {
    let mut pl = Planner::new(project);
    let (rate, t) = pl.track(track)?;
    if at.rate != rate {
        return Err(OpError::BadDuration(track));
    }
    if at.value >= t.duration_ticks() {
        pl.overwrite(track, at.value, item)?;
        return Ok(pl.finish("Insert"));
    }
    pl.ensure_cut(track, at.value)?;
    let (_, t) = pl.track(track)?;
    let index = t.item_starts().iter().filter(|st| **st < at.value).count();
    pl.push(Op::InsertItem { track, index, item })?;
    Ok(pl.finish("Insert"))
}

/// Multicam: cut every angle track at `at` and show only angle `chosen` from there on (the other angles'
/// pieces are switched off). `angles` are the tracks holding the camera angles, in angle order.
pub fn multicam_cut(
    project: &Project,
    angles: &[Id],
    chosen: usize,
    at: RationalTime,
) -> Result<Op> {
    let Some(&keep) = angles.get(chosen) else {
        return Err(OpError::OutOfRange("there is no such camera angle"));
    };
    let mut pl = Planner::new(project);
    let (rate, _) = pl.track(keep)?;
    if at.rate != rate {
        return Err(OpError::BadDuration(keep));
    }
    for a in angles {
        pl.ensure_cut(*a, at.value)?;
    }
    // the piece that starts at the cut on each angle: on for the chosen one, off for the rest
    for a in angles {
        let (_, t) = pl.track(*a)?;
        let starts = t.item_starts();
        let hit = t
            .items
            .iter()
            .zip(&starts)
            .find(|(it, st)| **st == at.value && it.is_content())
            .map(|(it, _)| it.id());
        if let Some(item) = hit {
            let enabled = *a == keep;
            pl.push(Op::SetEnabled { item, enabled })?;
        }
    }
    if pl.ops.is_empty() {
        return Err(OpError::OutOfRange("no angle has a clip at this time"));
    }
    Ok(pl.finish("Multicam cut"))
}

/// Append at the end of the track.
pub fn append(project: &Project, track: Id, clip: Clip) -> Op {
    let index = project
        .find_track(track)
        .and_then(|(tl, ti)| project.timeline(tl).map(|t| t.tracks[ti].items.len()))
        .unwrap_or(0);
    Op::InsertItem {
        track,
        index,
        item: Item::Clip(clip),
    }
}

/// Drag a clip to `at` on `to_track` (overwrite semantics, like dragging in Premiere/Resolve).
pub fn move_item(project: &Project, item: Id, to_track: Id, at: RationalTime) -> Result<Op> {
    let loc = locate(project, item)?;
    let moving = project.item(loc).expect("located").clone();
    if !moving.is_content() {
        return Err(OpError::NotFound("clip", item));
    }
    let from_track = project.timeline(loc.timeline).expect("located").tracks[loc.track].id;
    let mut pl = Planner::new(project);
    let (rate, _) = pl.track(to_track)?;
    if at.rate != rate {
        return Err(OpError::BadDuration(to_track));
    }
    let gap = Item::Gap(Gap {
        id: Id::new(),
        duration: moving.duration(),
    });
    pl.push(Op::ReplaceItem { item, with: gap })?;
    pl.overwrite(to_track, at.value, moving)?;
    pl.clean_gaps(from_track)?;
    if to_track != from_track {
        pl.clean_gaps(to_track)?;
    }
    Ok(pl.finish("Move"))
}

/// Move the cut between two neighbouring items by `delta`; total length is unchanged.
pub fn roll(project: &Project, left: Id, right: Id, delta: RationalTime) -> Result<Op> {
    let (l, r) = (locate(project, left)?, locate(project, right)?);
    if l.timeline != r.timeline || l.track != r.track || r.index != l.index + 1 {
        return Err(OpError::NotJoinable("roll needs two neighbouring items"));
    }
    let mut pl = Planner::new(project);
    pl.push(Op::Trim {
        item: left,
        edge: Edge::Out,
        delta,
    })?;
    pl.push(Op::Trim {
        item: right,
        edge: Edge::In,
        delta,
    })?;
    Ok(pl.finish("Roll"))
}

/// Move an item along the track by `delta`, taking time from one neighbour and giving it to the other.
pub fn slide(project: &Project, item: Id, delta: RationalTime) -> Result<Op> {
    let loc = locate(project, item)?;
    let t = &project.timeline(loc.timeline).expect("located").tracks[loc.track];
    let prev = loc
        .index
        .checked_sub(1)
        .and_then(|i| t.items.get(i))
        .ok_or(OpError::OutOfRange("slide needs a previous item"))?;
    let next = t
        .items
        .get(loc.index + 1)
        .ok_or(OpError::OutOfRange("slide needs a next item"))?;
    let mut pl = Planner::new(project);
    pl.push(Op::Trim {
        item: prev.id(),
        edge: Edge::Out,
        delta,
    })?;
    pl.push(Op::Trim {
        item: next.id(),
        edge: Edge::In,
        delta,
    })?;
    Ok(pl.finish("Slide"))
}

/// Merge adjacent gaps and remove a trailing gap on `track`.
pub fn clean_gaps(project: &Project, track: Id) -> Result<Option<Op>> {
    let mut pl = Planner::new(project);
    pl.clean_gaps(track)?;
    Ok(if pl.ops.is_empty() {
        None
    } else {
        Some(pl.finish("Clean up gaps"))
    })
}

/// Items of every track of `timeline` that lie inside `[start, end)` after cutting at both ends.
fn range_items(
    pl: &mut Planner,
    timeline: Id,
    start: i64,
    end: i64,
    skip: &[Id],
) -> Result<Vec<Id>> {
    let tl = pl
        .project
        .timeline(timeline)
        .ok_or(OpError::NotFound("timeline", timeline))?;
    let tracks: Vec<Id> = tl
        .tracks
        .iter()
        .map(|t| t.id)
        .filter(|t| !skip.contains(t))
        .collect();
    let mut inside = Vec::new();
    for track in tracks {
        pl.ensure_cut(track, start)?;
        pl.ensure_cut(track, end)?;
        let (_, t) = pl.track(track)?;
        inside.extend(
            t.items
                .iter()
                .zip(t.item_starts())
                .filter(|(it, st)| *st >= start && *st + it.duration().value <= end)
                .map(|(it, _)| it.id()),
        );
    }
    Ok(inside)
}

fn check_range(
    project: &Project,
    timeline: Id,
    start: RationalTime,
    end: RationalTime,
) -> Result<(i64, i64)> {
    let tl = project
        .timeline(timeline)
        .ok_or(OpError::NotFound("timeline", timeline))?;
    if start.rate != tl.rate || end.rate != tl.rate || start.value < 0 || end.value <= start.value {
        return Err(OpError::OutOfRange(
            "range must be a non-empty span at the timeline rate",
        ));
    }
    Ok((start.value, end.value))
}

/// Remove `[start, end)` from every track and close the hole, so all tracks stay in sync (ripple).
pub fn extract_range(
    project: &Project,
    timeline: Id,
    start: RationalTime,
    end: RationalTime,
) -> Result<Op> {
    extract_range_except(project, timeline, start, end, &[])
}

/// Like [`extract_range`], leaving the tracks in `skip` untouched.
pub fn extract_range_except(
    project: &Project,
    timeline: Id,
    start: RationalTime,
    end: RationalTime,
    skip: &[Id],
) -> Result<Op> {
    let (s, e) = check_range(project, timeline, start, end)?;
    let mut pl = Planner::new(project);
    for item in range_items(&mut pl, timeline, s, e, skip)? {
        pl.push(Op::RemoveItem { item })?;
    }
    if pl.ops.is_empty() {
        return Err(OpError::OutOfRange("nothing in that range"));
    }
    Ok(pl.finish("Extract range"))
}

/// Replace the contents of `[start, end)` on every track with gaps (nothing moves).
pub fn lift_range(
    project: &Project,
    timeline: Id,
    start: RationalTime,
    end: RationalTime,
) -> Result<Op> {
    lift_range_except(project, timeline, start, end, &[])
}

/// Like [`lift_range`], leaving the tracks in `skip` untouched.
pub fn lift_range_except(
    project: &Project,
    timeline: Id,
    start: RationalTime,
    end: RationalTime,
    skip: &[Id],
) -> Result<Op> {
    let (s, e) = check_range(project, timeline, start, end)?;
    let mut pl = Planner::new(project);
    for item in range_items(&mut pl, timeline, s, e, skip)? {
        let loc = locate(&pl.project, item)?;
        if let Some(c) = pl.project.item(loc).filter(|i| i.is_content()) {
            let with = Item::Gap(Gap {
                id: Id::new(),
                duration: c.duration(),
            });
            pl.push(Op::ReplaceItem { item, with })?;
        }
    }
    if pl.ops.is_empty() {
        return Err(OpError::OutOfRange("nothing in that range"));
    }
    let tracks: Vec<Id> = pl
        .project
        .timeline(timeline)
        .expect("checked")
        .tracks
        .iter()
        .map(|t| t.id)
        .collect();
    for track in tracks {
        pl.clean_gaps(track)?;
    }
    Ok(pl.finish("Lift range"))
}
