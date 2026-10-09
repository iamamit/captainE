//! Project file: settings plus the operation log. State is rebuilt by replaying the log,
//! so autosave is "append an op" and undo/redo survive restarts (docs/03).

use std::fs;
use std::path::Path;

use captaine_model::{Project, ProjectSettings};
use captaine_ops::{Author, Editor, HistoryEntry, Op};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    settings: ProjectSettings,
    applied: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

pub struct Session {
    pub editor: Editor,
    settings: ProjectSettings,
    redo: Vec<HistoryEntry>,
}

impl Session {
    pub fn new(settings: ProjectSettings) -> Session {
        Session {
            editor: Editor::new(Project::new(settings.clone())),
            settings,
            redo: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Session, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let f: File = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a captaine project: {e}", path.display()))?;
        if f.version != 1 {
            return Err(format!("unsupported project version {}", f.version));
        }
        let mut s = Session::new(f.settings);
        for e in f.applied {
            s.editor
                .apply(e.author, e.op)
                .map_err(|e| format!("project log does not replay: {e}"))?;
        }
        s.redo = f.redo;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let f = File {
            version: 1,
            settings: self.settings.clone(),
            applied: self.editor.history().to_vec(),
            redo: self.redo.clone(),
        };
        let json = serde_json::to_string_pretty(&f).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn apply(&mut self, op: Op) -> Result<(), String> {
        self.editor
            .apply(Author::User, op)
            .map_err(|e| e.to_string())?;
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self) -> Option<String> {
        let entry = self.editor.history().last()?.clone();
        self.editor.undo();
        let d = entry.op.describe();
        self.redo.push(entry);
        Some(d)
    }

    pub fn redo(&mut self) -> Option<String> {
        let entry = self.redo.pop()?;
        let d = entry.op.describe();
        self.editor
            .apply(entry.author, entry.op)
            .expect("redo of an undone op applies");
        Some(d)
    }
}
