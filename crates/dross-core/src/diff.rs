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
            DiffTarget::WorktreeVsHead => {
                self.inner
                    .diff_tree_to_workdir_with_index(head_tree.as_ref(), Some(&mut opts))?
            }
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
}
