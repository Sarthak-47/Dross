# Dross

**Dross** is the metallurgical term for the impurities skimmed off molten metal before it's cast. This tool does the same to a diff before you commit it.

It catches what agent-generated diffs specifically get wrong — duplicated logic, self-validating tests, silently changed contracts, needless over-engineering, and swallowed exceptions — across whichever AI coding tool you use.

**Every check is a parser, a hash, or a graph algorithm. There are no model calls anywhere in the pipeline.** That is deliberate: it is what makes runs deterministic, reproducible, offline, and free.

> Status: pre-release. The engine, CLI, adapters, desktop app, and benchmark harness are implemented and tested. Published precision figures across open-source repositories are not yet available — see [Benchmarks](#benchmarks) for exactly what has and has not been measured.

---

## Why this exists

AI-assisted coding moved the bottleneck from writing code to verifying it. The failure modes are specific and mechanical, which means they can be caught structurally rather than by asking another model for an opinion.

Existing tooling in this space is enterprise SaaS: cloud-hosted, subscription-priced, built for team governance. Dross is local-first, deterministic, and free.

## The six checks

| Check | What it catches |
|---|---|
| **Authorship tagging** | Scopes the other checks by whether a hunk was agent-written. Architectural, not a bolt-on. |
| **Swallowed exception** | Empty catch bodies, log-only handlers, overly broad catch types, and failure paths that return a default value shaped like success. |
| **Structural clone** | A function that reinvents logic already in the repository, detected on normalized AST shape so renamed identifiers still match. |
| **Tautological test** | A test whose expected value is derived by re-invoking the logic under test, so it passes regardless of correctness. |
| **Contract change** | Signature changes — new required parameters, narrowed optionality, widened return types, sync becoming async — whose breakage is invisible in the diff itself. |
| **Over-engineering** | Single-implementation abstractions, pass-through wrappers, excess indirection, one-variant factories, unused generality, and changes that are statistical complexity outliers against the repository's own history. |

Each check reports per-signal, so you can tune or disable one signal without losing the rest.

### The self-calibrating baseline

The over-engineering check does not use a hardcoded "functions over N branches are bad" rule, which breaks across codebases with different norms. It builds a distribution from the repository's own commit history, so "unusually complex" means unusual *for this codebase*. Below 30 history samples the signal stays silent rather than reporting noise.

## Install

Requires Rust 1.88+ (the workspace uses `let` chains).

```bash
git clone https://github.com/Sarthak-47/Dross.git
```

```bash
cargo build --release
```

The CLI lands at `target/release/dross`.

## Use

Build the index once per repository. This also replays history to construct the complexity baseline.

```bash
dross index
```

Check what you're about to commit:

```bash
dross check --staged
```

Wire it into your tools:

```bash
dross connections
```

```bash
dross connections install git
```

Other commands: `dross check --worktree` for live edits, `dross history` for the risk trend, `dross init` to write a config file, and `--format json` or `--format compact` on any check for machine-readable output.

## Integrations

| Tool | Mechanism | Caveats |
|---|---|---|
| **Claude Code** | Native hooks — `PostToolUse` on edits, `PreToolUse` gate on `git commit` | Primary integration; most mature hook surface |
| **OpenAI Codex CLI** | `hooks.json`, `PostToolUse` on `git commit` | Hooks are opt-in and disabled by default. `PreToolUse` only intercepts the Bash tool, so edit-level interception is unavailable |
| **Google Antigravity** | JSON hooks | Covers Manager-view autonomous flows; Editor-view commits go through the git fallback |
| **Everything else** | git `pre-commit` hook | Covers Cursor, Copilot, and terminal use |

**One honest limitation:** desktop apps that commit through their own UI button rather than a terminal can bypass `.git/hooks` entirely. The fallback works from a terminal but will not fire for that specific flow. Each adapter reports its own caveats in the Connections panel rather than burying them here.

Every adapter merges into existing configuration rather than overwriting it, tags what it added, and removes only that on uninstall.

## Desktop app

The Tauri app is the primary surface: findings with inline source context, a Connections panel, and a risk-history trend.

```bash
npm install --prefix apps/desktop
```

```bash
npm run tauri dev --prefix apps/desktop
```

## Benchmarks

Numbers are the reason to trust a tool that claims to catch things. Here is the current state, stated precisely.

**Measured — ground-truth corpus.** `fixtures/seeded` contains defective and correct code for every check, and the integration suite asserts that every positive is caught and no negative is flagged. All 11 cases pass. The negative cases carry most of the weight: any check reaches perfect recall by flagging everything, so each defect is paired with code that resembles it but is correct.

**Not yet measured — precision across open-source repositories.** The harness exists and runs, but the labeled sample has not been collected.

```bash
cargo run -p dross-bench -- run --repo-dir .bench/repos --agent-only
```

```bash
cargo run -p dross-bench -- label --per-signal 30
```

```bash
cargo run -p dross-bench -- report --labels .bench/worksheet.jsonl
```

Three design choices make the eventual numbers mean what they appear to:

- The sample distinguishes agent-authored commits from human ones. Precision over arbitrary commits would not demonstrate what the tool claims to detect.
- Sampling is stratified per signal, because rare signals are exactly the ones whose precision is least certain.
- Results carry Wilson confidence intervals, and two independent label passes produce a Cohen's kappa. A bare percentage from a small sample invites over-reading.

Recall is reported only against the seeded corpus and is otherwise explicitly marked unmeasured, since it cannot be derived from a label pass over emitted findings.

## Known limitations

Stated plainly, because a tool that hides these gets uninstalled at the first surprise:

- **Symbol resolution is name-based, not semantic.** tree-sitter is a syntax parser with no binder or type checker, so two functions sharing a name collapse into one entry and re-export chains are not followed. Checks that depend on repo-wide resolution consult an ambiguity set and decline to fire on ambiguous names — the limitation costs recall, never precision. A real resolver means shelling out to `tsc`/`pyright`, which would break the offline guarantee.
- **Authorship tagging is heuristic.** Commit trailers are reliable when present; burst-write timing is not. A hunk mistagged as human silently drops to a lighter check pass, so confidence is surfaced in the UI and can be corrected rather than hidden.
- **Language support is JS/TS/TSX and Python.** These were the launch targets.
- **The complexity-outlier signal needs history.** Under 30 baseline samples it reports nothing.

## Architecture

```
crates/
  dross-core       engine: diff, AST, checks, fingerprint index, scoring
  dross-cli        CLI (binary: dross)
  dross-adapters   Claude Code, Codex, Antigravity, git hook
  dross-bench      benchmark harness
apps/desktop       Tauri 2 + React + TypeScript
fixtures/seeded    ground-truth corpus
```

The CLI, the desktop app, and the benchmark harness all call the same `dross-core` engine, so they cannot drift apart.

## Development

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs on Linux, macOS, and Windows, and includes a determinism gate: the same diff analyzed twice must produce byte-identical output. Reproducibility is the core claim, so it is enforced rather than asserted.

## License

Apache-2.0. See [LICENSE](LICENSE).
