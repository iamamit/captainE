use captaine_model::{Id, Item, MediaRef, Timeline, Track};
use captaine_time::RationalTime;
use serde::{Deserialize, Serialize};

/// Which end of a clip a trim moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    In,
    Out,
}

/// A small, semantic change. Time arguments are at the target timeline's rate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    AddMedia {
        media: MediaRef,
    },
    RemoveMedia {
        id: Id,
    },
    AddTimeline {
        timeline: Timeline,
    },
    RemoveTimeline {
        id: Id,
    },
    AddTrack {
        timeline: Id,
        index: usize,
        track: Track,
    },
    RemoveTrack {
        track: Id,
    },
    /// Insert at `index`; later items move right (ripple).
    InsertItem {
        track: Id,
        index: usize,
        item: Item,
    },
    /// Remove and close the hole (ripple delete / extract).
    RemoveItem {
        item: Id,
    },
    /// Swap an item for another of the same duration (e.g. lift: clip -> gap). Nothing moves.
    ReplaceItem {
        item: Id,
        with: Item,
    },
    /// Cut `item` into two at `at` from its start. The right half gets `new_id`.
    Split {
        item: Id,
        at: RationalTime,
        new_id: Id,
    },
    /// Inverse of `Split`: merge `right` back into `left` if they are contiguous and identical otherwise.
    Join {
        left: Id,
        right: Id,
    },
    /// Ripple trim. `delta` moves the chosen edge later (positive) or earlier (negative):
    /// In + shortens the clip from the front; Out + lengthens it.
    Trim {
        item: Id,
        edge: Edge,
        delta: RationalTime,
    },
    SetEnabled {
        item: Id,
        enabled: bool,
    },
    SetName {
        item: Id,
        name: String,
    },
    /// Ops applied in order as one undo step. Fails as a whole.
    Transaction {
        description: String,
        ops: Vec<Op>,
    },
}

impl Op {
    /// Short human-readable summary for history lists and agent logs.
    pub fn describe(&self) -> String {
        match self {
            Op::AddMedia { media } => format!("Add media {}", media.name),
            Op::RemoveMedia { id } => format!("Remove media {id}"),
            Op::AddTimeline { timeline } => format!("Add timeline {}", timeline.name),
            Op::RemoveTimeline { id } => format!("Remove timeline {id}"),
            Op::AddTrack { track, .. } => format!("Add track {}", track.name),
            Op::RemoveTrack { track } => format!("Remove track {track}"),
            Op::InsertItem { index, .. } => format!("Insert item at {index}"),
            Op::RemoveItem { item } => format!("Ripple delete {item}"),
            Op::ReplaceItem { item, .. } => format!("Replace {item}"),
            Op::Split { item, at, .. } => format!("Split {item} at +{}", at.value),
            Op::Join { left, right } => format!("Join {left} and {right}"),
            Op::Trim { item, edge, delta } => format!("Trim {edge:?} of {item} by {}", delta.value),
            Op::SetEnabled { item, enabled } => {
                format!("{} {item}", if *enabled { "Enable" } else { "Disable" })
            }
            Op::SetName { item, name } => format!("Rename {item} to {name}"),
            Op::Transaction { description, .. } => description.clone(),
        }
    }
}
