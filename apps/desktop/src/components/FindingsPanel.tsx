import {
  AUTHORSHIP_LABELS,
  CHECK_LABELS,
  type Finding,
  type Report,
} from "../types";

interface Props {
  report: Report | null;
  selected: Finding | null;
  onSelect: (finding: Finding) => void;
}

export function FindingsPanel({ report, selected, onSelect }: Props) {
  if (!report) {
    return (
      <section className="panel">
        <div className="empty">
          <h2>No analysis yet</h2>
          <p>
            Open a repository and run Analyze. Every check is a parser, a hash,
            or a graph query — nothing leaves this machine.
          </p>
        </div>
      </section>
    );
  }

  if (report.findings.length === 0) {
    return (
      <section className="panel">
        <div className="empty empty-clean">
          <h2>Clean</h2>
          <p>
            {report.files_analyzed} file
            {report.files_analyzed === 1 ? "" : "s"} analyzed in{" "}
            {report.duration_ms}ms. No findings.
          </p>
          <SkippedNotes report={report} />
        </div>
      </section>
    );
  }

  return (
    <section className="panel">
      <div className="panel-head">
        <RiskMeter score={report.risk_score} />
        <div className="panel-stats">
          <span>
            {report.findings.length} finding
            {report.findings.length === 1 ? "" : "s"}
          </span>
          <span className="dim">
            {report.files_analyzed} files · {report.duration_ms}ms
          </span>
        </div>
      </div>

      <ul className="findings">
        {report.findings.map((finding, i) => {
          const isSelected =
            selected?.span.file === finding.span.file &&
            selected?.span.start_line === finding.span.start_line &&
            selected?.signal === finding.signal;
          return (
            <li key={`${finding.signal}-${i}`}>
              <button
                className={isSelected ? "finding finding-active" : "finding"}
                onClick={() => onSelect(finding)}
              >
                <div className="finding-top">
                  <span className={`sev sev-${finding.severity}`}>
                    {finding.severity}
                  </span>
                  <span className="finding-msg">{finding.message}</span>
                </div>
                <div className="finding-loc">
                  {finding.span.file}:{finding.span.start_line}
                </div>
                <div className="finding-evidence">{finding.evidence}</div>
                <div className="finding-tags">
                  <span className="tag">{CHECK_LABELS[finding.check]}</span>
                  <span className="tag tag-dim">{finding.signal}</span>
                  {finding.authorship !== "unknown" && (
                    <span
                      className={`tag tag-authorship tag-${finding.authorship}`}
                      title="Authorship detection is heuristic and can be corrected"
                    >
                      {AUTHORSHIP_LABELS[finding.authorship]}
                    </span>
                  )}
                </div>
              </button>
            </li>
          );
        })}
      </ul>

      <SkippedNotes report={report} />
    </section>
  );
}

function SkippedNotes({ report }: { report: Report }) {
  if (report.skipped.length === 0) return null;
  return (
    <div className="skipped">
      {report.skipped.map((s) => (
        <div key={s.check} className="skipped-row">
          <strong>{s.check}</strong> skipped — {s.reason}
        </div>
      ))}
    </div>
  );
}

function RiskMeter({ score }: { score: number }) {
  const level = score >= 60 ? "high" : score >= 25 ? "medium" : "low";
  return (
    <div className={`risk risk-${level}`}>
      <div className="risk-score">{score}</div>
      <div className="risk-label">risk / 100</div>
    </div>
  );
}
