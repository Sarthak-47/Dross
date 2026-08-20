import { useEffect, useRef } from "react";
import { Loc } from "./controls";
import { AUTHORSHIP_TEXT, type SeedFinding, type SkippedRow } from "../types";

interface Props {
  findings: SeedFinding[];
  skipped: SkippedRow[];
  selected: number;
  riskScore: number;
  onSelect: (index: number) => void;
  onOpen: (location: string) => void;
}

/** Weighted the same way the engine scores: errors dominate, info barely moves it. */
const WEIGHT = { error: 20, warning: 12, info: 4 } as const;

export function Findings({
  findings,
  skipped,
  selected,
  riskScore,
  onSelect,
  onOpen,
}: Props) {
  const listRef = useRef<HTMLDivElement>(null);

  // Arrow keys move the selection; Enter opens the editor.
  useEffect(() => {
    const node = listRef.current;
    if (!node) return;

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const next =
          event.key === "ArrowDown"
            ? Math.min(selected + 1, findings.length - 1)
            : Math.max(selected - 1, 0);
        onSelect(next);
      } else if (event.key === "Enter" && findings[selected]) {
        onOpen(findings[selected].location);
      }
    };

    node.addEventListener("keydown", onKey);
    return () => node.removeEventListener("keydown", onKey);
  }, [findings, selected, onSelect, onOpen]);

  const counts = {
    error: findings.filter((f) => f.severity === "error").length,
    warning: findings.filter((f) => f.severity === "warning").length,
    info: findings.filter((f) => f.severity === "info").length,
  };

  // Segment widths are proportional to counts against the cap, not to the total.
  const width = (n: number, weight: number) => `${Math.min(n * weight, 100)}%`;

  return (
    <div className="split__left">
      <div className="risk">
        <div>
          <span className="micro">risk score</span>
          <div className="risk__score">
            {riskScore}
            <span className="risk__of">/100</span>
          </div>
        </div>

        <div className="risk__right">
          <div className="sevbar">
            {counts.error > 0 && (
              <div
                className="sevbar__seg--error"
                style={{ width: width(counts.error, WEIGHT.error) }}
              />
            )}
            {counts.warning > 0 && (
              <div
                className="sevbar__seg--warning"
                style={{ width: width(counts.warning, WEIGHT.warning) }}
              />
            )}
            {counts.info > 0 && (
              <div
                className="sevbar__seg--info"
                style={{ width: width(counts.info, WEIGHT.info) }}
              />
            )}
            <div className="sevbar__rest" />
          </div>

          <div className="risk__counts">
            <span>{counts.error} error</span>
            <span>{counts.warning} warning</span>
            <span>{counts.info} info</span>
            {skipped.length > 0 && (
              <span className="muted">{skipped.length} skipped</span>
            )}
          </div>

          <span className="risk__formula">
            weighted: {counts.error}×error({WEIGHT.error}) +{" "}
            {counts.warning}×warning({WEIGHT.warning}) + {counts.info}×info(
            {WEIGHT.info}) · capped at 100
          </span>
        </div>
      </div>

      <div className="findings" ref={listRef} tabIndex={-1}>
        {findings.map((finding, index) => {
          const isSelected = index === selected;
          return (
            <button
              key={`${finding.location}-${index}`}
              type="button"
              className={`finding finding--${finding.severity}`}
              aria-current={isSelected}
              onClick={() => onSelect(index)}
            >
              <span className={`sq sq--${finding.severity}`} />
              <span className="finding__body">
                <span className="finding__msg">{finding.message}</span>

                <span className="finding__meta">
                  <Loc value={finding.location} onOpen={onOpen} />
                  {finding.tags.map((tag) => (
                    <span className="tag" key={tag}>
                      {tag}
                    </span>
                  ))}
                </span>

                <span className="finding__evidence">{finding.evidence}</span>

                {finding.authorship && (
                  <span
                    className={
                      finding.authorship === "heuristic"
                        ? "authorship authorship--heuristic"
                        : "authorship"
                    }
                  >
                    {AUTHORSHIP_TEXT[finding.authorship]}
                  </span>
                )}

                {isSelected && finding.related.length > 0 && (
                  <span className="related">
                    {finding.related.map((ref) => (
                      <span className="related__row" key={ref.key + ref.value}>
                        <span className="related__key">{ref.key}</span>
                        <Loc value={ref.value} onOpen={onOpen} />
                      </span>
                    ))}
                  </span>
                )}
              </span>
            </button>
          );
        })}

        {/* A check that could not run always says why. */}
        {skipped.map((row) => (
          <div className="skipped" key={row.check}>
            <span className="sq sq--hollow" />
            <div className="skipped__body">
              <div className="skipped__head">
                <span className="skipped__word">skipped</span>
                <span className="skipped__check">{row.check}</span>
              </div>
              <span className="skipped__reason">{row.reason}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
