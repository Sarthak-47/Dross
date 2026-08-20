// Mirrors the serde representations in dross-core, plus the view models the
// UI renders. Kept in one place so a backend rename surfaces as a type error
// rather than an undefined at runtime.

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
  /** Whether burst-write authorship detection has a live event source. */
  watcherActive: boolean;
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

/** Mirrors dross_core::config::Config. */
export interface DrossConfig {
  disabled_checks: CheckId[];
  disabled_signals: string[];
  min_severity: Severity;
  clone_threshold: number;
  complexity_z_threshold: number;
  ignore_dirs: string[];
  baseline_commits: number;
  block_at: Severity | null;
}

// --- view models -------------------------------------------------------

export type Tab = "findings" | "connections" | "history" | "settings";

export type Target = "working" | "staged";

/**
 * Which body the Findings tab shows. Derived in the app from repository
 * presence, index presence, in-flight work, baseline size and finding count —
 * never chosen by the user.
 */
export type ViewState =
  | "norepo"
  | "noindex"
  | "building"
  | "clean"
  | "findings"
  | "smallbase";

export type CodeLine = [number, string, string, number?];

export interface RelatedRef {
  key: string;
  value: string;
}

export interface Metric {
  label: string;
  value: string;
}

/** A finding with everything the source pane needs alongside it. */
export interface SeedFinding {
  severity: Severity;
  message: string;
  location: string;
  tags: string[];
  evidence: string;
  authorship: "confirmed" | "heuristic" | null;
  related: RelatedRef[];
  file: string;
  range: string;
  method: string;
  metrics: Metric[];
  code: CodeLine[];
}

export interface SkippedRow {
  check: string;
  reason: string;
}

export interface SignalRow {
  name: string;
  /** Measured precision, as a percentage. */
  precision: number;
  on: boolean;
  def: "on" | "off";
  rounds: string[];
  reason: string;
  heuristic?: boolean;
}

export interface CheckRow {
  name: string;
  note: string;
  on: boolean;
}

export type ConnectionStatus = "connected" | "detected" | "not found";

export interface ConnectionCard {
  name: string;
  status: ConnectionStatus;
  path: string;
  signal: string;
  limitation: string;
}

export type HistoryBar = [number, number, number, string];

export interface HistoryRow {
  when: string;
  sha: string;
  subject: string;
  e: number;
  w: number;
  i: number;
  risk: number;
}

export const AUTHORSHIP_TEXT = {
  confirmed: "agent-written (confirmed by trailer)",
  heuristic: "agent-written (heuristic — burst-write timing)",
} as const;
