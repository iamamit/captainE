//! Multi-selection, copy, paste, duplicate and delete across several items.

use captaine_model::{Id, Item, Project};
use captaine_ops::{apply, edit, Op, OpError};
use captaine_time::RationalTime;

use crate::app::App;

/// One copied item: where it sat relative to the earliest copied item, and on which track.
#[derive(Clone, Debug)]
pub struct Copied {
    pub item: Item,
    /// Index of its track in the timeline when copied
    pub track_index: usize,
    /// Ticks after the earliest copied start
    pub offset: i64,
}

impl App {
    /// Everything selected: the primary item first, then the extra ones.
    pub fn selection_ids(&self) -> Vec<Id> {
        let Some(primary) = self.selected_clip else {
            return Vec::new();
        };
        let mut v = vec![primary];
        v.extend(
            self.extra_selected
                .iter()
                .copied()
                .filter(|i| Some(*i) != self.selected_clip),
        );
        v
    }

    pub fn is_selected(&self, id: Id) -> bool {
        self.selected_clip == Some(id)
            || (self.selected_clip.is_some() && self.extra_selected.contains(&id))
    }

    /// Shift-click: add the item to the selection, or take it out.
    pub fn toggle_selected(&mut self, id: Id) {
        if self.selected_clip.is_none() {
            self.selected_clip = Some(id);
        } else if self.selected_clip == Some(id) {
            self.selected_clip = self.extra_selected.pop();
        } else if let Some(i) = self.extra_selected.iter().position(|x| *x == id) {
            self.extra_selected.remove(i);
        } else {
            self.extra_selected.push(id);
        }
    }

    /// Select exactly these items (the first becomes the primary one).
    pub fn select_only(&mut self, ids: &[Id]) {
        self.selected_clip = ids.first().copied();
        self.extra_selected = ids.iter().skip(1).copied().collect();
    }

    pub fn select_all(&mut self) {
        let ids: Vec<Id> = self
            .timeline()
            .map(|tl| {
                tl.tracks
                    .iter()
                    .flat_map(|t| t.items.iter())
                    .filter(|i| i.is_content())
                    .map(|i| i.id())
                    .collect()
            })
            .unwrap_or_default();
        self.select_only(&ids);
    }

    /// Forget selected items that no longer exist.
    pub fn prune_selection(&mut self) {
        let live: std::collections::HashSet<Id> = self
            .timeline()
            .map(|tl| {
                tl.tracks
                    .iter()
                    .flat_map(|t| t.items.iter())
                    .map(|i| i.id())
                    .collect()
            })
            .unwrap_or_default();
        let ids: Vec<Id> = self
            .selection_ids()
            .into_iter()
            .filter(|i| live.contains(i))
            .collect();
        self.select_only(&ids);
    }

    /// The selected items with track index and start tick, in timeline order.
    fn selection_entries(&self) -> Vec<(Item, usize, i64)> {
        let ids = self.selection_ids();
        let Some(tl) = self.timeline() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (ti, t) in tl.tracks.iter().enumerate() {
            for (it, st) in t.items.iter().zip(t.item_starts()) {
                if ids.contains(&it.id()) {
                    out.push((it.clone(), ti, st));
                }
            }
        }
        out.sort_by_key(|(_, ti, st)| (*st, *ti));
        out
    }

    pub fn copy_selection(&mut self) {
        let entries = self.selection_entries();
        let Some(base) = entries.iter().map(|e| e.2).min() else {
            self.toast("Select something to copy first", false);
            return;
        };
        self.clipboard = entries
            .into_iter()
            .map(|(item, track_index, st)| Copied {
                item,
                track_index,
                offset: st - base,
            })
            .collect();
        let n = self.clipboard.len();
        self.toast(
            format!("Copied {n} item{}", if n == 1 { "" } else { "s" }),
            false,
        );
    }

    /// Put the copied items down starting at `at`, each on the track it came from. Returns the new ids.
    pub fn paste_at(&mut self, at: i64) -> Vec<Id> {
        if self.clipboard.is_empty() {
            self.toast("Nothing copied yet", false);
            return Vec::new();
        }
        let Some(tl) = self.timeline().cloned() else {
            return Vec::new();
        };
        let mut p = self.project().clone();
        let mut ops = Vec::new();
        let mut made = Vec::new();
        let mut links: std::collections::HashMap<Id, Id> = std::collections::HashMap::new();
        for c in self.clipboard.clone() {
            let Some(track) = tl.tracks.get(c.track_index) else {
                continue;
            };
            let mut item = fresh(&c.item);
            relink(&mut item, &mut links);
            let id = item.id();
            let start = RationalTime::new(at + c.offset, tl.rate);
            match edit::overwrite_item(&p, track.id, start, item).and_then(|op| {
                let (next, _) = apply(&p, &op)?;
                p = next;
                Ok(op)
            }) {
                Ok(op) => {
                    ops.push(op);
                    made.push(id);
                }
                Err(e) => {
                    self.toast(e.to_string(), true);
                    return Vec::new();
                }
            }
        }
        if ops.is_empty() {
            self.toast("The copied tracks are not in this timeline", true);
            return Vec::new();
        }
        let description = format!(
            "Paste {} item{}",
            made.len(),
            if made.len() == 1 { "" } else { "s" }
        );
        if !self.do_op(Op::Transaction { description, ops }) {
            return Vec::new();
        }
        self.select_only(&made);
        made
    }

    /// Copy the selection and slot the copies in straight after it, pushing later items along. On each
    /// track the copies follow that track's last selected item.
    pub fn duplicate_selection(&mut self) {
        let ids = self.selection_ids();
        if ids.is_empty() {
            self.toast("Select something to duplicate first", false);
            return;
        }
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let mut ops = Vec::new();
        let mut made = Vec::new();
        let mut links: std::collections::HashMap<Id, Id> = std::collections::HashMap::new();
        for track in &tl.tracks {
            let picked: Vec<(usize, &Item)> = track
                .items
                .iter()
                .enumerate()
                .filter(|(_, it)| ids.contains(&it.id()))
                .collect();
            let Some(&(last, _)) = picked.last() else {
                continue;
            };
            for (n, (_, it)) in picked.iter().enumerate() {
                let mut item = fresh(it);
                relink(&mut item, &mut links);
                made.push(item.id());
                ops.push(Op::InsertItem {
                    track: track.id,
                    index: last + 1 + n,
                    item,
                });
            }
        }
        let description = format!(
            "Duplicate {} item{}",
            made.len(),
            if made.len() == 1 { "" } else { "s" }
        );
        if self.do_op(Op::Transaction { description, ops }) {
            self.select_only(&made);
        }
    }

    /// Remove every selected item: leave gaps (lift) or close them (ripple).
    pub fn delete_selection(&mut self, ripple: bool) {
        let ids = self.selection_ids();
        if ids.len() < 2 {
            return;
        }
        let mut p = self.project().clone();
        let mut ops = Vec::new();
        for id in ids {
            let plan: Result<Op, OpError> = if ripple {
                Ok(Op::RemoveItem { item: id })
            } else {
                edit::lift(&p, id)
            };
            match plan.and_then(|op| {
                let (next, _) = apply(&p, &op)?;
                p = next;
                Ok(op)
            }) {
                Ok(op) => ops.push(op),
                Err(e) => {
                    self.toast(e.to_string(), true);
                    return;
                }
            }
        }
        let description = format!(
            "{} {} items",
            if ripple { "Ripple delete" } else { "Lift" },
            ops.len()
        );
        if self.do_op(Op::Transaction { description, ops }) {
            self.select_only(&[]);
        }
    }
}

/// A pasted copy of a linked clip joins a new link group, shared by the copies of its partners.
fn relink(item: &mut Item, links: &mut std::collections::HashMap<Id, Id>) {
    if let Item::Clip(c) = item {
        if let Some(old) = c.link {
            c.link = Some(*links.entry(old).or_insert_with(Id::new));
        }
    }
}

/// A copy of the item that can sit beside the original: new ids, no transition.
fn fresh(item: &Item) -> Item {
    match item {
        Item::Clip(c) => {
            let mut n = c.clone();
            n.id = Id::new();
            n.transition = None;
            Item::Clip(n)
        }
        Item::Title(t) => {
            let mut n = t.clone();
            n.id = Id::new();
            Item::Title(n)
        }
        Item::Gap(g) => {
            let mut n = g.clone();
            n.id = Id::new();
            Item::Gap(n)
        }
    }
}

#[allow(dead_code)]
fn _assert_project_is_used(_: &Project) {}

impl App {
    /// Bring a timeline in from an `.otio` or `.edl` file.
    pub fn import_timeline_file(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => return self.toast(format!("cannot read {}: {e}", path.display()), true),
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let plan = match ext.as_str() {
            "otio" => captaine_interchange::otio::import(self.project(), &text),
            "edl" => match self.timeline_id() {
                Some(tl) => captaine_interchange::edl::import(self.project(), tl, &text),
                None => Err("open or create a timeline first".into()),
            },
            _ => Err("only .otio and .edl files can be imported".into()),
        };
        match plan {
            Ok(ops) => {
                let n = ops.len();
                if self.do_op(Op::Transaction {
                    description: format!(
                        "Import {}",
                        path.file_name()
                            .map_or(String::new(), |n| n.to_string_lossy().into())
                    ),
                    ops,
                }) {
                    self.toast(format!("Imported {} ({n} changes)", path.display()), false);
                }
            }
            Err(e) => self.toast(e, true),
        }
    }
}
