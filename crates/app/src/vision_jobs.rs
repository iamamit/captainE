//! Slow analyses that run in the background and finish by making one undoable edit.

use std::sync::mpsc::{channel, Receiver, TryRecvError};

use captaine_model::{Effect, EffectKind, Id};
use captaine_ops::Op;
use captaine_vision::MatteMode;

use crate::app::App;

pub enum Outcome {
    /// A subject matte effect to put on the clip
    Matte(Effect),
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
