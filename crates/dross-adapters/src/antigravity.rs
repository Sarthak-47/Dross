//! Google Antigravity adapter (spec section 3).
//!
//! Antigravity carries JSON hooks over from the Gemini CLI hook system. Its
//! Editor view commits like a normal VS Code fork, so the git-hook fallback
//! covers that path; these native hooks add coverage for the autonomous
//! Manager-view agent flows the git hook cannot see.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::{read_json, write_json, Adapter, AdapterId, AdapterStatus, DROSS_MARKER};

pub struct AntigravityAdapter;

impl AntigravityAdapter {
    fn config_dirs() -> Vec<PathBuf> {
        let Some(home) = crate::home_dir() else {
            return Vec::new();
        };
        vec![home.join(".antigravity"), home.join(".gemini")]
    }

    fn hooks_path(repo_root: &Path) -> PathBuf {
        let local = repo_root.join(".antigravity").join("hooks.json");
        if local.exists() {
            return local;
        }
        Self::config_dirs()
            .into_iter()
            .find(|d| d.is_dir())
            .map(|d| d.join("hooks.json"))
            .unwrap_or(local)
    }
}

impl Adapter for AntigravityAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::Antigravity
    }

    fn status(&self, repo_root: &Path) -> AdapterStatus {
        let path = Self::hooks_path(repo_root);
        let doc = read_json(&path);
        let installed = serde_json::to_string(&doc)
            .map(|s| s.contains(DROSS_MARKER))
            .unwrap_or(false);
        let detected = path.exists()
            || repo_root.join(".antigravity").is_dir()
            || Self::config_dirs().iter().any(|d| d.is_dir());

        AdapterStatus {
            id: AdapterId::Antigravity,
            label: AdapterId::Antigravity.label().to_string(),
            detected,
            installed,
            config_path: Some(path),
            limitations: vec![
                "Editor-view commits go through the normal VS Code git path, so the \
                 git pre-commit fallback covers them; these hooks exist for Manager-view \
                 autonomous agent flows."
                    .to_string(),
            ],
        }
    }

    fn install(&self, repo_root: &Path) -> Result<()> {
        let path = Self::hooks_path(repo_root);
        let mut doc = read_json(&path);
        if !doc.is_object() {
            doc = json!({});
        }
        let hooks = doc
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({}));
        if !hooks.is_object() {
            *hooks = json!({});
        }
        let hooks = hooks.as_object_mut().unwrap();

        for (event, command) in [
            ("PostToolUse", "dross check --worktree --format json --quiet"),
            ("PreToolUse", "dross check --staged --hook --if-git-commit"),
        ] {
            let list = hooks.entry(event).or_insert_with(|| json!([]));
            if !list.is_array() {
                *list = json!([]);
            }
            let list = list.as_array_mut().unwrap();
            strip_dross(list);
            list.push(json!({
                "matcher": "*",
                "command": command,
                "_source": DROSS_MARKER
            }));
        }

        write_json(&path, &doc)
    }

    fn uninstall(&self, repo_root: &Path) -> Result<()> {
        let path = Self::hooks_path(repo_root);
        if !path.exists() {
            return Ok(());
        }
        let mut doc = read_json(&path);
        if let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for event in ["PostToolUse", "PreToolUse"] {
                if let Some(list) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
                    strip_dross(list);
                    if list.is_empty() {
                        hooks.remove(event);
                    }
                }
            }
        }
        write_json(&path, &doc)
    }
}

fn strip_dross(list: &mut Vec<Value>) {
    list.retain(|entry| {
        !serde_json::to_string(entry)
            .map(|s| s.contains(DROSS_MARKER))
            .unwrap_or(false)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_then_uninstall_leaves_no_trace() {
        let repo = std::env::temp_dir().join(format!("dross-ag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join(".antigravity")).unwrap();
        write_json(&repo.join(".antigravity").join("hooks.json"), &json!({})).unwrap();

        AntigravityAdapter.install(&repo).unwrap();
        assert!(AntigravityAdapter.status(&repo).installed);

        AntigravityAdapter.uninstall(&repo).unwrap();
        assert!(!AntigravityAdapter.status(&repo).installed);

        std::fs::remove_dir_all(&repo).ok();
    }
}
