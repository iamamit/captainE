//! The ordinary ops that turn one project state into another.
//!
//! A merge computes its result as a project; this turns that result back into ops so the merge commit reads and
//! undoes like any other. It is checked by applying the ops: if they do not reproduce the target exactly the
//! caller gets an error instead of a wrong project.

use std::collections::{HashMap, HashSet};

use captaine_model::*;
use captaine_ops::{apply, Op};

/// Positions `i` (into `order`) that form the longest increasing run.
fn keep_in_order(order: &[usize]) -> HashSet<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut prev: Vec<Option<usize>> = vec![None; order.len()];
    for i in 0..order.len() {
        let pos = tails.partition_point(|&t| order[t] < order[i]);
        if pos > 0 {
            prev[i] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(i);
        } else {
            tails[pos] = i;
        }
    }
    let mut keep = HashSet::new();
    let mut cur = tails.last().copied();
    while let Some(i) = cur {
        keep.insert(i);
        cur = prev[i];
    }
    keep
}

pub fn sync_ops(from: &Project, to: &Project) -> Result<Vec<Op>, String> {
    if from.settings != to.settings {
        return Err("the two states have different project settings".into());
    }
    let mut ops: Vec<Op> = Vec::new();

    // media first: new items and proxies
    for (id, m) in &to.media {
        match from.media.get(id) {
            None => ops.push(Op::AddMedia { media: m.clone() }),
            Some(old) if old != m => {
                let same_but_proxy = MediaRef {
                    proxy: old.proxy.clone(),
                    ..m.clone()
                } == *old;
                if !same_but_proxy {
                    return Err(format!(
                        "media '{}' changed in a way that has no op",
                        m.name
                    ));
                }
                ops.push(Op::SetProxy {
                    media: *id,
                    proxy: m.proxy.clone(),
                });
            }
            _ => {}
        }
    }
    // new timelines whole
    for (id, tl) in &to.timelines {
        if !from.timelines.contains_key(id) {
            ops.push(Op::AddTimeline {
                timeline: (**tl).clone(),
            });
        }
    }
    let both: Vec<Id> = to
        .timelines
        .keys()
        .filter(|id| from.timelines.contains_key(id))
        .copied()
        .collect();
    for tid in &both {
        let (f, t) = (&from.timelines[tid], &to.timelines[tid]);
        if f.rate != t.rate {
            return Err(format!(
                "timeline '{}' was retimed, which has no op",
                t.name
            ));
        }
        if f.name != t.name {
            ops.push(Op::RenameTimeline {
                id: *tid,
                name: t.name.clone(),
            });
        }
    }

    // phase 1: take out every item that is gone or out of order, in every surviving track
    for tid in &both {
        let (f, t) = (&from.timelines[tid], &to.timelines[tid]);
        for ft in &f.tracks {
            let Some(tt) = t.tracks.iter().find(|x| x.id == ft.id) else {
                continue;
            };
            if ft.kind != tt.kind {
                return Err(format!("track '{}' changed kind", tt.name));
            }
            let target_pos: HashMap<Id, usize> = tt
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| (it.id(), i))
                .collect();
            // items of this track that also stay in it, in their current order, with their target positions
            let stay: Vec<(usize, usize)> = ft
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| target_pos.get(&it.id()).map(|p| (i, *p)))
                .collect();
            let order: Vec<usize> = stay.iter().map(|(_, p)| *p).collect();
            let keep = keep_in_order(&order);
            let kept: HashSet<usize> = stay
                .iter()
                .enumerate()
                .filter(|(k, _)| keep.contains(k))
                .map(|(_, (i, _))| *i)
                .collect();
            for (i, it) in ft.items.iter().enumerate() {
                if !kept.contains(&i) {
                    ops.push(Op::RemoveItem { item: it.id() });
                }
            }
        }
    }
    // tracks: remove, add, settings, order
    for tid in &both {
        let (f, t) = (&from.timelines[tid], &to.timelines[tid]);
        for ft in &f.tracks {
            if !t.tracks.iter().any(|x| x.id == ft.id) {
                ops.push(Op::RemoveTrack { track: ft.id });
            }
        }
        for (i, tt) in t.tracks.iter().enumerate() {
            if !f.tracks.iter().any(|x| x.id == tt.id) {
                ops.push(Op::AddTrack {
                    timeline: *tid,
                    index: i.min(f.tracks.len()),
                    track: Track {
                        items: vec![],
                        ..(**tt).clone()
                    },
                });
            }
        }
        for tt in &t.tracks {
            if let Some(ft) = f.tracks.iter().find(|x| x.id == tt.id) {
                if ft.name != tt.name {
                    return Err(format!("track '{}' was renamed, which has no op", ft.name));
                }
                if ft.enabled != tt.enabled || ft.gain_db != tt.gain_db {
                    ops.push(Op::SetTrack {
                        track: tt.id,
                        enabled: tt.enabled,
                        gain_db: tt.gain_db,
                    });
                }
            }
        }
        // order: surviving tracks, then new ones, as they stand now
        let mut cur: Vec<Id> = f
            .tracks
            .iter()
            .map(|x| x.id)
            .filter(|id| t.tracks.iter().any(|x| x.id == *id))
            .collect();
        for tt in &t.tracks {
            if !cur.contains(&tt.id) {
                let idx = t
                    .tracks
                    .iter()
                    .position(|x| x.id == tt.id)
                    .unwrap_or(cur.len())
                    .min(cur.len());
                cur.insert(idx, tt.id);
            }
        }
        for (i, tt) in t.tracks.iter().enumerate() {
            if cur[i] != tt.id {
                let j = cur.iter().position(|x| *x == tt.id).expect("track present");
                cur.remove(j);
                cur.insert(i, tt.id);
                ops.push(Op::MoveTrack {
                    track: tt.id,
                    index: i,
                });
            }
        }
    }
    // phase 6: put items in, in target order, and bring kept ones up to date
    for tid in &both {
        let (f, t) = (&from.timelines[tid], &to.timelines[tid]);
        for tt in &t.tracks {
            let Some(ft) = f.tracks.iter().find(|x| x.id == tt.id) else {
                // a brand new track: everything goes in
                for (i, it) in tt.items.iter().enumerate() {
                    ops.push(Op::InsertItem {
                        track: tt.id,
                        index: i,
                        item: it.clone(),
                    });
                }
                continue;
            };
            let old: HashMap<Id, &Item> = ft.items.iter().map(|it| (it.id(), it)).collect();
            let target_ids: HashSet<Id> = tt.items.iter().map(|it| it.id()).collect();
            // what phase 1 left in this track
            let target_pos: HashMap<Id, usize> = tt
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| (it.id(), i))
                .collect();
            let stay: Vec<(Id, usize)> = ft
                .items
                .iter()
                .filter_map(|it| target_pos.get(&it.id()).map(|p| (it.id(), *p)))
                .collect();
            let order: Vec<usize> = stay.iter().map(|(_, p)| *p).collect();
            let keep = keep_in_order(&order);
            let mut cur: Vec<Id> = stay
                .iter()
                .enumerate()
                .filter(|(k, _)| keep.contains(k))
                .map(|(_, (id, _))| *id)
                .collect();
            for (i, it) in tt.items.iter().enumerate() {
                if cur.get(i) == Some(&it.id()) {
                    if old.get(&it.id()).is_some_and(|o| **o != *it) {
                        ops.push(Op::ReplaceRaw {
                            item: it.id(),
                            with: it.clone(),
                        });
                    }
                } else {
                    ops.push(Op::InsertItem {
                        track: tt.id,
                        index: i,
                        item: it.clone(),
                    });
                    cur.insert(i, it.id());
                }
            }
            debug_assert!(cur.iter().all(|id| target_ids.contains(id)));
        }
    }
    for id in from.timelines.keys() {
        if !to.timelines.contains_key(id) {
            ops.push(Op::RemoveTimeline { id: *id });
        }
    }
    for id in from.media.keys() {
        if !to.media.contains_key(id) {
            ops.push(Op::RemoveMedia { id: *id });
        }
    }

    // prove it
    let mut p = from.clone();
    for op in &ops {
        p = apply(&p, op)
            .map_err(|e| format!("generated ops do not apply: {e}"))?
            .0;
    }
    if &p != to {
        return Err("generated ops do not reproduce the target state".into());
    }
    Ok(ops)
}
