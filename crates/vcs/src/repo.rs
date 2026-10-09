use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Mutex;

use captaine_model::{Project, ProjectSettings};
use captaine_ops::{apply, Author, Op};
use serde::{Deserialize, Serialize};

/// Content address of a commit (BLAKE3 over its parents, author, message, time and ops).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitId(String);

impl CommitId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn short(&self) -> &str {
        &self.0[..8.min(self.0.len())]
    }
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short())
    }
}

impl fmt::Debug for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Commit({})", self.short())
    }
}

/// A step in the history: `ops` applied to the state of the first parent give this commit's state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub id: CommitId,
    /// Empty for the root; two for a merge (the branch merged into first, the branch merged in second).
    pub parents: Vec<CommitId>,
    pub author: Author,
    pub message: String,
    /// Seconds since the Unix epoch
    pub time: u64,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepoError {
    NoSuchBranch(String),
    NoSuchCommit(String),
    BranchExists(String),
    BadName(String),
    CannotDeleteCurrent,
    /// A commit's ops do not apply to its parent's state: the history is corrupt
    Replay(String),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RepoError::NoSuchBranch(b) => write!(f, "there is no branch '{b}'"),
            RepoError::NoSuchCommit(c) => write!(f, "there is no commit '{c}'"),
            RepoError::BranchExists(b) => write!(f, "a branch named '{b}' already exists"),
            RepoError::BadName(b) => write!(
                f,
                "'{b}' is not a usable branch name (no spaces, not empty)"
            ),
            RepoError::CannotDeleteCurrent => write!(f, "you cannot delete the branch you are on"),
            RepoError::Replay(e) => write!(f, "history does not replay: {e}"),
        }
    }
}
impl std::error::Error for RepoError {}

/// All commits, the branches pointing into them and the branch that is checked out.
#[derive(Serialize, Deserialize)]
pub struct Repo {
    settings: ProjectSettings,
    commits: BTreeMap<CommitId, Commit>,
    branches: BTreeMap<String, CommitId>,
    head: String,
    root: CommitId,
    #[serde(skip)]
    cache: Mutex<HashMap<CommitId, Project>>,
}

impl Clone for Repo {
    fn clone(&self) -> Repo {
        Repo {
            settings: self.settings.clone(),
            commits: self.commits.clone(),
            branches: self.branches.clone(),
            head: self.head.clone(),
            root: self.root.clone(),
            cache: Mutex::new(self.cache.lock().expect("cache").clone()),
        }
    }
}

impl fmt::Debug for Repo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Repo({} commits, branches {:?}, on {})",
            self.commits.len(),
            self.branches.keys().collect::<Vec<_>>(),
            self.head
        )
    }
}

fn hash_commit(
    parents: &[CommitId],
    author: &Author,
    message: &str,
    time: u64,
    ops: &[Op],
) -> CommitId {
    let body =
        serde_json::to_vec(&(parents, author, message, time, ops)).expect("commits serialize");
    CommitId(blake3::hash(&body).to_hex().to_string())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(char::is_whitespace)
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("..")
}

impl Repo {
    /// A new repository whose root commit is the empty project, with the branch `main` checked out.
    pub fn new(settings: ProjectSettings, time: u64) -> Repo {
        let root_commit = Commit {
            id: hash_commit(&[], &Author::User, "Start", time, &[]),
            parents: vec![],
            author: Author::User,
            message: "Start".into(),
            time,
            ops: vec![],
        };
        let root = root_commit.id.clone();
        Repo {
            settings,
            commits: BTreeMap::from([(root.clone(), root_commit)]),
            branches: BTreeMap::from([("main".to_string(), root.clone())]),
            head: "main".into(),
            root,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn settings(&self) -> &ProjectSettings {
        &self.settings
    }

    pub fn head_branch(&self) -> &str {
        &self.head
    }

    pub fn head_commit(&self) -> &CommitId {
        &self.branches[&self.head]
    }

    pub fn branch_head(&self, branch: &str) -> Result<&CommitId, RepoError> {
        self.branches
            .get(branch)
            .ok_or_else(|| RepoError::NoSuchBranch(branch.into()))
    }

    pub fn branches(&self) -> impl Iterator<Item = (&String, &CommitId)> {
        self.branches.iter()
    }

    pub fn commit(&self, id: &CommitId) -> Result<&Commit, RepoError> {
        self.commits
            .get(id)
            .ok_or_else(|| RepoError::NoSuchCommit(id.to_string()))
    }

    /// Find a commit by full id or unique prefix.
    pub fn find_commit(&self, prefix: &str) -> Result<&Commit, RepoError> {
        let hits: Vec<_> = self
            .commits
            .keys()
            .filter(|k| k.as_str().starts_with(prefix))
            .collect();
        match hits.as_slice() {
            [one] if !prefix.is_empty() => self.commit(one),
            _ => Err(RepoError::NoSuchCommit(prefix.into())),
        }
    }

    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }

    /// The project as it was at `id`.
    pub fn snapshot(&self, id: &CommitId) -> Result<Project, RepoError> {
        if let Some(p) = self.cache.lock().expect("cache").get(id) {
            return Ok(p.clone());
        }
        // walk first parents back to the nearest cached state (or the root), then replay forward
        let mut chain = Vec::new();
        let mut cur = id.clone();
        let mut state = loop {
            if let Some(p) = self.cache.lock().expect("cache").get(&cur) {
                break p.clone();
            }
            let c = self.commit(&cur)?;
            match c.parents.first() {
                None => break Project::new(self.settings.clone()),
                Some(parent) => {
                    chain.push(c);
                    cur = parent.clone();
                }
            }
        };
        for c in chain.into_iter().rev() {
            for op in &c.ops {
                state = apply(&state, op)
                    .map_err(|e| RepoError::Replay(format!("commit {}: {e}", c.id)))?
                    .0;
            }
            self.cache
                .lock()
                .expect("cache")
                .insert(c.id.clone(), state.clone());
        }
        Ok(state)
    }

    pub fn head_snapshot(&self) -> Result<Project, RepoError> {
        self.snapshot(self.head_commit())
    }

    /// Add a commit on `branch` (which advances it). `ops` must apply to the branch's current state.
    pub fn commit_on(
        &mut self,
        branch: &str,
        author: Author,
        message: &str,
        ops: Vec<Op>,
        time: u64,
    ) -> Result<CommitId, RepoError> {
        let parent = self.branch_head(branch)?.clone();
        self.add_commit(branch, vec![parent], author, message, ops, time)
    }

    /// Commit on the checked-out branch.
    pub fn commit_head(
        &mut self,
        author: Author,
        message: &str,
        ops: Vec<Op>,
        time: u64,
    ) -> Result<CommitId, RepoError> {
        let b = self.head.clone();
        self.commit_on(&b, author, message, ops, time)
    }

    /// A merge commit on `into`: parents are `[into head, from]`, `ops` turn the first parent's state into the merge result.
    pub fn merge_commit(
        &mut self,
        into: &str,
        from: &CommitId,
        author: Author,
        message: &str,
        ops: Vec<Op>,
        time: u64,
    ) -> Result<CommitId, RepoError> {
        self.commit(from)?;
        let parent = self.branch_head(into)?.clone();
        self.add_commit(into, vec![parent, from.clone()], author, message, ops, time)
    }

    fn add_commit(
        &mut self,
        branch: &str,
        parents: Vec<CommitId>,
        author: Author,
        message: &str,
        ops: Vec<Op>,
        time: u64,
    ) -> Result<CommitId, RepoError> {
        // refuse commits that do not replay, so the stored history is always sound
        let mut state = self.snapshot(&parents[0])?;
        for op in &ops {
            state = apply(&state, op)
                .map_err(|e| RepoError::Replay(e.to_string()))?
                .0;
        }
        let id = hash_commit(&parents, &author, message, time, &ops);
        if !self.commits.contains_key(&id) {
            let c = Commit {
                id: id.clone(),
                parents,
                author,
                message: message.into(),
                time,
                ops,
            };
            self.commits.insert(id.clone(), c);
        }
        self.cache.lock().expect("cache").insert(id.clone(), state);
        self.branches.insert(branch.to_string(), id.clone());
        Ok(id)
    }

    pub fn create_branch(&mut self, name: &str, at: &CommitId) -> Result<(), RepoError> {
        if !valid_name(name) {
            return Err(RepoError::BadName(name.into()));
        }
        if self.branches.contains_key(name) {
            return Err(RepoError::BranchExists(name.into()));
        }
        self.commit(at)?;
        self.branches.insert(name.to_string(), at.clone());
        Ok(())
    }

    pub fn delete_branch(&mut self, name: &str) -> Result<(), RepoError> {
        if name == self.head {
            return Err(RepoError::CannotDeleteCurrent);
        }
        self.branches
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| RepoError::NoSuchBranch(name.into()))
    }

    pub fn rename_branch(&mut self, from: &str, to: &str) -> Result<(), RepoError> {
        if !valid_name(to) {
            return Err(RepoError::BadName(to.into()));
        }
        if self.branches.contains_key(to) {
            return Err(RepoError::BranchExists(to.into()));
        }
        let id = self
            .branches
            .remove(from)
            .ok_or_else(|| RepoError::NoSuchBranch(from.into()))?;
        self.branches.insert(to.to_string(), id);
        if self.head == from {
            self.head = to.to_string();
        }
        Ok(())
    }

    /// Make `branch` the checked-out one.
    pub fn checkout(&mut self, branch: &str) -> Result<(), RepoError> {
        self.branch_head(branch)?;
        self.head = branch.to_string();
        Ok(())
    }

    /// Move `branch` forward to `to`, which must descend from it.
    pub fn fast_forward(&mut self, branch: &str, to: &CommitId) -> Result<(), RepoError> {
        let cur = self.branch_head(branch)?.clone();
        if !self.is_ancestor(&cur, to) {
            return Err(RepoError::Replay("not a fast-forward".into()));
        }
        self.branches.insert(branch.to_string(), to.clone());
        Ok(())
    }

    /// Every commit reachable from `id`, itself included.
    pub fn ancestors(&self, id: &CommitId) -> HashSet<CommitId> {
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([id.clone()]);
        while let Some(c) = queue.pop_front() {
            if !seen.insert(c.clone()) {
                continue;
            }
            if let Some(commit) = self.commits.get(&c) {
                queue.extend(commit.parents.iter().cloned());
            }
        }
        seen
    }

    pub fn is_ancestor(&self, maybe_ancestor: &CommitId, of: &CommitId) -> bool {
        self.ancestors(of).contains(maybe_ancestor)
    }

    /// Longest distance from the root.
    fn generation(&self, id: &CommitId, memo: &mut HashMap<CommitId, usize>) -> usize {
        if let Some(g) = memo.get(id) {
            return *g;
        }
        let g = self.commits.get(id).map_or(0, |c| {
            c.parents
                .iter()
                .map(|p| self.generation(p, memo) + 1)
                .max()
                .unwrap_or(0)
        });
        memo.insert(id.clone(), g);
        g
    }

    /// The best common ancestor of two commits: the one no other common ancestor descends from
    /// (the latest, if there are several).
    pub fn merge_base(&self, a: &CommitId, b: &CommitId) -> Option<CommitId> {
        let (sa, sb) = (self.ancestors(a), self.ancestors(b));
        let common: Vec<&CommitId> = sa.intersection(&sb).collect();
        let mut memo = HashMap::new();
        common
            .iter()
            .filter(|c| !common.iter().any(|d| d != *c && self.is_ancestor(c, d)))
            .max_by_key(|c| (self.generation(c, &mut memo), (**c).clone()))
            .map(|c| (*c).clone())
    }

    /// History of a branch, newest first. Merged-in commits appear in time order among the others.
    pub fn log(&self, branch: &str) -> Result<Vec<&Commit>, RepoError> {
        let head = self.branch_head(branch)?;
        let mut commits: Vec<&Commit> = self
            .ancestors(head)
            .iter()
            .filter_map(|id| self.commits.get(id))
            .collect();
        let mut memo = HashMap::new();
        commits.sort_by_key(|c| std::cmp::Reverse((self.generation(&c.id, &mut memo), c.time)));
        Ok(commits)
    }

    /// Branches whose head is exactly `id`.
    pub fn branches_at(&self, id: &CommitId) -> Vec<&str> {
        self.branches
            .iter()
            .filter(|(_, v)| *v == id)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use captaine_model::*;
    use captaine_time::{Rational, RationalTime};

    use super::*;

    fn settings() -> ProjectSettings {
        ProjectSettings {
            frame_rate: Rational::FPS_24,
            width: 640,
            height: 360,
            sample_rate: 48_000,
        }
    }

    fn gap_op(n: u128) -> Op {
        // an op that always applies to any state of these tests: a media entry with a distinct id
        Op::AddMedia {
            media: MediaRef {
                id: Id::from_u128(n),
                name: format!("m{n}"),
                path: format!("/m{n}"),
                duration: Some(RationalTime::new(10, Rational::FPS_24)),
                has_audio: false,
                width: 4,
                height: 4,
                has_video: true,
                proxy: None,
            },
        }
    }

    fn names(p: &Project) -> Vec<String> {
        p.media.values().map(|m| m.name.clone()).collect()
    }

    #[test]
    fn commits_branches_and_snapshots() {
        let mut r = Repo::new(settings(), 1);
        assert_eq!((r.head_branch(), r.commit_count()), ("main", 1));
        let a = r
            .commit_head(Author::User, "one", vec![gap_op(1)], 2)
            .unwrap();
        r.create_branch("try", &a).unwrap();
        let b = r
            .commit_head(Author::User, "two", vec![gap_op(2)], 3)
            .unwrap();
        r.checkout("try").unwrap();
        let c = r
            .commit_head(Author::Agent("copilot".into()), "other", vec![gap_op(3)], 4)
            .unwrap();
        assert_eq!(names(&r.snapshot(&a).unwrap()), ["m1"]);
        assert_eq!(names(&r.snapshot(&b).unwrap()), ["m1", "m2"]);
        assert_eq!(names(&r.snapshot(&c).unwrap()), ["m1", "m3"]);
        assert_eq!(r.branch_head("main").unwrap(), &b);
        assert_eq!(r.merge_base(&b, &c), Some(a.clone()));
        assert!(r.is_ancestor(&a, &b) && !r.is_ancestor(&b, &c));
        // log is newest first, per branch
        let msgs: Vec<_> = r
            .log("try")
            .unwrap()
            .iter()
            .map(|c| c.message.clone())
            .collect();
        assert_eq!(msgs, ["other", "one", "Start"]);
        assert_eq!(
            r.commit(&c).unwrap().author,
            Author::Agent("copilot".into())
        );
        // identical content hashes to the same id, so re-committing it adds nothing
        let mut r2 = Repo::new(settings(), 1);
        let a2 = r2
            .commit_head(Author::User, "one", vec![gap_op(1)], 2)
            .unwrap();
        assert_eq!(a2, a);
    }

    #[test]
    fn merge_commits_and_base_selection() {
        let mut r = Repo::new(settings(), 1);
        let a = r
            .commit_head(Author::User, "a", vec![gap_op(1)], 2)
            .unwrap();
        r.create_branch("feature", &a).unwrap();
        let f1 = r
            .commit_on("feature", Author::User, "f1", vec![gap_op(2)], 3)
            .unwrap();
        let m1 = r
            .commit_on("main", Author::User, "m1", vec![gap_op(3)], 4)
            .unwrap();
        assert_eq!(r.merge_base(&m1, &f1), Some(a));
        // merge feature into main: ops are relative to main's state
        let merged = r
            .merge_commit(
                "main",
                &f1,
                Author::Merge,
                "Merge feature",
                vec![gap_op(2)],
                5,
            )
            .unwrap();
        assert_eq!(names(&r.snapshot(&merged).unwrap()), ["m1", "m2", "m3"]);
        assert_eq!(r.commit(&merged).unwrap().parents, [m1.clone(), f1.clone()]);
        assert!(r.is_ancestor(&f1, &merged));
        // after the merge the best common ancestor of the two heads is the feature head itself
        assert_eq!(r.merge_base(&merged, &f1), Some(f1.clone()));
        // feature moves on; the next base is the old feature head, reachable only through the merge's second parent
        let f2 = r
            .commit_on("feature", Author::User, "f2", vec![gap_op(4)], 6)
            .unwrap();
        assert_eq!(r.merge_base(&merged, &f2), Some(f1));
        assert_eq!(r.log("main").unwrap().len(), 5);
    }

    #[test]
    fn bad_input_is_refused_cleanly() {
        let mut r = Repo::new(settings(), 1);
        let root = r.head_commit().clone();
        assert!(matches!(
            r.create_branch("two words", &root),
            Err(RepoError::BadName(_))
        ));
        assert!(matches!(
            r.create_branch("", &root),
            Err(RepoError::BadName(_))
        ));
        assert!(matches!(
            r.create_branch("main", &root),
            Err(RepoError::BranchExists(_))
        ));
        r.create_branch("agent/tighten-pauses", &root).unwrap();
        assert!(matches!(
            r.checkout("nope"),
            Err(RepoError::NoSuchBranch(_))
        ));
        assert!(matches!(
            r.delete_branch("main"),
            Err(RepoError::CannotDeleteCurrent)
        ));
        r.delete_branch("agent/tighten-pauses").unwrap();
        assert!(matches!(
            r.delete_branch("agent/tighten-pauses"),
            Err(RepoError::NoSuchBranch(_))
        ));
        // a commit whose ops do not apply is rejected, leaving the history as it was
        let bad = Op::RemoveItem {
            item: Id::from_u128(99),
        };
        assert!(matches!(
            r.commit_head(Author::User, "bad", vec![bad], 2),
            Err(RepoError::Replay(_))
        ));
        assert_eq!(r.commit_count(), 1);
        assert!(matches!(
            r.find_commit("zzzz"),
            Err(RepoError::NoSuchCommit(_))
        ));
        assert_eq!(r.find_commit(&root.short()[..6]).unwrap().id, root);
        r.rename_branch("main", "master").unwrap();
        assert_eq!(r.head_branch(), "master");
    }

    #[test]
    fn fast_forward_and_serialization() {
        let mut r = Repo::new(settings(), 1);
        let root = r.head_commit().clone();
        r.create_branch("side", &root).unwrap();
        let a = r
            .commit_head(Author::User, "a", vec![gap_op(1)], 2)
            .unwrap();
        r.fast_forward("side", &a).unwrap();
        assert_eq!(r.branch_head("side").unwrap(), &a);
        assert!(
            r.fast_forward("side", &root).is_err(),
            "cannot go backwards"
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: Repo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.branch_head("main").unwrap(), &a);
        assert_eq!(
            names(&back.head_snapshot().unwrap()),
            ["m1"],
            "snapshots rebuild from the ops"
        );
        assert_eq!(r.branches_at(&a), ["main", "side"]);
    }
}
