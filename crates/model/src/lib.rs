//! Project model (docs/03).
//!
//! Plain data with stable [`Id`]s. Snapshots are cheap to clone: timelines and
//! tracks sit behind `Arc` and are copy-on-write, so an edit only copies the
//! track it touches and older snapshots stay valid for readers on other threads.
//! The model has no editing logic; all changes go through the `ops` crate.

mod id;
mod project;

pub use id::Id;
pub use project::*;
