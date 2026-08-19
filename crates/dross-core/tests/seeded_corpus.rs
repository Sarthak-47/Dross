//! Runs the seeded-defect corpus in `fixtures/seeded` and asserts the
//! ground-truth contract: every positive must be caught, and no negative may
//! be flagged.
//!
//! This is where recall actually comes from. Labeling emitted findings can
//! only measure precision — that sample contains no false negatives by
//! construction — so the corpus supplies the other half.

use std::path::{Path, PathBuf};

use dross_core::authorship::{AuthorshipMap, Tag, TaggedRange};
use dross_core::config::Config;
use dross_core::diff::{ChangeKind, FileDiff, Hunk};
use dross_core::engine::Engine;
use dross_core::finding::{CheckId, Finding};
use dross_core::index::FingerprintIndex;
use dross_core::lang::Language;

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/seeded")
        .canonicalize()
        .expect("seeded corpus directory must exist")
}

fn files_in(check: &str, polarity: &str) -> Vec<PathBuf> {
    let dir = corpus_root().join(check).join(polarity);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && Language::from_path(p).is_some())
        .collect();
    out.sort();
    out
}

/// Treats a whole file as one added hunk — the "agent wrote this file" case.
fn whole_file_diff(path: &Path, source: &str) -> FileDiff {
    let relative = path
        .strip_prefix(corpus_root())
        .unwrap_or(path)
        .to_path_buf();
    FileDiff {
        language: Language::from_path(path),
        old_path: None,
        kind: ChangeKind::Added,
        hunks: vec![Hunk {
            new_start: 1,
            new_lines: source.lines().count().max(1),
            old_start: 0,
            old_lines: 0,
        }],
        new_source: Some(source.to_string()),
        old_source: None,
        path: relative,
    }
}

fn modified_diff(path: &Path, before: &str, after: &str) -> FileDiff {
    let relative = path
        .strip_prefix(corpus_root())
        .unwrap_or(path)
        .to_path_buf();
    FileDiff {
        language: Language::from_path(path),
        old_path: None,
        kind: ChangeKind::Modified,
        hunks: vec![Hunk {
            new_start: 1,
            new_lines: after.lines().count().max(1),
            old_start: 1,
            old_lines: before.lines().count().max(1),
        }],
        new_source: Some(after.to_string()),
        old_source: Some(before.to_string()),
        path: relative,
    }
}

/// Everything is tagged agent-authored so authorship-scoped checks run.
fn agent_authorship(diffs: &[FileDiff]) -> AuthorshipMap {
    let mut map = AuthorshipMap::new();
    for diff in diffs {
        map.insert(
            diff.path.clone(),
            TaggedRange {
                start_line: 1,
                end_line: 100_000,
                tag: Tag::Confirmed,
                reason: "seeded corpus".to_string(),
            },
        );
    }
    map
}

/// Each case is analyzed in its own directory containing only that case's
/// files. The corpus deliberately reuses names across positive and negative
/// variants (two `render`s, two `fetchUser`s), and the repo-wide symbol table
/// treats a duplicated name as ambiguous and declines to fire. Analyzing the
/// whole corpus as one repository would therefore silence exactly the checks
/// under test — an artifact of the fixture layout, not of the code.
struct IsolatedCase {
    root: PathBuf,
}

impl IsolatedCase {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "dross-corpus-{}-{tag}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn place(&self, name: &str, source: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, source).unwrap();
        path
    }
}

impl Drop for IsolatedCase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn analyze_in(root: &Path, diffs: Vec<FileDiff>, index: Option<FingerprintIndex>) -> Vec<Finding> {
    let mut engine = Engine::new(Config::default());
    if let Some(index) = index {
        engine = engine.with_index(index);
    }
    let authorship = agent_authorship(&diffs);
    engine
        .analyze_diffs(root, &diffs, &authorship)
        .expect("analysis must not fail")
        .findings
}

fn analyze(diffs: Vec<FileDiff>, index: Option<FingerprintIndex>) -> Vec<Finding> {
    analyze_in(&corpus_root(), diffs, index)
}

/// Runs each file independently and in isolation, so neither a sibling
/// fixture's findings nor its name collisions can affect the result.
fn run_per_file(check: &str, polarity: &str) -> Vec<(PathBuf, Vec<Finding>)> {
    files_in(check, polarity)
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let case = IsolatedCase::new(&name.replace('.', "_"));
            let placed = case.place(&name, &source);

            let diff = FileDiff {
                language: Language::from_path(&placed),
                old_path: None,
                kind: ChangeKind::Added,
                hunks: vec![Hunk {
                    new_start: 1,
                    new_lines: source.lines().count().max(1),
                    old_start: 0,
                    old_lines: 0,
                }],
                new_source: Some(source.clone()),
                old_source: None,
                path: PathBuf::from(&name),
            };
            let findings = analyze_in(&case.root, vec![diff], None);
            (path, findings)
        })
        .collect()
}

fn has(findings: &[Finding], check: CheckId) -> bool {
    findings.iter().any(|f| f.check == check)
}

fn name_of(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().to_string()
}

// --- swallowed exception -------------------------------------------------

#[test]
fn catches_every_seeded_swallowed_exception() {
    let mut missed = Vec::new();
    for (path, findings) in run_per_file("swallowed-exception", "positive") {
        if !has(&findings, CheckId::SwallowedException) {
            missed.push(name_of(&path));
        }
    }
    assert!(missed.is_empty(), "false negatives: {missed:?}");
}

#[test]
fn does_not_flag_correct_error_handling() {
    let mut wrong = Vec::new();
    for (path, findings) in run_per_file("swallowed-exception", "negative") {
        for f in findings
            .iter()
            .filter(|f| f.check == CheckId::SwallowedException)
        {
            wrong.push(format!("{}: {}", name_of(&path), f.signal));
        }
    }
    assert!(wrong.is_empty(), "false positives: {wrong:?}");
}

// --- tautological test ---------------------------------------------------

#[test]
fn catches_every_seeded_tautological_test() {
    let mut missed = Vec::new();
    for (path, findings) in run_per_file("tautological-test", "positive") {
        if !has(&findings, CheckId::TautologicalTest) {
            missed.push(name_of(&path));
        }
    }
    assert!(missed.is_empty(), "false negatives: {missed:?}");
}

#[test]
fn does_not_flag_well_formed_tests() {
    let mut wrong = Vec::new();
    for (path, findings) in run_per_file("tautological-test", "negative") {
        for f in findings
            .iter()
            .filter(|f| f.check == CheckId::TautologicalTest)
        {
            wrong.push(format!("{}: {}", name_of(&path), f.signal));
        }
    }
    assert!(wrong.is_empty(), "false positives: {wrong:?}");
}

// --- contract change -----------------------------------------------------

fn run_contract_pairs(polarity: &str) -> Vec<(String, Vec<Finding>)> {
    let dir = corpus_root().join("contract-change").join(polarity);
    let mut pairs: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".before.ts").map(|s| s.to_string())
        })
        .collect();
    pairs.sort();

    pairs
        .into_iter()
        .map(|stem| {
            let before = std::fs::read_to_string(dir.join(format!("{stem}.before.ts"))).unwrap();
            let after = std::fs::read_to_string(dir.join(format!("{stem}.after.ts"))).unwrap();
            let path = dir.join(format!("{stem}.ts"));
            let findings = analyze(vec![modified_diff(&path, &before, &after)], None);
            (stem, findings)
        })
        .collect()
}

#[test]
fn catches_every_seeded_contract_change() {
    let mut missed = Vec::new();
    for (stem, findings) in run_contract_pairs("positive") {
        if !has(&findings, CheckId::ContractChange) {
            missed.push(stem);
        }
    }
    assert!(missed.is_empty(), "false negatives: {missed:?}");
}

#[test]
fn does_not_flag_compatible_signature_changes() {
    let mut wrong = Vec::new();
    for (stem, findings) in run_contract_pairs("negative") {
        for f in findings
            .iter()
            .filter(|f| f.check == CheckId::ContractChange)
        {
            wrong.push(format!("{stem}: {}", f.signal));
        }
    }
    assert!(wrong.is_empty(), "false positives: {wrong:?}");
}

// --- over-engineering ----------------------------------------------------

#[test]
fn catches_every_seeded_over_engineering_case() {
    let mut missed = Vec::new();
    for (path, findings) in run_per_file("over-engineering", "positive") {
        if !has(&findings, CheckId::OverEngineering) {
            missed.push(name_of(&path));
        }
    }
    assert!(missed.is_empty(), "false negatives: {missed:?}");
}

#[test]
fn does_not_flag_justified_abstractions() {
    let mut wrong = Vec::new();
    for (path, findings) in run_per_file("over-engineering", "negative") {
        for f in findings
            .iter()
            .filter(|f| f.check == CheckId::OverEngineering)
        {
            wrong.push(format!("{}: {}", name_of(&path), f.signal));
        }
    }
    assert!(wrong.is_empty(), "false positives: {wrong:?}");
}

// --- structural clone ----------------------------------------------------

/// The clone check needs the baseline indexed first, since it compares a
/// changed function against what the repository already contains.
fn run_clone_case(polarity: &str, candidate: &str) -> Vec<Finding> {
    let dir = corpus_root().join("structural-clone").join(polarity);
    let mut index = FingerprintIndex::open_in_memory().unwrap();
    let baseline_path = dir.join("_baseline.js");
    let baseline = std::fs::read_to_string(&baseline_path).unwrap();
    let baseline_rel = baseline_path.strip_prefix(corpus_root()).unwrap();
    index
        .index_file(baseline_rel, Language::JavaScript, &baseline)
        .unwrap();

    let path = dir.join(candidate);
    let source = std::fs::read_to_string(&path).unwrap();
    analyze(vec![whole_file_diff(&path, &source)], Some(index))
}

#[test]
fn catches_a_renamed_duplicate_of_an_indexed_function() {
    let findings = run_clone_case("positive", "renamed_duplicate.js");
    assert!(
        has(&findings, CheckId::StructuralClone),
        "false negative: renamed duplicate not detected"
    );
}

#[test]
fn does_not_flag_genuinely_different_logic() {
    let findings = run_clone_case("negative", "genuinely_different.js");
    let clones: Vec<&str> = findings
        .iter()
        .filter(|f| f.check == CheckId::StructuralClone)
        .map(|f| f.signal.as_str())
        .collect();
    assert!(clones.is_empty(), "false positives: {clones:?}");
}

// --- corpus integrity ----------------------------------------------------

#[test]
fn corpus_has_both_polarities_for_every_check() {
    for check in [
        "swallowed-exception",
        "tautological-test",
        "contract-change",
        "over-engineering",
        "structural-clone",
    ] {
        assert!(
            !files_in(check, "positive").is_empty(),
            "{check} has no positive cases"
        );
        assert!(
            !files_in(check, "negative").is_empty(),
            "{check} has no negative cases"
        );
    }
}
