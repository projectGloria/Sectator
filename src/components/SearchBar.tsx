import { forwardRef } from "react";
import { Loader2, Search, Square, X } from "lucide-react";
import { operatorTokens, operatorValue, removeToken, setOperator } from "../lib/dsl";
import { CATEGORIES } from "../lib/types";

interface Props {
  value: string;
  busy: boolean;
  onChange: (v: string) => void;
  onSubmit: (v: string) => void;
  onCancel: () => void;
}

const OP_COLOR: Record<string, string> = {
  cat: "text-accent",
  res: "text-violet",
  codec: "text-violet",
  src: "text-violet",
  seeds: "text-good",
  size: "text-good",
  site: "text-warn",
  source: "text-warn",
  file: "text-blue",
  by: "text-pink",
};

export const SearchBar = forwardRef<HTMLInputElement, Props>(function SearchBar({ value, busy, onChange, onSubmit, onCancel }, ref) {
  const pills = operatorTokens(value);
  const category = operatorValue(value, "cat");
  const update = (v: string) => {
    onChange(v);
    onSubmit(v);
  };

  return (
    <div className="chrome shrink-0 border-b border-line bg-panel px-3 py-2">
      <div className="flex items-center gap-2">
        <div className="group flex h-9 flex-1 items-center gap-2 rounded-md border border-line bg-bg px-2.5 transition-colors duration-150 focus-within:border-accent/60 hover:border-edge">
          {busy ? <Loader2 size={15} className="shrink-0 animate-spin text-accent" /> : <Search size={15} className="shrink-0 text-faint" />}
          <input
            ref={ref}
            value={value}
            spellCheck={false}
            autoFocus
            placeholder='Search…  e.g. ubuntu 24.04 cat:software  ·  "exact phrase" -cam res:1080p seeds:>10 size:<8gb'
            onChange={(e) => onChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onSubmit(value);
              if (e.key === "Escape") {
                if (busy) onCancel();
                else if (value) update("");
                e.currentTarget.blur();
              }
            }}
            className="num h-full min-w-0 flex-1 select-text bg-transparent text-[13px] outline-none placeholder:font-sans placeholder:text-faint"
            style={{ outline: "none" }}
          />
          {value && (
            <button
              aria-label="Clear search"
              onClick={() => update("")}
              className="rounded p-0.5 text-faint transition-colors duration-150 hover:bg-raised hover:text-fg active:bg-line"
            >
              <X size={14} />
            </button>
          )}
        </div>
        <select
          aria-label="Category"
          value={category ?? ""}
          onChange={(e) => update(setOperator(value, "cat", e.target.value || null))}
          className="h-9 cursor-pointer appearance-none rounded-md border border-line bg-bg px-3 pr-7 text-[12px] text-dim transition-colors duration-150 hover:border-edge hover:text-fg focus-visible:border-accent/60"
          style={{
            backgroundImage:
              "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M1 1l4 4 4-4' stroke='%238b93a1' fill='none' stroke-width='1.5'/%3E%3C/svg%3E\")",
            backgroundRepeat: "no-repeat",
            backgroundPosition: "right 10px center",
          }}
        >
          <option value="">All categories</option>
          {CATEGORIES.map((c) => (
            <option key={c} value={c}>
              {c[0].toUpperCase() + c.slice(1)}
            </option>
          ))}
        </select>
        {busy && (
          <button
            onClick={onCancel}
            aria-label="Stop search"
            title="Stop search (Esc)"
            className="flex h-9 w-9 items-center justify-center rounded-md border border-line text-dim transition-colors duration-150 hover:border-bad/50 hover:text-bad focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent active:bg-raised"
          >
            <Square size={12} fill="currentColor" />
          </button>
        )}
      </div>
      {pills.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {pills.map((p, i) => (
            <span key={p.raw + i} className="num inline-flex h-6 items-center gap-1 rounded border border-line bg-raised pl-2 pr-1 text-[11px]">
              {p.op ? (
                <>
                  <span className={OP_COLOR[p.op] ?? "text-dim"}>{p.op}</span>
                  <span className="text-fg">{p.value}</span>
                </>
              ) : (
                <>
                  <span className="text-bad">not</span>
                  <span className="text-fg">{p.value}</span>
                </>
              )}
              <button
                aria-label={`Remove ${p.raw}`}
                onClick={() => update(removeToken(value, p.raw))}
                className="rounded p-0.5 text-faint transition-colors duration-150 hover:bg-line hover:text-fg"
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
});
