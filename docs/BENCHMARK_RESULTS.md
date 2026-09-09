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

### Two tiers, not one

An independent reviewer, working from the literature on code cloning, in-band
error returns (Go's own code-review guidance), speculative generality
(Microsoft's framework-design rules), and essential-versus-accidental
complexity (SEI), assessed the four. Their conclusion split the group in two,
and it is reflected in the severities:

- **Concrete hazards** — `near-duplicate-function` and `silent-optimistic-return`.
  These are correctness and maintainability problems with a defensible
  yes/no answer, and they fire at **warning**.
- **Design-judgement prompts** — `single-implementation-abstraction` and
  `complexity-to-problem-size-outlier`. These ask a question a human answers
  rather than deciding, and they fire at **info**. `complexity` was demoted
  from warning to info for exactly this reason: "problem size" is not directly
  measurable, so an outlier is a prompt to look, never a verdict.

The distinction matters, but note what that review did and did not do. It
judged the **premises** of the four signals — sound, and well grounded — from
their descriptions. It did **not** look at Dross's actual output on the corpus,
so it cannot substitute for the per-finding pass that decides whether *this
implementation* fires accurately enough to enable. The two are independent
questions: `silent-optimistic-return` has a premise the reviewer rated "very
high", and a first read of its twelve actual findings on this corpus was
eleven false positives — a sound idea whose implementation still needs the
contract awareness it is missing. The premise being right is a reason to keep
working on the signal, not a reason to trust its current output.

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

### The one that did not work, and was deleted

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

**Removed in 1.0**, rather than left disabled. A signal that cannot be made to
work is dead code, and a config key for it is an invitation to turn it on.

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

Same 22 repositories, 300 commits each, and it reproduces from a clean run:
the figures above were re-measured for 1.0 against a corpus whose clones had
since been deepened, and came back identical. Reproduce with:

```bash
python -m venv .venv && .venv/bin/pip install -r .bench/requirements.txt
cd .bench && npm install && cd ..
cargo run -p dross-bench -- run --repo-dir .bench/repos --commits 300 --all-signals --out findings.jsonl
python .bench/crossvalidate.py findings.jsonl
python .bench/clonevalidate.py findings.jsonl
python .bench/clonevalidate.py findings.jsonl --shuffle   # the negative control
python .bench/complexityvalidate.py --all
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

### A third signal, against an independent clone detector

`near-duplicate-function` is checked the same way against **jscpd**, a
token-based copy/paste detector. The result is shaped differently from the ruff
and oxlint comparison and must be read differently.

jscpd compares token streams. Dross compares normalized ASTs, so it ignores
identifier and literal differences on purpose — a renamed copy is the case it
exists to catch, and the exact case jscpd is built not to see. The two tools
therefore overlap on one class of finding and diverge on another, and the split
is not symmetric:

- **jscpd agrees** — the fragments are duplicated at the token level. An
  independent detector calls it a clone, so it is a true positive under any
  definition of the word.
- **jscpd is silent** — this says nothing either way. The pair may be a renamed
  clone (Dross right, jscpd blind by construction) or parallel structure over
  different vocabulary (Dross wrong). The comparison cannot separate those.

| | Findings |
|---|---:|
| near-duplicate-function, whole corpus | 247 |
| counterpart absent from the tree at that commit | 166 |
| **compared against jscpd** | **81** |
| corroborated — duplicated at the token level too | **35** |
| jscpd has nothing to say | 46 |

**35 of 81 is a lower bound on true positives, not a precision figure**, and it
is written that way everywhere it appears. Turning it into a rate would mean
counting jscpd's blind spot as a Dross error.

The 166 dropped findings are not a defect in either tool. The fingerprint index
is built from the checked-out tree while the harness replays history, so a
finding at an old commit can name a counterpart at its present-day path —
date-fns moved `src/` to `pkgs/core/src/`, and every finding across that move
names a file the old commit does not have. That is how the tool is used in
practice (index the repository you have, check the diff you are about to
commit); it is only the historical replay that cannot follow it.

The harness's own negative control repoints each finding at a random file of the
same language elsewhere in the same tree: **0 of 247 corroborated**, against 35
of 81 for the real pairings. Two weaker controls were tried first
and are recorded because they are the ones a reader would reach for: repointing
at a different *finding's* counterpart scored 30%, and keeping the file while
moving the line scored 33% — not because the harness is loose, but because the
corpus contains genuine clone families. date-fns has a dozen mutually
near-identical `differenceInCalendar*` functions in files that are themselves
almost entirely duplicated, so a random swap inside one still lands on a real
duplicate. A control the subject matter can pass by accident measures the
corpus, not the harness.

It also caught the harness being wrong. The first run reported 0 of 12
corroborated, which reads exactly like a result. jscpd had been handed a Windows
path, fast-glob read the backslashes as escape characters, and jscpd exited 0
having scanned no files at all.

`near-duplicate-function` stays **off by default**. A floor on true positives is
not precision, and nothing here measures the 46.

### The complexity metric, against a reference implementation

The other two comparisons check *findings*. This one checks a *number*, and it
is the only one of the three with a right answer.

`Metrics::cyclomatic` calls itself "McCabe cyclomatic complexity". That is a
named metric from a 1976 paper with a settled definition, not a heuristic — so
an independent implementation either agrees or one of the two is wrong. ruff
ships `C901`, which is the reference `mccabe` algorithm, and will report the
figure for every function when `max-complexity` is set to `0`.

It did not agree. **Two real faults**, both found this way:

- **`else` was counted as a decision of its own.** An `if`/`else` is one
  decision; nothing in McCabe counts the else arm separately. `for`/`else` and
  `while`/`else` were double-counted the same way. This is the worst shape of
  error available to this particular signal, because it scores a change against
  a distribution: the inflation was proportional to how many else branches a
  function happened to have, which moves functions around in that distribution
  for a reason unrelated to complexity.
- **Python's `match` was counted as nothing at all.** The branch-point list had
  the JavaScript node kinds (`switch_case`, `case_statement`) and not Python's
  `case_clause`, so a five-arm match statement scored exactly the same as a
  straight line. Fixing that surfaced a second, smaller version of the `else`
  fault in the same place: `case _` and `case name` always match, so like an
  `else` they are the default path rather than a decision, and counting them
  made every one of black's pattern-matching fixtures read one too high.

Two further divergences were differences of definition and were resolved toward
the standard, because the point of naming a published metric is that someone
else's implementation can check it: boolean operators (`and`, `&&`) and ternary
expressions are counted by the *extended* variant of the metric, not by
McCabe's, and are no longer counted here.

| | Agreement with ruff `C901` |
|---|---:|
| Before | 74% |
| After | **98%** |

34,431 functions, compared one at a time across the ten Python repositories in
the corpus. `.bench/complexityvalidate.py` reproduces it.

**The residual 2% is one deliberate difference**, and it is one-directional —
after these fixes there is no function anywhere in the corpus that Dross scores
*higher* than mccabe does. mccabe folds a nested
function's decisions into its parent *and* reports the nested function
separately, so the same branches appear in both figures it prints. Dross indexes
every function once and scores it once: a parent gets one point for containing a
definition and nothing more. Both behaviours are pinned by tests carrying the
reference figures.

This does not make `complexity-to-problem-size-outlier` trustworthy — it is
still unlabelled and still ships disabled. It makes the number the signal is
built on correct, which is a prerequisite rather than a substitute. The index
schema version is bumped so that no baseline built from the old figures is
scored against the new ones.

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

## A note on comparing volumes across rounds

The corpus clones are shallow and have been deepened since round 4, so every
repository now replays more history than it did then: react-router went from 7
commits producing findings to 79, socket.io from 14 to 60. Finding *counts* are
therefore not comparable between rounds, and the before/after volume table above
should be read only within the round that produced it. The precision figures are
per-finding and unaffected; the corroboration figures were re-run on the current
corpus rather than carried forward.

## Reproducing

```bash
cargo run -p dross-bench -- report --labels docs/benchmark-labels-final.jsonl
```
