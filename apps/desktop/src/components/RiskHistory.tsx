import { useMemo } from "react";
import type { RiskEntry } from "../types";

interface Props {
  entries: RiskEntry[];
}

export function RiskHistory({ entries }: Props) {
  const buckets = useMemo(() => groupByDay(entries), [entries]);

  if (entries.length === 0) {
    return (
      <div className="empty">
        <h2>No history yet</h2>
        <p className="dim">
          Each analysis run is recorded locally. Run Analyze a few times to
          build a trend.
        </p>
      </div>
    );
  }

  const max = Math.max(...buckets.map((b) => b.total), 1);

  return (
    <div className="history">
      <div className="trend" role="img" aria-label="Findings per day">
        {buckets.map((bucket) => (
          <div className="trend-col" key={bucket.day} title={`${bucket.day}: ${bucket.total}`}>
            <div className="trend-stack">
              {(["error", "warning", "info"] as const).map((sev) => {
                const value = bucket.bySeverity[sev] ?? 0;
                if (value === 0) return null;
                return (
                  <div
                    key={sev}
                    className={`trend-bar trend-${sev}`}
                    style={{ height: `${(value / max) * 100}%` }}
                  />
                );
              })}
            </div>
            <span className="trend-label">{bucket.day.slice(5)}</span>
          </div>
        ))}
      </div>

      <table className="table">
        <thead>
          <tr>
            <th>When</th>
            <th>Signal</th>
            <th>Severity</th>
            <th className="num">Count</th>
          </tr>
        </thead>
        <tbody>
          {entries.map((entry, i) => (
            <tr key={i}>
              <td className="dim">{entry.recorded_at.replace("T", " ").slice(0, 19)}</td>
              <td>{entry.signal}</td>
              <td>
                <span className={`sev sev-${entry.severity}`}>
                  {entry.severity}
                </span>
              </td>
              <td className="num">{entry.count}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface Bucket {
  day: string;
  total: number;
  bySeverity: Record<string, number>;
}

function groupByDay(entries: RiskEntry[]): Bucket[] {
  const map = new Map<string, Bucket>();
  for (const entry of entries) {
    const day = entry.recorded_at.slice(0, 10);
    const bucket = map.get(day) ?? { day, total: 0, bySeverity: {} };
    bucket.total += entry.count;
    bucket.bySeverity[entry.severity] =
      (bucket.bySeverity[entry.severity] ?? 0) + entry.count;
    map.set(day, bucket);
  }
  return [...map.values()].sort((a, b) => a.day.localeCompare(b.day));
}
