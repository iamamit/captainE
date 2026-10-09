//! Git for edits (docs/07).
//!
//! * [`Repo`]: commits (a list of ops on top of a parent), branches, the commit graph.
//! * [`diff`]: what differs between two states of the project, matched by stable ids.
//! * [`merge`]: three-way merge of two branches with a common ancestor, with conflicts and per-change choices.
//! * [`sync_ops`]: the ordinary ops that turn one project state into another, so a merge is an editable,
//!   undoable, readable commit like any other.

mod diff;
mod merge;
mod repo;
mod sync;

pub use diff::{diff, summarize, Change, ChangeKind, Target, What};
pub use merge::{merge, plan_merge, Choice, MergeOutcome, MergePlan, PlanEntry, Status};
pub use repo::{Commit, CommitId, Repo, RepoError};
pub use sync::sync_ops;
