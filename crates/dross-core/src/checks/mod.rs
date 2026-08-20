//! The six checks and the context they share.

pub mod contract_change;
pub mod over_engineering;
pub mod structural_clone;
pub mod swallowed_exception;
pub mod tautological_test;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::ParsedFile;
use crate::authorship::{AuthorshipMap, Tag};
use crate::diff::FileDiff;
use crate::finding::{AuthorshipConfidence, CheckId, Finding};
use crate::index::FingerprintIndex;

/// Everything a check is allowed to see. Checks are pure over this — same
/// context in, same findings out, which is what makes runs reproducible.
pub struct CheckContext<'a> {
    pub repo_root: &'a Path,
    pub diffs: &'a [FileDiff],
    pub authorship: &'a AuthorshipMap,
    pub index: Option<&'a FingerprintIndex>,
    /// Tunables the checks read. Without this the configured thresholds are
    /// inert and the settings UI silently does nothing.
    pub config: &'a crate::config::Config,
    parsed: HashMap<PathBuf, ParsedFile>,
}

impl<'a> CheckContext<'a> {
    pub fn new(
        repo_root: &'a Path,
        diffs: &'a [FileDiff],
        authorship: &'a AuthorshipMap,
        index: Option<&'a FingerprintIndex>,
        config: &'a crate::config::Config,
    ) -> Self {
        // Parse each changed file once; every check reuses the same tree.
        let mut parsed = HashMap::new();
        for diff in diffs {
            let (Some(language), Some(source)) = (diff.language, diff.new_source.as_ref()) else {
                continue;
            };
            // The ignore list was only applied when walking the filesystem to
            // build the index, never to files arriving through the diff. A
            // repository that commits its build output therefore got its
            // bundles analyzed: lodash's `dist/lodash.min.js` alone produced
            // 1,575 findings, none of them about code anyone writes.
            if config.is_ignored(&diff.path) || is_generated(&diff.path, source) {
                continue;
            }
            if let Ok(file) = ParsedFile::parse(language, source.clone()) {
                parsed.insert(diff.path.clone(), file);
            }
        }
        Self {
            repo_root,
            diffs,
            authorship,
            index,
            config,
            parsed,
        }
    }

    pub fn parsed(&self, path: &Path) -> Option<&ParsedFile> {
        self.parsed.get(path)
    }

    /// Files with a parsed post-image — deletions and unsupported languages
    /// are filtered out.
    pub fn changed_files(&self) -> impl Iterator<Item = &FileDiff> {
        self.diffs
            .iter()
            .filter(|d| self.parsed.contains_key(&d.path))
    }

    /// Parses the pre-image of a file, for checks that compare revisions.
    pub fn parsed_old(&self, diff: &FileDiff) -> Option<ParsedFile> {
        let language = diff.language?;
        let source = diff.old_source.as_ref()?;
        ParsedFile::parse(language, source.clone()).ok()
    }

    pub fn tag_for(&self, path: &Path, line: usize) -> Tag {
        self.authorship.tag_for(path, line)
    }
}

/// Detects build output that lives outside an ignored directory.
///
/// Reviewing generated code is not the job — nobody edits a bundle by hand, so
/// a finding in one is noise by construction. Minified files are also
/// pathological input: everything looks duplicated and every function is huge.
pub fn is_generated(path: &Path, source: &str) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if name.contains(".min.") || name.ends_with(".bundle.js") || name.ends_with(".map") {
        return true;
    }

    // An explicit generator marker, conventionally in the first few lines.
    let head: String = source.lines().take(5).collect::<Vec<_>>().join(
        "
",
    );
    if head.contains("@generated")
        || head.contains("DO NOT EDIT")
        || head.contains("Code generated")
    {
        return true;
    }

    // Minified sources have very long lines. Hand-written code effectively
    // never averages this, even without a formatter.
    let mut lines = 0usize;
    let mut chars = 0usize;
    for line in source.lines().take(200) {
        lines += 1;
        chars += line.len();
    }
    lines > 0 && chars / lines > 400
}

pub trait Check: Send + Sync {
    fn id(&self) -> CheckId;
    fn run(&self, ctx: &CheckContext<'_>) -> Vec<Finding>;

    /// Checks that only make sense for agent-written code return false here.
    /// Per spec section 5, untagged hunks get the lighter four-check pass.
    fn applies_to_human_code(&self) -> bool {
        true
    }
}

pub fn all_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(swallowed_exception::SwallowedExceptionCheck),
        Box::new(structural_clone::StructuralCloneCheck),
        Box::new(tautological_test::TautologicalTestCheck),
        Box::new(contract_change::ContractChangeCheck),
        Box::new(over_engineering::OverEngineeringCheck),
    ]
}

/// Stamps each finding with the authorship confidence of the line it sits on.
pub fn annotate_authorship(findings: &mut [Finding], authorship: &AuthorshipMap) {
    for finding in findings.iter_mut() {
        let tag = authorship.tag_for(&finding.span.file, finding.span.start_line);
        finding.authorship = match tag {
            Tag::Confirmed => AuthorshipConfidence::Confirmed,
            Tag::Heuristic => AuthorshipConfidence::Heuristic,
            Tag::Human => AuthorshipConfidence::Unknown,
            Tag::UserOverride(_) => AuthorshipConfidence::UserOverride,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minified_bundles_are_treated_as_generated() {
        let long = format!("var a=1;{}", "b=2;".repeat(200));
        assert!(is_generated(Path::new("dist/lodash.min.js"), "x"));
        assert!(is_generated(Path::new("vendor/app.bundle.js"), "x"));
        assert!(is_generated(Path::new("src/anything.js"), &long));
    }

    #[test]
    fn generator_markers_are_honoured() {
        assert!(is_generated(
            Path::new("src/schema.py"),
            "# @generated by protoc
class A:
    pass
"
        ));
        assert!(is_generated(
            Path::new("src/api.ts"),
            "// DO NOT EDIT
export const x = 1;
"
        ));
    }

    #[test]
    fn hand_written_source_is_not_treated_as_generated() {
        let src = "export function add(a, b) {
  return a + b;
}
";
        assert!(!is_generated(Path::new("src/math.js"), src));
        assert!(!is_generated(Path::new("src/utils/minify.js"), src));
    }
}
