//! OpenAI Codex CLI adapter (spec section 3).
//!
//! Two documented constraints shape this adapter, and both are surfaced to the
//! user rather than silently worked around:
//!   1. Hooks are opt-in and disabled by default, so detection alone does not
//!      mean the hook will fire — the user must enable them in Codex config.
//!   2. PreToolUse only intercepts the Bash tool, so file-edit interception is
//!      not available. Dross hooks PostToolUse on `git commit` invocations
//!      instead of on edits directly.

use anyhow::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

use crate::{Adapter, AdapterId, AdapterStatus, DROSS_MARKER, read_json, write_json};

pub struct CodexAdapter;

impl CodexAdapter {
    fn config_dir() -> Option<PathBuf> {
        crate::home_dir().map(|h| h.join(".codex"))
    }

    fn hooks_path(repo_root: &Path) -> PathBuf {
        let local = repo_root.join(".codex").join("hooks.json");
        if local.exists() {
            return local;
        }
        Self::config_dir()
            .map(|d| d.join("hooks.json"))
            .unwrap_or(local)
    }
}

impl Adapter for CodexAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::CodexCli
    }

    fn status(&self, repo_root: &Path) -> AdapterStatus {
        let path = Self::hooks_path(repo_root);
        let doc = read_json(&path);
        let installed = serde_json::to_string(&doc)
            .map(|s| s.contains(DROSS_MARKER))
            .unwrap_or(false);
        let detected = path.exists()
            || Self::config_dir().is_some_and(|d| d.is_dir())
            || repo_root.join(".codex").is_dir();

        AdapterStatus {
            id: AdapterId::CodexCli,
            label: AdapterId::CodexCli.label().to_string(),
            detected,
            installed,
            config_path: Some(path),
            limitations: vec![
                "Codex hooks are opt-in and disabled by default; they must be enabled in \
                 Codex's own config before this fires."
                    .to_string(),
                "PreToolUse only intercepts the Bash tool, so Dross hooks PostToolUse on \
                 `git commit` rather than on individual file edits."
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

        let list = hooks.entry("PostToolUse").or_insert_with(|| json!([]));
        if !list.is_array() {
            *list = json!([]);
        }
        let list = list.as_array_mut().unwrap();
        strip_dross(list);
        list.push(json!({
            "matcher": "Bash",
            // Only act on commit invocations; the CLI no-ops otherwise.
            "command": "dross check --staged --hook --if-git-commit",
            "_source": DROSS_MARKER
        }));

        write_json(&path, &doc)
    }

    fn uninstall(&self, repo_root: &Path) -> Result<()> {
        let path = Self::hooks_path(repo_root);
        if !path.exists() {
            return Ok(());
        }
        let mut doc = read_json(&path);
        if let Some(hooks) = doc.get_mut("hooks").and_then(|h| h.as_object_mut())
            && let Some(list) = hooks.get_mut("PostToolUse").and_then(|v| v.as_array_mut())
        {
            strip_dross(list);
            if list.is_empty() {
                hooks.remove("PostToolUse");
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
    fn status_reports_both_documented_limitations() {
        let status = CodexAdapter.status(Path::new("."));
        assert_eq!(status.limitations.len(), 2);
        assert!(status.limitations.iter().any(|l| l.contains("opt-in")));
        assert!(status.limitations.iter().any(|l| l.contains("PreToolUse")));
    }

    #[test]
    fn install_is_idempotent() {
        let repo = std::env::temp_dir().join(format!("dross-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(repo.join(".codex")).unwrap();
        write_json(&repo.join(".codex").join("hooks.json"), &json!({})).unwrap();

        CodexAdapter.install(&repo).unwrap();
        CodexAdapter.install(&repo).unwrap();

        let doc = read_json(&CodexAdapter::hooks_path(&repo));
        let count = serde_json::to_string(&doc)
            .unwrap()
            .matches(DROSS_MARKER)
            .count();
        assert_eq!(count, 1);

        std::fs::remove_dir_all(&repo).ok();
    }
}
