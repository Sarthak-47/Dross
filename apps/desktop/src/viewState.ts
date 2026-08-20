/**
 * Chooses which body the Findings tab shows.
 *
 * Pulled out of the component because both of its bugs were ordering mistakes
 * that no rendering test would have caught: the small-baseline state sat after
 * the clean state and could never win, and a repository with no analysis fell
 * through to the split pane, which rendered seed fixtures as though they were
 * that repository's findings.
 */

import type { RepositoryInfo, ViewState } from "./types";

/** Below this many samples a z-score says more about the sample than the code. */
export const MIN_BASELINE_SAMPLES = 30;

export interface ViewInputs {
  repo: RepositoryInfo | null;
  indexing: boolean;
  /** Null until an analysis has run. */
  findingCount: number | null;
}

export function deriveView({ repo, indexing, findingCount }: ViewInputs): ViewState {
  if (!repo) return "norepo";
  if (indexing) return "building";
  if (!repo.indexBuilt) return "noindex";

  // No analysis behind it means there is nothing to show. Never fall through
  // to the split pane here — presenting invented findings for somebody's real
  // code would be worse than presenting nothing.
  if (findingCount === null) return "unanalyzed";

  if (findingCount === 0) {
    // Checked before "clean": a complexity signal that is staying silent is
    // the more specific fact, and "clean" would otherwise swallow it.
    return repo.baselineSamples < MIN_BASELINE_SAMPLES ? "smallbase" : "clean";
  }

  return "findings";
}
