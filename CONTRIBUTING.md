# Contributing

## Ground rules for this codebase

**No model calls in the analysis pipeline.** Every check must be a parser, a
hash, or a graph algorithm. This is the property the tool is built on; a change
that adds inference to `dross-core` will not be accepted regardless of how much
accuracy it buys.

**Every check needs negative tests.** A check with only positive tests can pass
by flagging everything. Any new signal must come with fixtures in
`fixtures/seeded/<check>/negative/` showing code that resembles the defect but
is correct.

**Report what you did not do.** A check that cannot run — disabled, missing
index, cold baseline — must say so. "No findings" and "never ran" must never
look the same.

**Prefer recall loss to precision loss.** Where repo-wide name resolution is
ambiguous, decline to fire. A false positive costs user trust permanently; a
missed finding costs one finding.

## Setup

```bash
cargo build
```

```bash
npm install --prefix apps/desktop
```

## Before opening a PR

```bash
cargo fmt --all
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

For frontend changes:

```bash
npm test --prefix apps/desktop
```

```bash
npm run build --prefix apps/desktop
```

Run the build, not `tsc --noEmit -p tsconfig.json`. The root config is
`files: []` plus project references, so on its own it typechecks nothing; only
`npm run build` runs `tsc -b` across them. A change that passed the shortcut
and broke the build has already happened once.

To see the UI against real findings rather than an empty state:

```bash
npm run dev --prefix apps/desktop
```

then open `http://localhost:5173/uiharness.html`. It stubs the Tauri IPC bridge
so the real components render real engine output in a browser. Every visual
check before it existed was of an empty state, and the first run of it found
six bugs.

## Adding a check signal

1. Implement it in the relevant module under `crates/dross-core/src/checks/`.
2. Give it a stable `signal` string — the benchmark reports precision per
   signal, so renaming one discards its history.
3. Add positive and negative fixtures under `fixtures/seeded/`.
4. Add unit tests covering both the firing case and at least one near-miss.
5. State the failure mode in the `evidence` text. A finding the user cannot
   verify by reading the code is a finding they will disable.

## Changing an existing signal

Cutting findings is easy; cutting false ones is the work. A change that reduces
volume has not necessarily improved anything — three separate fixes to
`near-duplicate-function` cut its volume without moving its precision at all,
because all three filtered on the same property its true positives share.

So, in order:

1. Measure before and after on the same repositories.
   `cargo run -p dross-bench -- run --repo-dir .bench/repos --all-signals`
   reaches the signals that ship disabled, which `Config::default()` does not.
2. Read some of what disappeared against its real source. `.bench/show.py`
   prints the code at the finding's own commit. A count is not evidence.
3. Check the seeded corpus still passes. It holds the known true positives, and
   a filter that removes them is not an improvement — one candidate fix here
   would have cut 137 false positives and the only confirmed true positive with
   them.
4. Say which of precision and volume you measured. They are not the same claim.

## Tests that can fail

A test that cannot fail reports coverage that does not exist. Three tests in
this repository were found to be exactly that: they re-implemented the logic
they claimed to check, so the code under test could have been deleted entirely
without failing them.

Before trusting a new test, break the thing it covers and watch it fail. Where
that has been done, the test's comment names the mutation, so the claim can be
re-checked rather than taken on trust.

## Commit messages

Explain why the change was needed, not just what changed. If a bug was found by
testing, say what the failure looked like — that context is what makes the
history useful later.

## License

Apache-2.0. Contributions are accepted under the same license.
