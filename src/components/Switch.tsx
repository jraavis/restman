//! Small accent-colored toggle switch, used in place of raw checkboxes for
//! boolean settings (e.g. "Follow redirects").

interface Props {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  disabled?: boolean;
}

export function Switch({ checked, onChange, label, disabled }: Props) {
  return (
    <label
      className={
        "inline-flex items-center gap-2 text-sm " +
        (disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer")
      }
    >
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={
          "relative h-5 w-9 shrink-0 rounded-full transition-colors " +
          (checked ? "bg-accent" : "bg-slate-300 dark:bg-slate-700")
        }
      >
        <span
          className={
            "absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform " +
            (checked ? "translate-x-4" : "translate-x-0")
          }
        />
      </button>
      {label && <span className="text-slate-700 dark:text-slate-300">{label}</span>}
    </label>
  );
}
