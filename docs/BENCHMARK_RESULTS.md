# Benchmark results

Run against 22 open-source JavaScript/TypeScript and Python repositories,
replaying up to 150 commits each. Method and labeling criteria are in
[BENCHMARK_RUBRIC.md](BENCHMARK_RUBRIC.md).

**These numbers are not good, and they are published as they came out.**
A tool that ships flattering numbers it cannot reproduce gets uninstalled at
the first false positive; the point of measuring was to find out where it
actually stands.

## Corpus

- 22 repositories, 9,201 findings
- 22 repositories produced at least one finding
- 276 findings came from commits with an agent trailer
- Labeled sample: 204 findings, stratified at 12 per signal

## Precision by check

| Check | TP | FP | Precision | 95% CI |
|---|---:|---:|---:|---|
| contract-change | 61 | 23 | 72.6% | 62–81% |
| structural-clone | 1 | 11 | 8.3% | 1–35% |
| swallowed-exception | 4 | 44 | 8.3% | 3–20% |
| over-engineering | 0 | 60 | 0.0% | 0–6% |
| **overall** | **66** | **138** | **32.4%** | 26–39% |

## Precision by signal

| Signal | TP | FP | Precision | 95% CI |
|---|---:|---:|---:|---|
| parameter-removed | 12 | 0 | 100.0% | 76–100% |
| return-type-changed | 12 | 0 | 100.0% | 76–100% |
| optional-parameter-became-required | 11 | 1 | 91.7% | 65–99% |
| parameter-type-changed | 9 | 3 | 75.0% | 47–91% |
| required-parameter-added | 8 | 4 | 66.7% | 39–86% |
| parameter-type-removed | 6 | 6 | 50.0% | 25–75% |
| became-async | 3 | 9 | 25.0% | 9–53% |
| log-only-catch | 3 | 9 | 25.0% | 9–53% |
| empty-catch-body | 1 | 11 | 8.3% | 1–35% |
| near-duplicate-function | 1 | 11 | 8.3% | 1–35% |
| complexity-to-problem-size-outlier | 0 | 12 | 0.0% | 0–24% |
| overkill-design-pattern | 0 | 12 | 0.0% | 0–24% |
| overly-broad-catch-type | 0 | 12 | 0.0% | 0–24% |
| pass-through-wrapper | 0 | 12 | 0.0% | 0–24% |
| silent-optimistic-return | 0 | 12 | 0.0% | 0–24% |
| single-implementation-abstraction | 0 | 12 | 0.0% | 0–24% |
| unused-generality | 0 | 12 | 0.0% | 0–24% |

## Recall

Not measured here. Recall cannot be derived from a label pass over emitted
findings, because that sample contains no false negatives by construction.
The seeded corpus in `fixtures/seeded` is the ground-truth half: every
positive case is caught and no negative case is flagged, verified in CI.

## Who labeled this

A single labeler — Claude Opus 5 — reading the source at each finding's
commit and applying the rubric. Same-family system as the tool is designed
to check, and one rater with no inter-rater agreement score. Treat this as an
internal signal, not a validated benchmark, until a human labels an
independent sample and the passes are compared with Cohen's kappa.

## Reproducing

The labeled sample and the machine-readable report are committed alongside this
document as `benchmark-labels.jsonl` and `benchmark-report.json`, so every
verdict can be checked against the code it refers to.

```bash
cargo run -p dross-bench -- report --labels docs/benchmark-labels.jsonl
```
