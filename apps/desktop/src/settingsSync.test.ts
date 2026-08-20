import { describe, expect, it } from "vitest";
import { applyConfig, groupHistory, toConfig } from "./settingsSync";
import type { CheckRow, DrossConfig, SignalRow } from "./types";

const config = (over: Partial<DrossConfig> = {}): DrossConfig => ({
  disabled_checks: [],
  disabled_signals: [],
  min_severity: "info",
  clone_threshold: 0.85,
  complexity_z_threshold: 2.5,
  ignore_dirs: [],
  baseline_commits: 200,
  block_at: null,
  ...over,
});

const signal = (name: string, on: boolean): SignalRow => ({
  name,
  precision: 90,
  on,
  def: "on",
  rounds: [],
  reason: "",
});

const check = (name: string, on: boolean): CheckRow => ({ name, note: "", on });

describe("applyConfig", () => {
  it("turns a stored disabled list into row state", () => {
    const applied = applyConfig(
      config({ disabled_signals: ["log-only-catch"], disabled_checks: ["structural-clone"] }),
      [signal("log-only-catch", true), signal("empty-catch-body", true)],
      [check("structural-clone", true), check("contract-change", true)],
    );

    expect(applied.signals.find((s) => s.name === "log-only-catch")?.on).toBe(false);
    expect(applied.signals.find((s) => s.name === "empty-catch-body")?.on).toBe(true);
    expect(applied.checks.find((c) => c.name === "structural-clone")?.on).toBe(false);
    expect(applied.checks.find((c) => c.name === "contract-change")?.on).toBe(true);
  });

  it("leaves a row with no engine counterpart alone", () => {
    // "complexity" is a UI grouping, not a CheckId.
    const applied = applyConfig(config(), [], [check("complexity", false)]);
    expect(applied.checks[0].on).toBe(false);
  });
});

describe("toConfig", () => {
  it("writes the disabled rows back, not the enabled ones", () => {
    const next = toConfig(config(), {
      signals: [signal("a", true), signal("b", false)],
      checks: [check("structural-clone", false), check("contract-change", true)],
      cloneThreshold: 0.74,
      zThreshold: 3.5,
      minSeverity: "warning",
      commitGate: "advisory",
    });

    expect(next.disabled_signals).toEqual(["b"]);
    expect(next.disabled_checks).toEqual(["structural-clone"]);
    expect(next.clone_threshold).toBe(0.74);
    expect(next.complexity_z_threshold).toBe(3.5);
    expect(next.min_severity).toBe("warning");
  });

  /** Advisory is the default and must stay it: a hook that blocks on a false
   * positive gets uninstalled rather than tuned. */
  it("maps the commit gate to a blocking severity only when asked", () => {
    const base = {
      signals: [],
      checks: [],
      cloneThreshold: 0.85,
      zThreshold: 2.5,
      minSeverity: "info" as const,
    };
    expect(toConfig(config(), { ...base, commitGate: "advisory" }).block_at).toBeNull();
    expect(toConfig(config(), { ...base, commitGate: "block" }).block_at).toBe("error");
  });

  it("preserves fields the UI does not own", () => {
    const next = toConfig(config({ ignore_dirs: ["vendor"], baseline_commits: 42 }), {
      signals: [],
      checks: [],
      cloneThreshold: 0.85,
      zThreshold: 2.5,
      minSeverity: "info",
      commitGate: "advisory",
    });
    expect(next.ignore_dirs).toEqual(["vendor"]);
    expect(next.baseline_commits).toBe(42);
  });

  it("round-trips through applyConfig", () => {
    const signals = [signal("a", true), signal("b", false)];
    const checks = [check("contract-change", true)];
    const stored = toConfig(config(), {
      signals,
      checks,
      cloneThreshold: 0.9,
      zThreshold: 2,
      minSeverity: "error",
      commitGate: "block",
    });
    const back = applyConfig(stored, signals, checks);
    expect(back.signals.map((s) => s.on)).toEqual([true, false]);
    expect(back.checks[0].on).toBe(true);
  });
});

describe("groupHistory", () => {
  /** The log stores one row per signal per run, so runs are recovered by
   * grouping on the timestamp. */
  it("folds per-signal rows into one run each", () => {
    const runs = groupHistory([
      { recorded_at: "2026-08-20T10:00:00Z", severity: "error", count: 2 },
      { recorded_at: "2026-08-20T10:00:00Z", severity: "warning", count: 3 },
      { recorded_at: "2026-08-21T10:00:00Z", severity: "info", count: 1 },
    ]);

    expect(runs).toHaveLength(2);
    expect(runs[0]).toMatchObject({ error: 2, warning: 3, info: 0 });
    expect(runs[1]).toMatchObject({ error: 0, warning: 0, info: 1 });
  });

  it("orders oldest first and keeps only the most recent fourteen", () => {
    const entries = Array.from({ length: 20 }, (_, i) => ({
      recorded_at: `2026-08-${String(i + 1).padStart(2, "0")}T00:00:00Z`,
      severity: "error",
      count: 1,
    }));
    const runs = groupHistory(entries);
    expect(runs).toHaveLength(14);
    expect(runs[0].recordedAt < runs[runs.length - 1].recordedAt).toBe(true);
    expect(runs[runs.length - 1].recordedAt).toContain("2026-08-20");
  });

  it("returns nothing for an empty log rather than inventing a run", () => {
    expect(groupHistory([])).toEqual([]);
  });
});
