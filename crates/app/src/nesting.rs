//! Sequences inside sequences: nest the selected clips into a new sequence that plays as one clip, put a
//! whole sequence into another, and step into a nested one to edit it.

use captaine_model::{Clip, Id, Item, MediaRef, Timeline, Track, TrackKind};
use captaine_ops::{apply, edit, Op};
use captaine_time::RationalTime;

use crate::app::App;

impl App {
    /// The nested-sequence media that plays `timeline`, if the project has one yet.
    fn media_for_sequence(&self, timeline: Id) -> Option<MediaRef> {
        self.project()
            .media
            .values()
            .find(|m| m.nested_timeline() == Some(timeline))
            .cloned()
    }

    /// Put a clip that plays `inner` on `track` at `at` (the playhead by default), as long as that sequence.
    pub fn insert_sequence(&mut self, inner: Id, at: Option<i64>) {
        let Some(cur) = self.timeline().cloned() else {
            return;
        };
        let Some(inner_tl) = self.project().timeline(inner).cloned() else {
            return;
        };
        if inner == cur.id {
            return self.toast("A sequence cannot contain itself", true);
        }
        if inner_tl.rate != cur.rate {
            return self.toast("That sequence has a different frame rate", true);
        }
        let mut ops = Vec::new();
        let media = match self.media_for_sequence(inner) {
            Some(m) => m,
            None => {
                let m = MediaRef::nested(&inner_tl, self.project().canvas(inner));
                ops.push(Op::AddMedia { media: m.clone() });
                m
            }
        };
        let ticks = inner_tl.duration().value.max(1);
        let Some(track) = cur.tracks.iter().rev().find(|t| t.kind == TrackKind::Video) else {
            return self.toast("There is no video track to put it on", true);
        };
        let clip = Clip::new(
            Id::new(),
            inner_tl.name.clone(),
            media.id,
            RationalTime::new(0, cur.rate),
            RationalTime::new(ticks, cur.rate),
        );
        let id = clip.id;
        let mut p = self.project().clone();
        for op in &ops {
            match apply(&p, op) {
                Ok((n, _)) => p = n,
                Err(e) => return self.toast(e.to_string(), true),
            }
        }
        let at = RationalTime::new(at.unwrap_or(self.playhead).max(0), cur.rate);
        match edit::overwrite(&p, track.id, at, clip) {
            Ok(op) => ops.push(op),
            Err(e) => return self.toast(e.to_string(), true),
        }
        if self.do_op(Op::Transaction {
            description: format!("Insert sequence {}", inner_tl.name),
            ops,
        }) {
            self.select_only(&[id]);
        }
    }

    /// Move the selected items into a new sequence and put one clip that plays it where they were.
    pub fn nest_selection(&mut self) {
        let ids = self.selection_ids();
        let Some(cur) = self.timeline().cloned() else {
            return;
        };
        // what is selected, with its track index and start
        let mut picked: Vec<(usize, i64, Item)> = Vec::new();
        for (ti, t) in cur.tracks.iter().enumerate() {
            for (it, st) in t.items.iter().zip(t.item_starts()) {
                if ids.contains(&it.id()) && it.is_content() {
                    picked.push((ti, st, it.clone()));
                }
            }
        }
        if picked.is_empty() {
            return self.toast("Select the clips to nest first", false);
        }
        let start = picked.iter().map(|p| p.1).min().unwrap_or(0);
        let end = picked
            .iter()
            .map(|(_, s, i)| s + i.duration().value)
            .max()
            .unwrap_or(start);
        // the new sequence has the tracks the picked items came from, in the same order
        let mut used: Vec<usize> = picked.iter().map(|p| p.0).collect();
        used.sort_unstable();
        used.dedup();
        let n = self.sequences().len() + 1;
        let mut inner = Timeline::new(Id::new(), format!("Nested Sequence {n}"), cur.rate);
        inner.canvas = cur.canvas;
        let mut scratch = self.project().clone();
        let mut ops: Vec<Op> = Vec::new();
        for ti in &used {
            let src = &cur.tracks[*ti];
            let mut t = Track::new(Id::new(), src.name.clone(), src.kind);
            t.gain_db = src.gain_db;
            inner.tracks.push(std::sync::Arc::new(t));
        }
        // the items go into the new sequence, shifted to start at zero, with fresh ids (the originals are
        // lifted out of the current sequence below)
        for (ti, st, it) in &picked {
            let slot = used.iter().position(|u| u == ti).expect("track used");
            let t = std::sync::Arc::make_mut(&mut inner.tracks[slot]);
            let at = st - start;
            let have: i64 = t.items.iter().map(|i| i.duration().value).sum();
            if at > have {
                t.items.push(Item::Gap(captaine_model::Gap {
                    id: Id::new(),
                    duration: RationalTime::new(at - have, cur.rate),
                }));
            }
            let mut copy = it.clone();
            match &mut copy {
                Item::Clip(c) => {
                    c.id = Id::new();
                    c.link = None;
                    c.transition = None;
                }
                Item::Title(x) => x.id = Id::new(),
                Item::Gap(g) => g.id = Id::new(),
            }
            t.items.push(copy);
        }
        let inner_id = inner.id;
        let media = MediaRef::nested(&inner, self.project().canvas(cur.id));
        let media_id = media.id;
        ops.push(Op::AddTimeline { timeline: inner });
        ops.push(Op::AddMedia { media });
        for (_, _, it) in &picked {
            match edit::lift(&scratch, it.id()) {
                Ok(op) => ops.push(op),
                Err(e) => return self.toast(e.to_string(), true),
            }
        }
        for op in &ops {
            match apply(&scratch, op) {
                Ok((p, _)) => scratch = p,
                Err(e) => return self.toast(e.to_string(), true),
            }
        }
        // the nested clip goes on the highest video track involved (or the highest audio one)
        let target = used
            .iter()
            .rev()
            .map(|u| &cur.tracks[*u])
            .find(|t| t.kind == TrackKind::Video)
            .or_else(|| used.iter().rev().map(|u| &cur.tracks[*u]).next())
            .expect("something was picked");
        let clip = Clip::new(
            Id::new(),
            format!("Nested Sequence {n}"),
            media_id,
            RationalTime::new(0, cur.rate),
            RationalTime::new((end - start).max(1), cur.rate),
        );
        let new_id = clip.id;
        match edit::overwrite(
            &scratch,
            target.id,
            RationalTime::new(start, cur.rate),
            clip,
        ) {
            Ok(op) => ops.push(op),
            Err(e) => return self.toast(e.to_string(), true),
        }
        if self.do_op(Op::Transaction {
            description: format!("Nest {} items", picked.len()),
            ops,
        }) {
            let _ = inner_id;
            self.select_only(&[new_id]);
        }
    }

    /// The sequence a clip plays, if it is a nested one.
    pub fn nested_sequence_of(&self, clip: &Clip) -> Option<Id> {
        self.project()
            .media
            .get(&clip.media)
            .and_then(|m| m.nested_timeline())
            .filter(|t| self.project().timeline(*t).is_some())
    }

    /// Switch to the sequence inside the selected nested clip.
    pub fn open_nested(&mut self) {
        let Some((clip, ..)) = self.selected() else {
            return self.toast("Select a nested sequence clip first", false);
        };
        match self.nested_sequence_of(&clip) {
            Some(t) => self.set_active_timeline(t),
            None => self.toast("That clip is not a nested sequence", false),
        }
    }
}
