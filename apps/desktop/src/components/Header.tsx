import { Segmented } from "./controls";
import type { IndexProgress, RepositoryInfo, Tab, Target } from "../types";

interface Props {
  repo: RepositoryInfo | null;
  branch: string | null;
  target: Target;
  busy: string | null;
  progress: IndexProgress | null;
  tab: Tab;
  counts: { findings: number; connections: string; history: number };
  onTargetChange: (target: Target) => void;
  onTabChange: (tab: Tab) => void;
  onAnalyze: () => void;
  onRebuild: () => void;
}

const TABS: { id: Tab; label: string }[] = [
  { id: "findings", label: "Findings" },
  { id: "connections", label: "Connections" },
  { id: "history", label: "Risk history" },
  { id: "settings", label: "Settings" },
];

export function Header({
  repo,
  branch,
  target,
  busy,
  progress,
  tab,
  counts,
  onTargetChange,
  onTabChange,
  onAnalyze,
  onRebuild,
}: Props) {
  const indexed = !repo
    ? "—"
    : progress && progress.total > 0
      ? `${progress.done.toLocaleString()} / ${progress.total.toLocaleString()}`
      : repo.indexBuilt
        ? repo.indexedFunctions.toLocaleString()
        : "not built";

  const baseline = !repo
    ? "—"
    : repo.baselineSamples >= 30
      ? `${repo.baselineSamples} functions`
      : `${repo.baselineSamples} — below minimum`;

  const authorship = !repo
    ? "—"
    : repo.watcherActive
      ? "watching"
      : "trailers only";

  const authorshipDot = !repo
    ? "dot"
    : repo.watcherActive
      ? "dot dot--ok"
      : "dot dot--warn";

  const pct =
    progress && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : 0;

  return (
    <>
      <header className="header">
        <div className="repo">
          <div className="repo__line">
            <span className="repo__name">{repo?.name ?? "no repository"}</span>
            {branch && <span className="repo__branch">{branch}</span>}
          </div>
          <span className="repo__path">{repo?.root ?? "—"}</span>
        </div>

        <div className="metrics">
          <div className="metric">
            <span className="metric__label">functions indexed</span>
            <span className="metric__value">{indexed}</span>
          </div>
          <div className="metric">
            <span className="metric__label">complexity baseline</span>
            <span className="metric__value">{baseline}</span>
          </div>
          <div className="metric">
            <span className="metric__label">authorship source</span>
            <span className="metric__value">
              <span className={authorshipDot} />
              {authorship}
            </span>
          </div>
        </div>

        <div className="header__spacer" />

        <Segmented
          label="Diff target"
          value={target}
          disabled={busy !== null}
          onChange={onTargetChange}
          options={[
            { value: "working", label: "Working tree" },
            { value: "staged", label: "Staged" },
          ]}
        />

        <button
          type="button"
          className="btn"
          onClick={onRebuild}
          disabled={!repo || busy !== null}
        >
          Rebuild index
        </button>
        <button
          type="button"
          className="btn btn--primary"
          onClick={onAnalyze}
          disabled={!repo || busy !== null}
        >
          Analyze
        </button>
      </header>

      {/* Work never blocks: the progress row reports phase and the UI stays usable. */}
      {busy && (
        <div className="progress">
          <span className="progress__phase">
            {progress?.phase
              ? `${busy.toLowerCase()} · ${progress.phase}`
              : `${busy.toLowerCase()}…`}
          </span>
          <div className="progress__track">
            <div className="progress__fill" style={{ width: `${pct}%` }} />
          </div>
          <span className="progress__pct">{pct}%</span>
        </div>
      )}

      <nav className="tabs" role="tablist">
        {TABS.map(({ id, label }) => {
          const count =
            id === "findings"
              ? counts.findings > 0
                ? String(counts.findings)
                : "0"
              : id === "connections"
                ? counts.connections
                : id === "history"
                  ? String(counts.history)
                  : null;
          return (
            <button
              key={id}
              type="button"
              role="tab"
              className="tab"
              aria-selected={tab === id}
              onClick={() => onTabChange(id)}
            >
              {label}
              {count !== null && <span className="tab__count">{count}</span>}
            </button>
          );
        })}
      </nav>
    </>
  );
}
