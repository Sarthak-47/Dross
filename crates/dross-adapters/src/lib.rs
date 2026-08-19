//! Integration adapters (spec section 3).
//!
//! Four paths into the same engine: Claude Code hooks, Codex CLI hooks,
//! Antigravity hooks, and a universal git `pre-commit` fallback that covers
//! Cursor, Copilot, and plain terminal use.

pub mod antigravity;
pub mod claude_code;
pub mod codex;
pub mod git_hook;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterId {
    ClaudeCode,
    CodexCli,
    Antigravity,
    GitHook,
}

impl AdapterId {
    pub fn label(self) -> &'static str {
        match self {
            AdapterId::ClaudeCode => "Claude Code",
            AdapterId::CodexCli => "OpenAI Codex CLI",
            AdapterId::Antigravity => "Google Antigravity",
            AdapterId::GitHook => "git pre-commit (Cursor, Copilot, terminal)",
        }
    }
}

/// What the Connections panel renders per integration (spec section 4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterStatus {
    pub id: AdapterId,
    pub label: String,
    /// The tool's config directory exists on this machine.
    pub detected: bool,
    /// Dross is currently wired into it.
    pub installed: bool,
    pub config_path: Option<PathBuf>,
    /// Honest caveats surfaced in the UI rather than buried in docs.
    pub limitations: Vec<String>,
}

pub trait Adapter {
    fn id(&self) -> AdapterId;
    /// Detects the tool and whether Dross is already wired in.
    fn status(&self, repo_root: &Path) -> AdapterStatus;
    /// Wires Dross in. Must be idempotent.
    fn install(&self, repo_root: &Path) -> anyhow::Result<()>;
    /// Removes only what Dross added, leaving other config intact.
    fn uninstall(&self, repo_root: &Path) -> anyhow::Result<()>;
}

pub fn all_adapters() -> Vec<Box<dyn Adapter>> {
    vec![
        Box::new(claude_code::ClaudeCodeAdapter),
        Box::new(codex::CodexAdapter),
        Box::new(antigravity::AntigravityAdapter),
        Box::new(git_hook::GitHookAdapter),
    ]
}

pub fn detect_all(repo_root: &Path) -> Vec<AdapterStatus> {
    all_adapters()
        .iter()
        .map(|a| a.status(repo_root))
        .collect()
}

/// The user's home directory, where the agent tools keep global config.
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Merges a Dross entry into an existing JSON config without clobbering the
/// user's other settings. Returns the updated document.
pub(crate) fn read_json(path: &Path) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

pub(crate) fn write_json(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

/// Marker written into every hook Dross installs, so uninstall can remove
/// exactly what was added and nothing else.
pub(crate) const DROSS_MARKER: &str = "dross-managed";
