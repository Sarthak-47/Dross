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
    parsed: HashMap<PathBuf, ParsedFile>,
}

impl<'a> CheckContext<'a> {
    pub fn new(
        repo_root: &'a Path,
        diffs: &'a [FileDiff],
        authorship: &'a AuthorshipMap,
        index: Option<&'a FingerprintIndex>,
    ) -> Self {
        // Parse each changed file once; every check reuses the same tree.
        let mut parsed = HashMap::new();
        for diff in diffs {
            let (Some(language), Some(source)) = (diff.language, diff.new_source.as_ref()) else {
                continue;
            };
            if let Ok(file) = ParsedFile::parse(language, source.clone()) {
                parsed.insert(diff.path.clone(), file);
            }
        }
        Self {
            repo_root,
            diffs,
            authorship,
            index,
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
