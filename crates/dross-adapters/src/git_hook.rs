//! Universal git `pre-commit` fallback (spec section 3).
//!
//! Covers Cursor, Copilot, and any tool that commits from a terminal. Known
//! limitation, surfaced in the UI rather than buried: desktop apps that commit
//! through their own UI button can bypass `.git/hooks` entirely.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::{Adapter, AdapterId, AdapterStatus, DROSS_MARKER};

pub struct GitHookAdapter;

const HOOK_BODY: &str = r#"#!/bin/sh
# dross-managed — remove this block to uninstall
# Runs the Dross pre-flight check over staged changes.
if command -v dross >/dev/null 2>&1; then
  dross check --staged --hook || exit $?
fi
"#;

impl GitHookAdapter {
    fn hook_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".git").join("hooks").join("pre-commit")
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

        let existing = std::fs::read_to_string(&path).unwrap_or_default();
        if existing.contains(DROSS_MARKER) {
            return Ok(());
        }

        // Preserve any hook already installed by the user or another tool.
        let merged = if existing.trim().is_empty() {
            HOOK_BODY.to_string()
        } else {
            format!("{}\n{}", existing.trim_end(), HOOK_BODY)
        };
        std::fs::write(&path, merged)?;
        make_executable(&path)?;
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
        let remaining: String = existing.replace(HOOK_BODY, "");
        // If nothing but the shebang is left, the file was ours alone.
        if remaining.trim().is_empty() || remaining.trim() == "#!/bin/sh" {
            std::fs::remove_file(&path)?;
        } else {
            std::fs::write(&path, remaining)?;
        }
        Ok(())
    }
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

    fn temp_repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dross-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git").join("hooks")).unwrap();
        dir
    }

    #[test]
    fn install_is_idempotent_and_preserves_existing_hooks() {
        let repo = temp_repo();
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
