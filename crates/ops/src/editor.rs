use captaine_model::Project;
use serde::{Deserialize, Serialize};

use crate::{apply, Op, OpError};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Author {
    User,
    Agent(String),
    Script(String),
    Merge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub author: Author,
    pub op: Op,
    pub inverse: Op,
    /// Set while a drag is in progress so that following edits with the same key fold into this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_key: Option<String>,
}

/// A project plus its undo/redo history. Every change goes through [`Editor::apply`].
#[derive(Clone, Debug)]
pub struct Editor {
    project: Project,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    revision: u64,
}

impl Editor {
    pub fn new(project: Project) -> Editor {
        Editor {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }

    /// Increases on every change to the project (edit, undo, redo); lets a UI know when to refresh.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Cheap copy-on-write snapshot for render threads and comparisons.
    pub fn snapshot(&self) -> Project {
        self.project.clone()
    }

    /// Applied operations, oldest first (undone ones excluded).
    pub fn history(&self) -> &[HistoryEntry] {
        &self.undo
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Applies an op. On error nothing changes. A successful edit clears the redo stack.
    pub fn apply(&mut self, author: Author, op: Op) -> Result<(), OpError> {
        if crate::lock::locked_hit(&self.project, &op).is_some() {
            return Err(OpError::OutOfRange("that track is locked"));
        }
        let (next, inverse) = apply(&self.project, &op)?;
        self.project = next;
        self.revision += 1;
        self.undo.push(HistoryEntry {
            author,
            op,
            inverse,
            merge_key: None,
        });
        self.redo.clear();
        Ok(())
    }

    /// Like [`apply`](Self::apply), but if the previous edit used the same `key` (and author) the two become
    /// one undo step. For dragging a slider: every tick is applied at once, and one undo reverts the drag.
    ///
    /// Only for ops that *set* absolute values (`SetVideo`, `SetAudio`, `SetTrack`), so the latest op alone
    /// still reproduces the final state when the log is replayed.
    pub fn apply_merging(&mut self, author: Author, op: Op, key: &str) -> Result<(), OpError> {
        if crate::lock::locked_hit(&self.project, &op).is_some() {
            return Err(OpError::OutOfRange("that track is locked"));
        }
        let (next, inverse) = apply(&self.project, &op)?;
        self.project = next;
        self.revision += 1;
        self.redo.clear();
        match self.undo.last_mut() {
            Some(last) if last.merge_key.as_deref() == Some(key) && last.author == author => {
                last.op = op
            }
            _ => self.undo.push(HistoryEntry {
                author,
                op,
                inverse,
                merge_key: Some(key.to_string()),
            }),
        }
        Ok(())
    }

    /// Ends a merge run: the next edit starts a new undo step even if it has the same key.
    pub fn end_merge(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.merge_key = None;
        }
    }

    /// Reverts the last op. Returns false if there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let (prev, _) = apply(&self.project, &entry.inverse)
            .expect("an inverse always applies to the state its op produced");
        self.project = prev;
        self.revision += 1;
        self.redo.push(entry);
        true
    }

    /// Re-applies the last undone op.
    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let (next, _) = apply(&self.project, &entry.op)
            .expect("a redo applies to the state it was undone from");
        self.project = next;
        self.revision += 1;
        self.undo.push(entry);
        true
    }
}
