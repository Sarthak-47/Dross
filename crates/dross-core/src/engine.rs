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

        let ctx = CheckContext::new(
            repo_root,
            diffs,
            authorship,
            self.index.as_ref(),
            &self.config,
        );
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
            if id == crate::finding::CheckId::StructuralClone {
                if self.index.is_none() {
                    skipped.push(SkippedCheck {
                        check: id.as_str().to_string(),
                        reason: "fingerprint index not built yet — run `dross index`".to_string(),
                    });
                    continue;
                }
                // An index that cannot be read must say so. The check queries
                // it per function and treats a failed lookup as "no match", so
                // a broken index produced a clean report rather than an error —
                // which is how a schema mismatch went unnoticed while clone
                // detection silently found nothing at all.
                if let Some(index) = self.index.as_ref()
                    && let Err(e) = index.health_check()
                {
                    skipped.push(SkippedCheck {
                        check: id.as_str().to_string(),
                        reason: format!("fingerprint index is unreadable: {e}"),
                    });
                    continue;
                }
            }

            let mut produced = check.run(&ctx);

            // Signal-level suppression happens here rather than inside each
            // check, so a disabled signal cannot be forgotten in one of them.
            produced.retain(|f| self.config.is_signal_enabled(&f.signal));

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
                // Must match how the check measures, or the distribution and
                // the sample scored against it are not comparable.
                let old_parsed = diff
                    .old_source
                    .as_ref()
                    .and_then(|src| crate::ast::ParsedFile::parse(language, src.clone()).ok());
                complexity +=
                    crate::metrics::added_complexity(&parsed, old_parsed.as_ref(), |s, en| {
                        diff.touches_range(s, en)
                    });
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
///
/// Two sources of truth, in this order: the configured `ignore_dirs`, and then
/// the repository's own `.gitignore`.
///
/// The second used to be missing, and a hardcoded directory list cannot stand
/// in for it. Indexing this very repository walked `.bench/repos` — twenty-one
/// full clones of the benchmark corpus, ignored by git and invisible to that
/// list — and ran past ten minutes. Any project with vendored code or build
/// output under a name the list does not happen to carry hit the same wall.
///
/// Skipping them is also correct rather than merely fast: an ignored file is
/// not committed, so it can never appear in a diff, and indexing it would let
/// clone detection point a finding at a file that is not part of the project.
pub fn source_files(root: &Path, config: &Config) -> Vec<(PathBuf, Language)> {
    let repo = git2::Repository::open(root).ok();
    let ignored = |path: &Path| -> bool {
        if config.is_ignored(path) {
            return true;
        }
        // Checked on directories too, so an ignored tree is pruned rather than
        // descended into and discarded a file at a time.
        repo.as_ref()
            .and_then(|r| r.is_path_ignored(path).ok())
            .unwrap_or(false)
    };

    walkdir::WalkDir::new(root)
        .into_iter()
        // The root is never filtered: a repository checked out inside an
        // ignored path would otherwise index nothing.
        .filter_entry(|e| e.path() == root || !ignored(e.path()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| Language::from_path(e.path()).map(|l| (e.path().to_path_buf(), l)))
        .collect()
}

impl<'a> CheckContext<'a> {
    /// Builds the symbol table lazily from the changed files plus, when a repo
    /// root is available, the rest of the repository.
    /// Builds the repo-wide symbol table.
    ///
    /// Prefers the cached symbols in the index. Rebuilding by re-parsing every
    /// file made a one-file check take seconds on a large repository, which is
    /// disqualifying for a pre-commit hook. Falls back to a full walk when no
    /// index exists, so the checks still work before `dross index` has run.
    pub fn symbols(&self) -> SymbolTable {
        let mut table = SymbolTable::new();

        // Changed files are supplied from their post-image parse, never from
        // disk or cache: staged content may differ from the working tree, and
        // counting a file twice would suppress the signals keyed on exact
        // call-site counts.
        let changed: std::collections::HashSet<PathBuf> =
            self.diffs.iter().map(|d| d.path.clone()).collect();

        let loaded_from_cache = match self.index {
            Some(index) => match index.load_symbols(&changed) {
                Ok(entries) if !entries.is_empty() => {
                    table.add_many(entries.iter().map(|(p, s)| (p.as_path(), s.clone())));
                    true
                }
                _ => false,
            },
            None => false,
        };

        if !loaded_from_cache {
            let config = Config::default();
            for (path, language) in source_files(self.repo_root, &config) {
                let relative = path.strip_prefix(self.repo_root).unwrap_or(&path);
                if changed.contains(relative) {
                    continue;
                }
                if let Ok(source) = std::fs::read_to_string(&path) {
                    table.add_file(relative, language, &source);
                }
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
        let config = Config::default();
        let ctx = CheckContext::new(Path::new("."), &diffs, &authorship, None, &config);
        assert_eq!(ctx.symbols().call_site_count("fetchUser"), 1);
    }

    /// Regression: the checks read hardcoded constants instead of the
    /// configured values, so `clone_threshold` was inert and the settings UI
    /// appeared to work while changing nothing.
    #[test]
    fn clone_threshold_from_config_is_honoured() {
        use crate::index::FingerprintIndex;

        let existing = "function computeTotal(items) {\n  let total = 0;\n  for (const item of items) {\n    total += item.price * item.quantity;\n  }\n  return total;\n}\n";
        let candidate = "function summarize(rows) {\n  let acc = 0;\n  for (const row of rows) {\n    acc += row.price * row.quantity;\n  }\n  log(acc);\n  return acc;\n}\n";

        let build_index = || {
            let mut index = FingerprintIndex::open_in_memory().unwrap();
            index
                .index_file(Path::new("src/cart.js"), Language::JavaScript, existing)
                .unwrap();
            index
        };

        let diffs = vec![diff_of("src/other.js", candidate, Language::JavaScript)];

        // An impossible threshold must silence the check entirely.
        let strict = Config {
            clone_threshold: 1.01,
            // The signal ships disabled; this test is about whether the
            // configured threshold is honoured, so it is enabled explicitly.
            disabled_signals: Default::default(),
            ..Config::default()
        };
        let strict_report = Engine::new(strict)
            .with_index(build_index())
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(
            !strict_report
                .findings
                .iter()
                .any(|f| f.check == crate::finding::CheckId::StructuralClone),
            "a threshold above 1.0 must produce no clone findings"
        );

        // A permissive threshold must surface the near-duplicate.
        let loose = Config {
            clone_threshold: 0.1,
            disabled_signals: Default::default(),
            ..Config::default()
        };
        let loose_report = Engine::new(loose)
            .with_index(build_index())
            .analyze_diffs(Path::new("."), &diffs, &AuthorshipMap::new())
            .unwrap();
        assert!(
            loose_report
                .findings
                .iter()
                .any(|f| f.check == crate::finding::CheckId::StructuralClone),
            "a permissive threshold must surface the near-duplicate"
        );
    }

    /// The cached symbol path must agree with the full-walk path. The cache
    /// exists purely for speed — a one-file check on a 3000-file repository
    /// went from ~2.9s to ~35ms — and an optimization that quietly changes
    /// findings would be worse than the slow version.
    #[test]
    fn cached_symbols_agree_with_a_full_walk() {
        use crate::index::FingerprintIndex;

        let case = std::env::temp_dir().join(format!(
            "dross-symcache-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&case);
        std::fs::create_dir_all(&case).unwrap();

        // `helper` is called once from an existing file, so the wrapper in the
        // changed file must NOT be reported as a single-call pass-through.
        // Getting this right depends on the call site being visible, which is
        // exactly what the cache has to preserve.
        let existing = "function helper(x) { return inner(x); }
helper(1);
helper(2);
";
        std::fs::write(case.join("existing.js"), existing).unwrap();

        let changed_src = "function wrapper(id) { return helper(id); }
wrapper(7);
";
        let diffs = vec![diff_of("changed.js", changed_src, Language::JavaScript)];
        let authorship = AuthorshipMap::new();

        // Uncached: no index, so symbols come from walking the directory.
        let config = Config::default();
        let walk_ctx = CheckContext::new(&case, &diffs, &authorship, None, &config);
        let walked = walk_ctx.symbols();

        // Cached: the same file indexed, symbols loaded from SQLite.
        let mut index = FingerprintIndex::open_in_memory().unwrap();
        index
            .index_file(Path::new("existing.js"), Language::JavaScript, existing)
            .unwrap();
        let cached_ctx = CheckContext::new(&case, &diffs, &authorship, Some(&index), &config);
        let cached = cached_ctx.symbols();

        for name in ["helper", "wrapper", "inner"] {
            assert_eq!(
                walked.call_site_count(name),
                cached.call_site_count(name),
                "call-site count for `{name}` diverged between walk and cache"
            );
            assert_eq!(
                walked.is_ambiguous(name),
                cached.is_ambiguous(name),
                "ambiguity for `{name}` diverged between walk and cache"
            );
        }

        std::fs::remove_dir_all(&case).ok();
    }

    #[test]
    fn summary_line_reports_clean_runs() {
        let engine = Engine::new(Config::default());
        let report = engine
            .analyze_diffs(Path::new("."), &[], &AuthorshipMap::new())
            .unwrap();
        assert!(report.summary_line().contains("clean"));
    }

    /// Regression: the walk consulted only the configured `ignore_dirs`, so a
    /// directory ignored by git but absent from that list was descended into.
    /// Indexing this repository walked twenty-one checked-out clones under
    /// `.bench/repos` and ran past ten minutes.
    #[test]
    fn the_walk_skips_directories_the_repository_ignores() {
        let root = std::env::temp_dir().join(format!("dross-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("scratch/nested")).unwrap();
        std::fs::write(
            root.join(".gitignore"),
            "scratch/
",
        )
        .unwrap();
        std::fs::write(
            root.join("src/kept.js"),
            "export const a = 1;
",
        )
        .unwrap();
        std::fs::write(
            root.join("scratch/nested/skipped.js"),
            "export const b = 2;
",
        )
        .unwrap();
        git2::Repository::init(&root).unwrap();

        let found = source_files(&root, &Config::default());
        let names: Vec<String> = found
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(names.contains(&"kept.js".to_string()), "{names:?}");
        assert!(
            !names.contains(&"skipped.js".to_string()),
            "an ignored directory was walked: {names:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The built-in list still applies where there is no repository at all, so
    /// the checks work on a plain directory.
    #[test]
    fn the_walk_still_honours_the_configured_list_without_a_repository() {
        let root = std::env::temp_dir().join(format!("dross-walk-plain-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/kept.js"),
            "export const a = 1;
",
        )
        .unwrap();
        std::fs::write(
            root.join("node_modules/pkg/dep.js"),
            "export const b = 2;
",
        )
        .unwrap();

        let names: Vec<String> = source_files(&root, &Config::default())
            .iter()
            .map(|(p, _)| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(names, vec!["kept.js".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Pins the severity weights.
    ///
    /// The desktop UI carries the same three numbers — WEIGHT in
    /// components/Findings.tsx — and prints them beneath the score this
    /// function produced, as the formula that produced it. They drifted once,
    /// so the app displayed a formula that did not yield the number beside it.
    /// derive.test.ts asserts the other copy.
    #[test]
    fn risk_weights_are_the_ones_the_ui_prints() {
        use crate::finding::{CheckId, Finding, Severity, SourceSpan};

        let at = |severity| {
            Finding::new(
                CheckId::SwallowedException,
                "s",
                severity,
                SourceSpan {
                    file: PathBuf::from("a.js"),
                    start_line: 1,
                    end_line: 1,
                },
                "m",
                "e",
            )
        };

        assert_eq!(risk_score(&[at(Severity::Error)]), 25);
        assert_eq!(risk_score(&[at(Severity::Warning)]), 8);
        assert_eq!(risk_score(&[at(Severity::Info)]), 2);

        // 2 errors + 3 warnings + 1 info = 50 + 24 + 2.
        let mixed = vec![
            at(Severity::Error),
            at(Severity::Error),
            at(Severity::Warning),
            at(Severity::Warning),
            at(Severity::Warning),
            at(Severity::Info),
        ];
        assert_eq!(risk_score(&mixed), 76);

        // Capped, not wrapped.
        assert_eq!(risk_score(&vec![at(Severity::Error); 5]), 100);
        assert_eq!(risk_score(&[]), 0);
    }
}
