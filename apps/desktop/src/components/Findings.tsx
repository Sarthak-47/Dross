import { useEffect, useRef } from "react";
import { Loc } from "./controls";
import { WEIGHT } from "../derive";
import { AUTHORSHIP_TEXT, type SeedFinding, type SkippedRow } from "../types";

interface Props {
  findings: SeedFinding[];
  skipped: SkippedRow[];
  selected: number;
  riskScore: number;
  onSelect: (index: number) => void;
  onOpen: (location: string) => void;
}

export function Findings({
  findings,
  skipped,
  selected,
  riskScore,
  onSelect,
  onOpen,
}: Props) {
  const listRef = useRef<HTMLDivElement>(null);

  /* Where the selection is, readable from the key handler without re-binding
   * it on every move. The handler used to close over `selected`, so a burst of
   * key events that arrived before React re-rendered all computed the same
   * next index: thirty presses advanced the selection by one. Holding an arrow
   * key is exactly that burst. */
  const selectedRef = useRef(selected);

  // Arrow keys move the selection; Enter opens the editor.
  useEffect(() => {
    const node = listRef.current;
    if (!node) return;

    // Arrows only. Enter is handled on the row itself: this listener sees the
    // event from whichever row has focus but would act on `selected`, so
    // tabbing to a row and pressing Enter opened a different finding's file
    // than the one under the cursor.
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      event.preventDefault();
      const next =
        event.key === "ArrowDown"
          ? Math.min(selectedRef.current + 1, findings.length - 1)
          : Math.max(selectedRef.current - 1, 0);
      // Advanced here as well as in state, so the next event in the same burst
      // starts from this one rather than from the last render.
      selectedRef.current = next;
      onSelect(next);
    };

    node.addEventListener("keydown", onKey);
    return () => node.removeEventListener("keydown", onKey);
  }, [findings.length, onSelect]);

  // Keep the selection in view.
  //
  // Without this the arrow keys moved a selection that stayed put on screen:
  // twelve presses down a real report put the selected row 1,200px below the
  // visible area, and the list never scrolled. It was invisible while the only
  // test data was three fixture rows that all fitted at once.
  //
  // `nearest` so a row already on screen does not jump, and so clicking a row
  // never scrolls the list out from under the cursor.
  useEffect(() => {
    // Also where the ref is resynced, rather than during render: a click, or
    // an analysis resetting to the top, has to be where the next arrow press
    // starts from.
    selectedRef.current = selected;

    const row = listRef.current?.children[selected];
    if (row instanceof HTMLElement) {
      row.scrollIntoView({ block: "nearest" });
    }
  }, [selected]);

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
            /* A row that contains buttons (the locations) cannot itself be a
               button — nesting them is invalid HTML. It is a div with the
               button role and keyboard handling instead. */
            <div
              key={`${finding.location}-${index}`}
              role="button"
              tabIndex={0}
              className={`finding finding--${finding.severity}`}
              aria-current={isSelected}
              onClick={() => onSelect(index)}
              onKeyDown={(event) => {
                // Enter acts on this row, not on whatever was selected before.
                if (event.key === "Enter") {
                  event.preventDefault();
                  onSelect(index);
                  onOpen(finding.location);
                } else if (event.key === " ") {
                  event.preventDefault();
                  onSelect(index);
                }
              }}
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
            </div>
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
