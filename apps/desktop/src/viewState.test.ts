import { describe, expect, it } from "vitest";
import { deriveView, MIN_BASELINE_SAMPLES } from "./viewState";
import type { RepositoryInfo } from "./types";

const repo = (over: Partial<RepositoryInfo> = {}): RepositoryInfo => ({
  root: "/tmp/x",
  name: "x",
  branch: "main",
  indexBuilt: true,
  indexedFunctions: 1204,
  baselineSamples: 412,
  watcherActive: true,
  ...over,
});

describe("deriveView", () => {
  it("shows the no-repository state before anything is open", () => {
    expect(deriveView({ repo: null, indexing: false, findingCount: null })).toBe("norepo");
  });

  it("reports building while an index is in flight, whatever else is true", () => {
    expect(
      deriveView({ repo: repo({ indexBuilt: false }), indexing: true, findingCount: 5 }),
    ).toBe("building");
  });

  it("asks for an index before anything else can run", () => {
    expect(
      deriveView({ repo: repo({ indexBuilt: false }), indexing: false, findingCount: null }),
    ).toBe("noindex");
  });

  /**
   * The bug this exists for: a repository with no analysis behind it used to
   * fall through to the split pane, which rendered seed fixtures as though
   * they were that repository's findings.
   */
  it("never falls through to the split pane before an analysis has run", () => {
    expect(deriveView({ repo: repo(), indexing: false, findingCount: null })).toBe(
      "unanalyzed",
    );
  });

  it("reports a clean analysis when the baseline is large enough", () => {
    expect(deriveView({ repo: repo(), indexing: false, findingCount: 0 })).toBe("clean");
  });

  /**
   * The other bug: this was checked after "clean", and both require an empty
   * finding list, so it could never be reached.
   */
  it("prefers the small-baseline state over clean, because it says more", () => {
    expect(
      deriveView({
        repo: repo({ baselineSamples: MIN_BASELINE_SAMPLES - 1 }),
        indexing: false,
        findingCount: 0,
      }),
    ).toBe("smallbase");
  });

  it("treats exactly the minimum sample count as sufficient", () => {
    expect(
      deriveView({
        repo: repo({ baselineSamples: MIN_BASELINE_SAMPLES }),
        indexing: false,
        findingCount: 0,
      }),
    ).toBe("clean");
  });

  it("shows the split pane once there is something to show", () => {
    expect(deriveView({ repo: repo(), indexing: false, findingCount: 3 })).toBe("findings");
  });

  it("still shows findings when the baseline is too small to score complexity", () => {
    expect(
      deriveView({
        repo: repo({ baselineSamples: 2 }),
        indexing: false,
        findingCount: 3,
      }),
    ).toBe("findings");
  });
});
