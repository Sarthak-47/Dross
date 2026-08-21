import { Segmented, Switch } from "./controls";
import type { CheckRow, Severity, SignalRow } from "../types";

interface Props {
  signals: SignalRow[];
  checks: CheckRow[];
  expanded: string | null;
  cloneThreshold: number;
  zThreshold: number;
  minSeverity: Severity;
  commitGate: "advisory" | "block";
  baselineSamples: number;
  labelledDiffs: number;
  rounds: number;
  /** No repository open, so there is no .dross.json to write to. */
  disabled: boolean;
  onToggleSignal: (name: string, on: boolean) => void;
  onToggleCheck: (name: string, on: boolean) => void;
  onExpand: (name: string | null) => void;
  onCloneThreshold: (value: number) => void;
  onZThreshold: (value: number) => void;
  onMinSeverity: (value: Severity) => void;
  onCommitGate: (value: "advisory" | "block") => void;
}

/** ≥80 ok · 50–79 warn · <50 ember. */
function band(precision: number) {
  if (precision >= 80) return "prec prec--ok";
  if (precision >= 50) return "prec prec--mid";
  return "prec prec--low";
}

export function Settings({
  signals,
  checks,
  expanded,
  cloneThreshold,
  zThreshold,
  minSeverity,
  commitGate,
  baselineSamples,
  labelledDiffs,
  rounds,
  disabled,
  onToggleSignal,
  onToggleCheck,
  onExpand,
  onCloneThreshold,
  onZThreshold,
  onMinSeverity,
  onCommitGate,
}: Props) {
  return (
    <div className="view">
      {/* Every control writes straight to the repository's .dross.json, so
          without a repository there is nowhere to write. The controls used to
          stay live here and silently discard the change. */}
      {disabled && (
        <p className="sub" style={{ marginBottom: 18 }}>
          Settings are stored per repository, in its <code>.dross.json</code>.
          Open a repository to change them.
        </p>
      )}
      <div className="settings">
        <div className="settings__main">
          <section>
            <div className="section__head">
              <h2 className="h">Signals</h2>
              <span className="mono-note">
                precision measured on {rounds} benchmark rounds · {labelledDiffs}{" "}
                labelled findings
              </span>
            </div>
            <p className="sub" style={{ maxWidth: 780, marginTop: 6 }}>
              Five signals ship disabled because they measured badly. The number
              beside each one is what it actually scored here, not a claim. Turn
              them on if you want them; they will be noisy.
            </p>

            <div className="signals">
              <div className="signals__row signals__row--head">
                <span>signal</span>
                <span>precision</span>
                <span>default</span>
                <span style={{ justifySelf: "end" }}>on</span>
              </div>

              {signals.map((signal) => {
                const open = expanded === signal.name;
                return (
                  <div key={signal.name}>
                    {/* Holds the toggle, so it is a div with the button role
                        rather than a button — nested buttons are invalid. */}
                    <div
                      role="button"
                      tabIndex={0}
                      className={
                        open ? "signals__row signals__row--open" : "signals__row"
                      }
                      aria-expanded={open}
                      onClick={() => onExpand(open ? null : signal.name)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" || event.key === " ") {
                          event.preventDefault();
                          onExpand(open ? null : signal.name);
                        }
                      }}
                    >
                      <span
                        className={
                          signal.on ? "signals__name" : "signals__name signals__name--off"
                        }
                      >
                        <span
                          style={{
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                          }}
                        >
                          {signal.name}
                        </span>
                        {signal.heuristic && <span className="chip">heuristic</span>}
                      </span>

                      <span className={band(signal.precision)}>
                        <span className="prec__num">{signal.precision}%</span>
                        <span className="prec__track">
                          <span
                            className="prec__fill"
                            style={{ width: `${signal.precision}%` }}
                          />
                        </span>
                      </span>

                      <span className="signals__default">
                        {signal.def} by default
                      </span>

                      <Switch
                        checked={signal.on}
                        label={`${signal.name} enabled`}
                        disabled={disabled}
                        onChange={(next) => onToggleSignal(signal.name, next)}
                      />
                    </div>

                    {open && (
                      <div className="signals__detail">
                        <p className="signals__reason">{signal.reason}</p>
                        <div className="signals__rounds">
                          {signal.rounds.map((round) => (
                            <span key={round}>{round}</span>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </section>

          <section>
            <h2 className="h">Checks</h2>
            <div className="checks">
              {checks.map((check) => (
                <div className="check" key={check.name}>
                  <div className="check__body">
                    <span className="check__name">{check.name}</span>
                    <span className="check__note">{check.note}</span>
                  </div>
                  <Switch
                    checked={check.on}
                    label={`${check.name} enabled`}
                    disabled={disabled}
                    onChange={(next) => onToggleCheck(check.name, next)}
                  />
                </div>
              ))}
            </div>
          </section>
        </div>

        <aside className="settings__rail">
          <div className="card">
            <span className="micro" style={{ letterSpacing: "0.14em" }}>
              thresholds
            </span>

            <div className="field">
              <div className="field__top">
                <span className="field__label">Clone similarity</span>
                <span className="field__value">{cloneThreshold.toFixed(2)}</span>
              </div>
              <input
                className="range"
                type="range"
                min={0.7}
                max={1}
                step={0.02}
                value={cloneThreshold}
                aria-label="Clone similarity threshold"
                style={
                  {
                    "--fill": `${((cloneThreshold - 0.7) / 0.3) * 100}%`,
                  } as React.CSSProperties
                }
                disabled={disabled}
                onChange={(event) => onCloneThreshold(Number(event.target.value))}
              />
              <span className="field__note">
                Normalized-AST match ratio required before a pair is reported.
              </span>
            </div>

            <div className="field">
              <div className="field__top">
                <span className="field__label">Complexity z-score</span>
                <span className="field__value">z ≥ {zThreshold.toFixed(1)}</span>
              </div>
              <input
                className="range"
                type="range"
                min={1}
                max={4}
                step={0.5}
                value={zThreshold}
                aria-label="Complexity z-score threshold"
                style={
                  { "--fill": `${((zThreshold - 1) / 3) * 100}%` } as React.CSSProperties
                }
                disabled={disabled}
                onChange={(event) => onZThreshold(Number(event.target.value))}
              />
              <span className="field__note">
                Silent below 30 baseline samples. This repository has{" "}
                {baselineSamples}.
              </span>
            </div>

            <div className="field">
              <span className="field__label">Minimum displayed severity</span>
              <Segmented
                full
                label="Minimum displayed severity"
                value={minSeverity}
                disabled={disabled}
              onChange={onMinSeverity}
                options={[
                  { value: "info", label: "info" },
                  { value: "warning", label: "warning" },
                  { value: "error", label: "error" },
                ]}
              />
            </div>
          </div>

          <div className="card">
            <span className="micro" style={{ letterSpacing: "0.14em" }}>
              commit gate
            </span>

            {(
              [
                {
                  id: "advisory" as const,
                  label: "Advisory (default)",
                  note: "Prints findings and exits 0. Never blocks a commit.",
                },
                {
                  id: "block" as const,
                  label: "Block on error severity",
                  note: "Exits non-zero on error findings only. --no-verify still bypasses it.",
                },
              ]
            ).map((option) => (
              <button
                key={option.id}
                type="button"
                role="radio"
                className="radio"
                aria-checked={commitGate === option.id}
                disabled={disabled}
                  onClick={() => onCommitGate(option.id)}
              >
                <span className="radio__dot" />
                <span className="radio__body">
                  <span className="radio__label">{option.label}</span>
                  <span className="radio__note">{option.note}</span>
                </span>
              </button>
            ))}
          </div>
        </aside>
      </div>
    </div>
  );
}
