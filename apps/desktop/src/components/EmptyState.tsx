/**
 * The five bodies that stand in for the findings split pane.
 *
 * All share one centred layout. There is no illustration and no icon — the
 * receipt of work done is what keeps the clean state from reading as an empty
 * void, so the facts list carries the weight.
 */

export interface Fact {
  key: string;
  value: string;
  tone?: "warn" | "ok" | "faint";
}

interface Props {
  kicker: string;
  title: string;
  body: string;
  facts: Fact[];
  cta?: { label: string; onClick: () => void; disabled?: boolean };
}

export function EmptyState({ kicker, title, body, facts, cta }: Props) {
  return (
    <div className="state">
      <div className="state__inner">
        <span className="kicker">{kicker}</span>
        <h2 className="state__title">{title}</h2>
        <p className="state__body">{body}</p>

        <dl className="state__facts">
          {facts.map((fact) => (
            <div className="fact" key={fact.key}>
              <dt className="fact__key">{fact.key}</dt>
              <dd
                className={
                  fact.tone ? `fact__value fact__value--${fact.tone}` : "fact__value"
                }
              >
                {fact.value}
              </dd>
            </div>
          ))}
        </dl>

        {cta && (
          <button
            type="button"
            className="btn btn--primary state__cta"
            onClick={cta.onClick}
            disabled={cta.disabled}
          >
            {cta.label}
          </button>
        )}
      </div>
    </div>
  );
}
