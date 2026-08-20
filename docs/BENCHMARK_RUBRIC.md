# Labeling rubric

A precision figure is only as good as the rule used to produce it. This is the
rule, written down before labeling so that decisions are consistent and a
reviewer can disagree with a specific label rather than the whole number.

## The question a label answers

For each finding: **would a competent reviewer, seeing this in a diff, want it
raised?**

Not "is the code perfect" and not "did the check fire as coded". A finding that
correctly implements its rule but points at code no reviewer would change is a
false positive — the tool exists to save review time, so noise counts against
it even when it is technically accurate.

- `tp` — the finding identifies a real problem, or a real risk worth a
  reviewer's attention.
- `fp` — the code is fine as written, or the finding misreads what the code
  does.

Ambiguous cases are labeled `fp`. A tool that gets the benefit of the doubt
reports a precision it will not reproduce for a user.

## Per-signal criteria

### swallowed-exception / empty-catch-body
- `tp`: the exception is discarded with no log, rethrow, or fallback.
- `fp`: the catch is empty but the surrounding code makes the failure visible
  another way — a checked sentinel assigned before the try, a documented
  best-effort cleanup path, or a loop that continues deliberately with the
  failure recorded elsewhere.

### swallowed-exception / log-only-catch
- `tp`: the caller cannot distinguish success from failure.
- `fp`: the function's contract is explicitly best-effort (telemetry,
  cache warming, cleanup, an optional plugin load), or the log is the intended
  product of the function.

### swallowed-exception / overly-broad-catch-type
- `tp`: the try block raises a narrow, knowable set and the broad catch would
  also swallow unrelated failures.
- `fp`: the boundary genuinely needs to catch everything — a top-level request
  handler, a plugin host, a test runner, a subprocess supervisor.

### swallowed-exception / silent-optimistic-return
- `tp`: the caller receives a value shaped like success and cannot tell the
  operation failed.
- `fp`: returning the default *is* the documented contract, e.g. a `get_or_none`,
  a parser with an explicit fallback argument, or a feature-detection probe.

### structural-clone / near-duplicate-function
- `tp`: the two functions do the same work and one could call the other.
- `fp`: structural similarity without semantic duplication — different domain
  objects, generated code, separate overload arms, framework-mandated shapes,
  or a deliberate copy across a module boundary that must not be coupled.

### tautological-test / *
- `tp`: the assertion cannot fail for a wrong implementation.
- `fp`: the expected side re-invokes something already independently verified,
  or the test's purpose is a round-trip or invariant property.

### contract-change / *
- `tp`: an existing caller would break, or silently change behaviour.
- `fp`: the symbol is not public API and every call site is updated in the same
  change, or the "change" is a rename the check paired up incorrectly.

### over-engineering / pass-through-wrapper
- `tp`: the wrapper adds nothing and has one caller.
- `fp`: the wrapper exists for a reason the AST cannot see — a public API
  boundary, a re-export, an interface implementation, a deprecation shim, or a
  seam that exists to be mocked.

### over-engineering / single-implementation-abstraction
- `tp`: the abstraction has one implementor and no evident reason to exist.
- `fp`: it is a published extension point, an implementation of an external
  interface, or has implementors the name-based resolver cannot see.

### over-engineering / unused-generality
- `tp`: the parameter is genuinely never varied and could be removed.
- `fp`: the symbol is public API, or callers outside the analyzed sample vary
  it.

### over-engineering / overkill-design-pattern
- `tp`: the dispatch has one branch and no registered alternatives.
- `fp`: variants are registered elsewhere (a plugin registry, a config file, a
  dynamic import) that the syntactic check cannot follow.

### over-engineering / complexity-to-problem-size-outlier
- `tp`: the change is genuinely more convoluted than the problem warrants.
- `fp`: the complexity is inherent — a parser, a state machine, a compatibility
  shim, or a vectorized numeric routine.

## Who labeled this run

Stated plainly because it bears on how much the number is worth:

The pass recorded in this repository was performed by **Claude Opus 5**, reading
the source at each finding's commit and applying the rubric above. It is a
single labeler, and it is the same family of system the tool is designed to
check the output of. That is a real conflict of interest and a real limitation.

Treat these figures as **an internal signal, not a published benchmark**. The
README should not quote them as validated precision until a human has labeled
an independent sample and the two passes have been compared with
`dross-bench report --labels <human> --labels <ai>`, which reports Cohen's
kappa for exactly this purpose.
