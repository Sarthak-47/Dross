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

import type { AdapterStatus, ConnectionCard, HistoryBar, HistoryRow } from "./types";
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
      when: run.recordedAt.replace("T", " · ").slice(0, 16),
      // The log records findings per run, not the commit they belong to.
      // Showing a dash is honest; inventing a sha would not be.
      sha: "—",
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
