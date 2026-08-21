import { describe, expect, it } from "vitest";
import { HISTORY_ROW_LIMIT, toConnectionCards, toHistoryBars, toHistoryRows } from "./derive";
import type { AdapterStatus } from "./types";
import type { Run } from "./settingsSync";

const run = (recordedAt: string, error = 0, warning = 0, info = 0): Run => ({
  recordedAt,
  error,
  warning,
  info,
});

const adapter = (over: Partial<AdapterStatus> = {}): AdapterStatus => ({
  id: "claude-code",
  label: "Claude Code",
  detected: false,
  installed: false,
  config_path: null,
  limitations: [],
  ...over,
});

/**
 * The bug this file exists for: every one of these mappings used to fall back
 * to data from the design handoff when the real data was missing, so the app
 * showed invented commits and reported integrations as connected that had
 * never been probed.
 */
describe("no fabricated data reaches a view", () => {
  it("renders no history bars when nothing has been recorded", () => {
    expect(toHistoryBars([])).toEqual([]);
  });

  it("renders no history rows when nothing has been recorded", () => {
    expect(toHistoryRows([])).toEqual([]);
  });

  it("renders no connection cards before detection has run", () => {
    expect(toConnectionCards(null)).toEqual([]);
  });

  it("renders no connection cards when detection found nothing", () => {
    expect(toConnectionCards([])).toEqual([]);
  });
});

describe("toHistoryBars", () => {
  it("carries each run's counts through", () => {
    const [bar] = toHistoryBars([run("2026-08-21T10:00:00Z", 2, 3, 1)]);
    expect(bar).toMatchObject({ error: 2, warning: 3, info: 1, day: "21" });
  });

  /** Keyed on the day, two runs on one date collided and React dropped one. */
  it("gives two runs on the same day distinct identities", () => {
    const bars = toHistoryBars([
      run("2026-08-21T10:00:00Z", 1),
      run("2026-08-21T18:30:00Z", 2),
    ]);
    expect(bars).toHaveLength(2);
    expect(bars[0].id).not.toBe(bars[1].id);
    expect(bars[0].day).toBe(bars[1].day);
  });
});

describe("toHistoryRows", () => {
  it("lists the most recent runs first", () => {
    const rows = toHistoryRows([
      run("2026-08-19T10:00:00Z", 1),
      run("2026-08-21T10:00:00Z", 5),
    ]);
    expect(rows[0].e).toBe(5);
  });

  it("caps the table without capping the chart", () => {
    const runs = Array.from({ length: 14 }, (_, i) =>
      run(`2026-08-${String(i + 1).padStart(2, "0")}T00:00:00Z`, 1),
    );
    expect(toHistoryRows(runs)).toHaveLength(HISTORY_ROW_LIMIT);
    expect(toHistoryBars(runs)).toHaveLength(14);
  });

  it("does not invent a commit sha the log never recorded", () => {
    expect(toHistoryRows([run("2026-08-21T10:00:00Z", 1)])[0].sha).toBe("—");
  });

  it("bounds the risk score at 100", () => {
    expect(toHistoryRows([run("2026-08-21T10:00:00Z", 40)])[0].risk).toBe(100);
  });

  it("does not mutate the run list it is given", () => {
    const runs = [run("2026-08-19T10:00:00Z"), run("2026-08-21T10:00:00Z")];
    toHistoryRows(runs);
    expect(runs[0].recordedAt).toBe("2026-08-19T10:00:00Z");
  });
});

describe("toConnectionCards", () => {
  it("reports an installed adapter as connected", () => {
    const [card] = toConnectionCards([adapter({ installed: true, detected: true })]);
    expect(card.status).toBe("connected");
    expect(card.signal).toContain("wired in");
  });

  it("distinguishes a detected but unwired tool from an absent one", () => {
    expect(toConnectionCards([adapter({ detected: true })])[0].status).toBe("detected");
    expect(toConnectionCards([adapter()])[0].status).toBe("not found");
  });

  it("shows a dash rather than a path it does not have", () => {
    expect(toConnectionCards([adapter()])[0].path).toBe("—");
  });

  it("always has limitation copy, because the card always prints it", () => {
    expect(toConnectionCards([adapter()])[0].limitation).not.toBe("");
  });
});
