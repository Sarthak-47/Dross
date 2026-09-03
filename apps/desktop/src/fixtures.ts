/* The signal table.
 *
 * These precision figures are the ones Dross actually measured across the
 * benchmark corpus, read from docs/benchmark-report-final.json. The table's own
 * copy says the number is "what it actually scored here, not a claim", so
 * placeholder figures would make the product's central claim untrue.
 *
 * This file used to also carry seed findings, connection cards and a risk
 * history from the design handoff, which the app fell back to whenever real
 * data was absent. That meant a freshly opened repository was shown six
 * invented commits with invented SHAs, and integrations it did not have were
 * reported as connected. For a tool that sells determinism and measurement,
 * presenting fabricated data as the user's own is the worst available failure,
 * so the fallbacks and the data behind them are both gone: the views now say
 * they have nothing to show. */

import type { CheckRow, SignalRow } from "./types";

export const SIGNALS: SignalRow[] = [
  {
    name: "parameter-type-changed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 83%", "r3 92%", "r4 100%"],
    reason:
      "A signature is a fact about the parse tree, not an estimate. The remaining risk is comparison rather than detection: union members are sorted before comparing, because reordering A | B | None to None | A | B changed nothing a caller could observe and was being reported as a change.",
  },
  {
    name: "required-parameter-added",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 67%", "r3 75%", "r4 100%"],
    reason:
      "Every caller that omitted the argument breaks. Test functions are excluded, because a fixture parameter injected by a framework has no external callers to break — that exclusion was the whole gap between 67% and 100%.",
  },
  {
    name: "return-type-changed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 100%", "r3 92%", "r4 100%"],
    reason:
      "Callers written against the old type may mishandle the new one with nothing failing at the call site.",
  },
  {
    name: "parameter-removed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 100%", "r3 92%", "r4 100%"],
    reason:
      "Call sites still passing the argument silently pass an ignored value in JavaScript, or fail outright in typed code.",
  },
  {
    name: "optional-parameter-became-required",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 92%", "r3 83%", "r4 100%"],
    reason: "Existing call sites that omitted the argument now break.",
  },
  {
    name: "became-async",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 25%", "r3 67%", "r4 100%"],
    reason:
      "Callers that do not await receive a promise instead of a value, which in JavaScript fails at use rather than at the call site. Early rounds were dominated by test suites migrating to async, which are now excluded.",
  },
  {
    name: "empty-catch-body",
    precision: 92,
    on: true,
    def: "on",
    rounds: ["r2 8%", "r3 45%", "r4 92%"],
    reason:
      "An empty handler carrying a comment is a decision somebody wrote down, and a narrow except ValueError names the one thing the author expected. Both are excluded now; what remains is a broad or untyped handler discarding a failure without saying so.",
  },
  {
    name: "parameter-type-removed",
    precision: 92,
    on: true,
    def: "on",
    rounds: ["r2 50%", "r3 42%", "r4 92%"],
    reason:
      "A parameter losing its annotation stops the contract being enforced. Reporting self was an artifact and is gone.",
  },
  {
    name: "unused-generality",
    precision: 86,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 83%", "r4 86%"],
    reason:
      "Only a literal counts. Earlier rounds reported that a parameter was always exc_info — a variable that happened to share the parameter's name, which says nothing about whether the parameter is exercised.",
  },
  {
    name: "overly-broad-catch-type",
    precision: 75,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 100%", "r4 75%"],
    reason:
      "Breadth alone is a style opinion. Every finding in the first round was a top-level request handler that must catch everything and then delegates. It is reported now only when the broad catch also fails to surface what it caught.",
  },
  {
    name: "pass-through-wrapper",
    precision: 67,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 50%", "r4 67%"],
    reason:
      "A wrapper has to forward its own parameters unchanged. Binding an argument is specialisation, and a body that is one call taking a callback keeps its logic in the callback — both were being reported as indirection.",
  },
  {
    name: "log-only-catch",
    precision: 42,
    on: true,
    def: "on",
    rounds: ["r2 25%", "r3 50%", "r4 42%"],
    reason:
      "The weakest signal still shipping on. Emitting an error event, handing the error to a callback and passing it to a reporter all count as surfacing, and a handler whose log message says the failure is expected is now read as documenting itself. Judging whether a debug line is sufficient still needs intent the syntax does not carry. This number predates that last change and is a floor, not a current measurement.",
  },
  {
    name: "near-duplicate-function",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 8%", "r3 8%", "r4 0%"],
    reason:
      "Structural identity alone could not tell an accidental reinvention from deliberate parallel structure — two adapters, a locale family, a pair of validators. It now also compares the vocabulary a function uses: the members it reaches for and the functions it calls, which is what survives a rename. That cut finding volume on the corpus by 77%, but the output has not been labelled since, so the number beside this signal is still the last one measured and it stays off.",
  },
  {
    name: "silent-optimistic-return",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 0%"],
    reason:
      "Returning a default on failure is the documented contract far more often than it is a concealment. Two shapes say so outright and are excluded now: a name that promises a safe or optional result — stringifySafely, get_or_none, try_parse — and a handler returning the same value the function already returns elsewhere, which every caller therefore handles. Volume fell 76% on the corpus; the remainder has not been labelled.",
  },
  {
    name: "overkill-design-pattern",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 —"],
    reason:
      "Zero true positives across 24 labelled findings, and three attempts to fix it left the volume higher than it started. Each made the per-branch definition more defensible, but the signal fires on exactly one variant, so any change to how variants are counted moves functions into the bucket as readily as out of it. The premise is what fails: an ordinary constructor has branches too.",
  },
  {
    name: "single-implementation-abstraction",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 —"],
    reason:
      "Zero of 24. What it found were published extension points subclassed by consumers the repository cannot see. A type named Base*, Abstract* or *Base is the author saying \"subclass this\", and those are excluded now — every finding on the corpus was one, and the volume fell by 44%. Still off: the remainder has not been labelled.",
  },
  {
    name: "complexity-to-problem-size-outlier",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 —", "r4 —"],
    reason:
      "It summed the complexity of every function a change touched rather than the complexity the change added, so a repository-wide reformat scored 8.4 standard deviations while adding nothing. Fixed, and it no longer runs on test suites or on changes whose absolute complexity is trivial — a z-score describes a distribution, not a magnitude. The baseline it scores against was also counting every re-index as new samples, which inflated the very count that gates this signal to thirty. Not re-validated since.",
  },
];

export const CHECKS: CheckRow[] = [
  { name: "swallowed-exception", note: "Parser only, no data flow", on: true },
  { name: "contract-change", note: "Signature diffing, body ignored", on: true },
  { name: "over-engineering", note: "Needs a repo-wide symbol index", on: true },
  { name: "tautological-test", note: "Agent-authored hunks only", on: true },
  { name: "structural-clone", note: "Requires a built index", on: false },
  { name: "complexity", note: "Requires ≥30 baseline samples", on: false },
];

