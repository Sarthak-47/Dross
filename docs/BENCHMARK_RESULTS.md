# Benchmark results

Run against 22 open-source JavaScript/TypeScript and Python repositories,
replaying up to 150 commits each. Method and labeling criteria are in
[BENCHMARK_RUBRIC.md](BENCHMARK_RUBRIC.md).

Two rounds are reported. Round 1 measured the tool as it stood. Round 2
measured it after fixing the defects round 1 exposed. Both are published,
because the first number is the honest starting point and hiding it would
make the second one unverifiable.

## Headline

| | Round 1 | Round 2 |
|---|---|---|
| Overall precision | 32.4% (26–39%) | **50.5%** (44–57%) |
| Findings across the corpus | 9,201 | 5,146 |
| Labeled sample | 204 | 204 |

## By check

| Check | Round 1 | Round 2 |
|---|---|---|
| contract-change | 72.6% | 70.2% |
| over-engineering | 0.0% | 41.7% |
| structural-clone | 8.3% | 8.3% |
| swallowed-exception | 8.3% | 37.5% |

## By signal, round 2

| Signal | TP | FP | Precision | 95% CI |
|---|---:|---:|---:|---|
| overly-broad-catch-type | 12 | 0 | 100.0% | 76–100% |
| parameter-removed | 11 | 1 | 91.7% | 65–99% |
| return-type-changed | 11 | 1 | 91.7% | 65–99% |
| optional-parameter-became-required | 10 | 2 | 83.3% | 55–95% |
| parameter-type-changed | 10 | 2 | 83.3% | 55–95% |
| unused-generality | 10 | 2 | 83.3% | 55–95% |
| complexity-to-problem-size-outlier | 9 | 3 | 75.0% | 47–91% |
| required-parameter-added | 9 | 3 | 75.0% | 47–91% |
| pass-through-wrapper | 6 | 6 | 50.0% | 25–75% |
| empty-catch-body | 5 | 7 | 41.7% | 19–68% |
| parameter-type-removed | 5 | 7 | 41.7% | 19–68% |
| became-async | 3 | 9 | 25.0% | 9–53% |
| log-only-catch | 1 | 11 | 8.3% | 1–35% |
| near-duplicate-function | 1 | 11 | 8.3% | 1–35% |
| overkill-design-pattern | 0 | 12 | 0.0% | 0–24% |
| silent-optimistic-return | 0 | 12 | 0.0% | 0–24% |
| single-implementation-abstraction | 0 | 12 | 0.0% | 0–24% |

## What changed between the rounds

Round 1's false positives were not diffuse noise. Reading the code behind
each one found specific, fixable defects:

- Python `@overload` sets and shadowed helpers collapsed into a single map
  entry, so every variant was compared against an arbitrary sibling. This
  alone accounted for roughly 4,200 findings.
- Build output was analyzed as if hand-written. `dist/lodash.min.js`
  produced 1,575 findings by itself.
- `unused-generality` reported variable names as though they were constants
  — "parameter `exc_info` is always `exc_info`".
- Factory-name matching hit substrings inside unrelated words, so
  `test_idmaker_...` matched "make".
- A body consisting of one call was treated as a pass-through even when it
  bound an argument or hid its logic in a callback.
- Errors surfaced through an emitter, a callback, or a reporter were counted
  as swallowed.
- The complexity outlier summed the complexity of every function a change
  touched, so a reformat scored 8.4 standard deviations while adding nothing.

Three signals still measured 0% after a fix attempt and now ship disabled:
`overkill-design-pattern`, `single-implementation-abstraction`, and
`complexity-to-problem-size-outlier`. They remain implemented and can be
switched on per repository. The reason to default them off is that a
pre-commit check is uninstalled as a whole — one noisy signal takes the
accurate ones with it.

## Recall

Not measured here, and it cannot be: a label pass over emitted findings
contains no false negatives by construction. The seeded corpus in
`fixtures/seeded` is the ground-truth half — every positive case is caught
and no negative case is flagged, verified in CI.

## How the labeling was done, and where it is weak

A single labeler — Claude Opus 5 — reading the source at each finding's
commit and applying the rubric. That is the same family of system the tool
is designed to check, which is a real conflict of interest.

Round 2 is a mix: the swallowed-exception and clone signals were read
individually; the contract signals were labeled by a rule validated against
round 1's individual reads (a test function has no external callers to
break). Findings not individually reviewed were labeled **against** the
tool, so the figure is understated rather than inflated. An earlier
rule-only pass scored 58.3%; correcting it with the actual reads brought it
down to 50.5%, which is the number reported.

Treat these as an internal signal, not a validated benchmark, until a human
labels an independent sample. `dross-bench report` takes two `--labels`
files and reports Cohen's kappa for exactly that comparison.

## Reproducing

Both labeled samples and machine-readable reports are committed, so every
verdict can be checked against the code it refers to.

```bash
cargo run -p dross-bench -- report --labels docs/benchmark-labels-round2.jsonl
```
