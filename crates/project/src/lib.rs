//! The open project (docs/03, 07).
//!
//! A project is a [`Repo`] (commits and branches) plus *working edits*: the operations made since the last
//! commit, held in an [`Editor`] so undo and redo work. Committing turns them into a commit on the current branch;
//! switching branch, merging and Copilot's branch edits commit them first, so nothing is ever lost.
//!
//! The file is JSON: the repo plus the working edits. Files from before branches (version 1) open as a project
//! with one branch, `main`, whose working edits are the old log.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use captaine_model::{Project, ProjectSettings};
use captaine_ops::{Author, Editor, HistoryEntry, Op};
use captaine_vcs::{
    diff, merge, plan_merge, summarize, sync_ops, Change, Choice, CommitId, MergePlan, Repo,
};
use serde::{Deserialize, Serialize};

pub use captaine_vcs;

#[derive(Serialize, Deserialize)]
struct FileV1 {
    version: u32,
    settings: ProjectSettings,
    applied: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// What gets written: borrows the repo instead of copying it, since this runs after every edit.
#[derive(Serialize)]
struct FileV2Ref<'a> {
    version: u32,
    repo: &'a Repo,
    working: Vec<HistoryEntry>,
    redo: &'a [HistoryEntry],
}

#[derive(Serialize, Deserialize)]
struct FileV2 {
    version: u32,
    repo: Repo,
    working: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// `<project>.bak`: the version before the latest save.
pub fn backup_path(path: &Path) -> std::path::PathBuf {
    let mut n = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    n.push(".bak");
    path.with_file_name(n)
}

fn snapshots_dir(path: &Path) -> std::path::PathBuf {
    let mut n = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    n.push(".backups");
    path.with_file_name(n)
}

fn snapshot_time(p: &Path) -> Option<u64> {
    p.file_stem()?.to_str()?.parse().ok()
}

/// Timed snapshots of a project, oldest first.
pub fn snapshots(path: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<(u64, std::path::PathBuf)> = fs::read_dir(snapshots_dir(path))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| snapshot_time(&p).map(|t| (t, p)))
        .collect();
    v.sort();
    v.into_iter().map(|(_, p)| p).collect()
}

/// Keep the version about to be replaced as `.bak`, but not more often than every 20 seconds, so a burst of
/// edits does not copy a big file each time yet a bad write never costs more than a short while of work.
fn keep_previous(path: &Path) {
    let bak = backup_path(path);
    let recent = fs::metadata(&bak)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 20);
    if !recent && path.exists() && Session::load(path).is_ok() {
        let _ = fs::copy(path, &bak);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One line of a branch list.
#[derive(Clone, Debug, PartialEq)]
pub struct BranchInfo {
    pub name: String,
    pub head: CommitId,
    pub current: bool,
    /// Commits on this branch that the current branch does not have
    pub ahead: usize,
    /// Commits on the current branch that this one does not have
    pub behind: usize,
    pub last_message: String,
    pub last_author: Author,
    pub last_time: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommitInfo {
    pub id: CommitId,
    pub message: String,
    pub author: Author,
    pub time: u64,
    pub ops: usize,
    pub is_merge: bool,
    pub branches: Vec<String>,
}

/// Everything needed to review a merge before doing it.
pub struct MergeSetup {
    pub from: String,
    pub into: String,
    pub base: Project,
    pub ours: Project,
    pub theirs: Project,
    pub plan: MergePlan,
    /// Their branch is already part of ours
    pub up_to_date: bool,
    /// Ours is part of theirs: the merge just moves our branch forward
    pub fast_forward: bool,
}

#[derive(Debug, PartialEq)]
pub enum MergeReport {
    UpToDate,
    FastForward,
    Merged {
        commit: CommitId,
        applied: usize,
        skipped: Vec<(String, String)>,
    },
}

pub struct Session {
    pub editor: Editor,
    repo: Repo,
    redo: Vec<HistoryEntry>,
    /// `editor.history()` entries before this index are already in a commit
    boundary: usize,
    /// Undo went back past the last commit: the working state is then described by a state difference
    diverged: bool,
    /// Bumped whenever the editor is replaced wholesale (switching branch, merging, opening), so a UI knows to refresh
    epoch: u64,
}

impl Session {
    pub fn new(settings: ProjectSettings) -> Session {
        let repo = Repo::new(settings.clone(), now());
        Session {
            editor: Editor::new(Project::new(settings)),
            repo,
            redo: Vec::new(),
            boundary: 0,
            diverged: false,
            epoch: 0,
        }
    }

    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// Changes whenever the whole project in the editor was swapped for another (see `checkout`, `merge`).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    // ---- files -----------------------------------------------------------------------------------------

    pub fn load(path: &Path) -> Result<Session, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let not_ours =
            |e: serde_json::Error| format!("{} is not a captaine project: {e}", path.display());
        let version = serde_json::from_str::<serde_json::Value>(&text).map_err(not_ours)?
            ["version"]
            .as_u64()
            .unwrap_or(0);
        match version {
            1 => {
                let f: FileV1 = serde_json::from_str(&text).map_err(not_ours)?;
                let mut s = Session::new(f.settings);
                for e in f.applied {
                    s.editor
                        .apply(e.author, e.op)
                        .map_err(|e| format!("project log does not replay: {e}"))?;
                }
                s.redo = f.redo;
                Ok(s)
            }
            2 => {
                let f: FileV2 = serde_json::from_str(&text).map_err(not_ours)?;
                let head = f.repo.head_snapshot().map_err(|e| e.to_string())?;
                let mut s = Session {
                    editor: Editor::new(head),
                    repo: f.repo,
                    redo: f.redo,
                    boundary: 0,
                    diverged: false,
                    epoch: 0,
                };
                for e in f.working {
                    s.editor
                        .apply(e.author, e.op)
                        .map_err(|e| format!("working edits do not replay: {e}"))?;
                }
                Ok(s)
            }
            v => Err(format!("unsupported project version {v}")),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let f = FileV2Ref {
            version: 2,
            repo: &self.repo,
            working: self.working_entries(),
            redo: &self.redo,
        };
        let json = serde_json::to_string_pretty(&f).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        keep_previous(path);
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Open a project; when the file is damaged, fall back to the last good copy (`.bak`) or the newest
    /// timed snapshot, and say so. The second value is that note.
    pub fn load_recovering(path: &Path) -> Result<(Session, Option<String>), String> {
        let first = match Session::load(path) {
            Ok(s) => return Ok((s, None)),
            Err(e) => e,
        };
        let mut tries = vec![backup_path(path)];
        tries.extend(snapshots(path).into_iter().rev());
        for b in tries {
            if let Ok(s) = Session::load(&b) {
                let note = format!(
                    "{} could not be read ({first}); opened the backup {} instead",
                    path.display(),
                    b.file_name()
                        .map_or(String::new(), |n| n.to_string_lossy().into_owned())
                );
                return Ok((s, Some(note)));
            }
        }
        Err(first)
    }

    /// Write a timed snapshot next to the project (`<project>.backups/<seconds>.json`), keeping the newest
    /// `keep`. A snapshot is only taken when the last one is older than `min_age_secs`.
    pub fn snapshot(&self, path: &Path, min_age_secs: u64, keep: usize) -> Result<bool, String> {
        let existing = snapshots(path);
        let now = now();
        if let Some(last) = existing.last().and_then(|p| snapshot_time(p)) {
            if now.saturating_sub(last) < min_age_secs {
                return Ok(false);
            }
        }
        let dir = snapshots_dir(path);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        self.save(&dir.join(format!("{now}.json")))?;
        let all = snapshots(path);
        for old in all.iter().take(all.len().saturating_sub(keep)) {
            let _ = fs::remove_file(old);
        }
        Ok(true)
    }

    // ---- editing (the working state) ------------------------------------------------------------------------

    pub fn apply(&mut self, op: Op) -> Result<(), String> {
        self.apply_as(Author::User, op)
    }

    pub fn apply_as(&mut self, author: Author, op: Op) -> Result<(), String> {
        self.editor.apply(author, op).map_err(|e| e.to_string())?;
        self.redo.clear();
        Ok(())
    }

    /// Apply a slider-style edit that folds into the previous one when `key` matches (see `Editor::apply_merging`).
    pub fn apply_merging(&mut self, op: Op, key: &str) -> Result<(), String> {
        self.editor
            .apply_merging(Author::User, op, key)
            .map_err(|e| e.to_string())?;
        self.redo.clear();
        Ok(())
    }

    pub fn end_merge(&mut self) {
        self.editor.end_merge();
    }

    pub fn undo(&mut self) -> Option<String> {
        let len = self.editor.history().len();
        let entry = self.editor.history().last()?.clone();
        if len <= self.boundary {
            self.boundary = len - 1;
            self.diverged = true;
        }
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

    // ---- commits ----------------------------------------------------------------------------------------------

    fn working_entries(&self) -> Vec<HistoryEntry> {
        let h = self.editor.history();
        if self.diverged || self.boundary > h.len() {
            let Ok(head) = self.repo.head_snapshot() else {
                return Vec::new();
            };
            return match sync_ops(&head, self.editor.project()) {
                Ok(ops) if !ops.is_empty() => vec![HistoryEntry {
                    author: Author::User,
                    op: Op::Transaction {
                        description: "Edits".into(),
                        ops,
                    },
                    inverse: Op::Transaction {
                        description: String::new(),
                        ops: vec![],
                    },
                    merge_key: None,
                }],
                _ => Vec::new(),
            };
        }
        h[self.boundary..].to_vec()
    }

    /// Edits made since the last commit.
    pub fn has_uncommitted(&self) -> bool {
        self.editor.history().len() > self.boundary || self.diverged
    }

    /// How many edits are waiting to be committed.
    pub fn uncommitted_count(&self) -> usize {
        if self.diverged {
            1
        } else {
            self.editor.history().len().saturating_sub(self.boundary)
        }
    }

    fn after_commit(&mut self) {
        self.editor.end_merge();
        self.boundary = self.editor.history().len();
        self.diverged = false;
    }

    /// Put the working edits into a commit on the current branch. `None` if there was nothing to commit.
    pub fn commit(&mut self, message: Option<&str>) -> Result<Option<CommitId>, String> {
        self.editor.end_merge();
        if !self.has_uncommitted() {
            return Ok(None);
        }
        let entries = self.working_entries();
        let mut ops: Vec<Op> = entries.iter().map(|e| e.op.clone()).collect();
        let head = self.repo.head_snapshot().map_err(|e| e.to_string())?;
        // the recorded ops must give exactly what is on screen; if not, fall back to describing the difference
        let replayed = ops.iter().try_fold(head.clone(), |p, op| {
            captaine_ops::apply(&p, op).map(|r| r.0)
        });
        if replayed
            .as_ref()
            .map_or(true, |p| p != self.editor.project())
        {
            ops = sync_ops(&head, self.editor.project())?;
        }
        if ops.is_empty() {
            self.after_commit();
            return Ok(None);
        }
        let author = entries.first().map_or(Author::User, |e| e.author.clone());
        let msg = message
            .map(str::to_string)
            .unwrap_or_else(|| auto_message(&ops));
        let id = self
            .repo
            .commit_head(author, &msg, ops, now())
            .map_err(|e| e.to_string())?;
        self.after_commit();
        Ok(Some(id))
    }

    /// Replace the editor with the project at the head of the current branch (dropping any working edits).
    fn reload(&mut self) -> Result<(), String> {
        let head = self.repo.head_snapshot().map_err(|e| e.to_string())?;
        self.editor = Editor::new(head);
        self.redo.clear();
        self.boundary = 0;
        self.diverged = false;
        self.epoch += 1;
        Ok(())
    }

    // ---- branches ---------------------------------------------------------------------------------------------

    pub fn branch(&self) -> &str {
        self.repo.head_branch()
    }

    pub fn branch_infos(&self) -> Vec<BranchInfo> {
        let cur = self.repo.head_commit().clone();
        let cur_set = self.repo.ancestors(&cur);
        self.repo
            .branches()
            .map(|(name, head)| {
                let set = self.repo.ancestors(head);
                let c = self.repo.commit(head).expect("branch heads exist");
                BranchInfo {
                    name: name.clone(),
                    head: head.clone(),
                    current: name == self.repo.head_branch(),
                    ahead: set.difference(&cur_set).count(),
                    behind: cur_set.difference(&set).count(),
                    last_message: c.message.clone(),
                    last_author: c.author.clone(),
                    last_time: c.time,
                }
            })
            .collect()
    }

    /// A new branch at the current commit. With `switch` it becomes the current branch and the working edits
    /// come along (like `git checkout -b`).
    pub fn create_branch(&mut self, name: &str, switch: bool) -> Result<(), String> {
        let head = self.repo.head_commit().clone();
        self.repo
            .create_branch(name, &head)
            .map_err(|e| e.to_string())?;
        if switch {
            self.repo.checkout(name).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// A new branch at any commit (id prefix), not switched to.
    pub fn create_branch_at(&mut self, name: &str, commit: &str) -> Result<(), String> {
        let id = self
            .repo
            .find_commit(commit)
            .map_err(|e| e.to_string())?
            .id
            .clone();
        self.repo
            .create_branch(name, &id)
            .map_err(|e| e.to_string())
    }

    /// Switch to another branch. Working edits are committed first so they are not lost.
    pub fn checkout(&mut self, branch: &str) -> Result<(), String> {
        if branch == self.branch() {
            return Ok(());
        }
        self.repo.branch_head(branch).map_err(|e| e.to_string())?;
        let here = self.branch().to_string();
        self.commit(Some(&format!("Work in progress on {here}")))?;
        self.repo.checkout(branch).map_err(|e| e.to_string())?;
        self.reload()
    }

    pub fn delete_branch(&mut self, name: &str) -> Result<(), String> {
        self.repo.delete_branch(name).map_err(|e| e.to_string())
    }

    pub fn rename_branch(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.repo.rename_branch(from, to).map_err(|e| e.to_string())
    }

    /// A branch name based on `wanted` that is not taken yet: `agent/tighten-pauses`, `agent/tighten-pauses-2`, ...
    pub fn unique_branch_name(&self, wanted: &str) -> String {
        let taken: Vec<&String> = self.repo.branches().map(|(n, _)| n).collect();
        if !taken.iter().any(|n| n.as_str() == wanted) {
            return wanted.to_string();
        }
        (2..)
            .map(|i| format!("{wanted}-{i}"))
            .find(|n| !taken.contains(&n))
            .expect("some name is free")
    }

    /// Put `ops` (built on the current working state) into a new branch of their own, without switching to it.
    /// This is how Copilot's suggestions are kept for review. Returns the branch name.
    pub fn commit_to_new_branch(
        &mut self,
        wanted: &str,
        author: Author,
        message: &str,
        ops: Vec<Op>,
    ) -> Result<String, String> {
        // the ops were made against what is on screen, so that has to be committed first
        self.commit(Some(&format!("Save before: {message}")))?;
        let name = self.unique_branch_name(wanted);
        let head = self.repo.head_commit().clone();
        self.repo
            .create_branch(&name, &head)
            .map_err(|e| e.to_string())?;
        self.repo
            .commit_on(&name, author, message, ops, now())
            .map_err(|e| e.to_string())?;
        Ok(name)
    }

    // ---- history and comparison -----------------------------------------------------------------------------------

    pub fn log(&self, branch: &str) -> Result<Vec<CommitInfo>, String> {
        let commits = self.repo.log(branch).map_err(|e| e.to_string())?;
        Ok(commits
            .into_iter()
            .map(|c| CommitInfo {
                id: c.id.clone(),
                message: c.message.clone(),
                author: c.author.clone(),
                time: c.time,
                ops: c.ops.len(),
                is_merge: c.parents.len() > 1,
                branches: self
                    .repo
                    .branches_at(&c.id)
                    .into_iter()
                    .map(String::from)
                    .collect(),
            })
            .collect())
    }

    /// The project as of a reference: `working` (what is on screen), a branch name, or a commit id prefix.
    pub fn resolve(&self, reference: &str) -> Result<Project, String> {
        if reference == "working" || reference == "@" {
            return Ok(self.editor.project().clone());
        }
        if let Ok(id) = self.repo.branch_head(reference) {
            return self.repo.snapshot(id).map_err(|e| e.to_string());
        }
        let c = self
            .repo
            .find_commit(reference)
            .map_err(|e| e.to_string())?;
        self.repo.snapshot(&c.id).map_err(|e| e.to_string())
    }

    /// What differs going from reference `a` to reference `b`.
    pub fn compare(&self, a: &str, b: &str) -> Result<Vec<Change>, String> {
        Ok(diff(&self.resolve(a)?, &self.resolve(b)?))
    }

    /// One sentence about what a commit changed compared to its first parent.
    pub fn commit_summary(&self, id: &CommitId) -> String {
        let Ok(c) = self.repo.commit(id) else {
            return String::new();
        };
        let Some(parent) = c.parents.first() else {
            return "Start of the project".into();
        };
        match (self.repo.snapshot(parent), self.repo.snapshot(id)) {
            (Ok(a), Ok(b)) => {
                let rate = b
                    .timelines
                    .values()
                    .next()
                    .map_or(captaine_time::Rational::FPS_30, |t| t.rate);
                summarize(&diff(&a, &b), rate)
            }
            _ => String::new(),
        }
    }

    // ---- merging --------------------------------------------------------------------------------------------------

    /// Look at what merging `from` into the current branch would do. Working edits are committed first.
    pub fn prepare_merge(&mut self, from: &str) -> Result<MergeSetup, String> {
        if from == self.branch() {
            return Err("a branch cannot be merged into itself".into());
        }
        self.repo.branch_head(from).map_err(|e| e.to_string())?;
        let here = self.branch().to_string();
        self.commit(Some(&format!("Work in progress on {here}")))?;
        let (ours_id, theirs_id) = (
            self.repo.head_commit().clone(),
            self.repo
                .branch_head(from)
                .map_err(|e| e.to_string())?
                .clone(),
        );
        let up_to_date = self.repo.is_ancestor(&theirs_id, &ours_id);
        let fast_forward = !up_to_date && self.repo.is_ancestor(&ours_id, &theirs_id);
        let base_id = self
            .repo
            .merge_base(&ours_id, &theirs_id)
            .ok_or("the branches share no history")?;
        let (base, ours, theirs) = (
            self.repo.snapshot(&base_id).map_err(|e| e.to_string())?,
            self.repo.snapshot(&ours_id).map_err(|e| e.to_string())?,
            self.repo.snapshot(&theirs_id).map_err(|e| e.to_string())?,
        );
        let plan = plan_merge(&base, &ours, &theirs);
        Ok(MergeSetup {
            from: from.to_string(),
            into: here,
            base,
            ours,
            theirs,
            plan,
            up_to_date,
            fast_forward,
        })
    }

    /// Merge `from` into the current branch. `choices` say, per change (by `Change::id`), whether to take or skip it;
    /// clean changes are taken and conflicts are skipped (ours wins) unless chosen otherwise.
    pub fn merge(
        &mut self,
        from: &str,
        choices: &HashMap<String, Choice>,
        message: Option<&str>,
    ) -> Result<MergeReport, String> {
        let setup = self.prepare_merge(from)?;
        if setup.up_to_date {
            return Ok(MergeReport::UpToDate);
        }
        let theirs_id = self
            .repo
            .branch_head(from)
            .map_err(|e| e.to_string())?
            .clone();
        let takes_all = !choices.values().any(|c| *c == Choice::Skip);
        if setup.fast_forward && takes_all {
            let into = setup.into.clone();
            self.repo
                .fast_forward(&into, &theirs_id)
                .map_err(|e| e.to_string())?;
            self.reload()?;
            return Ok(MergeReport::FastForward);
        }
        let outcome = merge(&setup.base, &setup.ours, &setup.theirs, choices)?;
        let msg = message
            .map(str::to_string)
            .unwrap_or_else(|| format!("Merge {from} into {}", setup.into));
        let into = setup.into.clone();
        let commit = self
            .repo
            .merge_commit(&into, &theirs_id, Author::Merge, &msg, outcome.ops, now())
            .map_err(|e| e.to_string())?;
        self.reload()?;
        Ok(MergeReport::Merged {
            commit,
            applied: outcome.applied.len(),
            skipped: outcome.skipped,
        })
    }
}

/// "Trim, Split and 2 more edits": a default commit message.
pub fn auto_message(ops: &[Op]) -> String {
    let names: Vec<String> = ops.iter().map(|o| o.describe()).collect();
    match names.len() {
        0 => "No changes".into(),
        1 => names[0].clone(),
        2 => format!("{} and {}", names[0], names[1].to_lowercase()),
        n => format!("{} and {} more edits", names[0], n - 1),
    }
}

/// `"Tighten the pauses in intro!"` → `tighten-the-pauses-in-intro`
pub fn slugify(text: &str, max_words: usize) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(max_words)
        .map(|w| w.to_lowercase())
        .collect();
    if words.is_empty() {
        "edit".into()
    } else {
        words.join("-")
    }
}

/// "just now", "5 min ago", "2 h ago", "3 d ago"
pub fn time_ago(secs: u64) -> String {
    let d = now().saturating_sub(secs);
    match d {
        0..=44 => "just now".into(),
        45..=3599 => format!("{} min ago", (d / 60).max(1)),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => format!("{} d ago", d / 86_400),
    }
}

/// Who made a commit, as a person would say it.
pub fn author_label(a: &Author) -> String {
    match a {
        Author::User => "You".into(),
        Author::Agent(n) if n == "copilot" => "Copilot".into(),
        Author::Agent(n) | Author::Script(n) => n.clone(),
        Author::Merge => "Merge".into(),
    }
}
