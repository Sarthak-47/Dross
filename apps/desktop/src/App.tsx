import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { api } from "./api";
import { Connections } from "./components/Connections";
import { FindingsPanel } from "./components/FindingsPanel";
import { RiskHistory } from "./components/RiskHistory";
import { Settings } from "./components/Settings";
import { SourceView } from "./components/SourceView";
import { RepoBar } from "./components/RepoBar";
import type {
  AdapterStatus,
  DrossConfig,
  Finding,
  IndexProgress,
  Report,
  RepositoryInfo,
  RiskEntry,
} from "./types";
import "./App.css";

type Tab = "findings" | "connections" | "history" | "settings";
type Target = "worktree" | "staged";

const TAB_LABELS: Record<Tab, string> = {
  findings: "Findings",
  connections: "Connections",
  history: "Risk history",
  settings: "Settings",
};

export default function App() {
  const [repo, setRepo] = useState<RepositoryInfo | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [connections, setConnections] = useState<AdapterStatus[]>([]);
  const [history, setHistory] = useState<RiskEntry[]>([]);
  const [config, setConfig] = useState<DrossConfig | null>(null);
  const [selected, setSelected] = useState<Finding | null>(null);
  const [tab, setTab] = useState<Tab>("findings");
  const [target, setTarget] = useState<Target>("worktree");
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Outside a Tauri window (browser-based UI development) the event bridge
    // is absent; degrade to no progress events rather than throwing.
    const unlisten = listen<IndexProgress>("dross://index-progress", (event) =>
      setProgress(event.payload),
    ).catch(() => undefined);

    return () => {
      void unlisten.then((fn) => fn?.());
    };
  }, []);

  // A repo may already be open when the window reloads during development.
  useEffect(() => {
    api.currentRepository().then(setRepo).catch(() => undefined);
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

  const openRepo = useCallback(
    async (path: string) => {
      const info = await guard("Opening repository", () =>
        api.openRepository(path),
      );
      if (!info) return;
      setRepo(info);
      setReport(null);
      setSelected(null);
      setConnections(await api.listConnections().catch(() => []));
      setConfig(await api.getConfig().catch(() => null));
    },
    [guard],
  );

  const saveConfig = useCallback(
    async (next: DrossConfig) => {
      // Applied optimistically: the settings pane must stay responsive, and a
      // failed write is reported in the error banner.
      setConfig(next);
      const saved = await guard("Saving settings", () => api.setConfig(next));
      if (saved) setConfig(saved);
    },
    [guard],
  );

  const runAnalysis = useCallback(async () => {
    const result = await guard("Analyzing diff", () => api.analyze(target));
    if (result) {
      setReport(result);
      setSelected(result.findings[0] ?? null);
    }
  }, [guard, target]);

  const buildIndex = useCallback(async () => {
    const info = await guard("Building index", () => api.buildIndex());
    if (info) setRepo(info);
  }, [guard]);

  const loadHistory = useCallback(async () => {
    setHistory(await api.riskHistory(100).catch(() => []));
  }, []);

  useEffect(() => {
    if (tab === "history") void loadHistory();
    if (tab === "connections" && repo) {
      api.listConnections().then(setConnections).catch(() => undefined);
    }
  }, [tab, repo, loadHistory]);

  return (
    <div className="app">
      <RepoBar
        repo={repo}
        target={target}
        busy={busy}
        progress={progress}
        onOpen={openRepo}
        onAnalyze={runAnalysis}
        onBuildIndex={buildIndex}
        onTargetChange={setTarget}
      />

      {error && (
        <div className="banner banner-error" role="alert">
          <strong>Error</strong>
          <span>{error}</span>
          <button onClick={() => setError(null)} aria-label="Dismiss">
            ×
          </button>
        </div>
      )}

      {repo && !repo.indexBuilt && (
        <div className="banner banner-note">
          <strong>Index not built</strong>
          <span>
            Clone detection stays off until the repository is indexed. The
            complexity baseline needs 30+ history samples before the
            over-engineering outlier signal reports anything.
          </span>
        </div>
      )}

      <nav className="tabs">
        {(Object.keys(TAB_LABELS) as Tab[]).map((t) => (
          <button
            key={t}
            className={t === tab ? "tab tab-active" : "tab"}
            onClick={() => setTab(t)}
          >
            {TAB_LABELS[t]}
            {t === "findings" && report && report.findings.length > 0 && (
              <span className="tab-count">{report.findings.length}</span>
            )}
          </button>
        ))}
      </nav>

      <main className="main">
        {tab === "findings" && (
          <div className="split">
            <FindingsPanel
              report={report}
              selected={selected}
              onSelect={setSelected}
            />
            <SourceView finding={selected} />
          </div>
        )}

        {tab === "connections" && (
          <Connections
            statuses={connections}
            disabled={!repo || busy !== null}
            onInstall={async (id) => {
              const next = await guard("Installing", () =>
                api.installConnection(id),
              );
              if (next) setConnections(next);
            }}
            onUninstall={async (id) => {
              const next = await guard("Removing", () =>
                api.uninstallConnection(id),
              );
              if (next) setConnections(next);
            }}
          />
        )}

        {tab === "history" && <RiskHistory entries={history} />}

        {tab === "settings" && (
          <Settings
            config={config}
            disabled={!repo || busy !== null}
            onChange={saveConfig}
          />
        )}
      </main>
    </div>
  );
}
