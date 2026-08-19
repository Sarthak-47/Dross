// Mirrors the serde representations in dross-core. Kept in one place so a
// backend rename surfaces as a type error rather than an undefined at runtime.

export type Severity = "info" | "warning" | "error";

export type CheckId =
  | "swallowed-exception"
  | "structural-clone"
  | "tautological-test"
  | "contract-change"
  | "over-engineering";

export type AuthorshipConfidence =
  | "confirmed"
  | "heuristic"
  | "unknown"
  | "user-override";

export interface SourceSpan {
  file: string;
  start_line: number;
  end_line: number;
}

export interface Finding {
  check: CheckId;
  signal: string;
  severity: Severity;
  span: SourceSpan;
  message: string;
  evidence: string;
  authorship: AuthorshipConfidence;
  related?: SourceSpan[];
}

export interface SkippedCheck {
  check: string;
  reason: string;
}

export interface Report {
  findings: Finding[];
  files_analyzed: number;
  duration_ms: number;
  risk_score: number;
  skipped: SkippedCheck[];
}

export interface RepositoryInfo {
  root: string;
  name: string;
  indexBuilt: boolean;
  indexedFunctions: number;
  baselineSamples: number;
}

export type AdapterId =
  | "claude-code"
  | "codex-cli"
  | "antigravity"
  | "git-hook";

export interface AdapterStatus {
  id: AdapterId;
  label: string;
  detected: boolean;
  installed: boolean;
  config_path: string | null;
  limitations: string[];
}

export interface RiskEntry {
  recorded_at: string;
  commit_sha: string | null;
  check_id: string;
  signal: string;
  severity: string;
  count: number;
}

export interface IndexProgress {
  done: number;
  total: number;
  phase: string;
}

export const CHECK_LABELS: Record<CheckId, string> = {
  "swallowed-exception": "Swallowed exception",
  "structural-clone": "Structural clone",
  "tautological-test": "Tautological test",
  "contract-change": "Contract change",
  "over-engineering": "Over-engineering",
};

export const AUTHORSHIP_LABELS: Record<AuthorshipConfidence, string> = {
  confirmed: "Agent-written (confirmed)",
  heuristic: "Agent-written (heuristic)",
  unknown: "Unattributed",
  "user-override": "Manually tagged",
};
