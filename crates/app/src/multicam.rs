//! Multicam editing: line up clips shot of the same event by their sound, then cut between the angles live.

use captaine_model::{Id, Item};
use captaine_ops::{apply, edit, Op};
use captaine_time::RationalTime;

use crate::app::App;

pub struct Multicam {
    /// The tracks holding the camera angles, in angle order (keys 1, 2, 3 ...)
    pub angles: Vec<Id>,
}

impl App {
    /// Move the other selected clips so their sound lines up with the first selected clip's.
    pub fn sync_selected_by_audio(&mut self) {
        let ids = self.selection_ids();
        let clips: Vec<_> = ids.iter().filter_map(|i| self.clip_by_id(*i)).collect();
        let Some((first, _, first_start)) = clips.first().cloned() else {
            return self.toast("Select two or more clips of the same event", false);
        };
        if clips.len() < 2 {
            return self.toast("Select two or more clips of the same event", false);
        }
        let fps = self.fps();
        let Some(fm) = self.project().media.get(&first.media).cloned() else {
            return;
        };
        let mut p = self.project().clone();
        let mut ops = Vec::new();
        let mut weak: Vec<String> = Vec::new();
        for (c, track, current) in clips.iter().skip(1) {
            let Some(m) = self.project().media.get(&c.media).cloned() else {
                continue;
            };
            if !fm.has_audio || !m.has_audio {
                weak.push(c.name.clone());
                continue;
            }
            let window = (first.duration.value.min(c.duration.value) as f64 / fps).clamp(2.0, 40.0);
            let al = match captaine_audio::find_offset(
                &fm.path,
                first.source_start.to_seconds_f64(),
                &m.path,
                c.source_start.to_seconds_f64(),
                window,
                30.0,
            ) {
                Ok(a) => a,
                Err(e) => return self.toast(e.to_string(), true),
            };
            if al.confidence < 3.5 {
                weak.push(c.name.clone());
                continue;
            }
            // events coincide when this clip starts at the first one's start plus the offset
            let new_start = (first_start + (al.offset_secs * fps).round() as i64).max(0);
            if new_start == *current {
                continue; // already in sync
            }
            let at = RationalTime::new(new_start, self.rate());
            match edit::move_item(&p, c.id, *track, at).and_then(|op| {
                let (n, _) = apply(&p, &op)?;
                p = n;
                Ok(op)
            }) {
                Ok(op) => ops.push(op),
                Err(e) => return self.toast(e.to_string(), true),
            }
        }
        if !weak.is_empty() {
            self.toast(
                format!("Could not find a clear match for: {}", weak.join(", ")),
                true,
            );
        }
        if !ops.is_empty() {
            let n = ops.len();
            self.do_op(Op::Transaction {
                description: format!("Sync {n} clips by audio"),
                ops,
            });
        }
    }

    /// Start switching between the tracks that hold the selected clips (the first selected is angle 1).
    pub fn start_multicam(&mut self) {
        let ids = self.selection_ids();
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let mut angles: Vec<Id> = Vec::new();
        let mut first_start = i64::MAX;
        for id in &ids {
            for t in &tl.tracks {
                for (it, st) in t.items.iter().zip(t.item_starts()) {
                    if it.id() == *id && matches!(it, Item::Clip(_)) && !angles.contains(&t.id) {
                        angles.push(t.id);
                        first_start = first_start.min(st);
                    }
                }
            }
        }
        if angles.len() < 2 {
            return self.toast(
                "Select clips on two or more tracks (one per camera angle)",
                false,
            );
        }
        self.multicam = Some(Multicam { angles });
        // start on angle 1 where the earliest clip starts
        self.multicam_cut(0, Some(first_start.max(0)));
    }

    /// Cut to angle `k` (0-based) at the playhead (or `at`).
    pub fn multicam_cut(&mut self, k: usize, at: Option<i64>) {
        let Some(mc) = &self.multicam else { return };
        let angles = mc.angles.clone();
        let when = RationalTime::new(at.unwrap_or(self.playhead).max(0), self.rate());
        match edit::multicam_cut(self.project(), &angles, k, when) {
            Ok(op) => {
                self.do_op(op);
            }
            Err(e) => self.toast(e.to_string(), true),
        }
    }

    pub fn end_multicam(&mut self) {
        self.multicam = None;
    }
}
