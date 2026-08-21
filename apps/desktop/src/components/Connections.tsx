import type { ConnectionCard } from "../types";

const DOT: Record<ConnectionCard["status"], string> = {
  connected: "dot dot--7 dot--ok",
  detected: "dot dot--7 dot--warn",
  "not found": "dot dot--7",
};

const STATUS_CLASS: Record<ConnectionCard["status"], string> = {
  connected: "conn__status conn__status--connected",
  detected: "conn__status conn__status--detected",
  "not found": "conn__status conn__status--notfound",
};

export function Connections({
  cards,
  disabled,
  onToggle,
}: {
  cards: ConnectionCard[];
  disabled: boolean;
  onToggle: (card: ConnectionCard) => void;
}) {
  return (
    <div className="view">
      <div className="view__head">
        <h2 className="h">Where authorship comes from</h2>
        <p className="sub">
          Dross reads commit trailers and file-write timing that these tools leave
          behind. It never queries them, and it never sends your diff anywhere.
        </p>
      </div>

      {/* No cards means detection has not run — which happens before a
          repository is open. Saying so is the only honest option: this panel
          used to fall back to a seeded list that reported integrations as
          connected when nothing had been checked. */}
      {cards.length === 0 && (
        <p className="sub">
          No integrations have been probed yet. Open a repository and Dross
          checks this machine for each tool's configuration file.
        </p>
      )}

      <div className="conns">
        {cards.map((card) => {
          const connected = card.status === "connected";
          // "Locate config…" used to appear here for an undetected tool, but
          // the button installs — there is no file picker behind it. Both
          // non-connected states run the same install, so both say so.
          const action = connected ? "Disconnect" : "Connect";

          return (
            <article className="conn" key={card.name}>
              <div className="conn__body">
                <div className="conn__top">
                  <span className={DOT[card.status]} />
                  <span className="conn__name">{card.name}</span>
                  <span className={STATUS_CLASS[card.status]}>{card.status}</span>
                </div>

                <code className="conn__path">{card.path}</code>
                <span className="conn__signal">{card.signal}</span>

                <button
                  type="button"
                  className={
                    connected
                      ? "btn btn--card conn__action"
                      : "btn btn--card btn--accent conn__action"
                  }
                  disabled={disabled}
                  onClick={() => onToggle(card)}
                >
                  {action}
                </button>
              </div>

              {/* Every card prints its own limitations. This is a designed
                  feature, not fine print — it does not move into a tooltip. */}
              <div className="conn__foot">
                <span className="micro" style={{ letterSpacing: "0.13em" }}>
                  known limitation
                </span>
                <p className="conn__limit">{card.limitation}</p>
              </div>
            </article>
          );
        })}
      </div>
    </div>
  );
}
