/**
 * Translates between the settings the UI renders and the config the engine
 * reads.
 *
 * The design says every control applies immediately and there is no Save
 * button, which only holds if each change reaches `.dross.json` — otherwise a
 * toggle looks like it worked and the next CLI run ignores it.
 */

import type { CheckId, CheckRow, DrossConfig, SignalRow } from "./types";

/** Check ids the engine knows, keyed by the name shown in the checks grid. */
const CHECK_IDS: Record<string, CheckId> = {
  "swallowed-exception": "swallowed-exception",
  "contract-change": "contract-change",
  "over-engineering": "over-engineering",
  "tautological-test": "tautological-test",
  "structural-clone": "structural-clone",
};

/** Applies a stored config onto the rows the UI renders. */
export function applyConfig(
  config: DrossConfig,
  signals: SignalRow[],
  checks: CheckRow[],
): { signals: SignalRow[]; checks: CheckRow[] } {
  const disabledSignals = new Set(config.disabled_signals ?? []);
  const disabledChecks = new Set(config.disabled_checks ?? []);

  return {
    signals: signals.map((signal) => ({
      ...signal,
      on: !disabledSignals.has(signal.name),
    })),
    checks: checks.map((check) => {
      const id = CHECK_IDS[check.name];
      // A row with no engine counterpart — "complexity" — keeps its own state.
      return id ? { ...check, on: !disabledChecks.has(id) } : check;
    }),
  };
}

/** Folds the UI's rows back into a config the engine can store. */
export function toConfig(
  base: DrossConfig,
  next: {
    signals: SignalRow[];
    checks: CheckRow[];
    cloneThreshold: number;
    zThreshold: number;
    minSeverity: DrossConfig["min_severity"];
    commitGate: "advisory" | "block";
  },
): DrossConfig {
  return {
    ...base,
    disabled_signals: next.signals.filter((s) => !s.on).map((s) => s.name),
    disabled_checks: next.checks
      .filter((c) => !c.on)
      .map((c) => CHECK_IDS[c.name])
      .filter((id): id is CheckId => Boolean(id)),
    clone_threshold: next.cloneThreshold,
    complexity_z_threshold: next.zThreshold,
    min_severity: next.minSeverity,
    // Advisory is the default and stays the default: a hook that blocks on a
    // false positive gets uninstalled rather than tuned.
    block_at: next.commitGate === "block" ? "error" : null,
  };
}

/** One recorded analysis, recovered from the per-signal log rows. */
export interface Run {
  recordedAt: string;
  error: number;
  warning: number;
  info: number;
}

/**
 * Groups risk-history entries into per-run bars and rows.
 *
 * The log stores one row per signal per run, so entries are bucketed by
 * timestamp to recover the runs the chart draws.
 */
export function groupHistory(
  entries: { recorded_at: string; severity: string; count: number }[],
): Run[] {
  const runs = new Map<string, { error: number; warning: number; info: number }>();

  for (const entry of entries) {
    const key = entry.recorded_at;
    const run = runs.get(key) ?? { error: 0, warning: 0, info: 0 };
    if (entry.severity === "error") run.error += entry.count;
    else if (entry.severity === "warning") run.warning += entry.count;
    else run.info += entry.count;
    runs.set(key, run);
  }

  return [...runs.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .slice(-14)
    .map(([recordedAt, run]) => ({ recordedAt, ...run }));
}
