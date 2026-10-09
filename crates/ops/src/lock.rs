//! Locked tracks: which tracks an op would change, so edits to a locked one can be refused.

use captaine_model::{Id, Project};

use crate::Op;

fn track_of_item(p: &Project, item: Id) -> Option<Id> {
    let loc = p.find_item(item)?;
    Some(p.timeline(loc.timeline)?.tracks[loc.track].id)
}

/// The tracks whose items `op` adds, removes or changes. Settings of a track itself (mute, level, pan,
/// solo, lock) are not edits of its content.
pub fn touched_tracks(p: &Project, op: &Op) -> Vec<Id> {
    let item = |i: &Id| track_of_item(p, *i).into_iter().collect::<Vec<_>>();
    match op {
        Op::InsertItem { track, .. } => vec![*track],
        Op::RemoveTrack { track } => vec![*track],
        Op::RemoveItem { item: i }
        | Op::ReplaceItem { item: i, .. }
        | Op::Split { item: i, .. }
        | Op::Trim { item: i, .. }
        | Op::SetRange { item: i, .. }
        | Op::ReplaceRaw { item: i, .. }
        | Op::Slip { item: i, .. }
        | Op::SetVideo { item: i, .. }
        | Op::SetAudio { item: i, .. }
        | Op::SetTitle { item: i, .. }
        | Op::SetAnimTrack { item: i, .. }
        | Op::SetEnabled { item: i, .. }
        | Op::SetName { item: i, .. }
        | Op::SetEffects { item: i, .. }
        | Op::SetTransition { item: i, .. }
        | Op::SetRetime { item: i, .. }
        | Op::SetLink { item: i, .. } => item(i),
        Op::Join { left, .. } => item(left),
        Op::Transaction { ops, .. } => ops.iter().flat_map(|o| touched_tracks(p, o)).collect(),
        _ => Vec::new(),
    }
}

/// The first locked track that `op` would change, if any.
pub fn locked_hit(p: &Project, op: &Op) -> Option<Id> {
    touched_tracks(p, op).into_iter().find(|t| {
        p.find_track(*t)
            .and_then(|(tl, ti)| p.timeline(tl).map(|tl| tl.tracks[ti].locked))
            .unwrap_or(false)
    })
}
