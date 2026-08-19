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
const MIN_SHINGLES: usize = 12;

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
                let fp = fingerprint(parsed, &func);
                if fp.shingle_count < MIN_SHINGLES {
                    continue;
                }
                let Ok(hits) = index.find_similar(&fp, DEFAULT_THRESHOLD, Some(&file.path)) else {
                    continue;
                };
                let Some((twin, similarity)) = hits.into_iter().next() else {
                    continue;
                };

                let name = func.name.as_deref().unwrap_or("this function");
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
