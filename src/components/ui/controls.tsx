import { AlertCircle, X } from "lucide-react";
import type { ReactNode } from "react";

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}

export function Toggle({ checked, onChange, label, disabled }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      className="toggle"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    />
  );
}

interface SegmentedProps<T extends string> {
  value: T;
  /** `title` names an icon-only option for tooltips and screen readers. */
  options: ReadonlyArray<{ value: T; label: ReactNode; title?: string }>;
  onChange: (value: T) => void;
  label: string;
  iconOnly?: boolean;
}

export function Segmented<T extends string>({ value, options, onChange, label, iconOnly }: SegmentedProps<T>) {
  return (
    <div className="segmented" data-icons={iconOnly} role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          aria-label={o.title}
          title={o.title}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Notice({ message, onClose }: { message: string; onClose: () => void }) {
  return (
    <div className="notice" role="alert">
      <AlertCircle className="notice__icon" aria-hidden />
      <p>{message}</p>
      <button type="button" className="icon-btn" aria-label="Cerrar" onClick={onClose}>
        <X />
      </button>
    </div>
  );
}
