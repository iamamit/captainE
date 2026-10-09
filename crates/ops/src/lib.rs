//! Operations: the only way to change a project (docs/02, 03).
//!
//! `apply(project, op)` is pure. It returns the new snapshot plus the *inverse* op,
//! which is what makes undo, history, diff and agent tools one mechanism.

mod apply;
pub mod edit;
mod editor;
mod op;

pub use apply::{apply, OpError};
pub use editor::{Author, Editor, HistoryEntry};
pub use op::{Edge, Op};
