use captaine_model::{
    AudioProps, Effect, Id, Item, Keyframe, Marker, MediaRef, Retime, Timeline, Title, Track,
    Transition, VideoProps,
};
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
    RenameTimeline {
        id: Id,
        name: String,
    },
    /// Give a sequence its own frame size (None: the project's).
    SetCanvas {
        timeline: Id,
        canvas: Option<(u32, u32)>,
    },
    AddMarker {
        timeline: Id,
        marker: Marker,
    },
    RemoveMarker {
        timeline: Id,
        marker: Id,
    },
    /// Replace the marker with this marker's id (move it, rename it, recolour it).
    SetMarker {
        timeline: Id,
        marker: Marker,
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
    /// Set a clip's source window and length exactly (ripple). The exact inverse of trims and slips.
    SetRange {
        item: Id,
        source_start: RationalTime,
        duration: RationalTime,
    },
    /// Swap an item for another of any length. Internal: the undo of `Join`.
    ReplaceRaw {
        item: Id,
        with: Item,
    },
    /// Move the source window under a clip without changing where or how long it is.
    Slip {
        item: Id,
        delta: RationalTime,
    },
    SetVideo {
        item: Id,
        video: VideoProps,
    },
    SetAudio {
        item: Id,
        audio: AudioProps,
    },
    SetTrack {
        track: Id,
        enabled: bool,
        gain_db: f64,
    },
    /// Reorder tracks within their timeline (index 0 is the bottom video layer).
    MoveTrack {
        track: Id,
        index: usize,
    },
    SetProxy {
        media: Id,
        proxy: Option<String>,
    },
    /// Replace a title's content and look (the id and the length stay).
    SetTitle {
        item: Id,
        title: Title,
    },
    /// Replace the keyframes of one property of a clip or title (empty list: remove the animation).
    SetAnimTrack {
        item: Id,
        prop: String,
        keys: Vec<Keyframe>,
    },
    /// Change a clip's speed, direction or freeze. The same stretch of source plays, so the clip
    /// gets shorter or longer (ripple).
    SetRetime {
        item: Id,
        retime: Retime,
    },
    /// Put a transition between this clip and the one before it, or take it away (None).
    SetTransition {
        item: Id,
        transition: Option<Transition>,
    },
    /// Join a clip to a link group (or take it out with None).
    SetLink {
        item: Id,
        link: Option<Id>,
    },
    /// Replace a clip's effect list.
    SetEffects {
        item: Id,
        effects: Vec<Effect>,
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
            Op::RenameTimeline { name, .. } => format!("Rename timeline to {name}"),
            Op::SetCanvas { canvas, .. } => match canvas {
                Some((w, h)) => format!("Set sequence size to {w}x{h}"),
                None => "Use the project frame size".to_string(),
            },
            Op::AddMarker { marker, .. } => format!("Add marker {}", marker.name),
            Op::RemoveMarker { marker, .. } => format!("Remove marker {marker}"),
            Op::SetMarker { marker, .. } => format!("Edit marker {}", marker.name),
            Op::AddTrack { track, .. } => format!("Add track {}", track.name),
            Op::RemoveTrack { track } => format!("Remove track {track}"),
            Op::InsertItem { index, .. } => format!("Insert item at {index}"),
            Op::RemoveItem { item } => format!("Ripple delete {item}"),
            Op::ReplaceItem { item, .. } => format!("Replace {item}"),
            Op::Split { item, at, .. } => format!("Split {item} at +{}", at.value),
            Op::Join { left, right } => format!("Join {left} and {right}"),
            Op::Trim { item, edge, delta } => format!("Trim {edge:?} of {item} by {}", delta.value),
            Op::SetRange { item, .. } => format!("Set range of {item}"),
            Op::ReplaceRaw { item, .. } => format!("Replace {item}"),
            Op::Slip { item, delta } => format!("Slip {item} by {}", delta.value),
            Op::SetVideo { item, .. } => format!("Set video properties of {item}"),
            Op::SetAudio { item, .. } => format!("Set audio properties of {item}"),
            Op::SetTrack { track, .. } => format!("Set track {track}"),
            Op::MoveTrack { track, index } => format!("Move track {track} to {index}"),
            Op::SetProxy { media, .. } => format!("Set proxy of {media}"),
            Op::SetTitle { item, .. } => format!("Edit title {item}"),
            Op::SetAnimTrack { item, prop, keys } => {
                if keys.is_empty() {
                    format!("Remove {prop} animation of {item}")
                } else {
                    format!("Animate {prop} of {item} ({} keys)", keys.len())
                }
            }
            Op::SetRetime { item, retime } => {
                if retime.freeze {
                    format!("Freeze frame of {item}")
                } else {
                    format!(
                        "Set speed of {item} to {:.0}%{}",
                        retime.speed * 100.0,
                        if retime.reverse { " reversed" } else { "" }
                    )
                }
            }
            Op::SetTransition { item, transition } => match transition {
                Some(t) => format!("{} before {item} ({} frames)", t.kind.label(), t.duration),
                None => format!("Remove transition before {item}"),
            },
            Op::SetEffects { item, effects } => {
                format!("Set effects of {item} ({} total)", effects.len())
            }
            Op::SetLink { item, link } => match link {
                Some(_) => format!("Link {item}"),
                None => format!("Unlink {item}"),
            },
            Op::SetEnabled { item, enabled } => {
                format!("{} {item}", if *enabled { "Enable" } else { "Disable" })
            }
            Op::SetName { item, name } => format!("Rename {item} to {name}"),
            Op::Transaction { description, .. } => description.clone(),
        }
    }
}
