import { useEffect, useState } from "react";
import { api } from "../api";
import type { Finding } from "../types";

/** Lines of context rendered around a finding's span. */
const CONTEXT = 6;

interface Props {
  finding: Finding | null;
}

export function SourceView({ finding }: Props) {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!finding) {
      setLines(null);
      return;
    }
    let cancelled = false;
    setError(null);
    api
      .fileSource(finding.span.file)
      .then((text) => {
        if (!cancelled) setLines(text.split(/\r?\n/));
      })
      .catch((e) => {
        if (!cancelled) {
          setError(String(e));
          setLines(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [finding]);

  if (!finding) {
    return (
      <section className="source">
        <div className="empty">
          <p className="dim">Select a finding to see the code it refers to.</p>
        </div>
      </section>
    );
  }

  const start = Math.max(1, finding.span.start_line - CONTEXT);
  const end = finding.span.end_line + CONTEXT;
  const window = lines?.slice(start - 1, end) ?? [];

  return (
    <section className="source">
      <div className="source-head">
        <span className="source-path">{finding.span.file}</span>
        <span className="dim">
          lines {finding.span.start_line}–{finding.span.end_line}
        </span>
      </div>

      {error && <div className="source-error">{error}</div>}

      {lines && (
        <pre className="code">
          {window.map((line, i) => {
            const lineNo = start + i;
            const inSpan =
              lineNo >= finding.span.start_line &&
              lineNo <= finding.span.end_line;
            return (
              <div
                key={lineNo}
                className={inSpan ? "code-line code-line-hit" : "code-line"}
              >
                <span className="code-no">{lineNo}</span>
                <span className="code-text">{line || " "}</span>
              </div>
            );
          })}
        </pre>
      )}

      {finding.related && finding.related.length > 0 && (
        <div className="related">
          <h3>Related</h3>
          {finding.related.map((r, i) => (
            <div key={i} className="related-row">
              {r.file}:{r.start_line}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
