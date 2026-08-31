# Architecture

This document explains how Dross is put together and, more usefully, *why* the
non-obvious decisions were made. It assumes you have read the README.

## The constraint everything follows from

**No model calls anywhere in the pipeline.** Every check is a parser, a hash,
or a graph algorithm.

This is not an aesthetic preference. It produces four properties that together
are the entire reason to install the tool:

- **Deterministic.** The same diff always yields the same findings. CI enforces
  this by analyzing one diff twice and comparing the JSON.
- **Reproducible.** A finding can be re-derived and argued with. "The model
  thought so" cannot.
- **Offline.** No code leaves the machine. For a tool that reads your entire
  working tree, this is a prerequisite for adoption, not a feature.
- **Free.** No inference cost means no subscription, which is what separates
  this from the enterprise SaaS tools in the space.

Every design decision below is downstream of preserving those four.

## Layers

```
                      ┌────────────────────────┐
                      │      dross-core        │
   ┌──────────────┐   │  diff → tag → check    │
   │ dross-cli    │──▶│      → score           │
   ├──────────────┤   │                        │
   │ dross-desktop│──▶│  lang · ast · diff     │
   ├──────────────┤   │  authorship · symbols  │
   │ dross-bench  │──▶│  fingerprint · index   │
   └──────────────┘   │  metrics · checks      │
          │           └────────────────────────┘
          ▼
   ┌──────────────┐
   │dross-adapters│  Claude Code · Codex · Antigravity · git hook
   └──────────────┘
```

All three surfaces call the same engine. This is deliberate: a CLI and a GUI
with separate analysis paths drift, and then the numbers you publish describe
neither.

## The pipeline

```
git diff ──▶ FileDiff/Hunk ──▶ authorship tags ──▶ CheckContext
                                                        │
              ┌─────────────────────────────────────────┤
              ▼                                         ▼
     agent-tagged hunks                         untagged hunks
     all six checks                             lighter four-check pass
              └─────────────────────┬───────────────────┘
                                    ▼
                       findings → severity filter → score
```

### Why two passes

Some failure modes are agent-specific. A tautological test is overwhelmingly a
generated-code artifact; a human writing `expect(f(x)).toBe(f(x))` is rare
enough that running the check over all human code costs more in false positives
than it returns. Clone detection, contract changes, over-engineering, and
swallowed exceptions are useful signal regardless of who wrote the code, so
they run on everything.

## Decisions worth explaining

### Authorship confidence is a value, not a boolean

Detection has two sources with very different reliability. Commit trailers are
reliable when present but depend on the tool writing one. Burst-write timing —
many large writes in quick succession — is heuristic and carries most of the
real-world load.

The costs are asymmetric:

| Mistake | Consequence |
|---|---|
| Human hunk tagged as agent | One extra check runs. Cheap. |
| Agent hunk tagged as human | Silently drops to the lighter pass. The tool under-delivers on exactly the code it exists to catch, and nothing tells the user. |

So the heuristic is tuned conservatively, confidence reaches the UI as a
visible label rather than being collapsed away, and the user can re-tag a
range. A wrong tag the user can see is a wrong tag the user can fix. The
failure this design refuses to accept is the silent one.

### Symbol resolution is name-based, and says so

tree-sitter is a syntax parser. It has no binder and no type checker, so
`SymbolTable` resolves by name: two functions sharing a name collapse into one
entry, and re-export chains are not followed.

The alternative is shelling out to `tsc` or `pyright`, which would break the
offline and zero-dependency guarantees — the properties the whole tool is built
around.

The mitigation is to make the limitation cost recall rather than precision.
Names declared more than once repo-wide land in an ambiguity set, and any check
depending on repo-wide resolution declines to fire on them. This is why the
seeded corpus analyzes each case in isolation: the fixtures deliberately reuse
identifiers across positive and negative variants, which correctly makes them
ambiguous.

Three over-engineering signals depend on this resolution and are therefore
weaker than the other three. That is a real limitation, documented rather than
hidden.

### The complexity baseline is per-repository

A hardcoded "more than N branches is too complex" rule breaks the moment it
meets a codebase with different norms — a parser and a CRUD controller do not
share a complexity budget.

Instead the tool replays the repository's own history and builds a distribution
of complexity-per-changed-line. A change is flagged when it is a statistical
outlier *against how this codebase normally solves similarly-sized problems*.
The repository becomes its own control group.

Below 30 samples the signal reports nothing. A z-score computed from a handful
of points is noise wearing a number's clothing, and a check that fires
confidently on nothing is worse than a check that stays quiet.

### Fingerprinting normalizes before hashing

Clone detection erases identifiers and literals before hashing, keeping only
structure. `computeTotal(items)` and `sumBasket(entries)` produce identical
fingerprints when the logic matches, which is precisely the case textual search
misses and the one agents produce most.

MinHash over shingles gives a similarity estimate; LSH banding over the
signature turns lookup into an indexed query instead of a scan. Hash
permutations are a seeded SplitMix64 finalizer rather than a random family, so
signatures are identical across runs and machines — required by the
determinism guarantee.

Functions below a shingle threshold are skipped. Getters and one-line wrappers
match each other trivially and would flood the check.

### Adapters merge, mark, and remove only their own entries

Each adapter writes a marker into what it installs. Installation merges into
existing configuration; uninstallation removes only marked entries. A tool that
overwrites a user's hook configuration gets uninstalled once and never
reinstalled.

The git hook records the resolved binary path at install time and **fails
loudly** if the binary cannot be found. An earlier version guarded the call
with `command -v dross`, which meant a missing binary let the commit through
silently. That is the worst available outcome for a pre-commit check: the user
believes the change was verified when it was not. Failing loudly is strictly
better than passing quietly.

### Checks report what they did not do

A disabled check, an unbuilt index, and a cold complexity baseline are all
reported explicitly rather than producing an empty result. "No findings"
and "the check never ran" look identical otherwise, and only one of them means
the code is fine.

## Testing

Three layers, each answering a different question:

- **Unit tests** — does this function behave? Every check has negative cases
  asserting it stays silent on correct code, because a check that only has
  positive tests can pass by flagging everything.
- **Seeded corpus** (`fixtures/seeded`) — does the whole pipeline catch known
  defects and leave known-correct code alone? This is where recall comes from;
  labeling emitted findings can only measure precision, since that sample
  contains no false negatives by construction.
- **Benchmark harness** (`dross-bench`) — what is the precision on real
  repositories? Stratified sampling per signal, Wilson confidence intervals,
  and two-labeler Cohen's kappa, because a bare percentage from a small sample
  invites over-reading.

CI additionally enforces determinism directly, since it is the central claim.

Several tests here were checked by deliberately breaking the code they cover
and confirming they fail. A test that cannot fail is worse than no test,
because it reports coverage that does not exist — and three tests in this
repository were found to be exactly that. The comment on such a test names the
mutation it was validated against, so the claim can be re-checked rather than
taken on trust.

## Where the index lives

`.dross/` inside the analyzed repository, holding the fingerprint index,
complexity baseline, and risk history. Keeping it in-repo means no global
state and nothing written outside the tree being analyzed.

The consequence is that it must be excluded from version control, so
installation appends `.dross/` to the repository's `.gitignore`. This was found
the direct way: an early test run committed `index.sqlite` into a fixture
repository.

What gets indexed is filtered by two ignore rules: the configured `ignore_dirs`,
and the repository's own `.gitignore` as git itself applies it.
A fixed directory list cannot stand in for the second — indexing this
repository once walked twenty-one cloned benchmark repositories that git
ignores and the list did not name. Skipping them is also correct rather than
merely fast: an ignored file is never committed, so it cannot appear in a diff,
and indexing it would let clone detection anchor a finding on a file that is
not part of the project.

The schema carries a version. Fingerprints from a different normalization are
not comparable, so a version bump clears the index rather than silently
comparing incompatible signatures.
