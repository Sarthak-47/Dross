//! Universal git `pre-commit` fallback (spec section 3).
//!
//! Covers Cursor, Copilot, and any tool that commits from a terminal. Known
//! limitation, surfaced in the UI rather than buried: desktop apps that commit
//! through their own UI button can bypass `.git/hooks` entirely.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::{Adapter, AdapterId, AdapterStatus, DROSS_MARKER};

pub struct GitHookAdapter;

/// The installed binary's absolute path is baked in at install time, with a
/// PATH lookup as fallback. A hook that silently skips when `dross` is not on
/// PATH is the worst outcome available: the commit succeeds and the user
/// believes it was checked. So a missing binary fails loudly instead.
const HOOK_TEMPLATE: &str = r#"#!/bin/sh
# dross-managed — remove this block to uninstall
# Runs the Dross pre-flight check over staged changes.
DROSS_BIN="__DROSS_BIN__"
if [ ! -x "$DROSS_BIN" ]; then
  DROSS_BIN="$(command -v dross 2>/dev/null)"
fi
if [ -z "$DROSS_BIN" ]; then
  echo "dross: pre-commit check did NOT run — the dross binary was not found." >&2
  echo "       Reinstall the hook with 'dross connections install git', or remove" >&2
  echo "       the dross block from .git/hooks/pre-commit to silence this." >&2
  exit 1
fi
"$DROSS_BIN" check --staged --hook || exit $?
"#;

/// Marks the boundaries of the block so uninstall can remove exactly what was
/// installed, even though the body varies by machine.
const BLOCK_START: &str = "# dross-managed — remove this block to uninstall";
const BLOCK_END: &str = "\"$DROSS_BIN\" check --staged --hook || exit $?";

impl GitHookAdapter {
    fn hook_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".git").join("hooks").join("pre-commit")
    }

    fn hook_body() -> String {
        let bin = std::env::current_exe()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .unwrap_or_else(|_| "dross".to_string());
        HOOK_TEMPLATE.replace("__DROSS_BIN__", &bin)
    }

    /// Strips a previously installed block, whatever binary path it recorded.
    fn strip_block(contents: &str) -> String {
        let Some(start) = contents.find(BLOCK_START) else {
            return contents.to_string();
        };
        // Keep the shebang and anything before our block.
        let head = &contents[..start];
        let rest = &contents[start..];
        let tail = match rest.find(BLOCK_END) {
            Some(end) => &rest[end + BLOCK_END.len()..],
            None => "",
        };
        format!("{}{}", head.trim_end(), tail)
    }
}

impl Adapter for GitHookAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::GitHook
    }

    fn status(&self, repo_root: &Path) -> AdapterStatus {
        let path = Self::hook_path(repo_root);
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        AdapterStatus {
            id: AdapterId::GitHook,
            label: AdapterId::GitHook.label().to_string(),
            detected: repo_root.join(".git").is_dir(),
            installed: contents.contains(DROSS_MARKER),
            config_path: Some(path),
            limitations: vec![
                "Desktop apps that commit through their own UI button rather than a \
                 terminal can bypass .git/hooks entirely; this hook will not fire for \
                 that flow."
                    .to_string(),
            ],
        }
    }

    fn install(&self, repo_root: &Path) -> Result<()> {
        let path = Self::hook_path(repo_root);
        std::fs::create_dir_all(path.parent().context("hooks dir has no parent")?)?;

        // Reinstall rather than bail, so the recorded binary path is refreshed
        // if Dross moved since the hook was written.
        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        let preserved = Self::strip_block(&existing);
        // A leftover bare shebang is not user content; our own body carries
        // one, and keeping both would emit the interpreter line twice.
        let preserved =
            if preserved.trim() == "#!/bin/sh" || preserved.trim() == "#!/usr/bin/env sh" {
                String::new()
            } else {
                preserved
            };

        let merged = if preserved.trim().is_empty() {
            Self::hook_body()
        } else {
            format!("{}\n{}", preserved.trim_end(), Self::hook_body())
        };
        std::fs::write(&path, merged)?;
        make_executable(&path)?;

        // The index lives inside the repository, so it must be excluded or it
        // ends up committed on the user's next `git add -A`.
        ignore_dross_dir(repo_root)?;
        Ok(())
    }

    fn uninstall(&self, repo_root: &Path) -> Result<()> {
        let path = Self::hook_path(repo_root);
        let Ok(existing) = std::fs::read_to_string(&path) else {
            return Ok(());
        };
        if !existing.contains(DROSS_MARKER) {
            return Ok(());
        }
        let remaining = Self::strip_block(&existing);
        // If nothing but the shebang is left, the file was ours alone.
        if remaining.trim().is_empty() || remaining.trim() == "#!/bin/sh" {
            std::fs::remove_file(&path)?;
        } else {
            std::fs::write(&path, remaining)?;
        }
        Ok(())
    }
}

/// Appends `.dross/` to the repository's `.gitignore` if it is not already
/// excluded. Idempotent, and leaves existing entries untouched.
pub fn ignore_dross_dir(repo_root: &Path) -> Result<()> {
    let path = repo_root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing
        .lines()
        .any(|l| matches!(l.trim(), ".dross" | ".dross/" | "/.dross" | "/.dross/"))
    {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str("\n# Dross index and risk history (local, regenerable)\n.dross/\n");
    std::fs::write(&path, updated)?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    // Git for Windows runs hooks through its bundled sh; no mode bit needed.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dross-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git").join("hooks")).unwrap();
        dir
    }

    /// Regression: the first hook body was `if command -v dross; then ... fi`,
    /// which turned a missing binary into a silent skip. A real commit with a
    /// seeded defect went through unchecked, and the user had no way to tell.
    #[test]
    fn hook_fails_loudly_when_the_binary_is_missing() {
        let repo = temp_repo("loud");
        GitHookAdapter.install(&repo).unwrap();
        let body = std::fs::read_to_string(GitHookAdapter::hook_path(&repo)).unwrap();

        assert!(
            body.contains("did NOT run"),
            "hook must announce that it did not run"
        );
        assert!(
            !body.contains("if command -v dross >/dev/null 2>&1; then"),
            "hook must not silently skip when dross is absent"
        );
        // The check invocation must not be nested inside an existence guard.
        assert!(body.contains("\"$DROSS_BIN\" check --staged --hook || exit $?"));

        std::fs::remove_dir_all(&repo).ok();
    }

    /// Regression: the index is written inside the repository, so without an
    /// ignore entry a `git add -A` committed `.dross/index.sqlite`.
    #[test]
    fn install_excludes_the_index_from_version_control() {
        let repo = temp_repo("ignore");
        std::fs::write(repo.join(".gitignore"), "node_modules/\n").unwrap();

        GitHookAdapter.install(&repo).unwrap();
        GitHookAdapter.install(&repo).unwrap();

        let ignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(ignore.contains("node_modules/"), "clobbered existing rules");
        assert_eq!(
            ignore.matches(".dross/").count(),
            1,
            "ignore entry added twice"
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    /// Regression: reinstalling over a previous install left the leftover
    /// shebang in place and appended a second one from the new body.
    #[test]
    fn reinstall_does_not_duplicate_the_shebang() {
        let repo = temp_repo("shebang");
        GitHookAdapter.install(&repo).unwrap();
        GitHookAdapter.install(&repo).unwrap();

        let body = std::fs::read_to_string(GitHookAdapter::hook_path(&repo)).unwrap();
        assert_eq!(body.matches("#!/bin/sh").count(), 1, "duplicated shebang");
        assert!(body.starts_with("#!/bin/sh"), "shebang must be first");

        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn install_records_an_absolute_binary_path() {
        let repo = temp_repo("abs");
        GitHookAdapter.install(&repo).unwrap();
        let body = std::fs::read_to_string(GitHookAdapter::hook_path(&repo)).unwrap();
        assert!(
            !body.contains("DROSS_BIN=\"dross\"") || std::env::current_exe().is_err(),
            "expected the resolved binary path, not a bare PATH lookup"
        );
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn install_is_idempotent_and_preserves_existing_hooks() {
        let repo = temp_repo("idempotent");
        let hook = GitHookAdapter::hook_path(&repo);
        std::fs::write(&hook, "#!/bin/sh\necho existing\n").unwrap();

        GitHookAdapter.install(&repo).unwrap();
        GitHookAdapter.install(&repo).unwrap();

        let contents = std::fs::read_to_string(&hook).unwrap();
        assert!(contents.contains("echo existing"), "clobbered user hook");
        assert_eq!(contents.matches(DROSS_MARKER).count(), 1, "installed twice");
        assert!(GitHookAdapter.status(&repo).installed);

        GitHookAdapter.uninstall(&repo).unwrap();
        let after = std::fs::read_to_string(&hook).unwrap();
        assert!(after.contains("echo existing"), "removed user hook");
        assert!(!after.contains(DROSS_MARKER));

        std::fs::remove_dir_all(&repo).ok();
    }
}
