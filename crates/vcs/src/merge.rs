//! Three-way merge of two states that share an ancestor (docs/07).
//!
//! Everything is matched by stable id. *Their* changes (base → theirs) are applied on top of *ours*, one change at
//! a time, so the user can take or skip each:
//!
//! * items and tracks are inserted next to the nearest neighbour that exists on our side, so an earlier ripple
//!   edit on either branch never makes a later change land in the wrong place;
//! * an item both sides changed is merged field by field; only the same field set to different values conflicts;
//! * deleted-on-one-side-changed-on-the-other conflicts;
//! * gaps (explicit items in sequential tracks) follow the change they belong to.
//!
//! The result is turned back into ordinary ops with [`crate::sync_ops`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use captaine_model::*;
use captaine_ops::Op;
use serde_json::Value;

use crate::diff::{changed_paths, diff, index_items, Change, ChangeKind, Target, What};
use crate::sync_ops;

/// What to do with one of their changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Bring their change in (for a conflict: theirs wins)
    Take,
    /// Leave it out (for a conflict: ours wins)
    Skip,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Clean,
    Conflict(String),
}

#[derive(Clone, Debug)]
pub struct PlanEntry {
    /// Their change, from the common ancestor
    pub change: Change,
    pub status: Status,
    /// What our side did to the same thing, for conflicts
    pub ours: Option<Change>,
}

#[derive(Clone, Debug)]
pub struct MergePlan {
    pub entries: Vec<PlanEntry>,
    /// Gap and ripple bookkeeping that follows the entries
    pub hidden: usize,
}

impl MergePlan {
    pub fn conflicts(&self) -> impl Iterator<Item = &PlanEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.status, Status::Conflict(_)))
    }
}

#[derive(Debug)]
pub struct MergeOutcome {
    pub project: Project,
    /// Ops that turn *our* state into `project`: the content of the merge commit
    pub ops: Vec<Op>,
    pub applied: Vec<String>,
    /// Their changes that could not be brought in, with the reason
    pub skipped: Vec<(String, String)>,
}

fn value_at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, k| cur.get(k))
}

fn set_at(v: &mut Value, path: &str, new: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = v;
    for (i, k) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            if let Some(m) = cur.as_object_mut() {
                m.insert((*k).to_string(), new);
            }
            return;
        }
        let Some(next) = cur.get_mut(*k) else { return };
        cur = next;
    }
}

fn json_of(item: &Item) -> Value {
    serde_json::to_value(item).expect("items serialize")
}

fn target_json(p: &Project, t: Target) -> Option<Value> {
    match t {
        Target::Item(id) => index_items(p).get(&id).map(|w| json_of(&w.item)),
        Target::Media(id) => p.media.get(&id).and_then(|m| serde_json::to_value(m).ok()),
        Target::Track(id) => p
            .timelines
            .values()
            .flat_map(|t| t.tracks.iter())
            .find(|x| x.id == id)
            .map(|x| {
                let mut t = (**x).clone();
                t.items.clear();
                serde_json::to_value(t).expect("tracks serialize")
            }),
        Target::Timeline(id) => p
            .timelines
            .get(&id)
            .map(|t| serde_json::json!({"name": t.name, "rate": t.rate, "markers": t.markers})),
    }
}

/// Is applying `theirs` on top of `ours` a conflict, and why?
fn conflict_of(
    c: &Change,
    o: Option<&Change>,
    base: &Project,
    ours: &Project,
    theirs: &Project,
) -> Status {
    // structural: a track one side removed while the other put something in it
    if let (Target::Item(id), ChangeKind::Added) = (c.target, c.kind) {
        if let Some(track) = c.track {
            let ours_has_track = ours
                .timelines
                .values()
                .any(|t| t.tracks.iter().any(|x| x.id == track));
            let base_has = base
                .timelines
                .values()
                .any(|t| t.tracks.iter().any(|x| x.id == track));
            if base_has && !ours_has_track {
                return Status::Conflict(format!(
                    "their new item '{}' goes on a track that was removed on our side",
                    c.name
                ));
            }
        }
        let _ = id;
    }
    let Some(o) = o else {
        if let (Target::Track(tid), ChangeKind::Removed) = (c.target, c.kind) {
            // they removed a track we may have put things on
            let placed: Vec<String> = index_items(ours)
                .values()
                .filter(|w| w.track == tid && !index_items(base).contains_key(&w.item.id()))
                .map(|w| w.item.id().to_string())
                .collect();
            if !placed.is_empty() {
                return Status::Conflict(format!(
                    "track {} was removed, but our side added {} item(s) to it",
                    c.name,
                    placed.len()
                ));
            }
        }
        return Status::Clean;
    };
    use ChangeKind::*;
    match (c.kind, o.kind) {
        (Removed, Removed) => Status::Clean,
        (Removed, _) => {
            Status::Conflict(format!("they deleted '{}', which our side changed", c.name))
        }
        (_, Removed) => {
            Status::Conflict(format!("they changed '{}', which our side deleted", c.name))
        }
        (Added, Added) => {
            if target_json(theirs, c.target) == target_json(ours, c.target) {
                Status::Clean
            } else {
                Status::Conflict(format!("'{}' was added differently on both sides", c.name))
            }
        }
        _ => {
            let (vt, vo) = (target_json(theirs, c.target), target_json(ours, c.target));
            let (vt, vo) = match (vt, vo) {
                (Some(a), Some(b)) => (a, b),
                _ => return Status::Clean,
            };
            let clash: Vec<&String> = c
                .fields
                .iter()
                .filter(|f| o.fields.contains(f))
                .filter(|f| value_at(&vt, f) != value_at(&vo, f))
                .collect();
            if !clash.is_empty() {
                return Status::Conflict(format!(
                    "both sides changed {} of '{}' differently",
                    clash
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    c.name
                ));
            }
            if c.kind == Moved && o.kind == Moved {
                let (wt, wo) = (index_items(theirs), index_items(ours));
                if let Target::Item(id) = c.target {
                    if let (Some(a), Some(b)) = (wt.get(&id), wo.get(&id)) {
                        if a.track != b.track || a.index != b.index {
                            return Status::Conflict(format!(
                                "both sides moved '{}', to different places",
                                c.name
                            ));
                        }
                    }
                }
            }
            Status::Clean
        }
    }
}

/// List their changes since the common ancestor and which of them clash with ours.
pub fn plan_merge(base: &Project, ours: &Project, theirs: &Project) -> MergePlan {
    let (ct, co) = (diff(base, theirs), diff(base, ours));
    let ours_by: HashMap<&str, &Change> = co
        .iter()
        .filter(|c| c.kind != ChangeKind::Shifted)
        .map(|c| (c.id.as_str(), c))
        .collect();
    let mut hidden = 0;
    let mut entries = Vec::new();
    for c in &ct {
        if c.is_noise() {
            hidden += 1;
            continue;
        }
        let o = ours_by.get(c.id.as_str()).copied();
        let status = conflict_of(c, o, base, ours, theirs);
        entries.push(PlanEntry {
            change: c.clone(),
            status,
            ours: o.cloned(),
        });
    }
    MergePlan { entries, hidden }
}

// ---- applying -----------------------------------------------------------------------------------------------

fn track_mut(p: &mut Project, id: Id) -> Option<&mut Track> {
    for tl in p.timelines.values_mut() {
        let tl = Arc::make_mut(tl);
        if let Some(i) = tl.tracks.iter().position(|t| t.id == id) {
            return Some(tl.track_mut(i));
        }
    }
    None
}

fn remove_item(p: &mut Project, id: Id) -> Option<Item> {
    let (tl_id, ti, ii) = p.timelines.values().find_map(|tl| {
        tl.tracks.iter().enumerate().find_map(|(ti, t)| {
            t.items
                .iter()
                .position(|i| i.id() == id)
                .map(|ii| (tl.id, ti, ii))
        })
    })?;
    Some(p.timeline_mut(tl_id)?.track_mut(ti).items.remove(ii))
}

fn find_item_mut(p: &mut Project, id: Id) -> Option<&mut Item> {
    let (tl_id, ti, ii) = p.timelines.values().find_map(|tl| {
        tl.tracks.iter().enumerate().find_map(|(ti, t)| {
            t.items
                .iter()
                .position(|i| i.id() == id)
                .map(|ii| (tl.id, ti, ii))
        })
    })?;
    Some(&mut p.timeline_mut(tl_id)?.track_mut(ti).items[ii])
}

/// Put their `item` into our track `track`, right after the nearest earlier neighbour (in their list) that we have.
fn insert_near(m: &mut Project, theirs: &Project, track: Id, item: Item) -> Result<(), String> {
    let their_track = theirs
        .timelines
        .values()
        .flat_map(|t| t.tracks.iter())
        .find(|t| t.id == track)
        .ok_or("their track is missing")?;
    let pos = their_track
        .items
        .iter()
        .position(|i| i.id() == item.id())
        .ok_or("their item is missing")?;
    let preds: Vec<Id> = their_track.items[..pos]
        .iter()
        .rev()
        .map(|i| i.id())
        .collect();
    let t =
        track_mut(m, track).ok_or_else(|| "the track does not exist on our side".to_string())?;
    let at = preds
        .iter()
        .find_map(|p| t.items.iter().position(|i| i.id() == *p).map(|i| i + 1))
        .unwrap_or(0);
    t.items.insert(at, item);
    Ok(())
}

fn apply_fields(current: &Item, theirs: &Item, fields: &[String]) -> Result<Item, String> {
    let mut v = json_of(current);
    let tv = json_of(theirs);
    for f in fields {
        if let Some(x) = value_at(&tv, f) {
            set_at(&mut v, f, x.clone());
        }
    }
    serde_json::from_value(v).map_err(|e| format!("merged item is not valid: {e}"))
}

/// Merge `theirs` into `ours` (both descended from `base`). `choices` overrides the defaults:
/// clean changes are taken, conflicting ones are skipped (ours wins).
pub fn merge(
    base: &Project,
    ours: &Project,
    theirs: &Project,
    choices: &HashMap<String, Choice>,
) -> Result<MergeOutcome, String> {
    let plan = plan_merge(base, ours, theirs);
    let all = diff(base, theirs);
    let mut taken: HashSet<String> = HashSet::new();
    let status: HashMap<&str, &Status> = plan
        .entries
        .iter()
        .map(|e| (e.change.id.as_str(), &e.status))
        .collect();
    for e in &plan.entries {
        let default = if e.status == Status::Clean {
            Choice::Take
        } else {
            Choice::Skip
        };
        if choices.get(&e.change.id).copied().unwrap_or(default) == Choice::Take {
            taken.insert(e.change.id.clone());
        }
    }

    let mut m = ours.clone();
    let mut applied = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    let (wb, wt) = (index_items(base), index_items(theirs));

    // 1. media
    for c in all
        .iter()
        .filter(|c| c.what == What::Media && taken.contains(&c.id))
    {
        let Target::Media(id) = c.target else {
            continue;
        };
        match c.kind {
            ChangeKind::Added => {
                if let Some(x) = theirs.media.get(&id) {
                    m.media.insert(id, x.clone());
                }
            }
            ChangeKind::Removed => {
                if index_items(&m)
                    .values()
                    .any(|w| w.item.as_clip().is_some_and(|k| k.media == id))
                {
                    skipped.push((c.summary.clone(), "it is still used by a clip".into()));
                    continue;
                }
                m.media.remove(&id);
            }
            _ => {
                if let (Some(cur), Some(th)) = (m.media.get(&id).cloned(), theirs.media.get(&id)) {
                    let mut v = serde_json::to_value(&cur).expect("media serialize");
                    let tv = serde_json::to_value(th).expect("media serialize");
                    for f in &c.fields {
                        if let Some(x) = value_at(&tv, f) {
                            set_at(&mut v, f, x.clone());
                        }
                    }
                    if let Ok(x) = serde_json::from_value(v) {
                        m.media.insert(id, x);
                    }
                }
            }
        }
        applied.push(c.summary.clone());
    }
    // 2. timelines and tracks
    for c in all
        .iter()
        .filter(|c| matches!(c.what, What::Timeline | What::Track) && taken.contains(&c.id))
    {
        match (c.target, c.kind) {
            (Target::Timeline(id), ChangeKind::Added) => {
                if let Some(t) = theirs.timelines.get(&id) {
                    // the tracks arrive empty; their items come in as item changes
                    let mut t2 = (**t).clone();
                    for tr in &mut t2.tracks {
                        Arc::make_mut(tr).items.clear();
                    }
                    m.timelines.insert(id, Arc::new(t2));
                }
            }
            (Target::Timeline(id), ChangeKind::Removed) => {
                m.timelines.remove(&id);
            }
            (Target::Timeline(id), _) => {
                if let (Some(tt), Some(mt)) = (theirs.timelines.get(&id), m.timeline_mut(id)) {
                    if c.fields.iter().any(|f| f == "name") {
                        mt.name = tt.name.clone();
                    }
                    if c.fields.iter().any(|f| f == "markers") {
                        mt.markers = tt.markers.clone();
                    }
                }
            }
            (Target::Track(id), ChangeKind::Added) => {
                if let Some((tl, idx, t)) = theirs.timelines.values().find_map(|tl| {
                    tl.tracks
                        .iter()
                        .position(|x| x.id == id)
                        .map(|i| (tl.id, i, tl.tracks[i].clone()))
                }) {
                    if let Some(mt) = m.timeline_mut(tl) {
                        let mut empty = (*t).clone();
                        empty.items.clear();
                        mt.tracks.insert(idx.min(mt.tracks.len()), Arc::new(empty));
                    }
                }
            }
            (Target::Track(id), ChangeKind::Removed) => {
                for tl in m.timelines.values_mut() {
                    Arc::make_mut(tl).tracks.retain(|t| t.id != id);
                }
            }
            (Target::Track(id), _) => {
                let tt = theirs
                    .timelines
                    .values()
                    .flat_map(|t| t.tracks.iter())
                    .find(|t| t.id == id)
                    .cloned();
                if let (Some(tt), Some(t)) = (tt, track_mut(&mut m, id)) {
                    if c.fields.iter().any(|f| f == "enabled") {
                        t.enabled = tt.enabled;
                    }
                    if c.fields.iter().any(|f| f == "gain_db") {
                        t.gain_db = tt.gain_db;
                    }
                    if c.fields.iter().any(|f| f == "name") {
                        t.name = tt.name.clone();
                    }
                }
                if c.fields.iter().any(|f| f == "order") {
                    if let Some((tl, idx)) = theirs.timelines.values().find_map(|tl| {
                        tl.tracks
                            .iter()
                            .position(|x| x.id == id)
                            .map(|i| (tl.id, i))
                    }) {
                        if let Some(mt) = m.timeline_mut(tl) {
                            if let Some(cur) = mt.tracks.iter().position(|x| x.id == id) {
                                let t = mt.tracks.remove(cur);
                                mt.tracks.insert(idx.min(mt.tracks.len()), t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        applied.push(c.summary.clone());
    }
    // 3. items (not gaps), removals first so placements see the final neighbours
    let item_changes: Vec<&Change> = all
        .iter()
        .filter(|c| {
            matches!(c.target, Target::Item(_))
                && c.what != What::Gap
                && c.kind != ChangeKind::Shifted
        })
        .collect();
    let mut taken_ranges: HashMap<Id, Vec<(i64, i64)>> = HashMap::new();
    let mut any_on_track: HashSet<Id> = HashSet::new();
    for c in &item_changes {
        if taken.contains(&c.id) {
            if let (Some(t), Some(r)) = (c.track, c.range) {
                taken_ranges.entry(t).or_default().push(r);
                any_on_track.insert(t);
            }
        }
    }
    for c in item_changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Removed && taken.contains(&c.id))
    {
        let Target::Item(id) = c.target else { continue };
        remove_item(&mut m, id);
        applied.push(c.summary.clone());
    }
    for c in item_changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Removed && taken.contains(&c.id))
    {
        let Target::Item(id) = c.target else { continue };
        let Some(w) = wt.get(&id) else { continue };
        let result: Result<(), String> = match c.kind {
            ChangeKind::Added => insert_near(&mut m, theirs, w.track, w.item.clone()),
            ChangeKind::Moved => match remove_item(&mut m, id) {
                Some(cur) => apply_fields(&cur, &w.item, &c.fields)
                    .and_then(|merged| insert_near(&mut m, theirs, w.track, merged)),
                None => Err("it no longer exists on our side".into()),
            },
            _ => match find_item_mut(&mut m, id) {
                Some(slot) => apply_fields(slot, &w.item, &c.fields).map(|merged| *slot = merged),
                None => Err("it no longer exists on our side".into()),
            },
        };
        match result {
            Ok(()) => applied.push(c.summary.clone()),
            Err(e) => skipped.push((c.summary.clone(), e)),
        }
    }
    // 4. gaps follow the changes they sit next to
    for c in all.iter().filter(|c| {
        c.what == What::Gap && matches!(c.target, Target::Item(_)) && c.kind != ChangeKind::Shifted
    }) {
        let Target::Item(id) = c.target else { continue };
        let Some(track) = c.track else { continue };
        let touches = match (taken_ranges.get(&track), c.range) {
            (Some(rs), Some((s, e))) => rs.iter().any(|(a, b)| *a <= e && s <= *b),
            _ => false,
        };
        // a gap on a track where no selectable change exists is pure layout: it follows whenever anything is merged
        let orphan = !item_changes.iter().any(|x| x.track == Some(track));
        if !(touches || any_on_track.contains(&track) || (orphan && !taken.is_empty())) {
            continue;
        }
        match c.kind {
            ChangeKind::Removed => {
                remove_item(&mut m, id);
            }
            ChangeKind::Added => {
                if let Some(w) = wt.get(&id) {
                    if let Err(e) = insert_near(&mut m, theirs, w.track, w.item.clone()) {
                        skipped.push((c.summary.clone(), e));
                    }
                }
            }
            _ => {
                if let (Some(w), Some(slot)) = (wt.get(&id), find_item_mut(&mut m, id)) {
                    if let Ok(merged) = apply_fields(slot, &w.item, &c.fields) {
                        *slot = merged;
                    }
                }
            }
        }
    }
    let _ = (&wb, &status);

    // 5. the result must be a sound project, and expressible as ops
    m.validate()
        .map_err(|e| format!("the merged project would be invalid: {}", e.join("; ")))?;
    let ops = sync_ops(ours, &m)?;
    let _ = changed_paths;
    Ok(MergeOutcome {
        project: m,
        ops,
        applied,
        skipped,
    })
}
