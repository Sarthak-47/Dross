import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api } from "./api";
import { Connections } from "./components/Connections";
import { EmptyState, type Fact } from "./components/EmptyState";
import { Findings } from "./components/Findings";
import { Header } from "./components/Header";
import { RiskHistory } from "./components/RiskHistory";
import { Settings } from "./components/Settings";
import { SourcePane } from "./components/SourcePane";
import {
  CHECKS,
  HISTORY_BARS,
  HISTORY_ROWS,
  SEED_CONNECTIONS,
  SEED_FINDINGS,
  SEED_SKIPPED,
  SIGNALS,
} from "./fixtures";
import type {
  AdapterStatus,
  CheckRow,
  ConnectionCard,
  IndexProgress,
  Report,
  RepositoryInfo,
  SeedFinding,
  Severity,
  SignalRow,
  SkippedRow,
  Tab,
  Target,
  ViewState,
} from "./types";
import "./theme.css";
import "./App.css";

/** Findings the design ships with, used until a repository is analysed. */
const DEMO = {
  findings: SEED_FINDINGS,
  skipped: SEED_SKIPPED,
  risk: 62,
};

export default function App() {
  const [repo, setRepo] = useState<RepositoryInfo | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [adapters, setAdapters] = useState<AdapterStatus[] | null>(null);
  const [tab, setTab] = useState<Tab>("findings");
  const [target, setTarget] = useState<Target>("working");
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Persisted settings. Every control applies immediately; there is no Save.
  const [signals, setSignals] = useState<SignalRow[]>(SIGNALS);
  const [checks, setChecks] = useState<CheckRow[]>(CHECKS);
  const [expanded, setExpanded] = useState<string | null>("near-duplicate-function");
  const [cloneThreshold, setCloneThreshold] = useState(0.92);
  const [zThreshold, setZThreshold] = useState(2.5);
  const [minSeverity, setMinSeverity] = useState<Severity>("info");
  const [commitGate, setCommitGate] = useState<"advisory" | "block">("advisory");

  useEffect(() => {
    // Outside a Tauri window the event bridge is absent; degrade to no
    // progress events rather than throwing.
    const unlisten = listen<IndexProgress>("dross://index-progress", (event) =>
      setProgress(event.payload),
    ).catch(() => undefined);
    return () => {
      void unlisten.then((fn) => fn?.());
    };
  }, []);

  useEffect(() => {
    api.currentRepository().then(setRepo).catch(() => undefined);
    api.listConnections().then(setAdapters).catch(() => undefined);
  }, []);

  const guard = useCallback(
    async <T,>(label: string, action: () => Promise<T>): Promise<T | null> => {
      setBusy(label);
      setError(null);
      try {
        return await action();
      } catch (e) {
        setError(String(e));
        return null;
      } finally {
        setBusy(null);
        setProgress(null);
      }
    },
    [],
  );

  const analyze = useCallback(async () => {
    const result = await guard("Analyzing", () =>
      api.analyze(target === "staged" ? "staged" : "worktree"),
    );
    if (result) {
      setReport(result);
      setSelected(0);
    }
  }, [guard, target]);

  const rebuild = useCallback(async () => {
    const info = await guard("Building index", () => api.buildIndex());
    if (info) setRepo(info);
  }, [guard]);

  const openRepo = useCallback(async () => {
    const picked = await api.pickDirectory();
    if (!picked) return;
    const info = await guard("Opening repository", () => api.openRepository(picked));
    if (!info) return;
    setRepo(info);
    setReport(null);
    setAdapters(await api.listConnections().catch(() => null));
  }, [guard]);

  const openInEditor = useCallback((location: string) => {
    void api.openInEditor(location).catch(() => undefined);
  }, []);

  /** Real findings once analysed; the design's seed data before that. */
  const shown = useMemo<{
    findings: SeedFinding[];
    skipped: SkippedRow[];
    risk: number;
  }>(() => {
    if (!report) return DEMO;
    return {
      risk: report.risk_score,
      skipped: report.skipped,
      findings: report.findings.map((f) => ({
        severity: f.severity,
        message: f.message,
        location: `${f.span.file}:${f.span.start_line}`,
        tags: [f.check, f.signal],
        evidence: f.evidence,
        authorship:
          f.authorship === "confirmed"
            ? "confirmed"
            : f.authorship === "heuristic"
              ? "heuristic"
              : null,
        related: (f.related ?? []).map((r) => ({
          key: "related",
          value: `${r.file}:${r.start_line}`,
        })),
        file: f.span.file,
        range: `lines ${f.span.start_line}–${f.span.end_line}`,
        method:
          "Every check is a parser, a hash, or a graph query. This finding comes from the same deterministic pipeline the CLI runs; re-running it on an unchanged diff produces the identical result.",
        metrics: [
          { label: "check", value: f.check },
          { label: "signal", value: f.signal },
          { label: "severity", value: f.severity },
        ],
        code: [],
      })),
    };
  }, [report]);

  /** Derived, never chosen by the user. */
  const view: ViewState = useMemo(() => {
    if (!repo) return "norepo";
    if (busy === "Building index") return "building";
    if (!repo.indexBuilt) return "noindex";
    if (report && report.findings.length === 0) return "clean";
    if (report && repo.baselineSamples < 30 && report.findings.length === 0)
      return "smallbase";
    return "findings";
  }, [repo, busy, report]);

  const connectionCards: ConnectionCard[] = useMemo(() => {
    if (!adapters) return SEED_CONNECTIONS;
    return adapters.map((a) => ({
      name: a.label,
      status: a.installed ? "connected" : a.detected ? "detected" : "not found",
      path: a.config_path ?? "—",
      signal: a.installed ? "wired in · runs dross --staged" : "not wired in",
      limitation:
        a.limitations[0] ??
        "No limitation recorded for this integration on this platform.",
    }));
  }, [adapters]);

  const connected = connectionCards.filter((c) => c.status === "connected").length;

  const facts = useCallback(
    (extra: Fact[]): Fact[] => extra,
    [],
  );

  function body() {
    if (tab === "connections") {
      return (
        <Connections
          cards={connectionCards}
          disabled={!repo || busy !== null}
          onToggle={async (card) => {
            const match = adapters?.find((a) => a.label === card.name);
            if (!match) return;
            const next = await guard(
              match.installed ? "Removing" : "Connecting",
              () =>
                match.installed
                  ? api.uninstallConnection(match.id)
                  : api.installConnection(match.id),
            );
            if (next) setAdapters(next);
          }}
        />
      );
    }

    if (tab === "history") {
      return (
        <RiskHistory
          bars={HISTORY_BARS}
          rows={HISTORY_ROWS}
          logPath={repo ? `${repo.root}/.dross/index.sqlite` : "~/.dross/index.sqlite"}
        />
      );
    }

    if (tab === "settings") {
      return (
        <Settings
          signals={signals}
          checks={checks}
          expanded={expanded}
          cloneThreshold={cloneThreshold}
          zThreshold={zThreshold}
          minSeverity={minSeverity}
          commitGate={commitGate}
          baselineSamples={repo?.baselineSamples ?? 0}
          labelledDiffs={157}
          rounds={3}
          onExpand={setExpanded}
          onToggleSignal={(name, on) =>
            setSignals((prev) =>
              prev.map((s) => (s.name === name ? { ...s, on } : s)),
            )
          }
          onToggleCheck={(name, on) =>
            setChecks((prev) => prev.map((c) => (c.name === name ? { ...c, on } : c)))
          }
          onCloneThreshold={setCloneThreshold}
          onZThreshold={setZThreshold}
          onMinSeverity={setMinSeverity}
          onCommitGate={setCommitGate}
        />
      );
    }

    // Findings tab: one of the designed states, or the split pane.
    if (view === "norepo") {
      return (
        <EmptyState
          kicker="no repository open"
          title="Open a repository to assay a diff."
          body="Dross reads the working tree on this machine and nothing else. No account, no upload, no model call — every check is a parser, a hash, or a graph walk."
          facts={facts([
            { key: "checks available", value: "6" },
            { key: "index", value: "none", tone: "faint" },
            { key: "network", value: "never used" },
          ])}
          cta={{ label: "Open repository…", onClick: openRepo }}
        />
      );
    }

    if (view === "building") {
      const done = progress?.done ?? 0;
      const total = progress?.total ?? repo?.indexedFunctions ?? 0;
      return (
        <EmptyState
          kicker="building index"
          title={`Hashing ${total.toLocaleString()} functions.`}
          body="Parsing each file once, normalizing identifiers and literals, then storing one hash per function in .dross/index.sqlite. You can keep working; findings other than clones are already available."
          facts={facts([
            { key: "phase", value: progress?.phase ?? "fingerprints" },
            {
              key: "functions hashed",
              value: `${done.toLocaleString()} / ${total.toLocaleString()}`,
            },
            {
              key: "clone-detection",
              value: "unavailable until complete",
              tone: "warn",
            },
          ])}
        />
      );
    }

    if (view === "noindex") {
      return (
        <EmptyState
          kicker="index not built"
          title="Clone detection is unavailable until the index is built."
          body="Everything else runs now. Structural-clone comparison needs a normalized-AST hash of every function in the repository, which is then incremental."
          facts={facts([
            { key: "checks running", value: "5 of 6" },
            {
              key: "clone-detection",
              value: "unavailable — no index",
              tone: "warn",
            },
            { key: "estimated build", value: "one pass over the tree" },
          ])}
          cta={{ label: "Build index", onClick: rebuild, disabled: busy !== null }}
        />
      );
    }

    if (view === "smallbase") {
      return (
        <EmptyState
          kicker="complexity baseline too small"
          title="The complexity signal is staying silent."
          body={`This repository has ${repo?.baselineSamples ?? 0} indexed samples. Below 30 a z-score says more about the sample than the code, so Dross reports nothing rather than reporting noise. Every other check ran.`}
          facts={facts([
            {
              key: "baseline samples",
              value: `${repo?.baselineSamples ?? 0} — need 30`,
              tone: "warn",
            },
            { key: "complexity-outlier", value: "silent", tone: "warn" },
            { key: "checks run", value: "5 of 6" },
          ])}
          cta={{ label: "Run remaining checks", onClick: analyze }}
        />
      );
    }

    if (view === "clean" && report) {
      return (
        <EmptyState
          kicker="analysis complete · risk 0"
          title="Nothing to skim off this diff."
          body={`Six checks ran against ${report.files_analyzed} changed ${report.files_analyzed === 1 ? "file" : "files"}. Everything came through clean.`}
          facts={facts([
            { key: "checks run", value: "6 of 6", tone: "ok" },
            {
              key: "files examined",
              value: `${report.files_analyzed}`,
            },
            {
              key: "elapsed",
              value: `${report.duration_ms}ms · deterministic`,
            },
          ])}
          cta={{ label: "Analyze again", onClick: analyze }}
        />
      );
    }

    return (
      <div className="split">
        <Findings
          findings={shown.findings}
          skipped={shown.skipped}
          selected={selected}
          riskScore={shown.risk}
          onSelect={setSelected}
          onOpen={openInEditor}
        />
        <SourcePane finding={shown.findings[selected] ?? null} onOpen={openInEditor} />
      </div>
    );
  }

  return (
    <div className="app">
      <Header
        repo={repo}
        branch={null}
        target={target}
        busy={busy}
        progress={progress}
        tab={tab}
        counts={{
          findings: shown.findings.length,
          connections: `${connected}/${connectionCards.length}`,
          history: HISTORY_ROWS.length,
        }}
        onTargetChange={(next) => {
          setTarget(next);
          if (repo) void analyze();
        }}
        onTabChange={setTab}
        onAnalyze={analyze}
        onRebuild={rebuild}
      />

      {error && (
        <div className="banner" role="alert">
          <span className="banner__kind">error</span>
          <span className="banner__msg">{error}</span>
          <button type="button" className="banner__action" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      <div className="app__body">{body()}</div>

      <div className="status">
        <span>no network calls · 0 bytes sent</span>
        <span>
          {busy
            ? `${busy.toLowerCase()}…`
            : report
              ? `last run ${(report.duration_ms / 1000).toFixed(1)}s · ${report.files_analyzed} files · deterministic`
              : "idle"}
        </span>
      </div>
    </div>
  );
}
