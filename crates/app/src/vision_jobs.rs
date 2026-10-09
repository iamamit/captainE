//! Slow analyses that run in the background and finish by making one undoable edit.

use std::sync::mpsc::{channel, Receiver, TryRecvError};

use captaine_model::{Effect, EffectKind, Id};
use captaine_ops::Op;
use captaine_vision::MatteMode;

use crate::app::App;

pub enum Outcome {
    /// A subject matte effect to put on the clip
    Matte(Effect),
    /// Edits to make, as one undo step, and a sequence to switch to afterwards
    Edit {
        description: String,
        ops: Vec<Op>,
        switch_to: Option<Id>,
    },
}

pub struct VisionJob {
    pub clip: Id,
    pub what: &'static str,
    rx: Receiver<Result<Outcome, String>>,
}

impl App {
    /// Cut the subject out of the clip's picture (in the background).
    pub fn remove_background(&mut self, clip_id: Id, mode: MatteMode) {
        let Some((clip, ..)) = self
            .selected()
            .filter(|(c, ..)| c.id == clip_id)
            .or_else(|| self.clip_by_id(clip_id))
        else {
            return;
        };
        let Some(media) = self.project().media.get(&clip.media).cloned() else {
            return;
        };
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = captaine_agent::remove_background(&clip, &media, mode).map(Outcome::Matte);
            let _ = tx.send(r);
            ctx.request_repaint();
        });
        self.vision_jobs.push(VisionJob {
            clip: clip_id,
            what: "Remove background",
            rx,
        });
    }

    /// Apply the results of finished jobs.
    pub fn poll_vision_jobs(&mut self) {
        if self.vision_jobs.is_empty() {
            return;
        }
        let mut done = Vec::new();
        for (i, j) in self.vision_jobs.iter().enumerate() {
            match j.rx.try_recv() {
                Ok(r) => done.push((i, Some(r))),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => done.push((i, None)),
            }
        }
        for (i, r) in done.into_iter().rev() {
            let job = self.vision_jobs.remove(i);
            match r {
                Some(Ok(Outcome::Matte(fx))) => {
                    let Some((clip, ..)) = self.clip_by_id(job.clip) else {
                        continue;
                    };
                    let mut effects = clip.effects.clone();
                    effects.retain(|e| e.kind != EffectKind::Matte);
                    effects.insert(0, fx);
                    self.do_op(Op::SetEffects {
                        item: job.clip,
                        effects,
                    });
                }
                Some(Ok(Outcome::Edit {
                    description,
                    ops,
                    switch_to,
                })) => {
                    if self.do_op(Op::Transaction { description, ops }) {
                        if let Some(tl) = switch_to {
                            self.set_active_timeline(tl);
                        }
                    }
                }
                Some(Err(e)) => self.toast(format!("{}: {e}", job.what), true),
                None => {}
            }
        }
    }

    /// A clip anywhere in the sequence, with its track and start.
    pub fn clip_by_id(&self, id: Id) -> Option<(captaine_model::Clip, Id, i64)> {
        let tl = self.timeline()?;
        tl.tracks.iter().find_map(|t| {
            t.items
                .iter()
                .zip(t.item_starts())
                .find_map(|(it, st)| match it {
                    captaine_model::Item::Clip(c) if c.id == id => Some((c.clone(), t.id, st)),
                    _ => None,
                })
        })
    }
}

/// Output frame sizes for the reframe presets, from the project's short side.
pub const REFRAMES: [(&str, u32, u32); 3] = [
    ("Vertical 9:16", 9, 16),
    ("Square 1:1", 1, 1),
    ("Portrait 4:5", 4, 5),
];

impl App {
    fn spawn_job(
        &mut self,
        clip: Id,
        what: &'static str,
        work: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        self.vision_jobs.push(VisionJob { clip, what, rx });
    }

    /// Make the selected title follow the subject of the selected clip.
    pub fn track_attach_selected(&mut self) {
        let ids = self.selection_ids();
        let tl = self.timeline().cloned();
        let (Some(tl), true) = (tl, !ids.is_empty()) else {
            return self.toast("Select a video clip and a title (shift-click) first", false);
        };
        let mut clip = None;
        let mut title = None;
        for (t, track) in tl.tracks.iter().map(|t| (t, t)) {
            for (it, st) in track.items.iter().zip(t.item_starts()) {
                if !ids.contains(&it.id()) {
                    continue;
                }
                match it {
                    captaine_model::Item::Clip(c) => clip = Some((c.clone(), st)),
                    captaine_model::Item::Title(x) => title = Some((x.clone(), st)),
                    _ => {}
                }
            }
        }
        let (Some((clip, cs)), Some((title, ts))) = (clip, title) else {
            return self.toast(
                "Select one video clip and one title together (shift-click)",
                false,
            );
        };
        let Some(media) = self.project().media.get(&clip.media).cloned() else {
            return;
        };
        let (canvas, fps) = (self.canvas(), self.fps());
        let id = clip.id;
        self.spawn_job(id, "Track subject", move || {
            let ops =
                captaine_agent::track_attach(&clip, &media, &title, cs, ts, canvas, fps, None)?;
            Ok(Outcome::Edit {
                description: "Attach title to subject".into(),
                ops,
                switch_to: None,
            })
        });
    }

    /// A copy of the sequence in another shape, each clip panned to keep its subject in view.
    pub fn auto_reframe(&mut self, label: &str, ratio: (u32, u32)) {
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let s = &self.project().settings;
        let short = s.width.min(s.height).max(2);
        let even = |v: f64| ((v.round() as u32).max(2)) & !1;
        let canvas = if ratio.0 <= ratio.1 {
            (
                even(short as f64 * ratio.0 as f64 / ratio.0.min(ratio.1) as f64),
                even(short as f64 * ratio.1 as f64 / ratio.0.min(ratio.1) as f64),
            )
        } else {
            (
                even(short as f64 * ratio.0 as f64 / ratio.1 as f64),
                even(short as f64),
            )
        };
        let name = format!("{} ({label})", tl.name);
        let mut copy = crate::sequences::duplicate(&tl, name);
        copy.canvas = Some(canvas);
        let new_id = copy.id;
        let media = self.project().media.clone();
        let fps = self.fps();
        let what: &'static str = "Auto reframe";
        let first = tl.id;
        self.spawn_job(first, what, move || {
            let mut ops: Vec<Op> = vec![Op::AddTimeline {
                timeline: copy.clone(),
            }];
            for track in &copy.tracks {
                for item in &track.items {
                    let captaine_model::Item::Clip(c) = item else {
                        continue;
                    };
                    let Some(m) = media.get(&c.media).filter(|m| m.has_video) else {
                        continue;
                    };
                    if track.kind != captaine_model::TrackKind::Video {
                        continue;
                    }
                    ops.extend(captaine_agent::reframe(c, m, canvas, fps)?);
                }
            }
            Ok(Outcome::Edit {
                description: format!(
                    "Auto reframe to {label_owned}",
                    label_owned = canvas.0.to_string() + "x" + &canvas.1.to_string()
                ),
                ops,
                switch_to: Some(new_id),
            })
        });
    }
}
