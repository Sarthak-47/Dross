/** Small shared controls. Everything applies immediately — there is no Save. */

interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  full?: boolean;
  disabled?: boolean;
  label: string;
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  full,
  disabled,
  label,
}: SegmentedProps<T>) {
  return (
    <div className={full ? "seg seg--full" : "seg"} role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className="seg__item"
          aria-pressed={value === option.value}
          disabled={disabled}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (next: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      className="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={(event) => {
        // A toggle inside a clickable row must not also expand it.
        event.stopPropagation();
        onChange(!checked);
      }}
    >
      <span className="switch__knob" />
    </button>
  );
}

/** A location that reads as clickable and opens the user's editor. */
export function Loc({
  value,
  onOpen,
}: {
  value: string;
  onOpen?: (value: string) => void;
}) {
  return (
    <button
      type="button"
      className="loc"
      onClick={(event) => {
        event.stopPropagation();
        onOpen?.(value);
      }}
    >
      {value}
    </button>
  );
}
