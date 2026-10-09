//! What differs between two states of the project, matched by stable ids (docs/07).

use std::collections::{BTreeMap, HashMap, HashSet};

use captaine_model::*;
use captaine_time::Rational;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Item(Id),
    Track(Id),
    Media(Id),
    Timeline(Id),
}

impl Target {
    fn key(&self) -> String {
        match self {
            Target::Item(i) => format!("item:{}", i.to_full_string()),
            Target::Track(i) => format!("track:{}", i.to_full_string()),
            Target::Media(i) => format!("media:{}", i.to_full_string()),
            Target::Timeline(i) => format!("timeline:{}", i.to_full_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Added,
    Removed,
    /// Another track, or another place in the order of its track
    Moved,
    /// Source in/out or length changed (possibly with other properties too)
    Trimmed,
    /// Properties changed (opacity, colour, volume, title text, a track's mute...)
    Modified,
    /// Only its start time differs: pushed along by an edit before it (ripple). Not a change in itself.
    Shifted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum What {
    Clip,
    Title,
    Gap,
    Track,
    Media,
    Timeline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// Stable key, e.g. `item:<id>`: the same object gives the same key in every diff
    pub id: String,
    pub kind: ChangeKind,
    pub what: What,
    pub target: Target,
    pub name: String,
    /// The properties that differ, as dotted paths (`video.opacity`, `duration.value`, `name`)
    pub fields: Vec<String>,
    /// Where it is in time: ticks `(start, end)` on the second state's timeline, or the first's for removals
    pub range: Option<(i64, i64)>,
    /// Length in ticks (of the added or removed thing, or its new length)
    pub duration: i64,
    pub track: Option<Id>,
    /// One readable line
    pub summary: String,
}

impl Change {
    /// Gaps and ripple shifts are bookkeeping, not edits a person would list.
    pub fn is_noise(&self) -> bool {
        self.what == What::Gap || self.kind == ChangeKind::Shifted
    }
}

/// Flatten JSON into `path -> leaf` (objects recurse; arrays and scalars are leaves).
pub(crate) fn flatten(v: &Value) -> BTreeMap<String, Value> {
    fn go(v: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    go(x, &p, out);
                }
            }
            other => {
                out.insert(prefix.to_string(), other.clone());
            }
        }
    }
    let mut out = BTreeMap::new();
    go(v, "", &mut out);
    out
}

pub(crate) fn changed_paths(a: &Value, b: &Value) -> Vec<String> {
    let (fa, fb) = (flatten(a), flatten(b));
    let mut keys: Vec<&String> = fa
        .keys()
        .chain(fb.keys())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    keys.sort();
    keys.into_iter()
        .filter(|k| fa.get(*k) != fb.get(*k))
        .cloned()
        .collect()
}

pub(crate) struct Where {
    pub timeline: Id,
    pub track: Id,
    pub index: usize,
    pub start: i64,
    pub item: Item,
}

/// Every item of every timeline with its place.
pub(crate) fn index_items(p: &Project) -> HashMap<Id, Where> {
    let mut out = HashMap::new();
    for tl in p.timelines.values() {
        for t in &tl.tracks {
            for (i, (it, st)) in t.items.iter().zip(t.item_starts()).enumerate() {
                out.insert(
                    it.id(),
                    Where {
                        timeline: tl.id,
                        track: t.id,
                        index: i,
                        start: st,
                        item: it.clone(),
                    },
                );
            }
        }
    }
    out
}

fn what_of(i: &Item) -> What {
    match i {
        Item::Clip(_) => What::Clip,
        Item::Title(_) => What::Title,
        Item::Gap(_) => What::Gap,
    }
}

fn name_of(i: &Item) -> String {
    match i {
        Item::Clip(c) => c.name.clone(),
        Item::Title(t) => t.name.clone(),
        Item::Gap(_) => "gap".into(),
    }
}

fn secs(ticks: i64, rate: Rational) -> String {
    format!("{:.1} s", ticks as f64 / rate.as_f64())
}

/// Indices (into `order`) that form the longest increasing run: the items that kept their relative order.
fn longest_increasing(order: &[usize]) -> HashSet<usize> {
    // patience sorting with back-pointers
    let mut tails: Vec<usize> = Vec::new(); // index into `order` of the smallest tail of each length
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

fn pretty(v: Option<&Value>) -> String {
    match v {
        None => "—".into(),
        Some(Value::Number(n)) => n.as_f64().map_or(n.to_string(), |x| {
            if x.fract() == 0.0 {
                format!("{x:.0}")
            } else {
                format!("{x:.2}")
            }
        }),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// Differences going from `a` to `b`.
pub fn diff(a: &Project, b: &Project) -> Vec<Change> {
    let mut out = Vec::new();
    let rate = b
        .timelines
        .values()
        .next()
        .or(a.timelines.values().next())
        .map_or(Rational::FPS_30, |t| t.rate);

    // ---- media ----------------------------------------------------------------------------------
    for id in a
        .media
        .keys()
        .chain(b.media.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let (ma, mb) = (a.media.get(id), b.media.get(id));
        let (kind, name, fields) = match (ma, mb) {
            (Some(m), None) => (ChangeKind::Removed, m.name.clone(), vec![]),
            (None, Some(m)) => (ChangeKind::Added, m.name.clone(), vec![]),
            (Some(x), Some(y)) => {
                let f = changed_paths(
                    &serde_json::to_value(x).unwrap(),
                    &serde_json::to_value(y).unwrap(),
                );
                if f.is_empty() {
                    continue;
                }
                (ChangeKind::Modified, y.name.clone(), f)
            }
            (None, None) => continue,
        };
        let summary = match kind {
            ChangeKind::Added => format!("Imported media '{name}'"),
            ChangeKind::Removed => format!("Removed media '{name}'"),
            _ => format!("Changed media '{name}': {}", fields.join(", ")),
        };
        out.push(Change {
            id: Target::Media(*id).key(),
            kind,
            what: What::Media,
            target: Target::Media(*id),
            name,
            fields,
            range: None,
            duration: 0,
            track: None,
            summary,
        });
    }

    // ---- timelines and tracks ---------------------------------------------------------------------
    for id in a
        .timelines
        .keys()
        .chain(b.timelines.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        match (a.timelines.get(id), b.timelines.get(id)) {
            (Some(t), None) => out.push(simple(
                Target::Timeline(*id),
                What::Timeline,
                ChangeKind::Removed,
                &t.name,
                format!("Removed timeline '{}'", t.name),
            )),
            (None, Some(t)) => out.push(simple(
                Target::Timeline(*id),
                What::Timeline,
                ChangeKind::Added,
                &t.name,
                format!("Added timeline '{}'", t.name),
            )),
            (Some(ta), Some(tb)) => {
                let mut f: Vec<String> = Vec::new();
                if ta.rate != tb.rate {
                    f.push("rate".into());
                }
                if ta.name != tb.name {
                    f.push("name".into());
                }
                if ta.markers != tb.markers {
                    f.push("markers".into());
                }
                if !f.is_empty() {
                    let mut c = simple(
                        Target::Timeline(*id),
                        What::Timeline,
                        ChangeKind::Modified,
                        &tb.name,
                        format!("Timeline '{}' changed ({})", tb.name, f.join(", ")),
                    );
                    c.fields = f;
                    out.push(c);
                }
                diff_tracks(ta, tb, &mut out);
            }
            (None, None) => {}
        }
    }

    // ---- items --------------------------------------------------------------------------------------
    let (ia, ib) = (index_items(a), index_items(b));
    // which common items kept their order within the same track (the rest were moved)
    let mut reordered: HashSet<Id> = HashSet::new();
    let track_ids: HashSet<Id> = ib.values().map(|w| w.track).collect();
    for track in track_ids {
        let in_b: Vec<(&Id, &Where)> = {
            let mut v: Vec<_> = ib.iter().filter(|(_, w)| w.track == track).collect();
            v.sort_by_key(|(_, w)| w.index);
            v
        };
        // common items that were on this same track in `a`: their order in `a`
        let common: Vec<(&Id, usize)> = in_b
            .iter()
            .filter_map(|(id, _)| {
                ia.get(*id)
                    .filter(|w| w.track == track)
                    .map(|w| (*id, w.index))
            })
            .collect();
        let order: Vec<usize> = common.iter().map(|(_, ai)| *ai).collect();
        let keep = longest_increasing(&order);
        for (i, (id, _)) in common.iter().enumerate() {
            if !keep.contains(&i) {
                reordered.insert(**id);
            }
        }
    }
    let mut ids: Vec<&Id> = ia
        .keys()
        .chain(ib.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    ids.sort_by_key(|id| {
        ib.get(id)
            .or(ia.get(id))
            .map(|w| (w.timeline, w.track, w.index))
    });
    for id in ids {
        let target = Target::Item(*id);
        match (ia.get(id), ib.get(id)) {
            (Some(x), None) => {
                let d = x.item.duration().value;
                let name = name_of(&x.item);
                out.push(Change {
                    id: target.key(),
                    kind: ChangeKind::Removed,
                    what: what_of(&x.item),
                    target,
                    name: name.clone(),
                    fields: vec![],
                    range: Some((x.start, x.start + d)),
                    duration: d,
                    track: Some(x.track),
                    summary: format!("Removed {} '{name}' ({})", noun(&x.item), secs(d, rate)),
                });
            }
            (None, Some(y)) => {
                let d = y.item.duration().value;
                let name = name_of(&y.item);
                out.push(Change {
                    id: target.key(),
                    kind: ChangeKind::Added,
                    what: what_of(&y.item),
                    target,
                    name: name.clone(),
                    fields: vec![],
                    range: Some((y.start, y.start + d)),
                    duration: d,
                    track: Some(y.track),
                    summary: format!("Added {} '{name}' ({})", noun(&y.item), secs(d, rate)),
                });
            }
            (Some(x), Some(y)) => {
                let (va, vb) = (
                    serde_json::to_value(&x.item).unwrap(),
                    serde_json::to_value(&y.item).unwrap(),
                );
                let fields = changed_paths(&va, &vb);
                let moved = x.track != y.track || reordered.contains(id);
                let d = y.item.duration().value;
                let kind = if moved {
                    ChangeKind::Moved
                } else if fields
                    .iter()
                    .any(|f| f.starts_with("duration") || f.starts_with("source_start"))
                {
                    ChangeKind::Trimmed
                } else if !fields.is_empty() {
                    ChangeKind::Modified
                } else if x.start != y.start {
                    ChangeKind::Shifted
                } else {
                    continue;
                };
                let name = name_of(&y.item);
                let flat = (flatten(&va), flatten(&vb));
                let detail: Vec<String> = fields
                    .iter()
                    .filter(|f| !f.ends_with(".rate"))
                    .take(4)
                    .map(|f| {
                        let label = f.trim_end_matches(".value");
                        let conv = |v: Option<&Value>| {
                            if f.starts_with("duration") || f.starts_with("source_start") {
                                v.and_then(Value::as_f64)
                                    .map_or("—".into(), |t| secs(t as i64, rate))
                            } else {
                                pretty(v)
                            }
                        };
                        format!("{label} {} → {}", conv(flat.0.get(f)), conv(flat.1.get(f)))
                    })
                    .collect();
                let summary = match kind {
                    ChangeKind::Moved => format!(
                        "Moved {} '{name}' to {}",
                        noun(&y.item),
                        secs(y.start, rate)
                    ),
                    ChangeKind::Trimmed => format!("Trimmed '{name}': {}", detail.join(", ")),
                    ChangeKind::Modified => format!("Changed '{name}': {}", detail.join(", ")),
                    _ => format!("'{name}' moves by {}", secs(y.start - x.start, rate)),
                };
                out.push(Change {
                    id: target.key(),
                    kind,
                    what: what_of(&y.item),
                    target,
                    name,
                    fields,
                    range: Some((y.start, y.start + d)),
                    duration: d,
                    track: Some(y.track),
                    summary,
                });
            }
            (None, None) => {}
        }
    }
    out
}

fn noun(i: &Item) -> &'static str {
    match i {
        Item::Clip(_) => "clip",
        Item::Title(_) => "title",
        Item::Gap(_) => "gap",
    }
}

fn simple(target: Target, what: What, kind: ChangeKind, name: &str, summary: String) -> Change {
    Change {
        id: target.key(),
        kind,
        what,
        target,
        name: name.to_string(),
        fields: vec![],
        range: None,
        duration: 0,
        track: None,
        summary,
    }
}

type TrackIndex<'a> = HashMap<Id, (usize, &'a Track)>;

fn diff_tracks<'a>(ta: &'a Timeline, tb: &'a Timeline, out: &mut Vec<Change>) {
    let index = |t: &'a Timeline| -> TrackIndex<'a> {
        t.tracks
            .iter()
            .enumerate()
            .map(|(i, x)| (x.id, (i, x.as_ref())))
            .collect()
    };
    let (ma, mb) = (index(ta), index(tb));
    let mut ids: Vec<&Id> = ma
        .keys()
        .chain(mb.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    ids.sort_by_key(|id| mb.get(id).or(ma.get(id)).map(|(i, _)| *i));
    for id in ids {
        match (ma.get(id), mb.get(id)) {
            (Some((_, t)), None) => out.push(simple(
                Target::Track(*id),
                What::Track,
                ChangeKind::Removed,
                &t.name,
                format!("Removed track {}", t.name),
            )),
            (None, Some((_, t))) => out.push(simple(
                Target::Track(*id),
                What::Track,
                ChangeKind::Added,
                &t.name,
                format!("Added track {}", t.name),
            )),
            (Some((ia, x)), Some((ib, y))) => {
                let mut f = Vec::new();
                if x.name != y.name {
                    f.push("name".to_string());
                }
                if x.enabled != y.enabled {
                    f.push("enabled".to_string());
                }
                if x.gain_db != y.gain_db {
                    f.push("gain_db".to_string());
                }
                if ia != ib {
                    f.push("order".to_string());
                }
                if !f.is_empty() {
                    let what = if f.iter().any(|x| x == "enabled") {
                        if y.enabled {
                            "shown"
                        } else {
                            "hidden or muted"
                        }
                    } else {
                        "changed"
                    };
                    let mut c = simple(
                        Target::Track(*id),
                        What::Track,
                        ChangeKind::Modified,
                        &y.name,
                        format!("Track {} {what} ({})", y.name, f.join(", ")),
                    );
                    c.fields = f;
                    out.push(c);
                }
            }
            (None, None) => {}
        }
    }
}

/// A sentence for a set of changes: "removes 3 clips (12.4 s), adds a title, trims 2 clips".
pub fn summarize(changes: &[Change], rate: Rational) -> String {
    let real: Vec<&Change> = changes.iter().filter(|c| !c.is_noise()).collect();
    if real.is_empty() {
        return "no differences".into();
    }
    let count = |what: &[What], kind: ChangeKind| {
        real.iter()
            .filter(|c| what.contains(&c.what) && c.kind == kind)
            .count()
    };
    let secs_of = |what: &[What], kind: ChangeKind| {
        real.iter()
            .filter(|c| what.contains(&c.what) && c.kind == kind)
            .map(|c| c.duration)
            .sum::<i64>()
    };
    let mut parts = Vec::new();
    let plural = |n: usize, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    let visual = [What::Clip, What::Title];
    let n = count(&visual, ChangeKind::Removed);
    if n > 0 {
        parts.push(format!(
            "removes {} ({})",
            plural(n, "clip or title"),
            secs(secs_of(&visual, ChangeKind::Removed), rate)
        ));
    }
    let n = count(&[What::Clip], ChangeKind::Added);
    if n > 0 {
        parts.push(format!(
            "adds {} ({})",
            plural(n, "clip"),
            secs(secs_of(&[What::Clip], ChangeKind::Added), rate)
        ));
    }
    let n = count(&[What::Title], ChangeKind::Added);
    if n > 0 {
        parts.push(format!("adds {}", plural(n, "title")));
    }
    let n = count(&visual, ChangeKind::Trimmed);
    if n > 0 {
        parts.push(format!("trims {}", plural(n, "item")));
    }
    let n = count(&visual, ChangeKind::Moved);
    if n > 0 {
        parts.push(format!("moves {}", plural(n, "item")));
    }
    let n = count(&visual, ChangeKind::Modified);
    if n > 0 {
        parts.push(format!("changes the properties of {}", plural(n, "item")));
    }
    let tracks = |kind: ChangeKind| {
        real.iter()
            .filter(|c| c.what == What::Track && c.kind == kind)
            .count()
    };
    let timelines = |kind: ChangeKind| {
        real.iter()
            .filter(|c| c.what == What::Timeline && c.kind == kind)
            .count()
    };
    if timelines(ChangeKind::Added) > 0 {
        parts.push(format!(
            "adds {}",
            plural(timelines(ChangeKind::Added), "timeline")
        ));
    }
    if timelines(ChangeKind::Removed) > 0 {
        parts.push(format!(
            "removes {}",
            plural(timelines(ChangeKind::Removed), "timeline")
        ));
    }
    if tracks(ChangeKind::Added) > 0 {
        parts.push(format!(
            "adds {}",
            plural(tracks(ChangeKind::Added), "track")
        ));
    }
    if tracks(ChangeKind::Removed) > 0 {
        parts.push(format!(
            "removes {}",
            plural(tracks(ChangeKind::Removed), "track")
        ));
    }
    let n = tracks(ChangeKind::Modified) + timelines(ChangeKind::Modified);
    if n > 0 {
        parts.push(format!(
            "changes {}",
            plural(n, "track or timeline setting")
        ));
    }
    let media = |kind: ChangeKind| {
        real.iter()
            .filter(|c| c.what == What::Media && c.kind == kind)
            .count()
    };
    if media(ChangeKind::Added) > 0 {
        parts.push(format!(
            "imports {}",
            plural(media(ChangeKind::Added), "media file")
        ));
    }
    if media(ChangeKind::Removed) > 0 {
        parts.push(format!(
            "removes {} from the media pool",
            plural(media(ChangeKind::Removed), "media file")
        ));
    }
    if media(ChangeKind::Modified) > 0 {
        parts.push(format!(
            "changes {} in the media pool",
            plural(media(ChangeKind::Modified), "item")
        ));
    }
    parts.join(", ")
}
