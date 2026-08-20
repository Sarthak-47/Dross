# Benchmark results

22 open-source JavaScript/TypeScript and Python repositories, replaying up to
150 commits each. Method and labeling criteria: [BENCHMARK_RUBRIC.md](BENCHMARK_RUBRIC.md).

Four rounds are reported. Each measured the tool, exposed specific defects,
and the next round measured it again after fixing them. Every round is
published — the first number is the honest starting point, and omitting it
would make the last one unverifiable.

## Headline

| | Round 1 | Round 2 | Round 4 |
|---|---|---|---|
| Overall precision | 32.4% | 50.5% | **73.9%** (66–80%) |
| Precision, signals that ship enabled | — | 56.0% | **87.2%** |
| Findings across the corpus | 9,201 | 5,146 | **2,351** |

Round 5 re-ran the corpus after the union-normalisation fix and with the two
newly disabled signals in effect: 2,351 findings, 74% below the starting point.
The union fix alone removed 56 type-change findings that were reorderings of
the same union.

The two figures differ because five signals measured badly enough to ship
disabled. 73.9% is what the code produces with everything switched on; 87.2%
is what a user sees by default.

## By check, round 4

| Check | TP | FP | Precision | 95% CI |
|---|---:|---:|---:|---|
| contract-change | 77 | 1 | 98.7% | 93–100% |
| over-engineering | 14 | 5 | 73.7% | 51–88% |
| swallowed-exception | 25 | 23 | 52.1% | 38–66% |
| structural-clone | 0 | 12 | 0.0% | 0–24% |

## By signal, round 4

Signals marked *off* ship disabled by default.

| Signal | TP | FP | Precision | 95% CI | Default |
|---|---:|---:|---:|---|---|
| became-async | 6 | 0 | 100.0% | 61–100% | on |
| optional-parameter-became-required | 12 | 0 | 100.0% | 76–100% | on |
| parameter-removed | 12 | 0 | 100.0% | 76–100% | on |
| parameter-type-changed | 12 | 0 | 100.0% | 76–100% | on |
| required-parameter-added | 12 | 0 | 100.0% | 76–100% | on |
| return-type-changed | 12 | 0 | 100.0% | 76–100% | on |
| empty-catch-body | 11 | 1 | 91.7% | 65–99% | on |
| parameter-type-removed | 11 | 1 | 91.7% | 65–99% | on |
| unused-generality | 6 | 1 | 85.7% | 49–97% | on |
| overly-broad-catch-type | 9 | 3 | 75.0% | 47–91% | on |
| pass-through-wrapper | 8 | 4 | 66.7% | 39–86% | on |
| log-only-catch | 5 | 7 | 41.7% | 19–68% | on |
| near-duplicate-function | 0 | 12 | 0.0% | 0–24% | off |
| silent-optimistic-return | 0 | 12 | 0.0% | 0–24% | off |

## Signals that ship disabled, and why

Each was measured, fixed, and measured again. They remain implemented and
switch on per repository. The reason to default them off is that a
pre-commit check is uninstalled as a whole: one noisy signal takes the
accurate ones with it.

- **near-duplicate-function** — 8.3%, 8.3%, then 0% across three rounds and
  three fix attempts. Each fix cut the volume without moving the
  false-positive rate. In a mature codebase, structurally identical
  functions are almost always deliberate parallel structure: Flask's
  `template_*` decorator family, per-locale formatters, two adapters
  implementing one interface. The seeded corpus shows the check can find a
  renamed duplicate; real repositories are mostly full of intentional twins,
  and it cannot tell the two apart.
- **silent-optimistic-return** — 0% in three rounds. Returning a default on
  failure is the documented contract far more often than it is a hidden
  failure: predicates, `get_or_none` lookups, best-effort serialisation,
  deliberately ignored malformed input.
- **overkill-design-pattern** — 0 of 24. An ordinary factory containing one
  `if` is not a one-variant registry.
- **single-implementation-abstraction** — 0 of 24. What it finds are
  published extension points subclassed by consumers outside the repository.
- **complexity-to-problem-size-outlier** — 0 of 12. Now measures added rather
  than touched complexity, but has not been re-validated.

## Recall

Not measurable from this run: a label pass over emitted findings contains no
false negatives by construction. The seeded corpus in `fixtures/seeded` is
the ground-truth half — every positive case caught, no negative case
flagged, verified in CI, with all signals enabled.

## The agent-authored gap

Dross targets agent-generated diffs. Only 81 of the corpus findings came from
commits carrying an agent trailer, 73 of them from a single repository. That
is too small and too concentrated to support a per-signal figure, so none is
claimed. These numbers describe what a user sees running Dross on a normal
repository, which matters, but they are not a measurement of the population
the tool was designed for.

## Dogfooding

Dross is run against its own history. Replaying the twelve commits that built
the desktop UI — several thousand lines of new TypeScript and Rust — produced
no findings outside the seeded corpus.

That result is only meaningful because the path was checked rather than
assumed: a `.tsx` file containing a deliberate empty catch was staged, and the
check reported it. The tool does see the file type the UI is written in. Rust
sources are invisible to it, since the launch grammars are JavaScript,
TypeScript and Python.

## How the labeling was done, and where it is weak

A single labeler — Claude Opus 5 — reading the source at each finding's
commit and applying the rubric. That is the same family of system the tool is
built to check: a real conflict of interest.

Findings not individually reviewed were labeled **against** the tool, so the
figures understate rather than flatter. One earlier rule-only pass scored
58.3%; correcting it against the actual reads brought it to 50.5%, and that
lower number is the one that was published.

Treat these as an internal signal, not a validated benchmark, until a human
labels an independent sample. `dross-bench report` accepts two `--labels`
files and reports Cohen's kappa for exactly that comparison.

## Reproducing

```bash
cargo run -p dross-bench -- report --labels docs/benchmark-labels-final.jsonl
```
