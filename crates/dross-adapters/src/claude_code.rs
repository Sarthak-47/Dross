//! Claude Code hook adapter (spec section 3) — the primary integration.
//!
//! Uses the native hook config: PostToolUse on file edits gives per-edit
//! feedback, and PreToolUse on `git commit` gives a pre-flight gate.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::{read_json, write_json, Adapter, AdapterId, AdapterStatus, DROSS_MARKER};

pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    /// Project-scoped settings so the wiring travels with the repo.
    fn settings_path(repo_root: &Path) -> PathBuf {
        repo_root.join(".claude").join("settings.json")
    }

    fn global_config_dir() -> Option<PathBuf> {
        crate::home_dir().map(|h| h.join(".claude"))
    }

    fn hook_entries() -> Value {
        json!([
            {
                "matcher": "Edit|Write|MultiEdit",
                "hooks": [{
                    "type": "command",
                    "command": "dross check --worktree --format json --quiet",
                    "_source": DROSS_MARKER
                }]
            }
        ])
    }

    fn pre_commit_entries() -> Value {
        json!([
            {
                "matcher": "Bash",
                "hooks": [{
                    "type": "command",
                    "command": "dross check --staged --hook --if-git-commit",
                    "_source": DROSS_MARKER
                }]
            }
        ])
    }
}

impl Adapter for ClaudeCodeAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::ClaudeCode
    }

    fn status(&self, repo_root: &Path) -> AdapterStatus {
        let path = Self::settings_path(repo_root);
        let doc = read_json(&path);
        let installed = serde_json::to_string(&doc)
            .map(|s| s.contains(DROSS_MARKER))
            .unwrap_or(false);

        let detected = path.exists()
            || repo_root.join(".claude").is_dir()
            || Self::global_config_dir().is_some_and(|d| d.is_dir());

        AdapterStatus {
            id: AdapterId::ClaudeCode,
            label: AdapterId::ClaudeCode.label().to_string(),
            detected,
            installed,
            config_path: Some(path),
            limitations: Vec::new(),
        }
    }

    fn install(&self, repo_root: &Path) -> Result<()> {
        let path = Self::settings_path(repo_root);
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

        merge_matchers(hooks, "PostToolUse", Self::hook_entries());
        merge_matchers(hooks, "PreToolUse", Self::pre_commit_entries());

        write_json(&path, &doc)
    }

    fn uninstall(&self, repo_root: &Path) -> Result<()> {
        let path = Self::settings_path(repo_root);
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

/// Appends Dross's matchers to an event without disturbing existing ones.
fn merge_matchers(
    hooks: &mut serde_json::Map<String, Value>,
    event: &str,
    entries: Value,
) {
    let list = hooks.entry(event).or_insert_with(|| json!([]));
    if !list.is_array() {
        *list = json!([]);
    }
    let list = list.as_array_mut().unwrap();
    strip_dross(list);
    if let Some(new_entries) = entries.as_array() {
        list.extend(new_entries.iter().cloned());
    }
}

/// Removes only entries Dross wrote, identified by the marker.
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

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dross-cc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn install_preserves_unrelated_settings_and_is_idempotent() {
        let repo = temp_repo("preserve");
        let path = ClaudeCodeAdapter::settings_path(&repo);
        write_json(
            &path,
            &json!({
                "model": "opus",
                "hooks": { "PostToolUse": [{ "matcher": "Bash", "hooks": [] }] }
            }),
        )
        .unwrap();

        ClaudeCodeAdapter.install(&repo).unwrap();
        ClaudeCodeAdapter.install(&repo).unwrap();

        let doc = read_json(&path);
        assert_eq!(doc["model"], "opus", "clobbered unrelated setting");
        let post = doc["hooks"]["PostToolUse"].as_array().unwrap();
        assert!(post.iter().any(|e| e["matcher"] == "Bash"), "dropped user hook");
        let dross_count = serde_json::to_string(&doc)
            .unwrap()
            .matches(DROSS_MARKER)
            .count();
        assert_eq!(dross_count, 2, "expected exactly one entry per event");
        assert!(ClaudeCodeAdapter.status(&repo).installed);

        ClaudeCodeAdapter.uninstall(&repo).unwrap();
        let after = read_json(&path);
        assert_eq!(after["model"], "opus");
        assert!(!serde_json::to_string(&after).unwrap().contains(DROSS_MARKER));
        assert!(after["hooks"]["PostToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["matcher"] == "Bash"));

        std::fs::remove_dir_all(&repo).ok();
    }
}
