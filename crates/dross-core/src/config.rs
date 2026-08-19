//! Configuration. Every value has a working default — the app must run with
//! no config file at all (spec section 4).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::finding::{CheckId, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub disabled_checks: HashSet<CheckId>,
    pub min_severity: Severity,
    /// Type-2/3 clone similarity threshold.
    pub clone_threshold: f64,
    /// Z-score at which the complexity outlier signal fires.
    pub complexity_z_threshold: f64,
    /// Directory names never indexed or scanned.
    pub ignore_dirs: Vec<String>,
    /// Commits replayed when building the complexity baseline.
    pub baseline_commits: usize,
    /// Severity at or above which a pre-commit hook blocks the commit.
    pub block_at: Option<Severity>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            disabled_checks: HashSet::new(),
            min_severity: Severity::Info,
            clone_threshold: crate::checks::structural_clone::DEFAULT_THRESHOLD,
            complexity_z_threshold: crate::checks::over_engineering::OUTLIER_Z_THRESHOLD,
            ignore_dirs: [
                "node_modules",
                ".git",
                "target",
                "dist",
                "build",
                "out",
                ".venv",
                "venv",
                "__pycache__",
                ".next",
                ".nuxt",
                "coverage",
                "vendor",
                ".tox",
                ".mypy_cache",
                ".pytest_cache",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            baseline_commits: 200,
            // Default to advisory, not blocking: a pre-commit hook that blocks
            // on a false positive is uninstalled immediately.
            block_at: None,
        }
    }
}

impl Config {
    pub fn is_enabled(&self, check: CheckId) -> bool {
        !self.disabled_checks.contains(&check)
    }

    pub fn is_ignored(&self, path: &Path) -> bool {
        path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            self.ignore_dirs.iter().any(|d| d == s.as_ref())
        })
    }

    /// Loads `.dross.json` from a repo root, falling back to defaults.
    pub fn load(repo_root: &Path) -> Self {
        let path = repo_root.join(".dross.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, repo_root: &Path) -> anyhow::Result<()> {
        let path = repo_root.join(".dross.json");
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Where the index lives. Kept inside the repo so nothing escapes it.
    pub fn index_path(repo_root: &Path) -> std::path::PathBuf {
        repo_root.join(".dross").join("index.sqlite")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_every_check() {
        let c = Config::default();
        assert!(c.is_enabled(CheckId::SwallowedException));
        assert!(c.is_enabled(CheckId::OverEngineering));
    }

    #[test]
    fn ignores_vendor_directories() {
        let c = Config::default();
        assert!(c.is_ignored(Path::new("a/node_modules/b.js")));
        assert!(!c.is_ignored(Path::new("src/b.js")));
    }

    #[test]
    fn roundtrips_through_json() {
        let mut c = Config::default();
        c.disabled_checks.insert(CheckId::OverEngineering);
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert!(!back.is_enabled(CheckId::OverEngineering));
    }
}
