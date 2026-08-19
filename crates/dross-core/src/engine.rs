//! The analysis pipeline: diff -> tag -> check -> score.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::authorship::AuthorshipMap;
use crate::checks::{CheckContext, all_checks, annotate_authorship};
use crate::config::Config;
use crate::diff::{DiffTarget, FileDiff, Repo};
use crate::finding::{Finding, Severity};
use crate::index::FingerprintIndex;
use crate::lang::Language;
use crate::symbols::SymbolTable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub files_analyzed: usize,
    pub duration_ms: u128,
    pub risk_score: u32,
    /// Checks that could not run and why — never silently skipped.
    pub skipped: Vec<SkippedCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedCheck {
    pub check: String,
    pub reason: String,
}

impl Report {
    pub fn has_blocking(&self, threshold: Severity) -> bool {
        self.findings.iter().any(|f| f.severity >= threshold)
    }

    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }

    /// One-line summary, the CLI's default output (spec section 4).
    pub fn summary_line(&self) -> String {
        if self.findings.is_empty() {
            return format!(
                "dross: clean ({} file(s), {}ms)",
                self.files_analyzed, self.duration_ms
            );
        }
        format!(
            "dross: {} finding(s) — {} error, {} warning, {} info across {} file(s) ({}ms)",
            self.findings.len(),
            self.count_by_severity(Severity::Error),
            self.count_by_severity(Severity::Warning),
            self.count_by_severity(Severity::Info),
            self.files_analyzed,
            self.duration_ms
        )
    }
}

pub struct Engine {
    config: Config,
    index: Option<FingerprintIndex>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            index: None,
        }
    }

    pub fn with_index(mut self, index: FingerprintIndex) -> Self {
        self.index = Some(index);
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn index_mut(&mut self) -> Option<&mut FingerprintIndex> {
        self.index.as_mut()
    }

    /// Analyzes a repository's current diff.
    pub fn analyze_repo(
        &self,
        repo_path: &Path,
        target: DiffTarget,
        authorship: &AuthorshipMap,
    ) -> Result<Report> {
        let repo = Repo::open(repo_path)?;
        let diffs = repo.diff(target)?;
        let root = repo.workdir()?.to_path_buf();
        self.analyze_diffs(&root, &diffs, authorship)
    }

    /// Analyzes a prepared diff set. Split out so the benchmark harness and
    /// tests can drive the pipeline without a working tree.
    pub fn analyze_diffs(
        &self,
        repo_root: &Path,
        diffs: &[FileDiff],
        authorship: &AuthorshipMap,
    ) -> Result<Report> {
        let started = std::time::Instant::now();

        let ctx = CheckContext::new(repo_root, diffs, authorship, self.index.as_ref());
        let mut findings = Vec::new();
        let mut skipped = Vec::new();

        for check in all_checks() {
            let id = check.id();
            if !self.config.is_enabled(id) {
                skipped.push(SkippedCheck {
                    check: id.as_str().to_string(),
                    reason: "disabled in config".to_string(),
                });
                continue;
            }
            if id == crate::finding::CheckId::StructuralClone && self.index.is_none() {
                skipped.push(SkippedCheck {
                    check: id.as_str().to_string(),
                    reason: "fingerprint index not built yet — run `dross index`".to_string(),
                });
                continue;
            }

            let mut produced = check.run(&ctx);

            // Spec section 5: untagged hunks get the lighter pass. Checks that
            // only make sense for agent code drop their human-code findings.
            if !check.applies_to_human_code() {
                produced.retain(|f| authorship.tag_for(&f.span.file, f.span.start_line).is_ai());
            }
            findings.extend(produced);
        }

        annotate_authorship(&mut findings, authorship);
        findings.retain(|f| f.severity >= self.config.min_severity);
        findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.span.file.cmp(&b.span.file))
                .then_with(|| a.span.start_line.cmp(&b.span.start_line))
        });

        let risk_score = risk_score(&findings);
        Ok(Report {
            files_analyzed: diffs.len(),
            duration_ms: started.elapsed().as_millis(),
            findings,
            risk_score,
            skipped,
        })
    }

    /// Builds the whole-repo fingerprint index (spec section 4, first open).
    pub fn build_index(
        &mut self,
        repo_root: &Path,
        mut progress: impl FnMut(usize, usize),
    ) -> Result<usize> {
        let files = source_files(repo_root, &self.config);
        let total = files.len();
        let Some(index) = self.index.as_mut() else {
            anyhow::bail!("no index configured");
        };

        let mut indexed = 0;
        for (i, (path, language)) in files.iter().enumerate() {
            if let Ok(source) = std::fs::read_to_string(path) {
                let relative = path.strip_prefix(repo_root).unwrap_or(path);
                indexed += index.index_file(relative, *language, &source).unwrap_or(0);
            }
            progress(i + 1, total);
        }
        Ok(indexed)
    }

    /// Replays the repo's own history to build the complexity baseline that
    /// the over-engineering outlier signal scores against (spec 5a).
    pub fn build_complexity_baseline(
        &mut self,
        repo_root: &Path,
        max_commits: usize,
    ) -> Result<usize> {
        let repo = Repo::open(repo_root)?;
        let mut walker = repo.inner().revwalk()?;
        walker.push_head()?;

        let shas: Vec<String> = walker
            .take(max_commits)
            .filter_map(|r| r.ok())
            .map(|oid| oid.to_string())
            .collect();

        let mut samples = 0;
        for pair in shas.windows(2) {
            let (new_sha, old_sha) = (&pair[0], &pair[1]);
            let Ok(diffs) = repo.diff_commits(old_sha, new_sha) else {
                continue;
            };
            let mut complexity = 0usize;
            let mut lines = 0usize;
            for diff in &diffs {
                let (Some(language), Some(source)) = (diff.language, diff.new_source.as_ref())
                else {
                    continue;
                };
                let Ok(parsed) = crate::ast::ParsedFile::parse(language, source.clone()) else {
                    continue;
                };
                lines += diff.changed_line_count();
                for func in parsed.functions() {
                    if !diff.touches_range(func.start_line, func.end_line) {
                        continue;
                    }
                    let m = crate::metrics::function_metrics(&parsed, &func);
                    complexity += m.cyclomatic + m.node_count / 10;
                }
            }
            if lines == 0 || complexity == 0 {
                continue;
            }
            if let Some(index) = self.index.as_ref() {
                index.record_baseline_sample(new_sha, lines, complexity)?;
                samples += 1;
            }
        }
        Ok(samples)
    }
}

/// Weighted risk score, 0-100. Errors dominate; info barely moves it.
fn risk_score(findings: &[Finding]) -> u32 {
    let raw: u32 = findings
        .iter()
        .map(|f| match f.severity {
            Severity::Error => 25,
            Severity::Warning => 8,
            Severity::Info => 2,
        })
        .sum();
    raw.min(100)
}

/// Walks the repo for indexable source files, honouring ignore rules.
pub fn source_files(root: &Path, config: &Config) -> Vec<(PathBuf, Language)> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !config.is_ignored(e.path()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| Language::from_path(e.path()).map(|l| (e.path().to_path_buf(), l)))
        .collect()
}

impl<'a> CheckContext<'a> {
    /// Builds the symbol table lazily from the changed files plus, when a repo
    /// root is available, the rest of the repository.
    pub fn symbols(&self) -> SymbolTable {
        let mut table = SymbolTable::new();
        let config = Config::default();

        // Changed files are added from their post-image parse, not from disk:
        // staged content may differ from the working tree, and adding a file
        // twice would double-count its call sites and suppress findings that
        // depend on exact counts.
        let changed: std::collections::HashSet<&Path> =
            self.diffs.iter().map(|d| d.path.as_path()).collect();

        for (path, language) in source_files(self.repo_root, &config) {
            let relative = path.strip_prefix(self.repo_root).unwrap_or(&path);
            if changed.contains(relative) {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(&path) {
                table.add_file(relative, language, &source);
            }
        }

        for diff in self.diffs {
            if let Some(parsed) = self.parsed(&diff.path) {
                table.add_parsed(&diff.path, parsed);
            }
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangeKind, Hunk};

    fn diff_of(path: &str, source: &str, language: Language) -> FileDiff {
        FileDiff {
            path: PathBuf::from(path),
            old_path: None,
            kind: ChangeKind::Added,
            language: Some(language),
            hunks: vec![Hunk {
                new_start: 1,
                new_lines: source.lines().count().max(1),
                old_start: 0,
                old_lines: 0,
            }],
            new_source: Some(source.to_string()),
            old_source: None,
        }
    }

    #[test]
    fn end_to_end_flags_a_swallowed_exception() {
        let engine = Engine::new(Config::default());
        let src = "function load() {\n  try {\n    return parse();\n  } catch (e) {}\n}\n";
        let diffs = vec![diff_of("src/a.js", src, Language::JavaScript)];
        let report = engine
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.signal == "empty-catch-body")
        );
        assert!(report.risk_score > 0);
    }

    #[test]
    fn clean_code_produces_no_findings() {
        let engine = Engine::new(Config::default());
        let src = "function add(a, b) {\n  return a + b;\n}\n";
        let diffs = vec![diff_of("src/a.js", src, Language::JavaScript)];
        let report = engine
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(report.findings.is_empty(), "got {:?}", report.findings);
        assert_eq!(report.risk_score, 0);
    }

    #[test]
    fn tautological_check_skips_untagged_human_code() {
        let engine = Engine::new(Config::default());
        let src = "it('x', () => { expect(f(a)).toBe(f(a)); });\n";
        let diffs = vec![diff_of("src/a.test.js", src, Language::JavaScript)];
        // No authorship tags = treated as human = lighter pass.
        let report = engine
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.check == crate::finding::CheckId::TautologicalTest)
        );
    }

    #[test]
    fn tautological_check_fires_on_tagged_ai_code() {
        use crate::authorship::{Tag, TaggedRange};
        let engine = Engine::new(Config::default());
        let src = "it('x', () => { expect(f(a)).toBe(f(a)); });\n";
        let diffs = vec![diff_of("src/a.test.js", src, Language::JavaScript)];
        let mut authorship = AuthorshipMap::new();
        authorship.insert(
            "src/a.test.js",
            TaggedRange {
                start_line: 1,
                end_line: 5,
                tag: Tag::Confirmed,
                reason: "test".into(),
            },
        );
        let report = engine
            .analyze_diffs(Path::new("."), &diffs, &authorship)
            .unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.check == crate::finding::CheckId::TautologicalTest)
        );
    }

    #[test]
    fn clone_check_is_reported_as_skipped_without_an_index() {
        let engine = Engine::new(Config::default());
        let diffs = vec![diff_of(
            "src/a.js",
            "function f(){return 1;}",
            Language::JavaScript,
        )];
        let report = engine
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(report.skipped.iter().any(|s| s.check == "structural-clone"));
    }

    #[test]
    fn changed_files_are_not_counted_twice_in_the_symbol_table() {
        // Regression: adding a changed file from both disk and the diff
        // overlay doubled its call-site counts, which silently suppressed
        // every signal keyed on an exact count.
        let src = "function fetchUser(id) { return getUser(id); }\nfetchUser(1);\n";
        let diffs = vec![diff_of("src/a.js", src, Language::JavaScript)];
        let authorship = AuthorshipMap::new();
        let ctx = CheckContext::new(Path::new("."), &diffs, &authorship, None);
        assert_eq!(ctx.symbols().call_site_count("fetchUser"), 1);
    }

    #[test]
    fn summary_line_reports_clean_runs() {
        let engine = Engine::new(Config::default());
        let report = engine
            .analyze_diffs(Path::new("."), &[], &AuthorshipMap::new())
            .unwrap();
        assert!(report.summary_line().contains("clean"));
    }
}
