import { describe, expect, it } from "vitest";
import {
  HISTORY_ROW_LIMIT,
  sourceWindow,
  toConnectionCards,
  toHistoryBars,
  toHistoryRows,
} from "./derive";
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

describe("sourceWindow", () => {
  const file = Array.from({ length: 40 }, (_, i) => `line ${i + 1}`).join("\n");

  /** The bug this covers: the split pane's right half rendered an empty code
   * block for every real finding, because nothing ever read the file. */
  it("returns the finding's lines plus context either side", () => {
    const win = sourceWindow(file, 20, 22, 3);
    expect(win.map((l) => l[0])).toEqual([17, 18, 19, 20, 21, 22, 23, 24, 25]);
    expect(win.find((l) => l[0] === 20)?.[2]).toBe("line 20");
  });

  it("marks only the finding's own lines as the hit", () => {
    const win = sourceWindow(file, 20, 22, 3);
    expect(win.filter((l) => l[3]).map((l) => l[0])).toEqual([20, 21, 22]);
    expect(win.filter((l) => !l[3]).every((l) => l[1] === " ")).toBe(true);
  });

  it("clamps at the start of the file", () => {
    expect(sourceWindow(file, 2, 2, 8).map((l) => l[0])[0]).toBe(1);
  });

  it("clamps at the end of the file", () => {
    const win = sourceWindow(file, 38, 40, 8);
    expect(win[win.length - 1][0]).toBe(40);
  });

  /** A span past the end means the index is stale against what is on disk. */
  it("returns nothing rather than guessing when the span is out of range", () => {
    expect(sourceWindow(file, 100, 102)).toEqual([]);
    expect(sourceWindow(file, 0, 1)).toEqual([]);
  });

  it("truncates an end line that runs past the file", () => {
    const win = sourceWindow(file, 39, 60, 0);
    expect(win.map((l) => l[0])).toEqual([39, 40]);
  });

  it("reads a file with Windows line endings", () => {
    const crlf = "alpha\r\nbeta\r\ngamma";
    expect(sourceWindow(crlf, 2, 2, 1).map((l) => l[2])).toEqual([
      "alpha",
      "beta",
      "gamma",
    ]);
  });

  it("handles a single-line file", () => {
    expect(sourceWindow("only", 1, 1)).toEqual([[1, "\u203a", "only", 1]]);
  });
});
