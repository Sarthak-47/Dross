import type { SeedFinding } from "../types";

/**
 * There is deliberately no syntax highlighting here. The highlight in this
 * pane means "this is what the finding refers to"; coloured tokens would
 * compete with it.
 */
export function SourcePane({
  finding,
  onOpen,
}: {
  finding: SeedFinding | null;
  onOpen: (location: string) => void;
}) {
  if (!finding) {
    return (
      <div className="split__right">
        <div className="state">
          <p className="state__body">Select a finding to see the code it refers to.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="split__right">
      <div className="source__head">
        <span className="source__path">{finding.file}</span>
        <span className="source__range">{finding.range}</span>
        <div className="header__spacer" />
        <button
          type="button"
          className="btn btn--tiny"
          onClick={() => onOpen(finding.location)}
        >
          Open in editor
        </button>
      </div>

      <div className="source__scroll">
        <pre className="code">
          {finding.code.map(([line, marker, text, hit]) => (
            <div
              key={line}
              className={hit ? "code__line code__line--hit" : "code__line"}
            >
              <span className="code__gutter">{line}</span>
              <span className="code__marker">{marker.trim() ? marker : " "}</span>
              <span>{text}</span>
            </div>
          ))}
        </pre>

        <div className="method">
          <span className="micro" style={{ letterSpacing: "0.14em" }}>
            how this was measured
          </span>
          <p className="method__prose">{finding.method}</p>
          <div className="method__metrics">
            {finding.metrics.map((metric) => (
              <div className="method__metric" key={metric.label}>
                <span className="micro">{metric.label}</span>
                <span className="method__value">{metric.value}</span>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
