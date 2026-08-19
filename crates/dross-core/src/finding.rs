//! The finding model shared by every check, the Tauri UI, and the CLI.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckId {
    SwallowedException,
    StructuralClone,
    TautologicalTest,
    ContractChange,
    OverEngineering,
}

impl CheckId {
    pub fn as_str(self) -> &'static str {
        match self {
            CheckId::SwallowedException => "swallowed-exception",
            CheckId::StructuralClone => "structural-clone",
            CheckId::TautologicalTest => "tautological-test",
            CheckId::ContractChange => "contract-change",
            CheckId::OverEngineering => "over-engineering",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// How sure we are that the hunk this finding sits on was agent-written.
///
/// Spec section 5 calls authorship tagging "the architectural core", but the
/// detection is heuristic. Surfacing the confidence keeps a mistag a visible,
/// correctable UI state instead of a silent under-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorshipConfidence {
    /// A commit trailer or tool session marker named the agent explicitly.
    Confirmed,
    /// Burst-write timestamps suggested an agent, but nothing confirmed it.
    Heuristic,
    /// No agent signal; treated as human-written.
    Unknown,
    /// The user overrode the detected value in the UI.
    UserOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub check: CheckId,
    /// Stable sub-signal identifier, e.g. "empty-catch-body". Lets the
    /// benchmark report precision per signal, not just per check.
    pub signal: String,
    pub severity: Severity,
    pub span: SourceSpan,
    pub message: String,
    /// Why this was flagged, in terms the user can verify by looking at code.
    pub evidence: String,
    pub authorship: AuthorshipConfidence,
    /// Secondary locations that explain the finding (the clone twin, the
    /// existing implementation, the single call site).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<SourceSpan>,
}

impl Finding {
    pub fn new(
        check: CheckId,
        signal: impl Into<String>,
        severity: Severity,
        span: SourceSpan,
        message: impl Into<String>,
        evidence: impl Into<String>,
    ) -> Self {
        Self {
            check,
            signal: signal.into(),
            severity,
            span,
            message: message.into(),
            evidence: evidence.into(),
            authorship: AuthorshipConfidence::Unknown,
            related: Vec::new(),
        }
    }

    pub fn with_authorship(mut self, authorship: AuthorshipConfidence) -> Self {
        self.authorship = authorship;
        self
    }

    pub fn with_related(mut self, related: Vec<SourceSpan>) -> Self {
        self.related = related;
        self
    }
}
