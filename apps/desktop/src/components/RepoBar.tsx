import { useState } from "react";
import type { IndexProgress, RepositoryInfo } from "../types";

interface Props {
  repo: RepositoryInfo | null;
  target: "worktree" | "staged";
  busy: string | null;
  progress: IndexProgress | null;
  onOpen: (path: string) => void;
  onAnalyze: () => void;
  onBuildIndex: () => void;
  onTargetChange: (target: "worktree" | "staged") => void;
}

export function RepoBar({
  repo,
  target,
  busy,
  progress,
  onOpen,
  onAnalyze,
  onBuildIndex,
  onTargetChange,
}: Props) {
  const [path, setPath] = useState("");

  return (
    <header className="repobar">
      <div className="repobar-brand">
        <span className="brand-mark">Dross</span>
        <span className="brand-tag">deterministic · offline · no model calls</span>
      </div>

      {repo ? (
        <div className="repobar-repo">
          <span className="repo-name">{repo.name}</span>
          <span className="repo-meta">
            {repo.indexedFunctions.toLocaleString()} functions indexed ·{" "}
            {repo.baselineSamples} baseline samples ·{" "}
            <span
              title={
                repo.watcherActive
                  ? "Edits are being watched, so burst-write authorship detection is active."
                  : "No file watcher, so authorship comes only from commit trailers."
              }
            >
              authorship: {repo.watcherActive ? "watching" : "trailers only"}
            </span>
          </span>
        </div>
      ) : (
        <form
          className="repobar-open"
          onSubmit={(e) => {
            e.preventDefault();
            if (path.trim()) onOpen(path.trim());
          }}
        >
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="Path to a git repository"
            spellCheck={false}
          />
          <button type="submit" disabled={!path.trim() || busy !== null}>
            Open
          </button>
        </form>
      )}

      <div className="repobar-actions">
        <div className="segmented" role="group" aria-label="Diff target">
          {(["worktree", "staged"] as const).map((t) => (
            <button
              key={t}
              className={t === target ? "seg seg-active" : "seg"}
              onClick={() => onTargetChange(t)}
              disabled={busy !== null}
            >
              {t === "worktree" ? "Working tree" : "Staged"}
            </button>
          ))}
        </div>

        <button
          className="btn"
          onClick={onBuildIndex}
          disabled={!repo || busy !== null}
        >
          Rebuild index
        </button>
        <button
          className="btn btn-primary"
          onClick={onAnalyze}
          disabled={!repo || busy !== null}
        >
          Analyze
        </button>
      </div>

      {busy && (
        <div className="repobar-busy">
          <span className="spinner" aria-hidden />
          <span>
            {busy}
            {progress && progress.total > 0
              ? ` — ${progress.done}/${progress.total} files`
              : progress?.phase === "baseline"
                ? " — replaying history for the complexity baseline"
                : "…"}
          </span>
        </div>
      )}
    </header>
  );
}
