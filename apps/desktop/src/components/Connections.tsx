import type { AdapterStatus } from "../types";

interface Props {
  statuses: AdapterStatus[];
  disabled: boolean;
  onInstall: (id: string) => void;
  onUninstall: (id: string) => void;
}

export function Connections({
  statuses,
  disabled,
  onInstall,
  onUninstall,
}: Props) {
  if (statuses.length === 0) {
    return (
      <div className="empty">
        <p className="dim">Open a repository to detect available integrations.</p>
      </div>
    );
  }

  return (
    <div className="connections">
      {statuses.map((status) => (
        <article
          key={status.id}
          className={status.installed ? "conn conn-on" : "conn"}
        >
          <div className="conn-head">
            <div>
              <h3>{status.label}</h3>
              <span
                className={
                  status.installed
                    ? "state state-on"
                    : status.detected
                      ? "state state-detected"
                      : "state state-off"
                }
              >
                {status.installed
                  ? "Connected"
                  : status.detected
                    ? "Detected"
                    : "Not found"}
              </span>
            </div>

            <button
              className={status.installed ? "btn" : "btn btn-primary"}
              disabled={disabled}
              onClick={() =>
                status.installed ? onUninstall(status.id) : onInstall(status.id)
              }
            >
              {status.installed ? "Disconnect" : "Connect"}
            </button>
          </div>

          {status.config_path && (
            <code className="conn-path">{status.config_path}</code>
          )}

          {/* Limitations are shown up front rather than buried in docs — a
              hook that silently does not fire is worse than one you know
              the boundaries of. */}
          {status.limitations.map((limitation, i) => (
            <p key={i} className="conn-note">
              {limitation}
            </p>
          ))}
        </article>
      ))}
    </div>
  );
}
