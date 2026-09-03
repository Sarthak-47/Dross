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
| log-only-catch \* | 5 | 7 | 41.7% | 19–68% | on |
| near-duplicate-function | 0 | 12 | 0.0% | 0–24% | off |
| silent-optimistic-return | 0 | 12 | 0.0% | 0–24% | off |

\* **log-only-catch has been changed since this was measured.** Two of the
false positives in this row were a handler whose log message states the failure
is expected (socket.io's `debug("ignore malformed buffer")`) and Python's
`warnings.warn`, which is a channel callers can escalate rather than a log
line. Both are now excluded. Replaying the same 227 commits through the new
code drops this signal from 25 findings to 19, and all six were read against
their source and confirmed false — but no true positive was re-checked, so the
precision above is the last figure actually measured and is left standing.
Treat it as a floor until a labelling pass replaces it.

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
  and it could not tell the two apart. **See the section below — this has
  since been worked on, and the volume is down 77%, but it stays disabled
  until it is labelled again.**
- **silent-optimistic-return** — 0% in three rounds. Returning a default on
  failure is the documented contract far more often than it is a hidden
  failure: predicates, `get_or_none` lookups, best-effort serialisation,
  deliberately ignored malformed input.
- **overkill-design-pattern** — 0 of 24. An ordinary factory containing one
  `if` is not a one-variant registry.
- **single-implementation-abstraction** — 0 of 24. What it finds are
  published extension points subclassed by consumers outside the repository.
- **complexity-to-problem-size-outlier** — 0 of 12. Measures added rather than
  touched complexity, and no longer runs on test files or on changes whose
  absolute complexity is trivial. Not re-labelled.

## Reworking the disabled signals

Measured with `dross-bench run --all-signals`, which exists because the harness
ran `Config::default()` — so a signal switched off after measuring badly could
never be measured again, which is exactly when a re-measurement is wanted. Same
22 repositories, 120 commits each, before and after.

| Signal | Findings before | After | Change |
|---|---:|---:|---|
| near-duplicate-function | 485 | **111** | −78% |
| silent-optimistic-return | 25 | **6** | −76% |
| single-implementation-abstraction | 9 | **5** | −45% |
| log-only-catch | 11 | **9** | −19% |
| overkill-design-pattern | 27 | 30 | **+11%** |
| complexity-to-problem-size-outlier | 19 | 22 | not comparable |

**This is finding volume, not precision.** No labelling pass has been run over
the new output, so none of these is re-enabled. A 77% drop in a signal that was
scoring 0% is a reason to go and label it, not a reason to trust it.

What changed, and why it was the right target: every previous attempt filtered
on *shape*, which is what the true and false positives have in common.
Normalization erases identifiers, and that is both what lets a renamed copy
match its original and what makes two parallel validators look identical. The
discriminator is vocabulary — the members a function reaches for and the
functions it calls, the part that survives a rename. The seeded duplicate
renames every local but still reads `.price` and `.quantity`.

Three corrections came out of reading the residue rather than guessing:

- Requiring three shared terms instead of two would have removed 137 of 238
  findings. It also removes the seeded duplicate, which shares exactly `price`
  and `quantity`. The problem was never the count: 114 of those 137 were
  Flask's decorators matching on `Callable` and `callable`, a type annotation
  and a builtin. Language vocabulary is now excluded from the comparison.
- httpx pairs a public `same_origin` with a private `_same_origin`. The
  same-name filter already covered that; the underscore hid it.
- date-fns keeps suites in files named `test.ts`, which the test-path
  patterns missed, so the complexity signal reported a test suite as
  over-engineered.

**What remains, and the limit of the approach.** 96 of the 111 surviving
findings are date-fns: `differenceInMinutes` against `differenceInSeconds`,
`startOfDecade` against `endOfDecade`. Sibling APIs in a single-subject library
share real vocabulary, because they are genuinely about the same things.
Vocabulary cannot separate those, and nothing in this round claims to.

### The one that did not work

`overkill-design-pattern` went **up**, 27 findings to 30, across three attempts.
Each attempt made the per-branch definition more defensible — a conditional that
assigns a field is not dispatch, a nested closure's branches are not the
factory's, a guard clause returning its own argument is not a variant, and an
unguarded `return new X()` is a variant that was not being counted. All four are
right. The volume still rose.

The reason is the trigger: the signal fires on *exactly* one variant, so any
change to how variants are counted moves functions into the bucket as readily as
out of it. The first attempt removed 22 findings and introduced 32.

The premise is what does not survive. "A factory-shaped function with one branch
is premature abstraction" is not separable from ordinary code by shape, because
ordinary constructors have branches too. Recorded here rather than tuned further:
three attempts in one sitting is enough to call it.

### A measurement that is not comparable

`complexity-to-problem-size-outlier` reads 19 → 22, and that comparison should
not be trusted in either direction. The baseline table had no uniqueness on the
commit, so every `dross index` re-inserted the whole replayed history — one
corpus repository held 126 rows for 18 commits. The "before" column was measured
against those inflated baselines and the "after" against clean ones, so the two
numbers describe different distributions.

The duplication mattered beyond this table: `sample_count` is what gates the
signal to thirty samples, so the gate could be passed by indexing a small
repository repeatedly. That is fixed; the measurement will be redone.

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

## Corroboration that does not come from the labeller

The precision figures above were produced by a single labeller — Claude Opus 5,
the same family of system Dross is built to check. That is a conflict of
interest, and no further labelling by the same party removes it.

Two of Dross's signals have close equivalents in linters maintained by large
communities, so for those the judgement can be taken out entirely:

| Dross signal | Independent rule | Agreed | Of | Rate |
|---|---|---:|---:|---:|
| empty-catch-body (JS/TS) | oxlint `no-empty` (ESLint's rule) | 113 | 113 | 100% |
| empty-catch-body (Python) | ruff `S110` try-except-pass | 6 | 6 | 100% |
| overly-broad-catch-type | ruff `BLE001` or `E722` | 30 | 30 | 100% |
| **all** | | **149** | **149** | **100%** |

Same 22 repositories, 300 commits each. Reproduce with:

```bash
python -m venv .venv && .venv/bin/pip install -r .bench/requirements.txt
cargo run -p dross-bench -- run --repo-dir .bench/repos --commits 300 --out findings.jsonl
python .bench/crossvalidate.py findings.jsonl
```

**What this is.** Every finding Dross made that these tools also have a rule for,
they also made. The rules were implemented independently, by people with no
stake in this repository.

**What it is not.** Agreement is not truth — two tools can share a blind spot,
and a rule that fires on the same line is not necessarily asking the same
question. It says nothing about the other twelve signals, which have no
equivalent to compare against, and nothing about recall: it measures what Dross
found, not what it missed.

It also changed the code. Comparing against `BLE001` surfaced nine handlers the
two tools disagreed about, all of them logging the traceback via
`logger.exception(...)` or `exc_info=True`. ruff exempts those, on the grounds
that recording a traceback propagates the failure rather than hiding it. That is
the more widely used judgement and Dross now shares it, which is the point of
comparing against something other than yourself.

The harness was checked against deliberately wrong mappings before its output
was believed — `log-only-catch` against `no-empty` scores 0 of 8 — because a
comparison that always agrees is not a comparison. An earlier version of it
scored 94% and was wrong to: it mapped breadth to `BLE001` alone, and counted
httpx's bare `except:` handlers as disagreements when ruff covers those under
`E722` instead. The fault was in the comparison, not the tool being compared.

### Why not the usual automated oracle

The standard substitute for human labelling is the closed-warning heuristic:
call a warning real if a later revision no longer raises it. It was considered
and rejected. Kang, Aw and Lo ([ICSE 2022](https://arxiv.org/abs/2202.05982))
hand-checked 1,357 such labels and found only 49% agreement with human
annotators, with a further 38% removed incidentally by unrelated edits. It would
have replaced one unreliable oracle with another.

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
