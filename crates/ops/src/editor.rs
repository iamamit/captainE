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
}

/// A project plus its undo/redo history. Every change goes through [`Editor::apply`].
#[derive(Clone, Debug)]
pub struct Editor {
    project: Project,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl Editor {
    pub fn new(project: Project) -> Editor {
        Editor {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
        }
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
        let (next, inverse) = apply(&self.project, &op)?;
        self.project = next;
        self.undo.push(HistoryEntry {
            author,
            op,
            inverse,
        });
        self.redo.clear();
        Ok(())
    }

    /// Reverts the last op. Returns false if there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        let (prev, _) = apply(&self.project, &entry.inverse)
            .expect("an inverse always applies to the state its op produced");
        self.project = prev;
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
        self.undo.push(entry);
        true
    }
}
