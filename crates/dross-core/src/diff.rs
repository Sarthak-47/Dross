//! Diff extraction over git2-rs.
//!
//! Produces the hunk set Dross checks operate on. Everything downstream works
//! from `FileDiff`/`Hunk`, so the checks never touch libgit2 directly.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::lang::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    /// Line range in the post-image (the "new" side of the diff).
    pub new_start: usize,
    pub new_lines: usize,
    pub old_start: usize,
    pub old_lines: usize,
}

impl Hunk {
    pub fn new_end(&self) -> usize {
        self.new_start + self.new_lines.saturating_sub(1)
    }

    /// Whether a 1-indexed post-image line falls inside this hunk.
    pub fn contains_new_line(&self, line: usize) -> bool {
        self.new_lines > 0 && line >= self.new_start && line <= self.new_end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
    pub language: Option<Language>,
    pub hunks: Vec<Hunk>,
    /// Post-image source. `None` for deletions and unreadable blobs.
    pub new_source: Option<String>,
    /// Pre-image source. `None` for additions.
    pub old_source: Option<String>,
}

impl FileDiff {
    /// True if any hunk touches this 1-indexed post-image line.
    pub fn touches_line(&self, line: usize) -> bool {
        self.hunks.iter().any(|h| h.contains_new_line(line))
    }

    /// True if any hunk overlaps the inclusive post-image line range.
    pub fn touches_range(&self, start: usize, end: usize) -> bool {
        self.hunks
            .iter()
            .any(|h| h.new_lines > 0 && h.new_start <= end && h.new_end() >= start)
    }

    pub fn changed_line_count(&self) -> usize {
        self.hunks.iter().map(|h| h.new_lines).sum()
    }
}

/// What to diff against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffTarget {
    /// Staged changes vs. HEAD — the pre-commit hook case.
    StagedVsHead,
    /// Working tree vs. HEAD — the file-watcher / live-edit case.
    WorktreeVsHead,
}

pub struct Repo {
    inner: git2::Repository,
}

impl Repo {
    pub fn open(path: &Path) -> Result<Self> {
        let inner = git2::Repository::discover(path)
            .with_context(|| format!("no git repository found at or above {}", path.display()))?;
        Ok(Self { inner })
    }

    pub fn workdir(&self) -> Result<&Path> {
        self.inner
            .workdir()
            .context("bare repositories are not supported")
    }

    /// The checked-out branch name, or `None` on a detached HEAD or an
    /// unborn branch.
    pub fn branch(&self) -> Option<String> {
        let head = self.inner.head().ok()?;
        if !head.is_branch() {
            return None;
        }
        // shorthand() is fallible in this git2 version, not optional.
        let name = head.shorthand().ok()?;
        Some(name.to_string())
    }

    pub fn inner(&self) -> &git2::Repository {
        &self.inner
    }

    pub fn diff(&self, target: DiffTarget) -> Result<Vec<FileDiff>> {
        let mut opts = git2::DiffOptions::new();
        opts.context_lines(0).ignore_whitespace_eol(false);

        let head_tree = match self.inner.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            // Unborn HEAD: a repo with no commits yet. Everything is "added".
            Err(_) => None,
        };

        let diff = match target {
            DiffTarget::StagedVsHead => {
                self.inner
                    .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))?
            }
            DiffTarget::WorktreeVsHead => self
                .inner
                .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))?,
        };

        self.collect(diff, target)
    }

    /// Diff two commits by revspec — used by the benchmark harness to replay
    /// a repo's own history.
    pub fn diff_commits(&self, old_rev: &str, new_rev: &str) -> Result<Vec<FileDiff>> {
        let old_tree = self.inner.revparse_single(old_rev)?.peel_to_tree()?;
        let new_tree = self.inner.revparse_single(new_rev)?.peel_to_tree()?;
        let mut opts = git2::DiffOptions::new();
        opts.context_lines(0);
        let diff =
            self.inner
                .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), Some(&mut opts))?;
        self.collect_from_trees(diff, &old_tree, &new_tree)
    }

    fn collect(&self, diff: git2::Diff<'_>, target: DiffTarget) -> Result<Vec<FileDiff>> {
        let workdir = self.workdir()?.to_path_buf();
        let mut acc = collect_deltas_and_hunks(&diff)?;

        // Load sources. Post-image comes from the index for staged diffs and
        // from disk for worktree diffs; pre-image always comes from HEAD.
        let head_tree = self.inner.head().ok().and_then(|h| h.peel_to_tree().ok());
        for entry in acc.values_mut() {
            if entry.kind != ChangeKind::Deleted {
                entry.new_source = match target {
                    DiffTarget::StagedVsHead => self.blob_from_index(&entry.path),
                    DiffTarget::WorktreeVsHead => {
                        std::fs::read_to_string(workdir.join(&entry.path)).ok()
                    }
                };
            }
            if entry.kind != ChangeKind::Added {
                let lookup = entry.old_path.as_ref().unwrap_or(&entry.path);
                entry.old_source = head_tree
                    .as_ref()
                    .and_then(|t| blob_from_tree(&self.inner, t, lookup));
            }
        }

        Ok(acc.into_values().collect())
    }

    fn collect_from_trees(
        &self,
        diff: git2::Diff<'_>,
        old_tree: &git2::Tree<'_>,
        new_tree: &git2::Tree<'_>,
    ) -> Result<Vec<FileDiff>> {
        let mut acc = collect_deltas_and_hunks(&diff)?;

        for entry in acc.values_mut() {
            if entry.kind != ChangeKind::Deleted {
                entry.new_source = blob_from_tree(&self.inner, new_tree, &entry.path);
            }
            if entry.kind != ChangeKind::Added {
                let lookup = entry.old_path.as_ref().unwrap_or(&entry.path);
                entry.old_source = blob_from_tree(&self.inner, old_tree, lookup);
            }
        }

        Ok(acc.into_values().collect())
    }

    fn blob_from_index(&self, path: &Path) -> Option<String> {
        let index = self.inner.index().ok()?;
        let entry = index.get_path(path, 0)?;
        let blob = self.inner.find_blob(entry.id).ok()?;
        String::from_utf8(blob.content().to_vec()).ok()
    }
}

/// libgit2's `foreach` hands the file and hunk callbacks separate borrows, so
/// hunks are gathered alongside the deltas and merged after the walk.
fn collect_deltas_and_hunks(diff: &git2::Diff<'_>) -> Result<BTreeMap<PathBuf, FileDiff>> {
    let mut files: BTreeMap<PathBuf, FileDiff> = BTreeMap::new();
    let mut hunks: Vec<(PathBuf, Hunk)> = Vec::new();

    diff.foreach(
        &mut |delta, _| {
            let Some(path) = delta.new_file().path().map(|p| p.to_path_buf()) else {
                return true;
            };
            files.insert(
                path.clone(),
                FileDiff {
                    language: Language::from_path(&path),
                    old_path: delta.old_file().path().map(|p| p.to_path_buf()),
                    kind: map_status(delta.status()),
                    hunks: Vec::new(),
                    new_source: None,
                    old_source: None,
                    path,
                },
            );
            true
        },
        None,
        Some(&mut |delta, hunk| {
            if let Some(path) = delta.new_file().path() {
                hunks.push((
                    path.to_path_buf(),
                    Hunk {
                        new_start: hunk.new_start() as usize,
                        new_lines: hunk.new_lines() as usize,
                        old_start: hunk.old_start() as usize,
                        old_lines: hunk.old_lines() as usize,
                    },
                ));
            }
            true
        }),
        None,
    )?;

    for (path, hunk) in hunks {
        if let Some(entry) = files.get_mut(&path) {
            entry.hunks.push(hunk);
        }
    }
    Ok(files)
}

fn blob_from_tree(repo: &git2::Repository, tree: &git2::Tree<'_>, path: &Path) -> Option<String> {
    let entry = tree.get_path(path).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    String::from_utf8(blob.content().to_vec()).ok()
}

fn map_status(status: git2::Delta) -> ChangeKind {
    match status {
        git2::Delta::Added | git2::Delta::Copied | git2::Delta::Untracked => ChangeKind::Added,
        git2::Delta::Deleted => ChangeKind::Deleted,
        git2::Delta::Renamed => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_line_containment() {
        let h = Hunk {
            new_start: 10,
            new_lines: 3,
            old_start: 10,
            old_lines: 0,
        };
        assert!(!h.contains_new_line(9));
        assert!(h.contains_new_line(10));
        assert!(h.contains_new_line(12));
        assert!(!h.contains_new_line(13));
    }

    #[test]
    fn pure_deletion_hunk_contains_nothing() {
        let h = Hunk {
            new_start: 10,
            new_lines: 0,
            old_start: 10,
            old_lines: 4,
        };
        assert!(!h.contains_new_line(10));
    }

    /// Builds a repository with one commit, then a staged edit and a further
    /// unstaged edit on top of it, so the two targets must disagree.
    fn repo_with_staged_and_unstaged(tag: &str) -> (std::path::PathBuf, Repo) {
        let dir = std::env::temp_dir().join(format!("dross-diff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let repo = git2::Repository::init(&dir).unwrap();
        let file = dir.join("a.js");
        let stage = |name: &str| {
            let mut idx = repo.index().unwrap();
            idx.add_path(Path::new(name)).unwrap();
            idx.write().unwrap();
        };

        std::fs::write(&file, "export const committed = 1;\n").unwrap();
        stage("a.js");
        // Committed alongside a.js, and later modified without ever being
        // staged. It is the only path that separates the two targets: every
        // other file differs from HEAD in both the index and the working tree.
        std::fs::write(dir.join("c.js"), "export const c = 1;\n").unwrap();
        stage("c.js");
        let tree = repo
            .find_tree(repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("t", "t@t").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // Stage a change to a.js, then edit it again without staging.
        std::fs::write(&file, "export const staged = 2;\n").unwrap();
        stage("a.js");
        std::fs::write(&file, "export const unstaged = 3;\n").unwrap();

        // b.js is added to the index and then edited again, so the index and
        // the working tree hold different content for a path present in both
        // diffs. This pins which source each target reads.
        std::fs::write(dir.join("b.js"), "export const b = 9;\n").unwrap();
        stage("b.js");
        std::fs::write(dir.join("b.js"), "export const b = 10;\n").unwrap();

        // c.js is modified in the working tree only. The index still matches
        // HEAD, so the staged target must not report it at all. This is what
        // pins which diff each target runs.
        std::fs::write(dir.join("c.js"), "export const c = 2;\n").unwrap();

        (dir.clone(), Repo::open(&dir).unwrap())
    }

    /// The distinction the whole pre-commit case rests on. If the staged target
    /// returned working-tree content, the hook would pass judgement on code the
    /// commit does not contain — and block or clear the wrong thing.
    #[test]
    fn staged_and_worktree_targets_see_different_content() {
        let (dir, repo) = repo_with_staged_and_unstaged("targets");

        let staged = repo.diff(DiffTarget::StagedVsHead).unwrap();
        let worktree = repo.diff(DiffTarget::WorktreeVsHead).unwrap();
        let find = |v: &[FileDiff], name: &str| {
            v.iter()
                .find(|d| d.path == Path::new(name))
                .cloned()
                .unwrap_or_else(|| panic!("{name} missing from diff"))
        };

        let staged_a = find(&staged, "a.js");
        let worktree_a = find(&worktree, "a.js");
        assert!(
            staged_a.new_source.as_deref().unwrap().contains("staged")
                && !staged_a.new_source.as_deref().unwrap().contains("unstaged"),
            "staged target read a.js from the working tree"
        );
        assert!(
            worktree_a
                .new_source
                .as_deref()
                .unwrap()
                .contains("unstaged"),
            "worktree target missed the unstaged edit to a.js"
        );

        // b.js was staged and then edited again, so its later edit exists only
        // in the working tree. This pair is what actually pins which git2 diff
        // each target runs: new_source is chosen by target independently, so
        // asserting on a.js alone still passed when StagedVsHead was rewritten
        // to diff the working tree.
        assert!(
            find(&staged, "b.js")
                .new_source
                .as_deref()
                .unwrap()
                .contains("= 9"),
            "staged target read b.js from the working tree"
        );
        assert!(
            find(&worktree, "b.js")
                .new_source
                .as_deref()
                .unwrap()
                .contains("= 10"),
            "worktree target read b.js from the index"
        );

        // c.js is the discriminator. new_source is chosen by target
        // independently of which diff was run, so content assertions alone
        // still passed when StagedVsHead was rewritten to diff the working
        // tree. Only the set of reported paths reveals that.
        assert!(
            !staged.iter().any(|d| d.path == Path::new("c.js")),
            "staged target reported a file that was never staged"
        );
        assert!(
            worktree.iter().any(|d| d.path == Path::new("c.js")),
            "worktree target missed a working-tree-only change"
        );

        // Both see the same pre-image, since both diff against HEAD.
        for d in [&staged_a, &worktree_a] {
            assert!(d.old_source.as_deref().unwrap().contains("committed"));
            assert_eq!(d.kind, ChangeKind::Modified);
            assert_eq!(d.language, Some(Language::JavaScript));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clean_tree_produces_no_diff() {
        let dir = std::env::temp_dir().join(format!("dross-diff-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = git2::Repository::init(&dir).unwrap();
        std::fs::write(
            dir.join("a.js"),
            "export const a = 1;
",
        )
        .unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("a.js")).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@t").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let repo = Repo::open(&dir).unwrap();
        assert!(repo.diff(DiffTarget::StagedVsHead).unwrap().is_empty());
        assert!(repo.diff(DiffTarget::WorktreeVsHead).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The header shows the branch, and a detached HEAD has none. It must come
    /// back as None rather than as a sha the user would read as a branch name.
    #[test]
    fn branch_is_none_on_a_detached_head() {
        let (dir, repo) = repo_with_staged_and_unstaged("branch");
        assert!(repo.branch().is_some(), "a normal checkout has a branch");

        let head = repo.inner().head().unwrap().target().unwrap();
        repo.inner().set_head_detached(head).unwrap();
        let reopened = Repo::open(&dir).unwrap();
        assert_eq!(reopened.branch(), None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
