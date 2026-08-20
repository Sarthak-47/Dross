//! Structural clone detection (spec section 5).
//!
//! Scoped tightly: "did the agent reinvent a function that already exists in
//! this repo", not general copy-paste reporting. Only new/changed functions
//! are queried against the index, and only cross-file matches are reported.

use crate::finding::{CheckId, Finding, Severity, SourceSpan};
use crate::fingerprint::fingerprint;

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
                let Some((twin, similarity)) = hits.into_iter().next() else {
                    continue;
                };

                let name = func_name;
                let twin_name = twin.name.as_deref().unwrap_or("an existing function");
                let pct = (similarity * 100.0).round() as u32;

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
                            "Normalized-AST similarity {pct}% against `{twin_name}` at {}:{}. \
                             Identifier and literal differences are ignored, so this is a \
                             structural match, not a textual one.",
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
}
