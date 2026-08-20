/* Seed data.
 *
 * The findings, skips, history and connection copy come from the design
 * handoff and stand in until a repository is analysed.
 *
 * The signals table does not. Its precision figures are the ones Dross
 * actually measured across the benchmark corpus, read from
 * docs/benchmark-report-final.json. That table's own copy says the number is
 * "what it actually scored here, not a claim", so placeholder figures would
 * make the product's central claim untrue. */

import type {
  CheckRow,
  ConnectionCard,
  HistoryBar,
  HistoryRow,
  SeedFinding,
  SignalRow,
  SkippedRow,
} from "./types";

export const SEED_FINDINGS: SeedFinding[] = [
  {
    severity: "error",
    message: "sumBasket duplicates existing logic in computeTotal",
    location: "src/checkout/basket.ts:42",
    tags: ["structural-clone", "exact-normalized-match"],
    evidence:
      "Normalized-AST similarity 100%. Identifier and literal differences are ignored, so this is a structural match, not a textual one.",
    authorship: "confirmed",
    related: [
      { key: "clone twin", value: "src/orders/total.ts:88" },
      { key: "call site", value: "src/checkout/index.ts:17" },
    ],
    file: "src/checkout/basket.ts",
    range: "lines 36–52",
    method:
      "Both function bodies are parsed, stripped of identifiers, literals and comments, then hashed. Equal hashes mean the two functions compute the same shape. Dross does not judge whether the duplication is intentional.",
    metrics: [
      { label: "ast nodes", value: "61 / 61" },
      { label: "hash", value: "a41f9c2e" },
      { label: "threshold", value: "0.92" },
    ],
    code: [
      [36, " ", "export function computeTotal(items: LineItem[]): Money {"],
      [37, " ", "  let total = zero(items[0]?.currency ?? 'USD');"],
      [38, " ", "  for (const item of items) {"],
      [39, " ", "    total = add(total, multiply(item.price, item.quantity));"],
      [40, " ", "  }"],
      [41, " ", "  return total;"],
      [42, " ", "}"],
      [43, " ", ""],
      [44, "+", "export function sumBasket(basket: BasketRow[]): Money {", 1],
      [45, "+", "  let running = zero(basket[0]?.currency ?? 'USD');", 1],
      [46, "+", "  for (const row of basket) {", 1],
      [47, "+", "    running = add(running, multiply(row.price, row.quantity));", 1],
      [48, "+", "  }", 1],
      [49, "+", "  return running;", 1],
      [50, "+", "}", 1],
      [51, " ", ""],
      [52, " ", "export function applyDiscount(m: Money, pct: number): Money {"],
    ],
  },
  {
    severity: "warning",
    message:
      "retryWithBackoff has cyclomatic complexity 24, z-score 3.1 against the repository baseline",
    location: "src/net/retry.ts:113",
    tags: ["complexity-outlier", "baseline-z"],
    evidence:
      "Baseline mean 6.2, standard deviation 5.7, computed from 412 functions in this repository. The reporting threshold is z ≥ 2.5.",
    authorship: "heuristic",
    related: [{ key: "call site", value: "src/net/client.ts:204" }],
    file: "src/net/retry.ts",
    range: "lines 108–124",
    method:
      "Cyclomatic complexity is counted from decision points in the control-flow graph. The z-score compares this function against every other function Dross indexed in this repository, not against an external corpus.",
    metrics: [
      { label: "complexity", value: "24" },
      { label: "baseline mean", value: "6.2" },
      { label: "z-score", value: "3.1" },
    ],
    code: [
      [108, " ", "const DEFAULT_ATTEMPTS = 5;"],
      [109, " ", ""],
      [110, " ", "type RetryOptions = {"],
      [111, " ", "  attempts?: number;"],
      [112, " ", "};"],
      [113, "+", "export async function retryWithBackoff<T>(", 1],
      [114, "+", "  operation: () => Promise<T>,", 1],
      [115, "+", "  options: RetryOptions = {},", 1],
      [116, "+", "): Promise<T> {", 1],
      [117, "+", "  const attempts = options.attempts ?? DEFAULT_ATTEMPTS;", 1],
      [118, "+", "  let lastError: unknown;", 1],
      [119, "+", "  for (let i = 0; i < attempts; i += 1) {", 1],
      [120, "+", "    try {", 1],
      [121, "+", "      return await operation();", 1],
      [122, "+", "    } catch (error) {", 1],
      [123, "+", "      lastError = error;", 1],
      [124, " ", "    }"],
    ],
  },
  {
    severity: "warning",
    message:
      "Unused export normalizeAddress has exactly one call site, inside its own test",
    location: "src/geo/address.ts:9",
    tags: ["dead-export", "test-only-reference"],
    evidence:
      "Import graph resolved across 1,204 modules. The only edge into this symbol comes from address.test.ts.",
    authorship: null,
    related: [{ key: "call site", value: "src/geo/address.test.ts:31" }],
    file: "src/geo/address.ts",
    range: "lines 4–20",
    method:
      "Every import in the repository is resolved to a file, then to a symbol. An export with no inbound edge from non-test code is reported. Re-exports through dynamic index barrels are not followed.",
    metrics: [
      { label: "modules", value: "1,204" },
      { label: "inbound edges", value: "1" },
      { label: "non-test edges", value: "0" },
    ],
    code: [
      [4, " ", "import { Address } from './types';"],
      [5, " ", ""],
      [6, " ", "const WHITESPACE = /\\s+/g;"],
      [7, " ", ""],
      [8, " ", ""],
      [9, "+", "export function normalizeAddress(a: Address): Address {", 1],
      [10, "+", "  return {", 1],
      [11, "+", "    ...a,", 1],
      [12, "+", "    street: a.street.trim().replace(WHITESPACE, ' '),", 1],
      [13, "+", "    city: a.city.trim(),", 1],
      [14, "+", "  };", 1],
      [15, "+", "}", 1],
      [16, " ", ""],
      [17, " ", "export function formatAddress(a: Address): string {"],
      [18, " ", "  return [a.street, a.city].filter(Boolean).join(', ');"],
      [19, " ", "}"],
      [20, " ", ""],
    ],
  },
  {
    severity: "info",
    message:
      "Two dependencies added in this diff resolve to the same package under different names",
    location: "package.json:31",
    tags: ["dependency-overlap", "lockfile-hash"],
    evidence:
      "Lockfile resolution: left-pad-x and padleft both resolve to the tarball with integrity sha512-9Kx1c…, so the second adds no code.",
    authorship: "confirmed",
    related: [{ key: "lockfile", value: "package-lock.json:2104" }],
    file: "package.json",
    range: "lines 26–38",
    method:
      "Dross compares the integrity hash each dependency resolves to in the lockfile. Two names resolving to one hash is reported as overlap. Intentional forks pinned under a second name look identical to this.",
    metrics: [
      { label: "packages added", value: "2" },
      { label: "distinct hashes", value: "1" },
      { label: "integrity", value: "sha512-9Kx1c" },
    ],
    code: [
      [26, " ", '  "dependencies": {'],
      [27, " ", '    "fast-json-stable-stringify": "^2.1.0",'],
      [28, " ", '    "get-stream": "^8.0.1",'],
      [29, " ", '    "http-cache-semantics": "^4.1.1",'],
      [30, " ", '    "is-plain-obj": "^4.1.0",'],
      [31, "+", '    "left-pad-x": "^1.0.4",', 1],
      [32, "+", '    "padleft": "^1.0.4",', 1],
      [33, " ", '    "normalize-url": "^8.0.1",'],
      [34, " ", '    "p-cancelable": "^4.0.1",'],
      [35, " ", '    "responselike": "^3.0.0"'],
      [36, " ", "  },"],
      [37, " ", '  "devDependencies": {'],
      [38, " ", '    "typescript": "^5.4.5"'],
    ],
  },
  {
    severity: "info",
    message: "loadConfig swallows a parse failure and returns an empty config",
    location: "src/config/load.ts:62",
    tags: ["empty-catch", "silent-default"],
    evidence:
      "The catch block has no log, no rethrow and no error return. A malformed config is indistinguishable from an absent one at every call site.",
    authorship: null,
    related: [{ key: "call site", value: "src/config/index.ts:12" }],
    file: "src/config/load.ts",
    range: "lines 54–70",
    method:
      "The parser walks each catch clause and reports bodies that neither log, rethrow, return an error, nor call a handler. It reads syntax only; a logger reached through an unresolved wrapper would be missed.",
    metrics: [
      { label: "catch clauses", value: "3" },
      { label: "reported", value: "1" },
      { label: "data flow", value: "not used" },
    ],
    code: [
      [54, " ", "export function parseConfig(raw: string): Config {"],
      [55, " ", "  const parsed = JSON.parse(raw) as Partial<Config>;"],
      [56, " ", "  return { ...defaults, ...parsed };"],
      [57, " ", "}"],
      [58, " ", ""],
      [59, " ", "const EMPTY: Config = { ...defaults };"],
      [60, " ", ""],
      [61, " ", ""],
      [62, "+", "export function loadConfig(path: string): Config {", 1],
      [63, "+", "  try {", 1],
      [64, "+", "    return parseConfig(readFileSync(path, 'utf8'));", 1],
      [65, "+", "  } catch {", 1],
      [66, "+", "    return EMPTY;", 1],
      [67, "+", "  }", 1],
      [68, "+", "}", 1],
      [69, " ", ""],
      [70, " ", "export const config = loadConfig(CONFIG_PATH);"],
    ],
  },
];

export const SEED_SKIPPED: SkippedRow[] = [
  {
    check: "secret-entropy",
    reason:
      "Two binary files in this diff have no text baseline to compare against. Skipped rather than guessed.",
  },
  {
    check: "complexity-outlier / python",
    reason:
      "No Python parser installed. Only .ts, .tsx and .js files were parsed in this run.",
  },
];

/* Real signals, real measured precision. Rounds are the labelled benchmark
 * rounds recorded in docs/BENCHMARK_RESULTS.md. */
export const SIGNALS: SignalRow[] = [
  {
    name: "parameter-type-changed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 83%", "r3 92%", "r4 100%"],
    reason:
      "A signature is a fact about the parse tree, not an estimate. The remaining risk is comparison rather than detection: union members are sorted before comparing, because reordering A | B | None to None | A | B changed nothing a caller could observe and was being reported as a change.",
  },
  {
    name: "required-parameter-added",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 67%", "r3 75%", "r4 100%"],
    reason:
      "Every caller that omitted the argument breaks. Test functions are excluded, because a fixture parameter injected by a framework has no external callers to break — that exclusion was the whole gap between 67% and 100%.",
  },
  {
    name: "return-type-changed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 100%", "r3 92%", "r4 100%"],
    reason:
      "Callers written against the old type may mishandle the new one with nothing failing at the call site.",
  },
  {
    name: "parameter-removed",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 100%", "r3 92%", "r4 100%"],
    reason:
      "Call sites still passing the argument silently pass an ignored value in JavaScript, or fail outright in typed code.",
  },
  {
    name: "optional-parameter-became-required",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 92%", "r3 83%", "r4 100%"],
    reason: "Existing call sites that omitted the argument now break.",
  },
  {
    name: "became-async",
    precision: 100,
    on: true,
    def: "on",
    rounds: ["r2 25%", "r3 67%", "r4 100%"],
    reason:
      "Callers that do not await receive a promise instead of a value, which in JavaScript fails at use rather than at the call site. Early rounds were dominated by test suites migrating to async, which are now excluded.",
  },
  {
    name: "empty-catch-body",
    precision: 92,
    on: true,
    def: "on",
    rounds: ["r2 8%", "r3 45%", "r4 92%"],
    reason:
      "An empty handler carrying a comment is a decision somebody wrote down, and a narrow except ValueError names the one thing the author expected. Both are excluded now; what remains is a broad or untyped handler discarding a failure without saying so.",
  },
  {
    name: "parameter-type-removed",
    precision: 92,
    on: true,
    def: "on",
    rounds: ["r2 50%", "r3 42%", "r4 92%"],
    reason:
      "A parameter losing its annotation stops the contract being enforced. Reporting self was an artifact and is gone.",
  },
  {
    name: "unused-generality",
    precision: 86,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 83%", "r4 86%"],
    reason:
      "Only a literal counts. Earlier rounds reported that a parameter was always exc_info — a variable that happened to share the parameter's name, which says nothing about whether the parameter is exercised.",
  },
  {
    name: "overly-broad-catch-type",
    precision: 75,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 100%", "r4 75%"],
    reason:
      "Breadth alone is a style opinion. Every finding in the first round was a top-level request handler that must catch everything and then delegates. It is reported now only when the broad catch also fails to surface what it caught.",
  },
  {
    name: "pass-through-wrapper",
    precision: 67,
    on: true,
    def: "on",
    rounds: ["r2 0%", "r3 50%", "r4 67%"],
    reason:
      "A wrapper has to forward its own parameters unchanged. Binding an argument is specialisation, and a body that is one call taking a callback keeps its logic in the callback — both were being reported as indirection.",
  },
  {
    name: "log-only-catch",
    precision: 42,
    on: true,
    def: "on",
    rounds: ["r2 25%", "r3 50%", "r4 42%"],
    reason:
      "The weakest signal still shipping on. Emitting an error event, handing the error to a callback and passing it to a reporter all count as surfacing now, but judging whether a debug line is sufficient needs intent the syntax does not carry.",
  },
  {
    name: "near-duplicate-function",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 8%", "r3 8%", "r4 0%"],
    reason:
      "Three rounds and three fix attempts; each cut the volume without moving the false-positive rate. In a mature codebase structurally identical functions are almost always deliberate parallel structure — a decorator family, one formatter per locale, two adapters behind one interface. The seeded corpus shows it can find a renamed duplicate; real repositories are full of intentional twins and it cannot tell the two apart.",
  },
  {
    name: "silent-optimistic-return",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 0%"],
    reason:
      "Returning a default on failure is the documented contract far more often than it is a concealment: predicates, get_or_none lookups, best-effort serialisation, deliberately ignored malformed input. Telling a contract from a concealment needs intent the AST does not carry.",
  },
  {
    name: "overkill-design-pattern",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 —"],
    reason:
      "Zero true positives across 24 labelled findings. An ordinary factory containing one if is not a one-variant registry, and separating the two needs resolution this check does not have.",
  },
  {
    name: "single-implementation-abstraction",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 0%", "r4 —"],
    reason:
      "Zero of 24. What it finds are published extension points subclassed by consumers the repository cannot see, and name-based resolution cannot distinguish those from speculative generality.",
  },
  {
    name: "complexity-to-problem-size-outlier",
    precision: 0,
    on: false,
    def: "off",
    rounds: ["r2 0%", "r3 —", "r4 —"],
    reason:
      "It summed the complexity of every function a change touched rather than the complexity the change added, so a repository-wide reformat scored 8.4 standard deviations while adding nothing. That is fixed but not re-validated, and a signal that fired on a formatting commit has to earn its way back on.",
  },
];

export const CHECKS: CheckRow[] = [
  { name: "swallowed-exception", note: "Parser only, no data flow", on: true },
  { name: "contract-change", note: "Signature diffing, body ignored", on: true },
  { name: "over-engineering", note: "Needs a repo-wide symbol index", on: true },
  { name: "tautological-test", note: "Agent-authored hunks only", on: true },
  { name: "structural-clone", note: "Requires a built index", on: false },
  { name: "complexity", note: "Requires ≥30 baseline samples", on: false },
];

export const SEED_CONNECTIONS: ConnectionCard[] = [
  {
    name: "Claude Code",
    status: "connected",
    path: "~/.claude/settings.json",
    signal: "trailer: Co-Authored-By · session log timestamps",
    limitation:
      "Trailers are written at commit time. If a commit is amended or rebased by hand, the trailer can survive onto lines that were later rewritten by a human.",
  },
  {
    name: "OpenAI Codex CLI",
    status: "detected",
    path: "~/.codex/config.toml",
    signal: "trailer: codex-session-id",
    limitation:
      "Detected but not connected. Without the trailer enabled, authorship for this tool falls back to burst-write timing, which is labelled heuristic and measured at 61% precision.",
  },
  {
    name: "Google Antigravity",
    status: "not found",
    path: "~/.antigravity/config.json",
    signal: "no config present",
    limitation:
      "No config found on this machine, so nothing is being read. If it is installed elsewhere, point Dross at the config path; Dross never scans your home directory unprompted.",
  },
  {
    name: "git pre-commit hook",
    status: "connected",
    path: ".git/hooks/pre-commit",
    signal: "universal · runs dross --staged",
    limitation:
      "Desktop apps that commit through their own UI button rather than a terminal can bypass .git/hooks entirely; this hook will not fire for that flow. It also does not run on --no-verify.",
  },
];

export const HISTORY_BARS: HistoryBar[] = [
  [3, 2, 1, "06"], [1, 4, 2, "07"], [0, 1, 1, "08"], [2, 3, 4, "11"],
  [5, 4, 2, "12"], [1, 2, 3, "13"], [0, 0, 1, "14"], [2, 5, 3, "15"],
  [4, 3, 2, "18"], [1, 1, 2, "19"], [0, 2, 1, "20"], [3, 6, 4, "21"],
  [1, 3, 2, "22"], [2, 3, 2, "25"],
];

export const HISTORY_ROWS: HistoryRow[] = [
  { when: "Aug 25 · 09:14", sha: "a41f9c2", subject: "checkout: extract basket totals", e: 2, w: 3, i: 2, risk: 62 },
  { when: "Aug 22 · 17:02", sha: "7be0d13", subject: "net: retry policy for flaky upstream", e: 1, w: 3, i: 2, risk: 41 },
  { when: "Aug 21 · 11:48", sha: "20ac9f8", subject: "geo: address normalization helpers", e: 3, w: 6, i: 4, risk: 74 },
  { when: "Aug 20 · 08:31", sha: "df10b47", subject: "chore: bump lockfile", e: 0, w: 2, i: 1, risk: 18 },
  { when: "Aug 19 · 15:55", sha: "9c02e6a", subject: "config: tolerate missing file", e: 1, w: 1, i: 2, risk: 29 },
  { when: "Aug 18 · 10:07", sha: "5f7a1cb", subject: "orders: total money type", e: 4, w: 3, i: 2, risk: 68 },
];
