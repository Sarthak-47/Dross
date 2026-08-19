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
npm run build --prefix apps/desktop
```

## Adding a check signal

1. Implement it in the relevant module under `crates/dross-core/src/checks/`.
2. Give it a stable `signal` string — the benchmark reports precision per
   signal, so renaming one discards its history.
3. Add positive and negative fixtures under `fixtures/seeded/`.
4. Add unit tests covering both the firing case and at least one near-miss.
5. State the failure mode in the `evidence` text. A finding the user cannot
   verify by reading the code is a finding they will disable.

## Commit messages

Explain why the change was needed, not just what changed. If a bug was found by
testing, say what the failure looked like — that context is what makes the
history useful later.

## License

Apache-2.0. Contributions are accepted under the same license.
