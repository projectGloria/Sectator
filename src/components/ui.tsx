// Custom-styled form controls with explicit hover / active / focus-visible / disabled states.
import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

type Tone = "default" | "primary" | "danger" | "ghost";

const TONE: Record<Tone, string> = {
  default: "border border-line bg-raised text-fg hover:border-edge hover:bg-lift active:bg-line",
  primary: "bg-accent font-medium text-on-accent hover:bg-accent-hi active:bg-accent-lo",
  danger: "border border-bad/30 text-bad hover:bg-bad/10 active:bg-bad/20",
  ghost: "text-dim hover:bg-raised hover:text-fg active:bg-line",
};

export function Button({ tone = "default", className = "", ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { tone?: Tone }) {
  return (
    <button
      type="button"
      {...rest}
      className={`chrome inline-flex h-7 items-center justify-center gap-1.5 rounded-md px-2.5 text-[12px] transition-colors duration-150 disabled:pointer-events-none disabled:opacity-40 ${TONE[tone]} ${className}`}
    />
  );
}

export function TextInput({ className = "", mono, ...rest }: InputHTMLAttributes<HTMLInputElement> & { mono?: boolean }) {
  return (
    <input
      spellCheck={false}
      {...rest}
      className={`h-7 min-w-0 select-text rounded-md border border-line bg-bg px-2 text-[12px] outline-none transition-colors duration-150 placeholder:text-faint hover:border-edge focus-visible:border-accent/60 disabled:opacity-40 ${mono ? "num" : ""} ${className}`}
      style={{ outline: "none" }}
    />
  );
}

export function NumberInput({ value, onChange, min, max, className = "" }: { value: number; onChange: (n: number) => void; min?: number; max?: number; className?: string }) {
  return (
    <TextInput
      mono
      inputMode="numeric"
      value={String(value)}
      onChange={(e) => {
        const n = Number(e.target.value.replace(/[^\d]/g, ""));
        if (Number.isFinite(n)) onChange(max != null ? Math.min(max, n) : n);
      }}
      onBlur={() => min != null && value < min && onChange(min)}
      className={`w-24 text-right ${className}`}
    />
  );
}

export function Toggle({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-4 w-7 shrink-0 items-center rounded-full transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:opacity-40 ${checked ? "bg-accent hover:bg-accent-hi" : "bg-line hover:bg-edge"}`}
    >
      <span
        className={`pointer-events-none absolute left-0 top-0.5 h-3 w-3 rounded-full shadow-sm transition-transform duration-150 ${checked ? "translate-x-3.5 bg-on-accent" : "translate-x-0.5 bg-fg"}`}
      />
    </button>
  );
}

/** A segmented control in place of a native <select>. */
export function Segmented<T extends string>({ value, options, onChange }: { value: T; options: [T, string, LucideIcon?][]; onChange: (v: T) => void }) {
  return (
    <div className="chrome inline-flex h-7 rounded-md border border-line p-0.5">
      {options.map(([v, label, Icon]) => (
        <button
          key={v}
          type="button"
          onClick={() => onChange(v)}
          className={`flex items-center gap-1.5 rounded px-2 text-[11.5px] transition-colors duration-150 focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent ${v === value ? "bg-raised text-fg" : "text-dim hover:text-fg active:bg-line"}`}
        >
          {Icon && <Icon size={12} className={v === value ? "text-accent" : ""} />}
          {label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[150px_1fr] items-center gap-3 py-1">
      <span className="chrome text-[12px] text-dim" title={hint}>
        {label}
      </span>
      <div className="flex min-w-0 items-center gap-2">{children}</div>
    </div>
  );
}
