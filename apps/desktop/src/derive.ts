/**
 * Maps engine data into what the History and Connections views render.
 *
 * Extracted for the same reason `deriveView` was: the bug these had lived in
 * the mapping, not in the rendering. Both used to substitute data from the
 * design handoff whenever the real data was absent, so a freshly opened
 * repository was shown six invented commits with invented SHAs, and
 * integrations that had never been probed were drawn as connected.
 *
 * The rule these encode: absent data produces an empty list, never a stand-in.
 */

import type {
  AdapterStatus,
  CodeLine,
  ConnectionCard,
  HistoryBar,
  HistoryRow,
} from "./types";
import type { Run } from "./settingsSync";

/** How many recorded runs the table lists. The chart shows all of them. */
export const HISTORY_ROW_LIMIT = 6;

export function toHistoryBars(runs: Run[]): HistoryBar[] {
  return runs.map((run) => ({
    // The timestamp, not the day: two runs on the same date would otherwise
    // collide as React keys and one would be dropped.
    id: run.recordedAt,
    error: run.error,
    warning: run.warning,
    info: run.info,
    day: run.recordedAt.slice(8, 10),
  }));
}

export function toHistoryRows(runs: Run[]): HistoryRow[] {
  return [...runs]
    .reverse()
    .slice(0, HISTORY_ROW_LIMIT)
    .map((run) => ({
      id: run.recordedAt,
      // Split rather than sliced to a fixed width: replacing "T" with " · "
      // makes the string longer than the timestamp, so slice(0, 16) cut the
      // clock in half and the column read "2026-08-12 · 09:".
      when: `${run.recordedAt.split("T")[0]} · ${(run.recordedAt.split("T")[1] ?? "").slice(0, 5)}`,
      subject: `${run.error + run.warning + run.info} findings recorded`,
      e: run.error,
      w: run.warning,
      i: run.info,
      risk: Math.min(run.error * 25 + run.warning * 8 + run.info * 2, 100),
    }));
}

/**
 * @param adapters null when detection has not run — before a repository is
 * open, or when the probe failed. Either way nothing is known, so nothing is
 * claimed.
 */
export function toConnectionCards(adapters: AdapterStatus[] | null): ConnectionCard[] {
  if (!adapters) return [];
  return adapters.map((a) => ({
    name: a.label,
    status: a.installed ? "connected" : a.detected ? "detected" : "not found",
    path: a.config_path ?? "—",
    signal: a.installed ? "wired in · runs dross --staged" : "not wired in",
    limitation:
      a.limitations[0] ??
      "No limitation recorded for this integration on this platform.",
  }));
}

/** Lines of context shown either side of a finding's own range. */
export const SOURCE_CONTEXT = 8;

/**
 * The slice of a file the source pane shows for one finding.
 *
 * The marker column is "›" on the finding's own lines rather than "+". Dross
 * reports on ranges the diff touched, but a touched range is not necessarily
 * an added one, and "+" would assert something about the line that this side
 * of the app does not know.
 *
 * @param startLine 1-based, inclusive, as spans are reported.
 */
export function sourceWindow(
  source: string,
  startLine: number,
  endLine: number,
  context = SOURCE_CONTEXT,
): CodeLine[] {
  const lines = source.split(/\r?\n/);
  // A span past the end of the file means the index is stale against what is
  // on disk. Showing nothing is right; guessing at a location is not.
  if (startLine > lines.length || startLine < 1) return [];

  const last = Math.min(endLine, lines.length);
  const from = Math.max(1, startLine - context);
  const to = Math.min(lines.length, last + context);

  const out: CodeLine[] = [];
  for (let n = from; n <= to; n += 1) {
    const inFinding = n >= startLine && n <= last;
    out.push(inFinding ? [n, "›", lines[n - 1], 1] : [n, " ", lines[n - 1]]);
  }
  return out;
}

/**
 * The engine's own weights, from `risk_score` in engine.rs: errors dominate,
 * info barely moves it, and the total is capped at 100.
 *
 * These must stay equal to it. They read 20/12/4 while the engine used 25/8/2,
 * so the formula printed under the score — "weighted: …error(20) + …" — did not
 * produce the score displayed beside it, and the severity bar's proportions
 * were wrong with it.
 */
export const WEIGHT = { error: 25, warning: 8, info: 2 } as const;

/**
 * How long a run took, for the status bar.
 *
 * Seconds to one decimal reads "0.0s" for anything under 50ms — which is the
 * normal case, and made a 41ms run look like it had not happened. Below a
 * second the honest unit is milliseconds.
 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}
