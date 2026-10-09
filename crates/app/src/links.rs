//! Linked clips: a video clip and its separated sound (or any clips the user links) are selected, moved,
//! trimmed, cut and deleted together.

use captaine_model::{Clip, Id, Item, TrackKind};
use captaine_ops::{apply, edit, Op};
use captaine_time::RationalTime;

use crate::app::App;

impl App {
    /// Every clip with its track id and start tick.
    fn all_clips(&self) -> Vec<(Clip, Id, i64)> {
        let Some(tl) = self.timeline() else {
            return Vec::new();
        };
        tl.tracks
            .iter()
            .flat_map(|t| {
                t.items
                    .iter()
                    .zip(t.item_starts())
                    .filter_map(move |(it, st)| match it {
                        Item::Clip(c) => Some((c.clone(), t.id, st)),
                        _ => None,
                    })
            })
            .collect()
    }

    /// The clips linked to `id` that play at the same time as it (a link group that was cut in two is two
    /// pairs, not one).
    pub fn link_partners(&self, id: Id) -> Vec<Id> {
        let all = self.all_clips();
        let Some((me, _, start)) = all.iter().find(|(c, ..)| c.id == id).cloned() else {
            return Vec::new();
        };
        let Some(link) = me.link else {
            return Vec::new();
        };
        let end = start + me.duration.value;
        all.into_iter()
            .filter(|(c, _, s)| {
                c.id != id && c.link == Some(link) && *s < end && s + c.duration.value > start
            })
            .map(|(c, ..)| c.id)
            .collect()
    }

    /// Select this clip and, when linking is on, the clips linked to it.
    pub fn select_with_links(&mut self, id: Id) {
        let mut ids = vec![id];
        if self.link_on {
            ids.extend(self.link_partners(id));
        }
        self.select_only(&ids);
    }

    /// Link the selected clips into one group.
    pub fn link_selected(&mut self) {
        let ids: Vec<Id> = self
            .selection_ids()
            .into_iter()
            .filter(|i| self.all_clips().iter().any(|(c, ..)| c.id == *i))
            .collect();
        if ids.len() < 2 {
            return self.toast("Select two or more clips to link them", false);
        }
        let link = Id::new();
        let ops = ids
            .iter()
            .map(|i| Op::SetLink {
                item: *i,
                link: Some(link),
            })
            .collect();
        self.do_op(Op::Transaction {
            description: format!("Link {} clips", ids.len()),
            ops,
        });
    }

    pub fn unlink_selected(&mut self) {
        let clips = self.all_clips();
        let ops: Vec<Op> = self
            .selection_ids()
            .into_iter()
            .filter(|i| clips.iter().any(|(c, ..)| c.id == *i && c.link.is_some()))
            .map(|i| Op::SetLink {
                item: i,
                link: None,
            })
            .collect();
        if ops.is_empty() {
            return self.toast("The selection has no linked clips", false);
        }
        let n = ops.len();
        self.do_op(Op::Transaction {
            description: format!("Unlink {n} clips"),
            ops,
        });
    }

    /// Split the picture and the sound of the selected video clip onto separate tracks, linked together.
    pub fn separate_audio(&mut self) {
        let Some((clip, track, start)) = self.selected() else {
            return self.toast("Select a video clip with sound first", false);
        };
        let Some(media) = self.project().media.get(&clip.media).cloned() else {
            return;
        };
        if !media.has_video || !media.has_audio || clip.audio.muted {
            return self.toast(
                "This clip has no sound that is still part of its picture",
                false,
            );
        }
        let Some(tl) = self.timeline().cloned() else {
            return;
        };
        let _ = track;
        // an audio track with nothing in the way
        let end = start + clip.duration.value;
        let free = tl.tracks.iter().find(|t| {
            t.kind == TrackKind::Audio
                && t.items
                    .iter()
                    .zip(t.item_starts())
                    .all(|(it, s)| !it.is_content() || s >= end || s + it.duration().value <= start)
        });
        let Some(audio_track) = free else {
            return self.toast(
                "There is no free audio track here; add an audio track first",
                true,
            );
        };
        let link = clip.link.unwrap_or_else(Id::new);
        let mut twin = clip.clone();
        twin.id = Id::new();
        twin.link = Some(link);
        twin.video = Default::default();
        twin.transition = None;
        twin.effects.retain(|e| e.kind.is_audio());
        // only the sound keyframes go with the sound
        let mut anim = captaine_model::Anim::default();
        for p in ["volume", "pan"] {
            if clip.anim.has(p) {
                anim.set_track(p, clip.anim.keys(p).to_vec());
            }
        }
        twin.anim = anim;
        let mut silent = clip.audio;
        silent.muted = true;
        let at = RationalTime::new(start, clip.duration.rate);
        let place = match edit::overwrite(self.project(), audio_track.id, at, twin) {
            Ok(op) => op,
            Err(e) => return self.toast(e.to_string(), true),
        };
        let ops = vec![
            Op::SetAudio {
                item: clip.id,
                audio: silent,
            },
            Op::SetLink {
                item: clip.id,
                link: Some(link),
            },
            place,
        ];
        // prove it applies before committing a half-edit
        let mut p = self.project().clone();
        for op in &ops {
            match apply(&p, op) {
                Ok((n, _)) => p = n,
                Err(e) => return self.toast(e.to_string(), true),
            }
        }
        self.do_op(Op::Transaction {
            description: "Separate audio".into(),
            ops,
        });
    }

    /// The tracks that hold the selected items (a cut at the playhead goes through all of them).
    pub fn selection_tracks(&self) -> Vec<Id> {
        let ids = self.selection_ids();
        let Some(tl) = self.timeline() else {
            return Vec::new();
        };
        tl.tracks
            .iter()
            .filter(|t| t.items.iter().any(|i| ids.contains(&i.id())))
            .map(|t| t.id)
            .collect()
    }

    /// Partners to move or trim together with `item`: the selected clips linked to it, or, for a plain
    /// multi-selection, every other selected item.
    pub fn companions_of(&self, item: Id) -> Vec<Id> {
        let sel = self.selection_ids();
        if !sel.contains(&item) {
            return Vec::new();
        }
        sel.into_iter().filter(|i| *i != item).collect()
    }
}
