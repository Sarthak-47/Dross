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
    /// Individual signals to suppress, by their stable signal id.
    ///
    /// Precision varies far more between signals than between checks — the
    /// benchmark measured `parameter-removed` at 100% and several
    /// over-engineering signals at 0% — so suppression has to be expressible
    /// at this granularity or a useful check gets disabled to silence one bad
    /// signal inside it.
    pub disabled_signals: HashSet<String>,
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
            disabled_signals: default_disabled_signals(),
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
                // Published or bundled copies of a repository's own source.
                // lodash ships npm-package/, socket.io ships client-dist/;
                // both were analyzed as if hand-written.
                "npm-package",
                "client-dist",
                "esm-dist",
                "umd",
                "lib-cov",
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

/// Signals off by default because their measured precision does not justify
/// interrupting a commit. See `docs/BENCHMARK_RESULTS.md`.
///
/// They remain implemented and can be switched on per repository. The reason
/// to default them off is that a pre-commit check is uninstalled as a whole:
/// one noisy signal takes the accurate ones with it.
fn default_disabled_signals() -> HashSet<String> {
    [
        // 0 true positives across 24 labeled findings, over two rounds and a
        // fix attempt. An ordinary factory containing one `if` is not a
        // one-variant registry, and separating the two needs resolution this
        // check does not have.
        "overkill-design-pattern",
        // 0 of 24. What it finds are published extension points — socket.io's
        // `BaseXHR`, `ClusterAdapter` — subclassed by consumers the repository
        // cannot see. Name-based resolution cannot tell those from
        // speculative generality.
        "single-implementation-abstraction",
        // 0 of 12. Now measures added rather than touched complexity, but has
        // not been re-validated, and a signal that fired on "chore: format" at
        // 8.4 sigma has to earn its way back on.
        "complexity-to-problem-size-outlier",
        // 8.3%, 8.3%, then 0% across three rounds and three fix attempts.
        // Each fix cut the volume without moving the false-positive rate,
        // because in a mature codebase structurally identical functions are
        // almost always deliberate parallel structure: Flask's `template_*`
        // decorator family, per-locale formatters, adapters implementing one
        // interface twice. The seeded corpus shows the check *can* find a
        // renamed duplicate; real repositories are mostly full of intentional
        // twins, and it cannot tell the two apart.
        "near-duplicate-function",
        // 0% in three separate rounds. Returning a default on failure is the
        // documented contract far more often than it is a hidden failure —
        // predicates, `get_or_none` lookups, best-effort serialisation,
        // deliberately ignored malformed input. Distinguishing a contract from
        // a concealment needs intent the AST does not carry.
        "silent-optimistic-return",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Config {
    pub fn is_enabled(&self, check: CheckId) -> bool {
        !self.disabled_checks.contains(&check)
    }

    pub fn is_signal_enabled(&self, signal: &str) -> bool {
        !self.disabled_signals.contains(signal)
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
    fn signals_with_no_measured_precision_are_off_by_default() {
        let c = Config::default();
        for signal in [
            "overkill-design-pattern",
            "single-implementation-abstraction",
            "complexity-to-problem-size-outlier",
            "near-duplicate-function",
            "silent-optimistic-return",
        ] {
            assert!(!c.is_signal_enabled(signal), "{signal} should default off");
        }
        // The signals that measured well stay on.
        for signal in [
            "empty-catch-body",
            "parameter-removed",
            "required-parameter-added",
        ] {
            assert!(c.is_signal_enabled(signal), "{signal} should default on");
        }
    }

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
