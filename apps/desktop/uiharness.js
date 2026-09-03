/* Development harness. Never part of a build: vite's only entry is index.html,
 * and nothing in src/ imports this.
 *
 * It stubs the Tauri IPC bridge so the real UI renders a real analysis in a
 * browser, where it can be inspected and captured. The report in
 * uiharness-report.json is genuine engine output — socket.io at 0ae76360f, 27
 * findings across 8 signals — so only the transport is stubbed; nothing about
 * the content is invented.
 *
 * This exists because every visual check before it was of an empty state. The
 * first run of it found six bugs, including a source pane that had never
 * rendered a line of source and a precision bar that had never drawn anything.
 *
 *     npm run dev --prefix apps/desktop
 *     open http://localhost:5173/uiharness.html
 *
 * ?tab=Settings selects a view, ?select=N a finding, ?expand=N a signal row,
 * so a headless browser can capture any of them:
 *
 *     chrome --headless --window-size=1440,900 --virtual-time-budget=6000  *       --screenshot=docs/images/settings.png  *       "http://localhost:5173/uiharness.html?tab=Settings&expand=11"
 */

const [REPORT, SOURCE] = await Promise.all([
  fetch("/uiharness-report.json").then((r) => r.json()),
  fetch("/uiharness-source.txt").then((r) => r.text()),
]);

const repo = {
  root: "~/src/socket.io",
  name: "socket.io",
  branch: "main",
  indexBuilt: true,
  indexedFunctions: 5360,
  baselineSamples: 279,
  watcherActive: true,
};

const config = {
  disabled_checks: [],
  disabled_signals: [
    "near-duplicate-function",
    "silent-optimistic-return",
    "overkill-design-pattern",
    "single-implementation-abstraction",
    "complexity-to-problem-size-outlier",
  ],
  min_severity: "info",
  clone_threshold: 0.85,
  complexity_z_threshold: 2.5,
  ignore_dirs: ["node_modules"],
  baseline_commits: 200,
  block_at: null,
};

const adapters = [
  {
    id: "claude-code",
    label: "Claude Code",
    detected: true,
    installed: true,
    config_path: "~/.claude/settings.json",
    limitations: [
      "Trailers are written at commit time. If a commit is amended or rebased by hand, the trailer can survive onto lines that were later rewritten by a human.",
    ],
  },
  {
    id: "codex-cli",
    label: "OpenAI Codex CLI",
    detected: true,
    installed: false,
    config_path: "~/.codex/hooks.json",
    limitations: [
      "PreToolUse only intercepts the Bash tool, so Dross hooks PostToolUse on git commit rather than on individual file edits.",
    ],
  },
  {
    id: "antigravity",
    label: "Google Antigravity",
    detected: false,
    installed: false,
    config_path: null,
    limitations: [
      "Editor-view commits go through the normal VS Code git path, so the git pre-commit fallback covers them; these hooks exist for Manager-view autonomous agent flows.",
    ],
  },
  {
    id: "git-hook",
    label: "git pre-commit (Cursor, Copilot, terminal)",
    detected: true,
    installed: true,
    config_path: "~/src/socket.io/.git/hooks/pre-commit",
    limitations: [
      "Desktop apps that commit through their own UI button rather than a terminal can bypass .git/hooks entirely; this hook will not fire for that flow.",
    ],
  },
];

const history = [];
const shape = [
  [3, 5, 1],
  [1, 8, 2],
  [5, 2, 0],
  [0, 4, 3],
  [2, 6, 1],
  [4, 3, 2],
  [1, 9, 0],
  [6, 1, 1],
  [2, 5, 3],
  [3, 7, 2],
  [0, 2, 1],
  [7, 15, 5],
];
shape.forEach(([e, w, i], d) => {
  const at = "2026-08-" + String(d + 1).padStart(2, "0") + "T09:15:00Z";
  const row = (severity, count) => ({
    recorded_at: at,
    commit_sha: null,
    check_id: "swallowed-exception",
    signal: "empty-catch-body",
    severity,
    count,
  });
  history.push(row("error", e), row("warning", w), row("info", i));
});

window.__TAURI_INTERNALS__ = {
  transformCallback: (cb) => cb,
  invoke: (cmd) => {
    switch (cmd) {
      case "current_repository":
      case "open_repository":
      case "build_index":
        return Promise.resolve(repo);
      case "analyze":
        return Promise.resolve(REPORT);
      case "get_config":
      case "set_config":
        return Promise.resolve(config);
      case "list_connections":
      case "install_connection":
      case "uninstall_connection":
        return Promise.resolve(adapters);
      case "risk_history":
        return Promise.resolve(history);
      case "file_source":
        return Promise.resolve(SOURCE);
      case "open_in_editor":
      case "override_authorship":
        return Promise.resolve(null);
      default:
        return Promise.reject(new Error("unmocked command: " + cmd));
    }
  },
};

await import("./src/main.tsx");

/* Drives the app for a headless capture: ?tab= selects a view, ?select= picks
   a finding. Without this a headless screenshot catches the empty state, since
   nothing has clicked Analyze. */
const params = new URLSearchParams(location.search);
const settle = (ms) => new Promise((r) => setTimeout(r, ms));
const byText = (sel, text) =>
  [...document.querySelectorAll(sel)].find((el) => el.innerText.trim().startsWith(text));

await settle(400);
byText("button", "Analyze")?.click();
await settle(500);

const index = Number(params.get("select") ?? "0");
document.querySelectorAll(".finding")[index]?.click();
await settle(400);

const tab = params.get("tab");
if (tab) {
  byText("button.tab", tab)?.click();
  await settle(400);
}
if (params.get("expand")) {
  document.querySelectorAll('.signals__row[role="button"]')[Number(params.get("expand"))]?.click();
  await settle(300);
}
document.body.dataset.ready = "1";

