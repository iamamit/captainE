//! Several sequences in one project: switching, creating, duplicating, renaming and deleting them.

use std::sync::Arc;

use captaine_model::{Id, Timeline, Track, TrackKind};
use captaine_ops::Op;

use crate::app::App;

/// A copy of `tl` that can live beside it: new ids on the timeline, its tracks, items and markers.
pub fn duplicate(tl: &Timeline, name: String) -> Timeline {
    tl.duplicate(name)
}

impl App {
    pub fn sequences(&self) -> Vec<(Id, String)> {
        self.project()
            .timelines
            .values()
            .map(|t| (t.id, t.name.clone()))
            .collect()
    }

    pub fn set_active_timeline(&mut self, id: Id) {
        if self.timeline_id() == Some(id) || self.project().timeline(id).is_none() {
            return;
        }
        self.pause();
        self.active_timeline = Some(id);
        self.select_only(&[]);
        self.playhead = 0;
        self.mark_in = None;
        self.mark_out = None;
        self.scroll = 0.0;
        self.sent_revision = None;
        self.last_requested = None;
        self.program_tick = -1;
    }

    fn unique_sequence_name(&self, base: &str) -> String {
        let names: Vec<String> = self.sequences().into_iter().map(|s| s.1).collect();
        if !names.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !names.contains(n))
            .expect("some number is free")
    }

    /// A new empty sequence with the project's usual tracks; it becomes the active one.
    pub fn new_sequence(&mut self) {
        let rate = self.rate();
        let id = Id::new();
        let name = self.unique_sequence_name("Sequence");
        let mut tl = Timeline::new(id, name, rate);
        for (n, k) in [
            ("V1", TrackKind::Video),
            ("V2", TrackKind::Video),
            ("A1", TrackKind::Audio),
            ("A2", TrackKind::Audio),
        ] {
            tl.tracks.push(Arc::new(Track::new(Id::new(), n, k)));
        }
        if self.do_op(Op::AddTimeline { timeline: tl }) {
            self.set_active_timeline(id);
        }
    }

    pub fn duplicate_sequence(&mut self) {
        let Some(cur) = self.timeline().cloned() else {
            return;
        };
        let name = self.unique_sequence_name(&format!("{} copy", cur.name));
        let copy = duplicate(&cur, name);
        let id = copy.id;
        if self.do_op(Op::AddTimeline { timeline: copy }) {
            self.set_active_timeline(id);
        }
    }

    pub fn rename_sequence(&mut self, id: Id, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || self.project().timeline(id).is_some_and(|t| t.name == name) {
            return;
        }
        self.do_op(Op::RenameTimeline { id, name });
    }

    pub fn delete_sequence(&mut self, id: Id) {
        if self.project().timelines.len() < 2 {
            self.toast("A project needs at least one sequence", false);
            return;
        }
        let was_active = self.timeline_id() == Some(id);
        let next = self.sequences().into_iter().map(|s| s.0).find(|x| *x != id);
        if self.do_op(Op::RemoveTimeline { id }) && was_active {
            self.active_timeline = None;
            if let Some(n) = next {
                self.set_active_timeline(n);
            }
        }
    }
}
