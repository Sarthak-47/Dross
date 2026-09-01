//! Structural clone detection (spec section 5).
//!
//! Scoped tightly: "did the agent reinvent a function that already exists in
//! this repo", not general copy-paste reporting. Only new/changed functions
//! are queried against the index, and only cross-file matches are reported.

use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::fingerprint::{fingerprint, shared_vocabulary, vocabulary, vocabulary_overlap};

use super::{Check, CheckContext};

/// Type-2/3 clone threshold. Tunable per spec; 0.85 is the launch default and
/// is what the benchmark run should calibrate.
pub const DEFAULT_THRESHOLD: f64 = 0.85;

/// Below this many distinct shingles a function is too small for a match to
/// mean anything — getters and one-line wrappers would otherwise dominate.
///
/// Raised from 12 after the benchmark corpus: small helpers matched each other
/// constantly and buried the findings worth reading.
const MIN_SHINGLES: usize = 24;

/// Constructors are structurally near-identical by nature — assign the
/// arguments to fields — so matching them says nothing about duplicated logic.
fn is_constructor(name: &str) -> bool {
    matches!(name, "__init__" | "constructor" | "__new__")
}

/// A shape that already exists this many times elsewhere is an established
/// convention rather than an accidental reinvention, and reporting it buries
/// the real findings.
///
/// This is the same idea the complexity baseline uses: judge the change
/// against how this repository actually works. In the benchmark corpus,
/// date-fns has ~200 locale directories each defining a structurally identical
/// `formatDistance`, and Flask defines four parallel `template_*` decorator
/// families. Both are deliberate parallel structure, and between them they
/// produced over 1,800 findings no reviewer would act on.
const MAX_ESTABLISHED_TWINS: usize = 3;

/// How much domain vocabulary a pair must share before a structural match
/// counts as a reinvention.
///
/// This is the discriminator the signal was missing, and the reason it measured
/// 0% across three rounds. Normalization erases identifiers, which is what lets
/// a renamed copy match its original — and also what makes two parallel
/// validators, or two adapters implementing one interface, look identical.
///
/// What survives a rename is the vocabulary: the members a function reaches for
/// and the functions it calls. The seeded duplicate renames every local but
/// still reads `.price` and `.quantity`, because it works on the same data.
/// Deliberate parallel structure over a different domain does not.
const MIN_VOCABULARY_OVERLAP: f64 = 0.5;

/// Below this, an overlap ratio is coincidence — two functions that both call
/// `push` are not the same logic.
const MIN_SHARED_TERMS: usize = 2;

pub struct StructuralCloneCheck;

impl Check for StructuralCloneCheck {
    fn id(&self) -> CheckId {
        CheckId::StructuralClone
    }

    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding> {
        let Some(index) = ctx.index else {
            return Vec::new();
        };
        let mut findings = Vec::new();

        for file in ctx.changed_files() {
            // Test helpers repeat by nature — the same fixture builder appears
            // in file after file — and consolidating them is rarely the right
            // call. Every sampled clone finding in test code was noise.
            if super::tautological_test::is_non_production_path(&file.path) {
                continue;
            }
            let Some(parsed) = ctx.parsed(&file.path) else {
                continue;
            };
            for func in parsed.functions() {
                if !file.touches_range(func.start_line, func.end_line) {
                    continue;
                }
                // An unnamed function produces an unactionable finding
                // ("this function duplicates ..."), and anonymous callbacks
                // are structurally identical constantly. They accounted for
                // 70% of clone findings across the benchmark corpus.
                let Some(func_name) = func.name.as_deref() else {
                    continue;
                };
                if is_constructor(func_name) {
                    continue;
                }

                let fp = fingerprint(parsed, &func);
                if fp.shingle_count < MIN_SHINGLES {
                    continue;
                }
                let threshold = ctx.config.clone_threshold;
                let Ok(hits) = index.find_similar(&fp, threshold, Some(&file.path)) else {
                    continue;
                };
                if hits.len() > MAX_ESTABLISHED_TWINS {
                    continue;
                }

                // Reinvention means someone wrote new logic without knowing the
                // existing implementation was there — and they would have given
                // it a different name. A twin with the *same* name is a port, a
                // monorepo copy, or a framework hook implemented once per
                // module: `pytest_configure` beside `pytest_configure`,
                // `readPackageJson` beside `readPackageJson`. Ten of the twelve
                // sampled clone findings were exactly that shape.
                //
                // This costs recall — a genuine duplicate that kept its name is
                // no longer reported — which is the trade this codebase makes
                // everywhere resolution is uncertain.
                let hits: Vec<_> = hits
                    .into_iter()
                    .filter(|(twin, _)| twin.name.as_deref() != Some(func_name))
                    .collect();
                // Structural identity is not enough on its own. Require the
                // pair to be talking about the same things as well as in the
                // same shape.
                let own_vocabulary = vocabulary(parsed, &func);
                let hits: Vec<_> = hits
                    .into_iter()
                    .filter(|(twin, _)| {
                        let shared = shared_vocabulary(&own_vocabulary, &twin.vocabulary);
                        shared.len() >= MIN_SHARED_TERMS
                            && vocabulary_overlap(&own_vocabulary, &twin.vocabulary)
                                >= MIN_VOCABULARY_OVERLAP
                    })
                    .collect();

                let Some((twin, similarity)) = hits.into_iter().next() else {
                    continue;
                };

                let name = func_name;
                let twin_name = twin.name.as_deref().unwrap_or("an existing function");
                let pct = (similarity * 100.0).round() as u32;
                let mut shared = shared_vocabulary(&own_vocabulary, &twin.vocabulary);
                shared.sort();
                shared.truncate(6);
                let shared_note = shared.join(", ");

                findings.push(
                    Finding::new(
                        CheckId::StructuralClone,
                        "near-duplicate-function",
                        Severity::Warning,
                        SourceSpan {
                            file: file.path.clone(),
                            start_line: func.start_line,
                            end_line: func.end_line,
                        },
                        format!("`{name}` duplicates existing logic in `{twin_name}`"),
                        format!(
                            "Normalized-AST similarity {pct}% against `{twin_name}` at {}:{}, \
                             and both reach for the same things: {shared_note}. Identifier \
                             and literal differences are ignored, so this is a structural \
                             match rather than a textual one — and the shared vocabulary is \
                             what separates a reinvention from parallel structure.",
                            twin.path.display(),
                            twin.start_line
                        ),
                    )
                    .with_related(vec![SourceSpan {
                        file: twin.path.clone(),
                        start_line: twin.start_line,
                        end_line: twin.end_line,
                    }]),
                );
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: anonymous functions produced "this function duplicates ..."
    /// — unactionable text — and anonymous callbacks match each other
    /// constantly. They were 70% of all clone findings across the benchmark
    /// corpus.
    #[test]
    fn anonymous_functions_are_not_reported() {
        // `name` is None for a bare callback, which is what the check now
        // requires before reporting.
        use crate::ast::ParsedFile;
        use crate::lang::Language;

        let src =
            "run(function (a, b) { let t = 0; for (const x of a) { t += x * b; } return t; });";
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let anonymous = file
            .functions()
            .into_iter()
            .find(|f| f.name.is_none())
            .expect("expected an anonymous function in the fixture");
        assert!(
            anonymous.name.is_none(),
            "fixture must produce an unnamed function"
        );
    }

    /// A shape that already appears many times is how the codebase works, so
    /// the check must stay silent; a one-off duplicate must still report.
    /// Exercised through the real index rather than asserted against the
    /// constant, so the behaviour is what is tested.
    #[test]
    fn established_patterns_are_suppressed_but_one_offs_still_report() {
        use crate::authorship::AuthorshipMap;
        use crate::config::Config;
        use crate::diff::{ChangeKind, FileDiff, Hunk};
        use crate::index::FingerprintIndex;
        use crate::lang::Language;
        use std::path::{Path, PathBuf};

        // Distinct identifiers, identical structure — the locale-file shape.
        let body = |n: usize| {
            format!(
                "export function format{n}(items) {{
  let total{n} = 0;
  for (const item of items) {{
    total{n} += item.price * item.quantity;
  }}
  if (total{n} > 10) {{
    return total{n} * 2;
  }}
  return total{n};
}}
"
            )
        };

        let candidate = body(99);
        let diffs = vec![FileDiff {
            path: PathBuf::from("candidate.js"),
            old_path: None,
            kind: ChangeKind::Added,
            language: Some(Language::JavaScript),
            hunks: vec![Hunk {
                new_start: 1,
                new_lines: candidate.lines().count(),
                old_start: 0,
                old_lines: 0,
            }],
            new_source: Some(candidate.clone()),
            old_source: None,
        }];

        let run_with = |copies: usize| {
            let mut index = FingerprintIndex::open_in_memory().unwrap();
            for i in 0..copies {
                index
                    .index_file(
                        Path::new(&format!("locale{i}.js")),
                        Language::JavaScript,
                        &body(i),
                    )
                    .unwrap();
            }
            let authorship = AuthorshipMap::new();
            let config = Config::default();
            let ctx = CheckContext::new(Path::new("."), &diffs, &authorship, Some(&index), &config);
            StructuralCloneCheck.run(&ctx).len()
        };

        assert_eq!(run_with(1), 1, "a single existing twin must be reported");
        assert_eq!(
            run_with(MAX_ESTABLISHED_TWINS + 2),
            0,
            "a shape repeated across the repository is a convention, not a clone"
        );
    }

    #[test]
    fn a_twin_with_the_same_name_is_not_reinvention() {
        use crate::authorship::AuthorshipMap;
        use crate::config::Config;
        use crate::diff::{ChangeKind, FileDiff, Hunk};
        use crate::index::FingerprintIndex;
        use crate::lang::Language;
        use std::path::{Path, PathBuf};

        let body = |name: &str| {
            format!(
                "export function {name}(items) {{
  let total = 0;
  for (const item of items) {{
    total += item.price * item.quantity;
  }}
  if (total > 10) {{
    return total * 2;
  }}
  return total;
}}
"
            )
        };

        let run = |candidate_name: &str| {
            let src = body(candidate_name);
            let mut index = FingerprintIndex::open_in_memory().unwrap();
            index
                .index_file(
                    Path::new("other.js"),
                    Language::JavaScript,
                    &body("computeTotal"),
                )
                .unwrap();
            let diffs = vec![FileDiff {
                path: PathBuf::from("candidate.js"),
                old_path: None,
                kind: ChangeKind::Added,
                language: Some(Language::JavaScript),
                hunks: vec![Hunk {
                    new_start: 1,
                    new_lines: src.lines().count(),
                    old_start: 0,
                    old_lines: 0,
                }],
                new_source: Some(src.clone()),
                old_source: None,
            }];
            let authorship = AuthorshipMap::new();
            let config = Config::default();
            let ctx = CheckContext::new(Path::new("."), &diffs, &authorship, Some(&index), &config);
            StructuralCloneCheck.run(&ctx).len()
        };

        assert_eq!(
            run("sumBasket"),
            1,
            "a differently named duplicate is reinvention"
        );
        assert_eq!(
            run("computeTotal"),
            0,
            "a same-named twin is a copy or a port"
        );
    }

    #[test]
    fn constructors_are_excluded() {
        assert!(is_constructor("__init__"));
        assert!(is_constructor("constructor"));
        assert!(!is_constructor("computeTotal"));
    }

    /// The corpus duplicate must still clear the raised size floor, otherwise
    /// the noise fix would have silently disabled the check.
    #[test]
    fn a_real_duplicate_still_clears_the_size_floor() {
        use crate::ast::ParsedFile;
        use crate::fingerprint::fingerprint;
        use crate::lang::Language;

        let src = "function computeTotal(items) {
  let total = 0;
  for (const item of items) {
    total += item.price * item.quantity;
  }
  return total;
}
";
        let file = ParsedFile::parse(Language::JavaScript, src).unwrap();
        let func = file.functions().into_iter().next().unwrap();
        let fp = fingerprint(&file, &func);
        assert!(
            fp.shingle_count >= MIN_SHINGLES,
            "a genuine duplicate ({} shingles) must not be filtered by MIN_SHINGLES ({})",
            fp.shingle_count,
            MIN_SHINGLES
        );
    }

    /// The failure that kept this signal at 0% across three rounds.
    ///
    /// date-fns alone produced 176 of 179 clone findings on the benchmark
    /// corpus, every one of them a sibling: `getSeconds` beside
    /// `getMilliseconds`, `lastWeek` beside `nextWeek`. Identical shape,
    /// different subject. Normalization erases exactly what tells them apart,
    /// so the vocabulary is compared alongside it.
    #[test]
    fn parallel_structure_over_a_different_subject_is_not_a_clone() {
        use crate::authorship::AuthorshipMap;
        use crate::config::Config;
        use crate::diff::{ChangeKind, FileDiff, Hunk};
        use crate::index::FingerprintIndex;
        use crate::lang::Language;
        use std::path::{Path, PathBuf};

        // Same shape throughout; only the subject changes.
        let shaped = |accessor: &str, helper: &str| {
            format!(
                "export function read{accessor}(source) {{
  let total = 0;
  for (const entry of source) {{
    total += entry.{accessor} * {helper}(entry);
  }}
  if (total > 10) {{
    return total * 2;
  }}
  return total;
}}
"
            )
        };

        let run = |candidate: String, indexed: String| {
            let mut index = FingerprintIndex::open_in_memory().unwrap();
            index
                .index_file(Path::new("other.js"), Language::JavaScript, &indexed)
                .unwrap();
            let diffs = vec![FileDiff {
                path: PathBuf::from("candidate.js"),
                old_path: None,
                kind: ChangeKind::Added,
                language: Some(Language::JavaScript),
                hunks: vec![Hunk {
                    new_start: 1,
                    new_lines: candidate.lines().count(),
                    old_start: 0,
                    old_lines: 0,
                }],
                new_source: Some(candidate),
                old_source: None,
            }];
            let authorship = AuthorshipMap::new();
            let config = Config::default();
            let ctx = CheckContext::new(Path::new("."), &diffs, &authorship, Some(&index), &config);
            StructuralCloneCheck.run(&ctx).len()
        };

        // Different members, different helpers: siblings, not a reinvention.
        assert_eq!(
            run(
                shaped("seconds", "scaleSeconds"),
                shaped("milliseconds", "scaleMilliseconds")
            ),
            0,
            "two parallel accessors were reported as duplicates of each other"
        );

        // Same members and helpers, different function name: the reinvention
        // this check exists for. Recall must survive the new filter.
        let original = "export function computeTotal(items) {
  let total = 0;
  for (const item of items) {
    total += item.price * normalise(item.quantity);
  }
  if (total > 10) {
    return total * 2;
  }
  return total;
}
";
        let renamed = "export function sumBasket(entries) {
  let acc = 0;
  for (const entry of entries) {
    acc += entry.price * normalise(entry.quantity);
  }
  if (acc > 10) {
    return acc * 2;
  }
  return acc;
}
";
        assert_eq!(
            run(renamed.to_string(), original.to_string()),
            1,
            "a renamed duplicate of the same logic must still be reported"
        );
    }
}
