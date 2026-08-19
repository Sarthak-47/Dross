import { CHECK_LABELS, type CheckId, type DrossConfig, type Severity } from "../types";

const CHECK_IDS = Object.keys(CHECK_LABELS) as CheckId[];

const SEVERITIES: Severity[] = ["info", "warning", "error"];

interface Props {
  config: DrossConfig | null;
  disabled: boolean;
  onChange: (config: DrossConfig) => void;
}

export function Settings({ config, disabled, onChange }: Props) {
  if (!config) {
    return (
      <div className="empty">
        <p className="dim">Open a repository to edit its settings.</p>
      </div>
    );
  }

  const disabledChecks = new Set(config.disabled_checks);

  const toggleCheck = (id: CheckId) => {
    const next = new Set(disabledChecks);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    onChange({ ...config, disabled_checks: [...next] });
  };

  return (
    <div className="settings">
      <section className="settings-group">
        <h3>Checks</h3>
        <p className="dim">
          Disabled checks are reported as skipped rather than silently omitted,
          so a run always accounts for every check.
        </p>
        {CHECK_IDS.map((id) => (
          <label key={id} className="row">
            <input
              type="checkbox"
              checked={!disabledChecks.has(id)}
              disabled={disabled}
              onChange={() => toggleCheck(id)}
            />
            <span>{CHECK_LABELS[id]}</span>
          </label>
        ))}
      </section>

      <section className="settings-group">
        <h3>Thresholds</h3>

        <label className="row row-stacked">
          <span>Minimum severity shown</span>
          <select
            value={config.min_severity}
            disabled={disabled}
            onChange={(e) =>
              onChange({ ...config, min_severity: e.target.value as Severity })
            }
          >
            {SEVERITIES.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </label>

        <label className="row row-stacked">
          <span>
            Clone similarity threshold
            <em className="dim"> — lower catches more, flags more</em>
          </span>
          <input
            type="range"
            min={0.5}
            max={1}
            step={0.01}
            value={config.clone_threshold}
            disabled={disabled}
            onChange={(e) =>
              onChange({
                ...config,
                clone_threshold: Number(e.target.value),
              })
            }
          />
          <span className="dim">{config.clone_threshold.toFixed(2)}</span>
        </label>

        <label className="row row-stacked">
          <span>
            Complexity outlier z-score
            <em className="dim"> — needs 30+ history samples to fire at all</em>
          </span>
          <input
            type="number"
            min={1}
            max={5}
            step={0.1}
            value={config.complexity_z_threshold}
            disabled={disabled}
            onChange={(e) =>
              onChange({
                ...config,
                complexity_z_threshold: Number(e.target.value),
              })
            }
          />
        </label>
      </section>

      <section className="settings-group">
        <h3>Commit gate</h3>
        <p className="dim">
          Controls the pre-commit hook only. The default is advisory: a hook
          that blocks on a false positive gets uninstalled, so blocking is
          opt-in.
        </p>
        <label className="row row-stacked">
          <span>Block commits at</span>
          <select
            value={config.block_at ?? "none"}
            disabled={disabled}
            onChange={(e) =>
              onChange({
                ...config,
                block_at:
                  e.target.value === "none" ? null : (e.target.value as Severity),
              })
            }
          >
            <option value="none">never block (advisory only)</option>
            {SEVERITIES.map((s) => (
              <option key={s} value={s}>
                {s} and above
              </option>
            ))}
          </select>
        </label>
      </section>

      <p className="dim settings-foot">
        Saved to <code>.dross.json</code> in the repository, so the CLI and any
        hook use the same settings.
      </p>
    </div>
  );
}
