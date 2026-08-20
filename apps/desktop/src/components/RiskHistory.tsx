import type { HistoryBar, HistoryRow } from "../types";

/** Height per finding, in pixels, within the 170px band. */
const UNIT = 11;

export function RiskHistory({
  bars,
  rows,
  logPath,
}: {
  bars: HistoryBar[];
  rows: HistoryRow[];
  logPath: string;
}) {
  return (
    <div className="view">
      <div className="history">
        <div className="history__head">
          <div className="view__head">
            <h2 className="h">Findings per analysis</h2>
            <span className="mono-note">
              local log · {logPath} · last {bars.length} runs
            </span>
          </div>

          <div className="legend">
            {(
              [
                ["error", "var(--ember)"],
                ["warning", "var(--warn)"],
                ["info", "var(--info)"],
              ] as const
            ).map(([label, color]) => (
              <span className="legend__item" key={label}>
                <span className="legend__sq" style={{ background: color }} />
                {label}
              </span>
            ))}
          </div>
        </div>

        {/* Flat bars, no rounding, no gridlines, no axis chrome. */}
        <div className="chart">
          {bars.map(([error, warning, info, day]) => (
            <div className="chart__col" key={day}>
              <div className="chart__stack">
                <div style={{ height: info * UNIT, background: "var(--info)" }} />
                <div style={{ height: warning * UNIT, background: "var(--warn)" }} />
                <div style={{ height: error * UNIT, background: "var(--ember)" }} />
              </div>
              <span className="chart__day">{day}</span>
            </div>
          ))}
        </div>

        <div className="table">
          <div className="table__row table__row--head">
            <span>when</span>
            <span>commit</span>
            <span>subject</span>
            <span>error</span>
            <span>warn</span>
            <span>info</span>
            <span>risk</span>
          </div>
          {rows.map((row) => (
            <div className="table__row" key={row.sha}>
              <span style={{ color: "var(--dim)" }}>{row.when}</span>
              <span style={{ color: "var(--ember)" }}>{row.sha}</span>
              <span className="table__subject">{row.subject}</span>
              <span style={{ color: "var(--text)" }}>{row.e}</span>
              <span style={{ color: "var(--dim)" }}>{row.w}</span>
              <span style={{ color: "var(--faint)" }}>{row.i}</span>
              <span style={{ color: "var(--text)" }}>{row.risk}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
